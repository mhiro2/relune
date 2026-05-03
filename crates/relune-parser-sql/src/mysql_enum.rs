//! `MySQL` inline `ENUM(...)` / `SET(...)` parsing and column population.

use crate::context::ParseContext;
use relune_core::{Diagnostic, Table, diagnostic::codes};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum MySqlEnumLikeParseError {
    #[error("expected a quoted enum/set value")]
    ExpectedQuotedValue,
    #[error("enum/set value ended with an incomplete escape sequence")]
    TrailingEscapeSequence,
    #[error("enum/set value is missing a closing quote")]
    UnterminatedQuotedValue,
    #[error("enum/set definition is missing a closing parenthesis")]
    MissingClosingParenthesis,
    #[error("enum/set definition contains an unexpected separator")]
    UnexpectedSeparator,
}

fn parse_mysql_enum_like_value(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, MySqlEnumLikeParseError> {
    if chars.next() != Some('\'') {
        return Err(MySqlEnumLikeParseError::ExpectedQuotedValue);
    }

    let mut value = String::new();
    loop {
        match chars.next() {
            Some('\'') => {
                if chars.peek() == Some(&'\'') {
                    value.push('\'');
                    chars.next();
                } else {
                    break;
                }
            }
            Some('\\') => {
                let Some(escaped) = chars.next() else {
                    return Err(MySqlEnumLikeParseError::TrailingEscapeSequence);
                };
                match escaped {
                    '\\' | '\'' => value.push(escaped),
                    other => {
                        value.push('\\');
                        value.push(other);
                    }
                }
            }
            Some(c) => value.push(c),
            None => return Err(MySqlEnumLikeParseError::UnterminatedQuotedValue),
        }
    }

    Ok(value)
}

pub(crate) fn parse_mysql_enum_like_type(
    data_type: &str,
) -> Result<Option<(String, Vec<String>)>, MySqlEnumLikeParseError> {
    let Some(start) = data_type.find('(') else {
        return Ok(None);
    };
    let Some(end) = data_type.rfind(')') else {
        return Err(MySqlEnumLikeParseError::MissingClosingParenthesis);
    };
    let kind = data_type[..start].trim();
    if !kind.eq_ignore_ascii_case("enum") && !kind.eq_ignore_ascii_case("set") {
        return Ok(None);
    }
    if start.saturating_add(1) > end {
        return Ok(None);
    }

    let mut values = Vec::new();
    let mut chars = data_type[start + 1..end].chars().peekable();

    while chars.peek().is_some() {
        while chars.peek().is_some_and(char::is_ascii_whitespace) {
            chars.next();
        }

        values.push(parse_mysql_enum_like_value(&mut chars)?);

        while chars.peek().is_some_and(char::is_ascii_whitespace) {
            chars.next();
        }

        match chars.peek() {
            Some(',') => {
                chars.next();
            }
            None => break,
            _ => return Err(MySqlEnumLikeParseError::UnexpectedSeparator),
        }
    }

    Ok(Some((kind.to_ascii_lowercase(), values)))
}

fn serialize_mysql_enum_like_type(kind: &str, values: &[String]) -> String {
    let serialized_values = values
        .iter()
        .map(|value| {
            let escaped = value.replace('\\', "\\\\").replace('\'', "''");
            format!("'{escaped}'")
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{kind}({serialized_values})")
}

pub(crate) fn canonicalize_mysql_enum_like_type(
    data_type: &str,
) -> Result<Option<String>, MySqlEnumLikeParseError> {
    Ok(parse_mysql_enum_like_type(data_type)?
        .map(|(kind, values)| serialize_mysql_enum_like_type(&kind, &values)))
}

/// Populates `Column::enum_values` for `MySQL` inline `ENUM(...)` / `SET(...)`
/// columns, and emits warnings for malformed definitions.
///
/// `MySQL` inline enums/sets are anonymous, per-column value lists rather than
/// named types, so they belong on the `Column` itself instead of being lifted
/// into `Schema::enums` under a synthetic, non-identifier name like
/// `enum('a','b')`.
pub(crate) fn populate_mysql_enum_columns(ctx: &mut ParseContext, tables: &mut [Table]) {
    for table in tables.iter_mut() {
        let qualified_name = table.qualified_name();
        for column in &mut table.columns {
            match parse_mysql_enum_like_type(&column.data_type) {
                Ok(Some((_kind, values))) => {
                    column.enum_values = Some(values);
                }
                Ok(None) => {}
                Err(error) => {
                    ctx.diagnostics.push(Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!(
                            "Malformed MySQL enum/set definition on {}.{}: {} ({error})",
                            qualified_name, column.name, column.data_type
                        ),
                    ));
                }
            }
        }
    }
}
