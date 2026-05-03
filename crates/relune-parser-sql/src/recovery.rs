//! Statement-level parsing with error recovery.

use crate::context::{LineOffsets, ParseContext, source_span_from_sql_span};
use relune_core::{Diagnostic, diagnostic::codes};
use sqlparser::ast::Statement;
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::Token;

/// Parse SQL statements with error recovery.
///
/// Instead of using `Parser::parse_sql` which aborts on the first error,
/// this function parses statement-by-statement and skips to the next
/// semicolon on error, allowing subsequent statements to be parsed.
pub(crate) fn parse_statements_with_recovery(
    dialect: &dyn Dialect,
    input: &str,
    offsets: &LineOffsets,
    ctx: &mut ParseContext,
) -> Vec<Statement> {
    // First, try the fast path: parse all at once
    let mut parser = match Parser::new(dialect).try_with_sql(input) {
        Ok(p) => p,
        Err(e) => {
            // Tokenizer error — nothing can be parsed
            ctx.diagnostics.push(Diagnostic::error(
                codes::parse_error(),
                format!("SQL parse error: {e}"),
            ));
            return Vec::new();
        }
    };

    let mut statements = Vec::new();

    loop {
        // Skip empty statements (consecutive semicolons)
        while parser.consume_token(&Token::SemiColon) {}

        if parser.peek_token().token == Token::EOF {
            break;
        }

        match parser.parse_statement() {
            Ok(stmt) => {
                statements.push(stmt);
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

    statements
}
