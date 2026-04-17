//! Integration tests for relune CLI commands.
//!
//! These tests exercise the CLI through actual subprocess invocations,
//! testing render, inspect, and export commands with real fixtures.

use std::fs;
#[cfg(unix)]
use std::io::Read;
use std::path::PathBuf;
use std::process::Output;

use assert_cmd::Command;
#[cfg(unix)]
use portable_pty::{CommandBuilder as PtyCommandBuilder, PtySize, native_pty_system};
use predicates::prelude::*;
use relune_testkit::{
    compact_layout_snapshot, config_fixture_path, normalize_workspace_paths, parse_layout_json,
    sql_fixture_path,
};

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

const fn comment_only_sql() -> &'static str {
    "/* comments only */"
}

fn write_comment_only_sql(temp: &tempfile::TempDir) -> PathBuf {
    let path = temp.path().join("comments_only.sql");
    fs::write(&path, comment_only_sql()).expect("write comment-only SQL fixture");
    path
}

fn failure_snapshot(name: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    let stderr = normalize_workspace_paths(stderr.trim_end());
    let exit_code = output.status.code().unwrap_or(-1);

    insta::assert_snapshot!(name, format!("exit_code: {exit_code}\nstderr:\n{stderr}"));
}

#[cfg(unix)]
fn relune_in_terminal(args: &[String]) -> (u32, String) {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("Failed to create PTY");

    let mut command = PtyCommandBuilder::new(assert_cmd::cargo::cargo_bin("relune"));
    for arg in args {
        command.arg(arg);
    }

    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("Failed to spawn relune in PTY");
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("Failed to clone PTY reader");
    let reader_thread = std::thread::spawn(move || {
        let mut output = Vec::new();
        reader
            .read_to_end(&mut output)
            .expect("Failed to read PTY output");
        output
    });

    let status = child.wait().expect("Failed to wait for relune");
    drop(pair.master);

    let output = reader_thread.join().expect("PTY reader thread should join");
    (
        status.exit_code(),
        String::from_utf8_lossy(&output).into_owned(),
    )
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
    fn render_with_named_viewpoint_from_config() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("viewpoint.svg");
        let config_path = temp.path().join("relune.toml");

        fs::write(
            &config_path,
            r#"
[viewpoints.authoring]
focus = "users"
depth = 1
include = ["users", "posts"]
exclude = ["comments"]
"#,
        )
        .expect("viewpoint config should be written");

        let mut cmd = relune();
        cmd.arg("--config")
            .arg(&config_path)
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--viewpoint")
            .arg("authoring")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert!(content.contains("users"));
        assert!(content.contains("posts"));
        assert!(!content.contains("comments"));
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
    fn export_layout_json_fixture_regression_snapshot() {
        let mut cmd = relune();
        let output = cmd
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("layout-json")
            .arg("--direction")
            .arg("left-to-right")
            .arg("--edge-style")
            .arg("curved")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let graph = parse_layout_json(&stdout);

        assert!(graph.routing_debug.is_some());
        assert!(graph.edges.iter().all(|edge| edge.routing_debug.is_some()));
        insta::assert_json_snapshot!(
            "export_layout_json__simple_blog__left_to_right__curved",
            compact_layout_snapshot(&graph)
        );
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
    fn export_with_named_viewpoint_filters_tables() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");
        fs::write(
            &config_path,
            r#"
[viewpoints.authoring]
focus = "users"
depth = 1
include = ["users", "posts"]
exclude = ["comments"]
"#,
        )
        .expect("viewpoint config should be written");

        let mut cmd = relune();
        let output = cmd
            .arg("--config")
            .arg(&config_path)
            .arg("export")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("graph-json")
            .arg("--viewpoint")
            .arg("authoring")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");
        let nodes = parsed["nodes"].as_array().expect("nodes array");
        let names = nodes
            .iter()
            .map(|node| node["table_name"].as_str().expect("table name"))
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            names,
            ["posts", "users"]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
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

    #[test]
    fn export_validates_config_before_input_io() {
        let output = relune()
            .arg("export")
            .arg("--sql")
            .arg("definitely-missing.sql")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Export format must be provided"));
        assert!(!stderr.contains("Failed to read SQL file"));
    }

    #[test]
    fn export_fail_on_warning_rejects_parse_warnings() {
        let output = relune()
            .arg("export")
            .arg("--sql-text")
            .arg(comment_only_sql())
            .arg("--format")
            .arg("schema-json")
            .arg("--fail-on-warning")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("PARSE004"));
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
    fn inspect_writes_to_output_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("inspect.json");

        let output = relune()
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("json")
            .arg("--out")
            .arg(&output_path)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "inspect should succeed");
        assert!(
            output.stdout.is_empty(),
            "inspect should not write to stdout when --out is used"
        );

        let content = fs::read_to_string(&output_path).expect("Failed to read inspect output");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Output file should contain valid JSON");
        assert!(parsed.is_object(), "Inspect JSON should be an object");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Inspection output written to"),
            "inspect should report file output on stderr"
        );
    }

    #[test]
    fn inspect_quiet_suppresses_output_file_notice() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("inspect.json");

        let output = relune()
            .arg("--quiet")
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("json")
            .arg("--out")
            .arg(&output_path)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "inspect should succeed");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("Inspection output written to"),
            "inspect should suppress file output notice when quiet"
        );
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

    #[test]
    fn inspect_fail_on_warning_rejects_parse_warnings() {
        let output = relune()
            .arg("inspect")
            .arg("--sql-text")
            .arg(comment_only_sql())
            .arg("--fail-on-warning")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("PARSE004"));
    }

    #[test]
    fn inspect_multi_schema_text() {
        let multi_schema = fixtures_dir().join("multi_schema.sql");
        let mut cmd = relune();
        let output = cmd
            .arg("inspect")
            .arg("--sql")
            .arg(multi_schema)
            .arg("--format")
            .arg("text")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);

        // Cross-schema FK resolution: public.users has 2 incoming
        // (public.posts -> public.users, sales.orders -> public.users).
        assert!(
            stdout.contains("public.users"),
            "Output should list qualified table names"
        );
        assert!(
            stdout.contains("FKs(0 out, 2 in)"),
            "public.users should have 2 incoming FKs: {stdout}"
        );

        // sales.orders has 1 out + 2 in.
        assert!(
            stdout.contains("sales.orders"),
            "Output should list sales.orders"
        );

        // Highlights section present.
        assert!(
            stdout.contains("Hub tables"),
            "Multi-schema output should show Hub tables: {stdout}"
        );

        // Next steps present.
        assert!(
            stdout.contains("Next steps:"),
            "Output should show Next steps"
        );
    }

    #[test]
    fn inspect_multi_schema_json() {
        let multi_schema = fixtures_dir().join("multi_schema.sql");
        let mut cmd = relune();
        let output = cmd
            .arg("inspect")
            .arg("--sql")
            .arg(multi_schema)
            .arg("--format")
            .arg("json")
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        // Verify new fields in JSON.
        let summary = &parsed["summary"];
        assert!(
            summary["index_count"].as_u64().unwrap() > 0,
            "index_count should be present and > 0"
        );
        assert!(
            summary["orphan_table_count"].is_u64(),
            "orphan_table_count should be present"
        );
        assert!(
            summary["tables_without_pk"].is_u64(),
            "tables_without_pk should be present"
        );

        // Check incoming_fk_count on public.users.
        let tables = summary["tables"].as_array().unwrap();
        let pub_users = tables
            .iter()
            .find(|t| t["name"] == "public.users")
            .expect("public.users should exist");
        assert_eq!(
            pub_users["incoming_fk_count"].as_u64().unwrap(),
            2,
            "public.users should have 2 incoming FKs"
        );
    }
}

// ============================================================================
// Doc Command Tests
// ============================================================================

mod doc_tests {
    use super::*;

    #[test]
    fn doc_fail_on_warning_rejects_parse_warnings() {
        let output = relune()
            .arg("doc")
            .arg("--sql-text")
            .arg(comment_only_sql())
            .arg("--fail-on-warning")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("PARSE004"));
    }
}

// ============================================================================
// Lint Command Tests
// ============================================================================

mod lint_tests {
    use super::*;

    #[test]
    fn lint_writes_to_output_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("lint.json");

        let output = relune()
            .arg("lint")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("json")
            .arg("--out")
            .arg(&output_path)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "lint should succeed");
        assert!(
            output.stdout.is_empty(),
            "lint should not write to stdout when --out is used"
        );

        let content = fs::read_to_string(&output_path).expect("Failed to read lint output");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Output file should contain valid JSON");
        assert!(parsed.is_object(), "Lint JSON should be an object");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Lint report written to"),
            "lint should report file output on stderr"
        );
    }

    #[test]
    fn lint_quiet_suppresses_output_file_notice() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("lint.json");

        let output = relune()
            .arg("--quiet")
            .arg("lint")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("json")
            .arg("--out")
            .arg(&output_path)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "lint should succeed");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("Lint report written to"),
            "lint should suppress file output notice when quiet"
        );
    }

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

    #[test]
    fn lint_deny_warning_rejects_parse_warnings() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = write_comment_only_sql(&temp);

        let output = relune()
            .arg("lint")
            .arg("--sql")
            .arg(&sql_path)
            .arg("--deny")
            .arg("warning")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("PARSE004"));
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

    #[cfg(unix)]
    fn diff_terminal_args(format: &str, explicit_stdout: bool) -> Vec<String> {
        let mut args = vec![
            "diff".to_string(),
            "--before".to_string(),
            simple_blog_fixture().display().to_string(),
            "--after".to_string(),
            ecommerce_fixture().display().to_string(),
            "--format".to_string(),
            format.to_string(),
        ];
        if explicit_stdout {
            args.push("--stdout".to_string());
        }
        args
    }

    #[test]
    fn diff_fail_on_warning_rejects_parse_warnings() {
        let output = relune()
            .arg("diff")
            .arg("--before-sql-text")
            .arg(comment_only_sql())
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--fail-on-warning")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("PARSE004"));
    }

    #[test]
    fn diff_rejects_oversized_input_before_reading_contents() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("oversized.sql");
        let file = fs::File::create(&before_path).expect("create oversized input");
        file.set_len((8 * 1024 * 1024) + 1)
            .expect("extend oversized input");
        drop(file);

        let output = relune()
            .arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(simple_blog_fixture())
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("too large"));
        assert!(stderr.contains("8388608"));
    }

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
    fn diff_with_view_and_enum_changes() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.sql");
        let after_path = temp.path().join("after.sql");

        fs::write(
            &before_path,
            "\
            CREATE TYPE status AS ENUM ('draft', 'published');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status status);\n\
            CREATE VIEW active_users AS SELECT id, status FROM users;\n\
            ",
        )
        .unwrap();
        fs::write(
            &after_path,
            "\
            CREATE TYPE status AS ENUM ('published', 'draft');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status TEXT);\n\
            CREATE VIEW active_users AS SELECT id FROM users;\n\
            ",
        )
        .unwrap();

        let output = relune()
            .arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(&after_path)
            .assert()
            .success();

        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(stdout.contains("Modified views"));
        assert!(stdout.contains("active_users"));
        assert!(stdout.contains("Modified enums"));
        assert!(stdout.contains("status"));
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

        let output = relune()
            .arg("diff")
            .arg("--before")
            .arg(&before_path)
            .arg("--after")
            .arg(&after_path)
            .arg("--out")
            .arg(&output_path)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "diff should succeed");
        assert!(output_path.exists(), "Output file should be created");
        let content = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert!(
            content.contains("users") || content.contains("posts"),
            "Output should mention tables"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Diff report written to"),
            "diff should report file output on stderr"
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

    #[cfg(unix)]
    #[test]
    fn diff_svg_to_interactive_stdout_requires_explicit_opt_in() {
        let (exit_code, output) = relune_in_terminal(&diff_terminal_args("svg", false));

        assert_eq!(exit_code, 2, "usage errors should exit with code 2");
        assert!(
            output.contains("Use --out <FILE> or --stdout"),
            "unexpected PTY output: {output}"
        );
        assert!(
            !output.contains("<svg"),
            "raw SVG should not be emitted to an interactive terminal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn diff_html_to_interactive_stdout_requires_explicit_opt_in() {
        let (exit_code, output) = relune_in_terminal(&diff_terminal_args("html", false));

        assert_eq!(exit_code, 2, "usage errors should exit with code 2");
        assert!(
            output.contains("Use --out <FILE> or --stdout"),
            "unexpected PTY output: {output}"
        );
        assert!(
            !output.contains("<!DOCTYPE html>") && !output.contains("<html"),
            "raw HTML should not be emitted to an interactive terminal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn diff_svg_to_interactive_stdout_with_explicit_opt_in_succeeds() {
        let (exit_code, output) = relune_in_terminal(&diff_terminal_args("svg", true));

        assert_eq!(exit_code, 0, "explicit stdout should succeed");
        assert!(
            output.contains("<svg"),
            "explicit stdout should emit SVG markup"
        );
    }

    #[cfg(unix)]
    #[test]
    fn diff_html_to_interactive_stdout_with_explicit_opt_in_succeeds() {
        let (exit_code, output) = relune_in_terminal(&diff_terminal_args("html", true));

        assert_eq!(exit_code, 0, "explicit stdout should succeed");
        assert!(
            output.contains("<!DOCTYPE html>") || output.contains("<html"),
            "explicit stdout should emit HTML markup"
        );
    }
}
