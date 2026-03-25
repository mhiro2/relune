//! `PostgreSQL` database introspection module.
//!
//! This module provides functionality to introspect `PostgreSQL` databases and
//! extract schema metadata, including tables, columns, foreign keys, indexes,
//! and enums.
//!
//! ## Architecture
//!
//! The module is organized into two main submodules:
//!
//! - **`catalog`**: Queries the `PostgreSQL` system catalogs (`information_schema`,
//!   `pg_catalog`) to fetch raw metadata about database objects.
//! - **`map`**: Converts the raw catalog data into the `Schema` type from
//!   `relune-core`, handling type mappings and relationship resolution.
//!
//! ## Example
//!
//! ```rust,ignore
//! use relune_introspect::postgres::introspect_postgres;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let database_url = "postgresql://user:password@localhost:5432/mydb";
//!     let schema = introspect_postgres(database_url).await?;
//!
//!     println!("Database: {} tables, {} views", schema.tables.len(), schema.views.len());
//!     Ok(())
//! }
//! ```
//!
//! ## Error Handling
//!
//! The `introspect_postgres` function returns `IntrospectError` for various
//! failure scenarios:
//!
//! - `Connection`: Failed to establish a database connection
//! - `InvalidDatabaseUrl`: The provided URL is malformed
//! - `Query`: Failed to execute a catalog query
//! - `MetadataMapping`: Failed to convert catalog data to schema objects

pub mod catalog;
pub mod map;

use relune_core::Schema;
use sqlx::postgres::PgPoolOptions;
use tracing::{debug, error, info, instrument};

use crate::error::IntrospectError;

/// Introspects a `PostgreSQL` database and extracts its schema metadata.
///
/// This function connects to the specified `PostgreSQL` database using the
/// provided connection URL, queries the system catalogs to fetch metadata
/// about tables, columns, foreign keys, indexes, and enums, and converts
/// the raw data into a structured `Schema` object.
///
/// # Arguments
///
/// * `database_url` - A `PostgreSQL` connection URL string. The URL should be
///   in the format: `postgresql://[user[:password]@][host][:port][/database][?param1=val1&...]`
///
/// # Returns
///
/// A `Result` containing:
/// - `Ok(Schema)` - The successfully extracted database schema
/// - `Err(IntrospectError)` - An error occurred during introspection
///
/// # Errors
///
/// This function can return the following errors:
///
/// - `IntrospectError::InvalidDatabaseUrl` - The connection URL is malformed
/// - `IntrospectError::Connection` - Failed to establish a connection to the database
/// - `IntrospectError::Query` - Failed to execute a catalog query
/// - `IntrospectError::MetadataMapping` - Failed to map catalog data to schema objects
///
/// # Example
///
/// ```rust,ignore
/// use relune_introspect::postgres::introspect_postgres;
///
/// async fn get_schema() -> Result<Schema, IntrospectError> {
///     let url = "postgresql://postgres:secret@localhost:5432/myapp";
///     introspect_postgres(url).await
/// }
/// ```
#[instrument(skip_all, fields(database_url = %crate::url::mask_credentials(database_url)))]
pub async fn introspect_postgres(database_url: &str) -> Result<Schema, IntrospectError> {
    info!("Starting PostgreSQL introspection");

    let database_url = crate::url::lowercase_scheme(database_url);

    // Validate the database URL
    if database_url.is_empty() {
        error!("Database URL is empty");
        return Err(IntrospectError::invalid_url("Database URL cannot be empty"));
    }

    if !database_url.starts_with("postgresql://") && !database_url.starts_with("postgres://") {
        error!("Invalid database URL scheme");
        return Err(IntrospectError::invalid_url(
            "Database URL must start with 'postgresql://' or 'postgres://'",
        ));
    }

    debug!("Connecting to PostgreSQL database");

    // Create a connection pool
    let pool = PgPoolOptions::new()
        .max_connections(catalog::pool_max_connections())
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect(&database_url)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to connect to database");
            if e.to_string().contains("invalid connection string") {
                IntrospectError::invalid_url(e.to_string())
            } else {
                IntrospectError::connection(e.to_string())
            }
        })?;

    debug!("Successfully connected to database");

    // Fetch metadata from catalog
    info!("Fetching database catalog metadata");
    let raw_schema = catalog::fetch_catalog_metadata(&pool).await?;

    debug!(
        tables = raw_schema.tables.len(),
        views = raw_schema.views.len(),
        "Retrieved raw catalog data"
    );

    // Convert raw catalog data to Schema
    info!("Mapping catalog metadata to Schema");
    let schema = map::map_to_schema(raw_schema)?;

    info!(
        tables = schema.tables.len(),
        views = schema.views.len(),
        enums = schema.enums.len(),
        "Introspection completed successfully"
    );

    // Pool::close() is infallible in sqlx 0.8; await it so connections drain cleanly.
    pool.close().await;

    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_url_returns_error() {
        let result = introspect_postgres("").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntrospectError::InvalidDatabaseUrl(_)
        ));
    }

    #[tokio::test]
    async fn test_invalid_url_scheme_returns_error() {
        let result = introspect_postgres("mysql://localhost:3306/mydb").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntrospectError::InvalidDatabaseUrl(_)
        ));
    }
}
