//! `SQLite` syntax that sqlparser's `SQLiteDialect` rejects.
//!
//! [`SqliteRescue`] only re-reads statements the stock dialect fails on (see
//! `recovery`), so a statement the stock grammar accepts is never parsed by
//! this module. Everything else is delegated to `SQLiteDialect` and reports
//! its identity, so a rescued statement is otherwise read the same way.

use std::any::TypeId;

use sqlparser::ast::{Statement, Value, ValueWithSpan};
use sqlparser::dialect::{BindPlaceholderStyle, Dialect, Precedence, SQLiteDialect};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::Token;

/// `SQLiteDialect` plus the `SQLite` forms it rejects.
#[derive(Debug)]
pub(crate) struct SqliteRescue;

macro_rules! delegate_flags {
    ($($name:ident),* $(,)?) => {
        $(fn $name(&self) -> bool {
            SQLiteDialect {}.$name()
        })*
    };
}

impl Dialect for SqliteRescue {
    fn dialect(&self) -> TypeId {
        SQLiteDialect {}.dialect()
    }

    delegate_flags!(
        supports_filter_during_aggregation,
        supports_start_transaction_modifier,
        supports_in_empty_list,
        supports_limit_comma,
        supports_asc_desc_in_column_definition,
        supports_dollar_placeholder,
        supports_notnull_operator,
        supports_is_operator,
        supports_comma_separated_trim,
        supports_numeric_literal_underscores,
        supports_double_eq_assignment,
        supports_string_literal_column_names,
        supports_cast_empty_data_type_to_unspecified,
        supports_national_string_literal,
        supports_parens_around_table_factor,
        supports_unterminated_block_comments,
        supports_over_keyword_as_column_alias,
        supports_indexed_by,
        supports_comma_join_constraint,
        supports_aggregate_order_by_with_empty_args,
        supports_bitwise_shift_operators,
        supports_in_table_expression,
    );

    fn is_delimited_identifier_start(&self, ch: char) -> bool {
        SQLiteDialect {}.is_delimited_identifier_start(ch)
    }

    fn identifier_quote_style(&self, identifier: &str) -> Option<char> {
        SQLiteDialect {}.identifier_quote_style(identifier)
    }

    fn ordered_bind_placeholder_style(&self) -> Option<BindPlaceholderStyle> {
        SQLiteDialect {}.ordered_bind_placeholder_style()
    }

    fn is_identifier_start(&self, ch: char) -> bool {
        SQLiteDialect {}.is_identifier_start(ch)
    }

    fn is_identifier_part(&self, ch: char) -> bool {
        SQLiteDialect {}.is_identifier_part(ch)
    }

    fn prec_value(&self, prec: Precedence) -> u8 {
        SQLiteDialect {}.prec_value(prec)
    }

    fn between_low_bound_precedence(&self) -> u8 {
        SQLiteDialect {}.between_low_bound_precedence()
    }

    fn is_column_alias(&self, kw: &Keyword, parser: &mut Parser<'_>) -> bool {
        SQLiteDialect {}.is_column_alias(kw, parser)
    }

    fn is_table_alias(&self, kw: &Keyword, parser: &mut Parser<'_>) -> bool {
        SQLiteDialect {}.is_table_alias(kw, parser)
    }

    fn is_table_factor(&self, kw: &Keyword, parser: &mut Parser<'_>) -> bool {
        SQLiteDialect {}.is_table_factor(kw, parser)
    }

    fn parse_statement(&self, parser: &mut Parser<'_>) -> Option<Result<Statement, ParserError>> {
        if parser.parse_keyword(Keyword::PRAGMA) {
            return Some(parse_pragma(parser));
        }
        if let Some(temporary) = trigger_if_not_exists(parser) {
            // The statement has no field for IF NOT EXISTS; the schema skips
            // triggers either way.
            return Some(
                parser
                    .parse_create_trigger(temporary, false, false, false)
                    .map(Statement::CreateTrigger),
            );
        }
        SQLiteDialect {}.parse_statement(parser)
    }
}

/// `PRAGMA [schema.]name [= value | (value)]`, where `SQLite` also accepts a
/// signed number or a bare word (`ON`, `WAL`, `NORMAL`, `true`, ...).
fn parse_pragma(parser: &mut Parser<'_>) -> Result<Statement, ParserError> {
    let name = parser.parse_object_name(false)?;
    let (value, is_eq) = if parser.consume_token(&Token::Eq) {
        (Some(parse_pragma_value(parser)?), true)
    } else if parser.consume_token(&Token::LParen) {
        let value = parse_pragma_value(parser)?;
        parser.expect_token(&Token::RParen)?;
        (Some(value), false)
    } else {
        (None, false)
    };
    Ok(Statement::Pragma { name, value, is_eq })
}

fn parse_pragma_value(parser: &mut Parser<'_>) -> Result<ValueWithSpan, ParserError> {
    let next = parser.next_token();
    let value = match &next.token {
        Token::Minus | Token::Plus => {
            let number = parser.next_token();
            let Token::Number(digits, long) = number.token else {
                return parser.expected("number", number);
            };
            let sign = if next.token == Token::Minus { "-" } else { "" };
            return Ok(Value::Number(format!("{sign}{digits}"), long)
                .with_span(next.span.union(&number.span)));
        }
        Token::Number(digits, long) => Value::Number(digits.clone(), *long),
        Token::SingleQuotedString(text) => Value::SingleQuotedString(text.clone()),
        Token::DoubleQuotedString(text) => Value::DoubleQuotedString(text.clone()),
        Token::Word(word) => Value::SingleQuotedString(word.value.clone()),
        _ => return parser.expected("PRAGMA value", next),
    };
    Ok(value.with_span(next.span))
}

/// Consumes `CREATE [TEMP | TEMPORARY] TRIGGER IF NOT EXISTS`, returning
/// whether the trigger is temporary, or consumes nothing.
fn trigger_if_not_exists(parser: &mut Parser<'_>) -> Option<bool> {
    let keyword = |n| match parser.peek_nth_token(n).token {
        Token::Word(word) => word.keyword,
        _ => Keyword::NoKeyword,
    };
    if keyword(0) != Keyword::CREATE {
        return None;
    }
    let temporary = matches!(keyword(1), Keyword::TEMP | Keyword::TEMPORARY);
    let at = 1 + usize::from(temporary);
    let form = [Keyword::TRIGGER, Keyword::IF, Keyword::NOT, Keyword::EXISTS];
    if !form
        .iter()
        .enumerate()
        .all(|(n, expected)| keyword(at + n) == *expected)
    {
        return None;
    }
    for _ in 0..at + form.len() {
        parser.advance_token();
    }
    Some(temporary)
}
