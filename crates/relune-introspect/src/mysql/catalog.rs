//! `MySQL` `information_schema` catalog queries.

use std::collections::BTreeMap;

use sqlx::MySqlPool;

use crate::common::{RawColumn, RawForeignKey, RawIndex, RawSchema, RawTable, RawView};
use crate::error::IntrospectError;

/// Fetches all catalog metadata from a `MySQL` database.
pub async fn fetch_catalog_metadata(pool: &MySqlPool) -> Result<RawSchema, IntrospectError> {
    let catalog = MySqlCatalog { pool: pool.clone() };
    catalog.fetch_all().await
}

struct MySqlCatalog {
    pool: MySqlPool,
}

impl MySqlCatalog {
    async fn fetch_all(&self) -> Result<RawSchema, IntrospectError> {
        let (tables, columns, fk_rows, index_rows, views) = tokio::try_join!(
            self.fetch_tables(),
            self.fetch_columns(),
            self.fetch_foreign_key_rows(),
            self.fetch_index_rows(),
            self.fetch_views(),
        )?;

        let foreign_keys = group_foreign_keys(fk_rows);
        let indexes = group_indexes(index_rows);

        Ok(RawSchema {
            tables,
            columns,
            foreign_keys,
            indexes,
            views,
            enums: Vec::new(),
        })
    }

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
        .map_err(|e| IntrospectError::query(format!("Failed to fetch tables: {e}")))?;

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
                ORDINAL_POSITION AS ordinal_position
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
        .map_err(|e| IntrospectError::query(format!("Failed to fetch columns: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|row| RawColumn {
                table_name: row.table_name,
                schema_name: row.schema_name,
                column_name: row.column_name,
                data_type: row.data_type,
                is_nullable: row.is_nullable,
                is_primary_key: row.is_primary_key,
                column_comment: row.column_comment,
                ordinal_position: row.ordinal_position.try_into().unwrap_or(i16::MAX),
            })
            .collect())
    }

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
                kcu.ORDINAL_POSITION AS ordinal_position
            FROM information_schema.KEY_COLUMN_USAGE kcu
            INNER JOIN information_schema.TABLE_CONSTRAINTS tc
                ON kcu.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
                AND kcu.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
                AND kcu.TABLE_SCHEMA = tc.TABLE_SCHEMA
                AND kcu.TABLE_NAME = tc.TABLE_NAME
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
        .map_err(|e| IntrospectError::query(format!("Failed to fetch foreign keys: {e}")))?;

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
                SEQ_IN_INDEX AS seq_in_index,
                NON_UNIQUE AS non_unique,
                IF(INDEX_NAME = 'PRIMARY', TRUE, FALSE) AS is_primary
            FROM information_schema.STATISTICS
            WHERE TABLE_SCHEMA NOT IN (
                'information_schema', 'mysql', 'performance_schema', 'sys',
                'mysql_innodb_cluster_metadata'
              )
              AND COLUMN_NAME IS NOT NULL
            ORDER BY TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query(format!("Failed to fetch indexes: {e}")))?;

        Ok(rows)
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
        .map_err(|e| IntrospectError::query(format!("Failed to fetch views: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|row| RawView {
                view_name: row.view_name,
                schema_name: row.schema_name,
                definition: row.definition,
                view_comment: row.view_comment,
            })
            .collect())
    }
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
}

#[derive(Debug, sqlx::FromRow, Clone)]
struct IndexColumnRow {
    schema_name: String,
    table_name: String,
    index_name: String,
    column_name: String,
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
        out.push(RawForeignKey {
            constraint_name: key.constraint,
            schema_name: key.schema,
            from_table: key.table,
            from_columns,
            to_schema,
            to_table,
            to_columns,
        });
    }
    out
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
        let index_columns: Vec<String> = cols.iter().map(|r| r.column_name.clone()).collect();
        out.push(RawIndex {
            index_name: key.index.clone(),
            schema_name: key.schema,
            table_name: key.table,
            columns: index_columns,
            is_unique: first.non_unique == 0 && !first.is_primary,
            is_primary: first.is_primary,
        });
    }
    out
}
