//! Error types for the relune application layer.

use relune_layout::LayoutError;
use relune_parser_sql::ParseError;
use relune_render_html::HtmlRenderError;
use thiserror::Error;

/// Application-level error.
#[derive(Debug, Error)]
pub enum AppError {
    /// SQL parsing error.
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    /// Layout error.
    #[error("Layout error: {0}")]
    Layout(#[from] LayoutError),

    /// HTML rendering error.
    #[error("HTML rendering error: {0}")]
    HtmlRender(#[from] HtmlRenderError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Input not found or invalid.
    #[error("Input error: {0}")]
    Input(String),

    /// Schema not found.
    #[error("Schema not found: {0}")]
    SchemaNotFound(String),

    /// Table not found.
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// Unsupported operation.
    #[error("Unsupported operation: {0}")]
    Unsupported(String),

    /// Database introspection error (live `--db-url` input).
    #[cfg(feature = "introspect")]
    #[error("Database introspection error: {0}")]
    Introspect(#[from] relune_introspect::IntrospectError),

    /// Generic error with message.
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Stable category string for WASM / API consumers (not localized).
    #[must_use]
    pub const fn category_code(&self) -> Option<&'static str> {
        match self {
            Self::Parse(_) => Some("PARSE_ERROR"),
            Self::Layout(_) => Some("LAYOUT_ERROR"),
            Self::HtmlRender(_) => Some("RENDER_ERROR"),
            Self::Io(_) => Some("IO_ERROR"),
            Self::Json(_) => Some("JSON_ERROR"),
            Self::Input(_) => Some("INPUT_ERROR"),
            Self::SchemaNotFound(_) => Some("SCHEMA_NOT_FOUND"),
            Self::TableNotFound(_) => Some("TABLE_NOT_FOUND"),
            Self::Unsupported(_) => Some("UNSUPPORTED"),
            #[cfg(feature = "introspect")]
            Self::Introspect(_) => Some("INTROSPECT_ERROR"),
            Self::Other(_) => None,
        }
    }

    /// Create a new input error.
    pub fn input(msg: impl Into<String>) -> Self {
        Self::Input(msg.into())
    }

    /// Create a new table not found error.
    pub fn table_not_found(table: impl Into<String>) -> Self {
        Self::TableNotFound(table.into())
    }

    /// Create a new unsupported operation error.
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }

    /// Check if this error is recoverable (allows partial output).
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::Other(_) | Self::Input(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AppError::table_not_found("users");
        assert!(err.to_string().contains("users"));
    }

    #[test]
    fn test_error_is_recoverable() {
        let err = AppError::input("test");
        assert!(err.is_recoverable());

        let err = AppError::table_not_found("test");
        assert!(!err.is_recoverable());
    }
}
