//! Integration tests for `PostgreSQL` introspection using testcontainers.
//!
//! These tests verify that the `introspect_postgres` function correctly
//! extracts schema metadata from a live `PostgreSQL` database and produces
//! results consistent with parsing the same SQL DDL using `relune_parser_sql`.

use relune_introspect::introspect_database;
use relune_introspect::introspect_sqlite;
use relune_introspect::postgres::introspect_postgres;
use relune_parser_sql::parse_sql_to_schema;
use std::collections::HashSet;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;
use testcontainers_modules::postgres::Postgres;

const POSTGRES_TAG: &str = "18";

/// Sets up a `PostgreSQL` container and executes the provided SQL against it.
///
/// Returns a tuple containing:
/// - The database connection URL
/// - The container instance (must be kept alive for the connection to work)
async fn setup_postgres_with_sql(
    sql: &str,
) -> Result<(String, testcontainers::ContainerAsync<Postgres>), Box<dyn std::error::Error>> {
    // Start PostgreSQL container
    let container = Postgres::default().with_tag(POSTGRES_TAG).start().await?;

    // Get connection parameters from the container
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;

    // Build connection URL
    let database_url = format!("postgresql://postgres:postgres@{host}:{port}/postgres");

    // Create connection pool
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;

    // Execute the SQL against the database
    sqlx::raw_sql(sql).execute(&pool).await?;

    // Close the pool
    pool.close().await;

    Ok((database_url, container))
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // integration assertions kept in one test for readability
async fn test_introspect_simple_blog() {
    // Read SQL fixture file
    let sql_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/sql/simple_blog.sql"
    );
    let sql =
        std::fs::read_to_string(sql_path).expect("Failed to read simple_blog.sql fixture file");

    // Setup PostgreSQL with SQL
    let (database_url, _container) = setup_postgres_with_sql(&sql)
        .await
        .expect("Failed to setup PostgreSQL container");

    // Introspect the live database
    let introspected_schema = introspect_postgres(&database_url)
        .await
        .expect("Failed to introspect PostgreSQL database");

    // Parse the SQL file
    let parsed_schema = parse_sql_to_schema(&sql).expect("Failed to parse SQL file");

    // --- Compare table names ---
    let introspected_table_names: HashSet<String> = introspected_schema
        .tables
        .iter()
        .map(|t| t.name.to_lowercase())
        .collect();

    let parsed_table_names: HashSet<String> = parsed_schema
        .tables
        .iter()
        .map(|t| t.name.to_lowercase())
        .collect();

    assert_eq!(
        introspected_table_names, parsed_table_names,
        "Table names mismatch.\nIntrospected: {introspected_table_names:?}\nParsed: {parsed_table_names:?}",
    );

    // --- Compare column names and types per table ---
    for parsed_table in &parsed_schema.tables {
        let table_name_lower = parsed_table.name.to_lowercase();

        // Find the corresponding introspected table
        let introspected_table = introspected_schema
            .tables
            .iter()
            .find(|t| t.name.to_lowercase() == table_name_lower)
            .unwrap_or_else(|| {
                panic!("Table '{table_name_lower}' not found in introspected schema")
            });

        // Compare column names
        let introspected_column_names: HashSet<String> = introspected_table
            .columns
            .iter()
            .map(|c| c.name.to_lowercase())
            .collect();

        let parsed_column_names: HashSet<String> = parsed_table
            .columns
            .iter()
            .map(|c| c.name.to_lowercase())
            .collect();

        assert_eq!(
            introspected_column_names, parsed_column_names,
            "Column names mismatch for table '{table_name_lower}'.\nIntrospected: {introspected_column_names:?}\nParsed: {parsed_column_names:?}",
        );

        // Compare column nullability
        for parsed_col in &parsed_table.columns {
            let col_name_lower = parsed_col.name.to_lowercase();
            let introspected_col = introspected_table
                .columns
                .iter()
                .find(|c| c.name.to_lowercase() == col_name_lower)
                .unwrap_or_else(|| {
                    panic!("Column '{col_name_lower}' not found in table '{table_name_lower}'")
                });

            assert_eq!(
                introspected_col.nullable, parsed_col.nullable,
                "Nullability mismatch for column '{col_name_lower}' in table '{table_name_lower}'",
            );
        }
    }

    // --- Compare foreign keys ---
    for parsed_table in &parsed_schema.tables {
        let table_name_lower = parsed_table.name.to_lowercase();

        let introspected_table = introspected_schema
            .tables
            .iter()
            .find(|t| t.name.to_lowercase() == table_name_lower)
            .unwrap_or_else(|| {
                panic!("Table '{table_name_lower}' not found in introspected schema")
            });

        // Build sets of foreign key relationships for comparison
        let introspected_fks: HashSet<(Vec<String>, String, Vec<String>)> = introspected_table
            .foreign_keys
            .iter()
            .map(|fk| {
                let from_cols: Vec<String> =
                    fk.from_columns.iter().map(|c| c.to_lowercase()).collect();
                let to_table = fk.to_table.to_lowercase();
                let to_cols: Vec<String> = fk.to_columns.iter().map(|c| c.to_lowercase()).collect();
                (from_cols, to_table, to_cols)
            })
            .collect();

        let parsed_fks: HashSet<(Vec<String>, String, Vec<String>)> = parsed_table
            .foreign_keys
            .iter()
            .map(|fk| {
                let from_cols: Vec<String> =
                    fk.from_columns.iter().map(|c| c.to_lowercase()).collect();
                let to_table = fk.to_table.to_lowercase();
                let to_cols: Vec<String> = fk.to_columns.iter().map(|c| c.to_lowercase()).collect();
                (from_cols, to_table, to_cols)
            })
            .collect();

        assert_eq!(
            introspected_fks, parsed_fks,
            "Foreign keys mismatch for table '{table_name_lower}'.\nIntrospected: {introspected_fks:?}\nParsed: {parsed_fks:?}",
        );
    }
}

#[tokio::test]
async fn test_introspect_ecommerce() {
    // Read SQL fixture file
    let sql_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/sql/ecommerce.sql"
    );
    let sql = std::fs::read_to_string(sql_path).expect("Failed to read ecommerce.sql fixture file");

    // Setup PostgreSQL with SQL
    let (database_url, _container) = setup_postgres_with_sql(&sql)
        .await
        .expect("Failed to setup PostgreSQL container");

    // Introspect the live database
    let introspected_schema = introspect_postgres(&database_url)
        .await
        .expect("Failed to introspect PostgreSQL database");

    // Parse the SQL file
    let parsed_schema = parse_sql_to_schema(&sql).expect("Failed to parse SQL file");

    // Compare table names
    let introspected_table_names: HashSet<String> = introspected_schema
        .tables
        .iter()
        .map(|t| t.name.to_lowercase())
        .collect();

    let parsed_table_names: HashSet<String> = parsed_schema
        .tables
        .iter()
        .map(|t| t.name.to_lowercase())
        .collect();

    assert_eq!(
        introspected_table_names, parsed_table_names,
        "Table names mismatch.\nIntrospected: {introspected_table_names:?}\nParsed: {parsed_table_names:?}",
    );
}

#[tokio::test]
async fn test_introspect_sqlite_file_minimal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("app.db");
    let pool = sqlx::sqlite::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("connect sqlite");
    sqlx::raw_sql(
        r"
        CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL
        );
        CREATE TABLE posts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            FOREIGN KEY (user_id) REFERENCES users(id)
        );
        CREATE INDEX idx_posts_user ON posts(user_id);
        ",
    )
    .execute(&pool)
    .await
    .expect("ddl");
    pool.close().await;

    let abs = db_path.canonicalize().expect("canonicalize db path");
    let url = format!("sqlite://{}", abs.display());
    let schema = introspect_sqlite(&url).await.expect("introspect sqlite");
    assert_eq!(schema.tables.len(), 2);
    let posts = schema
        .tables
        .iter()
        .find(|t| t.name == "posts")
        .expect("posts table");
    assert_eq!(posts.foreign_keys.len(), 1);
}

#[tokio::test]
async fn test_introspect_database_dispatches_postgres_url() {
    let sql = r"
        CREATE TABLE t1 (id INT PRIMARY KEY);
    ";
    let container = Postgres::default()
        .with_tag(POSTGRES_TAG)
        .start()
        .await
        .expect("postgres");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let database_url = format!("postgresql://postgres:postgres@{host}:{port}/postgres");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("pool");
    sqlx::raw_sql(sql).execute(&pool).await.expect("ddl");
    pool.close().await;

    let schema = introspect_database(&database_url)
        .await
        .expect("introspect_database");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].name, "t1");
}

/// Sets up a `MySQL` container and executes SQL.
async fn setup_mysql_with_sql(
    sql: &str,
) -> Result<(String, testcontainers::ContainerAsync<Mysql>), Box<dyn std::error::Error>> {
    let container = Mysql::default().start().await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(3306).await?;
    // `testcontainers-modules` MySQL image uses root with empty password by default.
    let admin_url = format!("mysql://root@{host}:{port}/mysql");

    let admin = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await?;

    sqlx::raw_sql("CREATE DATABASE IF NOT EXISTS relune_introspect_itest")
        .execute(&admin)
        .await?;
    admin.close().await;

    // Avoid the `mysql` system schema: introspection excludes it from user tables.
    let database_url = format!("mysql://root@{host}:{port}/relune_introspect_itest");

    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;

    sqlx::raw_sql(sql).execute(&pool).await?;
    pool.close().await;

    Ok((database_url, container))
}

#[tokio::test]
async fn test_introspect_mysql_minimal() {
    let sql = r"
        CREATE TABLE users (
            id INT AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        );
        CREATE TABLE posts (
            id INT AUTO_INCREMENT PRIMARY KEY,
            user_id INT NOT NULL,
            CONSTRAINT fk_posts_user FOREIGN KEY (user_id) REFERENCES users(id)
        );
    ";

    let (database_url, _container) = setup_mysql_with_sql(sql).await.expect("mysql setup");

    let schema = introspect_database(&database_url)
        .await
        .expect("introspect mysql");

    assert_eq!(schema.tables.len(), 2);
    let posts = schema
        .tables
        .iter()
        .find(|t| t.name == "posts")
        .expect("posts");
    assert_eq!(posts.foreign_keys.len(), 1);
}

// Note: cyclic_fk.sql contains forward references (e.g., cycle_a references cycle_c before cycle_c is created)
// and requires ALTER TABLE statements to add constraints after table creation.
// This makes it incompatible with simple SQL execution in a single batch.
// A proper cyclic FK test would need to split the SQL and execute in the correct order.
