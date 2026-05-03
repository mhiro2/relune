//! SQL dialect detection from input text or token stream.

use relune_core::SqlDialect;
use sqlparser::dialect::{Dialect, GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::tokenizer::{Token, Tokenizer};

/// Detect the SQL dialect from the content of the SQL string.
///
/// Uses token-based heuristics so comments and string literals do not skew the
/// result. Falls back to `PostgreSQL` if no dialect-specific markers are found.
#[must_use]
pub fn detect_dialect(input: &str) -> SqlDialect {
    match Tokenizer::new(&GenericDialect {}, input).tokenize() {
        Ok(tokens) => detect_dialect_from_tokens(&tokens),
        Err(_) => detect_dialect_from_source(input),
    }
}

fn detect_dialect_from_tokens(tokens: &[Token]) -> SqlDialect {
    let significant_tokens = significant_tokens(tokens);

    let mysql_score = score_dialect_signals(&[
        (
            significant_tokens
                .iter()
                .any(|token| is_backtick_identifier(token)),
            2,
        ),
        (contains_word(&significant_tokens, "AUTO_INCREMENT"), 4),
        (contains_word(&significant_tokens, "UNSIGNED"), 3),
        (
            contains_word_sequence(&significant_tokens, &["DEFAULT", "CHARSET"])
                || contains_word_sequence(&significant_tokens, &["CHARACTER", "SET"]),
            3,
        ),
        (contains_word(&significant_tokens, "COLLATE"), 2),
        (contains_word(&significant_tokens, "FULLTEXT"), 2),
        (
            contains_word_sequence(&significant_tokens, &["ON", "UPDATE", "CURRENT_TIMESTAMP"]),
            3,
        ),
    ]);

    let sqlite_score = score_dialect_signals(&[
        (contains_word(&significant_tokens, "AUTOINCREMENT"), 4),
        (
            contains_word_sequence(&significant_tokens, &["WITHOUT", "ROWID"]),
            4,
        ),
        (contains_word(&significant_tokens, "PRAGMA"), 4),
        (
            contains_word_sequence(&significant_tokens, &["INTEGER", "PRIMARY", "KEY"])
                && !contains_word(&significant_tokens, "AUTO_INCREMENT"),
            3,
        ),
        (contains_word(&significant_tokens, "STRICT"), 2),
    ]);

    let pg_score = score_dialect_signals(&[
        (
            contains_word_sequence(&significant_tokens, &["CREATE", "TYPE"])
                && contains_word_sequence(&significant_tokens, &["AS", "ENUM"]),
            4,
        ),
        (
            contains_word(&significant_tokens, "SERIAL")
                || contains_word(&significant_tokens, "BIGSERIAL"),
            3,
        ),
        (
            contains_word_sequence(&significant_tokens, &["COMMENT", "ON"]),
            4,
        ),
        (
            contains_word_sequence(&significant_tokens, &["CREATE", "EXTENSION"]),
            4,
        ),
        (
            contains_word_sequence(&significant_tokens, &["CREATE", "SEQUENCE"]),
            4,
        ),
        (
            significant_tokens
                .iter()
                .any(|token| matches!(token, Token::DoubleColon)),
            3,
        ),
        (contains_word(&significant_tokens, "RETURNING"), 2),
        (contains_word(&significant_tokens, "ILIKE"), 2),
    ]);

    if mysql_score > sqlite_score && mysql_score > pg_score {
        SqlDialect::Mysql
    } else if sqlite_score > mysql_score && sqlite_score > pg_score {
        SqlDialect::Sqlite
    } else {
        SqlDialect::Postgres
    }
}

fn detect_dialect_from_source(input: &str) -> SqlDialect {
    let upper = input.to_uppercase();

    let mysql_score = score_dialect_signals(&[
        (upper.contains("ENGINE=") || upper.contains("ENGINE ="), 4),
        (upper.contains("AUTO_INCREMENT"), 4),
        (upper.contains("UNSIGNED"), 3),
        (
            upper.contains("DEFAULT CHARSET") || upper.contains("CHARACTER SET"),
            3,
        ),
        (upper.contains("COLLATE=") || upper.contains("COLLATE "), 2),
        (upper.contains("FULLTEXT"), 2),
        (upper.contains("ON UPDATE CURRENT_TIMESTAMP"), 3),
        (input.contains('`'), 2),
    ]);

    let sqlite_score = score_dialect_signals(&[
        (upper.contains("AUTOINCREMENT"), 4),
        (upper.contains("WITHOUT ROWID"), 4),
        (upper.contains("PRAGMA"), 4),
        (
            upper.contains("INTEGER PRIMARY KEY") && !upper.contains("AUTO_INCREMENT"),
            3,
        ),
        (upper.contains("STRICT"), 2),
    ]);

    let pg_score = score_dialect_signals(&[
        (
            upper.contains("CREATE TYPE") && upper.contains("AS ENUM"),
            4,
        ),
        (upper.contains("SERIAL") || upper.contains("BIGSERIAL"), 3),
        (upper.contains("COMMENT ON"), 4),
        (upper.contains("CREATE EXTENSION"), 4),
        (upper.contains("CREATE SEQUENCE"), 4),
        (upper.contains("::"), 3),
        (upper.contains("RETURNING"), 2),
        (upper.contains("ILIKE"), 2),
    ]);

    if mysql_score > sqlite_score && mysql_score > pg_score {
        SqlDialect::Mysql
    } else if sqlite_score > mysql_score && sqlite_score > pg_score {
        SqlDialect::Sqlite
    } else {
        SqlDialect::Postgres
    }
}

fn significant_tokens(tokens: &[Token]) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|token| !matches!(token, Token::Whitespace(_)))
        .collect()
}

fn contains_word(tokens: &[&Token], expected: &str) -> bool {
    tokens.iter().copied().any(|token| is_word(token, expected))
}

fn contains_word_sequence(tokens: &[&Token], sequence: &[&str]) -> bool {
    if sequence.is_empty() || tokens.len() < sequence.len() {
        return false;
    }

    tokens.windows(sequence.len()).any(|window| {
        window
            .iter()
            .zip(sequence)
            .all(|(token, expected)| is_word(token, expected))
    })
}

fn is_word(token: &Token, expected: &str) -> bool {
    matches!(token, Token::Word(word) if word.value.eq_ignore_ascii_case(expected))
}

fn is_backtick_identifier(token: &Token) -> bool {
    matches!(token, Token::Word(word) if word.quote_style == Some('`'))
}

fn score_dialect_signals(signals: &[(bool, u8)]) -> u32 {
    signals
        .iter()
        .filter_map(|(matched, weight)| matched.then_some(u32::from(*weight)))
        .sum()
}

/// Resolve `SqlDialect::Auto` to a concrete dialect by detecting from SQL content.
pub(crate) fn resolve_dialect(dialect: SqlDialect, input: &str) -> SqlDialect {
    match dialect {
        SqlDialect::Auto => detect_dialect(input),
        other => other,
    }
}

/// Get the sqlparser `Dialect` implementation for a given `SqlDialect`.
pub(crate) fn dialect_impl(dialect: SqlDialect) -> Box<dyn Dialect> {
    match dialect {
        SqlDialect::Postgres | SqlDialect::Auto => Box::new(PostgreSqlDialect {}),
        SqlDialect::Mysql => Box::new(MySqlDialect {}),
        SqlDialect::Sqlite => Box::new(SQLiteDialect {}),
    }
}
