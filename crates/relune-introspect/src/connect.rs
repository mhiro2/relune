//! Connection hardening helpers for live introspection.

use std::future::Future;
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

use sqlx::mysql::{MySqlConnectOptions, MySqlConnection, MySqlDatabaseError, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{Database, Pool, query, query_scalar};
use tracing::warn;

use crate::error::{IntrospectError, connect_error};

/// `MySQL` `ER_UNKNOWN_SYSTEM_VARIABLE` error number.
///
/// Returned when a server predating the session statement-timeout variable
/// (older `MySQL`/`MariaDB`) is asked to `SET` it.
const ER_UNKNOWN_SYSTEM_VARIABLE: u16 = 1193;

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);
const STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const POOL_CLOSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound on the total wall-clock time spent fetching catalog metadata.
///
/// The per-statement deadline only bounds a single query, so a backend that
/// runs many sequential queries (notably `SQLite`, one set of `PRAGMA`s per
/// table) or a server that cannot enforce a session timeout could otherwise
/// accumulate unbounded total time. This deadline caps the whole catalog fetch.
const OVERALL_INTROSPECTION_TIMEOUT: Duration = Duration::from_mins(10);

/// Environment variable that overrides the default overall introspection deadline.
///
/// Accepts a positive integer number of seconds. Operators introspecting very
/// large schemas can raise the cap; values that are non-positive, non-numeric,
/// or empty are ignored and the built-in default applies.
pub(crate) const INTROSPECTION_TIMEOUT_ENV: &str = "RELUNE_DB_INTROSPECTION_TIMEOUT_SECS";

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

/// Returns the shared per-statement execution deadline.
///
/// `PostgreSQL`/`MySQL` enforce this at the database session level via
/// `statement_timeout`/`max_execution_time`. Backends that lack a server-side
/// equivalent (notably `SQLite`) can use this constant to wrap each query in
/// a client-side `tokio::time::timeout`, giving them the same upper bound on
/// hung queries.
#[must_use]
pub(crate) const fn statement_timeout() -> Duration {
    STATEMENT_TIMEOUT
}

/// Resolves the effective overall introspection deadline.
///
/// Reads `RELUNE_DB_INTROSPECTION_TIMEOUT_SECS`; a positive integer wins,
/// otherwise the built-in [`OVERALL_INTROSPECTION_TIMEOUT`] applies.
#[must_use]
pub(crate) fn introspection_timeout() -> Duration {
    introspection_timeout_override_from(std::env::var(INTROSPECTION_TIMEOUT_ENV).ok().as_deref())
        .unwrap_or(OVERALL_INTROSPECTION_TIMEOUT)
}

fn introspection_timeout_override_from(value: Option<&str>) -> Option<Duration> {
    value
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
}

/// Bounds a catalog-fetch future by the overall introspection deadline.
///
/// Returns [`IntrospectError::Timeout`] if the future does not resolve within
/// the deadline. Cancelling the future drops any in-flight queries; the caller
/// is expected to close the pool afterwards (see [`close_pool_when_done`]).
pub(crate) async fn with_introspection_deadline<T, F>(fut: F) -> Result<T, IntrospectError>
where
    F: Future<Output = Result<T, IntrospectError>>,
{
    run_with_deadline(introspection_timeout(), fut).await
}

async fn run_with_deadline<T, F>(deadline: Duration, fut: F) -> Result<T, IntrospectError>
where
    F: Future<Output = Result<T, IntrospectError>>,
{
    match tokio::time::timeout(deadline, fut).await {
        Ok(result) => result,
        Err(_) => Err(IntrospectError::timeout(format!(
            "Catalog introspection did not complete within {} seconds",
            deadline.as_secs()
        ))),
    }
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
///
/// Remote connections that do not opt into a TLS mode via the URL default to
/// `VerifyFull`, which validates both the certificate chain and the server
/// hostname. Users who connect to clusters with self-signed certificates can
/// still opt out by passing an explicit `sslmode=require` (encryption only)
/// in the URL; that explicit choice is respected.
pub(crate) fn postgres_connect_options(
    database_url: &str,
) -> Result<PgConnectOptions, IntrospectError> {
    let options = PgConnectOptions::from_str(database_url)
        .map_err(|error| connect_error("PostgreSQL", database_url, error))?;
    let options = if postgres_uses_local_transport(&options) || postgres_tls_is_enforced(&options) {
        options
    } else {
        options.ssl_mode(PgSslMode::VerifyFull)
    };

    Ok(options.options([(
        "statement_timeout",
        format!("{}ms", STATEMENT_TIMEOUT.as_millis()),
    )]))
}

/// Builds hardened MySQL/MariaDB connect options from a URL.
///
/// Remote connections that do not opt into a TLS mode via the URL default to
/// `VerifyIdentity`, which validates both the certificate chain and the
/// server hostname. Users who need to relax verification (for self-signed
/// certificates) can still opt out by passing an explicit `ssl-mode=required`
/// in the URL; that explicit choice is respected.
pub(crate) fn mysql_connect_options(
    database_url: &str,
) -> Result<MySqlConnectOptions, IntrospectError> {
    let options = MySqlConnectOptions::from_str(database_url)
        .map_err(|error| connect_error("MySQL", database_url, error))?;

    if mysql_uses_local_transport(&options) || mysql_tls_is_enforced(&options) {
        Ok(options)
    } else {
        Ok(options.ssl_mode(MySqlSslMode::VerifyIdentity))
    }
}

/// Configures a per-session statement execution deadline for `MySQL`/`MariaDB`.
///
/// Servers that predate the session timeout variable reject it as an unknown
/// system variable; that is downgraded to a warning so introspection still
/// proceeds (the overall introspection deadline remains the client-side bound).
/// A failure here must not abort the connection: sqlx retries `after_connect`
/// in a backoff loop until the acquire deadline, so a hard error would surface
/// as a misleading "connection timed out" after a 30 second hang.
pub(crate) async fn configure_mysql_session(
    connection: &mut MySqlConnection,
) -> Result<(), sqlx::Error> {
    let version = query_scalar::<_, String>("SELECT VERSION()")
        .fetch_one(&mut *connection)
        .await?;

    let outcome = if version.to_ascii_lowercase().contains("mariadb") {
        query("SET SESSION max_statement_time = ?")
            .bind(STATEMENT_TIMEOUT.as_secs_f64())
            .execute(&mut *connection)
            .await
    } else {
        query("SET SESSION max_execution_time = ?")
            .bind(statement_timeout_millis())
            .execute(&mut *connection)
            .await
    };

    match outcome {
        Ok(_) => Ok(()),
        Err(error) if is_unknown_system_variable(&error) => {
            warn!(
                error = %error,
                "MySQL server does not support a session statement timeout; \
                 relying on the overall introspection deadline instead"
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Returns true when a `MySQL` error is `ER_UNKNOWN_SYSTEM_VARIABLE`.
fn is_unknown_system_variable(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database_error| database_error.try_downcast_ref::<MySqlDatabaseError>())
        .is_some_and(|mysql_error| mysql_error.number() == ER_UNKNOWN_SYSTEM_VARIABLE)
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
    if host == "localhost" {
        return true;
    }
    // Strip an IPv6 zone identifier (e.g. `fe80::1%eth0`) before parsing so
    // link-local loopback addresses with explicit zone IDs are still
    // recognised as local. `IpAddr::parse` rejects the `%zone` suffix.
    let address_part = host.split_once('%').map_or(host.as_str(), |(addr, _)| addr);
    address_part
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_remote_connections_default_to_verify_full() {
        let options = postgres_connect_options("postgres://user:pass@example.com/app")
            .expect("postgres URL should parse");

        assert!(matches!(options.get_ssl_mode(), PgSslMode::VerifyFull));
        assert_eq!(options.get_options(), Some("-c statement_timeout=30000ms"));
    }

    #[test]
    fn postgres_explicit_require_is_respected_for_self_signed_clusters() {
        let options =
            postgres_connect_options("postgres://user:pass@example.com/app?sslmode=require")
                .expect("postgres URL should parse");

        assert!(matches!(options.get_ssl_mode(), PgSslMode::Require));
    }

    #[test]
    fn postgres_localhost_keeps_local_transport_without_tls_upgrade() {
        let options = postgres_connect_options("postgres://user:pass@127.0.0.1/app")
            .expect("postgres URL should parse");

        assert!(matches!(options.get_ssl_mode(), PgSslMode::Prefer));
        assert_eq!(options.get_options(), Some("-c statement_timeout=30000ms"));
    }

    #[test]
    fn mysql_remote_connections_default_to_verify_identity() {
        let options = mysql_connect_options("mysql://user:pass@example.com/app")
            .expect("mysql URL should parse");

        assert!(matches!(
            options.get_ssl_mode(),
            MySqlSslMode::VerifyIdentity
        ));
    }

    #[test]
    fn mysql_explicit_required_is_respected_for_self_signed_clusters() {
        let options = mysql_connect_options("mysql://user:pass@example.com/app?ssl-mode=required")
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
    fn is_unknown_system_variable_ignores_non_database_errors() {
        // Connection/pool-level errors carry no database error payload, so they
        // must never be mistaken for an unsupported session variable.
        assert!(!is_unknown_system_variable(&sqlx::Error::PoolClosed));
    }

    #[test]
    fn is_local_host_recognises_loopback_with_ipv6_zone_id() {
        assert!(is_local_host("::1%lo0"));
        assert!(is_local_host("[::1%eth0]"));
        assert!(!is_local_host("fe80::1%eth0"));
        assert!(!is_local_host("2001:db8::1%eth0"));
    }

    #[test]
    fn is_local_host_recognises_plain_loopback_addresses() {
        assert!(is_local_host("localhost"));
        assert!(is_local_host("127.0.0.1"));
        assert!(is_local_host("::1"));
        assert!(is_local_host("[::1]"));
    }

    #[test]
    fn introspection_timeout_override_accepts_positive_seconds() {
        assert_eq!(
            introspection_timeout_override_from(Some("125")),
            Some(Duration::from_secs(125))
        );
        assert_eq!(
            introspection_timeout_override_from(Some("  45 ")),
            Some(Duration::from_secs(45))
        );
    }

    #[test]
    fn introspection_timeout_override_rejects_invalid_or_non_positive_values() {
        assert_eq!(introspection_timeout_override_from(None), None);
        assert_eq!(introspection_timeout_override_from(Some("")), None);
        assert_eq!(introspection_timeout_override_from(Some("   ")), None);
        assert_eq!(introspection_timeout_override_from(Some("0")), None);
        assert_eq!(introspection_timeout_override_from(Some("-5")), None);
        assert_eq!(introspection_timeout_override_from(Some("soon")), None);
    }

    #[tokio::test(start_paused = true)]
    async fn run_with_deadline_times_out_when_future_exceeds_budget() {
        let deadline = Duration::from_secs(30);
        let result: Result<(), IntrospectError> = run_with_deadline(deadline, async {
            tokio::time::sleep(deadline * 2).await;
            Ok(())
        })
        .await;

        let err = result.expect_err("a future exceeding the deadline should time out");
        assert!(matches!(err, IntrospectError::Timeout(_)));
        assert!(err.to_string().contains("30 seconds"));
    }

    #[tokio::test]
    async fn run_with_deadline_returns_inner_result_within_budget() {
        let value = run_with_deadline(Duration::from_secs(30), async { Ok(11_u32) })
            .await
            .expect("a fast future should resolve within the deadline");
        assert_eq!(value, 11);
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
