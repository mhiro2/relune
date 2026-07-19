//! `MySQL` `information_schema` catalog queries.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::MySqlPool;
use thiserror::Error;
use tracing::warn;

use crate::catalog::ParallelCatalogReader;
use crate::common::{
    RawCheckConstraint, RawColumn, RawEnum, RawForeignKey, RawIndex, RawIndexKeyPart, RawSchema,
    RawTable, RawView, parse_referential_action,
};
use crate::connect::pool_max_connections_with_default;
use crate::error::IntrospectError;

const PARALLEL_CATALOG_QUERIES: u32 = 6;

/// Fetches all catalog metadata from a `MySQL` database.
pub async fn fetch_catalog_metadata(pool: &MySqlPool) -> Result<RawSchema, IntrospectError> {
    let catalog = MySqlCatalog { pool: pool.clone() };
    catalog.fetch_all().await
}

struct MySqlCatalog {
    pool: MySqlPool,
}

impl ParallelCatalogReader for MySqlCatalog {
    async fn fetch_tables(&self) -> Result<Vec<RawTable>, IntrospectError> {
        let rows: Vec<RawTableRow> = sqlx::query_as(
            r"
            SELECT
                CONVERT(TABLE_SCHEMA USING utf8mb4) AS schema_name,
                CONVERT(TABLE_NAME USING utf8mb4) AS table_name,
                NULLIF(CONVERT(TABLE_COMMENT USING utf8mb4), '') AS table_comment
            FROM information_schema.TABLES
            WHERE TABLE_TYPE = 'BASE TABLE'
              AND TABLE_SCHEMA NOT IN (
                  'information_schema', 'mysql', 'performance_schema', 'sys',
                  'mysql_innodb_cluster_metadata'
              )
            ORDER BY TABLE_SCHEMA, TABLE_NAME
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch tables", e))?;

        Ok(rows
            .into_iter()
            .map(|row| RawTable {
                table_name: row.table_name,
                schema_name: row.schema_name,
                table_comment: row.table_comment,
            })
            .collect())
    }

    async fn fetch_columns(&self) -> Result<Vec<RawColumn>, IntrospectError> {
        let rows: Vec<RawColumnRow> = sqlx::query_as(
            r"
            SELECT
                CONVERT(TABLE_SCHEMA USING utf8mb4) AS schema_name,
                CONVERT(TABLE_NAME USING utf8mb4) AS table_name,
                CONVERT(COLUMN_NAME USING utf8mb4) AS column_name,
                CONVERT(COLUMN_TYPE USING utf8mb4) AS data_type,
                IF(IS_NULLABLE = 'YES', TRUE, FALSE) AS is_nullable,
                IF(COLUMN_KEY = 'PRI', TRUE, FALSE) AS is_primary_key,
                NULLIF(CONVERT(COLUMN_COMMENT USING utf8mb4), '') AS column_comment,
                ORDINAL_POSITION AS ordinal_position,
                CONVERT(COLUMN_DEFAULT USING utf8mb4) AS column_default,
                CONVERT(EXTRA USING utf8mb4) AS extra,
                NULLIF(CONVERT(GENERATION_EXPRESSION USING utf8mb4), '') AS generation_expression
            FROM information_schema.COLUMNS
            WHERE TABLE_SCHEMA NOT IN (
                'information_schema', 'mysql', 'performance_schema', 'sys',
                'mysql_innodb_cluster_metadata'
            )
            ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch columns", e))?;

        rows.into_iter()
            .map(|row| {
                let ordinal_position = ordinal_position_from_row(
                    row.ordinal_position,
                    &row.schema_name,
                    &row.table_name,
                )?;

                let mut column = RawColumn::new(
                    row.table_name,
                    row.schema_name,
                    row.column_name,
                    row.data_type,
                    row.is_nullable,
                    row.is_primary_key,
                    row.column_comment,
                    ordinal_position,
                );
                let extra = row.extra.unwrap_or_default();
                let extra_lower = extra.to_lowercase();
                column.auto_increment = extra_lower.contains("auto_increment");
                let is_generated = (extra_lower.contains("stored generated")
                    || extra_lower.contains("virtual generated"))
                    && row.generation_expression.is_some();
                if is_generated {
                    column.generated_expression = row.generation_expression;
                    column.generated_stored = extra_lower.contains("stored generated");
                } else {
                    // MySQL returns literal string defaults unquoted in
                    // COLUMN_DEFAULT (e.g. `active` for DEFAULT 'active'), while
                    // expression defaults are flagged via EXTRA=DEFAULT_GENERATED.
                    // Restore the quoting for literal string-typed defaults so
                    // the value round-trips as SQL and matches the parser.
                    let is_expression_default = extra_lower.contains("default_generated");
                    column.default_expression = row.column_default.map(|value| {
                        if !is_expression_default && is_mysql_string_type(&column.data_type) {
                            format!("'{}'", value.replace('\'', "''"))
                        } else {
                            value
                        }
                    });
                }
                if let Some(pos) = extra_lower.find("on update ") {
                    let rest = extra[pos + "on update ".len()..].trim();
                    if !rest.is_empty() {
                        column.on_update = Some(rest.to_string());
                    }
                }
                Ok::<RawColumn, IntrospectError>(column)
            })
            .collect::<Result<Vec<RawColumn>, IntrospectError>>()
    }

    /// Fetch table-level `CHECK` constraints (`MySQL` 8.0.16+). The
    /// `CHECK_CLAUSE` is rendered as `(<expr>)`; the outer parentheses are
    /// stripped to match the parser's representation.
    async fn fetch_checks(&self) -> Result<Vec<RawCheckConstraint>, IntrospectError> {
        let rows: Vec<RawCheckRow> = sqlx::query_as(
            r"
            SELECT
                CONVERT(tc.TABLE_SCHEMA USING utf8mb4) AS schema_name,
                CONVERT(tc.TABLE_NAME USING utf8mb4) AS table_name,
                CONVERT(cc.CONSTRAINT_NAME USING utf8mb4) AS name,
                CONVERT(cc.CHECK_CLAUSE USING utf8mb4) AS check_clause
            FROM information_schema.CHECK_CONSTRAINTS cc
            INNER JOIN information_schema.TABLE_CONSTRAINTS tc
                ON tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA
                AND tc.CONSTRAINT_NAME = cc.CONSTRAINT_NAME
            WHERE tc.TABLE_SCHEMA NOT IN (
                'information_schema', 'mysql', 'performance_schema', 'sys',
                'mysql_innodb_cluster_metadata'
            )
            ORDER BY tc.TABLE_SCHEMA, tc.TABLE_NAME, cc.CONSTRAINT_NAME
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch check constraints", e))?;

        Ok(rows
            .into_iter()
            .map(|row| RawCheckConstraint {
                schema_name: row.schema_name,
                table_name: row.table_name,
                name: Some(row.name),
                expression: strip_outer_parens(&row.check_clause),
            })
            .collect())
    }

    async fn fetch_foreign_keys(&self) -> Result<Vec<RawForeignKey>, IntrospectError> {
        let rows = self.fetch_foreign_key_rows().await?;
        Ok(group_foreign_keys(rows))
    }

    async fn fetch_indexes(&self) -> Result<Vec<RawIndex>, IntrospectError> {
        let rows = self.fetch_index_rows().await?;
        Ok(group_indexes(rows))
    }

    async fn fetch_views(&self) -> Result<Vec<RawView>, IntrospectError> {
        let rows: Vec<RawViewRow> = sqlx::query_as(
            r"
            SELECT
                CONVERT(TABLE_SCHEMA USING utf8mb4) AS schema_name,
                CONVERT(TABLE_NAME USING utf8mb4) AS view_name,
                CONVERT(VIEW_DEFINITION USING utf8mb4) AS definition,
                NULL AS view_comment
            FROM information_schema.VIEWS
            WHERE TABLE_SCHEMA NOT IN (
                'information_schema', 'mysql', 'performance_schema', 'sys',
                'mysql_innodb_cluster_metadata'
              )
            ORDER BY TABLE_SCHEMA, TABLE_NAME
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch views", e))?;

        Ok(rows.into_iter().map(map_view_row).collect())
    }

    async fn fetch_enums(&self) -> Result<Vec<RawEnum>, IntrospectError> {
        let rows: Vec<RawEnumRow> = sqlx::query_as(
            r"
            SELECT
                CONVERT(TABLE_SCHEMA USING utf8mb4) AS schema_name,
                CONVERT(COLUMN_TYPE USING utf8mb4) AS column_type
            FROM information_schema.COLUMNS
            WHERE DATA_TYPE IN ('enum', 'set')
              AND TABLE_SCHEMA NOT IN (
                  'information_schema', 'mysql', 'performance_schema', 'sys',
                  'mysql_innodb_cluster_metadata'
              )
            ORDER BY TABLE_SCHEMA, COLUMN_TYPE
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch enums", e))?;

        let mut seen = BTreeSet::new();
        let mut enums = Vec::new();

        for row in rows {
            let key = (row.schema_name.clone(), row.column_type.clone());
            if !seen.insert(key) {
                continue;
            }

            enums.push(raw_enum_from_mysql_column_type(
                &row.schema_name,
                &row.column_type,
            )?);
        }

        Ok(enums)
    }
}

impl MySqlCatalog {
    async fn fetch_foreign_key_rows(&self) -> Result<Vec<FkColumnRow>, IntrospectError> {
        let rows: Vec<FkColumnRow> = sqlx::query_as(
            r"
            SELECT
                CONVERT(kcu.CONSTRAINT_NAME USING utf8mb4) AS constraint_name,
                CONVERT(kcu.TABLE_SCHEMA USING utf8mb4) AS schema_name,
                CONVERT(kcu.TABLE_NAME USING utf8mb4) AS table_name,
                CONVERT(kcu.COLUMN_NAME USING utf8mb4) AS column_name,
                CONVERT(kcu.REFERENCED_TABLE_SCHEMA USING utf8mb4) AS referenced_schema,
                CONVERT(kcu.REFERENCED_TABLE_NAME USING utf8mb4) AS referenced_table,
                CONVERT(kcu.REFERENCED_COLUMN_NAME USING utf8mb4) AS referenced_column,
                kcu.ORDINAL_POSITION AS ordinal_position,
                CONVERT(rc.DELETE_RULE USING utf8mb4) AS delete_rule,
                CONVERT(rc.UPDATE_RULE USING utf8mb4) AS update_rule
            FROM information_schema.KEY_COLUMN_USAGE kcu
            INNER JOIN information_schema.TABLE_CONSTRAINTS tc
                ON kcu.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
                AND kcu.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
                AND kcu.TABLE_SCHEMA = tc.TABLE_SCHEMA
                AND kcu.TABLE_NAME = tc.TABLE_NAME
            INNER JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
                ON rc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA
                AND rc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME
            WHERE tc.CONSTRAINT_TYPE = 'FOREIGN KEY'
              AND kcu.TABLE_SCHEMA NOT IN (
                  'information_schema', 'mysql', 'performance_schema', 'sys',
                  'mysql_innodb_cluster_metadata'
              )
            ORDER BY
                kcu.TABLE_SCHEMA,
                kcu.TABLE_NAME,
                kcu.CONSTRAINT_NAME,
                kcu.ORDINAL_POSITION
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch foreign keys", e))?;

        Ok(rows)
    }

    async fn fetch_index_rows(&self) -> Result<Vec<IndexColumnRow>, IntrospectError> {
        let rows: Vec<IndexColumnRow> = sqlx::query_as(
            r"
            SELECT
                CONVERT(TABLE_SCHEMA USING utf8mb4) AS schema_name,
                CONVERT(TABLE_NAME USING utf8mb4) AS table_name,
                CONVERT(INDEX_NAME USING utf8mb4) AS index_name,
                CONVERT(COLUMN_NAME USING utf8mb4) AS column_name,
                CONVERT(EXPRESSION USING utf8mb4) AS expression,
                SUB_PART AS sub_part,
                CONVERT(INDEX_TYPE USING utf8mb4) AS index_type,
                SEQ_IN_INDEX AS seq_in_index,
                NON_UNIQUE AS non_unique,
                IF(INDEX_NAME = 'PRIMARY', TRUE, FALSE) AS is_primary
            FROM information_schema.STATISTICS
            WHERE TABLE_SCHEMA NOT IN (
                'information_schema', 'mysql', 'performance_schema', 'sys',
                'mysql_innodb_cluster_metadata'
              )
            ORDER BY TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch indexes", e))?;

        Ok(rows)
    }
}

/// Returns the effective pool max connection count for `MySQL`.
///
/// Defaults to `PARALLEL_CATALOG_QUERIES` so the parallel catalog fan-out
/// never queues on connection acquisition. Operators on constrained
/// environments can override the cap with `RELUNE_DB_POOL_MAX_CONNECTIONS`.
#[must_use]
pub(crate) fn pool_max_connections() -> u32 {
    pool_max_connections_with_default(PARALLEL_CATALOG_QUERIES)
}

#[derive(Debug, sqlx::FromRow)]
struct RawTableRow {
    schema_name: String,
    table_name: String,
    table_comment: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct RawColumnRow {
    schema_name: String,
    table_name: String,
    column_name: String,
    data_type: String,
    is_nullable: bool,
    is_primary_key: bool,
    column_comment: Option<String>,
    ordinal_position: u64,
    column_default: Option<String>,
    extra: Option<String>,
    generation_expression: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct RawCheckRow {
    schema_name: String,
    table_name: String,
    name: String,
    check_clause: String,
}

/// Returns `true` for `MySQL` column types whose literal `DEFAULT` values are
/// reported unquoted by `information_schema.COLUMN_DEFAULT` and therefore need
/// quoting restored (character, text, enum/set types).
fn is_mysql_string_type(column_type: &str) -> bool {
    let lower = column_type.to_ascii_lowercase();
    ["char", "text", "enum", "set"]
        .iter()
        .any(|kind| lower.contains(kind))
}

/// Remove one balanced layer of surrounding parentheses (e.g. a `MySQL`
/// `CHECK_CLAUSE` `(expr)` or a `pg`-style wrapped expression).
fn strip_outer_parens(clause: &str) -> String {
    let trimmed = clause.trim();
    trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .map_or_else(|| trimmed.to_string(), |inner| inner.trim().to_string())
}

#[derive(Debug, sqlx::FromRow, Clone)]
struct FkColumnRow {
    constraint_name: String,
    schema_name: String,
    table_name: String,
    column_name: String,
    referenced_schema: String,
    referenced_table: String,
    referenced_column: String,
    ordinal_position: u64,
    delete_rule: String,
    update_rule: String,
}

#[derive(Debug, sqlx::FromRow, Clone)]
struct IndexColumnRow {
    schema_name: String,
    table_name: String,
    index_name: String,
    // NULL for a functional key part; `expression` carries the text instead.
    column_name: Option<String>,
    // Functional-index expression (MySQL 8.0.13+); NULL for plain columns.
    expression: Option<String>,
    // Indexed prefix length (e.g. `col(10)`); NULL when the whole column is indexed.
    sub_part: Option<i64>,
    index_type: Option<String>,
    seq_in_index: u64,
    non_unique: i64, // 0 = UNIQUE or PRIMARY
    is_primary: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct RawViewRow {
    schema_name: String,
    view_name: String,
    definition: Option<String>,
    view_comment: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct RawEnumRow {
    schema_name: String,
    column_type: String,
}

/// Maps a raw `information_schema.VIEWS` row to a [`RawView`].
///
/// `MySQL` returns an empty `VIEW_DEFINITION` (rather than an error) when the
/// connection lacks the `SHOW VIEW` privilege, which is indistinguishable from
/// a definition the server simply did not record. Empty definitions are
/// normalized to `None` and logged so the gap is visible rather than surfacing
/// as a view with a blank body.
fn map_view_row(row: RawViewRow) -> RawView {
    let definition = row
        .definition
        .filter(|definition| !definition.trim().is_empty());

    if definition.is_none() {
        warn!(
            schema = %row.schema_name,
            view = %row.view_name,
            "MySQL view definition is empty; the connection may lack the SHOW VIEW privilege"
        );
    }

    RawView {
        view_name: row.view_name,
        schema_name: row.schema_name,
        definition,
        view_comment: row.view_comment,
    }
}

fn raw_enum_from_mysql_column_type(
    schema_name: &str,
    column_type: &str,
) -> Result<RawEnum, IntrospectError> {
    let values = parse_mysql_enum_like_values(column_type)
        .map_err(|error| {
            IntrospectError::metadata_mapping(format!(
                "unsupported MySQL enum/set definition in {schema_name}: {column_type} ({error})"
            ))
        })?
        .ok_or_else(|| {
            IntrospectError::metadata_mapping(format!(
                "unsupported MySQL enum/set definition in {schema_name}: {column_type}"
            ))
        })?;

    Ok(RawEnum {
        enum_name: column_type.to_string(),
        schema_name: schema_name.to_string(),
        values,
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
enum MySqlEnumLikeParseError {
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

fn parse_mysql_enum_like_values(
    column_type: &str,
) -> Result<Option<Vec<String>>, MySqlEnumLikeParseError> {
    let Some(start) = column_type.find('(') else {
        return Ok(None);
    };
    let Some(end) = column_type.rfind(')') else {
        return Err(MySqlEnumLikeParseError::MissingClosingParenthesis);
    };
    let kind = column_type[..start].trim();
    if !kind.eq_ignore_ascii_case("enum") && !kind.eq_ignore_ascii_case("set") {
        return Ok(None);
    }

    let mut values = Vec::new();
    let mut chars = column_type[start + 1..end].chars().peekable();

    while chars.peek().is_some() {
        while chars.peek().is_some_and(char::is_ascii_whitespace) {
            chars.next();
        }

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
                    value.push(escaped);
                }
                Some(c) => value.push(c),
                None => return Err(MySqlEnumLikeParseError::UnterminatedQuotedValue),
            }
        }
        values.push(value);

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

    Ok(Some(values))
}

fn group_foreign_keys(rows: Vec<FkColumnRow>) -> Vec<RawForeignKey> {
    #[derive(Eq, PartialEq, Ord, PartialOrd, Clone)]
    struct Key {
        schema: String,
        table: String,
        constraint: String,
    }

    let mut groups: BTreeMap<Key, Vec<FkColumnRow>> = BTreeMap::new();
    for row in rows {
        let key = Key {
            schema: row.schema_name.clone(),
            table: row.table_name.clone(),
            constraint: row.constraint_name.clone(),
        };
        groups.entry(key).or_default().push(row);
    }

    let mut out = Vec::new();
    for (key, mut cols) in groups {
        cols.sort_by_key(|r| r.ordinal_position);
        let from_columns: Vec<String> = cols.iter().map(|r| r.column_name.clone()).collect();
        let to_columns: Vec<String> = cols.iter().map(|r| r.referenced_column.clone()).collect();
        let to_schema = cols.first().map(|r| r.referenced_schema.clone());
        let to_table = cols
            .first()
            .map(|r| r.referenced_table.clone())
            .unwrap_or_default();
        let on_delete = cols
            .first()
            .map(|r| parse_referential_action(&r.delete_rule))
            .unwrap_or_default();
        let on_update = cols
            .first()
            .map(|r| parse_referential_action(&r.update_rule))
            .unwrap_or_default();
        out.push(RawForeignKey {
            constraint_name: key.constraint,
            schema_name: key.schema,
            from_table: key.table,
            from_columns,
            to_schema,
            to_table,
            to_columns,
            on_delete,
            on_update,
        });
    }
    out
}

fn ordinal_position_from_row(
    ordinal_position: u64,
    schema_name: &str,
    table_name: &str,
) -> Result<i16, IntrospectError> {
    i16::try_from(ordinal_position).map_err(|_| {
        IntrospectError::metadata_mapping(format!(
            "ordinal_position {ordinal_position} out of range for {schema_name}.{table_name}"
        ))
    })
}

fn group_indexes(rows: Vec<IndexColumnRow>) -> Vec<RawIndex> {
    #[derive(Eq, PartialEq, Ord, PartialOrd, Clone)]
    struct Key {
        schema: String,
        table: String,
        index: String,
    }

    let mut groups: BTreeMap<Key, Vec<IndexColumnRow>> = BTreeMap::new();
    for row in rows {
        let key = Key {
            schema: row.schema_name.clone(),
            table: row.table_name.clone(),
            index: row.index_name.clone(),
        };
        groups.entry(key).or_default().push(row);
    }

    let mut out = Vec::new();
    for (key, mut cols) in groups {
        cols.sort_by_key(|r| r.seq_in_index);
        let first = cols.first();
        let Some(first) = first else {
            continue;
        };
        let is_primary = first.is_primary;
        let is_unique = first.non_unique == 0 && !first.is_primary;
        let method = first.index_type.as_ref().map(|t| t.to_lowercase());
        let key_parts: Vec<RawIndexKeyPart> = cols
            .iter()
            .map(|r| match &r.column_name {
                Some(name) => RawIndexKeyPart::Column {
                    name: name.clone(),
                    prefix_length: r.sub_part.and_then(|p| u32::try_from(p).ok()),
                },
                // A functional key part reports a NULL column and carries its
                // definition in EXPRESSION.
                None => RawIndexKeyPart::Expression(r.expression.clone().unwrap_or_default()),
            })
            .collect();
        out.push(RawIndex {
            index_name: key.index.clone(),
            schema_name: key.schema,
            table_name: key.table,
            key_parts,
            is_unique,
            is_primary,
            predicate: None,
            included_columns: Vec::new(),
            method,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_max_connections_is_positive() {
        // Whichever value wins (default or env override), it must remain
        // positive so the pool can be constructed.
        assert!(pool_max_connections() > 0);
    }

    #[test]
    fn rejects_oversized_ordinal_positions() {
        let err = ordinal_position_from_row(i16::MAX as u64 + 1, "public", "users")
            .expect_err("ordinal_position should overflow");
        assert!(matches!(err, IntrospectError::MetadataMapping(_)));
    }

    #[test]
    fn parses_mysql_enum_values() {
        assert_eq!(
            parse_mysql_enum_like_values("enum('draft','published')"),
            Ok(Some(vec!["draft".to_string(), "published".to_string()]))
        );
    }

    #[test]
    fn parses_mysql_set_values_with_escaped_quotes() {
        assert_eq!(
            parse_mysql_enum_like_values("set('O''Reilly','back\\\\slash')"),
            Ok(Some(vec![
                "O'Reilly".to_string(),
                "back\\slash".to_string()
            ]))
        );
    }

    #[test]
    fn rejects_non_enum_like_column_types() {
        assert_eq!(parse_mysql_enum_like_values("varchar(255)"), Ok(None));
    }

    #[test]
    fn rejects_malformed_enum_values_with_incomplete_escape_sequences() {
        assert_eq!(
            parse_mysql_enum_like_values("enum('bad\\)"),
            Err(MySqlEnumLikeParseError::TrailingEscapeSequence)
        );
    }

    #[test]
    fn map_view_row_keeps_non_empty_definitions() {
        let view = map_view_row(RawViewRow {
            schema_name: "app".to_string(),
            view_name: "active_users".to_string(),
            definition: Some("select 1".to_string()),
            view_comment: None,
        });
        assert_eq!(view.definition.as_deref(), Some("select 1"));
    }

    #[test]
    fn map_view_row_normalizes_empty_definitions_to_none() {
        for definition in [None, Some(String::new()), Some("   ".to_string())] {
            let view = map_view_row(RawViewRow {
                schema_name: "app".to_string(),
                view_name: "active_users".to_string(),
                definition,
                view_comment: None,
            });
            assert_eq!(view.definition, None);
        }
    }

    #[test]
    fn builds_raw_enum_from_mysql_column_type() {
        let raw_enum = raw_enum_from_mysql_column_type("app", "enum('a','b')")
            .expect("enum definition should parse");

        assert_eq!(raw_enum.schema_name, "app");
        assert_eq!(raw_enum.enum_name, "enum('a','b')");
        assert_eq!(raw_enum.values, vec!["a".to_string(), "b".to_string()]);
    }
}
