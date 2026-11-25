//! Statement-level parsing with error recovery.

use crate::context::{LineOffsets, ParseContext, source_span_from_sql_span};
use crate::dialect::{Rescue, complete_statements, rescue, tokenize};
use crate::pg_dump::{self, Rescued};
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
    // Statements the stock grammar rejects get a second, dialect-specific
    // reading; a rescue grammar walks the same tokens in step.
    let mut rescuer = rescue(dialect).map(|rescue| match rescue {
        Rescue::Grammar(grammar) => {
            Rescuer::Grammar(Parser::new(grammar).with_tokens_with_locations(tokens.clone()))
        }
        Rescue::PgDump => Rescuer::PgDump(dialect),
    });
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
                Comment::Applied(statement) => Rescued::Statement(statement),
                Comment::Ignored { object, span } => Rescued::Skipped {
                    construct: format!("COMMENT ON {object}"),
                    span,
                },
            })
        } else {
            match rescuer.as_mut() {
                Some(rescuer) => parse_with_rescue(&mut parser, rescuer),
                None => parser
                    .parse_statement()
                    .map(|statement| Rescued::Statement(Box::new(statement))),
            }
        };
        match parsed {
            Ok(Rescued::Statement(stmt)) => {
                statements.push(*stmt);
            }
            Ok(Rescued::Skipped { construct, span }) => {
                ctx.warn_unsupported(&construct, source_span_from_sql_span(input, offsets, span));
            }
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

/// The second reading of statements the stock grammar rejects.
enum Rescuer<'a> {
    /// A rescue grammar's parser, kept in step with the stock one.
    Grammar(Parser<'a>),
    /// `pg_dump` forms, re-read with the stock grammar.
    PgDump(&'a dyn Dialect),
}

/// Parses the next statement with the stock grammar and, only if that fails,
/// with `rescuer`, so the rescue never changes how an accepted statement is
/// read.
fn parse_with_rescue(
    parser: &mut Parser<'_>,
    rescuer: &mut Rescuer<'_>,
) -> Result<Rescued, ParserError> {
    let start = parser.index();
    if let Ok(statement) = parser.try_parse(parse_whole_statement) {
        return Ok(Rescued::Statement(Box::new(statement)));
    }
    let second_reading = match rescuer {
        Rescuer::Grammar(rescue) => {
            while rescue.index() < start {
                rescue.next_token_no_skip();
            }
            rescue
                .try_parse(parse_whole_statement)
                .ok()
                .map(|statement| {
                    while parser.index() < rescue.index() {
                        parser.next_token_no_skip();
                    }
                    Rescued::Statement(Box::new(statement))
                })
        }
        Rescuer::PgDump(dialect) => pg_dump::rescue(parser, *dialect),
    };
    // Otherwise read it as the stock grammar does: report its error at the
    // position it stopped, or keep the statement it read from the start.
    second_reading.map_or_else(
        || {
            parser
                .parse_statement()
                .map(|statement| Rescued::Statement(Box::new(statement)))
        },
        Ok,
    )
}

/// Parses a statement that must end at a `;` or the end of input.
///
/// The stock grammar can accept a statement's start and stop before a clause
/// it does not know, such as `WITH NO DATA` after a materialized view's
/// query; such a statement is rejected here so the rescue sees all of it.
pub(crate) fn parse_whole_statement(parser: &mut Parser<'_>) -> Result<Statement, ParserError> {
    let statement = parser.parse_statement()?;
    let next = parser.peek_token();
    match next.token {
        Token::SemiColon | Token::EOF => Ok(statement),
        _ => parser.expected("end of statement", next),
    }
}
