//! Error types for SVG rendering.

use thiserror::Error;

/// Errors that can occur during SVG rendering.
#[derive(Debug, Error)]
pub enum SvgRenderError {
    /// Formatting into the output buffer failed.
    #[error("failed to format SVG output")]
    Format(#[from] std::fmt::Error),
}
