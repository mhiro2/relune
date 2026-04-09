//! Error types for the relune introspection crate.
//!
//! This module defines error types for database introspection operations,
//! including connection errors, query failures, and metadata mapping issues.

use sqlx::Error as SqlxError;
use thiserror::Error;

/// Main error type for introspection operations.
#[derive(Debug, Error)]
pub enum IntrospectError {
    /// Database connection failed.
    #[error("Database connection error: {0}")]
    Connection(String),

    /// Failed to connect due to invalid database URL.
    #[error("Invalid database URL: {0}")]
    InvalidDatabaseUrl(String),

    /// Query execution failed.
    #[error("Query error: {0}")]
    Query(String),

    /// Failed to map database metadata to schema objects.
    #[error("Metadata mapping error: {0}")]
    MetadataMapping(String),

    /// Operation timed out.
    #[error("Timeout error: {0}")]
    Timeout(String),

    /// I/O error during introspection.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl IntrospectError {
    /// Create a new connection error.
    pub fn connection(msg: impl Into<String>) -> Self {
        Self::Connection(msg.into())
    }

    /// Create a new invalid database URL error.
    pub fn invalid_url(msg: impl Into<String>) -> Self {
        Self::InvalidDatabaseUrl(msg.into())
    }

    /// Create a new query error.
    pub fn query(msg: impl Into<String>) -> Self {
        Self::Query(msg.into())
    }

    /// Create a new metadata mapping error.
    pub fn metadata_mapping(msg: impl Into<String>) -> Self {
        Self::MetadataMapping(msg.into())
    }

    /// Create a new timeout error.
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }

    /// Returns a copy of this error with any embedded database URL sanitized.
    #[must_use]
    pub fn sanitized_for_url(self, database_url: &str) -> Self {
        match self {
            Self::Connection(message) => {
                Self::Connection(crate::url::sanitize_error_message(database_url, &message))
            }
            Self::InvalidDatabaseUrl(message) => {
                Self::InvalidDatabaseUrl(crate::url::sanitize_error_message(database_url, &message))
            }
            Self::Query(message) => {
                Self::Query(crate::url::sanitize_error_message(database_url, &message))
            }
            Self::MetadataMapping(message) => {
                Self::MetadataMapping(crate::url::sanitize_error_message(database_url, &message))
            }
            Self::Timeout(message) => {
                Self::Timeout(crate::url::sanitize_error_message(database_url, &message))
            }
            Self::Io(error) => Self::Io(error),
        }
    }
}

/// Convert a `sqlx` connect error into a sanitized introspection error.
pub(crate) fn connect_error(
    database_name: &'static str,
    database_url: &str,
    error: SqlxError,
) -> IntrospectError {
    match error {
        SqlxError::Configuration(_) => {
            IntrospectError::invalid_url(format!("{database_name} database URL is invalid"))
        }
        SqlxError::PoolTimedOut => {
            IntrospectError::timeout(format!("{database_name} connection timed out"))
        }
        other => IntrospectError::connection(format!(
            "{database_name} connection failed: {}",
            sanitize_connect_error_message(database_url, &other.to_string())
        )),
    }
}

fn sanitize_connect_error_message(database_url: &str, message: &str) -> String {
    crate::url::sanitize_error_message(database_url, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_error() {
        let err = IntrospectError::connection("Failed to connect to localhost:5432");
        assert!(err.to_string().contains("Database connection error"));
        assert!(err.to_string().contains("localhost:5432"));
    }

    #[test]
    fn test_invalid_url_error() {
        let err = IntrospectError::invalid_url("Missing scheme");
        assert!(err.to_string().contains("Invalid database URL"));
    }

    #[test]
    fn test_query_error() {
        let err = IntrospectError::query("Table 'users' does not exist");
        assert!(err.to_string().contains("Query error"));
    }

    #[test]
    fn test_metadata_mapping_error() {
        let err = IntrospectError::metadata_mapping("Unknown column type");
        assert!(err.to_string().contains("Metadata mapping error"));
    }

    #[test]
    fn test_timeout_error() {
        let err = IntrospectError::timeout("Connection timed out after 30s");
        assert!(err.to_string().contains("Connection timed out after 30s"));
    }

    #[test]
    fn test_connect_error_classification() {
        let invalid = connect_error(
            "PostgreSQL",
            "postgres://user:pass@localhost/db",
            SqlxError::Configuration(
                std::io::Error::other("postgres://user:pass@localhost/db").into(),
            ),
        );
        assert!(matches!(invalid, IntrospectError::InvalidDatabaseUrl(_)));
        assert!(!invalid.to_string().contains("postgres://"));

        let timeout = connect_error(
            "MySQL",
            "mysql://user:pass@localhost/db",
            SqlxError::PoolTimedOut,
        );
        assert!(matches!(timeout, IntrospectError::Timeout(_)));

        let connection = connect_error(
            "SQLite",
            "sqlite://tmp.db",
            SqlxError::Io(std::io::Error::other("down")),
        );
        assert!(matches!(connection, IntrospectError::Connection(_)));
        assert!(connection.to_string().contains("SQLite connection failed"));
    }

    #[test]
    fn test_connect_error_masks_credentials_in_connection_message() {
        let connection = connect_error(
            "PostgreSQL",
            "postgres://user:secret@localhost/db",
            SqlxError::Io(std::io::Error::other(
                "failed to connect to postgres://user:secret@localhost/db",
            )),
        );

        assert!(matches!(connection, IntrospectError::Connection(_)));
        assert!(
            connection
                .to_string()
                .contains("postgres://***:***@localhost/db")
        );
        assert!(
            !connection
                .to_string()
                .contains("postgres://user:secret@localhost/db")
        );
    }

    #[test]
    fn sanitized_for_url_masks_query_secrets() {
        let error = IntrospectError::query(
            "failed against postgres://user:secret@localhost/db?password=hunter2&token=abc",
        )
        .sanitized_for_url("postgres://user:secret@localhost/db?password=hunter2&token=abc");

        assert!(matches!(error, IntrospectError::Query(_)));
        assert!(
            error
                .to_string()
                .contains("postgres://***:***@localhost/db?password=***&token=***")
        );
        assert!(!error.to_string().contains("hunter2"));
        assert!(!error.to_string().contains("token=abc"));
    }
}
