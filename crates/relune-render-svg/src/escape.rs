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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text_content() {
        let input = "<script>alert('xss')</script>";
        assert_eq!(
            escape_text(input),
            "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn escapes_attribute_content() {
        let input = r#"" onload="alert('xss')"#;
        assert_eq!(
            escape_attribute(input),
            "&quot; onload=&quot;alert(&#39;xss&#39;)"
        );
    }
}
