//! Integration tests for relune CLI commands.
//!
//! These tests exercise the CLI through actual subprocess invocations,
//! testing render, inspect, and export commands with real fixtures.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

fn relune() -> Command {
    Command::cargo_bin("relune").expect("Failed to find relune binary")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("sql")
}

fn simple_blog_fixture() -> PathBuf {
    fixtures_dir().join("simple_blog.sql")
}

fn ecommerce_fixture() -> PathBuf {
    fixtures_dir().join("ecommerce.sql")
}

fn config_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("config")
}

// ============================================================================
// Render Command Tests
// ============================================================================

mod render_tests {
    use super::*;

    #[test]
    fn render_sql_to_svg_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("output.svg");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        // Verify output file was created
        assert!(output_path.exists(), "Output SVG file should be created");

        // Verify it contains valid SVG content
        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert!(
            content.contains("<svg"),
            "Output should contain SVG element"
        );
        assert!(
            content.contains("viewBox="),
            "Output should have viewBox attribute"
        );
    }

    #[test]
    fn render_sql_to_html_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("output.html");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("html")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        // Verify output file was created
        assert!(output_path.exists(), "Output HTML file should be created");

        // Verify it contains valid HTML content
        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert!(
            content.contains("<!DOCTYPE html>") || content.contains("<html"),
            "Output should contain HTML element"
        );
        assert!(
            content.contains("<svg"),
            "Output should contain embedded SVG"
        );
    }

    #[test]
    fn render_sql_to_graph_json_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("graph.json");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("graph-json")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        // Verify output file was created
        assert!(output_path.exists(), "Output JSON file should be created");

        // Verify it contains valid JSON
        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Output should be valid JSON");
        assert!(parsed.is_object(), "Graph JSON should be a JSON object");
    }

    #[test]
    fn render_sql_to_schema_json_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("schema.json");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("schema-json")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        // Verify output file was created
        assert!(output_path.exists(), "Output JSON file should be created");

        // Verify it contains valid JSON with tables
        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Output should be valid JSON");
        assert!(
            parsed.get("tables").is_some(),
            "Schema JSON should have tables field"
        );
    }

    #[test]
    fn render_sql_to_stdout() {
        let mut cmd = relune();
        let output = cmd
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(stdout.contains("<svg"), "Stdout should contain SVG");
    }

    #[test]
    fn render_with_stats() {
        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--stats")
            .assert()
            .stderr(predicate::str::contains("tables").or(predicate::str::contains("Tables")));
    }

    #[test]
    fn render_with_theme_dark() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("output.svg");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--theme")
            .arg("dark")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        // Dark theme should have dark background colors
        assert!(
            content.contains("<svg"),
            "Output should contain SVG element"
        );
    }

    #[test]
    fn render_with_focus() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("output.svg");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--focus")
            .arg("users")
            .arg("--depth")
            .arg("2")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        assert!(output_path.exists(), "Output file should be created");
    }

    #[test]
    fn render_with_force_directed_layout() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("force.svg");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--layout")
            .arg("force-directed")
            .arg("--edge-style")
            .arg("orthogonal")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert!(
            content.contains("<svg"),
            "Output should contain SVG element"
        );
    }

    #[test]
    fn render_missing_input_fails() {
        let mut cmd = relune();
        // The command should fail when no input is provided
        // Exit code 2 indicates invalid arguments
        cmd.arg("render").assert().failure().code(2);
    }

    #[test]
    fn render_nonexistent_file_fails() {
        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg("/nonexistent/path/file.sql")
            .assert()
            .failure();
    }

    #[test]
    fn render_uses_config_format_and_theme() {
        let mut cmd = relune();
        let output = cmd
            .arg("--config")
            .arg(config_fixtures_dir().join("valid_full.toml"))
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(stdout.contains("<!DOCTYPE html>") || stdout.contains("<html"));
        assert!(
            stdout.contains("#0f172a") || stdout.contains("#111827"),
            "HTML output should use the configured dark theme"
        );
    }
}

// ============================================================================
// Export Command Tests
// ============================================================================

mod export_tests {
    use super::*;

    #[test]
    fn export_schema_json_to_stdout() {
        let mut cmd = relune();
        let output = cmd
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("schema-json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        assert!(
            parsed.get("tables").is_some(),
            "Schema JSON should have tables field"
        );
    }

    #[test]
    fn export_graph_json_to_stdout() {
        let mut cmd = relune();
        let output = cmd
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("graph-json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        assert!(parsed.is_object(), "Graph JSON should be a JSON object");
    }

    #[test]
    fn export_layout_json_to_stdout() {
        let mut cmd = relune();
        let output = cmd
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("layout-json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        assert!(parsed.is_object(), "Layout JSON should be a JSON object");
    }

    #[test]
    fn export_layout_json_with_orthogonal_edges() {
        let mut cmd = relune();
        let output = cmd
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("layout-json")
            .arg("--layout")
            .arg("force-directed")
            .arg("--edge-style")
            .arg("orthogonal")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        assert_eq!(parsed["edges"][0]["route"]["style"], "orthogonal");
    }

    #[test]
    fn export_schema_json_to_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("schema.json");

        let mut cmd = relune();
        cmd.arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("schema-json")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        assert!(output_path.exists(), "Output file should be created");

        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Output should be valid JSON");
        assert!(
            parsed.get("tables").is_some(),
            "Schema JSON should have tables field"
        );
    }

    #[test]
    fn export_with_focus() {
        let mut cmd = relune();
        let output = cmd
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("schema-json")
            .arg("--focus")
            .arg("users")
            .arg("--depth")
            .arg("1")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        // With focus on users, the schema should contain limited tables
        assert!(
            parsed.get("tables").is_some(),
            "Schema JSON should have tables field"
        );
    }

    #[test]
    fn export_missing_format_fails() {
        let mut cmd = relune();
        cmd.arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .failure()
            .code(2);
    }

    #[test]
    fn export_uses_config_format() {
        let mut cmd = relune();
        let output = cmd
            .arg("--config")
            .arg(config_fixtures_dir().join("valid_full.toml"))
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");
        assert!(parsed.is_object(), "Graph JSON should be a JSON object");
    }
}

// ============================================================================
// Inspect Command Tests
// ============================================================================

mod inspect_tests {
    use super::*;

    #[test]
    fn inspect_summary_text() {
        let mut cmd = relune();
        let output = cmd
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("text")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);

        // Should contain table count and basic info
        assert!(
            stdout.contains("Table") || stdout.contains("table"),
            "Output should mention tables"
        );
    }

    #[test]
    fn inspect_summary_json() {
        let mut cmd = relune();
        let output = cmd
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        assert!(parsed.is_object(), "Inspect JSON should be an object");
    }

    #[test]
    fn inspect_specific_table() {
        let mut cmd = relune();
        let output = cmd
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--table")
            .arg("users")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);

        // Should contain the table name
        assert!(
            stdout.contains("users"),
            "Output should mention the users table"
        );
    }

    #[test]
    fn inspect_specific_table_json() {
        let mut cmd = relune();
        let output = cmd
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--table")
            .arg("users")
            .arg("--format")
            .arg("json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        assert!(parsed.is_object(), "Inspect JSON should be an object");
    }

    #[test]
    fn inspect_uses_config_format() {
        let mut cmd = relune();
        let output = cmd
            .arg("--config")
            .arg(config_fixtures_dir().join("valid_full.toml"))
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");
        assert!(parsed.is_object(), "Inspect JSON should be an object");
    }

    #[test]
    fn inspect_nonexistent_table() {
        let mut cmd = relune();
        cmd.arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--table")
            .arg("nonexistent_table")
            .assert()
            .failure();
    }
}

// ============================================================================
// Lint Command Tests
// ============================================================================

mod lint_tests {
    use super::*;

    #[test]
    fn lint_uses_config_format() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");

        fs::write(&config_path, "[lint]\nformat = \"json\"\n").unwrap();

        let mut cmd = relune();
        let output = cmd
            .arg("--config")
            .arg(&config_path)
            .arg("lint")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");
        assert!(parsed.is_object(), "Lint JSON should be an object");
    }
}

// ============================================================================
// Config Validation Tests
// ============================================================================

mod config_validation_tests {
    use super::*;

    #[test]
    fn config_typo_fails_fast() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");

        fs::write(&config_path, "[render]\ntehme = \"dark\"\n").unwrap();

        let mut cmd = relune();
        cmd.arg("--config")
            .arg(&config_path)
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains("unknown field `tehme`"));
    }

    #[test]
    fn config_unknown_root_key_fails_fast() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");

        fs::write(&config_path, "unknown_field = true\n").unwrap();

        let mut cmd = relune();
        cmd.arg("--config")
            .arg(&config_path)
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains("unknown field `unknown_field`"));
    }
}

// ============================================================================
// Doctor Command Tests
// ============================================================================

mod doctor_tests {
    use super::*;

    #[test]
    fn doctor_succeeds() {
        let mut cmd = relune();
        let output = cmd.arg("doctor").assert().success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(
            stdout.contains("ok") || stdout.contains("wired"),
            "Doctor should report status"
        );
    }
}

// ============================================================================
// Global Flag Tests
// ============================================================================

mod global_flag_tests {
    use super::*;

    #[test]
    fn quiet_flag_suppresses_output() {
        let mut cmd = relune();
        cmd.arg("--quiet")
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();
        // With quiet flag, stderr should be minimal (only errors)
    }

    #[test]
    fn verbose_flag_increases_output() {
        let mut cmd = relune();
        cmd.arg("-v")
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();
        // Verbose should produce more log output
    }

    #[test]
    fn color_never_flag() {
        let mut cmd = relune();
        cmd.arg("--color")
            .arg("never")
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .assert()
            .success();
        // Color never should not produce ANSI codes
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

mod error_tests {
    use super::*;

    #[test]
    fn broken_sql_fails_gracefully() {
        let broken_fixture = fixtures_dir().join("broken_input.sql");

        let mut cmd = relune();
        // broken_input.sql contains severe syntax errors that should cause failure
        cmd.arg("render")
            .arg("--sql")
            .arg(&broken_fixture)
            .assert()
            .failure();
    }

    #[test]
    fn fail_on_warning_with_valid_input() {
        let mut cmd = relune();
        // With --fail-on-warning on valid input, should succeed
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--fail-on-warning")
            .assert()
            .success();
    }
}

// ============================================================================
// Multi-format Output Tests
// ============================================================================

mod multi_format_tests {
    use super::*;

    #[test]
    fn render_ecommerce_schema() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("ecommerce.svg");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(ecommerce_fixture())
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        assert!(output_path.exists(), "Output file should be created");
        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert!(
            content.contains("<svg"),
            "Output should contain SVG element"
        );
    }

    #[test]
    fn export_ecommerce_graph_json() {
        let mut cmd = relune();
        let output = cmd
            .arg("export")
            .arg("--sql")
            .arg(ecommerce_fixture())
            .arg("--format")
            .arg("graph-json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");
        assert!(parsed.is_object(), "Graph JSON should be a JSON object");
    }
}

// ============================================================================
// Diff Command Tests
// ============================================================================

mod diff_tests {
    use super::*;

    #[test]
    fn diff_no_changes() {
        let mut cmd = relune();
        let output = cmd
            .arg("diff")
            .arg("--before")
            .arg(simple_blog_fixture())
            .arg("--after")
            .arg(simple_blog_fixture())
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(
            stdout.contains("No changes"),
            "Output should report no changes"
        );
    }

    #[test]
    fn diff_with_added_table() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.sql");
        let after_path = temp.path().join("after.sql");

        fs::write(&before_path, "CREATE TABLE users (id INT PRIMARY KEY);").unwrap();
        fs::write(&after_path, "CREATE TABLE users (id INT PRIMARY KEY);\nCREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));").unwrap();

        let mut cmd = relune();
        let output = cmd
            .arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(&after_path)
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(
            stdout.contains("posts"),
            "Output should mention the added table"
        );
    }

    #[test]
    fn diff_with_removed_table() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.sql");
        let after_path = temp.path().join("after.sql");

        fs::write(
            &before_path,
            "CREATE TABLE users (id INT PRIMARY KEY);\nCREATE TABLE posts (id INT PRIMARY KEY);",
        )
        .unwrap();
        fs::write(&after_path, "CREATE TABLE users (id INT PRIMARY KEY);").unwrap();

        let mut cmd = relune();
        let output = cmd
            .arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(&after_path)
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(
            stdout.contains("posts"),
            "Output should mention the removed table"
        );
    }

    #[test]
    fn diff_to_json_format() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.sql");
        let after_path = temp.path().join("after.sql");

        fs::write(&before_path, "CREATE TABLE users (id INT PRIMARY KEY);").unwrap();
        fs::write(
            &after_path,
            "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));",
        )
        .unwrap();

        let mut cmd = relune();
        let output = cmd
            .arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(&after_path)
            .arg("--format")
            .arg("json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");
        assert!(parsed.is_object(), "Diff JSON should be an object");
    }

    #[test]
    fn diff_uses_config_defaults() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.sql");
        let after_path = temp.path().join("after.sql");
        let config_path = temp.path().join("relune.toml");

        fs::write(&before_path, "CREATE TABLE users (id INT PRIMARY KEY);").unwrap();
        fs::write(
            &after_path,
            "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));",
        )
        .unwrap();
        fs::write(
            &config_path,
            "[diff]\nformat = \"json\"\ndialect = \"postgres\"\n",
        )
        .unwrap();

        let mut cmd = relune();
        let output = cmd
            .arg("--config")
            .arg(&config_path)
            .arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(&after_path)
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");
        assert!(parsed.is_object(), "Diff JSON should be an object");
    }

    #[test]
    fn diff_to_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.sql");
        let after_path = temp.path().join("after.sql");
        let output_path = temp.path().join("diff.txt");

        fs::write(&before_path, "CREATE TABLE users (id INT PRIMARY KEY);").unwrap();
        fs::write(&after_path, "CREATE TABLE posts (id INT PRIMARY KEY);").unwrap();

        let mut cmd = relune();
        cmd.arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(&after_path)
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        assert!(output_path.exists(), "Output file should be created");
        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert!(
            content.contains("users") || content.contains("posts"),
            "Output should mention tables"
        );
    }

    #[test]
    fn diff_missing_before_fails() {
        let mut cmd = relune();
        cmd.arg("diff")
            .arg("--after")
            .arg(simple_blog_fixture())
            .assert()
            .failure();
    }

    #[test]
    fn diff_missing_after_fails() {
        let mut cmd = relune();
        cmd.arg("diff")
            .arg("--before")
            .arg(simple_blog_fixture())
            .assert()
            .failure();
    }
}
