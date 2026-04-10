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

/// Returns the shared acquire timeout for connection pools.
#[must_use]
pub(crate) const fn acquire_timeout() -> Duration {
    ACQUIRE_TIMEOUT
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
pub(crate) async fn close_pool_when_done<DB, T, E, F>(pool: &Pool<DB>, operation: F) -> Result<T, E>
where
    DB: Database,
    F: Future<Output = Result<T, E>>,
{
    let result = operation.await;
    pool.close().await;
    result
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
}
