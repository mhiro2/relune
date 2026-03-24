//! Shared XML/SVG escape utilities.

/// Escapes special characters for use in SVG text content.
///
/// Escapes `&`, `<`, `>`, `"`, and `'` for defense-in-depth XSS protection.
#[must_use]
pub fn escape_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Escapes special characters for use in SVG attribute values.
#[must_use]
pub fn escape_attribute(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
