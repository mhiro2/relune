use serde::{Deserialize, Serialize};
use std::fmt;

/// Severity level for diagnostics.
///
/// Order is significant: Error > Warning > Info > Hint.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational hint (lowest severity).
    Hint,
    /// Informational message.
    Info,
    /// Warning (potential issue).
    Warning,
    /// Error (default, highest severity).
    #[default]
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
            Self::Info => write!(f, "info"),
            Self::Hint => write!(f, "hint"),
        }
    }
}

/// A diagnostic code identifying the type of issue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DiagnosticCode {
    /// Code prefix (e.g., "PARSE", "SCHEMA", "LINT").
    pub prefix: String,
    /// Numeric code within the prefix category.
    pub number: u32,
}

impl DiagnosticCode {
    /// Creates a new diagnostic code.
    #[must_use]
    pub fn new(prefix: impl Into<String>, number: u32) -> Self {
        Self {
            prefix: prefix.into(),
            number,
        }
    }

    /// Returns the full code string (e.g., "PARSE001").
    #[must_use]
    pub fn full_code(&self) -> String {
        format!("{}{:03}", self.prefix, self.number)
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.full_code())
    }
}

/// Source span for locating the origin of a diagnostic.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSpan {
    /// Byte offset from the start of the source.
    pub offset: usize,
    /// Length of the span in bytes.
    pub length: usize,
}

impl SourceSpan {
    /// Creates a new source span.
    #[must_use]
    pub const fn new(offset: usize, length: usize) -> Self {
        Self { offset, length }
    }
}

/// A diagnostic message with optional source location.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    /// Severity level.
    pub severity: Severity,
    /// Diagnostic code.
    pub code: DiagnosticCode,
    /// Human-readable message.
    pub message: String,
    /// Optional source span.
    pub span: Option<SourceSpan>,
    /// Optional source file path.
    pub source: Option<String>,
}

impl Diagnostic {
    /// Creates a new error diagnostic.
    pub fn error(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span: None,
            source: None,
        }
    }

    /// Creates a new warning diagnostic.
    pub fn warning(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span: None,
            source: None,
        }
    }

    /// Creates a new info diagnostic.
    pub fn info(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            code,
            message: message.into(),
            span: None,
            source: None,
        }
    }

    /// Creates a new hint diagnostic.
    pub fn hint(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Hint,
            code,
            message: message.into(),
            span: None,
            source: None,
        }
    }

    /// Adds a source span to the diagnostic.
    #[must_use]
    pub const fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    /// Adds a source file path to the diagnostic.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Common diagnostic codes.
pub mod codes {
    use super::DiagnosticCode;

    /// Returns the code for a parse error.
    #[must_use]
    pub fn parse_error() -> DiagnosticCode {
        DiagnosticCode::new("PARSE", 1)
    }
    /// Returns the code for unsupported syntax.
    #[must_use]
    pub fn parse_unsupported() -> DiagnosticCode {
        DiagnosticCode::new("PARSE", 2)
    }
    /// Returns the code for skipped DML statements.
    #[must_use]
    pub fn parse_skipped() -> DiagnosticCode {
        DiagnosticCode::new("PARSE", 3)
    }
    /// Returns the code for SQL input that produced no schema objects.
    #[must_use]
    pub fn parse_empty_schema() -> DiagnosticCode {
        DiagnosticCode::new("PARSE", 4)
    }

    /// Returns the code for an unknown table reference.
    #[must_use]
    pub fn schema_unknown_table() -> DiagnosticCode {
        DiagnosticCode::new("SCHEMA", 1)
    }
    /// Returns the code for a duplicate table definition.
    #[must_use]
    pub fn schema_duplicate_table() -> DiagnosticCode {
        DiagnosticCode::new("SCHEMA", 2)
    }
    /// Returns the code for an unknown column reference.
    #[must_use]
    pub fn schema_unknown_column() -> DiagnosticCode {
        DiagnosticCode::new("SCHEMA", 3)
    }

    /// Returns the code for a missing primary key.
    #[must_use]
    pub fn lint_no_pk() -> DiagnosticCode {
        DiagnosticCode::new("LINT", 1)
    }
    /// Returns the code for an orphan table.
    #[must_use]
    pub fn lint_orphan_table() -> DiagnosticCode {
        DiagnosticCode::new("LINT", 2)
    }
}
