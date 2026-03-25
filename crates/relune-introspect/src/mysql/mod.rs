//! `MySQL` database introspection module.
//!
//! This module provides functionality to introspect `MySQL` databases and
//! extract schema metadata, including tables, columns, foreign keys, and indexes.

pub mod catalog;

use relune_core::Schema;
use sqlx::mysql::MySqlPoolOptions;
use tracing::{debug, error, info, instrument};

use crate::error::IntrospectError;

const MARIADB_SCHEME_PREFIX: &str = "mariadb://";

/// Introspects a `MySQL` database and extracts its schema metadata.
///
/// # Arguments
///
/// * `database_url` - A `MySQL` connection URL string in the format:
///   `mysql://[user[:password]@][host][:port][/database][?param1=val1&...]`
#[instrument(skip_all, fields(database_url = %crate::url::mask_credentials(database_url)))]
pub async fn introspect_mysql(database_url: &str) -> Result<Schema, IntrospectError> {
    info!("Starting MySQL introspection");

    if database_url.trim().is_empty() {
        error!("Database URL is empty");
        return Err(IntrospectError::invalid_url("Database URL cannot be empty"));
    }

    let connect_url = normalize_mysql_url(&crate::url::lowercase_scheme(database_url));
    let lc = connect_url.to_ascii_lowercase();
    if !lc.starts_with("mysql://") {
        error!("Invalid database URL scheme");
        return Err(IntrospectError::invalid_url(
            "Database URL must start with 'mysql://' or 'mariadb://'",
        ));
    }

    debug!("Connecting to MySQL database");

    let pool = MySqlPoolOptions::new()
        .max_connections(catalog::pool_max_connections())
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect(&connect_url)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to connect to database");
            IntrospectError::connection(e.to_string())
        })?;

    debug!("Successfully connected to database");

    info!("Fetching database catalog metadata");
    let raw_schema = catalog::fetch_catalog_metadata(&pool).await?;

    debug!(
        tables = raw_schema.tables.len(),
        "Retrieved raw catalog data"
    );

    info!("Mapping catalog metadata to Schema");
    let schema = crate::common::map_to_schema(raw_schema)?;

    info!(
        tables = schema.tables.len(),
        "Introspection completed successfully"
    );

    pool.close().await;

    Ok(schema)
}

/// Rewrites `mariadb://` to `mysql://` for sqlx.
pub(crate) fn normalize_mysql_url(url: &str) -> String {
    let t = url.trim();
    if t.get(..MARIADB_SCHEME_PREFIX.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(MARIADB_SCHEME_PREFIX))
    {
        format!("mysql://{}", &t[MARIADB_SCHEME_PREFIX.len()..])
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_url_returns_error() {
        let result = introspect_mysql("").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntrospectError::InvalidDatabaseUrl(_)
        ));
    }

    #[test]
    fn normalize_mariadb_prefix() {
        assert_eq!(
            normalize_mysql_url("mariadb://user@h/db"),
            "mysql://user@h/db"
        );
    }

    #[tokio::test]
    async fn test_invalid_url_scheme_returns_error() {
        let result = introspect_mysql("postgresql://localhost:5432/mydb").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntrospectError::InvalidDatabaseUrl(_)
        ));
    }
}
