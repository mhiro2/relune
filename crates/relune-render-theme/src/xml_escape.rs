//! XML / HTML text and attribute escaping shared by SVG and HTML renderers.

/// Escapes `&`, `<`, `>`, `"`, and `'` for use in XML text nodes and HTML text content.
///
/// Characters that XML 1.0 forbids even as character references (C0 controls
/// other than tab, newline, and carriage return, plus U+FFFE and U+FFFF) are
/// replaced with U+FFFD so the output always parses as XML.
///
/// Single-pass scan avoids intermediate `String` allocations from chained `.replace()`.
#[must_use]
pub fn escape_xml_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            ch if is_forbidden_in_xml(ch) => out.push(char::REPLACEMENT_CHARACTER),
            _ => out.push(ch),
        }
    }
    out
}

/// Returns whether `ch` falls outside the XML 1.0 `Char` production.
const fn is_forbidden_in_xml(ch: char) -> bool {
    matches!(ch, '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
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
    fn replaces_characters_forbidden_in_xml() {
        assert_eq!(
            escape_xml_text("a\u{0}b\u{8}c\u{b}d\u{1f}e\u{fffe}f"),
            "a\u{fffd}b\u{fffd}c\u{fffd}d\u{fffd}e\u{fffd}f"
        );
        assert_eq!(escape_xml_attribute("x\u{1}"), "x\u{fffd}");
    }

    #[test]
    fn keeps_whitespace_controls_and_other_characters() {
        let input = "tab\there\nline\r\u{7f}\u{85}\u{fffd}\u{10000}";
        assert_eq!(escape_xml_text(input), input);
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
