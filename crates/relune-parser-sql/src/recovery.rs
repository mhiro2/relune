//! Statement-level parsing with error recovery.

use crate::context::{LineOffsets, ParseContext, source_span_from_sql_span};
use crate::dialect::{complete_statements, rescue_dialect, tokenize};
use crate::postgres::{Comment, parse_comment};
use relune_core::{Diagnostic, diagnostic::codes};
use sqlparser::ast::Statement;
use sqlparser::dialect::{Dialect, PostgreSqlDialect};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Span, Token};

/// Parse SQL statements with error recovery.
///
/// Instead of using `Parser::parse_sql` which aborts on the first error,
/// this function parses statement-by-statement and skips to the next
/// semicolon on error, allowing subsequent statements to be parsed.
///
/// Recovery is semicolon-delimited: on a parse error the parser discards tokens
/// up to the next `;`. A malformed statement that is **not** terminated by a
/// semicolon therefore consumes the following statement(s) up to the next
/// semicolon, so inputs should keep statements semicolon-terminated for each
/// one to recover independently. A tokenizer error ends the input: the
/// statements before the last `;` ahead of it are parsed, and the rest is
/// reported as one error.
pub(crate) fn parse_statements_with_recovery(
    dialect: &dyn Dialect,
    input: &str,
    offsets: &LineOffsets,
    ctx: &mut ParseContext,
) -> Vec<Statement> {
    // A tokenizer error leaves nothing readable after it, but the statements
    // that end before it are still parsed.
    let (tokens, tokenizer_error) = tokenize(dialect, input);
    let tokens = complete_statements(tokens, tokenizer_error.as_ref());
    // Statements the stock grammar rejects get a second reading from a
    // dialect-specific rescue grammar over the same tokens.
    let mut rescue = rescue_dialect(dialect)
        .map(|rescue| Parser::new(rescue).with_tokens_with_locations(tokens.clone()));
    let mut parser = Parser::new(dialect).with_tokens_with_locations(tokens);

    let postgres = dialect.is::<PostgreSqlDialect>();
    let mut statements = Vec::new();

    loop {
        // Skip empty statements (consecutive semicolons)
        while parser.consume_token(&Token::SemiColon) {}

        if parser.peek_token().token == Token::EOF {
            break;
        }

        let start = parser.index();
        let parsed = if postgres && parser.peek_keyword(Keyword::COMMENT) {
            parse_comment(&mut parser).map(|comment| match comment {
                Comment::Applied(statement) => Some(*statement),
                Comment::Ignored { object, span } => {
                    ctx.warn_unsupported(
                        &format!("COMMENT ON {object}"),
                        source_span_from_sql_span(input, offsets, span),
                    );
                    None
                }
            })
        } else {
            match rescue.as_mut() {
                Some(rescue) => parse_with_rescue(&mut parser, rescue),
                None => parser.parse_statement(),
            }
            .map(Some)
        };
        match parsed {
            Ok(Some(stmt)) => {
                statements.push(stmt);
            }
            Ok(None) => {}
            Err(e) => {
                let error_msg = format!("SQL parse error: {e}");
                // Try to extract location from the error token's current position
                let span = {
                    let tok = parser.peek_token();
                    source_span_from_sql_span(input, offsets, tok.span)
                };
                let mut diagnostic = Diagnostic::error(codes::parse_error(), error_msg);
                if let Some(span) = span {
                    diagnostic = diagnostic.with_span(span);
                }
                ctx.diagnostics.push(diagnostic);

                // Skip tokens until the next semicolon or EOF for recovery,
                // unless the failed parse already consumed its semicolon:
                // skipping on would swallow the following statement.
                let consumed_terminator =
                    parser.index() > start && parser.get_current_token().token == Token::SemiColon;
                if !consumed_terminator {
                    while !matches!(parser.next_token().token, Token::SemiColon | Token::EOF) {}
                }
            }
        }
    }

    if let Some(error) = tokenizer_error {
        let location = error.location;
        ctx.diagnostics.push(
            Diagnostic::error(
                codes::parse_error(),
                format!("SQL parse error: {}", ParserError::from(error)),
            )
            .with_span_opt(source_span_from_sql_span(
                input,
                offsets,
                Span::new(location, location),
            )),
        );
    }

    statements
}

/// Parses the next statement with the stock grammar and, only if that fails,
/// with `rescue`, so the rescue grammar never changes how an accepted
/// statement is read. Both parsers walk the same tokens and are kept in step.
// The closures are needed for try_parse's higher-ranked parser lifetime.
#[allow(clippy::redundant_closure_for_method_calls)]
fn parse_with_rescue(
    parser: &mut Parser<'_>,
    rescue: &mut Parser<'_>,
) -> Result<Statement, ParserError> {
    let start = parser.index();
    if let Ok(statement) = parser.try_parse(|parser| parser.parse_statement()) {
        return Ok(statement);
    }
    while rescue.index() < start {
        rescue.next_token_no_skip();
    }
    match rescue.try_parse(|parser| parser.parse_statement()) {
        Ok(statement) => {
            while parser.index() < rescue.index() {
                parser.next_token_no_skip();
            }
            Ok(statement)
        }
        // Report the stock grammar's error at the position it stopped.
        Err(_) => parser.parse_statement(),
    }
}
