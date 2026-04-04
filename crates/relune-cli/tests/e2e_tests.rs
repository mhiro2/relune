//! End-to-end tests that exercise the **built CLI binary** against live databases.
//!
//! Each test starts a throwaway database via testcontainers, seeds it with a
//! fixture, and then invokes the `relune` binary as a subprocess to verify that
//! the full pipeline (CLI → introspect → render / inspect / lint) works.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;
use testcontainers_modules::postgres::Postgres;

fn relune() -> Command {
    Command::cargo_bin("relune").expect("Failed to find relune binary")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sql")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Start a `PostgreSQL` container and return `(url, container)`.
async fn pg_container(sql: &str) -> (String, testcontainers::ContainerAsync<Postgres>) {
    let container = Postgres::default().start().await.expect("start postgres");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgresql://postgres:postgres@{host}:{port}/postgres");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect postgres");
    sqlx::raw_sql(sql).execute(&pool).await.expect("seed pg");
    pool.close().await;

    (url, container)
}

/// Start a `MySQL` container and return `(url, container)`.
async fn mysql_container(sql: &str) -> (String, testcontainers::ContainerAsync<Mysql>) {
    let container = Mysql::default().start().await.expect("start mysql");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(3306).await.expect("port");

    let admin_url = format!("mysql://root@{host}:{port}/mysql");
    let admin = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .expect("connect mysql admin");
    sqlx::raw_sql("CREATE DATABASE IF NOT EXISTS relune_e2e")
        .execute(&admin)
        .await
        .expect("create db");
    admin.close().await;

    let url = format!("mysql://root@{host}:{port}/relune_e2e");
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect mysql");
    sqlx::raw_sql(sql).execute(&pool).await.expect("seed mysql");
    pool.close().await;

    (url, container)
}

/// Create a temporary `SQLite` database and return `(url, tempdir)`.
async fn sqlite_db(sql: &str) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("e2e.db");

    let pool = sqlx::sqlite::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("connect sqlite");
    sqlx::raw_sql(sql)
        .execute(&pool)
        .await
        .expect("seed sqlite");
    pool.close().await;

    let abs = db_path.canonicalize().expect("canonicalize");
    let url = format!("sqlite://{}", abs.display());
    (url, dir)
}

// ===========================================================================
// PostgreSQL
// ===========================================================================

mod postgres {
    use super::*;

    #[tokio::test]
    async fn inspect_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        relune()
            .args(["inspect", "--db-url", &url, "--format", "json"])
            .assert()
            .success()
            .stdout(predicates::str::contains("\"tables\""));
    }

    #[tokio::test]
    async fn render_svg_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out.svg");

        relune()
            .args(["render", "--db-url", &url, "--out"])
            .arg(&out)
            .assert()
            .success();

        let content = fs::read_to_string(&out).unwrap();
        assert!(content.contains("<svg"), "expected SVG output");
    }

    #[tokio::test]
    async fn render_html_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out.html");

        relune()
            .args(["render", "--db-url", &url, "--format", "html", "--out"])
            .arg(&out)
            .assert()
            .success();

        let content = fs::read_to_string(&out).unwrap();
        assert!(
            content.contains("<!DOCTYPE html>") || content.contains("<html"),
            "expected HTML output"
        );
    }

    #[tokio::test]
    async fn lint_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        relune()
            .args(["lint", "--db-url", &url, "--format", "json"])
            .assert()
            .success();
    }

    #[tokio::test]
    async fn inspect_table_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        relune()
            .args(["inspect", "--db-url", &url, "--table", "posts"])
            .assert()
            .success()
            .stdout(predicates::str::contains("posts"));
    }

    #[tokio::test]
    async fn render_focus_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("ecommerce.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("focus.svg");

        relune()
            .args(["render", "--db-url", &url, "--focus", "orders", "--out"])
            .arg(&out)
            .assert()
            .success();

        assert!(out.exists(), "focused SVG should be created");
    }
}

// ===========================================================================
// MySQL
// ===========================================================================

mod mysql {
    use super::*;

    #[tokio::test]
    async fn inspect_mysql() {
        let sql = fs::read_to_string(fixtures_dir().join("mysql_ecommerce.sql")).unwrap();
        let (url, _c) = mysql_container(&sql).await;

        relune()
            .args(["inspect", "--db-url", &url, "--format", "json"])
            .assert()
            .success()
            .stdout(predicates::str::contains("\"tables\""));
    }

    #[tokio::test]
    async fn render_svg_mysql() {
        let sql = fs::read_to_string(fixtures_dir().join("mysql_ecommerce.sql")).unwrap();
        let (url, _c) = mysql_container(&sql).await;

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out.svg");

        relune()
            .args(["render", "--db-url", &url, "--out"])
            .arg(&out)
            .assert()
            .success();

        let content = fs::read_to_string(&out).unwrap();
        assert!(content.contains("<svg"), "expected SVG output");
    }

    #[tokio::test]
    async fn lint_mysql() {
        let sql = fs::read_to_string(fixtures_dir().join("mysql_ecommerce.sql")).unwrap();
        let (url, _c) = mysql_container(&sql).await;

        relune()
            .args(["lint", "--db-url", &url, "--format", "json"])
            .assert()
            .success();
    }
}

// ===========================================================================
// SQLite
// ===========================================================================

mod sqlite {
    use super::*;

    #[tokio::test]
    async fn inspect_sqlite() {
        let sql = fs::read_to_string(fixtures_dir().join("sqlite_blog.sql")).unwrap();
        let (url, _dir) = sqlite_db(&sql).await;

        relune()
            .args(["inspect", "--db-url", &url, "--format", "json"])
            .assert()
            .success()
            .stdout(predicates::str::contains("\"tables\""));
    }

    #[tokio::test]
    async fn render_svg_sqlite() {
        let sql = fs::read_to_string(fixtures_dir().join("sqlite_blog.sql")).unwrap();
        let (url, _dir) = sqlite_db(&sql).await;

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out.svg");

        relune()
            .args(["render", "--db-url", &url, "--out"])
            .arg(&out)
            .assert()
            .success();

        let content = fs::read_to_string(&out).unwrap();
        assert!(content.contains("<svg"), "expected SVG output");
    }

    #[tokio::test]
    async fn lint_sqlite() {
        let sql = fs::read_to_string(fixtures_dir().join("sqlite_blog.sql")).unwrap();
        let (url, _dir) = sqlite_db(&sql).await;

        relune()
            .args(["lint", "--db-url", &url, "--format", "json"])
            .assert()
            .success();
    }
}
