//! `PostgreSQL` syntax that sqlparser's `PostgreSqlDialect` does not handle.

use sqlparser::ast::{CommentObject, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{
    Location, Span, Token, TokenWithSpan, Tokenizer, TokenizerError, Whitespace,
};

use crate::context::LineOffsets;

/// Tokenizes `sql` as `PostgreSQL`'s lexer reads it.
///
/// On top of sqlparser's tokenizer this follows the server's string constant
/// rules: literals continued across a newline are one constant read in their
/// head's mode, and a `U&'...'` constant takes the escape character its
/// `UESCAPE` clause names. Where sqlparser would split a constant at the
/// wrong place (an `E'...'` continuation holding `\'`) or reject it (a
/// Unicode escape only valid under `UESCAPE`), the constant is read from the
/// source and tokenizing resumes after it. On an error, the tokens read
/// before it are returned with the error.
pub(crate) fn tokenize(sql: &str) -> (Vec<TokenWithSpan>, Option<TokenizerError>) {
    let mut constants = Constants::new(sql);
    // sqlparser's own tokens, which give the next tokenizer run its context.
    let mut lexed = Vec::new();
    let mut from = 0;
    let error = 'runs: loop {
        let base = constants.location(from);
        let start = lexed.len();
        let result = Tokenizer::new(&PostgreSqlDialect {}, &sql[from..])
            .tokenize_with_location_into_buf_with_mapper(&mut lexed, |mut token| {
                token.span = Span::new(
                    relocate(token.span.start, base),
                    relocate(token.span.end, base),
                );
                token
            });
        for index in start..lexed.len() {
            match constants.push(lexed[index].clone()) {
                Ok(None) => {}
                Ok(Some(end)) => {
                    lexed.truncate(index + 1);
                    from = end;
                    continue 'runs;
                }
                Err(error) => break 'runs Some(error),
            }
        }
        let Err(mut error) = result else {
            break None;
        };
        // The token sqlparser failed on starts right after the last one it
        // read; take it over if it is a constant read here.
        let at = lexed[start..]
            .last()
            .and_then(|token| constants.offset(token.span.end))
            .unwrap_or(from);
        let Some(token) = constants.take_over(at) else {
            error.location = relocate(error.location, base);
            break Some(error);
        };
        match constants.push(token.clone()) {
            Ok(resume) => {
                from = match resume {
                    Some(end) => end,
                    None => constants.offset(token.span.end).unwrap_or(sql.len()),
                };
                lexed.push(token);
            }
            Err(error) => break Some(error),
        }
    };
    constants.finish(error)
}

/// Moves a location in a tokenizer run that started at `base` to the whole
/// input.
const fn relocate(location: Location, base: Location) -> Location {
    match location.line {
        0 => location,
        1 => Location {
            line: base.line,
            column: base.column + location.column - 1,
        },
        line => Location {
            line: base.line + line - 1,
            column: location.column,
        },
    }
}

/// Joins continued string constants and applies `UESCAPE` over the token
/// stream, reading constant bodies from the source.
struct Constants<'a> {
    sql: &'a str,
    offsets: LineOffsets,
    joined: Vec<TokenWithSpan>,
    /// The constant a continuation would extend, and whether a newline has
    /// followed it since.
    open: Option<(usize, bool)>,
    /// Each `U&'...'` constant in `joined` with its undecoded body, decoded
    /// once its `UESCAPE` clause, if any, is known.
    unicode: Vec<(usize, String)>,
}

impl<'a> Constants<'a> {
    fn new(sql: &'a str) -> Self {
        Self {
            sql,
            offsets: LineOffsets::new(sql),
            joined: Vec::new(),
            open: None,
            unicode: Vec::new(),
        }
    }

    fn offset(&self, location: Location) -> Option<usize> {
        self.offsets.location_to_offset(self.sql, location)
    }

    fn location(&self, offset: usize) -> Location {
        self.offsets.offset_to_location(self.sql, offset)
    }

    /// Adds the next token. Returns the offset to resume tokenizing from when
    /// a continuation ends somewhere other than where sqlparser ended it.
    fn push(&mut self, token: TokenWithSpan) -> Result<Option<usize>, TokenizerError> {
        if let Some((head, newline)) = self.open {
            match &token.token {
                // Only whitespace and `--` comments may separate a
                // continuation from its head.
                Token::Whitespace(whitespace)
                    if !matches!(whitespace, Whitespace::MultiLineComment(_)) =>
                {
                    self.open = Some((head, newline || *whitespace == Whitespace::Newline));
                    self.joined.push(token);
                    return Ok(None);
                }
                Token::SingleQuotedString(_) if newline => {
                    return self.continue_constant(head, &token);
                }
                _ => {}
            }
        }
        self.open = None;
        if let Some(prefix) = constant_prefix(&token.token) {
            self.open = Some((self.joined.len(), false));
            if prefix == "U&" {
                // Doubled quotes collapse before Unicode escapes are decoded.
                let body = self.body(&token, prefix)?.replace("''", "'");
                self.unicode.push((self.joined.len(), body));
            }
        }
        self.joined.push(token);
        Ok(None)
    }

    /// Appends the continuation that starts at `token` to the constant at
    /// `head`, reading it in the head's mode: escapes apply after `E'...'`,
    /// and after `B'...'` / `X'...'` it holds more digits.
    fn continue_constant(
        &mut self,
        head: usize,
        token: &TokenWithSpan,
    ) -> Result<Option<usize>, TokenizerError> {
        let head_token = &self.joined[head].token;
        let prefix = constant_prefix(head_token).unwrap_or_default();
        let start = self.offset(token.span.start).unwrap_or(self.sql.len());
        let unterminated = || TokenizerError {
            message: "Unterminated string literal".to_owned(),
            location: token.span.start,
        };
        // Inside COPY, keep sqlparser's reading: resuming would lose the
        // state it reads the statement's inline data with.
        let escapes = prefix == "E" && !self.in_copy();
        let (body, end) = read_body(self.sql, start, escapes).ok_or_else(unterminated)?;
        let text = match prefix {
            "" | "N" | "U&" => body.replace("''", "'"),
            _ => decode_constant(head_token, &format!("{prefix}'{body}'")).ok_or_else(|| {
                TokenizerError {
                    message: format!("Invalid {prefix}'...' string continuation"),
                    location: token.span.start,
                }
            })?,
        };
        if prefix == "U&"
            && let Some((_, raw)) = self.unicode.last_mut()
        {
            raw.push_str(&text);
        }
        self.joined.truncate(head + 1);
        let end_location = self.location(end);
        let literal = &mut self.joined[head];
        if let Some(value) = constant_text(&mut literal.token) {
            value.push_str(&text);
        }
        literal.span = Span::new(literal.span.start, end_location);
        self.open = Some((head, false));
        Ok((self.offset(token.span.end) != Some(end)).then_some(end))
    }

    /// Whether the statement read so far is a `COPY`.
    fn in_copy(&self) -> bool {
        let start = self
            .joined
            .iter()
            .rposition(|token| token.token == Token::SemiColon)
            .map_or(0, |semicolon| semicolon + 1);
        self.joined[start..]
            .iter()
            .find(|token| !matches!(token.token, Token::Whitespace(_)))
            .is_some_and(
                |token| matches!(&token.token, Token::Word(word) if word.keyword == Keyword::COPY),
            )
    }

    /// The undecoded body of the constant `token`, after its `prefix`.
    fn body(&self, token: &TokenWithSpan, prefix: &str) -> Result<&'a str, TokenizerError> {
        let start = self.offset(token.span.start).unwrap_or(self.sql.len()) + prefix.len();
        read_body(self.sql, start, false)
            .map(|(body, _)| body)
            .ok_or_else(|| TokenizerError {
                message: "Unterminated unicode encoded string literal".to_owned(),
                location: token.span.start,
            })
    }

    /// A constant starting at `at` that sqlparser cannot read but the server
    /// can: a continuation of an `E'...'` constant, which sqlparser reads in
    /// the wrong mode, or a `U&'...'` constant whose escapes only hold under
    /// its `UESCAPE` character. Only its span matters; `push` reads the body.
    fn take_over(&self, at: usize) -> Option<TokenWithSpan> {
        if self.in_copy() {
            return None;
        }
        let rest = &self.sql[at..];
        let (token, end) = if rest.starts_with('\'')
            && let Some((head, true)) = self.open
            && matches!(self.joined[head].token, Token::EscapedStringLiteral(_))
        {
            let (_, end) = read_body(self.sql, at, true)?;
            (Token::SingleQuotedString(String::new()), end)
        } else if rest
            .get(..3)
            .is_some_and(|head| head.eq_ignore_ascii_case("U&'"))
        {
            let (_, end) = read_body(self.sql, at + 2, false)?;
            (Token::UnicodeStringLiteral(String::new()), end)
        } else {
            return None;
        };
        Some(TokenWithSpan::new(
            token,
            Span::new(self.location(at), self.location(end)),
        ))
    }

    /// Decodes every `U&'...'` constant with its escape character and drops
    /// the `UESCAPE` clauses. A constant that does not decode ends the tokens
    /// before it, and its error replaces `error`, which lies further on.
    fn finish(
        mut self,
        mut error: Option<TokenizerError>,
    ) -> (Vec<TokenWithSpan>, Option<TokenizerError>) {
        let mut dropped = vec![false; self.joined.len()];
        for (index, body) in std::mem::take(&mut self.unicode) {
            let clause = self.uescape(index);
            let decoded = clause
                .map_or(Ok('\\'), |(_, escape)| escape)
                .and_then(|escape| decode_unicode(&body, escape));
            match decoded {
                Ok(text) => {
                    if let Some((end, _)) = clause {
                        let span = self.joined[index].span.union(&self.joined[end].span);
                        self.joined[index].span = span;
                        dropped[index + 1..=end].fill(true);
                    }
                    self.joined[index].token = Token::UnicodeStringLiteral(text);
                }
                Err(message) => {
                    // Point at the escape string when that is at fault.
                    let at = match clause {
                        Some((end, Err(_))) => end,
                        _ => index,
                    };
                    error = Some(TokenizerError {
                        message: message.to_owned(),
                        location: self.joined[at].span.start,
                    });
                    self.joined.truncate(index);
                    dropped.truncate(index);
                    break;
                }
            }
        }
        let tokens = self
            .joined
            .into_iter()
            .zip(dropped)
            .filter_map(|(token, dropped)| (!dropped).then_some(token))
            .collect();
        (tokens, error)
    }

    /// The `UESCAPE 'c'` clause after the constant at `index`: the index of
    /// its last token, and its escape character or why it is not one.
    fn uescape(&self, index: usize) -> Option<(usize, Result<char, &'static str>)> {
        let mut significant = self
            .joined
            .iter()
            .enumerate()
            .skip(index + 1)
            .filter(|(_, token)| !matches!(token.token, Token::Whitespace(_)));
        match significant.next()?.1.token {
            Token::Word(ref word)
                if word.quote_style.is_none() && word.value.eq_ignore_ascii_case("UESCAPE") => {}
            _ => return None,
        }
        match significant.next() {
            Some((
                end,
                TokenWithSpan {
                    token: Token::SingleQuotedString(text) | Token::EscapedStringLiteral(text),
                    ..
                },
            )) => Some((end, escape_character(text))),
            Some((
                end,
                TokenWithSpan {
                    token: Token::DollarQuotedString(text),
                    ..
                },
            )) => Some((end, escape_character(&text.value))),
            // The server rejects a clause without its string; so does the
            // parser once the word is left in place.
            _ => None,
        }
    }
}

/// The prefix that marks the kind of a string constant token.
const fn constant_prefix(token: &Token) -> Option<&'static str> {
    Some(match token {
        Token::SingleQuotedString(_) => "",
        Token::NationalStringLiteral(_) => "N",
        Token::EscapedStringLiteral(_) => "E",
        Token::UnicodeStringLiteral(_) => "U&",
        Token::SingleQuotedByteStringLiteral(_) => "B",
        Token::HexStringLiteral(_) => "X",
        _ => return None,
    })
}

/// The text of a string constant token.
const fn constant_text(token: &mut Token) -> Option<&mut String> {
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

/// Reads `source`, one constant of the same kind as `kind`, with sqlparser.
fn decode_constant(kind: &Token, source: &str) -> Option<String> {
    let mut tokens = Tokenizer::new(&PostgreSqlDialect {}, source)
        .tokenize()
        .ok()?;
    match tokens.as_mut_slice() {
        [token] if std::mem::discriminant(token) == std::mem::discriminant(kind) => {
            constant_text(token).map(std::mem::take)
        }
        _ => None,
    }
}

/// The body of the quoted string opening at byte `start` and the offset just
/// past its closing quote. `''` is a quote; with `escapes`, so is `\'`.
fn read_body(sql: &str, start: usize, escapes: bool) -> Option<(&str, usize)> {
    let body = sql.get(start..)?.strip_prefix('\'')?;
    let mut chars = body.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' if escapes => {
                chars.next();
            }
            '\'' if chars.peek().is_some_and(|&(_, next)| next == '\'') => {
                chars.next();
            }
            '\'' => return Some((&body[..index], start + index + 2)),
            _ => {}
        }
    }
    None
}

/// The character a `UESCAPE` string names, which the server limits to one
/// byte that cannot be mistaken for part of an escape.
fn escape_character(text: &str) -> Result<char, &'static str> {
    match text.as_bytes() {
        &[byte]
            if !(byte.is_ascii_hexdigit()
                || matches!(
                    byte,
                    b'+' | b'\'' | b'"' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c'
                )) =>
        {
            Ok(char::from(byte))
        }
        _ => Err("invalid Unicode escape character"),
    }
}

/// Decodes a `U&'...'` body: `escape` followed by four hex digits, by `+` and
/// six hex digits, or by itself; UTF-16 surrogate pairs combine.
fn decode_unicode(body: &str, escape: char) -> Result<String, &'static str> {
    const SURROGATE: &str = "invalid Unicode surrogate pair";
    let mut text = String::with_capacity(body.len());
    let mut rest = body;
    let mut high: Option<u32> = None;
    while let Some(ch) = rest.chars().next() {
        rest = &rest[ch.len_utf8()..];
        if ch != escape || rest.starts_with(escape) {
            if high.is_some() {
                return Err(SURROGATE);
            }
            if ch == escape {
                rest = &rest[escape.len_utf8()..];
            }
            text.push(ch);
            continue;
        }
        let (digits, width) = rest
            .strip_prefix('+')
            .map_or((rest, 4), |digits| (digits, 6));
        let code = digits
            .get(..width)
            .filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .ok_or("invalid Unicode escape")?;
        rest = &digits[width..];
        let code = match (high.take(), code) {
            (Some(high), 0xDC00..=0xDFFF) => 0x10000 + ((high - 0xD800) << 10) + (code - 0xDC00),
            (Some(_), _) | (None, 0xDC00..=0xDFFF) => return Err(SURROGATE),
            (None, 0xD800..=0xDBFF) => {
                high = Some(code);
                continue;
            }
            (None, code) => code,
        };
        text.push(
            char::from_u32(code)
                .filter(|&ch| ch != '\0')
                .ok_or("invalid Unicode escape value")?,
        );
    }
    if high.is_some() {
        return Err(SURROGATE);
    }
    Ok(text)
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

    fn tokens(sql: &str) -> Vec<TokenWithSpan> {
        let (tokens, error) = tokenize(sql);
        assert_eq!(error, None, "{sql}");
        tokens
    }

    fn literals(sql: &str) -> Vec<String> {
        tokens(sql)
            .into_iter()
            .filter_map(|mut token| constant_text(&mut token.token).map(std::mem::take))
            .collect()
    }

    fn significant(tokens: Vec<TokenWithSpan>) -> Vec<Token> {
        tokens
            .into_iter()
            .map(|token| token.token)
            .filter(|token| !matches!(token, Token::Whitespace(_)))
            .collect()
    }

    fn failure(sql: &str) -> (TokenizerError, Vec<Token>) {
        let (tokens, error) = tokenize(sql);
        (error.expect(sql), significant(tokens))
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
    fn escape_string_continuations_end_where_the_server_ends_them() {
        // `\'` inside the continuation is an escaped quote, not its end.
        assert_eq!(literals("E'a'\n'b\\'c'"), ["ab'c"]);
        assert_eq!(literals("E'x\\\\'\n'\\''"), ["x\\'"]);
        assert_eq!(literals("E'a'\n'b\\'c'\n'd'"), ["ab'cd"]);
        // Read as a plain string this continuation would never end.
        assert_eq!(literals("E'a'\n'\\''"), ["a'"]);
        // Tokens after the continuation keep their place in the source.
        let tokens = tokens("SELECT E'a'\n'b\\'c', 'd';");
        let d = tokens
            .iter()
            .find(|token| token.token == Token::SingleQuotedString("d".into()))
            .unwrap();
        assert_eq!((d.span.start.line, d.span.start.column), (2, 9));
        let literal = &tokens[2];
        assert_eq!(literal.token, Token::EscapedStringLiteral("ab'c".into()));
        assert_eq!((literal.span.end.line, literal.span.end.column), (2, 7));
    }

    #[test]
    fn prefixed_continuations_take_their_head_mode() {
        assert_eq!(literals("U&'a'\n'\\0062'"), ["ab"]);
        assert_eq!(literals("N'a'\n'b'"), ["ab"]);
        assert_eq!(
            significant(tokens("B'10'\n'01' X'1F'\n'2A'")),
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
        let literal = tokens("SELECT 'a'\n  'b';")
            .into_iter()
            .find(|token| matches!(token.token, Token::SingleQuotedString(_)))
            .unwrap();
        assert_eq!((literal.span.start.line, literal.span.start.column), (1, 8));
        assert_eq!((literal.span.end.line, literal.span.end.column), (2, 6));
    }

    #[test]
    fn unicode_escapes_follow_uescape() {
        let dat = [Token::UnicodeStringLiteral("dat".into())];
        assert_eq!(significant(tokens("U&'d!0061t' UESCAPE '!'")), dat);
        assert_eq!(significant(tokens("u&'d!0061t' uescape\n/* c */ '!'")), dat);
        // The clause applies to the whole continued constant.
        assert_eq!(significant(tokens("U&'d!0061'\n't' UESCAPE '!'")), dat);
        assert_eq!(literals("U&'!+01F600!!' UESCAPE '!'"), ["😀!"]);
        // A backslash is an ordinary character under another escape, even
        // where sqlparser would reject it.
        assert_eq!(literals("U&'a\\b!0063' UESCAPE '!'"), ["a\\bc"]);
        // Surrogate pairs combine; escapes may span a continuation.
        assert_eq!(literals("U&'!D83D!DE00' UESCAPE '!'"), ["😀"]);
        assert_eq!(literals("U&'\\D83D\\DE00'"), ["😀"]);
        assert_eq!(literals("U&'\\00'\n'61'"), ["a"]);
        // Doubled quotes collapse, in the head and its continuations.
        assert_eq!(literals("U&'it''s'\n'b''c'"), ["it'sb'c"]);
        // Any string constant names the escape, dollar quoted included.
        assert_eq!(literals("U&'!0061' UESCAPE $$!$$"), ["a"]);
        // The clause is part of the constant's span.
        let literal = &tokens("U&'x' UESCAPE '!';")[0];
        assert_eq!((literal.span.end.line, literal.span.end.column), (1, 18));
    }

    #[test]
    fn invalid_unicode_constants_end_the_tokens() {
        for (sql, message) in [
            ("U&'x' UESCAPE 'a'", "invalid Unicode escape character"),
            ("U&'x' UESCAPE '+'", "invalid Unicode escape character"),
            ("U&'x' UESCAPE ' '", "invalid Unicode escape character"),
            ("U&'x' UESCAPE '\"'", "invalid Unicode escape character"),
            ("U&'x' UESCAPE '!!'", "invalid Unicode escape character"),
            // The server takes one byte, so a multibyte character is out.
            ("U&'é0061' UESCAPE 'é'", "invalid Unicode escape character"),
            // The string after UESCAPE is continued like any other.
            ("U&'x' UESCAPE '!'\n'b'", "invalid Unicode escape character"),
            ("U&'!D83D' UESCAPE '!'", "invalid Unicode surrogate pair"),
            ("U&'!DE00' UESCAPE '!'", "invalid Unicode surrogate pair"),
            ("U&'!0000' UESCAPE '!'", "invalid Unicode escape value"),
            ("U&'!+110000' UESCAPE '!'", "invalid Unicode escape value"),
            ("U&'!zz' UESCAPE '!'", "invalid Unicode escape"),
            ("U&'\\zz'", "invalid Unicode escape"),
        ] {
            let (error, tokens) = failure(&format!("SELECT 1;\nSELECT {sql};"));
            assert_eq!(error.message, message, "{sql}");
            assert_eq!(error.location.line, 2, "{sql}");
            assert_eq!(tokens.last(), Some(&Token::make_keyword("SELECT")), "{sql}");
        }
        // A bad escape string is reported where it is.
        let (error, _) = failure("SELECT U&'x' UESCAPE 'a';");
        assert_eq!(error.location.column, 22);
    }

    #[test]
    fn tokenizer_errors_keep_the_tokens_before_them() {
        let (error, tokens) = failure("SELECT 1;\nSELECT 'unterminated");
        assert_eq!(error.location.line, 2);
        assert_eq!(
            tokens,
            [
                Token::make_keyword("SELECT"),
                Token::Number("1".into(), false),
                Token::SemiColon,
                Token::make_keyword("SELECT"),
            ]
        );
        // An error after a resumed continuation is placed in the whole input.
        let (error, _) = failure("SELECT E'a'\n'b\\'c', 'd\n");
        assert_eq!((error.location.line, error.location.column), (2, 9));
    }
}
