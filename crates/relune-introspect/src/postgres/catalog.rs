//! `PostgreSQL` catalog query functions.
//!
//! This module provides read-only metadata query functions for `PostgreSQL`
//! using sqlx. It queries the `PostgreSQL` system catalogs (`information_schema`
//! and `pg_catalog`) and returns common raw metadata types.

use sqlx::PgPool;

use crate::catalog::ParallelCatalogReader;
use crate::common::{
    RawColumn, RawEnum, RawForeignKey, RawIndex, RawSchema, RawTable, RawView,
    parse_referential_action,
};
use crate::connect::pool_max_connections_with_default;
use crate::error::IntrospectError;

/// `PostgreSQL` catalog reader.
///
/// Provides methods to query `PostgreSQL` system catalogs for schema metadata.
pub struct PostgresCatalog {
    pool: PgPool,
}

const PARALLEL_CATALOG_QUERIES: u32 = 6;
const FETCH_COLUMNS_QUERY: &str = r"
            SELECT
                t.relname AS table_name,
                n.nspname AS schema_name,
                a.attname AS column_name,
                pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
                CASE WHEN a.attnotnull THEN false ELSE true END AS is_nullable,
                COALESCE(pk.is_pk, false) AS is_primary_key,
                pg_catalog.col_description(a.attrelid, a.attnum) AS column_comment,
                a.attnum AS ordinal_position
            FROM pg_catalog.pg_attribute a
            INNER JOIN pg_catalog.pg_class t ON t.oid = a.attrelid
            INNER JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace
            LEFT JOIN (
                SELECT
                    i.indrelid,
                    a.attnum,
                    true AS is_pk
                FROM pg_catalog.pg_index i
                INNER JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
                WHERE i.indisprimary
            ) pk ON pk.indrelid = a.attrelid AND pk.attnum = a.attnum
            WHERE a.attnum > 0
                AND NOT a.attisdropped
                AND t.relkind IN ('r', 'v', 'm')
                AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                AND n.nspname NOT LIKE 'pg_%'
            ORDER BY n.nspname, t.relname, a.attnum
            ";
const FETCH_VIEWS_QUERY: &str = r"
            SELECT
                c.relname AS view_name,
                n.nspname AS schema_name,
                pg_catalog.pg_get_viewdef(c.oid, true) AS definition,
                pg_catalog.obj_description(c.oid, 'pg_class') AS view_comment
            FROM pg_catalog.pg_class c
            INNER JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind IN ('v', 'm')
                AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                AND n.nspname NOT LIKE 'pg_%'
            ORDER BY n.nspname, c.relname
            ";

impl PostgresCatalog {
    /// Create a new `PostgreSQL` catalog reader.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // `PgPool` is not constructible in `const` context
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ParallelCatalogReader for PostgresCatalog {
    /// Fetch all tables from the database, excluding system schemas.
    async fn fetch_tables(&self) -> Result<Vec<RawTable>, IntrospectError> {
        let rows: Vec<RawTableRow> = sqlx::query_as(
            r"
            SELECT
                c.relname AS table_name,
                n.nspname AS schema_name,
                pg_catalog.obj_description(c.oid, 'pg_class') AS table_comment
            FROM pg_catalog.pg_class c
            INNER JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind = 'r'
                AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                AND n.nspname NOT LIKE 'pg_%'
            ORDER BY n.nspname, c.relname
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

    /// Fetch all columns from the database, excluding system schemas.
    async fn fetch_columns(&self) -> Result<Vec<RawColumn>, IntrospectError> {
        let rows: Vec<RawColumnRow> = sqlx::query_as(FETCH_COLUMNS_QUERY)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| IntrospectError::query_with_source("Failed to fetch columns", e))?;

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
                ordinal_position: row.ordinal_position,
            })
            .collect())
    }

    /// Fetch all foreign keys from the database.
    async fn fetch_foreign_keys(&self) -> Result<Vec<RawForeignKey>, IntrospectError> {
        let rows: Vec<RawForeignKeyRow> = sqlx::query_as(
            r"
            SELECT
                tc.conname AS constraint_name,
                src_ns.nspname AS schema_name,
                src_cls.relname AS from_table,
                array_agg(src_attr.attname ORDER BY u.ord) AS from_columns,
                dst_ns.nspname AS to_schema,
                dst_cls.relname AS to_table,
                array_agg(dst_attr.attname ORDER BY u.ord) AS to_columns,
                tc.confdeltype::text AS on_delete_code,
                tc.confupdtype::text AS on_update_code
            FROM pg_catalog.pg_constraint tc
            INNER JOIN pg_catalog.pg_class src_cls ON src_cls.oid = tc.conrelid
            INNER JOIN pg_catalog.pg_namespace src_ns ON src_ns.oid = src_cls.relnamespace
            INNER JOIN pg_catalog.pg_class dst_cls ON dst_cls.oid = tc.confrelid
            INNER JOIN pg_catalog.pg_namespace dst_ns ON dst_ns.oid = dst_cls.relnamespace
            CROSS JOIN LATERAL UNNEST(tc.conkey, tc.confkey) WITH ORDINALITY AS u(src_attnum, dst_attnum, ord)
            INNER JOIN pg_catalog.pg_attribute src_attr ON src_attr.attrelid = tc.conrelid AND src_attr.attnum = u.src_attnum
            INNER JOIN pg_catalog.pg_attribute dst_attr ON dst_attr.attrelid = tc.confrelid AND dst_attr.attnum = u.dst_attnum
            WHERE tc.contype = 'f'
                AND src_ns.nspname NOT IN ('pg_catalog', 'information_schema')
                AND src_ns.nspname NOT LIKE 'pg_%'
                AND dst_ns.nspname NOT IN ('pg_catalog', 'information_schema')
                AND dst_ns.nspname NOT LIKE 'pg_%'
            GROUP BY tc.conname, src_ns.nspname, src_cls.relname, dst_ns.nspname, dst_cls.relname, tc.confdeltype, tc.confupdtype
            ORDER BY src_ns.nspname, src_cls.relname, tc.conname
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch foreign keys", e))?;

        Ok(rows
            .into_iter()
            .map(|row| RawForeignKey {
                constraint_name: row.constraint_name,
                schema_name: row.schema_name,
                from_table: row.from_table,
                from_columns: row.from_columns.unwrap_or_default(),
                to_schema: Some(row.to_schema),
                to_table: row.to_table,
                to_columns: row.to_columns.unwrap_or_default(),
                on_delete: parse_referential_action(&row.on_delete_code),
                on_update: parse_referential_action(&row.on_update_code),
            })
            .collect())
    }

    /// Fetch all indexes from the database.
    async fn fetch_indexes(&self) -> Result<Vec<RawIndex>, IntrospectError> {
        let rows: Vec<RawIndexRow> = sqlx::query_as(
            r"
            SELECT
                i.relname AS index_name,
                n.nspname AS schema_name,
                t.relname AS table_name,
                array_agg(a.attname ORDER BY array_position(ix.indkey, a.attnum)) AS columns,
                ix.indisunique AS is_unique,
                ix.indisprimary AS is_primary
            FROM pg_catalog.pg_index ix
            INNER JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid
            INNER JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid
            INNER JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace
            INNER JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(ix.indkey)
            WHERE t.relkind = 'r'
                AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                AND n.nspname NOT LIKE 'pg_%'
                -- Skip expression indexes: indkey entries of 0 represent
                -- expressions, which have no matching pg_attribute row and
                -- would silently truncate the index column list.
                AND 0 <> ALL(ix.indkey)
            GROUP BY i.relname, n.nspname, t.relname, ix.indisunique, ix.indisprimary
            ORDER BY n.nspname, t.relname, i.relname
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch indexes", e))?;

        Ok(rows
            .into_iter()
            .map(|row| RawIndex {
                index_name: row.index_name,
                schema_name: row.schema_name,
                table_name: row.table_name,
                columns: row.columns.unwrap_or_default(),
                is_unique: row.is_unique,
                is_primary: row.is_primary,
            })
            .collect())
    }

    /// Fetch all views from the database.
    async fn fetch_views(&self) -> Result<Vec<RawView>, IntrospectError> {
        let rows: Vec<RawViewRow> = sqlx::query_as(FETCH_VIEWS_QUERY)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| IntrospectError::query_with_source("Failed to fetch views", e))?;

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

    /// Fetch all enum types from the database.
    async fn fetch_enums(&self) -> Result<Vec<RawEnum>, IntrospectError> {
        let rows: Vec<RawEnumRow> = sqlx::query_as(
            r"
            SELECT
                t.typname AS enum_name,
                n.nspname AS schema_name,
                array_agg(e.enumlabel ORDER BY e.enumsortorder) AS values
            FROM pg_catalog.pg_type t
            INNER JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
            INNER JOIN pg_catalog.pg_enum e ON e.enumtypid = t.oid
            WHERE t.typtype = 'e'
                AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                AND n.nspname NOT LIKE 'pg_%'
            GROUP BY t.typname, n.nspname
            ORDER BY n.nspname, t.typname
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IntrospectError::query_with_source("Failed to fetch enums", e))?;

        Ok(rows
            .into_iter()
            .map(|row| RawEnum {
                enum_name: row.enum_name,
                schema_name: row.schema_name,
                values: row.values.unwrap_or_default(),
            })
            .collect())
    }
}

/// Returns the effective pool max connection count for `PostgreSQL`.
///
/// Defaults to `PARALLEL_CATALOG_QUERIES` so the parallel catalog fan-out
/// never queues on connection acquisition. Operators on constrained
/// environments can override the cap with `RELUNE_DB_POOL_MAX_CONNECTIONS`.
#[must_use]
pub(crate) fn pool_max_connections() -> u32 {
    pool_max_connections_with_default(PARALLEL_CATALOG_QUERIES)
}

/// Fetches all catalog metadata from a `PostgreSQL` database.
///
/// This is a convenience function that creates a `PostgresCatalog` and
/// fetches all metadata in parallel.
pub async fn fetch_catalog_metadata(pool: &PgPool) -> Result<RawSchema, IntrospectError> {
    let catalog = PostgresCatalog::new(pool.clone());
    catalog.fetch_all().await
}

// Internal row structs for sqlx mapping

#[derive(Debug, sqlx::FromRow)]
struct RawTableRow {
    table_name: String,
    schema_name: String,
    table_comment: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct RawColumnRow {
    table_name: String,
    schema_name: String,
    column_name: String,
    data_type: String,
    is_nullable: bool,
    is_primary_key: bool,
    column_comment: Option<String>,
    ordinal_position: i16,
}

#[derive(Debug, sqlx::FromRow)]
struct RawForeignKeyRow {
    constraint_name: String,
    schema_name: String,
    from_table: String,
    from_columns: Option<Vec<String>>,
    to_schema: String,
    to_table: String,
    to_columns: Option<Vec<String>>,
    on_delete_code: String,
    on_update_code: String,
}

#[derive(Debug, sqlx::FromRow)]
struct RawIndexRow {
    index_name: String,
    schema_name: String,
    table_name: String,
    columns: Option<Vec<String>>,
    is_unique: bool,
    is_primary: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct RawViewRow {
    view_name: String,
    schema_name: String,
    definition: Option<String>,
    view_comment: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct RawEnumRow {
    enum_name: String,
    schema_name: String,
    values: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use relune_core::ReferentialAction;

    use super::*;

    #[test]
    fn test_raw_table_fields() {
        let table = RawTable {
            table_name: "users".to_string(),
            schema_name: "public".to_string(),
            table_comment: Some("User accounts".to_string()),
        };
        assert_eq!(table.table_name, "users");
        assert_eq!(table.schema_name, "public");
    }

    #[test]
    fn test_raw_column_fields() {
        let column = RawColumn {
            table_name: "users".to_string(),
            schema_name: "public".to_string(),
            column_name: "id".to_string(),
            data_type: "integer".to_string(),
            is_nullable: false,
            is_primary_key: true,
            column_comment: None,
            ordinal_position: 1,
        };
        assert_eq!(column.column_name, "id");
        assert!(column.is_primary_key);
        assert!(!column.is_nullable);
    }

    #[test]
    fn test_raw_foreign_key_fields() {
        let fk = RawForeignKey {
            constraint_name: "fk_posts_user_id".to_string(),
            schema_name: "public".to_string(),
            from_table: "posts".to_string(),
            from_columns: vec!["user_id".to_string()],
            to_schema: Some("public".to_string()),
            to_table: "users".to_string(),
            to_columns: vec!["id".to_string()],
            on_delete: ReferentialAction::Cascade,
            on_update: ReferentialAction::NoAction,
        };
        assert_eq!(fk.constraint_name, "fk_posts_user_id");
        assert_eq!(fk.from_table, "posts");
        assert_eq!(fk.to_table, "users");
    }

    #[test]
    fn test_raw_index_fields() {
        let index = RawIndex {
            index_name: "idx_users_email".to_string(),
            schema_name: "public".to_string(),
            table_name: "users".to_string(),
            columns: vec!["email".to_string()],
            is_unique: true,
            is_primary: false,
        };
        assert_eq!(index.index_name, "idx_users_email");
        assert!(index.is_unique);
        assert!(!index.is_primary);
    }

    #[test]
    fn test_raw_view_fields() {
        let view = RawView {
            view_name: "active_users".to_string(),
            schema_name: "public".to_string(),
            definition: Some("SELECT * FROM users WHERE active = true".to_string()),
            view_comment: None,
        };
        assert_eq!(view.view_name, "active_users");
    }

    #[test]
    fn test_raw_enum_fields() {
        let enum_type = RawEnum {
            enum_name: "status".to_string(),
            schema_name: "public".to_string(),
            values: vec!["active".to_string(), "inactive".to_string()],
        };
        assert_eq!(enum_type.enum_name, "status");
        assert_eq!(enum_type.values.len(), 2);
    }

    #[test]
    fn test_raw_schema_default() {
        let schema = RawSchema::default();
        assert!(schema.tables.is_empty());
        assert!(schema.columns.is_empty());
        assert!(schema.foreign_keys.is_empty());
        assert!(schema.indexes.is_empty());
        assert!(schema.views.is_empty());
        assert!(schema.enums.is_empty());
    }

    #[test]
    fn pool_max_connections_is_positive() {
        // Whichever value wins (default or env override), it must remain
        // positive so the pool can be constructed.
        assert!(pool_max_connections() > 0);
    }

    #[test]
    fn fetch_columns_query_includes_views() {
        assert!(FETCH_COLUMNS_QUERY.contains("t.relkind IN ('r', 'v', 'm')"));
    }

    #[test]
    fn fetch_views_query_includes_materialized_views() {
        assert!(FETCH_VIEWS_QUERY.contains("c.relkind IN ('v', 'm')"));
    }
}
