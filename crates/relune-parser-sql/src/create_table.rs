//! `CREATE TABLE` parsing and shared column helpers.

use crate::context::{LineOffsets, ParseContext, ParsedColumn, span_from_spanned};
use crate::mysql_enum::canonicalize_mysql_enum_like_type;
use crate::names::{build_foreign_key, normalized_stable_id, split_object_name_with_diagnostics};
use crate::query_columns::columns_from_query;
use relune_core::{
    CheckConstraint, ColumnId, ColumnSemantics, Diagnostic, GeneratedColumn, IdentitySpec, Index,
    IndexKey, SourceSpan, SqlDialect, Table, diagnostic::codes, normalize_identifier,
};
use sqlparser::ast::{
    ColumnOption, DataType, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, GeneratedAs,
    GeneratedExpressionMode, Ident, IndexColumn, IndexConstraint, IndexOption, OrderBySort,
    TableConstraint, Value,
};
use sqlparser::tokenizer::Token;

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
    let mut table_check_constraints: Vec<CheckConstraint> = Vec::new();
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
                    push_unique_index(
                        &mut indexes,
                        constraint_name,
                        vec![IndexKey::column(col_name)],
                    );
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
                if let Some(key_parts) = unique_key_parts(&unique.columns, ctx.dialect) {
                    let constraint_name =
                        unique.name.as_ref().map(|n| normalize_identifier(&n.value));
                    push_unique_index(&mut indexes, constraint_name, key_parts);
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
            TableConstraint::Check(check) => {
                table_check_constraints.push(CheckConstraint {
                    name: check.name.as_ref().map(|n| normalize_identifier(&n.value)),
                    expression: check.expr.to_string(),
                });
            }
            TableConstraint::Index(index) => {
                if let Some(index) = index_from_constraint(index, ctx.dialect) {
                    indexes.push(index);
                }
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
            TableConstraint::Exclude(_) => {
                ctx.warn_unsupported(
                    "EXCLUDE constraint",
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
        indexes, // Inline UNIQUE/KEY indexes; CREATE INDEX statements are merged in a second pass.
        primary_key_name,
        comment: None, // Comments are added in third pass
        check_constraints: table_check_constraints,
    })
}

/// Append a UNIQUE index entry to `indexes`, deduplicating by name and key parts.
pub(crate) fn push_unique_index(
    indexes: &mut Vec<Index>,
    name: Option<String>,
    key_parts: Vec<IndexKey>,
) {
    if key_parts.is_empty() {
        return;
    }
    let signature = key_part_signature(&key_parts);
    let already_present = indexes.iter().any(|existing| {
        if !existing.is_unique {
            return false;
        }
        if let (Some(a), Some(b)) = (&existing.name, &name)
            && a.eq_ignore_ascii_case(b)
        {
            return true;
        }
        key_part_signature(&existing.key_parts) == signature
    });
    if already_present {
        return;
    }
    indexes.push(Index {
        name,
        key_parts,
        is_unique: true,
        predicate: None,
        included_columns: Vec::new(),
        method: None,
    });
}

/// Case-insensitive identity of an index key list (column name or expression
/// text plus prefix length), used to deduplicate equivalent UNIQUE entries.
fn key_part_signature(key_parts: &[IndexKey]) -> Vec<(String, Option<u32>)> {
    key_parts
        .iter()
        .map(|part| match part {
            IndexKey::Column(column) => (column.name.to_ascii_lowercase(), column.prefix_length),
            IndexKey::Expression(expr) => (expr.to_ascii_lowercase(), None),
        })
        .collect()
}

/// Build a non-unique index from a `MySQL` inline `KEY`/`INDEX` definition
/// (`CREATE TABLE ... KEY idx (col)` or `ALTER TABLE ... ADD INDEX idx (col)`).
///
/// Expression key parts are kept as explicit expression parts, matching
/// `CREATE INDEX`, so the index still counts toward FK coverage decisions.
pub(crate) fn index_from_constraint(
    constraint: &IndexConstraint,
    dialect: SqlDialect,
) -> Option<Index> {
    let key_parts = index_key_parts(&constraint.columns, dialect);
    if key_parts.is_empty() {
        return None;
    }
    // `USING` may appear before the column list (`index_type`) or after it
    // (as an index option); either spelling names the access method.
    let method = constraint
        .index_type
        .as_ref()
        .or_else(|| {
            constraint
                .index_options
                .iter()
                .find_map(|option| match option {
                    IndexOption::Using(index_type) => Some(index_type),
                    IndexOption::Comment(_) => None,
                })
        })
        .map(|index_type| index_type.to_string().to_lowercase());
    Some(Index {
        name: constraint
            .name
            .as_ref()
            .map(|ident| normalize_identifier(&ident.value)),
        key_parts,
        is_unique: false,
        predicate: None,
        included_columns: Vec::new(),
        method,
    })
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

/// Key parts of a UNIQUE constraint, or `None` if any part is a
/// functional/expression column (see [`plain_column_names`] for why such
/// constraints are dropped). `MySQL` prefix parts (`col(10)`) are kept with
/// their prefix length, which marks them as not guaranteeing whole-column
/// uniqueness.
pub(crate) fn unique_key_parts(
    columns: &[IndexColumn],
    dialect: SqlDialect,
) -> Option<Vec<IndexKey>> {
    let key_parts = index_key_parts(columns, dialect);
    if key_parts
        .iter()
        .any(|part| matches!(part, IndexKey::Expression(_)))
    {
        return None;
    }
    Some(key_parts)
}

/// Build structured [`relune_core::IndexKey`] parts from a parsed index column
/// list, preserving sort/nulls ordering and `MySQL` prefix lengths for plain
/// columns and recording functional/expression parts as
/// [`relune_core::IndexKey::Expression`].
pub(crate) fn index_key_parts(columns: &[IndexColumn], dialect: SqlDialect) -> Vec<IndexKey> {
    use relune_core::{IndexColumn as ModelIndexColumn, NullsOrder, SortOrder};

    columns
        .iter()
        .map(|index_col| {
            let order_by = &index_col.column;
            let (name, prefix_length) = match &order_by.expr {
                Expr::Identifier(ident) => (Some(normalize_identifier(&ident.value)), None),
                Expr::CompoundIdentifier(parts) => (
                    parts.last().map(|ident| normalize_identifier(&ident.value)),
                    None,
                ),
                expr if dialect == SqlDialect::Mysql => match mysql_prefix_column(expr) {
                    Some((name, length)) => (Some(name), Some(length)),
                    None => (None, None),
                },
                _ => (None, None),
            };
            match name {
                Some(name) => IndexKey::Column(ModelIndexColumn {
                    name,
                    order: match order_by.options.sort {
                        Some(OrderBySort::Asc) => Some(SortOrder::Asc),
                        Some(OrderBySort::Desc) => Some(SortOrder::Desc),
                        Some(OrderBySort::Using(_)) | None => None,
                    },
                    nulls: order_by.options.nulls_first.map(|first| {
                        if first {
                            NullsOrder::First
                        } else {
                            NullsOrder::Last
                        }
                    }),
                    prefix_length,
                }),
                None => IndexKey::Expression(order_by.expr.to_string()),
            }
        })
        .collect()
}

/// Recognize a `MySQL` prefix key part such as `name(10)`.
///
/// sqlparser has no dedicated node for it and parses it as a one-argument
/// function call. `MySQL` requires functional key parts to be wrapped in their
/// own parentheses (`((lower(name)))`), so an unwrapped call with a single
/// integer literal argument is always a column prefix.
fn mysql_prefix_column(expr: &Expr) -> Option<(String, u32)> {
    let Expr::Function(function) = expr else {
        return None;
    };
    if function.filter.is_some()
        || function.over.is_some()
        || function.null_treatment.is_some()
        || !function.within_group.is_empty()
        || !matches!(function.parameters, FunctionArguments::None)
    {
        return None;
    }
    let [name] = function.name.0.as_slice() else {
        return None;
    };
    let name = name.as_ident()?;
    let FunctionArguments::List(list) = &function.args else {
        return None;
    };
    if list.duplicate_treatment.is_some() || !list.clauses.is_empty() {
        return None;
    }
    let [FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Value(value)))] = list.args.as_slice()
    else {
        return None;
    };
    let Value::Number(length, _) = &value.value else {
        return None;
    };
    let length = length.parse::<u32>().ok().filter(|length| *length > 0)?;
    Some((normalize_identifier(&name.value), length))
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
    pub(crate) semantics: ColumnSemantics,
}

/// Interpret column options into the subset of attributes tracked by the model,
/// including the extended semantics (`DEFAULT`, `CHECK`, generated, identity,
/// collation, character set, auto-increment, `ON UPDATE`) needed so `diff` does
/// not silently miss changes to them.
/// Each item pairs a column option with the optional constraint name from its
/// enclosing `ColumnOptionDef` (e.g. `CONSTRAINT x_positive CHECK (...)`).
/// Column-level `CHECK` constraints carry their name on the outer definition
/// rather than the inner option, so callers that have it (`CREATE TABLE`) must
/// thread it through; callers that lack it (`MODIFY`/`CHANGE COLUMN`) pass
/// `None`.
pub(crate) fn column_attributes_from_options<'a>(
    options: impl IntoIterator<Item = (Option<&'a Ident>, &'a ColumnOption)>,
) -> ColumnAttributes {
    let mut nullable = true;
    let mut is_primary_key = false;
    let mut comment: Option<String> = None;
    let mut semantics = ColumnSemantics::default();

    for (constraint_name, option) in options {
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
            ColumnOption::Default(expr) => {
                semantics.default_expression = Some(expr.to_string());
            }
            ColumnOption::Check(check) => {
                // A column-level check records its name on the enclosing
                // `ColumnOptionDef` (`CONSTRAINT <name> CHECK (...)`); the inner
                // option's own name is always `None`, so fall back to it.
                let name = check
                    .name
                    .as_ref()
                    .or(constraint_name)
                    .map(|n| normalize_identifier(&n.value));
                semantics.check_constraints.push(CheckConstraint {
                    name,
                    expression: check.expr.to_string(),
                });
            }
            ColumnOption::CharacterSet(name) => {
                semantics.character_set = Some(name.to_string());
            }
            ColumnOption::Collation(name) => {
                semantics.collation = Some(name.to_string());
            }
            ColumnOption::OnUpdate(expr) => {
                semantics.on_update = Some(expr.to_string());
            }
            ColumnOption::Generated {
                generated_as,
                generation_expr,
                generation_expr_mode,
                ..
            } => {
                if let Some(expr) = generation_expr {
                    semantics.generated = Some(GeneratedColumn {
                        expression: expr.to_string(),
                        stored: matches!(
                            (generated_as, generation_expr_mode),
                            (GeneratedAs::ExpStored, _)
                                | (_, Some(GeneratedExpressionMode::Stored))
                        ),
                    });
                } else {
                    // `GENERATED { ALWAYS | BY DEFAULT } AS IDENTITY` carries no
                    // expression; record it as an identity column instead.
                    semantics.identity = Some(IdentitySpec {
                        always: matches!(generated_as, GeneratedAs::Always),
                    });
                }
            }
            ColumnOption::Identity(_) => {
                semantics.identity = Some(IdentitySpec { always: true });
                semantics.auto_increment = true;
            }
            ColumnOption::DialectSpecific(tokens) => {
                if tokens_contain_auto_increment(tokens) {
                    semantics.auto_increment = true;
                }
            }
            ColumnOption::Unique(_)
            | ColumnOption::ForeignKey(_)
            | ColumnOption::Materialized(_)
            | ColumnOption::Ephemeral(_)
            | ColumnOption::Alias(_)
            | ColumnOption::Options(_)
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
        semantics,
    }
}

/// Returns `true` when a dialect-specific token stream declares an
/// auto-increment column (`AUTO_INCREMENT` / `AUTOINCREMENT`).
fn tokens_contain_auto_increment(tokens: &[Token]) -> bool {
    tokens.iter().any(|token| match token {
        Token::Word(word) => {
            word.value.eq_ignore_ascii_case("AUTO_INCREMENT")
                || word.value.eq_ignore_ascii_case("AUTOINCREMENT")
        }
        _ => false,
    })
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
    let attrs = column_attributes_from_options(
        column
            .options
            .iter()
            .map(|option| (option.name.as_ref(), &option.option)),
    );
    ParsedColumn {
        name: normalize_identifier(&column.name.value),
        data_type: canonicalize_data_type(&column.data_type),
        nullable: attrs.nullable,
        is_primary_key: attrs.is_primary_key,
        comment: attrs.comment,
        semantics: attrs.semantics,
    }
}
