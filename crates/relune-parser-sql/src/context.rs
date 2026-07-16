//! Shared parsing context, span helpers, and column staging types.

use relune_core::{
    Column, ColumnId, Diagnostic, Severity, SourceSpan, SqlDialect, TableId, diagnostic::codes,
};
use sqlparser::ast::Spanned;
use sqlparser::tokenizer::{Location, Span as SqlSpan};
use std::collections::HashSet;

/// Pre-computed byte offsets for each line start, enabling O(1) line
/// lookup followed by a short character walk instead of scanning the
/// entire input for every `Location → byte offset` conversion.
pub(crate) struct LineOffsets {
    /// Byte offset of the start of each 1-based line.
    /// `starts[0]` is unused; `starts[1]` = 0 (line 1 starts at byte 0).
    starts: Vec<usize>,
    /// Total byte length of the input.
    len: usize,
}

impl LineOffsets {
    pub(crate) fn new(input: &str) -> Self {
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

    pub(crate) fn location_to_offset(&self, input: &str, location: Location) -> Option<usize> {
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

pub(crate) struct ParseContext {
    /// Next table ID to assign.
    next_table_id: u64,
    /// Diagnostics collected during parsing.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Set of seen table `stable_ids` for duplicate detection.
    pub(crate) seen_tables: HashSet<String>,
    /// The resolved SQL dialect being used.
    pub(crate) dialect: SqlDialect,
}

impl ParseContext {
    pub(crate) fn new() -> Self {
        Self {
            next_table_id: 1,
            diagnostics: Vec::new(),
            seen_tables: HashSet::new(),
            dialect: SqlDialect::Postgres,
        }
    }

    pub(crate) const fn next_table_id(&mut self) -> TableId {
        let id = TableId(self.next_table_id);
        self.next_table_id += 1;
        id
    }

    pub(crate) fn warn_unsupported(&mut self, construct: &str, span: Option<SourceSpan>) {
        self.diagnostics.push(
            Diagnostic::warning(
                codes::parse_unsupported(),
                format!("Unsupported SQL construct: {construct}. This statement will be skipped."),
            )
            .with_span_opt(span),
        );
    }

    pub(crate) fn info_skipped(&mut self, construct: &str) {
        self.diagnostics.push(Diagnostic::info(
            codes::parse_skipped(),
            format!("Skipped DML statement: {construct}. Only DDL statements are processed."),
        ));
    }

    pub(crate) fn warn_duplicate_table(&mut self, table_name: &str, span: Option<SourceSpan>) {
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

    pub(crate) fn warn_empty_schema(&mut self) {
        self.diagnostics.push(Diagnostic::warning(
            codes::parse_empty_schema(),
            "No schema objects were produced from the input. Check whether the SQL only contains comments, whitespace, or unsupported statements.",
        ));
    }

    pub(crate) fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

pub(crate) struct ParsedColumn {
    pub(crate) name: String,
    pub(crate) data_type: String,
    pub(crate) nullable: bool,
    pub(crate) is_primary_key: bool,
    pub(crate) comment: Option<String>,
    pub(crate) semantics: relune_core::ColumnSemantics,
}

impl ParsedColumn {
    pub(crate) fn into_column(self, id: ColumnId) -> Column {
        Column {
            id,
            name: self.name,
            data_type: self.data_type,
            nullable: self.nullable,
            is_primary_key: self.is_primary_key,
            comment: self.comment,
            enum_values: None,
            semantics: self.semantics,
        }
    }
}

pub(crate) fn source_span_from_sql_span(
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

pub(crate) fn span_from_spanned<T: Spanned>(
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

pub(crate) fn span_from_ident(
    input: &str,
    offsets: &LineOffsets,
    ident: &sqlparser::ast::Ident,
) -> Option<SourceSpan> {
    source_span_from_sql_span(input, offsets, ident.span)
}
