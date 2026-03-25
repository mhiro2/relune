//! `SQLite` database introspection.

pub mod catalog;

use relune_core::Schema;
use sqlx::sqlite::SqlitePoolOptions;
use tracing::{debug, error, info, instrument};

use crate::error::IntrospectError;

const POOL_MAX_CONNECTIONS: u32 = 1;

/// Introspects a `SQLite` database and extracts its schema metadata.
///
/// Accepts URLs understood by `sqlx`, for example `sqlite://path/to.db`,
/// `sqlite:///absolute/path.db`, or `sqlite::memory:`.
#[instrument(skip_all, fields(database_url = %crate::url::mask_credentials(database_url)))]
pub async fn introspect_sqlite(database_url: &str) -> Result<Schema, IntrospectError> {
    info!("Starting SQLite introspection");

    let trimmed = database_url.trim();
    if trimmed.is_empty() {
        error!("Database URL is empty");
        return Err(IntrospectError::invalid_url("Database URL cannot be empty"));
    }

    if !trimmed.to_ascii_lowercase().starts_with("sqlite:") {
        error!("Invalid database URL scheme");
        return Err(IntrospectError::invalid_url(
            "Database URL must start with 'sqlite:'",
        ));
    }

    debug!("Connecting to SQLite database");

    let pool = SqlitePoolOptions::new()
        .max_connections(pool_max_connections())
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect(trimmed)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to connect to database");
            if e.to_string().contains("invalid") {
                IntrospectError::invalid_url(e.to_string())
            } else {
                IntrospectError::connection(e.to_string())
            }
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

#[must_use]
pub(crate) const fn pool_max_connections() -> u32 {
    POOL_MAX_CONNECTIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_url_returns_error() {
        let result = introspect_sqlite("").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntrospectError::InvalidDatabaseUrl(_)
        ));
    }

    #[tokio::test]
    async fn test_invalid_url_scheme_returns_error() {
        let result = introspect_sqlite("mysql://localhost/db").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntrospectError::InvalidDatabaseUrl(_)
        ));
    }

    #[test]
    fn test_pool_max_connections_matches_execution_model() {
        assert_eq!(pool_max_connections(), POOL_MAX_CONNECTIONS);
    }
}
