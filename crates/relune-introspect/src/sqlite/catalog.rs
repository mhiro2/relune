//! `SQLite` catalog introspection via `sqlite_master` and `PRAGMA`s.

use std::collections::BTreeMap;
use std::future::Future;

use sqlx::sqlite::SqlitePool;

use crate::catalog::raw_schema;
use crate::common::{
    RawCheckConstraint, RawColumn, RawForeignKey, RawIndex, RawIndexKeyPart, RawSchema, RawTable,
    RawView, parse_referential_action,
};
use crate::connect::statement_timeout;
use crate::error::IntrospectError;

const MAIN_SCHEMA: &str = "main";

/// Fetches all catalog metadata from a `SQLite` database (default `main` schema).
pub async fn fetch_catalog_metadata(pool: &SqlitePool) -> Result<RawSchema, IntrospectError> {
    let table_names = list_user_tables(pool).await?;
    let mut columns = Vec::new();
    let mut foreign_keys = Vec::new();
    let mut indexes = Vec::new();
    let mut tables = Vec::new();
    let mut checks = Vec::new();

    for table_name in &table_names {
        let q = quote_ident(table_name)?;
        tables.push(RawTable {
            table_name: table_name.clone(),
            schema_name: MAIN_SCHEMA.to_string(),
            table_comment: None,
        });

        let col_rows = pragma_table_info(pool, &q).await?;
        for row in col_rows {
            let ordinal_position =
                ordinal_position_from_row(row.cid.saturating_add(1), table_name)?;

            let mut column = RawColumn::new(
                table_name.clone(),
                MAIN_SCHEMA.to_string(),
                row.name,
                row.col_type,
                row.notnull == 0,
                row.pk > 0,
                None,
                ordinal_position,
            );
            column.default_expression = row.dflt_value;
            columns.push(column);
        }

        let fk_rows = pragma_foreign_key_list(pool, &q).await?;
        foreign_keys.extend(group_sqlite_fks(table_name, fk_rows));

        let idx_rows = pragma_index_list(pool, &q).await?;
        indexes.extend(collect_table_indexes(pool, table_name, idx_rows).await?);

        // SQLite exposes CHECK constraints only through the CREATE TABLE text.
        if let Some(sql) = fetch_table_sql(pool, table_name).await? {
            for (name, expression) in parse_sqlite_table_checks(&sql) {
                checks.push(RawCheckConstraint {
                    schema_name: MAIN_SCHEMA.to_string(),
                    table_name: table_name.clone(),
                    name,
                    expression,
                });
            }
        }
    }

    let views = list_views(pool).await?;

    Ok(raw_schema(
        tables,
        columns,
        foreign_keys,
        indexes,
        views,
        Vec::new(),
        checks,
    ))
}

/// Fetch the `CREATE TABLE` text for a named table from `sqlite_master`.
async fn fetch_table_sql(
    pool: &SqlitePool,
    table_name: &str,
) -> Result<Option<String>, IntrospectError> {
    with_query_timeout("sqlite_master table sql", async {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
        )
        .bind(table_name)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
        .map_err(|e| IntrospectError::query_with_source("Failed to read table definition", e))
    })
    .await
}

/// Best-effort extraction of top-level `CHECK (<expr>)` constraints from a
/// `CREATE TABLE` statement, quote- and depth-aware. Returns each constraint's
/// optional name (from a preceding `CONSTRAINT <name>`) and expression text.
///
/// This covers both table-level checks and column-level `CHECK` clauses, since
/// catalogs do not distinguish them; both are attached at table level.
fn parse_sqlite_table_checks(sql: &str) -> Vec<(Option<String>, String)> {
    let bytes = sql.as_bytes();
    // Enter the outermost parenthesised body.
    let mut i = 0usize;
    let mut body_open = None;
    while i < bytes.len() {
        let skipped = skip_sqlite_quote(bytes, i);
        if skipped != i {
            i = skipped;
            continue;
        }
        if bytes[i] == b'(' {
            body_open = Some(i);
            break;
        }
        i += 1;
    }
    let Some(body_open) = body_open else {
        return Vec::new();
    };

    let mut checks = Vec::new();
    let mut depth = 0usize;
    let mut pending_name: Option<String> = None;
    let mut k = body_open;
    while k < bytes.len() {
        let skipped = skip_sqlite_quote(bytes, k);
        if skipped != k {
            k = skipped;
            continue;
        }
        match bytes[k] {
            b'(' => {
                depth += 1;
                k += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
                k += 1;
            }
            _ if depth == 1 => {
                // Track `CONSTRAINT <name>` only when it directly names a CHECK
                // (`CONSTRAINT c CHECK (...)`). A name in front of a different
                // constraint type (e.g. `CONSTRAINT x_nn NOT NULL`) must not
                // leak onto a later CHECK clause.
                if keyword_at(bytes, k, b"constraint") {
                    let after = k + "constraint".len();
                    let (name, next) = read_identifier(sql, bytes, skip_ws(bytes, after));
                    let following = skip_ws(bytes, next);
                    pending_name = if keyword_at(bytes, following, b"check") {
                        name
                    } else {
                        None
                    };
                    k = next;
                    continue;
                }
                if keyword_at(bytes, k, b"check") {
                    let paren = skip_ws(bytes, k + "check".len());
                    if paren < bytes.len()
                        && bytes[paren] == b'('
                        && let Some((expr, next)) = read_balanced(sql, bytes, paren)
                    {
                        checks.push((pending_name.take(), expr.trim().to_string()));
                        k = next;
                        continue;
                    }
                }
                // A comma at the top level ends the current column/constraint.
                if bytes[k] == b',' {
                    pending_name = None;
                }
                k += 1;
            }
            _ => k += 1,
        }
    }

    checks
}

/// `true` if a whole-word (case-insensitive) keyword `kw` starts at `pos`.
fn keyword_at(bytes: &[u8], pos: usize, kw: &[u8]) -> bool {
    let end = pos + kw.len();
    let matches = bytes
        .get(pos..end)
        .is_some_and(|s| s.eq_ignore_ascii_case(kw));
    let prev_ok = pos == 0 || !is_ident_byte(bytes[pos - 1]);
    let next_ok = bytes.get(end).is_none_or(|b| !is_ident_byte(*b));
    matches && prev_ok && next_ok
}

/// Skip ASCII whitespace, returning the next non-space index.
fn skip_ws(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

/// Read an identifier (optionally quoted) starting at `pos`, returning its
/// unquoted text and the index just past it.
fn read_identifier(sql: &str, bytes: &[u8], pos: usize) -> (Option<String>, usize) {
    if pos >= bytes.len() {
        return (None, pos);
    }
    let end = skip_sqlite_quote(bytes, pos);
    if end != pos {
        // Quoted identifier: strip the surrounding quote characters.
        let inner = sql[pos + 1..end - 1].replace("\"\"", "\"");
        return (Some(inner), end);
    }
    let mut e = pos;
    while e < bytes.len() && is_ident_byte(bytes[e]) {
        e += 1;
    }
    if e == pos {
        (None, pos)
    } else {
        (Some(sql[pos..e].to_string()), e)
    }
}

/// Read a balanced `(...)` group starting at `open`, returning the inner text
/// and the index just past the closing paren. Quote- and depth-aware.
fn read_balanced(sql: &str, bytes: &[u8], open: usize) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut j = open;
    while j < bytes.len() {
        let skipped = skip_sqlite_quote(bytes, j);
        if skipped != j {
            j = skipped;
            continue;
        }
        match bytes[j] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((sql[open + 1..j].to_string(), j + 1));
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// Wraps a `SQLite` query future with the shared per-statement deadline.
///
/// `SQLite` has no server-side `statement_timeout` equivalent, so a
/// hostile or corrupted database file could cause `sqlx::query_as(...)
/// .fetch_all(...)` to hang forever. Mirroring the 30s deadline used by
/// `PostgreSQL` and `MySQL` bounds each individual query. The total catalog
/// fetch (one set of `PRAGMA`s per table) is bounded separately by the overall
/// introspection deadline applied in `sqlite::introspect_sqlite`.
async fn with_query_timeout<T, F>(context: &'static str, fut: F) -> Result<T, IntrospectError>
where
    F: Future<Output = Result<T, IntrospectError>>,
{
    match tokio::time::timeout(statement_timeout(), fut).await {
        Ok(result) => result,
        Err(_) => Err(IntrospectError::timeout(format!(
            "{context} did not complete within {} seconds",
            statement_timeout().as_secs()
        ))),
    }
}

async fn list_user_tables(pool: &SqlitePool) -> Result<Vec<String>, IntrospectError> {
    with_query_timeout("Listing tables", async {
        let rows: Vec<(String,)> = sqlx::query_as(
            r"
            SELECT name
            FROM sqlite_master
            WHERE type = 'table'
              AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            ",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to list tables", e))?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    })
    .await
}

async fn list_views(pool: &SqlitePool) -> Result<Vec<RawView>, IntrospectError> {
    let rows: Vec<(String, Option<String>)> = with_query_timeout("Listing views", async {
        sqlx::query_as(
            r"
            SELECT name, sql
            FROM sqlite_master
            WHERE type = 'view'
              AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            ",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to list views", e))
    })
    .await?;

    Ok(rows
        .into_iter()
        .map(|(name, sql)| RawView {
            view_name: name,
            schema_name: MAIN_SCHEMA.to_string(),
            definition: sql,
            view_comment: None,
        })
        .collect())
}

fn quote_ident(name: &str) -> Result<String, IntrospectError> {
    if name.contains('\0') {
        return Err(IntrospectError::metadata_mapping(format!(
            "SQLite identifier contains NUL byte: {name:?}"
        )));
    }

    let escaped = name.replace('"', "\"\"");
    Ok(format!(r#""{escaped}""#))
}

fn ordinal_position_from_row(
    ordinal_position: i64,
    table_name: &str,
) -> Result<i16, IntrospectError> {
    i16::try_from(ordinal_position).map_err(|_| {
        IntrospectError::metadata_mapping(format!(
            "ordinal_position {ordinal_position} out of range for {MAIN_SCHEMA}.{table_name}"
        ))
    })
}

async fn pragma_table_info(
    pool: &SqlitePool,
    quoted_table: &str,
) -> Result<Vec<SqliteTableInfoRow>, IntrospectError> {
    let sql = format!("PRAGMA table_info({quoted_table})");
    with_query_timeout("PRAGMA table_info", async {
        sqlx::query_as::<_, SqliteTableInfoRow>(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .map_err(|e| IntrospectError::query_with_source("PRAGMA table_info failed", e))
    })
    .await
}

async fn pragma_foreign_key_list(
    pool: &SqlitePool,
    quoted_table: &str,
) -> Result<Vec<SqliteFkRow>, IntrospectError> {
    let sql = format!("PRAGMA foreign_key_list({quoted_table})");
    with_query_timeout("PRAGMA foreign_key_list", async {
        sqlx::query_as::<_, SqliteFkRow>(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .map_err(|e| IntrospectError::query_with_source("PRAGMA foreign_key_list failed", e))
    })
    .await
}

async fn pragma_index_list(
    pool: &SqlitePool,
    quoted_table: &str,
) -> Result<Vec<SqliteIndexListRow>, IntrospectError> {
    let sql = format!("PRAGMA index_list({quoted_table})");
    with_query_timeout("PRAGMA index_list", async {
        sqlx::query_as::<_, SqliteIndexListRow>(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .map_err(|e| IntrospectError::query_with_source("PRAGMA index_list failed", e))
    })
    .await
}

async fn pragma_index_info(
    pool: &SqlitePool,
    quoted_index: &str,
) -> Result<Vec<SqliteIndexInfoRow>, IntrospectError> {
    let sql = format!("PRAGMA index_info({quoted_index})");
    with_query_timeout("PRAGMA index_info", async {
        sqlx::query_as::<_, SqliteIndexInfoRow>(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .map_err(|e| IntrospectError::query_with_source("PRAGMA index_info failed", e))
    })
    .await
}

#[derive(Debug, sqlx::FromRow)]
struct SqliteTableInfoRow {
    cid: i64,
    name: String,
    #[sqlx(rename = "type")]
    col_type: String,
    notnull: i64,
    dflt_value: Option<String>,
    pk: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct SqliteFkRow {
    id: i64,
    seq: i64,
    table: String,
    #[sqlx(rename = "from")]
    from_col: String,
    to: String,
    on_update: String,
    on_delete: String,
}

#[derive(Debug, sqlx::FromRow)]
struct SqliteIndexListRow {
    #[allow(dead_code)]
    seq: i64,
    name: String,
    #[sqlx(rename = "unique")]
    is_unique_flag: i64,
    origin: String,
    partial: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct SqliteIndexInfoRow {
    seqno: i64,
    #[allow(dead_code)]
    cid: i64,
    name: Option<String>,
}

fn group_sqlite_fks(from_table: &str, rows: Vec<SqliteFkRow>) -> Vec<RawForeignKey> {
    #[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy)]
    struct Gk {
        id: i64,
    }

    let mut groups: BTreeMap<Gk, Vec<SqliteFkRow>> = BTreeMap::new();
    for row in rows {
        groups.entry(Gk { id: row.id }).or_default().push(row);
    }

    let mut out = Vec::new();
    for (gk, mut cols) in groups {
        cols.sort_by_key(|r| r.seq);
        let to_table = cols.first().map(|r| r.table.clone()).unwrap_or_default();
        let from_columns: Vec<String> = cols.iter().map(|r| r.from_col.clone()).collect();
        let to_columns: Vec<String> = cols.iter().map(|r| r.to.clone()).collect();
        let constraint_name = format!("fk_{from_table}_{}", gk.id);
        let on_delete = cols
            .first()
            .map(|r| parse_referential_action(&r.on_delete))
            .unwrap_or_default();
        let on_update = cols
            .first()
            .map(|r| parse_referential_action(&r.on_update))
            .unwrap_or_default();
        out.push(RawForeignKey {
            constraint_name,
            schema_name: MAIN_SCHEMA.to_string(),
            from_table: from_table.to_string(),
            from_columns,
            to_schema: None,
            to_table,
            to_columns,
            on_delete,
            on_update,
        });
    }
    out
}

async fn collect_table_indexes(
    pool: &SqlitePool,
    table_name: &str,
    list_rows: Vec<SqliteIndexListRow>,
) -> Result<Vec<RawIndex>, IntrospectError> {
    let mut out = Vec::new();
    for entry in list_rows {
        if entry.origin == "pk" {
            continue;
        }
        let index_name = entry.name;
        if index_name.starts_with("sqlite_autoindex_") {
            continue;
        }
        let quoted_idx = quote_ident(&index_name)?;
        let mut info = pragma_index_info(pool, &quoted_idx).await?;
        info.sort_by_key(|r| r.seqno);

        // PRAGMA index_info reports NULL column names for expression key parts
        // and exposes no predicate. Recover both from the CREATE INDEX text so
        // expression indexes are neither dropped nor mistaken for plain-column
        // indexes.
        let def_sql = fetch_index_sql(pool, &index_name).await?;
        let (key_defs, predicate) = def_sql
            .as_deref()
            .map(split_sqlite_index_def)
            .unwrap_or_default();

        let key_parts: Vec<RawIndexKeyPart> = info
            .iter()
            .enumerate()
            .map(|(pos, r)| match &r.name {
                Some(name) => RawIndexKeyPart::column(name.clone()),
                None => RawIndexKeyPart::Expression(
                    key_defs
                        .get(pos)
                        .cloned()
                        .unwrap_or_else(|| "(expression)".to_string()),
                ),
            })
            .collect();
        if key_parts.is_empty() {
            continue;
        }
        out.push(RawIndex {
            index_name: index_name.clone(),
            schema_name: MAIN_SCHEMA.to_string(),
            table_name: table_name.to_string(),
            key_parts,
            is_unique: entry.is_unique_flag != 0,
            is_primary: false,
            predicate: if entry.partial != 0 { predicate } else { None },
            included_columns: Vec::new(),
            method: None,
        });
    }
    Ok(out)
}

/// Fetch the `CREATE INDEX` text for a named index from `sqlite_master`.
async fn fetch_index_sql(
    pool: &SqlitePool,
    index_name: &str,
) -> Result<Option<String>, IntrospectError> {
    with_query_timeout("sqlite_master index sql", async {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
        )
        .bind(index_name)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
        .map_err(|e| IntrospectError::query_with_source("Failed to read index definition", e))
    })
    .await
}

/// Split a `CREATE INDEX ... ON t (<key list>) [WHERE <predicate>]` definition
/// into the ordered key-part texts and the optional partial predicate.
///
/// Best-effort parse over the raw DDL: it locates the top-level parenthesised
/// key list and any trailing `WHERE`, splitting the key list on top-level
/// commas. Used only to label expression key parts and recover the predicate;
/// plain-column parts come from `PRAGMA index_info`.
fn split_sqlite_index_def(sql: &str) -> (Vec<String>, Option<String>) {
    let bytes = sql.as_bytes();

    // Locate the top-level key-list parentheses, ignoring any '(' that appears
    // inside a quoted identifier or string literal (e.g. a quoted index name
    // `"idx(foo)"`).
    let mut i = 0usize;
    let mut open = None;
    while i < bytes.len() {
        let skipped = skip_sqlite_quote(bytes, i);
        if skipped != i {
            i = skipped;
            continue;
        }
        if bytes[i] == b'(' {
            open = Some(i);
            break;
        }
        i += 1;
    }
    let Some(open) = open else {
        return (Vec::new(), None);
    };

    let mut depth = 0usize;
    let mut close = None;
    let mut j = open;
    while j < bytes.len() {
        let skipped = skip_sqlite_quote(bytes, j);
        if skipped != j {
            j = skipped;
            continue;
        }
        match bytes[j] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(j);
                    break;
                }
            }
            _ => {}
        }
        j += 1;
    }
    let Some(close) = close else {
        return (Vec::new(), None);
    };

    // Split the key list on top-level commas, skipping quotes and nested parens.
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut seg_start = open + 1;
    let mut k = open + 1;
    while k < close {
        let skipped = skip_sqlite_quote(bytes, k);
        if skipped != k {
            k = skipped.min(close);
            continue;
        }
        match bytes[k] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(sql[seg_start..k].trim().to_string());
                seg_start = k + 1;
            }
            _ => {}
        }
        k += 1;
    }
    parts.push(sql[seg_start..close].trim().to_string());

    // Find the top-level `WHERE` keyword in the tail, ignoring occurrences
    // inside quoted strings/identifiers and matching only a whole word.
    let mut predicate = None;
    let mut m = close + 1;
    while m < bytes.len() {
        let skipped = skip_sqlite_quote(bytes, m);
        if skipped != m {
            m = skipped;
            continue;
        }
        let is_where = bytes
            .get(m..m + 5)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"where"));
        let prev_boundary = m == 0 || !is_ident_byte(bytes[m - 1]);
        let next_boundary = bytes.get(m + 5).is_none_or(|b| !is_ident_byte(*b));
        if is_where && prev_boundary && next_boundary {
            let candidate = sql[m + 5..].trim();
            if !candidate.is_empty() {
                predicate = Some(candidate.to_string());
            }
            break;
        }
        m += 1;
    }

    (parts, predicate)
}

/// If `bytes[i]` opens a quoted string/identifier (`'`, `"`, `` ` ``, `[`),
/// return the index just past its close; otherwise return `i` unchanged.
/// Doubled quote characters are treated as escapes (SQLite/SQL convention).
fn skip_sqlite_quote(bytes: &[u8], i: usize) -> usize {
    let close = match bytes[i] {
        b'\'' => b'\'',
        b'"' => b'"',
        b'`' => b'`',
        b'[' => b']',
        _ => return i,
    };
    let doubled_escape = bytes[i] != b'[';
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == close {
            if doubled_escape && bytes.get(j + 1) == Some(&close) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    bytes.len()
}

/// `true` for bytes that can appear inside an unquoted SQL identifier/keyword.
const fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_null_bytes_in_identifiers() {
        let err = quote_ident("bad\0name").expect_err("NUL bytes must be rejected");
        assert!(matches!(err, IntrospectError::MetadataMapping(_)));
        assert!(err.to_string().contains("NUL byte"));
    }

    #[test]
    fn quotes_identifiers_with_double_quotes() {
        let quoted = quote_ident(r#"na"me"#).expect("identifier should be quoted");
        assert_eq!(quoted, r#""na""me""#);
    }

    #[test]
    fn split_index_def_handles_quoted_name_with_paren() {
        // A '(' inside the quoted index name must not be treated as the start
        // of the key list.
        let (parts, predicate) = split_sqlite_index_def(r#"CREATE INDEX "idx(foo)" ON t (a, b)"#);
        assert_eq!(parts, vec!["a".to_string(), "b".to_string()]);
        assert!(predicate.is_none());
    }

    #[test]
    fn split_index_def_recovers_expression_and_predicate() {
        let (parts, predicate) =
            split_sqlite_index_def("CREATE INDEX i ON t (lower(email), id) WHERE id > 0");
        assert_eq!(parts, vec!["lower(email)".to_string(), "id".to_string()]);
        assert_eq!(predicate.as_deref(), Some("id > 0"));
    }

    #[test]
    fn split_index_def_ignores_where_inside_string_literal() {
        // The literal contains "where"; the real predicate keyword follows it.
        let (parts, predicate) =
            split_sqlite_index_def("CREATE INDEX i ON t (note) WHERE note = 'nowhere land'");
        assert_eq!(parts, vec!["note".to_string()]);
        assert_eq!(predicate.as_deref(), Some("note = 'nowhere land'"));
    }

    #[test]
    fn split_index_def_does_not_split_commas_in_expression() {
        let (parts, _) = split_sqlite_index_def("CREATE INDEX i ON t (coalesce(a, b), c)");
        assert_eq!(parts, vec!["coalesce(a, b)".to_string(), "c".to_string()]);
    }

    #[test]
    fn parse_table_checks_extracts_named_and_unnamed() {
        let sql = "CREATE TABLE t (\n  amount INTEGER CHECK (amount >= 0),\n  qty INTEGER,\n  CONSTRAINT qty_positive CHECK (qty > 0)\n)";
        let checks = parse_sqlite_table_checks(sql);
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0], (None, "amount >= 0".to_string()));
        assert_eq!(
            checks[1],
            (Some("qty_positive".to_string()), "qty > 0".to_string())
        );
    }

    #[test]
    fn parse_table_checks_does_not_leak_constraint_name_to_later_check() {
        // `x_nn` names the NOT NULL constraint, not the unnamed CHECK.
        let sql = "CREATE TABLE t (\n  x INTEGER CONSTRAINT x_nn NOT NULL CHECK (x > 0)\n)";
        let checks = parse_sqlite_table_checks(sql);
        assert_eq!(checks, vec![(None, "x > 0".to_string())]);
    }

    #[test]
    fn parse_table_checks_ignores_check_inside_string_or_nested() {
        // A parenthesised default expression must not confuse the top-level scan,
        // and only real CHECK clauses are captured.
        let sql = "CREATE TABLE t (\n  label TEXT DEFAULT '(check me)',\n  n INTEGER CHECK (n <> (1 + 2))\n)";
        let checks = parse_sqlite_table_checks(sql);
        assert_eq!(checks, vec![(None, "n <> (1 + 2)".to_string())]);
    }

    #[test]
    fn rejects_oversized_ordinal_positions() {
        let err = ordinal_position_from_row(i64::from(i16::MAX) + 1, "users")
            .expect_err("ordinal_position should overflow");
        assert!(matches!(err, IntrospectError::MetadataMapping(_)));
    }

    #[tokio::test(start_paused = true)]
    async fn with_query_timeout_returns_timeout_when_future_hangs() {
        let result: Result<(), IntrospectError> = with_query_timeout("test query", async {
            tokio::time::sleep(statement_timeout() * 2).await;
            Ok(())
        })
        .await;

        let err = result.expect_err("hung future should yield a timeout error");
        assert!(matches!(err, IntrospectError::Timeout(_)));
        assert!(err.to_string().contains("test query"));
    }

    #[tokio::test]
    async fn with_query_timeout_propagates_inner_result_when_within_deadline() {
        let value = with_query_timeout::<u32, _>("ok", async { Ok(7) })
            .await
            .expect("future should succeed within deadline");
        assert_eq!(value, 7);
    }
}
