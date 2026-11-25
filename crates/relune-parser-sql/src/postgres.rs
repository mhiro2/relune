//! `PostgreSQL` syntax that sqlparser's `PostgreSqlDialect` does not handle.

use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer, Whitespace};

/// Joins string literals that `PostgreSQL` treats as one constant.
///
/// Two string constants separated only by whitespace that includes at least
/// one newline are a single constant (`'a'` / newline / `'b'` is `'ab'`).
/// `--` comments may sit in that whitespace; a `/* */` comment, a same-line
/// separator, or a continuation that is not a plain `'...'` literal keeps the
/// literals apart, exactly as the server rejects them.
pub(crate) fn join_string_continuations(tokens: Vec<TokenWithSpan>) -> Vec<TokenWithSpan> {
    let mut joined: Vec<TokenWithSpan> = Vec::with_capacity(tokens.len());
    // The literal a continuation would extend, and whether a newline has
    // followed it since.
    let mut open: Option<(usize, bool)> = None;
    for mut token in tokens {
        if let Some((head, newline)) = open {
            match &token.token {
                Token::Whitespace(whitespace)
                    if !matches!(whitespace, Whitespace::MultiLineComment(_)) =>
                {
                    open = Some((head, newline || *whitespace == Whitespace::Newline));
                    joined.push(token);
                    continue;
                }
                Token::SingleQuotedString(continuation) if newline => {
                    if let Some(text) = continuation_text(&joined[head].token, continuation) {
                        joined.truncate(head + 1);
                        let literal = &mut joined[head];
                        if let Some(value) = literal_text(&mut literal.token) {
                            value.push_str(&text);
                        }
                        literal.span = literal.span.union(&token.span);
                        open = Some((head, false));
                        continue;
                    }
                }
                _ => {}
            }
        }
        open = literal_text(&mut token.token)
            .is_some()
            .then_some((joined.len(), false));
        joined.push(token);
    }
    joined
}

/// The text of a string constant a continuation may extend.
const fn literal_text(token: &mut Token) -> Option<&mut String> {
    match token {
        Token::SingleQuotedString(text)
        | Token::NationalStringLiteral(text)
        | Token::EscapedStringLiteral(text)
        | Token::UnicodeStringLiteral(text)
        | Token::SingleQuotedByteStringLiteral(text)
        | Token::HexStringLiteral(text) => Some(text),
        _ => None,
    }
}

/// The text a plain `continuation` adds to the literal `head`.
///
/// The server reads a continuation in its head's mode: after `E'...'` its
/// backslash escapes apply, after `U&'...'` its Unicode escapes, and after
/// `B'...'` / `X'...'` it holds more digits. Re-reading the continuation with
/// the head's prefix applies the same rules.
fn continuation_text(head: &Token, continuation: &str) -> Option<String> {
    let prefix = match head {
        Token::SingleQuotedString(_) | Token::NationalStringLiteral(_) => {
            return Some(continuation.to_owned());
        }
        Token::EscapedStringLiteral(_) => "E",
        Token::UnicodeStringLiteral(_) => "U&",
        Token::SingleQuotedByteStringLiteral(_) => "B",
        Token::HexStringLiteral(_) => "X",
        _ => return None,
    };
    let source = format!("{prefix}'{}'", continuation.replace('\'', "''"));
    let mut tokens = Tokenizer::new(&PostgreSqlDialect {}, &source)
        .tokenize()
        .ok()?;
    match tokens.as_mut_slice() {
        [token] if std::mem::discriminant(token) == std::mem::discriminant(head) => {
            literal_text(token).map(std::mem::take)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literals(sql: &str) -> Vec<String> {
        let tokens = Tokenizer::new(&PostgreSqlDialect {}, sql)
            .tokenize_with_location()
            .unwrap();
        join_string_continuations(tokens)
            .into_iter()
            .filter_map(|mut token| literal_text(&mut token.token).map(std::mem::take))
            .collect()
    }

    #[test]
    fn literals_separated_by_a_newline_are_one_constant() {
        assert_eq!(literals("'a'\n'b'\r\n  'c'"), ["abc"]);
        assert_eq!(literals("'it''s '\n'fine'"), ["it's fine"]);
        // Line comments are whitespace, with or without a newline of their own.
        assert_eq!(literals("'a' -- note\n'b'"), ["ab"]);
        assert_eq!(literals("'a'\n-- note\n'b'"), ["ab"]);
    }

    #[test]
    fn escape_string_continuations_take_its_escapes() {
        // The server reads `'y\n'` after `E'x\t'` as an escape string too.
        assert_eq!(literals("E'x\\t'\n'y\\n'"), ["x\ty\n"]);
        assert_eq!(literals("E'it\\'s '\n'Bob''s'"), ["it's Bob's"]);
    }

    #[test]
    fn prefixed_continuations_take_their_head_mode() {
        assert_eq!(literals("U&'a'\n'\\0062'"), ["ab"]);
        assert_eq!(literals("N'a'\n'b'"), ["ab"]);
        let tokens = Tokenizer::new(&PostgreSqlDialect {}, "B'10'\n'01' X'1F'\n'2A'")
            .tokenize_with_location()
            .unwrap();
        let digits: Vec<Token> = join_string_continuations(tokens)
            .into_iter()
            .map(|token| token.token)
            .filter(|token| !matches!(token, Token::Whitespace(_) | Token::EOF))
            .collect();
        assert_eq!(
            digits,
            [
                Token::SingleQuotedByteStringLiteral("1001".into()),
                Token::HexStringLiteral("1F2A".into()),
            ]
        );
    }

    #[test]
    fn literals_the_server_keeps_apart_stay_apart() {
        // No newline between the literals.
        assert_eq!(literals("'a' 'b'"), ["a", "b"]);
        // A block comment breaks the continuation, even across lines.
        assert_eq!(literals("'a' /* c */\n'b'"), ["a", "b"]);
        assert_eq!(literals("'a'\n/* c */ 'b'"), ["a", "b"]);
        // Only a plain literal continues a constant.
        assert_eq!(literals("'a'\nE'b'"), ["a", "b"]);
        assert_eq!(literals("'a'\n$$b$$"), ["a"]);
        // Anything else between the literals ends the run.
        assert_eq!(literals("'a',\n'b'"), ["a", "b"]);
    }

    #[test]
    fn joined_literal_spans_every_fragment() {
        let tokens = Tokenizer::new(&PostgreSqlDialect {}, "SELECT 'a'\n  'b';")
            .tokenize_with_location()
            .unwrap();
        let literal = join_string_continuations(tokens)
            .into_iter()
            .find(|token| matches!(token.token, Token::SingleQuotedString(_)))
            .unwrap();
        assert_eq!((literal.span.start.line, literal.span.start.column), (1, 8));
        assert_eq!((literal.span.end.line, literal.span.end.column), (2, 6));
    }
}
