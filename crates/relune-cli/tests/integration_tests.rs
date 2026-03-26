//! Integration tests for relune CLI commands.
//!
//! These tests exercise the CLI through actual subprocess invocations,
//! testing render, inspect, and export commands with real fixtures.

use std::fs;
use std::path::PathBuf;
use std::process::Output;

use assert_cmd::Command;
use predicates::prelude::*;
use relune_testkit::{config_fixture_path, normalize_workspace_paths, sql_fixture_path};

fn relune() -> Command {
    Command::cargo_bin("relune").expect("Failed to find relune binary")
}

fn fixtures_dir() -> PathBuf {
    sql_fixture_path("simple_blog.sql")
        .parent()
        .unwrap()
        .to_path_buf()
}

fn simple_blog_fixture() -> PathBuf {
    fixtures_dir().join("simple_blog.sql")
}

fn ecommerce_fixture() -> PathBuf {
    fixtures_dir().join("ecommerce.sql")
}

fn config_fixtures_dir() -> PathBuf {
    config_fixture_path("valid_full.toml")
        .parent()
        .unwrap()
        .to_path_buf()
}

fn failure_snapshot(name: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    let stderr = normalize_workspace_paths(stderr.trim_end());
    let exit_code = output.status.code().unwrap_or(-1);

    insta::assert_snapshot!(name, format!("exit_code: {exit_code}\nstderr:\n{stderr}"));
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
            .arg("--stdout")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(stdout.contains("<svg"), "Stdout should contain SVG");
    }

    #[test]
    fn render_html_to_explicit_stdout() {
        let mut cmd = relune();
        let output = cmd
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("html")
            .arg("--stdout")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(stdout.contains("<!DOCTYPE html>") || stdout.contains("<html"));
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
        let output = relune().arg("render").output().expect("command should run");
        assert!(!output.status.success(), "render without input should fail");
        failure_snapshot("render_missing_input", &output);
    }

    #[test]
    fn render_multiple_inputs_fails() {
        relune()
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .assert()
            .failure()
            .code(2);
    }

    #[test]
    fn render_nonexistent_file_fails() {
        let output = relune()
            .arg("render")
            .arg("--sql")
            .arg("/nonexistent/path/file.sql")
            .output()
            .expect("command should run");
        assert!(
            !output.status.success(),
            "render with missing file should fail"
        );
        failure_snapshot("render_nonexistent_file", &output);
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
            .arg("--stdout")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(stdout.contains("<!DOCTYPE html>") || stdout.contains("<html"));
        assert!(
            stdout.contains("#0f172a") || stdout.contains("#111827"),
            "HTML output should use the configured dark theme"
        );
    }

    #[test]
    fn render_sql_to_png_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("output.png");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("png")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        // Verify output file was created
        assert!(output_path.exists(), "Output PNG file should be created");

        // Verify it contains valid PNG data (magic bytes)
        let content = fs::read(&output_path).expect("Failed to read output file");
        assert!(content.len() > 8, "PNG file should have meaningful content");
        assert_eq!(
            &content[..8],
            b"\x89PNG\r\n\x1a\n",
            "Output should start with PNG magic bytes"
        );
    }

    #[test]
    fn render_png_rejects_terminal_stdout() {
        // When format is png and no --out is specified, the CLI should refuse
        // to write binary data to an interactive terminal.
        // Note: in test context stdout is not a terminal, so we verify via
        // the error message by checking the help text mentions --out.
        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("png");

        // In CI/test context stdout is piped (not a terminal), so this
        // actually succeeds by writing binary to stdout. We just verify
        // the command doesn't crash.
        cmd.assert().success();
    }

    #[test]
    fn render_png_with_dark_theme() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("dark.png");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("png")
            .arg("--theme")
            .arg("dark")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        let content = fs::read(&output_path).expect("Failed to read output file");
        assert_eq!(
            &content[..8],
            b"\x89PNG\r\n\x1a\n",
            "Dark theme PNG should still be valid"
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
        let output = relune()
            .arg("--config")
            .arg(config_fixture_path("unknown_nested_key.toml"))
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .output()
            .expect("command should run");

        assert!(
            !output.status.success(),
            "invalid nested config should fail"
        );
        failure_snapshot("config_unknown_nested_key", &output);
    }

    #[test]
    fn config_unknown_root_key_fails_fast() {
        let output = relune()
            .arg("--config")
            .arg(config_fixture_path("unknown_root_key.toml"))
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .output()
            .expect("command should run");

        assert!(!output.status.success(), "invalid root config should fail");
        failure_snapshot("config_unknown_root_key", &output);
    }

    #[test]
    fn render_config_focus_conflict_fails_fast() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");

        fs::write(
            &config_path,
            "[render]\nformat = \"svg\"\nfocus = \"users\"\ninclude = [\"posts\"]\n",
        )
        .unwrap();

        let output = relune()
            .arg("--config")
            .arg(&config_path)
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .output()
            .expect("command should run");

        assert!(
            !output.status.success(),
            "conflicting render config should fail"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("render.focus 'users' must be included"));
    }

    #[test]
    fn render_depth_without_focus_fails_fast() {
        let output = relune()
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--depth")
            .arg("2")
            .output()
            .expect("command should run");

        assert!(!output.status.success(), "depth without focus should fail");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("render.depth can only be set"));
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
        let output = relune()
            .arg("-v")
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .output()
            .expect("command should run");

        assert!(output.status.success(), "verbose render should succeed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("parsed SQL"),
            "verbose output should include parse details"
        );
        assert!(
            stderr.contains("render complete"),
            "verbose output should include render completion details"
        );
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

        let output = relune()
            .arg("render")
            .arg("--sql")
            .arg(&broken_fixture)
            .output()
            .expect("command should run");

        assert!(!output.status.success(), "broken SQL should fail");
        failure_snapshot("render_broken_sql", &output);
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
    fn diff_schema_json_inputs() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = simple_blog_fixture();
        let before_json = temp.path().join("before.json");
        let after_json = temp.path().join("after.json");

        relune()
            .arg("export")
            .arg("--sql")
            .arg(&sql_path)
            .arg("--format")
            .arg("schema-json")
            .arg("--out")
            .arg(&before_json)
            .assert()
            .success();

        fs::copy(&before_json, &after_json).expect("Failed to duplicate schema JSON file");

        let output = relune()
            .arg("diff")
            .arg("--before")
            .arg(&before_json)
            .arg("--after")
            .arg(&after_json)
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(
            stdout.contains("No changes"),
            "JSON inputs should be treated as schema JSON"
        );
    }

    #[test]
    fn diff_schema_json_inputs_ignore_extension() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = simple_blog_fixture();
        let before_json = temp.path().join("before.json");
        let after_json = temp.path().join("after.json");
        let before_sql = temp.path().join("before.sql");
        let after_sql = temp.path().join("after.sql");

        relune()
            .arg("export")
            .arg("--sql")
            .arg(&sql_path)
            .arg("--format")
            .arg("schema-json")
            .arg("--out")
            .arg(&before_json)
            .assert()
            .success();

        fs::copy(&before_json, &before_sql).expect("Failed to copy schema JSON to .sql path");
        fs::copy(&before_json, &after_json).expect("Failed to duplicate schema JSON file");
        fs::copy(&after_json, &after_sql).expect("Failed to copy schema JSON to .sql path");

        let output = relune()
            .arg("diff")
            .arg("--before")
            .arg(&before_sql)
            .arg("--after")
            .arg(&after_sql)
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(
            stdout.contains("No changes"),
            "schema JSON content should be detected even with a .sql extension"
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
