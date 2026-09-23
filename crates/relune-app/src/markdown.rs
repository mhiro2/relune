//! Context-aware escaping helpers for Markdown output.
//!
//! Schema identifiers, comments, and review messages are user-controlled, so
//! every value embedded in generated Markdown must go through the helper that
//! matches its context. Otherwise a `|`, a newline, a backtick run, or raw HTML
//! can break table rows, close code fences, or spoof report structure.

use std::borrow::Cow;

/// ASCII punctuation that can start inline Markdown or HTML constructs.
const INLINE_SPECIAL: &[char] = &[
    '\\', '`', '*', '_', '[', ']', '<', '>', '#', '|', '!', '~', '&',
];

/// Escape free-form text for use in an inline context (paragraph, list item,
/// heading, or table cell).
///
/// Line breaks and other control characters collapse into a single space so
/// the value stays on one line, and punctuation that could open emphasis,
/// links, HTML, or table cells is backslash-escaped. Intraword underscores
/// cannot open emphasis in GFM, so `snake_case` identifiers stay readable.
pub fn text(value: &str) -> String {
    let flat = single_line(value);
    let chars: Vec<char> = flat.trim().chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        let intraword_underscore = c == '_'
            && i > 0
            && chars[i - 1].is_alphanumeric()
            && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
        if INLINE_SPECIAL.contains(&c) && !intraword_underscore {
            out.push('\\');
        }
        out.push(c);
    }
    escape_block_start(out)
}

/// Render a value as an inline code span.
///
/// The delimiter is one backtick longer than the longest backtick run in the
/// value, so the content cannot terminate the span early. Empty values fall
/// back to an empty `<code>` element because Markdown has no empty code span.
pub fn code(value: &str) -> String {
    let flat = single_line(value);
    if flat.is_empty() {
        return "<code></code>".to_string();
    }
    let fence = "`".repeat(max_backtick_run(&flat) + 1);
    // CommonMark strips one leading and trailing space when both are present
    // (unless the content is all spaces), so pad whenever the content touches
    // the delimiter with a backtick or space.
    let all_spaces = flat.chars().all(|c| c == ' ');
    let needs_padding = !all_spaces && (flat.starts_with(['`', ' ']) || flat.ends_with(['`', ' ']));
    let pad = if needs_padding { " " } else { "" };
    format!("{fence}{pad}{flat}{pad}{fence}")
}

/// Render a fenced code block whose fence is longer than any backtick run in
/// the body, so the body cannot close the block early.
pub fn fenced_code(info: &str, body: &str) -> String {
    let body: String = body
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    let body = body.trim_end_matches('\n');
    let fence = "`".repeat(max_backtick_run(body).max(2) + 1);
    format!("{fence}{info}\n{body}\n{fence}")
}

/// Escape a value for use inside raw HTML (e.g. `<summary><code>…</code>`).
pub fn html(value: &str) -> String {
    let flat = single_line(value);
    let mut out = String::with_capacity(flat.len());
    for c in flat.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Replace line breaks and other control characters with a single space.
fn single_line(value: &str) -> Cow<'_, str> {
    if !value.chars().any(char::is_control) {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len());
    let mut last_was_space = false;
    for c in value.chars() {
        if c.is_control() {
            if !last_was_space {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = c == ' ';
        }
    }
    Cow::Owned(out)
}

/// Escape a leading marker that would turn the text into a list item, block
/// quote, or setext heading underline when it starts a line.
fn escape_block_start(mut out: String) -> String {
    if out.starts_with(['-', '+', '=']) {
        out.insert(0, '\\');
        return out;
    }
    let digits = out.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && out[digits..].starts_with(['.', ')']) {
        out.insert(digits, '\\');
    }
    out
}

fn max_backtick_run(value: &str) -> usize {
    value.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escapes_inline_markup_and_flattens_lines() {
        assert_eq!(
            text("a|b\n# not *heading* <b>&</b>"),
            r"a\|b \# not \*heading\* \<b\>\&\</b\>"
        );
        assert_eq!(text("C:\\path"), r"C:\\path");
        assert_eq!(text("line1\r\n\r\nline2"), "line1 line2");
        assert_eq!(text("user_id _x_ a__b"), r"user_id \_x\_ a\_\_b");
    }

    #[test]
    fn text_escapes_block_markers_at_start() {
        assert_eq!(text("- item"), r"\- item");
        assert_eq!(text("=== "), r"\===");
        assert_eq!(text("12. item"), r"12\. item");
        assert_eq!(text("3) item"), r"3\) item");
        assert_eq!(text("> quote"), r"\> quote");
        assert_eq!(text("v1. release"), "v1. release");
    }

    #[test]
    fn code_uses_delimiter_longer_than_content_runs() {
        assert_eq!(code("users"), "`users`");
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("a``b`"), "``` a``b` ```");
        assert_eq!(code("x\ny"), "`x y`");
        assert_eq!(code(" a"), "`  a `");
        assert_eq!(code("  "), "`  `");
        assert_eq!(code(""), "<code></code>");
    }

    #[test]
    fn fenced_code_outgrows_backtick_runs_in_body() {
        assert_eq!(fenced_code("sql", "SELECT 1"), "```sql\nSELECT 1\n```");
        assert_eq!(
            fenced_code("sql", "SELECT '```'\n````\n"),
            "`````sql\nSELECT '```'\n````\n`````"
        );
        assert_eq!(fenced_code("", "a\r\nb\u{7}"), "```\na\nb\n```");
    }

    #[test]
    fn html_escapes_special_chars() {
        assert_eq!(
            html("<a href=\"x\">&</a>\n"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt; "
        );
    }
}
