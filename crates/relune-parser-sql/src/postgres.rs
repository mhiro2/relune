//! `PostgreSQL` syntax that sqlparser's `PostgreSqlDialect` does not handle.

use sqlparser::ast::{CommentObject, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Span, Token, TokenWithSpan, Tokenizer, Whitespace};

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

/// Object kinds `COMMENT ON` accepts, with each phrase listed before any
/// shorter phrase it starts with.
const COMMENT_OBJECTS: &[&str] = &[
    "ACCESS METHOD",
    "AGGREGATE",
    "CAST",
    "COLLATION",
    "COLUMN",
    "CONSTRAINT",
    "CONVERSION",
    "DATABASE",
    "DOMAIN",
    "EVENT TRIGGER",
    "EXTENSION",
    "FOREIGN DATA WRAPPER",
    "FOREIGN TABLE",
    "FUNCTION",
    "INDEX",
    "LARGE OBJECT",
    "MATERIALIZED VIEW",
    "OPERATOR CLASS",
    "OPERATOR FAMILY",
    "OPERATOR",
    "POLICY",
    "PROCEDURAL LANGUAGE",
    "LANGUAGE",
    "PROCEDURE",
    "PUBLICATION",
    "ROLE",
    "ROUTINE",
    "RULE",
    "SCHEMA",
    "SEQUENCE",
    "SERVER",
    "STATISTICS",
    "SUBSCRIPTION",
    "TABLE",
    "TABLESPACE",
    "TEXT SEARCH CONFIGURATION",
    "TEXT SEARCH DICTIONARY",
    "TEXT SEARCH PARSER",
    "TEXT SEARCH TEMPLATE",
    "TRANSFORM FOR",
    "TRIGGER",
    "TYPE",
    "VIEW",
];

/// A `COMMENT ON` statement as `PostgreSQL` reads it.
pub(crate) enum Comment {
    /// A table or column comment, which the schema records.
    Applied(Box<Statement>),
    /// A comment on any other object, which the schema has no place for.
    Ignored {
        /// The object kind, such as `FUNCTION` or `CONSTRAINT`.
        object: &'static str,
        /// The object kind and reference.
        span: Span,
    },
}

/// Parses `COMMENT ON` with `PostgreSQL`'s grammar.
///
/// sqlparser rejects several forms `pg_dump` writes, such as routine
/// signatures (`FUNCTION f(integer)`), objects named relative to a table
/// (`CONSTRAINT c ON t`), and dollar-quoted text. Only table and column
/// comments reach the schema, so any other object reference is checked for
/// balance and skipped up to its `IS`.
pub(crate) fn parse_comment(parser: &mut Parser<'_>) -> Result<Comment, ParserError> {
    parser.expect_keywords(&[Keyword::COMMENT, Keyword::ON])?;
    let start = parser.peek_token().span;
    let Some(object) = COMMENT_OBJECTS.iter().copied().find(|object| {
        object
            .split(' ')
            .enumerate()
            .all(|(n, word)| is_bare_word(&parser.peek_nth_token(n).token, word))
    }) else {
        return parser.expected("COMMENT ON object type", parser.peek_token());
    };
    for _ in object.split(' ') {
        parser.advance_token();
    }

    let target = match object {
        "TABLE" => Some((CommentObject::Table, parser.parse_object_name(false)?)),
        "COLUMN" => Some((CommentObject::Column, parser.parse_object_name(false)?)),
        _ => {
            skip_object_reference(parser)?;
            None
        }
    };
    let span = start.union(&parser.get_current_token().span);
    parser.expect_keyword_is(Keyword::IS)?;
    let comment = parse_comment_text(parser)?;
    let next = parser.peek_token();
    if !matches!(next.token, Token::SemiColon | Token::EOF) {
        return parser.expected("end of COMMENT statement", next);
    }

    Ok(match target {
        Some((object_type, object_name)) => Comment::Applied(Box::new(Statement::Comment {
            object_type,
            object_name,
            comment,
            if_exists: false,
        })),
        None => Comment::Ignored { object, span },
    })
}

fn is_bare_word(token: &Token, expected: &str) -> bool {
    matches!(token, Token::Word(word) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case(expected))
}

/// Skips an object reference up to the `IS` that ends it, keeping parentheses
/// balanced so an unclosed signature is still an error.
fn skip_object_reference(parser: &mut Parser<'_>) -> Result<(), ParserError> {
    let mut depth = 0_usize;
    let mut empty = true;
    loop {
        let next = parser.peek_token();
        match &next.token {
            Token::Word(word)
                if depth == 0 && word.keyword == Keyword::IS && word.quote_style.is_none() =>
            {
                return if empty {
                    parser.expected("object name", next)
                } else {
                    Ok(())
                };
            }
            Token::LParen => depth += 1,
            Token::RParen if depth > 0 => depth -= 1,
            Token::RParen | Token::SemiColon | Token::EOF => {
                return parser.expected(if depth > 0 { ")" } else { "IS" }, next);
            }
            _ => {}
        }
        parser.advance_token();
        empty = false;
    }
}

/// `NULL`, which removes the comment, or any string constant form.
fn parse_comment_text(parser: &mut Parser<'_>) -> Result<Option<String>, ParserError> {
    if parser.parse_keyword(Keyword::NULL) {
        return Ok(None);
    }
    // Peek first so a missing text leaves the `;` for statement recovery.
    let next = parser.peek_token();
    let text = match next.token {
        Token::SingleQuotedString(text)
        | Token::EscapedStringLiteral(text)
        | Token::UnicodeStringLiteral(text)
        | Token::NationalStringLiteral(text) => text,
        Token::DollarQuotedString(text) => text.value,
        _ => return parser.expected("comment string or NULL", next),
    };
    parser.advance_token();
    Ok(Some(text))
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
