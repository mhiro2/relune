//! `SQLite` syntax that sqlparser's `SQLiteDialect` rejects.
//!
//! [`SqliteRescue`] only re-reads statements the stock dialect fails on (see
//! `recovery`), so a statement the stock grammar accepts is never parsed by
//! this module. Everything else is delegated to `SQLiteDialect` and reports
//! its identity, so a rescued statement is otherwise read the same way.

use std::any::TypeId;

use sqlparser::ast::{
    BinaryOperator, DataType, Expr, Ident, ObjectName, Statement, TypeModifier, Value,
    ValueWithSpan,
};
use sqlparser::dialect::{BindPlaceholderStyle, Dialect, Precedence, SQLiteDialect};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Span, Token};

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

    fn parse_data_type(&self, parser: &mut Parser<'_>) -> Option<Result<DataType, ParserError>> {
        SQLiteDialect {}
            .parse_data_type(parser)
            .or_else(|| parse_multi_word_type(parser))
    }

    fn get_next_precedence(&self, parser: &Parser<'_>) -> Option<Result<u8, ParserError>> {
        if is_bare_word(&parser.peek_token().token, "ISNULL") {
            return Some(Ok(self.prec_value(Precedence::Is)));
        }
        SQLiteDialect {}.get_next_precedence(parser)
    }

    fn parse_infix(
        &self,
        parser: &mut Parser<'_>,
        expr: &Expr,
        precedence: u8,
    ) -> Option<Result<Expr, ParserError>> {
        if is_bare_word(&parser.peek_token().token, "ISNULL") {
            parser.advance_token();
            return Some(Ok(Expr::IsNull(Box::new(expr.clone()))));
        }
        for operator in [Keyword::GLOB, Keyword::MATCH] {
            if parser.parse_keywords(&[Keyword::NOT, operator]) {
                // The AST has no negated GLOB or MATCH; keep the spelling.
                return Some(
                    parser
                        .parse_subexpr(precedence)
                        .map(|right| Expr::BinaryOp {
                            left: Box::new(expr.clone()),
                            op: BinaryOperator::Custom(format!("NOT {operator}")),
                            right: Box::new(right),
                        }),
                );
            }
        }
        SQLiteDialect {}.parse_infix(parser, expr, precedence)
    }
}

fn is_bare_word(token: &Token, expected: &str) -> bool {
    matches!(token, Token::Word(word) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case(expected))
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

/// Words that end a column's type name and start its constraints.
const COLUMN_CONSTRAINT_WORDS: &[Keyword] = &[
    Keyword::AS,
    Keyword::CHECK,
    Keyword::COLLATE,
    Keyword::CONSTRAINT,
    Keyword::DEFAULT,
    Keyword::DEFERRABLE,
    Keyword::GENERATED,
    Keyword::NOT,
    Keyword::NULL,
    Keyword::PRIMARY,
    Keyword::REFERENCES,
    Keyword::UNIQUE,
];

/// A type name of several words, such as `UNSIGNED BIG INT` or
/// `VARYING CHARACTER(255)`. `SQLite` accepts any sequence of names (with up
/// to two signed-number modifiers) and derives the column affinity from it.
fn parse_multi_word_type(parser: &mut Parser<'_>) -> Option<Result<DataType, ParserError>> {
    let word_at = |n| match parser.peek_nth_token(n).token {
        Token::Word(word)
            if word.quote_style.is_none() && !COLUMN_CONSTRAINT_WORDS.contains(&word.keyword) =>
        {
            Some(word.value)
        }
        _ => None,
    };
    let words: Vec<String> = (0..).map_while(word_at).collect();
    if words.len() < 2 {
        return None;
    }
    for _ in &words {
        parser.advance_token();
    }
    let modifiers = match parse_type_modifiers(parser) {
        Ok(modifiers) => modifiers,
        Err(error) => return Some(Err(error)),
    };
    let name = ObjectName::from(vec![Ident::with_span(Span::empty(), words.join(" "))]);
    Some(Ok(DataType::Custom(name, modifiers)))
}

fn parse_type_modifiers(parser: &mut Parser<'_>) -> Result<Vec<TypeModifier>, ParserError> {
    if !parser.consume_token(&Token::LParen) {
        return Ok(Vec::new());
    }
    let mut modifiers = Vec::new();
    // SQLite takes one or two signed numbers.
    while modifiers.len() < 2 {
        let sign = if parser.consume_token(&Token::Minus) {
            "-"
        } else {
            // A leading `+` changes nothing.
            let _ = parser.consume_token(&Token::Plus);
            ""
        };
        let next = parser.next_token();
        let Token::Number(digits, long) = next.token else {
            return parser.expected("type size", next);
        };
        modifiers.push(TypeModifier::Number(format!("{sign}{digits}"), long));
        if !parser.consume_token(&Token::Comma) {
            break;
        }
    }
    parser.expect_token(&Token::RParen)?;
    Ok(modifiers)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Method names defined in `source`'s `impl Dialect for <ty>` block,
    /// including the ones generated by `delegate_flags!`.
    fn dialect_methods(source: &str, ty: &str) -> BTreeSet<String> {
        let header = format!("\nimpl Dialect for {ty} {{\n");
        let start = source.find(&header).expect("dialect impl") + header.len();
        let body = &source[start..start + source[start..].find("\n}\n").expect("impl end")];
        let defined = body
            .lines()
            .filter_map(|line| line.strip_prefix("    fn "))
            .map(|rest| rest.split(['(', '<']).next().unwrap_or(rest).to_owned());
        let delegated = body
            .split("delegate_flags!(")
            .skip(1)
            .flat_map(|list| list[..list.find(')').unwrap_or(0)].split(','))
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        defined.chain(delegated).collect()
    }

    /// The source of the `sqlparser` package this crate is built against.
    fn sqlparser_source(file: &str) -> String {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let output = Command::new(cargo)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--manifest-path",
            ])
            .arg(&manifest)
            .output()
            .expect("run cargo metadata");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("cargo metadata JSON");
        // The sqlparser this crate depends on, by package id.
        let nodes = metadata["resolve"]["nodes"].as_array().expect("resolve");
        let node = nodes
            .iter()
            .find(|node| {
                node["id"]
                    .as_str()
                    .is_some_and(|id| id.contains("relune-parser-sql"))
            })
            .expect("relune-parser-sql node");
        let dependency = node["deps"]
            .as_array()
            .expect("deps")
            .iter()
            .find(|dep| dep["name"] == "sqlparser")
            .and_then(|dep| dep["pkg"].as_str())
            .expect("sqlparser dependency");
        let package = metadata["packages"]
            .as_array()
            .expect("packages")
            .iter()
            .find(|package| package["id"] == dependency)
            .expect("sqlparser package");
        let path = PathBuf::from(package["manifest_path"].as_str().expect("manifest path"))
            .with_file_name("src")
            .join(file);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    /// `SqliteRescue` lists what it delegates by hand, so a method
    /// `SQLiteDialect` starts overriding after a sqlparser bump would silently
    /// fall back to the trait default in rescued statements.
    #[test]
    fn rescue_covers_every_method_sqlite_dialect_overrides() {
        let stock = dialect_methods(&sqlparser_source("dialect/sqlite.rs"), "SQLiteDialect");
        let rescue = dialect_methods(include_str!("sqlite.rs"), "SqliteRescue");
        assert!(
            stock.len() > 10,
            "SQLiteDialect methods not found: {stock:?}"
        );
        let missing: Vec<_> = stock.difference(&rescue).collect();
        assert!(
            missing.is_empty(),
            "SqliteRescue must delegate or override these SQLiteDialect methods: {missing:?}"
        );
    }
}
