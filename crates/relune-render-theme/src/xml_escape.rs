//! XML / HTML text and attribute escaping shared by SVG and HTML renderers.

/// Escapes `&`, `<`, `>`, `"`, and `'` for use in XML text nodes and HTML text content.
///
/// Apply the same mapping for SVG `<text>` and attribute values used in this project.
#[must_use]
pub fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Escapes special characters for use in XML / HTML attribute values.
#[must_use]
pub fn escape_xml_attribute(input: &str) -> String {
    escape_xml_text(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text_content() {
        let input = "<script>alert('xss')</script>";
        assert_eq!(
            escape_xml_text(input),
            "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn escapes_attribute_content() {
        let input = r#"" onload="alert('xss')"#;
        assert_eq!(
            escape_xml_attribute(input),
            "&quot; onload=&quot;alert(&#39;xss&#39;)"
        );
    }
}
