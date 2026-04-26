//! Connection hardening helpers for live introspection.

use std::future::Future;
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

use sqlx::mysql::{MySqlConnectOptions, MySqlConnection, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{Database, Pool, query, query_scalar};

use crate::error::{IntrospectError, connect_error};

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);
const STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const POOL_CLOSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Environment variable that overrides the default per-dialect pool max.
///
/// Each dialect picks its own default (`PARALLEL_CATALOG_QUERIES` for
/// `PostgreSQL`/`MySQL`, single-writer for `SQLite`). When set to a positive
/// integer, this env var raises or lowers the cap so constrained CI runners
/// or larger introspection workloads can tune the pool size without code
/// changes. Non-positive or non-numeric values are ignored.
pub(crate) const POOL_MAX_CONNECTIONS_ENV: &str = "RELUNE_DB_POOL_MAX_CONNECTIONS";

/// Returns the shared acquire timeout for connection pools.
#[must_use]
pub(crate) const fn acquire_timeout() -> Duration {
    ACQUIRE_TIMEOUT
}

/// Resolves the effective pool max connection count for a dialect.
///
/// Reads `RELUNE_DB_POOL_MAX_CONNECTIONS`; if set to a positive integer the
/// override wins, otherwise falls back to `default_max`. Invalid values
/// (zero, negative, non-numeric, empty) are ignored.
#[must_use]
pub(crate) fn pool_max_connections_with_default(default_max: u32) -> u32 {
    pool_max_override_from(std::env::var(POOL_MAX_CONNECTIONS_ENV).ok().as_deref())
        .unwrap_or(default_max)
}

fn pool_max_override_from(value: Option<&str>) -> Option<u32> {
    value
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| raw.parse::<u32>().ok())
        .filter(|parsed| *parsed > 0)
}

/// Builds hardened `PostgreSQL` connect options from a URL.
pub(crate) fn postgres_connect_options(
    database_url: &str,
) -> Result<PgConnectOptions, IntrospectError> {
    let options = PgConnectOptions::from_str(database_url)
        .map_err(|error| connect_error("PostgreSQL", database_url, error))?;
    let options = if postgres_uses_local_transport(&options) || postgres_tls_is_enforced(&options) {
        options
    } else {
        options.ssl_mode(PgSslMode::Require)
    };

    Ok(options.options([(
        "statement_timeout",
        format!("{}ms", STATEMENT_TIMEOUT.as_millis()),
    )]))
}

/// Builds hardened MySQL/MariaDB connect options from a URL.
pub(crate) fn mysql_connect_options(
    database_url: &str,
) -> Result<MySqlConnectOptions, IntrospectError> {
    let options = MySqlConnectOptions::from_str(database_url)
        .map_err(|error| connect_error("MySQL", database_url, error))?;

    if mysql_uses_local_transport(&options) || mysql_tls_is_enforced(&options) {
        Ok(options)
    } else {
        Ok(options.ssl_mode(MySqlSslMode::Required))
    }
}

/// Configures a per-session statement execution deadline for `MySQL`/`MariaDB`.
pub(crate) async fn configure_mysql_session(
    connection: &mut MySqlConnection,
) -> Result<(), sqlx::Error> {
    let version = query_scalar::<_, String>("SELECT VERSION()")
        .fetch_one(&mut *connection)
        .await?;

    if version.to_ascii_lowercase().contains("mariadb") {
        query("SET SESSION max_statement_time = ?")
            .bind(STATEMENT_TIMEOUT.as_secs_f64())
            .execute(&mut *connection)
            .await?;
    } else {
        query("SET SESSION max_execution_time = ?")
            .bind(statement_timeout_millis())
            .execute(&mut *connection)
            .await?;
    }

    Ok(())
}

/// Ensures explicit pool draining runs before returning from introspection.
///
/// Surfaces drain-timeout failures even when the operation itself succeeded:
/// a hung close (e.g., a connection that never finishes draining) is reported
/// as `IntrospectError::Timeout` rather than disappearing into a successful
/// return. If the operation already failed, that error wins so the original
/// cause is not masked by cleanup state.
pub(crate) async fn close_pool_when_done<DB, T, F>(
    pool: &Pool<DB>,
    operation: F,
) -> Result<T, IntrospectError>
where
    DB: Database,
    F: Future<Output = Result<T, IntrospectError>>,
{
    let op_result = operation.await;
    let close_result = tokio::time::timeout(POOL_CLOSE_TIMEOUT, pool.close()).await;
    match (op_result, close_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(_)) => Err(IntrospectError::timeout(format!(
            "Connection pool drain did not complete within {} seconds",
            POOL_CLOSE_TIMEOUT.as_secs()
        ))),
        (Err(error), _) => Err(error),
    }
}

fn statement_timeout_millis() -> u64 {
    u64::try_from(STATEMENT_TIMEOUT.as_millis()).unwrap_or(u64::MAX)
}

fn postgres_uses_local_transport(options: &PgConnectOptions) -> bool {
    options.get_socket().is_some() || is_local_host(options.get_host())
}

fn mysql_uses_local_transport(options: &MySqlConnectOptions) -> bool {
    options.get_socket().is_some() || is_local_host(options.get_host())
}

fn postgres_tls_is_enforced(options: &PgConnectOptions) -> bool {
    matches!(
        options.get_ssl_mode(),
        PgSslMode::Require | PgSslMode::VerifyCa | PgSslMode::VerifyFull
    )
}

fn mysql_tls_is_enforced(options: &MySqlConnectOptions) -> bool {
    matches!(
        options.get_ssl_mode(),
        MySqlSslMode::Required | MySqlSslMode::VerifyCa | MySqlSslMode::VerifyIdentity
    )
}

fn is_local_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    matches!(host.as_str(), "localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_remote_connections_require_tls() {
        let options = postgres_connect_options("postgres://user:pass@example.com/app")
            .expect("postgres URL should parse");

        assert!(matches!(options.get_ssl_mode(), PgSslMode::Require));
        assert_eq!(options.get_options(), Some("-c statement_timeout=30000ms"));
    }

    #[test]
    fn postgres_localhost_keeps_local_transport_without_tls_upgrade() {
        let options = postgres_connect_options("postgres://user:pass@127.0.0.1/app")
            .expect("postgres URL should parse");

        assert!(matches!(options.get_ssl_mode(), PgSslMode::Prefer));
        assert_eq!(options.get_options(), Some("-c statement_timeout=30000ms"));
    }

    #[test]
    fn mysql_remote_connections_require_tls() {
        let options = mysql_connect_options("mysql://user:pass@example.com/app")
            .expect("mysql URL should parse");

        assert!(matches!(options.get_ssl_mode(), MySqlSslMode::Required));
    }

    #[test]
    fn mysql_local_socket_does_not_force_tls() {
        let options =
            mysql_connect_options("mysql://user:pass@localhost/app?socket=/tmp/mysql.sock")
                .expect("mysql URL should parse");

        assert!(matches!(options.get_ssl_mode(), MySqlSslMode::Preferred));
    }

    #[tokio::test]
    async fn close_pool_when_done_returns_operation_error_when_close_succeeds() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");

        let result: Result<(), IntrospectError> = close_pool_when_done(&pool, async {
            Err(IntrospectError::query("synthetic operation failure"))
        })
        .await;

        let err = result.expect_err("operation error should be surfaced");
        assert!(matches!(err, IntrospectError::Query { .. }));
        assert!(err.to_string().contains("synthetic operation failure"));
    }

    #[test]
    fn pool_max_override_accepts_positive_integers() {
        assert_eq!(pool_max_override_from(Some("12")), Some(12));
        assert_eq!(pool_max_override_from(Some("  3 ")), Some(3));
    }

    #[test]
    fn pool_max_override_rejects_invalid_or_non_positive_values() {
        assert_eq!(pool_max_override_from(None), None);
        assert_eq!(pool_max_override_from(Some("")), None);
        assert_eq!(pool_max_override_from(Some("   ")), None);
        assert_eq!(pool_max_override_from(Some("0")), None);
        assert_eq!(pool_max_override_from(Some("-4")), None);
        assert_eq!(pool_max_override_from(Some("foo")), None);
        assert_eq!(pool_max_override_from(Some("12abc")), None);
    }

    #[tokio::test]
    async fn close_pool_when_done_returns_value_when_both_succeed() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");

        let result = close_pool_when_done(&pool, async { Ok(42_u32) }).await;
        assert_eq!(result.expect("operation succeeds and close completes"), 42);
    }
}
