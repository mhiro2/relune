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
    AlterTableOperation, ColumnOption, CreateIndex, ObjectName, ObjectNamePart, Spanned, Statement,
    TableConstraint, UserDefinedTypeRepresentation,
};
use sqlparser::dialect::{Dialect, GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Location, Span as SqlSpan, Token, Tokenizer};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

// Re-export diagnostic codes for convenience
pub use relune_core::diagnostic::codes;

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

/// Context for tracking parsing state and generating IDs.
/// Pre-computed byte offsets for each line start, enabling O(1) line
/// lookup followed by a short character walk instead of scanning the
/// entire input for every `Location → byte offset` conversion.
struct LineOffsets {
    /// Byte offset of the start of each 1-based line.
    /// `starts[0]` is unused; `starts[1]` = 0 (line 1 starts at byte 0).
    starts: Vec<usize>,
    /// Total byte length of the input.
    len: usize,
}

impl LineOffsets {
    fn new(input: &str) -> Self {
        let mut starts = vec![0, 0]; // index 0 unused, line 1 starts at byte 0
        for (i, byte) in input.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(i + 1);
            }
        }
        Self {
            starts,
            len: input.len(),
        }
    }

    fn location_to_offset(&self, input: &str, location: Location) -> Option<usize> {
        if location.line == 0 || location.column == 0 {
            return None;
        }
        let line = usize::try_from(location.line).ok()?;
        let col = usize::try_from(location.column).ok()?;
        let &line_start = self.starts.get(line)?;

        // Walk characters from the line start to reach the target column.
        let line_slice = &input[line_start..];
        let mut char_col = 1usize;
        for (byte_off, _ch) in line_slice.char_indices() {
            if char_col == col {
                return Some(line_start + byte_off);
            }
            char_col += 1;
        }
        // Column just past the last character in the line (or file).
        if char_col == col {
            Some((line_start + line_slice.len()).min(self.len))
        } else {
            None
        }
    }
}

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

    fn warn_empty_schema(&mut self) {
        self.diagnostics.push(Diagnostic::warning(
            codes::parse_empty_schema(),
            "No schema objects were produced from the input. Check whether the SQL only contains comments, whitespace, or unsupported statements.",
        ));
    }

    fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

struct ParsedColumn {
    name: String,
    data_type: String,
    nullable: bool,
    is_primary_key: bool,
}

impl ParsedColumn {
    fn into_column(self, id: ColumnId) -> Column {
        Column {
            id,
            name: self.name,
            data_type: self.data_type,
            nullable: self.nullable,
            is_primary_key: self.is_primary_key,
            comment: None,
        }
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
/// Uses token-based heuristics so comments and string literals do not skew the
/// result. Falls back to `PostgreSQL` if no dialect-specific markers are found.
#[must_use]
pub fn detect_dialect(input: &str) -> SqlDialect {
    match Tokenizer::new(&GenericDialect {}, input).tokenize() {
        Ok(tokens) => detect_dialect_from_tokens(&tokens),
        Err(_) => detect_dialect_from_source(input),
    }
}

fn detect_dialect_from_tokens(tokens: &[Token]) -> SqlDialect {
    let significant_tokens = significant_tokens(tokens);

    let mysql_score = score_dialect_signals(&[
        (
            significant_tokens
                .iter()
                .any(|token| is_backtick_identifier(token)),
            2,
        ),
        (contains_word(&significant_tokens, "AUTO_INCREMENT"), 4),
        (contains_word(&significant_tokens, "UNSIGNED"), 3),
        (
            contains_word_sequence(&significant_tokens, &["DEFAULT", "CHARSET"])
                || contains_word_sequence(&significant_tokens, &["CHARACTER", "SET"]),
            3,
        ),
        (contains_word(&significant_tokens, "COLLATE"), 2),
        (contains_word(&significant_tokens, "FULLTEXT"), 2),
        (
            contains_word_sequence(&significant_tokens, &["ON", "UPDATE", "CURRENT_TIMESTAMP"]),
            3,
        ),
    ]);

    let sqlite_score = score_dialect_signals(&[
        (contains_word(&significant_tokens, "AUTOINCREMENT"), 4),
        (
            contains_word_sequence(&significant_tokens, &["WITHOUT", "ROWID"]),
            4,
        ),
        (contains_word(&significant_tokens, "PRAGMA"), 4),
        (
            contains_word_sequence(&significant_tokens, &["INTEGER", "PRIMARY", "KEY"])
                && !contains_word(&significant_tokens, "AUTO_INCREMENT"),
            3,
        ),
        (contains_word(&significant_tokens, "STRICT"), 2),
    ]);

    let pg_score = score_dialect_signals(&[
        (
            contains_word_sequence(&significant_tokens, &["CREATE", "TYPE"])
                && contains_word_sequence(&significant_tokens, &["AS", "ENUM"]),
            4,
        ),
        (
            contains_word(&significant_tokens, "SERIAL")
                || contains_word(&significant_tokens, "BIGSERIAL"),
            3,
        ),
        (
            contains_word_sequence(&significant_tokens, &["COMMENT", "ON"]),
            4,
        ),
        (
            contains_word_sequence(&significant_tokens, &["CREATE", "EXTENSION"]),
            4,
        ),
        (
            contains_word_sequence(&significant_tokens, &["CREATE", "SEQUENCE"]),
            4,
        ),
        (
            significant_tokens
                .iter()
                .any(|token| matches!(token, Token::DoubleColon)),
            3,
        ),
        (contains_word(&significant_tokens, "RETURNING"), 2),
        (contains_word(&significant_tokens, "ILIKE"), 2),
    ]);

    if mysql_score > sqlite_score && mysql_score > pg_score {
        SqlDialect::Mysql
    } else if sqlite_score > mysql_score && sqlite_score > pg_score {
        SqlDialect::Sqlite
    } else {
        SqlDialect::Postgres
    }
}

fn detect_dialect_from_source(input: &str) -> SqlDialect {
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

fn significant_tokens(tokens: &[Token]) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|token| !matches!(token, Token::Whitespace(_)))
        .collect()
}

fn contains_word(tokens: &[&Token], expected: &str) -> bool {
    tokens.iter().copied().any(|token| is_word(token, expected))
}

fn contains_word_sequence(tokens: &[&Token], sequence: &[&str]) -> bool {
    if sequence.is_empty() || tokens.len() < sequence.len() {
        return false;
    }

    tokens.windows(sequence.len()).any(|window| {
        window
            .iter()
            .zip(sequence)
            .all(|(token, expected)| is_word(token, expected))
    })
}

fn is_word(token: &Token, expected: &str) -> bool {
    matches!(token, Token::Word(word) if word.value.eq_ignore_ascii_case(expected))
}

fn is_backtick_identifier(token: &Token) -> bool {
    matches!(token, Token::Word(word) if word.quote_style == Some('`'))
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

fn source_span_from_sql_span(
    input: &str,
    offsets: &LineOffsets,
    span: SqlSpan,
) -> Option<SourceSpan> {
    let start = offsets.location_to_offset(input, span.start)?;
    let end = offsets.location_to_offset(input, span.end)?;
    debug_assert!(
        end >= start,
        "sql span end must not precede start: {span:?}"
    );
    if end < start {
        return None;
    }

    let length = (end - start).max(1);
    Some(SourceSpan::new(start, length))
}

fn span_from_spanned<T: Spanned>(
    input: &str,
    offsets: &LineOffsets,
    value: &T,
) -> Option<SourceSpan> {
    let span = value.span();
    if span == SqlSpan::empty() {
        None
    } else {
        source_span_from_sql_span(input, offsets, span)
    }
}

fn span_from_ident(
    input: &str,
    offsets: &LineOffsets,
    ident: &sqlparser::ast::Ident,
) -> Option<SourceSpan> {
    source_span_from_sql_span(input, offsets, ident.span)
}

fn normalized_stable_id(schema_name: Option<&str>, name: &str) -> String {
    match schema_name {
        Some(schema_name) => format!(
            "{}.{}",
            normalize_identifier(schema_name),
            normalize_identifier(name)
        ),
        None => normalize_identifier(name),
    }
}

const MAX_UNSUPPORTED_DEBUG_LEN: usize = 80;
const MAX_UNSUPPORTED_DEBUG_PREFIX_LEN: usize = MAX_UNSUPPORTED_DEBUG_LEN - 3;

fn truncate_unsupported_debug(debug_str: &str) -> String {
    if debug_str.len() <= MAX_UNSUPPORTED_DEBUG_LEN {
        return debug_str.to_owned();
    }

    let boundary = debug_str.floor_char_boundary(MAX_UNSUPPORTED_DEBUG_PREFIX_LEN);
    format!("{}...", &debug_str[..boundary])
}

fn parse_mysql_enum_like_value(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, MySqlEnumLikeParseError> {
    if chars.next() != Some('\'') {
        return Err(MySqlEnumLikeParseError::ExpectedQuotedValue);
    }

    let mut value = String::new();
    loop {
        match chars.next() {
            Some('\'') => {
                if chars.peek() == Some(&'\'') {
                    value.push('\'');
                    chars.next();
                } else {
                    break;
                }
            }
            Some('\\') => {
                let Some(escaped) = chars.next() else {
                    return Err(MySqlEnumLikeParseError::TrailingEscapeSequence);
                };
                match escaped {
                    '\\' | '\'' => value.push(escaped),
                    other => {
                        value.push('\\');
                        value.push(other);
                    }
                }
            }
            Some(c) => value.push(c),
            None => return Err(MySqlEnumLikeParseError::UnterminatedQuotedValue),
        }
    }

    Ok(value)
}

#[derive(Debug, Error, PartialEq, Eq)]
enum MySqlEnumLikeParseError {
    #[error("expected a quoted enum/set value")]
    ExpectedQuotedValue,
    #[error("enum/set value ended with an incomplete escape sequence")]
    TrailingEscapeSequence,
    #[error("enum/set value is missing a closing quote")]
    UnterminatedQuotedValue,
    #[error("enum/set definition is missing a closing parenthesis")]
    MissingClosingParenthesis,
    #[error("enum/set definition contains an unexpected separator")]
    UnexpectedSeparator,
}

fn parse_mysql_enum_like_type(
    data_type: &str,
) -> Result<Option<(String, Vec<String>)>, MySqlEnumLikeParseError> {
    let Some(start) = data_type.find('(') else {
        return Ok(None);
    };
    let Some(end) = data_type.rfind(')') else {
        return Err(MySqlEnumLikeParseError::MissingClosingParenthesis);
    };
    let kind = data_type[..start].trim();
    if !kind.eq_ignore_ascii_case("enum") && !kind.eq_ignore_ascii_case("set") {
        return Ok(None);
    }
    if start.saturating_add(1) > end {
        return Ok(None);
    }

    let mut values = Vec::new();
    let mut chars = data_type[start + 1..end].chars().peekable();

    while chars.peek().is_some() {
        while chars.peek().is_some_and(char::is_ascii_whitespace) {
            chars.next();
        }

        values.push(parse_mysql_enum_like_value(&mut chars)?);

        while chars.peek().is_some_and(char::is_ascii_whitespace) {
            chars.next();
        }

        match chars.peek() {
            Some(',') => {
                chars.next();
            }
            None => break,
            _ => return Err(MySqlEnumLikeParseError::UnexpectedSeparator),
        }
    }

    Ok(Some((kind.to_ascii_lowercase(), values)))
}

fn serialize_mysql_enum_like_type(kind: &str, values: &[String]) -> String {
    let serialized_values = values
        .iter()
        .map(|value| {
            let escaped = value.replace('\\', "\\\\").replace('\'', "''");
            format!("'{escaped}'")
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{kind}({serialized_values})")
}

fn canonicalize_mysql_enum_like_type(
    data_type: &str,
) -> Result<Option<String>, MySqlEnumLikeParseError> {
    Ok(parse_mysql_enum_like_type(data_type)?
        .map(|(kind, values)| serialize_mysql_enum_like_type(&kind, &values)))
}

fn infer_mysql_enums(ctx: &mut ParseContext, tables: &[Table]) -> Vec<Enum> {
    let mut enums = Vec::new();
    let mut seen = HashSet::new();

    for table in tables {
        for column in &table.columns {
            let parsed = match parse_mysql_enum_like_type(&column.data_type) {
                Ok(Some(parsed)) => parsed,
                Ok(None) => continue,
                Err(error) => {
                    ctx.diagnostics.push(Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!(
                            "Malformed MySQL enum/set definition on {}.{}: {} ({error})",
                            table.qualified_name(),
                            column.name,
                            column.data_type
                        ),
                    ));
                    continue;
                }
            };
            let (kind, values) = parsed;
            let enum_name = serialize_mysql_enum_like_type(&kind, &values);
            let key = format!(
                "{}:{}",
                table.schema_name.as_deref().unwrap_or_default(),
                enum_name
            );
            if !seen.insert(key) {
                continue;
            }

            enums.push(Enum {
                id: normalized_stable_id(table.schema_name.as_deref(), &enum_name),
                schema_name: table.schema_name.clone(),
                name: enum_name,
                values,
            });
        }
    }

    enums
}

fn split_object_name_parts(name: &ObjectName) -> Vec<String> {
    name.0.iter().map(object_name_part_to_string).collect()
}

fn warn_truncated_object_name(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    max_parts: usize,
    context: &str,
) {
    let parts = split_object_name_parts(name);
    if parts.len() <= max_parts {
        return;
    }

    let ignored = parts[..parts.len() - max_parts].join(".");
    let retained = parts[parts.len() - max_parts..].join(".");
    ctx.diagnostics.push(
        Diagnostic::warning(
            codes::parse_unsupported(),
            format!(
                "{context}: object name `{name}` has more than {max_parts} parts; ignoring leading qualifier(s) `{ignored}` and using `{retained}`"
            ),
        )
        .with_span_opt(span_from_spanned(input, offsets,name)),
    );
}

fn split_object_name_with_diagnostics(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    context: &str,
) -> (Option<String>, String) {
    warn_truncated_object_name(ctx, input, offsets, name, 2, context);
    split_object_name(name)
}

fn normalized_stable_id_for_object_name_with_diagnostics(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    context: &str,
) -> String {
    let (schema_name, object_name) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, context);
    normalized_stable_id(schema_name.as_deref(), &object_name)
}

fn foreign_key_target(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    target: &ObjectName,
    context: &str,
) -> (Option<String>, String) {
    split_object_name_with_diagnostics(ctx, input, offsets, target, context)
}

#[allow(clippy::too_many_arguments)]
fn build_foreign_key(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    constraint_name: Option<&sqlparser::ast::Ident>,
    from_columns: Vec<String>,
    foreign_table: &ObjectName,
    referred_columns: &[sqlparser::ast::Ident],
    on_delete: Option<sqlparser::ast::ReferentialAction>,
    on_update: Option<sqlparser::ast::ReferentialAction>,
    context: &str,
) -> ForeignKey {
    let (to_schema, to_table) = foreign_key_target(ctx, input, offsets, foreign_table, context);
    let to_columns = referred_columns
        .iter()
        .map(|column| normalize_identifier(&column.value))
        .collect();

    ForeignKey {
        name: constraint_name.map(|ident| normalize_identifier(&ident.value)),
        from_columns,
        to_schema: to_schema.map(|schema| normalize_identifier(&schema)),
        to_table: normalize_identifier(&to_table),
        to_columns,
        on_delete: convert_referential_action(on_delete),
        on_update: convert_referential_action(on_update),
    }
}

/// Parse SQL statements with error recovery.
///
/// Instead of using `Parser::parse_sql` which aborts on the first error,
/// this function parses statement-by-statement and skips to the next
/// semicolon on error, allowing subsequent statements to be parsed.
fn parse_statements_with_recovery(
    dialect: &dyn Dialect,
    input: &str,
    offsets: &LineOffsets,
    ctx: &mut ParseContext,
) -> Vec<Statement> {
    // First, try the fast path: parse all at once
    let mut parser = match Parser::new(dialect).try_with_sql(input) {
        Ok(p) => p,
        Err(e) => {
            // Tokenizer error — nothing can be parsed
            ctx.diagnostics.push(
                Diagnostic::error(codes::parse_error(), format!("SQL parse error: {e}"))
                    .with_span(SourceSpan::new(0, input.len().min(100))),
            );
            return Vec::new();
        }
    };

    let mut statements = Vec::new();

    loop {
        // Skip empty statements (consecutive semicolons)
        while parser.consume_token(&Token::SemiColon) {}

        if parser.peek_token().token == Token::EOF {
            break;
        }

        match parser.parse_statement() {
            Ok(stmt) => {
                statements.push(stmt);
            }
            Err(e) => {
                let error_msg = format!("SQL parse error: {e}");
                // Try to extract location from the error token's current position
                let span = {
                    let tok = parser.peek_token();
                    let sql_span = tok.span;
                    source_span_from_sql_span(input, offsets, sql_span)
                        .unwrap_or_else(|| SourceSpan::new(0, input.len().min(100)))
                };
                ctx.diagnostics
                    .push(Diagnostic::error(codes::parse_error(), error_msg).with_span(span));

                // Skip tokens until the next semicolon or EOF for recovery
                loop {
                    let tok = parser.next_token();
                    if matches!(tok.token, Token::SemiColon | Token::EOF) {
                        break;
                    }
                }
            }
        }
    }

    statements
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
        let mut seen_enum_ids: HashSet<String> =
            enums.iter().map(|enum_| enum_.id.clone()).collect();
        for enum_ in infer_mysql_enums(&mut ctx, &tables) {
            if seen_enum_ids.insert(enum_.id.clone()) {
                enums.push(enum_);
            }
        }
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

/// Parse a CREATE TABLE statement into a Table.
#[allow(clippy::too_many_lines)]
#[allow(clippy::unnecessary_wraps)]
fn parse_create_table(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    create: &sqlparser::ast::CreateTable,
) -> Option<Table> {
    let (schema_name, name) =
        split_object_name_with_diagnostics(ctx, input, offsets, &create.name, "CREATE TABLE");
    let stable_id = normalized_stable_id(schema_name.as_deref(), &name);

    let table_id = ctx.next_table_id();

    // Parse columns
    let mut columns = Vec::new();
    let mut next_column_id: u64 = 1;

    for column in &create.columns {
        let parsed_column = parsed_column_from_column_def(column);
        columns.push(parsed_column.into_column(ColumnId(next_column_id)));
        next_column_id += 1;
    }

    // Parse inline foreign key constraints from columns
    let mut foreign_keys = Vec::new();
    for column in &create.columns {
        for option in &column.options {
            if let ColumnOption::ForeignKey(constraint) = &option.option {
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
    input: &str,
    offsets: &LineOffsets,
    create_index: &CreateIndex,
    tables: &mut [Table],
    table_map: &std::collections::HashMap<String, usize>,
) {
    // Get the table name
    let stable_id = normalized_stable_id_for_object_name_with_diagnostics(
        ctx,
        input,
        offsets,
        &create_index.table_name,
        "CREATE INDEX",
    );

    // Find the table
    let Some(&table_idx) = table_map.get(&stable_id) else {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_table(),
                format!("CREATE INDEX references unknown table: {stable_id}"),
            )
            .with_span_opt(span_from_spanned(input, offsets, create_index)),
        );
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
            // ObjectName is a wrapper around Vec<ObjectNamePart>.
            // Use the last part as the actual index name (earlier parts are schema qualifiers).
            ident
                .0
                .last()
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
#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_arguments)]
fn parse_comment(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
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
            let stable_id = normalized_stable_id_for_object_name_with_diagnostics(
                ctx,
                input,
                offsets,
                object_name,
                "COMMENT ON TABLE",
            );

            if let Some(&table_idx) = table_map.get(&stable_id) {
                tables[table_idx].comment = Some(comment_text);
            } else {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_table(),
                        format!("COMMENT ON TABLE references unknown table: {stable_id}"),
                    )
                    .with_span_opt(span_from_spanned(
                        input,
                        offsets,
                        object_name,
                    )),
                );
            }
        }
        sqlparser::ast::CommentObject::Column => {
            // For columns, object_name is typically "table.column" or "schema.table.column"
            let parts = split_object_name_parts(object_name);

            // Extract column name (last part) and table name (remaining parts)
            if parts.len() < 2 {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::parse_unsupported(),
                        "Invalid COMMENT ON COLUMN syntax: expected table.column".to_string(),
                    )
                    .with_span_opt(span_from_spanned(
                        input,
                        offsets,
                        object_name,
                    )),
                );
                return;
            }

            warn_truncated_object_name(ctx, input, offsets, object_name, 3, "COMMENT ON COLUMN");

            let column_name = normalize_identifier(&parts[parts.len() - 1]);
            let table_parts = &parts[..parts.len() - 1];

            let stable_id = match table_parts {
                [table] => normalize_identifier(table),
                [schema, table] | [.., schema, table] => normalized_stable_id(Some(schema), table),
                _ => unreachable!("COMMENT ON COLUMN must have at least two parts"),
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
                    )
                    .with_span_opt(span_from_spanned(input, offsets,object_name)));
                }
            } else {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_table(),
                        format!("COMMENT ON COLUMN references unknown table: {stable_id}"),
                    )
                    .with_span_opt(span_from_spanned(
                        input,
                        offsets,
                        object_name,
                    )),
                );
            }
        }
        _ => {
            // Other comment types (view, function, etc.) are not supported
            ctx.warn_unsupported(
                &format!("COMMENT ON {object_type:?}"),
                span_from_spanned(input, offsets, object_name),
            );
        }
    }
}

/// Build `stable_id` the same way as [`parse_create_table`] so `ALTER TABLE` resolves targets.
fn stable_id_for_alter_target(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table_name: &ObjectName,
) -> String {
    normalized_stable_id_for_object_name_with_diagnostics(
        ctx,
        input,
        offsets,
        table_name,
        "ALTER TABLE",
    )
}

fn table_name_matches_reference(table: &Table, target_table: &str) -> bool {
    table.name.eq_ignore_ascii_case(target_table)
        || table.stable_id.eq_ignore_ascii_case(target_table)
}

fn table_schema_matches(table: &Table, target_schema: Option<&str>) -> bool {
    match target_schema {
        Some(target_schema) => table
            .schema_name
            .as_deref()
            .is_some_and(|schema_name| schema_name.eq_ignore_ascii_case(target_schema)),
        None => table.schema_name.is_none(),
    }
}

fn single_matching_table_index(
    tables: &[Table],
    target_table: &str,
    target_schema: Option<&str>,
) -> Option<usize> {
    let mut found = None;
    for (table_idx, table) in tables.iter().enumerate() {
        if !table_schema_matches(table, target_schema)
            || !table_name_matches_reference(table, target_table)
        {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(table_idx);
    }
    found
}

fn single_matching_table_index_any_schema(tables: &[Table], target_table: &str) -> Option<usize> {
    let mut found = None;
    for (table_idx, table) in tables.iter().enumerate() {
        if !table_name_matches_reference(table, target_table) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(table_idx);
    }
    found
}

fn foreign_key_resolves_to_table(
    tables: &[Table],
    source_idx: usize,
    fk: &ForeignKey,
    target_idx: usize,
) -> bool {
    if let Some(target_schema) = fk.to_schema.as_deref() {
        return single_matching_table_index(tables, &fk.to_table, Some(target_schema))
            == Some(target_idx);
    }

    if let Some(source_schema) = tables[source_idx].schema_name.as_deref()
        && let Some(match_idx) =
            single_matching_table_index(tables, &fk.to_table, Some(source_schema))
    {
        return match_idx == target_idx;
    }

    single_matching_table_index(tables, &fk.to_table, None)
        .or_else(|| single_matching_table_index_any_schema(tables, &fk.to_table))
        == Some(target_idx)
}

fn foreign_keys_referencing_table(tables: &[Table], target_idx: usize) -> Vec<(usize, usize)> {
    let mut references = Vec::new();
    for (table_idx, table) in tables.iter().enumerate() {
        for (fk_idx, fk) in table.foreign_keys.iter().enumerate() {
            if foreign_key_resolves_to_table(tables, table_idx, fk, target_idx) {
                references.push((table_idx, fk_idx));
            }
        }
    }
    references
}

#[allow(clippy::too_many_lines)]
fn apply_alter_table_operations(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    tables: &mut [Table],
    table_map: &mut HashMap<String, usize>,
    table_name: &ObjectName,
    operations: &[AlterTableOperation],
) {
    let stable_id = stable_id_for_alter_target(ctx, input, offsets, table_name);
    let Some(&idx) = table_map.get(&stable_id) else {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_table(),
                format!("ALTER TABLE references unknown table: {stable_id}"),
            )
            .with_span_opt(span_from_spanned(input, offsets, table_name)),
        );
        return;
    };

    for op in operations {
        apply_single_alter_operation(ctx, input, offsets, tables, table_map, idx, op);
    }
}

#[allow(clippy::too_many_lines)]
fn apply_single_alter_operation(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
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
            add_column_from_alter(
                ctx,
                input,
                offsets,
                &mut tables[idx],
                column_def,
                *if_not_exists,
            );
        }
        AlterTableOperation::DropColumn {
            column_names,
            if_exists,
            ..
        } => {
            let stable = tables[idx].stable_id.clone();
            for ident in column_names {
                let col_name = normalize_identifier(&ident.value);
                let pos = tables[idx].columns.iter().position(|c| c.name == col_name);
                if let Some(p) = pos {
                    let incoming_fks_to_remove: HashSet<(usize, usize)> =
                        foreign_keys_referencing_table(tables, idx)
                            .into_iter()
                            .filter(|(table_idx, fk_idx)| {
                                tables[*table_idx].foreign_keys[*fk_idx]
                                    .to_columns
                                    .contains(&col_name)
                            })
                            .collect();

                    let table = &mut tables[idx];
                    table.columns.remove(p);
                    table.indexes.retain(|ix| !ix.columns.contains(&col_name));

                    for (table_idx, table) in tables.iter_mut().enumerate() {
                        let mut fk_idx = 0usize;
                        table.foreign_keys.retain(|fk| {
                            let remove = (table_idx == idx && fk.from_columns.contains(&col_name))
                                || incoming_fks_to_remove.contains(&(table_idx, fk_idx));
                            fk_idx += 1;
                            !remove
                        });
                    }
                } else if !if_exists {
                    ctx.diagnostics.push(
                        Diagnostic::warning(
                            codes::schema_unknown_column(),
                            format!(
                                "ALTER TABLE DROP COLUMN: unknown column `{col_name}` on `{stable}`"
                            ),
                        )
                        .with_span_opt(span_from_ident(input, offsets, ident)),
                    );
                }
            }
        }
        AlterTableOperation::AddConstraint { constraint, .. } => {
            apply_add_table_constraint(ctx, input, offsets, &mut tables[idx], constraint);
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
                )
                .with_span_opt(span_from_ident(input, offsets,name)));
            }
        }
        AlterTableOperation::RenameColumn {
            old_column_name,
            new_column_name,
        } => {
            let old = normalize_identifier(&old_column_name.value);
            let new = normalize_identifier(&new_column_name.value);
            let stable = tables[idx].stable_id.clone();
            let found = tables[idx].columns.iter().any(|c| c.name == old);
            if found {
                let referencing_fks = foreign_keys_referencing_table(tables, idx);
                // Update the column name, from_columns in local FKs, and index columns.
                let table = &mut tables[idx];
                if let Some(col) = table.columns.iter_mut().find(|c| c.name == old) {
                    col.name.clone_from(&new);
                }
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

                for (table_idx, fk_idx) in referencing_fks {
                    if let Some(fk) = tables
                        .get_mut(table_idx)
                        .and_then(|table| table.foreign_keys.get_mut(fk_idx))
                    {
                        for c in &mut fk.to_columns {
                            if *c == old {
                                c.clone_from(&new);
                            }
                        }
                    }
                }
            } else {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_column(),
                        format!("ALTER TABLE RENAME COLUMN: unknown `{old}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(
                        input,
                        offsets,
                        old_column_name,
                    )),
                );
            }
        }
        AlterTableOperation::RenameTable {
            table_name: new_table,
        } => {
            let referencing_fks = foreign_keys_referencing_table(tables, idx);
            let old_stable = tables[idx].stable_id.clone();
            let old_schema = tables[idx].schema_name.clone();
            let renamed_target = match new_table {
                sqlparser::ast::RenameTableNameKind::As(name)
                | sqlparser::ast::RenameTableNameKind::To(name) => name,
            };
            let (new_schema_raw, new_name_raw) = split_object_name_with_diagnostics(
                ctx,
                input,
                offsets,
                renamed_target,
                "ALTER TABLE RENAME TO",
            );
            let new_schema = new_schema_raw
                .map(|schema_name| normalize_identifier(&schema_name))
                .or_else(|| old_schema.clone());
            let new_name = normalize_identifier(&new_name_raw);
            let renamed_stable_id = normalized_stable_id(new_schema.as_deref(), &new_name);
            let table = &mut tables[idx];
            table.schema_name.clone_from(&new_schema);
            table.name.clone_from(&new_name);
            table.stable_id.clone_from(&renamed_stable_id);
            table_map.remove(&old_stable);
            table_map.insert(renamed_stable_id.clone(), idx);
            ctx.seen_tables.remove(&old_stable);
            ctx.seen_tables.insert(renamed_stable_id.clone());

            for (table_idx, fk_idx) in referencing_fks {
                if let Some(fk) = tables
                    .get_mut(table_idx)
                    .and_then(|table| table.foreign_keys.get_mut(fk_idx))
                {
                    fk.to_table.clone_from(&new_name);
                    if fk.to_schema.is_some() || old_schema != new_schema {
                        fk.to_schema.clone_from(&new_schema);
                    }
                }
            }
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
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!("ALTER TABLE DROP FOREIGN KEY: no FK named `{sym}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(input, offsets, name)),
                );
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
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!("ALTER TABLE DROP INDEX: no index named `{n}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(input, offsets, name)),
                );
            }
        }
        other => {
            ctx.warn_unsupported(
                &format!("ALTER TABLE operation (unsupported): {other:?}"),
                span_from_spanned(input, offsets, op),
            );
        }
    }
}

fn parsed_column_from_column_def(column: &sqlparser::ast::ColumnDef) -> ParsedColumn {
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
    let raw_data_type = column.data_type.to_string();
    let data_type = canonicalize_mysql_enum_like_type(&raw_data_type)
        .ok()
        .flatten()
        .unwrap_or(raw_data_type);
    ParsedColumn {
        name: column_name,
        data_type,
        nullable,
        is_primary_key,
    }
}

fn add_column_from_alter(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table: &mut Table,
    column_def: &sqlparser::ast::ColumnDef,
    if_not_exists: bool,
) {
    let col_name = normalize_identifier(&column_def.name.value);
    if table.columns.iter().any(|c| c.name == col_name) {
        if !if_not_exists {
            ctx.diagnostics.push(
                Diagnostic::warning(
                    codes::parse_unsupported(),
                    format!(
                        "ALTER TABLE ADD COLUMN: duplicate column `{col_name}` on `{}`",
                        table.stable_id
                    ),
                )
                .with_span_opt(span_from_spanned(input, offsets, column_def)),
            );
        }
        return;
    }

    let next_id = table.columns.iter().map(|c| c.id.0).max().unwrap_or(0) + 1;
    let col = parsed_column_from_column_def(column_def).into_column(ColumnId(next_id));
    table.columns.push(col);

    for option in &column_def.options {
        if let ColumnOption::ForeignKey(constraint) = &option.option {
            let from_column = col_name.clone();
            table.foreign_keys.push(build_foreign_key(
                ctx,
                input,
                offsets,
                option.name.as_ref(),
                vec![from_column],
                &constraint.foreign_table,
                &constraint.referred_columns,
                constraint.on_delete,
                constraint.on_update,
                "ALTER TABLE ADD COLUMN inline FOREIGN KEY",
            ));
        }
    }
}

fn apply_add_table_constraint(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
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
            table.foreign_keys.push(build_foreign_key(
                ctx,
                input,
                offsets,
                foreign_key.name.as_ref(),
                from_cols,
                &foreign_key.foreign_table,
                &foreign_key.referred_columns,
                foreign_key.on_delete,
                foreign_key.on_update,
                "ALTER TABLE ADD CONSTRAINT FOREIGN KEY",
            ));
        }
        TableConstraint::Check(_) | TableConstraint::Index(_) => {}
        TableConstraint::FulltextOrSpatial(_) => {
            ctx.warn_unsupported(
                "FULLTEXT/SPATIAL constraint",
                span_from_spanned(input, offsets, constraint),
            );
        }
    }
}

/// Parse a CREATE TYPE ... AS ENUM statement into an Enum.
fn parse_create_type_enum(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    labels: &[sqlparser::ast::Ident],
) -> Enum {
    let (schema_name, type_name) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "CREATE TYPE");

    // Generate a stable ID for the enum
    let id = normalized_stable_id(schema_name.as_deref(), &type_name);

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
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    view_columns: &[sqlparser::ast::ViewColumnDef],
    query: &sqlparser::ast::Query,
) -> Option<View> {
    let (schema_name, view_name) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "CREATE VIEW");

    // Generate a stable ID for the view
    let id = normalized_stable_id(schema_name.as_deref(), &view_name);

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
                id: ColumnId((i as u64) + 1),
                name: normalize_identifier(&def.name.value),
                data_type,
                nullable: true,
                is_primary_key: false,
                comment: None,
            }
        })
        .collect()
}

/// Extract column names from the top-level `SELECT` items in a view query.
///
/// Complex queries such as nested subqueries, set operations, or wildcard-only
/// projections may not yield derived column names here unless the view declares
/// them explicitly in `CREATE VIEW ... (col1, col2)`.
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
                id: ColumnId((i as u64) + 1),
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
    fn parses_target_schema_for_create_table_foreign_keys() {
        let sql = r"
        CREATE TABLE auth.accounts (
          id BIGINT PRIMARY KEY
        );

        CREATE TABLE auth.orgs (
          id BIGINT PRIMARY KEY
        );

        CREATE TABLE public.users (
          id BIGINT PRIMARY KEY,
          account_id BIGINT REFERENCES auth.accounts(id),
          org_id BIGINT,
          CONSTRAINT fk_users_org FOREIGN KEY (org_id) REFERENCES auth.orgs(id)
        );
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema
            .tables
            .iter()
            .find(|table| table.stable_id == "public.users")
            .expect("users table should exist");

        assert_eq!(users.foreign_keys.len(), 2);
        assert_eq!(users.foreign_keys[0].to_schema.as_deref(), Some("auth"));
        assert_eq!(users.foreign_keys[0].to_table, "accounts");
        assert_eq!(users.foreign_keys[1].to_schema.as_deref(), Some("auth"));
        assert_eq!(users.foreign_keys[1].to_table, "orgs");
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
    fn warns_when_object_names_have_more_than_two_parts() {
        let sql = r"
        CREATE TABLE db.public.users (
          id BIGINT PRIMARY KEY
        );
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        let schema = output.schema.expect("schema should exist");

        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].stable_id, "public.users");
        let warning = output
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code == codes::parse_unsupported()
                    && diagnostic.message.contains("db.public.users")
            })
            .expect("warning should exist");
        assert!(warning.message.contains("ignoring leading qualifier"));
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
    fn normalizes_stable_ids_for_lookups() {
        let sql = r"
        CREATE TABLE Public.Users (
          id BIGINT PRIMARY KEY
        );

        CREATE INDEX idx_users_id ON public.users (id);
        COMMENT ON TABLE PUBLIC.USERS IS 'User accounts';
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];

        assert_eq!(table.stable_id, "public.users");
        assert_eq!(table.comment, Some("User accounts".to_string()));
        assert_eq!(table.indexes.len(), 1);
        assert_eq!(table.indexes[0].name, Some("idx_users_id".to_string()));
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
    fn truncates_unsupported_debug_output_on_utf8_boundaries() {
        let debug = "絵文字🙂".repeat(20);
        let truncated = truncate_unsupported_debug(&debug);

        assert!(truncated.ends_with("..."));
        assert!(truncated.is_char_boundary(truncated.len() - 3));
        assert!(truncated.len() <= MAX_UNSUPPORTED_DEBUG_LEN);
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
    fn parses_target_schema_for_alter_table_foreign_keys() {
        let sql = r"
        CREATE TABLE auth.orgs (id BIGINT PRIMARY KEY);
        CREATE TABLE auth.accounts (id BIGINT PRIMARY KEY);
        CREATE TABLE public.users (
          id BIGINT PRIMARY KEY,
          org_id BIGINT
        );

        ALTER TABLE public.users ADD COLUMN account_id BIGINT REFERENCES auth.accounts(id);
        ALTER TABLE public.users ADD CONSTRAINT fk_users_org
          FOREIGN KEY (org_id) REFERENCES auth.orgs(id);
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema
            .tables
            .iter()
            .find(|table| table.stable_id == "public.users")
            .expect("users table should exist");

        assert_eq!(users.foreign_keys.len(), 2);
        assert_eq!(users.foreign_keys[0].to_schema.as_deref(), Some("auth"));
        assert_eq!(users.foreign_keys[0].to_table, "accounts");
        assert_eq!(users.foreign_keys[1].name.as_deref(), Some("fk_users_org"));
        assert_eq!(users.foreign_keys[1].to_schema.as_deref(), Some("auth"));
        assert_eq!(users.foreign_keys[1].to_table, "orgs");
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
        assert!(
            output
                .diagnostics
                .iter()
                .find(|d| d.code == codes::schema_duplicate_table())
                .and_then(|d| d.span)
                .is_some()
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
    fn parse_error_wraps_sqlparser_errors_as_strings() {
        let parser_error = Parser::new(&PostgreSqlDialect {})
            .try_with_sql("CREATE TABLE")
            .expect("tokenization should succeed")
            .parse_statement()
            .expect_err("statement should fail");

        let error = ParseError::from(parser_error);

        match error {
            ParseError::Sql(message) => {
                assert!(!message.is_empty());
                assert!(message.contains("sql parser error"));
            }
            ParseError::Schema(message) => panic!("unexpected schema error: {message}"),
        }
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
    fn warns_when_input_produces_empty_schema() {
        let output = parse_sql_to_schema_with_diagnostics("  -- comments only\n");

        assert!(output.schema.is_some());
        assert!(output.has_warnings());
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == codes::parse_empty_schema()
                && diagnostic
                    .message
                    .contains("No schema objects were produced")
        }));
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
    fn generates_column_ids_for_alter_table_add_column() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY
        );

        ALTER TABLE users ADD COLUMN name TEXT NOT NULL;
        ALTER TABLE users ADD COLUMN email TEXT;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];

        assert_eq!(table.columns[0].id, ColumnId(1));
        assert_eq!(table.columns[1].id, ColumnId(2));
        assert_eq!(table.columns[2].id, ColumnId(3));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "sql span end must not precede start")]
    fn rejects_reversed_sql_spans_in_debug_builds() {
        let span = SqlSpan::new(Location::new(1, 5), Location::new(1, 3));
        let offsets = LineOffsets::new("abcd");

        let _ = source_span_from_sql_span("abcd", &offsets, span);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn ignores_reversed_sql_spans_in_release_builds() {
        let span = SqlSpan::new(Location::new(1, 5), Location::new(1, 3));
        let offsets = LineOffsets::new("abcd");

        assert_eq!(source_span_from_sql_span("abcd", &offsets, span), None);
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
    fn unknown_table_warnings_include_spans() {
        let sql = r"
        CREATE INDEX idx_missing ON nonexistent_table (id);
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        let warning = output
            .diagnostics
            .iter()
            .find(|d| d.code == codes::schema_unknown_table())
            .expect("warning should exist");

        assert!(warning.span.is_some());
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

    #[test]
    fn test_detect_dialect_ignores_comment_markers() {
        let sql = r"
            -- ENGINE=InnoDB AUTO_INCREMENT
            /* PRAGMA foreign_keys = ON */
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
        ";

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
    fn test_parse_mysql_enum_and_set_types_into_schema_enums() {
        let sql = r"
            CREATE TABLE `users` (
                `id` BIGINT NOT NULL AUTO_INCREMENT,
                `status` ENUM('draft', 'published') NOT NULL,
                `flags` SET('featured', 'archived') NULL,
                PRIMARY KEY (`id`)
            ) ENGINE=InnoDB;
        ";
        let schema =
            parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");

        assert_eq!(schema.enums.len(), 2);
        assert_eq!(
            schema.tables[0].columns[1].data_type,
            "enum('draft','published')"
        );
        assert_eq!(
            schema.tables[0].columns[2].data_type,
            "set('featured','archived')"
        );

        let status_enum = schema
            .enums
            .iter()
            .find(|enum_| enum_.name == "enum('draft','published')")
            .expect("status enum should be inferred");
        assert_eq!(status_enum.values, vec!["draft", "published"]);

        let flags_enum = schema
            .enums
            .iter()
            .find(|enum_| enum_.name == "set('featured','archived')")
            .expect("flags set should be inferred");
        assert_eq!(flags_enum.values, vec!["featured", "archived"]);
    }

    #[test]
    fn test_parse_mysql_enum_like_type_rejects_reversed_parentheses() {
        assert_eq!(parse_mysql_enum_like_type(")enum("), Ok(None));
    }

    #[test]
    fn test_parse_mysql_enum_like_type_preserves_trailing_backslash() {
        assert_eq!(
            parse_mysql_enum_like_type("enum('back\\\\')"),
            Ok(Some(("enum".to_string(), vec!["back\\".to_string()])))
        );
    }

    #[test]
    fn test_parse_mysql_enum_like_type_preserves_unknown_backslash_sequences() {
        assert_eq!(
            parse_mysql_enum_like_type(r"enum('line\nbreak')"),
            Ok(Some(("enum".to_string(), vec![r"line\nbreak".to_string()])))
        );
    }

    #[test]
    fn test_parse_mysql_enum_like_type_rejects_incomplete_escape_sequences() {
        assert_eq!(
            parse_mysql_enum_like_type("enum('bad\\)"),
            Err(MySqlEnumLikeParseError::TrailingEscapeSequence)
        );
    }

    #[test]
    fn test_infer_mysql_enums_warns_on_malformed_definitions() {
        let mut ctx = ParseContext::new();
        ctx.dialect = SqlDialect::Mysql;

        let enums = infer_mysql_enums(
            &mut ctx,
            &[Table {
                id: TableId(1),
                stable_id: "users".to_string(),
                schema_name: None,
                name: "users".to_string(),
                columns: vec![Column {
                    id: ColumnId(1),
                    name: "status".to_string(),
                    data_type: "enum('bad\\')".to_string(),
                    nullable: false,
                    is_primary_key: false,
                    comment: None,
                }],
                foreign_keys: vec![],
                indexes: vec![],
                comment: None,
            }],
        );

        assert!(enums.is_empty());
        assert!(
            ctx.diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.severity == Severity::Warning })
        );
        assert!(ctx.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("Malformed MySQL enum/set definition")
        }));
    }

    #[test]
    fn test_parse_mysql_deduplicates_identical_enum_definitions_per_schema() {
        let sql = r"
            CREATE TABLE `users` (
                `id` BIGINT NOT NULL AUTO_INCREMENT,
                `status` ENUM('draft', 'published') NOT NULL,
                PRIMARY KEY (`id`)
            ) ENGINE=InnoDB;

            CREATE TABLE `posts` (
                `id` BIGINT NOT NULL AUTO_INCREMENT,
                `status` ENUM('draft', 'published') NOT NULL,
                PRIMARY KEY (`id`)
            ) ENGINE=InnoDB;
        ";
        let schema =
            parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");

        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.enums[0].name, "enum('draft','published')");
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

    // ========================================================================
    // ALTER TABLE operation tests
    // ========================================================================

    #[test]
    fn alter_table_drop_column_removes_column() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          name TEXT NOT NULL,
          email TEXT
        );
        ALTER TABLE users DROP COLUMN email;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];
        assert_eq!(table.columns.len(), 2);
        assert!(!table.columns.iter().any(|c| c.name == "email"));
    }

    #[test]
    fn alter_table_drop_column_cascades_to_fk_and_index() {
        let sql = r"
        CREATE TABLE orgs (id BIGINT PRIMARY KEY);
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          org_id BIGINT,
          CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
        );
        CREATE INDEX idx_org ON users (org_id);
        ALTER TABLE users DROP COLUMN org_id;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(!users.columns.iter().any(|c| c.name == "org_id"));
        assert!(
            users.foreign_keys.is_empty(),
            "FK referencing dropped column should be removed"
        );
        assert!(
            users.indexes.is_empty(),
            "index referencing dropped column should be removed"
        );
    }

    #[test]
    fn alter_table_drop_column_removes_incoming_fk_to_column() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        CREATE TABLE orders (
          id BIGINT PRIMARY KEY,
          user_id BIGINT,
          CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
        );
        ALTER TABLE users DROP COLUMN id;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
        assert!(
            orders.foreign_keys.is_empty(),
            "FK referencing dropped target column should be removed"
        );
    }

    #[test]
    fn alter_table_drop_unknown_column_warns() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users DROP COLUMN ghost;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        assert!(output.schema.is_some());
        assert!(output.has_warnings());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == codes::schema_unknown_column() && d.message.contains("ghost"))
        );
    }

    #[test]
    fn alter_table_rename_column() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          name TEXT NOT NULL
        );
        ALTER TABLE users RENAME COLUMN name TO full_name;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];
        assert!(!table.columns.iter().any(|c| c.name == "name"));
        assert!(table.columns.iter().any(|c| c.name == "full_name"));
    }

    #[test]
    fn alter_table_rename_column_updates_fk_and_index() {
        let sql = r"
        CREATE TABLE orgs (id BIGINT PRIMARY KEY);
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          org_id BIGINT,
          CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
        );
        CREATE INDEX idx_org ON users (org_id);
        ALTER TABLE users RENAME COLUMN org_id TO organization_id;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(users.columns.iter().any(|c| c.name == "organization_id"));
        assert!(
            users.foreign_keys[0]
                .from_columns
                .contains(&"organization_id".to_string()),
            "FK from_columns should be updated after rename"
        );
        assert!(
            users.indexes[0]
                .columns
                .contains(&"organization_id".to_string()),
            "index columns should be updated after rename"
        );
    }

    #[test]
    fn alter_table_rename_column_updates_referring_fk_to_columns() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        CREATE TABLE orders (
          id BIGINT PRIMARY KEY,
          user_id BIGINT,
          CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
        );
        ALTER TABLE users RENAME COLUMN id TO user_id;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
        assert_eq!(
            orders.foreign_keys[0].to_columns,
            vec!["user_id".to_string()],
            "FK to_columns should be updated when referenced column is renamed"
        );
    }

    #[test]
    fn alter_table_rename_column_updates_unqualified_same_schema_referring_fk() {
        let sql = r"
        CREATE TABLE public.users (id BIGINT PRIMARY KEY);
        CREATE TABLE public.orders (
          id BIGINT PRIMARY KEY,
          user_id BIGINT,
          CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
        );
        ALTER TABLE public.users RENAME COLUMN id TO user_id;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
        assert_eq!(
            orders.foreign_keys[0].to_columns,
            vec!["user_id".to_string()],
            "same-schema unqualified FK to_columns should be updated"
        );
    }

    #[test]
    fn alter_table_rename_column_updates_self_referencing_fk() {
        let sql = r"
        CREATE TABLE employees (
          id BIGINT PRIMARY KEY,
          manager_id BIGINT,
          CONSTRAINT fk_manager FOREIGN KEY (manager_id) REFERENCES employees(id)
        );
        ALTER TABLE employees RENAME COLUMN id TO employee_id;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let emp = &schema.tables[0];
        assert!(emp.columns.iter().any(|c| c.name == "employee_id"));
        assert_eq!(
            emp.foreign_keys[0].to_columns,
            vec!["employee_id".to_string()],
            "self-referencing FK to_columns should be updated"
        );
    }

    #[test]
    fn alter_table_rename_unknown_column_warns() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users RENAME COLUMN ghost TO phantom;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        assert!(output.schema.is_some());
        assert!(output.has_warnings());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == codes::schema_unknown_column() && d.message.contains("ghost"))
        );
    }

    #[test]
    fn alter_table_rename_table() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users RENAME TO accounts;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "accounts");
        assert_eq!(schema.tables[0].stable_id, "accounts");
    }

    #[test]
    fn alter_table_rename_table_preserves_schema_when_new_name_is_unqualified() {
        let sql = r"
        CREATE TABLE public.users (id BIGINT PRIMARY KEY);
        ALTER TABLE public.users RENAME TO accounts;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].schema_name.as_deref(), Some("public"));
        assert_eq!(schema.tables[0].name, "accounts");
        assert_eq!(schema.tables[0].stable_id, "public.accounts");
    }

    #[test]
    fn alter_table_rename_table_updates_referring_fk() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        CREATE TABLE orders (
          id BIGINT PRIMARY KEY,
          user_id BIGINT,
          CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
        );
        ALTER TABLE users RENAME TO accounts;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
        assert_eq!(
            orders.foreign_keys[0].to_table, "accounts",
            "FK to_table should be updated when referenced table is renamed"
        );
    }

    #[test]
    fn alter_table_rename_table_updates_unqualified_same_schema_referring_fk() {
        let sql = r"
        CREATE TABLE public.users (id BIGINT PRIMARY KEY);
        CREATE TABLE public.orders (
          id BIGINT PRIMARY KEY,
          user_id BIGINT,
          CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
        );
        ALTER TABLE public.users RENAME TO accounts;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
        assert_eq!(
            orders.foreign_keys[0].to_table, "accounts",
            "same-schema unqualified FK to_table should be updated"
        );
        assert_eq!(
            orders.foreign_keys[0].to_schema, None,
            "unqualified FK should remain unqualified when schema did not change"
        );
    }

    #[test]
    fn alter_table_rename_table_allows_reusing_old_name() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users RENAME TO accounts;
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table_names: std::collections::HashSet<&str> = schema
            .tables
            .iter()
            .map(|table| table.name.as_str())
            .collect();
        assert_eq!(schema.tables.len(), 2);
        assert!(table_names.contains("accounts"));
        assert!(table_names.contains("users"));
    }

    #[test]
    fn alter_table_drop_constraint_removes_fk() {
        let sql = r"
        CREATE TABLE orgs (id BIGINT PRIMARY KEY);
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          org_id BIGINT,
          CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
        );
        ALTER TABLE users DROP CONSTRAINT fk_org;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(
            users.foreign_keys.is_empty(),
            "FK should be removed by DROP CONSTRAINT"
        );
    }

    #[test]
    fn alter_table_drop_constraint_removes_index() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          email TEXT
        );
        CREATE INDEX idx_email ON users (email);
        ALTER TABLE users DROP CONSTRAINT idx_email;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(
            users.indexes.is_empty(),
            "index should be removed by DROP CONSTRAINT"
        );
    }

    #[test]
    fn alter_table_drop_unknown_constraint_warns() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users DROP CONSTRAINT ghost;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        assert!(output.schema.is_some());
        assert!(output.has_warnings());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ghost"))
        );
    }

    #[test]
    fn alter_table_drop_primary_key() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          name TEXT NOT NULL
        );
        ALTER TABLE users DROP PRIMARY KEY;
        ";

        let schema = parse_sql_to_schema(sql).expect("parse should succeed");
        let table = &schema.tables[0];
        assert!(
            !table.columns.iter().any(|c| c.is_primary_key),
            "all PK flags should be cleared after DROP PRIMARY KEY"
        );
    }

    #[test]
    fn alter_table_drop_foreign_key_mysql_style() {
        let sql = r"
        CREATE TABLE orgs (id BIGINT PRIMARY KEY);
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          org_id BIGINT,
          CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
        );
        ALTER TABLE users DROP FOREIGN KEY fk_org;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        let schema = output.schema.expect("parse should succeed");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(
            users.foreign_keys.is_empty(),
            "FK should be removed by DROP FOREIGN KEY"
        );
    }

    #[test]
    fn alter_table_drop_unknown_foreign_key_warns() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users DROP FOREIGN KEY ghost;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        assert!(output.schema.is_some());
        assert!(output.has_warnings());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ghost"))
        );
    }

    #[test]
    fn alter_table_drop_index() {
        let sql = r"
        CREATE TABLE users (
          id BIGINT PRIMARY KEY,
          email TEXT
        );
        CREATE INDEX idx_email ON users (email);
        ALTER TABLE users DROP INDEX idx_email;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        let schema = output.schema.expect("parse should succeed");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(
            users.indexes.is_empty(),
            "index should be removed by DROP INDEX"
        );
    }

    #[test]
    fn alter_table_drop_unknown_index_warns() {
        let sql = r"
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        ALTER TABLE users DROP INDEX ghost;
        ";

        let output = parse_sql_to_schema_with_diagnostics(sql);
        assert!(output.schema.is_some());
        assert!(output.has_warnings());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ghost"))
        );
    }
}
