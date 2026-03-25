//! `SQLite` catalog introspection via `sqlite_master` and `PRAGMA`s.

use std::collections::BTreeMap;

use sqlx::sqlite::SqlitePool;

use crate::common::{
    RawColumn, RawForeignKey, RawIndex, RawSchema, RawTable, RawView, parse_referential_action,
};
use crate::error::IntrospectError;

const MAIN_SCHEMA: &str = "main";

/// Fetches all catalog metadata from a `SQLite` database (default `main` schema).
pub async fn fetch_catalog_metadata(pool: &SqlitePool) -> Result<RawSchema, IntrospectError> {
    let table_names = list_user_tables(pool).await?;
    let mut columns = Vec::new();
    let mut foreign_keys = Vec::new();
    let mut indexes = Vec::new();
    let mut tables = Vec::new();

    for table_name in &table_names {
        let q = quote_ident(table_name);
        tables.push(RawTable {
            table_name: table_name.clone(),
            schema_name: MAIN_SCHEMA.to_string(),
            table_comment: None,
        });

        let col_rows = pragma_table_info(pool, &q).await?;
        for row in col_rows {
            columns.push(RawColumn {
                table_name: table_name.clone(),
                schema_name: MAIN_SCHEMA.to_string(),
                column_name: row.name,
                data_type: row.col_type,
                is_nullable: row.notnull == 0,
                is_primary_key: row.pk > 0,
                column_comment: None,
                ordinal_position: i16::try_from(row.cid.saturating_add(1)).unwrap_or(i16::MAX),
            });
        }

        let fk_rows = pragma_foreign_key_list(pool, &q).await?;
        foreign_keys.extend(group_sqlite_fks(table_name, fk_rows));

        let idx_rows = pragma_index_list(pool, &q).await?;
        indexes.extend(collect_table_indexes(pool, table_name, idx_rows).await?);
    }

    let views = list_views(pool).await?;

    Ok(RawSchema {
        tables,
        columns,
        foreign_keys,
        indexes,
        views,
        enums: Vec::new(),
    })
}

async fn list_user_tables(pool: &SqlitePool) -> Result<Vec<String>, IntrospectError> {
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
    .map_err(|e| IntrospectError::query(format!("Failed to list tables: {e}")))?;

    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn list_views(pool: &SqlitePool) -> Result<Vec<RawView>, IntrospectError> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
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
    .map_err(|e| IntrospectError::query(format!("Failed to list views: {e}")))?;

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

fn quote_ident(name: &str) -> String {
    let escaped = name.replace('"', "\"\"");
    format!(r#""{escaped}""#)
}

async fn pragma_table_info(
    pool: &SqlitePool,
    quoted_table: &str,
) -> Result<Vec<SqliteTableInfoRow>, IntrospectError> {
    let sql = format!("PRAGMA table_info({quoted_table})");
    sqlx::query_as::<_, SqliteTableInfoRow>(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| IntrospectError::query(format!("PRAGMA table_info failed: {e}")))
}

async fn pragma_foreign_key_list(
    pool: &SqlitePool,
    quoted_table: &str,
) -> Result<Vec<SqliteFkRow>, IntrospectError> {
    let sql = format!("PRAGMA foreign_key_list({quoted_table})");
    sqlx::query_as::<_, SqliteFkRow>(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| IntrospectError::query(format!("PRAGMA foreign_key_list failed: {e}")))
}

async fn pragma_index_list(
    pool: &SqlitePool,
    quoted_table: &str,
) -> Result<Vec<SqliteIndexListRow>, IntrospectError> {
    let sql = format!("PRAGMA index_list({quoted_table})");
    sqlx::query_as::<_, SqliteIndexListRow>(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| IntrospectError::query(format!("PRAGMA index_list failed: {e}")))
}

async fn pragma_index_info(
    pool: &SqlitePool,
    quoted_index: &str,
) -> Result<Vec<SqliteIndexInfoRow>, IntrospectError> {
    let sql = format!("PRAGMA index_info({quoted_index})");
    sqlx::query_as::<_, SqliteIndexInfoRow>(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| IntrospectError::query(format!("PRAGMA index_info failed: {e}")))
}

#[derive(Debug, sqlx::FromRow)]
struct SqliteTableInfoRow {
    cid: i64,
    name: String,
    #[sqlx(rename = "type")]
    col_type: String,
    notnull: i64,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
        let quoted_idx = quote_ident(&index_name);
        let mut info = pragma_index_info(pool, &quoted_idx).await?;
        info.sort_by_key(|r| r.seqno);
        let col_names: Vec<String> = info.into_iter().filter_map(|r| r.name).collect();
        if col_names.is_empty() {
            continue;
        }
        out.push(RawIndex {
            index_name: index_name.clone(),
            schema_name: MAIN_SCHEMA.to_string(),
            table_name: table_name.to_string(),
            columns: col_names,
            is_unique: entry.is_unique_flag != 0,
            is_primary: false,
        });
    }
    Ok(out)
}
