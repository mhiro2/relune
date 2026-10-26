//! End-to-end tests that exercise the **built CLI binary** against live databases.
//!
//! Each test starts a throwaway database via testcontainers, seeds it with a
//! fixture, and then invokes the `relune` binary as a subprocess to verify that
//! the full pipeline (CLI → introspect → render / inspect / lint) works.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;
use testcontainers_modules::postgres::Postgres;

const POSTGRES_TAG: &str = "18";

fn relune() -> Command {
    let mut cmd = Command::cargo_bin("relune").expect("Failed to find relune binary");
    cmd.env_remove("DATABASE_URL");
    cmd.env_remove("RUST_LOG");
    cmd
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sql")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Start a `PostgreSQL` container and return `(url, container)`.
async fn pg_container(sql: &str) -> (String, testcontainers::ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_tag(POSTGRES_TAG)
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgresql://postgres:postgres@{host}:{port}/postgres");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect postgres");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&pool)
        .await
        .expect("seed pg");
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
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&pool)
        .await
        .expect("seed mysql");
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
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&pool)
        .await
        .expect("seed sqlite");
    pool.close().await;

    let abs = db_path.canonicalize().expect("canonicalize");
    let url = format!("sqlite://{}", abs.display());
    (url, dir)
}

/// Introspected tables are qualified with the schema (or database) they
/// live in.
fn qualified(schema: &str, table: &str) -> String {
    format!("{schema}.{table}")
}

fn names(schema: &str, tables: &[&str]) -> BTreeSet<String> {
    tables
        .iter()
        .map(|table| qualified(schema, table))
        .collect()
}

fn edge_pairs(schema: &str, pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    pairs
        .iter()
        .map(|(from, to)| (qualified(schema, from), qualified(schema, to)))
        .collect()
}

/// Runs the CLI against `url` and parses its JSON stdout.
fn run_json(url: &str, args: &[&str]) -> serde_json::Value {
    let output = relune()
        .args(args)
        .args(["--db-url", url, "--format", "json"])
        .assert()
        .success();
    serde_json::from_slice(&output.get_output().stdout).expect("stdout should be JSON")
}

/// Table names and foreign key count reported by `inspect`.
fn inspect_summary(url: &str) -> (BTreeSet<String>, u64) {
    let parsed = run_json(url, &["inspect"]);
    let summary = &parsed["summary"];
    let tables = summary["tables"]
        .as_array()
        .expect("summary tables")
        .iter()
        .map(|table| table["name"].as_str().expect("table name").to_string())
        .collect();
    let foreign_keys = summary["foreign_key_count"]
        .as_u64()
        .expect("foreign key count");
    (tables, foreign_keys)
}

/// `(rule_id, table_name)` of every issue reported by `lint`.
fn lint_issues(url: &str) -> BTreeSet<(String, String)> {
    run_json(url, &["lint"])["issues"]
        .as_array()
        .expect("lint issues")
        .iter()
        .map(|issue| {
            (
                issue["rule_id"].as_str().expect("rule id").to_string(),
                issue["table_name"]
                    .as_str()
                    .expect("table name")
                    .to_string(),
            )
        })
        .collect()
}

/// Renders `url` with `extra` arguments and returns the table IDs and
/// `(from, to)` foreign key edges drawn in the output.
fn rendered_tables_and_edges(
    url: &str,
    file_name: &str,
    extra: &[&str],
) -> (BTreeSet<String>, BTreeSet<(String, String)>) {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join(file_name);
    relune()
        .args(["render", "--db-url", url])
        .args(extra)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    let content = fs::read_to_string(&out).unwrap();
    let edges = relune_testkit::svg::edges(&content)
        .into_iter()
        .filter(|edge| edge.classes.contains("edge-kind-foreign-key"))
        .map(|edge| (edge.from, edge.to))
        .collect();
    (relune_testkit::svg::table_ids(&content), edges)
}

// ===========================================================================
// PostgreSQL
// ===========================================================================

mod postgres {
    use super::*;

    const SCHEMA: &str = "public";

    fn blog_tables() -> BTreeSet<String> {
        names(SCHEMA, &["comments", "posts", "users"])
    }

    fn blog_edges() -> BTreeSet<(String, String)> {
        edge_pairs(
            SCHEMA,
            &[
                ("comments", "posts"),
                ("comments", "users"),
                ("posts", "users"),
            ],
        )
    }

    #[tokio::test]
    async fn inspect_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        assert_eq!(inspect_summary(&url), (blog_tables(), 3));
    }

    #[tokio::test]
    async fn render_svg_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        assert_eq!(
            rendered_tables_and_edges(&url, "out.svg", &[]),
            (blog_tables(), blog_edges())
        );
    }

    #[tokio::test]
    async fn render_html_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        assert_eq!(
            rendered_tables_and_edges(&url, "out.html", &["--format", "html"]),
            (blog_tables(), blog_edges())
        );
    }

    #[tokio::test]
    async fn lint_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        let issues = lint_issues(&url);
        for table in ["comments", "posts", "users"] {
            assert!(
                issues.contains(&(
                    "missing-table-comment".to_string(),
                    qualified(SCHEMA, table)
                )),
                "{table} has no comment in the database; got: {issues:?}"
            );
        }
    }

    #[tokio::test]
    async fn inspect_table_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("simple_blog.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        let parsed = run_json(&url, &["inspect", "--table", "posts"]);
        let table = &parsed["table"];
        assert_eq!(table["name"], qualified(SCHEMA, "posts"));
        let columns = table["columns"]
            .as_array()
            .expect("table columns")
            .iter()
            .map(|column| column["name"].as_str().expect("column name"))
            .collect::<Vec<_>>();
        assert_eq!(
            columns,
            [
                "id",
                "user_id",
                "title",
                "content",
                "published_at",
                "created_at",
                "updated_at"
            ]
        );
        let foreign_keys = table["foreign_keys"].as_array().expect("foreign keys");
        assert_eq!(foreign_keys.len(), 1);
        assert_eq!(
            foreign_keys[0]["from_columns"],
            serde_json::json!(["user_id"])
        );
        assert_eq!(foreign_keys[0]["to_table"], "users");
        assert_eq!(foreign_keys[0]["to_columns"], serde_json::json!(["id"]));
    }

    #[tokio::test]
    async fn render_focus_postgres() {
        let sql = fs::read_to_string(fixtures_dir().join("ecommerce.sql")).unwrap();
        let (url, _c) = pg_container(&sql).await;

        let (tables, edges) =
            rendered_tables_and_edges(&url, "focus.svg", &["--focus", "orders", "--depth", "1"]);
        assert_eq!(
            tables,
            names(SCHEMA, &["customers", "order_items", "orders"])
        );
        assert_eq!(
            edges,
            edge_pairs(
                SCHEMA,
                &[("order_items", "orders"), ("orders", "customers")]
            )
        );
    }
}

// ===========================================================================
// MySQL
// ===========================================================================

mod mysql {
    use super::*;

    const SCHEMA: &str = "relune_e2e";

    fn ecommerce_tables() -> BTreeSet<String> {
        names(
            SCHEMA,
            &["categories", "order_items", "orders", "products", "users"],
        )
    }

    #[tokio::test]
    async fn inspect_mysql() {
        let sql = fs::read_to_string(fixtures_dir().join("mysql_ecommerce.sql")).unwrap();
        let (url, _c) = mysql_container(&sql).await;

        assert_eq!(inspect_summary(&url), (ecommerce_tables(), 5));
    }

    #[tokio::test]
    async fn render_svg_mysql() {
        let sql = fs::read_to_string(fixtures_dir().join("mysql_ecommerce.sql")).unwrap();
        let (url, _c) = mysql_container(&sql).await;

        assert_eq!(
            rendered_tables_and_edges(&url, "out.svg", &[]),
            (
                ecommerce_tables(),
                edge_pairs(
                    SCHEMA,
                    &[
                        ("categories", "categories"),
                        ("order_items", "orders"),
                        ("order_items", "products"),
                        ("orders", "users"),
                        ("products", "categories"),
                    ]
                )
            )
        );
    }

    #[tokio::test]
    async fn lint_mysql() {
        let sql = fs::read_to_string(fixtures_dir().join("mysql_ecommerce.sql")).unwrap();
        let (url, _c) = mysql_container(&sql).await;

        let issues = lint_issues(&url);
        assert!(
            issues.contains(&(
                "nullable-foreign-key-lazy-load".to_string(),
                qualified(SCHEMA, "categories")
            )),
            "the nullable parent_id FK should be reported; got: {issues:?}"
        );
    }
}

// ===========================================================================
// SQLite
// ===========================================================================

mod sqlite {
    use super::*;

    const SCHEMA: &str = "main";

    fn blog_tables() -> BTreeSet<String> {
        names(SCHEMA, &["comments", "post_tags", "posts", "tags", "users"])
    }

    #[tokio::test]
    async fn inspect_sqlite() {
        let sql = fs::read_to_string(fixtures_dir().join("sqlite_blog.sql")).unwrap();
        let (url, _dir) = sqlite_db(&sql).await;

        assert_eq!(inspect_summary(&url), (blog_tables(), 5));
    }

    #[tokio::test]
    async fn render_svg_sqlite() {
        let sql = fs::read_to_string(fixtures_dir().join("sqlite_blog.sql")).unwrap();
        let (url, _dir) = sqlite_db(&sql).await;

        assert_eq!(
            rendered_tables_and_edges(&url, "out.svg", &[]),
            (
                blog_tables(),
                edge_pairs(
                    SCHEMA,
                    &[
                        ("comments", "posts"),
                        ("comments", "users"),
                        ("post_tags", "posts"),
                        ("post_tags", "tags"),
                        ("posts", "users"),
                    ]
                )
            )
        );
    }

    #[tokio::test]
    async fn lint_sqlite() {
        let sql = fs::read_to_string(fixtures_dir().join("sqlite_blog.sql")).unwrap();
        let (url, _dir) = sqlite_db(&sql).await;

        let issues = lint_issues(&url);
        assert!(
            issues.contains(&(
                "missing-foreign-key-index".to_string(),
                qualified(SCHEMA, "post_tags")
            )),
            "post_tags.tag_id has no index; got: {issues:?}"
        );
    }
}
