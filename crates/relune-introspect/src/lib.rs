//! # relune-introspect
//!
//! Database introspection for relune.
//!
//! This crate provides database-specific adapters for extracting schema metadata
//! from live databases and converting it to the `Schema` type from `relune-core`.
//!
//! ## Supported Databases
//!
//! - **`PostgreSQL`** via the `postgres` module
//! - **`MySQL`** via the `mysql` module
//! - **`SQLite`** via the `sqlite` module
//!
//! ## Example
//!
//! ```rust,ignore
//! use relune_introspect::introspect_database;
//!
//! let schema = introspect_database(&database_url).await?;
//! println!("Found {} tables", schema.tables.len());
//! ```

pub mod common;
pub mod error;
pub mod mysql;
pub mod postgres;
pub mod sqlite;

mod url;

pub use mysql::introspect_mysql;
pub use postgres::introspect_postgres;
pub use sqlite::introspect_sqlite;

// Re-export error types
pub use error::IntrospectError;

/// Introspects a live database from a connection URL.
///
/// Supported schemes: `postgres://` / `postgresql://`, `mysql://`, `mariadb://`, and `sqlite:`.
pub async fn introspect_database(
    connection_url: &str,
) -> Result<relune_core::Schema, IntrospectError> {
    let t = connection_url.trim();
    if t.is_empty() {
        return Err(IntrospectError::invalid_url("Database URL cannot be empty"));
    }
    let prefix = t.to_ascii_lowercase();
    if prefix.starts_with("postgres://") || prefix.starts_with("postgresql://") {
        introspect_postgres(&url::lowercase_scheme(t)).await
    } else if prefix.starts_with("mysql://") {
        introspect_mysql(&url::lowercase_scheme(t)).await
    } else if prefix.starts_with("mariadb://") {
        introspect_mysql(&mysql::normalize_mysql_url(&url::lowercase_scheme(t))).await
    } else if prefix.starts_with("sqlite:") {
        introspect_sqlite(t).await
    } else {
        Err(IntrospectError::invalid_url(
            "Unsupported database URL; expected postgres://, mysql://, mariadb://, or sqlite:...",
        ))
    }
}

// Re-export types from relune-core that are relevant for introspection consumers
pub use relune_core::{Column, ColumnId, Enum, ForeignKey, Index, Schema, Table, TableId, View};
