//! Statement-level parsing with error recovery.

use crate::context::{LineOffsets, ParseContext, source_span_from_sql_span};
use crate::dialect::{complete_statements, tokenize};
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
    let mut parser = Parser::new(dialect).with_tokens_with_locations(tokens);

    let postgres = dialect.is::<PostgreSqlDialect>();
    let mut statements = Vec::new();

    loop {
        // Skip empty statements (consecutive semicolons)
        while parser.consume_token(&Token::SemiColon) {}

        if parser.peek_token().token == Token::EOF {
            break;
        }

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
            parser.parse_statement().map(Some)
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

                // Skip tokens until the next semicolon or EOF for recovery
                loop {
                    let tok = parser.next_token();
                    if matches!(tok.token, Token::SemiColon | Token::EOF) {
                        break;
                    }
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
