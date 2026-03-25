//! SQL Parser for relune - parses SQL DDL statements into Schema objects.
//!
//! This crate provides multi-dialect SQL parsing with support for:
//! - CREATE TABLE statements with columns, constraints, and foreign keys
//! - CREATE INDEX statements
//! - ALTER TABLE (statement-order application: `ADD`/`DROP` column, `ADD`/`DROP` constraint,
//!   `RENAME` column/table, `DROP PRIMARY KEY`, MySQL-style `DROP FOREIGN KEY` / `DROP INDEX`)
//! - Schema-qualified table names
//! - Diagnostic collection for unsupported constructs
//!
//! Supported dialects: `PostgreSQL`, `MySQL`, `SQLite` (with auto-detection).

use relune_core::{
    Column, ColumnId, Diagnostic, Enum, ForeignKey, Index, ReferentialAction, Schema, Severity,
    SourceSpan, SqlDialect, Table, TableId, View, normalize_identifier,
};
use sqlparser::ast::{
    AlterTableOperation, ColumnOption, CreateIndex, ObjectName, ObjectNamePart, Statement,
    TableConstraint, UserDefinedTypeRepresentation,
};
use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

// Re-export diagnostic codes for convenience
pub use relune_core::diagnostic::codes;

/// Error type for parse failures.
#[derive(Debug, Error)]
pub enum ParseError {
    /// SQL parsing error from sqlparser.
    #[error("SQL parse error: {0}")]
    Sql(#[from] sqlparser::parser::ParserError),

    /// Fatal error during schema construction.
    #[error("Schema error: {0}")]
    Schema(String),
}

/// Output from parsing SQL with diagnostics support.
#[derive(Debug, Clone)]
pub struct ParseOutput {
    /// The resolved SQL dialect used for parsing.
    pub dialect: SqlDialect,
    /// The parsed schema, if parsing succeeded (may be partial).
    pub schema: Option<Schema>,
    /// Diagnostics collected during parsing.
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseOutput {
    /// Returns true if there are any error-level diagnostics.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Returns true if there are any warning-level diagnostics.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning)
    }
}

/// Context for tracking parsing state and generating IDs.
struct ParseContext {
    /// Next table ID to assign.
    next_table_id: u64,
    /// Diagnostics collected during parsing.
    diagnostics: Vec<Diagnostic>,
    /// Set of seen table `stable_ids` for duplicate detection.
    seen_tables: HashSet<String>,
    /// The resolved SQL dialect being used.
    dialect: SqlDialect,
}

impl ParseContext {
    fn new() -> Self {
        Self {
            next_table_id: 1,
            diagnostics: Vec::new(),
            seen_tables: HashSet::new(),
            dialect: SqlDialect::Postgres,
        }
    }

    const fn next_table_id(&mut self) -> TableId {
        let id = TableId(self.next_table_id);
        self.next_table_id += 1;
        id
    }

    fn warn_unsupported(&mut self, construct: &str, span: Option<SourceSpan>) {
        self.diagnostics.push(
            Diagnostic::warning(
                codes::parse_unsupported(),
                format!("Unsupported SQL construct: {construct}. This statement will be skipped."),
            )
            .with_span_opt(span),
        );
    }

    fn info_skipped(&mut self, construct: &str) {
        self.diagnostics.push(Diagnostic::info(
            codes::parse_skipped(),
            format!("Skipped DML statement: {construct}. Only DDL statements are processed."),
        ));
    }

    fn warn_duplicate_table(&mut self, table_name: &str, span: Option<SourceSpan>) {
        self.diagnostics.push(
            Diagnostic::warning(
                codes::schema_duplicate_table(),
                format!(
                    "Duplicate table definition: {table_name}. The first definition will be used."
                ),
            )
            .with_span_opt(span),
        );
    }

    fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Extension trait to add optional span support.
trait WithSpanOpt: Sized {
    fn with_span_opt(self, span: Option<SourceSpan>) -> Self;
}

impl WithSpanOpt for Diagnostic {
    fn with_span_opt(mut self, span: Option<SourceSpan>) -> Self {
        if let Some(s) = span {
            self.span = Some(s);
        }
        self
    }
}

/// Detect the SQL dialect from the content of the SQL string.
///
/// Uses heuristics to identify `MySQL` and `SQLite`-specific constructs.
/// Falls back to `PostgreSQL` if no dialect-specific markers are found.
#[must_use]
pub fn detect_dialect(input: &str) -> SqlDialect {
    let upper = input.to_uppercase();

    let mysql_score = score_dialect_signals(&[
        (upper.contains("ENGINE=") || upper.contains("ENGINE ="), 4),
        (upper.contains("AUTO_INCREMENT"), 4),
        (upper.contains("UNSIGNED"), 3),
        (
            upper.contains("DEFAULT CHARSET") || upper.contains("CHARACTER SET"),
            3,
        ),
        (upper.contains("COLLATE=") || upper.contains("COLLATE "), 2),
        (upper.contains("FULLTEXT"), 2),
        (upper.contains("ON UPDATE CURRENT_TIMESTAMP"), 3),
        (input.contains('`'), 2),
    ]);

    let sqlite_score = score_dialect_signals(&[
        (upper.contains("AUTOINCREMENT"), 4),
        (upper.contains("WITHOUT ROWID"), 4),
        (upper.contains("PRAGMA"), 4),
        (
            upper.contains("INTEGER PRIMARY KEY") && !upper.contains("AUTO_INCREMENT"),
            3,
        ),
        (upper.contains("STRICT"), 2),
    ]);

    let pg_score = score_dialect_signals(&[
        (
            upper.contains("CREATE TYPE") && upper.contains("AS ENUM"),
            4,
        ),
        (upper.contains("SERIAL") || upper.contains("BIGSERIAL"), 3),
        (upper.contains("COMMENT ON"), 4),
        (upper.contains("CREATE EXTENSION"), 4),
        (upper.contains("CREATE SEQUENCE"), 4),
        (upper.contains("::"), 3),
        (upper.contains("RETURNING"), 2),
        (upper.contains("ILIKE"), 2),
    ]);

    if mysql_score > sqlite_score && mysql_score > pg_score {
        SqlDialect::Mysql
    } else if sqlite_score > mysql_score && sqlite_score > pg_score {
        SqlDialect::Sqlite
    } else {
        SqlDialect::Postgres
    }
}

fn score_dialect_signals(signals: &[(bool, u8)]) -> u32 {
    signals
        .iter()
        .filter_map(|(matched, weight)| matched.then_some(u32::from(*weight)))
        .sum()
}

/// Resolve `SqlDialect::Auto` to a concrete dialect by detecting from SQL content.
fn resolve_dialect(dialect: SqlDialect, input: &str) -> SqlDialect {
    match dialect {
        SqlDialect::Auto => detect_dialect(input),
        other => other,
    }
}

/// Get the sqlparser `Dialect` implementation for a given `SqlDialect`.
fn dialect_impl(dialect: SqlDialect) -> Box<dyn Dialect> {
    match dialect {
        SqlDialect::Postgres | SqlDialect::Auto => Box::new(PostgreSqlDialect {}),
        SqlDialect::Mysql => Box::new(MySqlDialect {}),
        SqlDialect::Sqlite => Box::new(SQLiteDialect {}),
    }
}

/// Parse SQL into a Schema, returning an error on fatal parse failures.
///
/// This is a convenience function that rejects error-level diagnostics.
///
/// Use the `_with_diagnostics` variant if you need to collect warnings and info messages
/// while still receiving a partial schema.
pub fn parse_sql_to_schema(input: &str) -> Result<Schema, ParseError> {
    parse_sql_to_schema_with_dialect(input, SqlDialect::Auto)
}

/// Parse SQL into a Schema with explicit dialect, returning an error on fatal parse failures.
pub fn parse_sql_to_schema_with_dialect(
    input: &str,
    dialect: SqlDialect,
) -> Result<Schema, ParseError> {
    let output = parse_sql_to_schema_with_diagnostics_and_dialect(input, dialect);
    if output.has_errors() {
        return Err(ParseError::Schema(error_summary(&output)));
    }

    output
        .schema
        .ok_or_else(|| ParseError::Schema("Failed to parse any valid schema elements".to_string()))
}

/// Parse SQL into a Schema with full diagnostics support (auto-detect dialect).
#[must_use]
pub fn parse_sql_to_schema_with_diagnostics(input: &str) -> ParseOutput {
    parse_sql_to_schema_with_diagnostics_and_dialect(input, SqlDialect::Auto)
}

/// Parse SQL into a Schema with full diagnostics support and explicit dialect.
///
/// This function parses all supported SQL statements and collects
/// diagnostics for any issues encountered (unsupported constructs,
/// duplicates, etc.).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn parse_sql_to_schema_with_diagnostics_and_dialect(
    input: &str,
    dialect: SqlDialect,
) -> ParseOutput {
    let resolved_dialect = resolve_dialect(dialect, input);
    let mut ctx = ParseContext::new();
    ctx.dialect = resolved_dialect;

    // Parse SQL statements
    let statements = match Parser::parse_sql(dialect_impl(resolved_dialect).as_ref(), input) {
        Ok(stmts) => stmts,
        Err(e) => {
            ctx.diagnostics.push(
                Diagnostic::error(codes::parse_error(), format!("SQL parse error: {e}"))
                    .with_span(SourceSpan::new(0, input.len().min(100))),
            );
            return ParseOutput {
                dialect: resolved_dialect,
                schema: None,
                diagnostics: ctx.diagnostics,
            };
        }
    };

    // Build schema in source order so ALTER TABLE is visible to later CREATE INDEX / COMMENT.
    let mut tables = Vec::new();
    let mut enums = Vec::new();
    let mut views = Vec::new();
    let mut table_map: HashMap<String, usize> = HashMap::new();

    for statement in &statements {
        match statement {
            Statement::CreateTable(create) => {
                if let Some(table) = parse_create_table(&mut ctx, create) {
                    let stable_id = table.stable_id.clone();
                    if ctx.seen_tables.contains(&stable_id) {
                        ctx.warn_duplicate_table(&stable_id, None);
                    } else {
                        ctx.seen_tables.insert(stable_id.clone());
                        let idx = tables.len();
                        tables.push(table);
                        table_map.insert(stable_id, idx);
                    }
                }
            }
            Statement::CreateType {
                name,
                representation,
            } => {
                if let Some(UserDefinedTypeRepresentation::Enum { labels }) = representation {
                    let enum_def = parse_create_type_enum(name, labels);
                    enums.push(enum_def);
                } else {
                    ctx.warn_unsupported("CREATE TYPE (non-enum)", None);
                }
            }
            Statement::CreateIndex(create_index) => {
                parse_create_index(&mut ctx, create_index, &mut tables, &table_map);
            }
            Statement::Comment {
                object_type,
                object_name,
                comment,
                ..
            } => {
                parse_comment(
                    &mut ctx,
                    *object_type,
                    object_name,
                    comment.as_ref(),
                    &mut tables,
                    &table_map,
                );
            }
            Statement::CreateView(create_view) => {
                if let Some(view) =
                    parse_create_view(&create_view.name, &create_view.columns, &create_view.query)
                {
                    views.push(view);
                }
            }
            Statement::AlterTable(alter_table) => {
                apply_alter_table_operations(
                    &mut ctx,
                    &mut tables,
                    &mut table_map,
                    &alter_table.name,
                    &alter_table.operations,
                );
            }
            _ => {}
        }
    }

    // Report unsupported statements
    for statement in &statements {
        match statement {
            Statement::CreateTable(_)
            | Statement::CreateIndex(_)
            | Statement::Comment { .. }
            | Statement::CreateView(_)
            | Statement::CreateType { .. }
            | Statement::AlterTable(_) => {
                // Handled in the ordered schema pass (ALTER warns per-operation there).
            }
            Statement::Insert { .. } => {
                ctx.info_skipped("INSERT");
            }
            Statement::Query(_) => {
                ctx.info_skipped("SELECT");
            }
            Statement::CreateFunction { .. } => {
                ctx.warn_unsupported("CREATE FUNCTION", None);
            }
            Statement::CreateTrigger { .. } => {
                ctx.warn_unsupported("CREATE TRIGGER", None);
            }
            Statement::CreateSequence { .. } => {
                ctx.warn_unsupported("CREATE SEQUENCE", None);
            }
            Statement::CreateExtension { .. } => {
                ctx.warn_unsupported("CREATE EXTENSION", None);
            }
            Statement::Drop { .. } => {
                ctx.warn_unsupported("DROP", None);
            }
            _ => {
                // Generic unsupported statement
                ctx.warn_unsupported(&format!("{statement:?}"), None);
            }
        }
    }

    let schema = if tables.is_empty() && views.is_empty() && enums.is_empty() {
        // Only return None if we have errors; empty schema is valid for no tables/views/enums
        if ctx.has_errors() {
            None
        } else {
            Some(Schema {
                tables,
                views,
                enums,
            })
        }
    } else {
        Some(Schema {
            tables,
            views,
            enums,
        })
    };

    ParseOutput {
        dialect: resolved_dialect,
        schema,
        diagnostics: ctx.diagnostics,
    }
}

/// Parse a CREATE TABLE statement into a Table.
#[allow(clippy::too_many_lines)]
#[allow(clippy::unnecessary_wraps)]
fn parse_create_table(
    ctx: &mut ParseContext,
    create: &sqlparser::ast::CreateTable,
) -> Option<Table> {
    let (schema_name, name) = split_object_name(&create.name);
    let stable_id = match &schema_name {
        Some(s) => format!("{s}.{name}"),
        None => name.clone(),
    };

    let table_id = ctx.next_table_id();

    // Parse columns
    let mut columns = Vec::new();
    let mut next_column_id: u64 = 1;

    for column in &create.columns {
        let mut nullable = true;
        let mut is_primary_key = false;

        for option in &column.options {
            match &option.option {
                ColumnOption::NotNull => nullable = false,
                ColumnOption::Null => nullable = true,
                ColumnOption::PrimaryKey(_) => {
                    is_primary_key = true;
                    nullable = false;
                }
                ColumnOption::Unique(_)
                | ColumnOption::Default(_)
                | ColumnOption::Check(_)
                | ColumnOption::DialectSpecific(_)
                | ColumnOption::CharacterSet(_)
                | ColumnOption::Collation(_)
                | ColumnOption::OnUpdate(_)
                | ColumnOption::Generated { .. }
                | ColumnOption::Comment(_)
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
                | ColumnOption::Invisible => {
                    // Informational only, not relevant to schema extraction, or handled separately
                }
            }
        }

        // Normalize the column name
        let column_name = normalize_identifier(&column.name.value);

        columns.push(Column {
            id: ColumnId(next_column_id),
            name: column_name,
            data_type: column.data_type.to_string(),
            nullable,
            is_primary_key,
            comment: None,
        });
        next_column_id += 1;
    }

    // Parse inline foreign key constraints from columns
    let mut foreign_keys = Vec::new();
    for column in &create.columns {
        for option in &column.options {
            if let ColumnOption::ForeignKey(constraint) = &option.option {
                let from_column = normalize_identifier(&column.name.value);
                let to_table = normalize_object_name(&constraint.foreign_table);
                let to_columns: Vec<String> = constraint
                    .referred_columns
                    .iter()
                    .map(|c| normalize_identifier(&c.value))
                    .collect();

                foreign_keys.push(ForeignKey {
                    name: option
                        .name
                        .as_ref()
                        .map(|ident| normalize_identifier(&ident.value)),
                    from_columns: vec![from_column],
                    to_schema: None,
                    to_table,
                    to_columns,
                    on_delete: convert_referential_action(constraint.on_delete),
                    on_update: convert_referential_action(constraint.on_update),
                });
            }
        }
    }

    // Parse table-level constraints
    for constraint in &create.constraints {
        match constraint {
            TableConstraint::PrimaryKey(primary_key) => {
                // Mark columns as primary key
                for pk_col in &primary_key.columns {
                    let col_name = extract_column_name(pk_col);
                    if let Some(column) = columns.iter_mut().find(|c| c.name == col_name) {
                        column.is_primary_key = true;
                        column.nullable = false;
                    }
                }
            }
            TableConstraint::Unique(unique) => {
                // UNIQUE constraints don't mark as primary key, but we could track them
                // For now, just extract column names for informational purposes
                let _col_names: Vec<String> =
                    unique.columns.iter().map(extract_column_name).collect();
            }
            TableConstraint::ForeignKey(foreign_key) => {
                let from_cols: Vec<String> = foreign_key
                    .columns
                    .iter()
                    .map(|c| normalize_identifier(&c.value))
                    .collect();
                let to_table = normalize_object_name(&foreign_key.foreign_table);
                let to_cols: Vec<String> = foreign_key
                    .referred_columns
                    .iter()
                    .map(|c| normalize_identifier(&c.value))
                    .collect();

                foreign_keys.push(ForeignKey {
                    name: foreign_key
                        .name
                        .as_ref()
                        .map(|ident| normalize_identifier(&ident.value)),
                    from_columns: from_cols,
                    to_schema: None,
                    to_table,
                    to_columns: to_cols,
                    on_delete: convert_referential_action(foreign_key.on_delete),
                    on_update: convert_referential_action(foreign_key.on_update),
                });
            }
            TableConstraint::Check(_) | TableConstraint::Index(_) => {
                // Check constraints and Index constraints are informational only
            }
            TableConstraint::FulltextOrSpatial(_) => {
                ctx.warn_unsupported("FULLTEXT/SPATIAL constraint", None);
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
        indexes: Vec::new(), // Indexes are added in second pass
        comment: None,       // Comments are added in third pass
    })
}

/// Extract the column name from an `IndexColumn`.
fn extract_column_name(index_col: &sqlparser::ast::IndexColumn) -> String {
    // The column is an OrderByExpr which contains an Expr
    // For simple column references, Expr is likely an Identifier
    let expr = &index_col.column.expr;

    // Try to extract identifier name from the expression
    match expr {
        sqlparser::ast::Expr::Identifier(ident) => normalize_identifier(&ident.value),
        _ => {
            // Fallback: use the string representation
            normalize_identifier(&expr.to_string())
        }
    }
}

/// Parse a CREATE INDEX statement and attach it to the appropriate table.
fn parse_create_index(
    ctx: &mut ParseContext,
    create_index: &CreateIndex,
    tables: &mut [Table],
    table_map: &std::collections::HashMap<String, usize>,
) {
    // Get the table name
    let (schema_name, table_name) = split_object_name(&create_index.table_name);
    let stable_id = match &schema_name {
        Some(s) => format!(
            "{}.{}",
            normalize_identifier(s),
            normalize_identifier(&table_name)
        ),
        None => normalize_identifier(&table_name),
    };

    // Find the table
    let Some(&table_idx) = table_map.get(&stable_id) else {
        ctx.diagnostics.push(Diagnostic::warning(
            codes::schema_unknown_table(),
            format!("CREATE INDEX references unknown table: {stable_id}"),
        ));
        return;
    };

    // Extract index columns
    let index_columns: Vec<String> = create_index
        .columns
        .iter()
        .map(extract_column_name)
        .collect();

    let index = Index {
        name: create_index.name.as_ref().map(|ident| {
            // ObjectName is a wrapper around Vec<ObjectNamePart>
            ident
                .0
                .first()
                .map(|part| normalize_identifier(&object_name_part_to_string(part)))
                .unwrap_or_default()
        }),
        columns: index_columns,
        is_unique: create_index.unique,
    };

    tables[table_idx].indexes.push(index);
}

/// Parse a COMMENT ON statement and apply it to the appropriate table or column.
#[allow(clippy::ref_option)]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn parse_comment(
    ctx: &mut ParseContext,
    object_type: sqlparser::ast::CommentObject,
    object_name: &ObjectName,
    comment: Option<&String>,
    tables: &mut [Table],
    table_map: &std::collections::HashMap<String, usize>,
) {
    let comment_text = match comment {
        Some(c) => c.clone(),
        None => return, // NULL comment means remove comment, we just skip
    };

    match object_type {
        sqlparser::ast::CommentObject::Table => {
            let (schema_name, table_name) = split_object_name(object_name);
            let stable_id = match &schema_name {
                Some(s) => format!(
                    "{}.{}",
                    normalize_identifier(s),
                    normalize_identifier(&table_name)
                ),
                None => normalize_identifier(&table_name),
            };

            if let Some(&table_idx) = table_map.get(&stable_id) {
                tables[table_idx].comment = Some(comment_text);
            } else {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::schema_unknown_table(),
                    format!("COMMENT ON TABLE references unknown table: {stable_id}"),
                ));
            }
        }
        sqlparser::ast::CommentObject::Column => {
            // For columns, object_name is typically "table.column" or "schema.table.column"
            let parts: Vec<String> = object_name
                .0
                .iter()
                .map(object_name_part_to_string)
                .collect();

            // Extract column name (last part) and table name (remaining parts)
            if parts.len() < 2 {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::parse_unsupported(),
                    "Invalid COMMENT ON COLUMN syntax: expected table.column".to_string(),
                ));
                return;
            }

            let column_name = normalize_identifier(&parts[parts.len() - 1]);
            let table_parts = &parts[..parts.len() - 1];

            let stable_id = match table_parts {
                [table] => normalize_identifier(table),
                [schema, table] => format!(
                    "{}.{}",
                    normalize_identifier(schema),
                    normalize_identifier(table)
                ),
                _ => {
                    ctx.diagnostics.push(Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!("Invalid COMMENT ON COLUMN syntax: {}", parts.join(".")),
                    ));
                    return;
                }
            };

            if let Some(&table_idx) = table_map.get(&stable_id) {
                if let Some(column) = tables[table_idx]
                    .columns
                    .iter_mut()
                    .find(|c| c.name == column_name)
                {
                    column.comment = Some(comment_text);
                } else {
                    ctx.diagnostics.push(Diagnostic::warning(
                        codes::schema_unknown_column(),
                        format!(
                            "COMMENT ON COLUMN references unknown column: {stable_id}.{column_name}"
                        ),
                    ));
                }
            } else {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::schema_unknown_table(),
                    format!("COMMENT ON COLUMN references unknown table: {stable_id}"),
                ));
            }
        }
        _ => {
            // Other comment types (view, function, etc.) are not supported
            ctx.warn_unsupported(&format!("COMMENT ON {object_type:?}"), None);
        }
    }
}

/// Build `stable_id` the same way as [`parse_create_table`] so `ALTER TABLE` resolves targets.
fn stable_id_for_alter_target(table_name: &ObjectName) -> String {
    let (schema_name, name) = split_object_name(table_name);
    match schema_name {
        Some(s) => format!("{s}.{name}"),
        None => name,
    }
}

#[allow(clippy::too_many_lines)]
fn apply_alter_table_operations(
    ctx: &mut ParseContext,
    tables: &mut [Table],
    table_map: &mut HashMap<String, usize>,
    table_name: &ObjectName,
    operations: &[AlterTableOperation],
) {
    let stable_id = stable_id_for_alter_target(table_name);
    let Some(&idx) = table_map.get(&stable_id) else {
        ctx.diagnostics.push(Diagnostic::warning(
            codes::schema_unknown_table(),
            format!("ALTER TABLE references unknown table: {stable_id}"),
        ));
        return;
    };

    for op in operations {
        apply_single_alter_operation(ctx, tables, table_map, idx, op);
    }
}

#[allow(clippy::too_many_lines)]
fn apply_single_alter_operation(
    ctx: &mut ParseContext,
    tables: &mut [Table],
    table_map: &mut HashMap<String, usize>,
    idx: usize,
    op: &AlterTableOperation,
) {
    match op {
        AlterTableOperation::AddColumn {
            column_def,
            if_not_exists,
            ..
        } => {
            add_column_from_alter(ctx, &mut tables[idx], column_def, *if_not_exists);
        }
        AlterTableOperation::DropColumn {
            column_names,
            if_exists,
            ..
        } => {
            let stable = tables[idx].stable_id.clone();
            for ident in column_names {
                let col_name = normalize_identifier(&ident.value);
                let table = &mut tables[idx];
                let pos = table.columns.iter().position(|c| c.name == col_name);
                if let Some(p) = pos {
                    table.columns.remove(p);
                    table
                        .foreign_keys
                        .retain(|fk| !fk.from_columns.contains(&col_name));
                    for ix in &mut table.indexes {
                        ix.columns.retain(|c| *c != col_name);
                    }
                } else if !if_exists {
                    ctx.diagnostics.push(Diagnostic::warning(
                        codes::schema_unknown_column(),
                        format!(
                            "ALTER TABLE DROP COLUMN: unknown column `{col_name}` on `{stable}`"
                        ),
                    ));
                }
            }
        }
        AlterTableOperation::AddConstraint { constraint, .. } => {
            apply_add_table_constraint(ctx, &mut tables[idx], constraint);
        }
        AlterTableOperation::DropConstraint {
            if_exists, name, ..
        } => {
            let cname = name.value.clone();
            let cname_norm = normalize_identifier(&cname);
            let stable = tables[idx].stable_id.clone();
            let table = &mut tables[idx];
            let before_fk = table.foreign_keys.len();
            let before_ix = table.indexes.len();
            table.foreign_keys.retain(|fk| {
                fk.name.as_ref().map(|n| normalize_identifier(n)) != Some(cname_norm.clone())
            });
            table.indexes.retain(|ix| {
                ix.name.as_ref().map(|n| normalize_identifier(n)) != Some(cname_norm.clone())
            });
            if table.foreign_keys.len() == before_fk
                && table.indexes.len() == before_ix
                && !if_exists
            {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::parse_unsupported(),
                    format!(
                        "ALTER TABLE DROP CONSTRAINT: no constraint named `{cname}` on `{stable}`"
                    ),
                ));
            }
        }
        AlterTableOperation::RenameColumn {
            old_column_name,
            new_column_name,
        } => {
            let old = normalize_identifier(&old_column_name.value);
            let new = normalize_identifier(&new_column_name.value);
            let stable = tables[idx].stable_id.clone();
            let table = &mut tables[idx];
            if let Some(col) = table.columns.iter_mut().find(|c| c.name == old) {
                col.name.clone_from(&new);
                for fk in &mut table.foreign_keys {
                    for c in &mut fk.from_columns {
                        if *c == old {
                            c.clone_from(&new);
                        }
                    }
                }
                for ix in &mut table.indexes {
                    for c in &mut ix.columns {
                        if *c == old {
                            c.clone_from(&new);
                        }
                    }
                }
            } else {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::schema_unknown_column(),
                    format!("ALTER TABLE RENAME COLUMN: unknown `{old}` on `{stable}`"),
                ));
            }
        }
        AlterTableOperation::RenameTable {
            table_name: new_table,
        } => {
            let old_stable = tables[idx].stable_id.clone();
            let renamed_target = match new_table {
                sqlparser::ast::RenameTableNameKind::As(name)
                | sqlparser::ast::RenameTableNameKind::To(name) => name,
            };
            let (new_schema_raw, new_name_raw) = split_object_name(renamed_target);
            let renamed_stable_id = match &new_schema_raw {
                Some(s) => format!("{s}.{new_name_raw}"),
                None => new_name_raw.clone(),
            };
            let table = &mut tables[idx];
            table.schema_name = new_schema_raw.map(|s| normalize_identifier(&s));
            table.name = normalize_identifier(&new_name_raw);
            table.stable_id.clone_from(&renamed_stable_id);
            table_map.remove(&old_stable);
            table_map.insert(renamed_stable_id, idx);
        }
        AlterTableOperation::DropPrimaryKey { .. } => {
            for col in &mut tables[idx].columns {
                col.is_primary_key = false;
            }
        }
        AlterTableOperation::DropForeignKey { name, .. } => {
            let sym = normalize_identifier(&name.value);
            let stable = tables[idx].stable_id.clone();
            let table = &mut tables[idx];
            let before = table.foreign_keys.len();
            table.foreign_keys.retain(|fk| {
                fk.name.as_ref().map(|n| normalize_identifier(n)) != Some(sym.clone())
            });
            if table.foreign_keys.len() == before {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::parse_unsupported(),
                    format!("ALTER TABLE DROP FOREIGN KEY: no FK named `{sym}` on `{stable}`"),
                ));
            }
        }
        AlterTableOperation::DropIndex { name } => {
            let n = normalize_identifier(&name.value);
            let stable = tables[idx].stable_id.clone();
            let table = &mut tables[idx];
            let before = table.indexes.len();
            table.indexes.retain(|ix| {
                ix.name.as_ref().map(|nm| normalize_identifier(nm)) != Some(n.clone())
            });
            if table.indexes.len() == before {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::parse_unsupported(),
                    format!("ALTER TABLE DROP INDEX: no index named `{n}` on `{stable}`"),
                ));
            }
        }
        other => {
            ctx.warn_unsupported(
                &format!("ALTER TABLE operation (unsupported): {other:?}"),
                None,
            );
        }
    }
}

fn column_from_column_def_body(column: &sqlparser::ast::ColumnDef) -> Column {
    let mut nullable = true;
    let mut is_primary_key = false;

    for option in &column.options {
        match &option.option {
            ColumnOption::NotNull => nullable = false,
            ColumnOption::Null => nullable = true,
            ColumnOption::PrimaryKey(_) => {
                is_primary_key = true;
                nullable = false;
            }
            ColumnOption::Unique(_)
            | ColumnOption::Default(_)
            | ColumnOption::Check(_)
            | ColumnOption::DialectSpecific(_)
            | ColumnOption::CharacterSet(_)
            | ColumnOption::Collation(_)
            | ColumnOption::OnUpdate(_)
            | ColumnOption::Generated { .. }
            | ColumnOption::Comment(_)
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

    let column_name = normalize_identifier(&column.name.value);
    Column {
        id: ColumnId(0),
        name: column_name,
        data_type: column.data_type.to_string(),
        nullable,
        is_primary_key,
        comment: None,
    }
}

fn add_column_from_alter(
    ctx: &mut ParseContext,
    table: &mut Table,
    column_def: &sqlparser::ast::ColumnDef,
    if_not_exists: bool,
) {
    let col_name = normalize_identifier(&column_def.name.value);
    if table.columns.iter().any(|c| c.name == col_name) {
        if !if_not_exists {
            ctx.diagnostics.push(Diagnostic::warning(
                codes::parse_unsupported(),
                format!(
                    "ALTER TABLE ADD COLUMN: duplicate column `{col_name}` on `{}`",
                    table.stable_id
                ),
            ));
        }
        return;
    }

    let next_id = table.columns.iter().map(|c| c.id.0).max().unwrap_or(0) + 1;
    let mut col = column_from_column_def_body(column_def);
    col.id = ColumnId(next_id);
    table.columns.push(col);

    for option in &column_def.options {
        if let ColumnOption::ForeignKey(constraint) = &option.option {
            let from_column = col_name.clone();
            let to_table = normalize_object_name(&constraint.foreign_table);
            let to_columns: Vec<String> = constraint
                .referred_columns
                .iter()
                .map(|c| normalize_identifier(&c.value))
                .collect();

            table.foreign_keys.push(ForeignKey {
                name: option
                    .name
                    .as_ref()
                    .map(|ident| normalize_identifier(&ident.value)),
                from_columns: vec![from_column],
                to_schema: None,
                to_table,
                to_columns,
                on_delete: convert_referential_action(constraint.on_delete),
                on_update: convert_referential_action(constraint.on_update),
            });
        }
    }
}

fn apply_add_table_constraint(
    ctx: &mut ParseContext,
    table: &mut Table,
    constraint: &TableConstraint,
) {
    match constraint {
        TableConstraint::PrimaryKey(primary_key) => {
            for pk_col in &primary_key.columns {
                let col_name = extract_column_name(pk_col);
                if let Some(column) = table.columns.iter_mut().find(|c| c.name == col_name) {
                    column.is_primary_key = true;
                    column.nullable = false;
                }
            }
        }
        TableConstraint::Unique(unique) => {
            let col_names: Vec<String> = unique.columns.iter().map(extract_column_name).collect();
            let index_name = unique
                .name
                .as_ref()
                .map(|ident| normalize_identifier(&ident.value));
            table.indexes.push(Index {
                name: index_name,
                columns: col_names,
                is_unique: true,
            });
        }
        TableConstraint::ForeignKey(foreign_key) => {
            let from_cols: Vec<String> = foreign_key
                .columns
                .iter()
                .map(|c| normalize_identifier(&c.value))
                .collect();
            let to_table = normalize_object_name(&foreign_key.foreign_table);
            let to_cols: Vec<String> = foreign_key
                .referred_columns
                .iter()
                .map(|c| normalize_identifier(&c.value))
                .collect();

            table.foreign_keys.push(ForeignKey {
                name: foreign_key
                    .name
                    .as_ref()
                    .map(|ident| normalize_identifier(&ident.value)),
                from_columns: from_cols,
                to_schema: None,
                to_table,
                to_columns: to_cols,
                on_delete: convert_referential_action(foreign_key.on_delete),
                on_update: convert_referential_action(foreign_key.on_update),
            });
        }
        TableConstraint::Check(_) | TableConstraint::Index(_) => {}
        TableConstraint::FulltextOrSpatial(_) => {
            ctx.warn_unsupported("FULLTEXT/SPATIAL constraint", None);
        }
    }
}

/// Parse a CREATE TYPE ... AS ENUM statement into an Enum.
fn parse_create_type_enum(name: &ObjectName, labels: &[sqlparser::ast::Ident]) -> Enum {
    let (schema_name, type_name) = split_object_name(name);

    // Generate a stable ID for the enum
    let id = match &schema_name {
        Some(s) => format!("{s}.{type_name}"),
        None => type_name.clone(),
    };

    // Extract enum values
    let values: Vec<String> = labels.iter().map(|l| l.value.clone()).collect();

    // Normalize names
    let normalized_schema = schema_name.map(|s| normalize_identifier(&s));
    let normalized_name = normalize_identifier(&type_name);

    Enum {
        id,
        schema_name: normalized_schema,
        name: normalized_name,
        values,
    }
}

/// Parse a CREATE VIEW statement into a View.
#[allow(clippy::unnecessary_wraps)]
fn parse_create_view(
    name: &ObjectName,
    view_columns: &[sqlparser::ast::ViewColumnDef],
    query: &sqlparser::ast::Query,
) -> Option<View> {
    let (schema_name, view_name) = split_object_name(name);

    // Generate a stable ID for the view
    let id = match &schema_name {
        Some(s) => format!("{s}.{view_name}"),
        None => view_name.clone(),
    };

    // Get the query definition as a string
    let definition = query.to_string();

    // Normalize names
    let normalized_schema = schema_name.map(|s| normalize_identifier(&s));
    let normalized_name = normalize_identifier(&view_name);

    // Extract columns: prefer explicit column list, fall back to SELECT items
    let columns = if view_columns.is_empty() {
        extract_view_columns_from_query(query)
    } else {
        extract_view_columns_from_defs(view_columns)
    };

    Some(View {
        id,
        schema_name: normalized_schema,
        name: normalized_name,
        columns,
        definition: Some(definition),
    })
}

/// Extract columns from explicit VIEW column definitions.
fn extract_view_columns_from_defs(defs: &[sqlparser::ast::ViewColumnDef]) -> Vec<Column> {
    defs.iter()
        .enumerate()
        .map(|(i, def)| {
            let data_type = def
                .data_type
                .as_ref()
                .map_or_else(|| "unknown".to_string(), std::string::ToString::to_string);
            Column {
                id: ColumnId(i as u64),
                name: normalize_identifier(&def.name.value),
                data_type,
                nullable: true,
                is_primary_key: false,
                comment: None,
            }
        })
        .collect()
}

/// Extract column names from the SELECT items in a view query.
fn extract_view_columns_from_query(query: &sqlparser::ast::Query) -> Vec<Column> {
    use sqlparser::ast::{SelectItem, SetExpr};

    let SetExpr::Select(select) = query.body.as_ref() else {
        return Vec::new();
    };

    let mut columns = Vec::new();
    for (i, item) in select.projection.iter().enumerate() {
        let col_name = match item {
            SelectItem::UnnamedExpr(expr) => extract_expr_column_name(expr),
            SelectItem::ExprWithAlias { alias, .. } => Some(normalize_identifier(&alias.value)),
            SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _) => None,
        };
        if let Some(name) = col_name {
            columns.push(Column {
                id: ColumnId(i as u64),
                name,
                data_type: "unknown".to_string(),
                nullable: true,
                is_primary_key: false,
                comment: None,
            });
        }
    }
    columns
}

/// Try to extract a column name from a simple expression.
fn extract_expr_column_name(expr: &sqlparser::ast::Expr) -> Option<String> {
    use sqlparser::ast::Expr;

    match expr {
        Expr::Identifier(ident) => Some(normalize_identifier(&ident.value)),
        Expr::CompoundIdentifier(parts) => {
            // Take the last part (e.g., "t.column_name" -> "column_name")
            parts.last().map(|ident| normalize_identifier(&ident.value))
        }
        _ => None,
    }
}

/// Split an `ObjectName` into (`schema_name`, `table_name`).
fn split_object_name(name: &ObjectName) -> (Option<String>, String) {
    let parts: Vec<String> = name.0.iter().map(object_name_part_to_string).collect();
    match parts.as_slice() {
        [table] => (None, table.clone()),
        [schema_name, table] => (Some(schema_name.clone()), table.clone()),
        [.., schema_name, table] => {
            // Handle longer qualified names by taking last two parts
            (Some(schema_name.clone()), table.clone())
        }
        [] => (None, String::new()),
    }
}

/// Convert an `ObjectNamePart` to a string.
fn object_name_part_to_string(part: &ObjectNamePart) -> String {
    match part {
        ObjectNamePart::Identifier(ident) => ident.value.clone(),
        ObjectNamePart::Function(func) => func.to_string(),
    }
}

/// Convert sqlparser's `ReferentialAction` to our model type.
const fn convert_referential_action(
    action: Option<sqlparser::ast::ReferentialAction>,
) -> ReferentialAction {
    match action {
        Some(sqlparser::ast::ReferentialAction::Cascade) => ReferentialAction::Cascade,
        Some(sqlparser::ast::ReferentialAction::SetNull) => ReferentialAction::SetNull,
        Some(sqlparser::ast::ReferentialAction::SetDefault) => ReferentialAction::SetDefault,
        Some(sqlparser::ast::ReferentialAction::Restrict) => ReferentialAction::Restrict,
        Some(sqlparser::ast::ReferentialAction::NoAction) | None => ReferentialAction::NoAction,
    }
}

/// Normalize an `ObjectName` to a single string (schema.table or just table).
fn normalize_object_name(name: &ObjectName) -> String {
    name.0
        .iter()
        .map(|part| normalize_identifier(&object_name_part_to_string(part)))
        .collect::<Vec<_>>()
        .join(".")
}

fn error_summary(output: &ParseOutput) -> String {
    let messages = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| {
            format!(
                "{} {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            )
        })
        .collect::<Vec<_>>();

    if messages.is_empty() {
        "Failed to parse any valid schema elements".to_string()
    } else {
        format!(
            "SQL parsing reported error diagnostics: {}",
            messages.join("; ")
        )
    }
}

// Keep the old function name for backward compatibility
/// Legacy function - use `parse_sql_to_schema` instead.
#[deprecated(since = "0.2.0", note = "Use `parse_sql_to_schema` instead")]
pub fn parse_schema(sql: &str) -> Result<Schema, ParseError> {
    parse_sql_to_schema(sql)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use relune_testkit::read_sql_fixture;

    fn snapshot_data(output: &ParseOutput) -> serde_json::Value {
        serde_json::json!({
            "dialect": output.dialect,
            "schema": output.schema,
            "diagnostics": output.diagnostics.iter().map(|d| serde_json::json!({
                "severity": format!("{}", d.severity),
                "code": d.code.full_code(),
                "message": d.message,
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn parses_primary_keys_and_foreign_keys() {
        let sql = r"
        CREATE TABLE public.users (
          id BIGINT PRIMARY KEY,
          name TEXT NOT NULL
        );

        CREATE TABLE posts (
          id BIGINT PRIMARY KEY,
          user_id BIGINT NOT NULL REFERENCES public.users(id),
          title TEXT NOT NULL,
          CONSTRAINT fk_posts_user FOREIGN KEY (user_id) REFERENCES public.users(id)
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 2);

        let users = &schema.tables[0];
        assert_eq!(users.stable_id, "public.users");
        assert!(users.columns[0].is_primary_key);
        assert_eq!(users.columns[0].name, "id");

        let posts = &schema.tables[1];
        assert!(posts.columns[0].is_primary_key);
        // Should have two foreign keys: one inline, one table-level
        assert_eq!(posts.foreign_keys.len(), 2);
    }

    #[test]
    fn parses_create_index() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          email TEXT NOT NULL
        );

        CREATE INDEX idx_users_email ON users (email);
        CREATE UNIQUE INDEX idx_users_id ON users (id);
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(users.indexes.len(), 2);

        let email_idx = &users.indexes[0];
        assert_eq!(email_idx.name, Some("idx_users_email".to_string()));
        assert_eq!(email_idx.columns, vec!["email"]);
        assert!(!email_idx.is_unique);

        let id_idx = &users.indexes[1];
        assert_eq!(id_idx.name, Some("idx_users_id".to_string()));
        assert_eq!(id_idx.columns, vec!["id"]);
        assert!(id_idx.is_unique);
    }

    #[test]
    fn handles_schema_qualified_names() {
        let sql = r"
        CREATE TABLE public.users (
          id BIGINT PRIMARY KEY
        );

        CREATE TABLE app.posts (
          id BIGINT PRIMARY KEY
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 2);

        assert_eq!(schema.tables[0].schema_name, Some("public".to_string()));
        assert_eq!(schema.tables[0].name, "users");
        assert_eq!(schema.tables[1].schema_name, Some("app".to_string()));
        assert_eq!(schema.tables[1].name, "posts");
    }

    #[test]
    fn normalizes_identifiers() {
        let sql = r"
        CREATE TABLE Users (
          ID BIGINT PRIMARY KEY,
          Name TEXT NOT NULL
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];

        assert_eq!(table.name, "users");
        assert_eq!(table.columns[0].name, "id");
        assert_eq!(table.columns[1].name, "name");
    }

    #[test]
    fn handles_table_level_primary_key() {
        let sql = r"
        CREATE TABLE order_items (
          order_id BIGINT NOT NULL,
          product_id BIGINT NOT NULL,
          quantity INTEGER NOT NULL,
          PRIMARY KEY (order_id, product_id)
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];

        assert!(table.columns[0].is_primary_key);
        assert!(table.columns[1].is_primary_key);
        assert!(!table.columns[2].is_primary_key);
    }

    #[test]
    fn returns_diagnostics_for_unsupported_constructs() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        CREATE VIEW user_view AS SELECT * FROM users;
        DROP TABLE users;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);

        assert!(output.schema.is_some());
        assert!(output.has_warnings());

        assert_eq!(
            output
                .diagnostics
                .iter()
                .filter(|d| d.code == codes::parse_unsupported())
                .count(),
            1
        );
    }

    #[test]
    fn alter_table_add_column_before_create_index() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users ADD COLUMN email TEXT;
        CREATE INDEX idx_users_email ON users (email);
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(table.columns.iter().any(|c| c.name == "email"));
        assert!(
            table
                .indexes
                .iter()
                .any(|i| i.columns.iter().any(|c| c == "email"))
        );
    }

    #[test]
    fn alter_table_add_foreign_key_constraint() {
        let sql = r"
        CREATE TABLE orgs (id BIGINT PRIMARY KEY);
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users ADD COLUMN org_id BIGINT;
        ALTER TABLE users ADD CONSTRAINT fk_users_org
          FOREIGN KEY (org_id) REFERENCES orgs (id);
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(
            users
                .foreign_keys
                .iter()
                .any(|fk| fk.name.as_deref() == Some("fk_users_org"))
        );
    }

    #[test]
    fn handles_duplicate_tables() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);

        assert!(output.schema.is_some());
        assert_eq!(output.schema.as_ref().unwrap().tables.len(), 1);

        assert!(output.has_warnings());
        assert_eq!(
            output
                .diagnostics
                .iter()
                .filter(|d| d.code == codes::schema_duplicate_table())
                .count(),
            1
        );
    }

    #[test]
    fn handles_invalid_sql() {
        let sql = "THIS IS NOT VALID SQL";

        let output = parse_sql_to_schema_with_diagnostics(sql);

        assert!(output.schema.is_none());
        assert!(output.has_errors());
    }

    #[test]
    fn strict_parse_rejects_error_diagnostics() {
        let sql = "THIS IS NOT VALID SQL";

        let err = parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres)
            .expect_err("strict parsing should reject error diagnostics");
        assert!(err.to_string().contains("error diagnostics"));
    }

    #[test]
    fn normalizes_constraint_and_index_names_on_storage() {
        let sql = r"
        CREATE TABLE orgs (
            id BIGINT PRIMARY KEY
        );

        CREATE TABLE users (
            id BIGINT PRIMARY KEY,
            org_id BIGINT,
            email TEXT,
            CONSTRAINT FK_USERS_ORG FOREIGN KEY (org_id) REFERENCES orgs(id)
        );

        CREATE INDEX IDX_USERS_EMAIL ON users (email);
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema
            .tables
            .iter()
            .find(|table| table.name == "users")
            .unwrap();

        assert_eq!(users.foreign_keys[0].name.as_deref(), Some("fk_users_org"));
        assert_eq!(users.indexes[0].name.as_deref(), Some("idx_users_email"));
    }

    #[test]
    fn parse_output_helpers() {
        let output = ParseOutput {
            dialect: SqlDialect::Postgres,
            schema: Some(Schema {
                tables: vec![],
                views: vec![],
                enums: vec![],
            }),
            diagnostics: vec![Diagnostic::warning(codes::parse_unsupported(), "test")],
        };

        assert!(!output.has_errors());
        assert!(output.has_warnings());

        let output_with_errors = ParseOutput {
            dialect: SqlDialect::Postgres,
            schema: None,
            diagnostics: vec![Diagnostic::error(codes::parse_error(), "test")],
        };

        assert!(output_with_errors.has_errors());
    }

    #[test]
    fn handles_composite_foreign_keys() {
        let sql = r"
        CREATE TABLE orders (
          id BIGINT PRIMARY KEY
        );

        CREATE TABLE order_items (
          order_id BIGINT NOT NULL,
          line_num INTEGER NOT NULL,
          product_id BIGINT NOT NULL,
          PRIMARY KEY (order_id, line_num),
          CONSTRAINT fk_order FOREIGN KEY (order_id) REFERENCES orders(id)
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 2);

        let order_items = &schema.tables[1];
        assert_eq!(order_items.foreign_keys.len(), 1);
        assert_eq!(
            order_items.foreign_keys[0].name,
            Some("fk_order".to_string())
        );
        assert_eq!(order_items.foreign_keys[0].from_columns, vec!["order_id"]);
        assert_eq!(order_items.foreign_keys[0].to_table, "orders");
        assert_eq!(order_items.foreign_keys[0].to_columns, vec!["id"]);
    }

    #[test]
    fn generates_sequential_ids() {
        let sql = r"
        CREATE TABLE first (id BIGINT PRIMARY KEY);
        CREATE TABLE second (id BIGINT PRIMARY KEY);
        CREATE TABLE third (id BIGINT PRIMARY KEY);
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 3);

        assert_eq!(schema.tables[0].id, TableId(1));
        assert_eq!(schema.tables[1].id, TableId(2));
        assert_eq!(schema.tables[2].id, TableId(3));
    }

    #[test]
    fn generates_column_ids_per_table() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          name TEXT NOT NULL,
          email TEXT
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];

        assert_eq!(table.columns[0].id, ColumnId(1));
        assert_eq!(table.columns[1].id, ColumnId(2));
        assert_eq!(table.columns[2].id, ColumnId(3));
    }

    #[test]
    fn handles_index_on_unknown_table() {
        let sql = r"
        CREATE INDEX idx_missing ON nonexistent_table (id);
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);

        // Schema should be Some but empty (no tables, but no errors)
        assert!(output.schema.is_some());
        assert_eq!(output.schema.as_ref().unwrap().tables.len(), 0);

        // Should have warning about unknown table
        assert!(output.has_warnings());
    }

    #[test]
    fn parses_create_type_as_enum() {
        let sql = r"
        CREATE TYPE status AS ENUM ('active', 'inactive', 'pending');
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.enums.len(), 1);

        let status_enum = &schema.enums[0];
        assert_eq!(status_enum.id, "status");
        assert_eq!(status_enum.schema_name, None);
        assert_eq!(status_enum.name, "status");
        assert_eq!(status_enum.values, vec!["active", "inactive", "pending"]);
    }

    #[test]
    fn parses_schema_qualified_enum() {
        let sql = r"
        CREATE TYPE public.user_role AS ENUM ('admin', 'user', 'guest');
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.enums.len(), 1);

        let role_enum = &schema.enums[0];
        assert_eq!(role_enum.id, "public.user_role");
        assert_eq!(role_enum.schema_name, Some("public".to_string()));
        assert_eq!(role_enum.name, "user_role");
        assert_eq!(role_enum.values, vec!["admin", "user", "guest"]);
    }

    #[test]
    fn handles_tables_and_enums_together() {
        let sql = r"
        CREATE TYPE status AS ENUM ('active', 'inactive');

        CREATE TABLE users (
            id BIGINT PRIMARY KEY,
            status TEXT NOT NULL
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.enums.len(), 1);

        assert_eq!(schema.enums[0].name, "status");
        assert_eq!(schema.tables[0].name, "users");
    }

    #[test]
    fn parses_comment_on_table() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          name TEXT NOT NULL
        );

        COMMENT ON TABLE users IS 'Stores user information';
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(users.comment, Some("Stores user information".to_string()));
    }

    #[test]
    fn parses_comment_on_column() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          email TEXT NOT NULL
        );

        COMMENT ON COLUMN users.email IS 'User email address';
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(users.columns[0].comment, None);
        assert_eq!(
            users.columns[1].comment,
            Some("User email address".to_string())
        );
    }

    #[test]
    fn parses_comment_on_schema_qualified_table() {
        let sql = r"
        CREATE TABLE public.users (
          id BIGINT PRIMARY KEY
        );

        COMMENT ON TABLE public.users IS 'Public users table';
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(users.comment, Some("Public users table".to_string()));
    }

    #[test]
    fn parses_comment_on_schema_qualified_column() {
        let sql = r"
        CREATE TABLE public.users (
          id BIGINT PRIMARY KEY,
          created_at TIMESTAMP
        );

        COMMENT ON COLUMN public.users.created_at IS 'Record creation timestamp';
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(
            users.columns[1].comment,
            Some("Record creation timestamp".to_string())
        );
    }

    #[test]
    fn handles_comment_on_unknown_table() {
        let sql = r"
        COMMENT ON TABLE nonexistent IS 'This table does not exist';
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);

        assert!(output.schema.is_some());
        assert!(output.has_warnings());

        assert_eq!(
            output
                .diagnostics
                .iter()
                .filter(|d| d.code == codes::schema_unknown_table())
                .count(),
            1
        );
    }

    #[test]
    fn handles_comment_on_unknown_column() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY
        );

        COMMENT ON COLUMN users.nonexistent IS 'This column does not exist';
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);

        assert!(output.schema.is_some());
        assert!(output.has_warnings());

        assert_eq!(
            output
                .diagnostics
                .iter()
                .filter(|d| d.code == codes::schema_unknown_column())
                .count(),
            1
        );
    }

    #[test]
    fn handles_null_comment() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY
        );

        COMMENT ON TABLE users IS NULL;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(users.comment, None);
    }

    #[test]
    fn parses_create_view() {
        let sql = r"
        CREATE TABLE users (
            id BIGINT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE VIEW user_view AS
            SELECT id, name FROM users WHERE id > 0;

        CREATE VIEW public.active_users AS
            SELECT id, name FROM users WHERE name IS NOT NULL;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.views.len(), 2);

        let user_view = &schema.views[0];
        assert_eq!(user_view.id, "user_view");
        assert_eq!(user_view.schema_name, None);
        assert_eq!(user_view.name, "user_view");
        assert!(user_view.definition.is_some());
        assert!(
            user_view
                .definition
                .as_ref()
                .unwrap()
                .contains("SELECT id, name FROM users")
        );
        // View columns are extracted from the SELECT items
        assert_eq!(user_view.columns.len(), 2);
        assert_eq!(user_view.columns[0].name, "id");
        assert_eq!(user_view.columns[1].name, "name");

        let active_users = &schema.views[1];
        assert_eq!(active_users.id, "public.active_users");
        assert_eq!(active_users.schema_name, Some("public".to_string()));
        assert_eq!(active_users.name, "active_users");
        assert!(active_users.definition.is_some());
    }

    // Snapshot tests for all fixtures
    mod snapshot_tests {
        use super::*;

        fn snapshot_fixture(name: &str, sql: &str) {
            let output = parse_sql_to_schema_with_diagnostics(sql);

            insta::assert_json_snapshot!(
                format!("fixture_{}", name.replace('.', "_")),
                snapshot_data(&output)
            );
        }

        #[test]
        fn snapshot_simple_blog() {
            let sql = read_sql_fixture("simple_blog.sql");
            snapshot_fixture("simple_blog", &sql);
        }

        #[test]
        fn snapshot_ecommerce() {
            let sql = read_sql_fixture("ecommerce.sql");
            snapshot_fixture("ecommerce", &sql);
        }

        #[test]
        fn snapshot_multi_schema() {
            let sql = read_sql_fixture("multi_schema.sql");
            snapshot_fixture("multi_schema", &sql);
        }

        #[test]
        fn snapshot_broken_input() {
            let sql = read_sql_fixture("broken_input.sql");
            snapshot_fixture("broken_input", &sql);
        }

        #[test]
        fn snapshot_cyclic_fk() {
            let sql = read_sql_fixture("cyclic_fk.sql");
            snapshot_fixture("cyclic_fk", &sql);
        }

        #[test]
        fn snapshot_join_heavy() {
            let sql = read_sql_fixture("join_heavy.sql");
            snapshot_fixture("join_heavy", &sql);
        }

        fn snapshot_fixture_with_dialect(name: &str, sql: &str, dialect: SqlDialect) {
            let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, dialect);

            insta::assert_json_snapshot!(
                format!("fixture_{}", name.replace('.', "_")),
                snapshot_data(&output)
            );
        }

        #[test]
        fn snapshot_mysql_ecommerce() {
            let sql = read_sql_fixture("mysql_ecommerce.sql");
            snapshot_fixture_with_dialect("mysql_ecommerce", &sql, SqlDialect::Mysql);
        }

        #[test]
        fn snapshot_sqlite_blog() {
            let sql = read_sql_fixture("sqlite_blog.sql");
            snapshot_fixture_with_dialect("sqlite_blog", &sql, SqlDialect::Sqlite);
        }
    }

    #[test]
    fn test_detect_dialect_mysql() {
        let sql = r"
            CREATE TABLE `users` (
                `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
                `name` VARCHAR(255) NOT NULL,
                PRIMARY KEY (`id`)
            ) ENGINE=InnoDB;
        ";
        assert_eq!(detect_dialect(sql), SqlDialect::Mysql);
    }

    #[test]
    fn test_detect_dialect_sqlite() {
        let sql = r"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );
        ";
        assert_eq!(detect_dialect(sql), SqlDialect::Sqlite);
    }

    #[test]
    fn test_detect_dialect_mysql_with_single_signal() {
        let sql = "CREATE TABLE `users` (`id` INT PRIMARY KEY);";
        assert_eq!(detect_dialect(sql), SqlDialect::Mysql);
    }

    #[test]
    fn test_detect_dialect_sqlite_with_single_signal() {
        let sql = r"
            CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL
            );
        ";
        assert_eq!(detect_dialect(sql), SqlDialect::Sqlite);
    }

    #[test]
    fn test_detect_dialect_postgres() {
        let sql = r"
            CREATE TYPE status AS ENUM ('active', 'inactive');
            CREATE TABLE users (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL
            );
            COMMENT ON TABLE users IS 'User accounts';
        ";
        assert_eq!(detect_dialect(sql), SqlDialect::Postgres);
    }

    #[test]
    fn test_detect_dialect_default_postgres() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name TEXT NOT NULL
            );
        ";
        // Generic SQL should default to Postgres
        assert_eq!(detect_dialect(sql), SqlDialect::Postgres);
    }

    proptest! {
        #[test]
        fn prop_detect_dialect_mysql_from_backticks(table in "[a-z][a-z0-9_]{0,15}") {
            let sql = format!("CREATE TABLE `{table}` (`id` INT PRIMARY KEY);");
            prop_assert_eq!(detect_dialect(&sql), SqlDialect::Mysql);
        }

        #[test]
        fn prop_detect_dialect_sqlite_from_integer_primary_key(table in "[a-z][a-z0-9_]{0,15}") {
            let sql = format!(
                "CREATE TABLE {table} (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"
            );
            prop_assert_eq!(detect_dialect(&sql), SqlDialect::Sqlite);
        }
    }

    #[test]
    fn auto_detection_matches_explicit_dialect_for_fixture_corpus() {
        let cases = [
            ("simple_blog.sql", SqlDialect::Postgres),
            ("ecommerce.sql", SqlDialect::Postgres),
            ("multi_schema.sql", SqlDialect::Postgres),
            ("cyclic_fk.sql", SqlDialect::Postgres),
            ("join_heavy.sql", SqlDialect::Postgres),
            ("mysql_ecommerce.sql", SqlDialect::Mysql),
            ("sqlite_blog.sql", SqlDialect::Sqlite),
        ];

        for (fixture, dialect) in cases {
            let sql = read_sql_fixture(fixture);
            let auto = parse_sql_to_schema_with_diagnostics(&sql);
            let explicit = parse_sql_to_schema_with_diagnostics_and_dialect(&sql, dialect);

            assert_eq!(
                auto.dialect, dialect,
                "auto-detected wrong dialect for {fixture}"
            );
            assert_eq!(
                snapshot_data(&auto),
                snapshot_data(&explicit),
                "auto-detected parse output diverged for {fixture}"
            );
        }
    }

    #[test]
    fn test_parse_mysql_basic() {
        let sql = r"
            CREATE TABLE `users` (
                `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
                `name` VARCHAR(255) NOT NULL,
                `email` VARCHAR(255) NOT NULL,
                PRIMARY KEY (`id`),
                UNIQUE KEY `idx_email` (`email`)
            ) ENGINE=InnoDB;
        ";
        let schema =
            parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(users.name, "users");
        assert_eq!(users.columns.len(), 3);
        assert_eq!(users.columns[0].name, "id");
        assert!(!users.columns[0].nullable);
    }

    #[test]
    fn test_parse_mysql_foreign_keys() {
        let sql = r"
            CREATE TABLE `users` (
                `id` BIGINT NOT NULL AUTO_INCREMENT,
                PRIMARY KEY (`id`)
            ) ENGINE=InnoDB;

            CREATE TABLE `posts` (
                `id` BIGINT NOT NULL AUTO_INCREMENT,
                `user_id` BIGINT NOT NULL,
                PRIMARY KEY (`id`),
                CONSTRAINT `fk_posts_user` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
            ) ENGINE=InnoDB;
        ";
        let schema =
            parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 2);

        let posts = &schema.tables[1];
        assert_eq!(posts.foreign_keys.len(), 1);
        assert_eq!(posts.foreign_keys[0].to_table, "users");
        assert_eq!(posts.foreign_keys[0].on_delete, ReferentialAction::Cascade);
    }

    #[test]
    fn test_parse_sqlite_basic() {
        let sql = r"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                email TEXT NOT NULL UNIQUE
            );
        ";
        let schema = parse_sql_to_schema_with_dialect(sql, SqlDialect::Sqlite)
            .expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);

        let users = &schema.tables[0];
        assert_eq!(users.name, "users");
        assert_eq!(users.columns.len(), 3);
        assert_eq!(users.columns[0].name, "id");
        assert!(users.columns[0].is_primary_key);
    }

    #[test]
    fn test_parse_sqlite_foreign_keys() {
        let sql = r"
            CREATE TABLE users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );

            CREATE TABLE posts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                author_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE CASCADE
            );
        ";
        let schema = parse_sql_to_schema_with_dialect(sql, SqlDialect::Sqlite)
            .expect("parse should succeed");
        assert_eq!(schema.tables.len(), 2);

        let posts = &schema.tables[1];
        assert_eq!(posts.foreign_keys.len(), 1);
        assert_eq!(posts.foreign_keys[0].to_table, "users");
    }

    #[test]
    fn test_parse_auto_detect_mysql() {
        let sql = r"
            CREATE TABLE `users` (
                `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
                `name` VARCHAR(255) NOT NULL,
                PRIMARY KEY (`id`)
            ) ENGINE=InnoDB;
        ";
        // Auto dialect should detect MySQL and parse correctly
        let schema =
            parse_sql_to_schema_with_dialect(sql, SqlDialect::Auto).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "users");
    }

    #[test]
    fn test_parse_auto_detect_sqlite() {
        let sql = r"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );
        ";
        // Auto dialect should detect SQLite and parse correctly
        let schema =
            parse_sql_to_schema_with_dialect(sql, SqlDialect::Auto).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "users");
    }

    #[test]
    fn test_sql_dialect_from_str() {
        assert_eq!(
            "postgres".parse::<SqlDialect>().unwrap(),
            SqlDialect::Postgres
        );
        assert_eq!(
            "postgresql".parse::<SqlDialect>().unwrap(),
            SqlDialect::Postgres
        );
        assert_eq!("pg".parse::<SqlDialect>().unwrap(), SqlDialect::Postgres);
        assert_eq!("mysql".parse::<SqlDialect>().unwrap(), SqlDialect::Mysql);
        assert_eq!("sqlite".parse::<SqlDialect>().unwrap(), SqlDialect::Sqlite);
        assert_eq!("sqlite3".parse::<SqlDialect>().unwrap(), SqlDialect::Sqlite);
        assert_eq!("auto".parse::<SqlDialect>().unwrap(), SqlDialect::Auto);
        assert!("unknown".parse::<SqlDialect>().is_err());
    }
}
