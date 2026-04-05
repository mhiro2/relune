//! Error types for the relune application layer.

use relune_layout::LayoutError;
use relune_parser_sql::ParseError;
use relune_render_html::HtmlRenderError;
use relune_render_svg::SvgRenderError;
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

    /// SVG rendering error.
    #[error("SVG rendering error: {0}")]
    SvgRender(#[from] SvgRenderError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Input not found or invalid.
    #[error("Input error ({input_type}): {message}")]
    Input {
        /// Logical input kind such as `sql_text` or `schema_json_file`.
        input_type: String,
        /// User-facing error message.
        message: String,
    },

    /// Schema not found.
    #[error("Schema not found: {schema}")]
    SchemaNotFound {
        /// Requested schema name.
        schema: String,
    },

    /// Table not found.
    #[error("Table not found: {table}")]
    TableNotFound {
        /// Requested table name.
        table: String,
    },

    /// Unsupported operation.
    #[error("Unsupported operation ({operation}): {message}")]
    Unsupported {
        /// Unsupported feature or operation name.
        operation: String,
        /// Additional diagnostic detail.
        message: String,
    },

    /// Database introspection error (live `--db-url` input).
    #[cfg(feature = "introspect")]
    #[error("Database introspection error: {0}")]
    Introspect(#[from] relune_introspect::IntrospectError),

    /// Generic error with message.
    #[error("{context}: {message}")]
    Other {
        /// Error context.
        context: String,
        /// User-facing message.
        message: String,
    },
}

impl AppError {
    /// Stable category string for WASM / API consumers (not localized).
    #[must_use]
    pub const fn category_code(&self) -> Option<&'static str> {
        match self {
            Self::Parse(_) => Some("PARSE_ERROR"),
            Self::Layout(_) => Some("LAYOUT_ERROR"),
            Self::HtmlRender(_) => Some("RENDER_ERROR"),
            Self::SvgRender(_) => Some("RENDER_ERROR"),
            Self::Io(_) => Some("IO_ERROR"),
            Self::Json(_) => Some("JSON_ERROR"),
            Self::Input { .. } => Some("INPUT_ERROR"),
            Self::SchemaNotFound { .. } => Some("SCHEMA_NOT_FOUND"),
            Self::TableNotFound { .. } => Some("TABLE_NOT_FOUND"),
            Self::Unsupported { .. } => Some("UNSUPPORTED"),
            #[cfg(feature = "introspect")]
            Self::Introspect(_) => Some("INTROSPECT_ERROR"),
            Self::Other { .. } => None,
        }
    }

    /// Create a new input error.
    pub fn input(msg: impl Into<String>) -> Self {
        Self::Input {
            input_type: "unknown".to_string(),
            message: msg.into(),
        }
    }

    /// Create a new typed input error.
    pub fn input_with_type(input_type: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Input {
            input_type: input_type.into(),
            message: msg.into(),
        }
    }

    /// Create a new table not found error.
    pub fn table_not_found(table: impl Into<String>) -> Self {
        Self::TableNotFound {
            table: table.into(),
        }
    }

    /// Create a new unsupported operation error.
    pub fn unsupported(operation: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Unsupported {
            operation: operation.into(),
            message: msg.into(),
        }
    }

    /// Create a generic error with context.
    pub fn other(context: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Other {
            context: context.into(),
            message: msg.into(),
        }
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
}
