//! Error types for HTML rendering.

use thiserror::Error;

/// Errors that can occur during HTML rendering.
#[derive(Debug, Error)]
pub enum HtmlRenderError {
    /// Failed to serialize metadata to JSON.
    #[error("Failed to serialize metadata: {0}")]
    MetadataSerialization(#[from] serde_json::Error),
}
