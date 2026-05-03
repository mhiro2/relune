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

mod alter_table;
mod comment;
mod context;
mod create_index;
mod create_table;
mod diagnostics;
mod dialect;
mod enum_type;
mod mysql_enum;
mod names;
mod recovery;
mod view;

use relune_core::{Diagnostic, Schema, Severity, SqlDialect};
use sqlparser::ast::{Spanned, Statement, UserDefinedTypeRepresentation};
use std::collections::HashMap;
use thiserror::Error;

// Re-export diagnostic codes for convenience
pub use relune_core::diagnostic::codes;

pub use dialect::detect_dialect;

use alter_table::apply_alter_table_operations;
use comment::parse_comment;
use context::{LineOffsets, ParseContext, source_span_from_sql_span};
use create_index::parse_create_index;
use create_table::parse_create_table;
use diagnostics::{error_summary, truncate_unsupported_debug};
use dialect::{dialect_impl, resolve_dialect};
use enum_type::parse_create_type_enum;
use mysql_enum::populate_mysql_enum_columns;
use recovery::parse_statements_with_recovery;
use view::parse_create_view;

/// Error type for parse failures.
#[derive(Debug, Error)]
pub enum ParseError {
    /// SQL parsing error message.
    #[error("SQL parse error: {0}")]
    Sql(String),

    /// Fatal error during schema construction.
    #[error("Schema error: {0}")]
    Schema(String),
}

impl From<sqlparser::parser::ParserError> for ParseError {
    fn from(error: sqlparser::parser::ParserError) -> Self {
        Self::Sql(error.to_string())
    }
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
    let offsets = LineOffsets::new(input);

    // Parse SQL statements with error recovery: parse statement-by-statement so that
    // a syntax error in one statement does not prevent parsing of subsequent statements.
    let statements = parse_statements_with_recovery(
        dialect_impl(resolved_dialect).as_ref(),
        input,
        &offsets,
        &mut ctx,
    );

    // Build schema in source order so ALTER TABLE is visible to later CREATE INDEX / COMMENT.
    let mut tables = Vec::new();
    let mut enums = Vec::new();
    let mut views = Vec::new();
    let mut table_map: HashMap<String, usize> = HashMap::new();

    for statement in &statements {
        match statement {
            Statement::CreateTable(create) => {
                if let Some(table) = parse_create_table(&mut ctx, input, &offsets, create) {
                    let stable_id = table.stable_id.clone();
                    if ctx.seen_tables.contains(&stable_id) {
                        ctx.warn_duplicate_table(
                            &stable_id,
                            source_span_from_sql_span(input, &offsets, create.span()),
                        );
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
                    let enum_def = parse_create_type_enum(&mut ctx, input, &offsets, name, labels);
                    enums.push(enum_def);
                } else {
                    ctx.warn_unsupported(
                        "CREATE TYPE (non-enum)",
                        source_span_from_sql_span(input, &offsets, statement.span()),
                    );
                }
            }
            Statement::CreateIndex(create_index) => {
                parse_create_index(
                    &mut ctx,
                    input,
                    &offsets,
                    create_index,
                    &mut tables,
                    &table_map,
                );
            }
            Statement::Comment {
                object_type,
                object_name,
                comment,
                ..
            } => {
                parse_comment(
                    &mut ctx,
                    input,
                    &offsets,
                    *object_type,
                    object_name,
                    comment.as_ref(),
                    &mut tables,
                    &table_map,
                );
            }
            Statement::CreateView(create_view) => {
                if let Some(view) = parse_create_view(
                    &mut ctx,
                    input,
                    &offsets,
                    &create_view.name,
                    &create_view.columns,
                    &create_view.query,
                ) {
                    views.push(view);
                }
            }
            Statement::AlterTable(alter_table) => {
                apply_alter_table_operations(
                    &mut ctx,
                    input,
                    &offsets,
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
                ctx.warn_unsupported(
                    "CREATE FUNCTION",
                    source_span_from_sql_span(input, &offsets, statement.span()),
                );
            }
            Statement::CreateTrigger { .. } => {
                ctx.warn_unsupported(
                    "CREATE TRIGGER",
                    source_span_from_sql_span(input, &offsets, statement.span()),
                );
            }
            Statement::CreateSequence { .. } => {
                ctx.warn_unsupported(
                    "CREATE SEQUENCE",
                    source_span_from_sql_span(input, &offsets, statement.span()),
                );
            }
            Statement::CreateExtension { .. } => {
                ctx.warn_unsupported(
                    "CREATE EXTENSION",
                    source_span_from_sql_span(input, &offsets, statement.span()),
                );
            }
            Statement::Drop { .. } => {
                ctx.warn_unsupported(
                    "DROP",
                    source_span_from_sql_span(input, &offsets, statement.span()),
                );
            }
            _ => {
                // Generic unsupported statement - truncate to avoid huge debug output
                let debug_str = format!("{statement:?}");
                let truncated = truncate_unsupported_debug(&debug_str);
                ctx.warn_unsupported(
                    &truncated,
                    source_span_from_sql_span(input, &offsets, statement.span()),
                );
            }
        }
    }

    if ctx.dialect == SqlDialect::Mysql {
        populate_mysql_enum_columns(&mut ctx, &mut tables);
    }

    let is_empty_schema = tables.is_empty() && views.is_empty() && enums.is_empty();
    if is_empty_schema && !ctx.has_errors() {
        ctx.warn_empty_schema();
    }

    let schema = if is_empty_schema && ctx.has_errors() {
        None
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

#[cfg(test)]
mod tests;
