//! `CREATE TABLE` parsing and shared column helpers.

use crate::context::{LineOffsets, ParseContext, ParsedColumn, span_from_spanned};
use crate::mysql_enum::canonicalize_mysql_enum_like_type;
use crate::names::{build_foreign_key, normalized_stable_id, split_object_name_with_diagnostics};
use crate::query_columns::columns_from_query;
use relune_core::{
    ColumnId, Diagnostic, Index, SourceSpan, Table, diagnostic::codes, normalize_identifier,
};
use sqlparser::ast::{ColumnOption, DataType, IndexColumn, TableConstraint};

/// Parse a CREATE TABLE statement into a Table.
#[allow(clippy::too_many_lines)]
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn parse_create_table(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    create: &sqlparser::ast::CreateTable,
) -> Option<Table> {
    let (schema_name, name) =
        split_object_name_with_diagnostics(ctx, input, offsets, &create.name, "CREATE TABLE");
    let stable_id = normalized_stable_id(schema_name.as_deref(), &name);

    let table_id = ctx.next_table_id();

    // Parse columns. `CREATE TABLE ... AS SELECT` carries no explicit column
    // definitions, so derive the projection columns from the query when one is
    // present and warn if none can be recovered (e.g. `SELECT *`) instead of
    // silently producing a column-less table.
    let mut columns = Vec::new();
    if create.columns.is_empty() {
        if let Some(query) = create.query.as_deref() {
            columns = columns_from_query(query);
            if columns.is_empty() {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!(
                            "CREATE TABLE AS SELECT: could not derive columns for `{stable_id}` from the query projection (e.g. `SELECT *`); the table is recorded with no columns"
                        ),
                    )
                    .with_span_opt(span_from_spanned(input, offsets, &create.name)),
                );
            }
        }
    } else {
        for (next_column_id, column) in (1_u64..).zip(create.columns.iter()) {
            let parsed_column = parsed_column_from_column_def(column);
            columns.push(parsed_column.into_column(ColumnId(next_column_id)));
        }
    }

    // Parse inline foreign key constraints from columns and capture any
    // column-level named PRIMARY KEY or UNIQUE constraint.
    let mut foreign_keys = Vec::new();
    let mut primary_key_name: Option<String> = None;
    let mut indexes: Vec<Index> = Vec::new();
    for column in &create.columns {
        for option in &column.options {
            match &option.option {
                ColumnOption::ForeignKey(constraint) => {
                    let from_column = normalize_identifier(&column.name.value);
                    foreign_keys.push(build_foreign_key(
                        ctx,
                        input,
                        offsets,
                        option.name.as_ref(),
                        vec![from_column],
                        &constraint.foreign_table,
                        &constraint.referred_columns,
                        constraint.on_delete,
                        constraint.on_update,
                        "CREATE TABLE inline FOREIGN KEY",
                    ));
                }
                ColumnOption::PrimaryKey(_) => {
                    if let Some(constraint_name) = &option.name {
                        primary_key_name = Some(normalize_identifier(&constraint_name.value));
                    }
                }
                ColumnOption::Unique(_) => {
                    let col_name = normalize_identifier(&column.name.value);
                    let constraint_name =
                        option.name.as_ref().map(|n| normalize_identifier(&n.value));
                    push_unique_index(&mut indexes, constraint_name, vec![col_name]);
                }
                _ => {}
            }
        }
    }

    // Parse table-level constraints
    for constraint in &create.constraints {
        match constraint {
            TableConstraint::PrimaryKey(primary_key) => {
                if let Some(pk_cols) = plain_column_names(&primary_key.columns) {
                    for col_name in &pk_cols {
                        if let Some(column) = columns.iter_mut().find(|c| &c.name == col_name) {
                            column.is_primary_key = true;
                            column.nullable = false;
                        }
                    }
                    if let Some(constraint_name) = &primary_key.name {
                        primary_key_name = Some(normalize_identifier(&constraint_name.value));
                    }
                } else {
                    warn_expression_key(
                        ctx,
                        span_from_spanned(input, offsets, constraint),
                        &stable_id,
                        "PRIMARY KEY",
                    );
                }
            }
            TableConstraint::Unique(unique) => {
                if let Some(col_names) = plain_column_names(&unique.columns) {
                    let constraint_name =
                        unique.name.as_ref().map(|n| normalize_identifier(&n.value));
                    push_unique_index(&mut indexes, constraint_name, col_names);
                } else {
                    warn_expression_key(
                        ctx,
                        span_from_spanned(input, offsets, constraint),
                        &stable_id,
                        "UNIQUE constraint",
                    );
                }
            }
            TableConstraint::ForeignKey(foreign_key) => {
                let from_cols: Vec<String> = foreign_key
                    .columns
                    .iter()
                    .map(|c| normalize_identifier(&c.value))
                    .collect();
                foreign_keys.push(build_foreign_key(
                    ctx,
                    input,
                    offsets,
                    foreign_key.name.as_ref(),
                    from_cols,
                    &foreign_key.foreign_table,
                    &foreign_key.referred_columns,
                    foreign_key.on_delete,
                    foreign_key.on_update,
                    "CREATE TABLE FOREIGN KEY",
                ));
            }
            TableConstraint::Check(_) | TableConstraint::Index(_) => {
                // Check constraints and Index constraints are informational only
            }
            TableConstraint::FulltextOrSpatial(_) => {
                ctx.warn_unsupported(
                    "FULLTEXT/SPATIAL constraint",
                    span_from_spanned(input, offsets, constraint),
                );
            }
            TableConstraint::PrimaryKeyUsingIndex(_) | TableConstraint::UniqueUsingIndex(_) => {
                ctx.warn_unsupported(
                    "PRIMARY KEY/UNIQUE USING INDEX constraint",
                    span_from_spanned(input, offsets, constraint),
                );
            }
        }
    }

    // Normalize schema and table names
    let normalized_schema = schema_name.map(|s| normalize_identifier(&s));
    let normalized_name = normalize_identifier(&name);

    Some(Table {
        id: table_id,
        stable_id,
        schema_name: normalized_schema,
        name: normalized_name,
        columns,
        foreign_keys,
        indexes, // Inline UNIQUE indexes; CREATE INDEX statements are merged in a second pass.
        primary_key_name,
        comment: None, // Comments are added in third pass
    })
}

/// Append a UNIQUE index entry to `indexes`, deduplicating by name and column set.
pub(crate) fn push_unique_index(
    indexes: &mut Vec<Index>,
    name: Option<String>,
    columns: Vec<String>,
) {
    if columns.is_empty() {
        return;
    }
    let lower_cols: Vec<String> = columns.iter().map(|c| c.to_ascii_lowercase()).collect();
    let already_present = indexes.iter().any(|existing| {
        if !existing.is_unique {
            return false;
        }
        if let (Some(a), Some(b)) = (&existing.name, &name)
            && a.eq_ignore_ascii_case(b)
        {
            return true;
        }
        let existing_lower: Vec<String> = existing
            .columns
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect();
        existing_lower == lower_cols
    });
    if already_present {
        return;
    }
    indexes.push(Index {
        name,
        columns,
        is_unique: true,
    });
}

/// Extract the referenced column name from an `IndexColumn`, if it is a plain
/// column reference.
///
/// Functional / expression index columns (e.g. `lower(email)`) reference no
/// real column, so they return `None` rather than a synthetic name that would
/// never match a modeled column.
fn extract_column_name(index_col: &IndexColumn) -> Option<String> {
    use sqlparser::ast::Expr;

    match &index_col.column.expr {
        Expr::Identifier(ident) => Some(normalize_identifier(&ident.value)),
        // Take the trailing identifier of a qualified reference (e.g. `t.col`).
        Expr::CompoundIdentifier(parts) => {
            parts.last().map(|ident| normalize_identifier(&ident.value))
        }
        _ => None,
    }
}

/// Collect the plain column names of an index or key column list, returning
/// `None` if any element is a functional/expression column.
///
/// Functional indexes and keys cannot be modeled faithfully, and keeping only
/// the plain columns would assert false uniqueness (e.g. `UNIQUE (a, lower(b))`
/// → `UNIQUE (a)`) or false leading-column index coverage. Callers therefore
/// drop the whole index/constraint when this returns `None`.
pub(crate) fn plain_column_names(columns: &[IndexColumn]) -> Option<Vec<String>> {
    columns.iter().map(extract_column_name).collect()
}

/// Warn that an index or key is dropped because it contains a
/// functional/expression column the model cannot represent.
pub(crate) fn warn_expression_key(
    ctx: &mut ParseContext,
    span: Option<SourceSpan>,
    stable_id: &str,
    kind: &str,
) {
    ctx.diagnostics.push(
        Diagnostic::warning(
            codes::parse_unsupported(),
            format!(
                "{kind} on `{stable_id}`: ignoring functional/expression column(s); not modeled (a partial column list would assert false uniqueness or index coverage)"
            ),
        )
        .with_span_opt(span),
    );
}

/// Column attributes derived from a list of column options (nullability,
/// primary-key membership, comment). Shared by `CREATE TABLE` column parsing
/// and `ALTER TABLE MODIFY/CHANGE COLUMN`, which redefine a column in full.
pub(crate) struct ColumnAttributes {
    pub(crate) nullable: bool,
    pub(crate) is_primary_key: bool,
    pub(crate) comment: Option<String>,
}

/// Interpret column options into the subset of attributes tracked by the model.
pub(crate) fn column_attributes_from_options<'a>(
    options: impl IntoIterator<Item = &'a ColumnOption>,
) -> ColumnAttributes {
    let mut nullable = true;
    let mut is_primary_key = false;
    let mut comment: Option<String> = None;

    for option in options {
        match option {
            ColumnOption::NotNull => nullable = false,
            ColumnOption::Null => nullable = true,
            ColumnOption::PrimaryKey(_) => {
                is_primary_key = true;
                nullable = false;
            }
            ColumnOption::Comment(text) => {
                comment = Some(text.clone());
            }
            ColumnOption::Unique(_)
            | ColumnOption::Default(_)
            | ColumnOption::Check(_)
            | ColumnOption::DialectSpecific(_)
            | ColumnOption::CharacterSet(_)
            | ColumnOption::Collation(_)
            | ColumnOption::OnUpdate(_)
            | ColumnOption::Generated { .. }
            | ColumnOption::ForeignKey(_)
            | ColumnOption::Materialized(_)
            | ColumnOption::Ephemeral(_)
            | ColumnOption::Alias(_)
            | ColumnOption::Options(_)
            | ColumnOption::Identity(_)
            | ColumnOption::OnConflict(_)
            | ColumnOption::Policy(_)
            | ColumnOption::Tags(_)
            | ColumnOption::Srid(_)
            | ColumnOption::Invisible => {}
        }
    }

    ColumnAttributes {
        nullable,
        is_primary_key,
        comment,
    }
}

/// Render a `DataType` into the model's data-type string, canonicalizing
/// `MySQL` inline `ENUM(...)`/`SET(...)` so enum values can be recovered later.
pub(crate) fn canonicalize_data_type(data_type: &DataType) -> String {
    let raw_data_type = data_type.to_string();
    canonicalize_mysql_enum_like_type(&raw_data_type)
        .ok()
        .flatten()
        .unwrap_or(raw_data_type)
}

pub(crate) fn parsed_column_from_column_def(column: &sqlparser::ast::ColumnDef) -> ParsedColumn {
    let attrs = column_attributes_from_options(column.options.iter().map(|option| &option.option));
    ParsedColumn {
        name: normalize_identifier(&column.name.value),
        data_type: canonicalize_data_type(&column.data_type),
        nullable: attrs.nullable,
        is_primary_key: attrs.is_primary_key,
        comment: attrs.comment,
    }
}
