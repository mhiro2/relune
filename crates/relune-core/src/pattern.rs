//! Glob matching for table-name patterns.
//!
//! Supported syntax:
//! - `*` matches any sequence of characters (including none)
//! - `?` matches exactly one character
//! - `[abc]`, `[a-z]` match one character from the set; `[!abc]` or `[^abc]`
//!   negate it, a leading `]` is taken literally, and a reversed range such
//!   as `[z-a]` matches nothing
//! - `\` escapes the next character so that it matches literally
//!
//! An unterminated `[` is treated as a literal `[`. Matching is
//! case-sensitive and `.` has no special meaning, so `*` also spans schema
//! separators.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(char),
    AnyChar,
    AnySequence,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

impl Token {
    fn matches(&self, c: char) -> bool {
        match self {
            Self::Literal(expected) => *expected == c,
            Self::AnyChar => true,
            Self::AnySequence => unreachable!("`*` is handled by the matcher loop"),
            Self::Class { negated, ranges } => {
                ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&c)) != *negated
            }
        }
    }
}

fn tokenize(pattern: &str) -> Vec<Token> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                // Collapse runs of `*`; they are equivalent to a single one.
                if tokens.last() != Some(&Token::AnySequence) {
                    tokens.push(Token::AnySequence);
                }
                i += 1;
            }
            '?' => {
                tokens.push(Token::AnyChar);
                i += 1;
            }
            '\\' if i + 1 < chars.len() => {
                tokens.push(Token::Literal(chars[i + 1]));
                i += 2;
            }
            '[' => {
                if let Some((token, next)) = parse_class(&chars, i + 1) {
                    tokens.push(token);
                    i = next;
                } else {
                    tokens.push(Token::Literal('['));
                    i += 1;
                }
            }
            c => {
                tokens.push(Token::Literal(c));
                i += 1;
            }
        }
    }
    tokens
}

/// Parse a bracket expression whose body starts at `start` (just after `[`).
/// Returns the class token and the index after the closing `]`, or `None`
/// when the bracket is never closed.
fn parse_class(chars: &[char], start: usize) -> Option<(Token, usize)> {
    let mut i = start;
    let negated = matches!(chars.get(i), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let body_start = i;
    let mut ranges = Vec::new();
    loop {
        let c = *chars.get(i)?;
        if c == ']' && i > body_start {
            return Some((Token::Class { negated, ranges }, i + 1));
        }
        let lo = if c == '\\' {
            i += 1;
            *chars.get(i)?
        } else {
            c
        };
        i += 1;
        if chars.get(i) == Some(&'-') && chars.get(i + 1).is_some_and(|&next| next != ']') {
            let hi = if chars[i + 1] == '\\' {
                i += 1;
                *chars.get(i + 1)?
            } else {
                chars[i + 1]
            };
            i += 2;
            // A reversed range such as `[z-a]` is empty, as in POSIX globs.
            ranges.push((lo, hi));
        } else {
            ranges.push((lo, lo));
        }
    }
}

/// Returns `true` when `value` matches the glob `pattern` in full.
#[must_use]
pub fn glob_match(pattern: &str, value: &str) -> bool {
    let tokens = tokenize(pattern);
    let chars: Vec<char> = value.chars().collect();

    let (mut t, mut v) = (0, 0);
    // Position of the most recent `*` and the value index it resumes from.
    let mut backtrack: Option<(usize, usize)> = None;
    while v < chars.len() {
        match tokens.get(t) {
            Some(Token::AnySequence) => {
                backtrack = Some((t, v));
                t += 1;
            }
            Some(token) if token.matches(chars[v]) => {
                t += 1;
                v += 1;
            }
            _ => {
                let Some((star, resume)) = backtrack else {
                    return false;
                };
                t = star + 1;
                v = resume + 1;
                backtrack = Some((star, resume + 1));
            }
        }
    }
    tokens[t..].iter().all(|token| *token == Token::AnySequence)
}

/// Returns `true` when any pattern matches either the schema-qualified name
/// or the bare table name.
#[must_use]
pub fn matches_table_pattern(patterns: &[String], qualified_name: &str, name: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| glob_match(pattern, qualified_name) || glob_match(pattern, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_and_star() {
        assert!(glob_match("users", "users"));
        assert!(!glob_match("users", "users_archive"));
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("audit_*", "audit_log"));
        assert!(glob_match("*_log", "audit_log"));
        assert!(glob_match("*user*", "my_user_table"));
        assert!(glob_match("**", "x"));
    }

    #[test]
    fn star_in_the_middle() {
        assert!(glob_match("foo*bar", "foobar"));
        assert!(glob_match("foo*bar", "foo_x_bar"));
        assert!(!glob_match("foo*bar", "foo_x_baz"));
        assert!(glob_match("a*b*c", "a_b_b_c"));
        assert!(!glob_match("a*b*c", "a_c_b"));
        assert!(glob_match("public.*", "public.users"));
    }

    #[test]
    fn question_mark() {
        assert!(glob_match("log_202?", "log_2024"));
        assert!(!glob_match("log_202?", "log_202"));
        assert!(!glob_match("log_202?", "log_20245"));
    }

    #[test]
    fn character_classes() {
        assert!(glob_match("log_[0-9]", "log_7"));
        assert!(!glob_match("log_[0-9]", "log_x"));
        assert!(glob_match("t[abc]", "tb"));
        assert!(glob_match("t[!abc]", "td"));
        assert!(!glob_match("t[^abc]", "ta"));
        assert!(glob_match("[]]", "]"));
        assert!(glob_match("[a-]", "-"));
        assert!(!glob_match("[9-0]", "5"));
        assert!(glob_match("[9-0x]", "x"));
    }

    #[test]
    fn escapes_and_literal_brackets() {
        assert!(glob_match(r"a\*b", "a*b"));
        assert!(!glob_match(r"a\*b", "axb"));
        assert!(glob_match(r"a\?", "a?"));
        assert!(glob_match(r"[\]]", "]"));
        assert!(glob_match("a[b", "a[b"));
        assert!(glob_match("trailing\\", "trailing\\"));
    }

    #[test]
    fn multibyte_characters() {
        assert!(glob_match("ユーザー_?", "ユーザー_1"));
        assert!(glob_match("*テスト", "結合テスト"));
    }

    #[test]
    fn table_pattern_checks_qualified_and_bare_names() {
        let patterns = vec!["audit_*".to_string(), "public.tmp_?".to_string()];
        assert!(matches_table_pattern(
            &patterns,
            "app.audit_log",
            "audit_log"
        ));
        assert!(matches_table_pattern(&patterns, "public.tmp_1", "tmp_1"));
        assert!(!matches_table_pattern(&patterns, "app.tmp_1", "tmp_1"));
        assert!(!matches_table_pattern(&[], "users", "users"));
    }
}
