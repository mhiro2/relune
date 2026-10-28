//! Integration tests for relune CLI commands.
//!
//! These tests exercise the CLI through actual subprocess invocations,
//! testing render, inspect, and export commands with real fixtures.

use std::collections::BTreeSet;
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
    let mut cmd = Command::cargo_bin("relune").expect("Failed to find relune binary");
    // The CLI falls back to DATABASE_URL for live-DB input when no input flag
    // is given, and RUST_LOG takes precedence over -v; clear both so tests are
    // deterministic regardless of the developer's shell environment.
    cmd.env_remove("DATABASE_URL");
    cmd.env_remove("RUST_LOG");
    cmd
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

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Schema whose `posts` table has a two-hop neighbour (`reactions`) and an
/// unconnected table (`settings`), so a depth-1 focus on `posts` must drop
/// both.
const FOCUS_SQL: &str = "
    CREATE TABLE users (id INT PRIMARY KEY);
    CREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
    CREATE TABLE comments (id INT PRIMARY KEY, post_id INT REFERENCES posts(id));
    CREATE TABLE reactions (id INT PRIMARY KEY, comment_id INT REFERENCES comments(id));
    CREATE TABLE settings (id INT PRIMARY KEY);
";

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

fn edge_pairs(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    pairs
        .iter()
        .map(|(from, to)| ((*from).to_string(), (*to).to_string()))
        .collect()
}

fn simple_blog_tables() -> BTreeSet<String> {
    names(&["comments", "posts", "users"])
}

fn simple_blog_edges() -> BTreeSet<(String, String)> {
    edge_pairs(&[
        ("comments", "posts"),
        ("comments", "users"),
        ("posts", "users"),
    ])
}

fn parse_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).expect("Output should be valid JSON")
}

/// Node IDs of a graph-json document.
fn graph_tables(graph: &serde_json::Value) -> BTreeSet<String> {
    graph["nodes"]
        .as_array()
        .expect("graph JSON should have a nodes array")
        .iter()
        .map(|node| node["id"].as_str().expect("node id").to_string())
        .collect()
}

/// `(from, to)` pairs of a graph-json document.
fn graph_edges(graph: &serde_json::Value) -> BTreeSet<(String, String)> {
    graph["edges"]
        .as_array()
        .expect("graph JSON should have an edges array")
        .iter()
        .map(|edge| {
            (
                edge["from"].as_str().expect("edge from").to_string(),
                edge["to"].as_str().expect("edge to").to_string(),
            )
        })
        .collect()
}

/// Fill color of the SVG canvas background pattern.
fn canvas_fill(svg: &str) -> &str {
    let pattern = svg
        .find(r#"<pattern id="canvas-grid""#)
        .expect("SVG should define the canvas pattern");
    let fill = pattern + svg[pattern..].find(r#"fill=""#).expect("canvas fill") + 6;
    let len = svg[fill..].find('"').expect("closing quote");
    &svg[fill..fill + len]
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
    command.env_remove("DATABASE_URL");
    command.env_remove("RUST_LOG");
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

        let graph = parse_json(&fs::read(&output_path).expect("Failed to read output file"));
        assert_eq!(graph_tables(&graph), simple_blog_tables());
        assert_eq!(graph_edges(&graph), simple_blog_edges());
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
    fn render_with_theme_selects_canvas_colors() {
        let render = |theme: &str| {
            let output = relune()
                .arg("render")
                .arg("--sql")
                .arg(simple_blog_fixture())
                .arg("--theme")
                .arg(theme)
                .arg("--stdout")
                .assert()
                .success();
            String::from_utf8_lossy(&output.get_output().stdout).into_owned()
        };

        assert_eq!(canvas_fill(&render("dark")), "#0c0f1a");
        assert_eq!(canvas_fill(&render("light")), "#f7f8fc");
    }

    #[test]
    fn render_with_focus() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("output.svg");

        let mut cmd = relune();
        cmd.arg("render")
            .arg("--sql-text")
            .arg(FOCUS_SQL)
            .arg("--focus")
            .arg("posts")
            .arg("--depth")
            .arg("1")
            .arg("--out")
            .arg(&output_path)
            .assert()
            .success();

        let svg = fs::read_to_string(&output_path).expect("Failed to read output file");
        assert_eq!(
            relune_testkit::svg::table_ids(&svg),
            names(&["comments", "posts", "users"])
        );
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
        let render = |layout: &str| {
            let output = relune()
                .arg("render")
                .arg("--sql")
                .arg(simple_blog_fixture())
                .arg("--layout")
                .arg(layout)
                .arg("--edge-style")
                .arg("orthogonal")
                .arg("--stdout")
                .assert()
                .success();
            String::from_utf8_lossy(&output.get_output().stdout).into_owned()
        };

        let force = relune_testkit::svg::table_bodies(&render("force-directed"));
        let hierarchical = relune_testkit::svg::table_bodies(&render("hierarchical"));

        assert_eq!(
            force.keys().cloned().collect::<BTreeSet<_>>(),
            simple_blog_tables()
        );
        assert_ne!(
            force, hierarchical,
            "force-directed layout should place cards differently from the default"
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
    fn database_url_env_is_used_as_db_input_fallback() {
        // With no input flag, DATABASE_URL is consumed as live-DB input. An
        // unsupported scheme fails fast (no connection), proving the env value
        // reached introspection rather than tripping the "no input" usage error.
        let output = relune()
            .arg("inspect")
            .env("DATABASE_URL", "unsupported://example/db")
            .output()
            .expect("command should run");

        assert!(!output.status.success(), "unsupported scheme should fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Unsupported database URL"),
            "DATABASE_URL should be treated as db input; stderr: {stderr}"
        );
    }

    #[test]
    fn explicit_input_flag_ignores_database_url_env() {
        // An explicit --sql must win even when DATABASE_URL is set.
        relune()
            .arg("inspect")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .env("DATABASE_URL", "unsupported://example/db")
            .assert()
            .success();
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
    fn render_missing_out_directory_is_usage_error() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let missing = temp.path().join("does-not-exist").join("erd.svg");

        let output = relune()
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--out")
            .arg(&missing)
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(2),
            "missing --out directory should be a usage error"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("does not exist") && stderr.contains("does-not-exist"),
            "stderr should name the missing directory: {stderr}"
        );
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

    #[cfg(unix)]
    #[test]
    fn render_png_rejects_terminal_stdout() {
        let (exit_code, output) = relune_in_terminal(&[
            "render".to_string(),
            "--sql".to_string(),
            simple_blog_fixture().display().to_string(),
            "--format".to_string(),
            "png".to_string(),
        ]);

        assert_eq!(exit_code, 2, "PNG on a terminal should be a usage error");
        assert!(
            output.contains("binary PNG data"),
            "terminal output should explain the refusal; got: {output}"
        );
        // Every PNG starts with an IHDR chunk; the terminal may rewrite line
        // endings in the signature, but not this chunk name.
        assert!(
            !output.contains("IHDR"),
            "no PNG bytes should reach the terminal"
        );
    }

    #[test]
    fn render_png_to_piped_stdout() {
        let output = relune()
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--format")
            .arg("png")
            .assert()
            .success();

        assert!(
            output.get_output().stdout.starts_with(PNG_SIGNATURE),
            "piped stdout should receive the PNG"
        );
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
        let pixmap = resvg::tiny_skia::Pixmap::decode_png(&content).expect("decode PNG");
        // The top-left pixel is plain canvas, painted with the dark theme base
        // color #0c0f1a.
        let corner = pixmap.pixel(0, 0).expect("corner pixel");
        assert_eq!(
            (corner.red(), corner.green(), corner.blue(), corner.alpha()),
            (0x0c, 0x0f, 0x1a, 0xff)
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

        let graph = parse_json(&output.get_output().stdout);
        assert_eq!(graph_tables(&graph), simple_blog_tables());
        assert_eq!(graph_edges(&graph), simple_blog_edges());
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

        let graph = parse_layout_json(&String::from_utf8_lossy(&output.get_output().stdout));
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect::<BTreeSet<_>>(),
            simple_blog_tables()
        );
        assert!(
            graph
                .nodes
                .iter()
                .all(|node| node.width > 0.0 && node.height > 0.0)
        );
        assert_eq!(
            graph
                .edges
                .iter()
                .map(|edge| (edge.from.clone(), edge.to.clone()))
                .collect::<BTreeSet<_>>(),
            simple_blog_edges()
        );
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
    fn export_layout_json_lays_out_cards_at_the_chosen_density() {
        let layout = |density: &str| {
            let output = relune()
                .arg("export")
                .arg("--sql")
                .arg(simple_blog_fixture())
                .arg("--format")
                .arg("layout-json")
                .arg("--density")
                .arg(density)
                .assert()
                .success();
            parse_layout_json(&String::from_utf8_lossy(&output.get_output().stdout))
        };

        let overview = layout("overview");
        assert!(overview.nodes.iter().all(|node| node.columns.is_empty()));

        let keys = layout("keys");
        let posts = keys.nodes.iter().find(|node| node.id == "posts").unwrap();
        let listed: Vec<&str> = posts.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(listed, ["id", "user_id"]);
        assert_eq!(posts.omitted.map(|omitted| omitted.count), Some(5));
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
    fn export_graph_json_with_focus() {
        let output = relune()
            .arg("export")
            .arg("--sql-text")
            .arg(FOCUS_SQL)
            .arg("--format")
            .arg("graph-json")
            .arg("--focus")
            .arg("posts")
            .arg("--depth")
            .arg("1")
            .assert()
            .success();

        let graph = parse_json(&output.get_output().stdout);
        assert_eq!(graph_tables(&graph), names(&["comments", "posts", "users"]));
        assert_eq!(
            graph_edges(&graph),
            edge_pairs(&[("comments", "posts"), ("posts", "users")])
        );
    }

    #[test]
    fn export_schema_json_keeps_every_table_under_focus() {
        // Schema JSON is the full normalized schema; focus only narrows the
        // graph-based formats.
        let output = relune()
            .arg("export")
            .arg("--sql-text")
            .arg(FOCUS_SQL)
            .arg("--format")
            .arg("schema-json")
            .arg("--focus")
            .arg("posts")
            .arg("--depth")
            .arg("1")
            .assert()
            .success();

        let schema = parse_json(&output.get_output().stdout);
        let tables = schema["tables"]
            .as_array()
            .expect("schema JSON should have a tables array")
            .iter()
            .map(|table| table["name"].as_str().expect("table name").to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            tables,
            names(&["comments", "posts", "reactions", "settings", "users"])
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

        // [export] picks graph-json, and the billing viewpoint (users at
        // depth 1, comments excluded) narrows the graph.
        let graph = parse_json(&output.get_output().stdout);
        assert!(graph.get("tables").is_none(), "should not be schema JSON");
        assert_eq!(graph_tables(&graph), names(&["posts", "users"]));
        assert_eq!(graph_edges(&graph), edge_pairs(&[("posts", "users")]));
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

        let parsed = parse_json(&output.get_output().stdout);
        let summary = &parsed["summary"];
        assert_eq!(summary["table_count"], 3);
        assert_eq!(summary["column_count"], 16);
        assert_eq!(summary["foreign_key_count"], 3);
        let users = summary["tables"]
            .as_array()
            .expect("summary tables")
            .iter()
            .find(|table| table["name"] == "users")
            .expect("users summary");
        assert_eq!(users["incoming_fk_count"], 2);
        assert!(
            parsed["table"].is_null(),
            "summary mode has no table detail"
        );
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
            .arg("posts")
            .arg("--format")
            .arg("json")
            .assert()
            .success();

        let parsed = parse_json(&output.get_output().stdout);
        let table = &parsed["table"];
        assert_eq!(table["name"], "posts");
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
        assert_eq!(
            table["foreign_keys"],
            serde_json::json!([{
                "name": null,
                "from_columns": ["user_id"],
                "to_table": "users",
                "to_columns": ["id"],
            }])
        );
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
    fn quiet_suppresses_non_error_diagnostics() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = write_comment_only_sql(&temp);

        // Without --quiet the parser warning is surfaced on stderr.
        let loud = relune()
            .arg("lint")
            .arg("--sql")
            .arg(&sql_path)
            .output()
            .expect("command should run");
        assert!(loud.status.success(), "lint should succeed on warnings");
        assert!(
            String::from_utf8_lossy(&loud.stderr).contains("PARSE004"),
            "warning should be printed without --quiet"
        );

        // With --quiet the same warning is suppressed (exit code unchanged).
        let quiet = relune()
            .arg("--quiet")
            .arg("lint")
            .arg("--sql")
            .arg(&sql_path)
            .output()
            .expect("command should run");
        assert!(quiet.status.success(), "lint should still succeed");
        assert!(
            !String::from_utf8_lossy(&quiet.stderr).contains("PARSE004"),
            "warning should be suppressed under --quiet"
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

    #[test]
    fn lint_deny_exits_with_code_10_when_issue_reaches_threshold() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = temp.path().join("schema.sql");
        // `no-primary-key` is a warning-level issue (not a parser diagnostic),
        // so this exercises the lint issue deny gate rather than the
        // diagnostic threshold.
        fs::write(&sql_path, "CREATE TABLE events (id INT, name TEXT);")
            .expect("write SQL fixture");

        let output = relune()
            .arg("lint")
            .arg("--sql")
            .arg(&sql_path)
            .arg("--rules")
            .arg("no-primary-key")
            .arg("--deny")
            .arg("warning")
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(10),
            "lint --deny should exit 10 when an issue reaches the threshold"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("at or above the configured severity threshold"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn lint_strict_profile_reports_missing_column_comment() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = temp.path().join("schema.sql");
        fs::write(
            &sql_path,
            r"
                CREATE TABLE users (
                    id INT PRIMARY KEY,
                    name TEXT
                );
                COMMENT ON TABLE users IS 'Application users';
            ",
        )
        .expect("write SQL fixture");

        let output = relune()
            .arg("lint")
            .arg("--sql")
            .arg(&sql_path)
            .arg("--profile")
            .arg("strict")
            .arg("--format")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(output.status.success(), "lint should succeed");
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("Output should be valid JSON");
        let issues = parsed["issues"]
            .as_array()
            .expect("issues should be an array");
        assert!(issues.iter().any(|issue| {
            issue["rule_id"] == serde_json::Value::String("missing-column-comment".to_string())
        }));
    }

    #[test]
    fn lint_except_table_suppresses_matching_table() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = temp.path().join("schema.sql");
        fs::write(
            &sql_path,
            r"
                CREATE TABLE audit_log (
                    event_name TEXT
                );
            ",
        )
        .expect("write SQL fixture");

        let output = relune()
            .arg("lint")
            .arg("--sql")
            .arg(&sql_path)
            .arg("--except-table")
            .arg("audit_*")
            .arg("--format")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(output.status.success(), "lint should succeed");
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("Output should be valid JSON");
        assert_eq!(parsed["stats"]["total"], 0);
        assert!(
            parsed["review"]["suppressed_issue_count"]
                .as_u64()
                .is_some_and(|count| count > 0)
        );
    }

    #[test]
    fn lint_rejects_explicit_documentation_rule_for_sqlite_sql() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let sql_path = temp.path().join("schema.sql");
        fs::write(
            &sql_path,
            r"
                CREATE TABLE posts (
                    id INTEGER PRIMARY KEY
                );
            ",
        )
        .expect("write SQL fixture");

        let output = relune()
            .arg("lint")
            .arg("--sql")
            .arg(&sql_path)
            .arg("--dialect")
            .arg("sqlite")
            .arg("--rules")
            .arg("missing-table-comment")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Documentation lint rules are unavailable"));
        assert!(stderr.contains("missing-table-comment"));
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
        assert!(stderr.contains(
            "focus target table not found after applying include/exclude filters: users"
        ));
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
        // Comment-only SQL always emits a PARSE004 warning, so the default run
        // has diagnostics for --quiet to hide.
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let render = |quiet: bool| {
            let mut cmd = relune();
            if quiet {
                cmd.arg("--quiet");
            }
            cmd.arg("render")
                .arg("--sql-text")
                .arg(comment_only_sql())
                .arg("--out")
                .arg(temp.path().join("output.svg"))
                .output()
                .expect("command should run")
        };

        let normal = render(false);
        assert!(normal.status.success());
        assert!(String::from_utf8_lossy(&normal.stderr).contains("PARSE004"));

        let quiet = render(true);
        assert!(quiet.status.success());
        assert!(
            quiet.stderr.is_empty(),
            "--quiet should hide warnings and progress; got: {}",
            String::from_utf8_lossy(&quiet.stderr)
        );
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
    fn rust_log_overrides_verbose_flag() {
        let output = relune()
            .env("RUST_LOG", "warn")
            .arg("-v")
            .arg("render")
            .arg("--sql")
            .arg(simple_blog_fixture())
            .arg("--stdout")
            .output()
            .expect("command should run");

        assert!(output.status.success(), "render should succeed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("parsed SQL"),
            "RUST_LOG=warn should silence info logs despite -v; got: {stderr}"
        );
    }

    #[test]
    fn color_flag_controls_ansi_diagnostics() {
        // Comment-only SQL always emits a PARSE004 warning to colorize.
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let stderr = |color: &str| {
            let output = relune()
                .arg("--color")
                .arg(color)
                .arg("render")
                .arg("--sql-text")
                .arg(comment_only_sql())
                .arg("--out")
                .arg(temp.path().join("output.svg"))
                .output()
                .expect("command should run");
            assert!(output.status.success());
            String::from_utf8_lossy(&output.stderr).into_owned()
        };

        let always = stderr("always");
        assert!(always.contains("PARSE004"));
        assert!(always.contains("\x1b["), "--color always should emit ANSI");

        let never = stderr("never");
        assert!(never.contains("PARSE004"));
        assert!(
            !never.contains('\x1b'),
            "--color never should not emit ANSI"
        );
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

        let graph = parse_json(&output.get_output().stdout);
        assert_eq!(
            graph_tables(&graph),
            names(&[
                "addresses",
                "categories",
                "customers",
                "order_items",
                "orders",
                "product_categories",
                "products",
            ])
        );
        assert_eq!(
            graph_edges(&graph),
            edge_pairs(&[
                ("addresses", "customers"),
                ("categories", "categories"),
                ("order_items", "orders"),
                ("order_items", "products"),
                ("orders", "customers"),
                ("product_categories", "categories"),
                ("product_categories", "products"),
            ])
        );
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
    fn diff_rejects_schema_with_duplicate_columns() {
        let output = relune()
            .arg("diff")
            .arg("--before-sql-text")
            .arg("CREATE TABLE t (id INT, id INT);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE t (id INT);")
            .output()
            .expect("command should run");

        failure_snapshot("diff_rejects_schema_with_duplicate_columns", &output);
    }

    #[test]
    fn diff_allow_invalid_schema_reports_validation_warning() {
        let output = relune()
            .arg("diff")
            .arg("--before-sql-text")
            .arg("CREATE TABLE t (id INT, id INT);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE t (id INT);")
            .arg("--allow-invalid-schema")
            .output()
            .expect("command should run");

        assert!(output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("SCHEMA004"));
        assert!(stderr.contains("duplicate column name 'id'"));
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

    #[cfg(unix)]
    #[test]
    fn diff_rejects_fifo_input_without_blocking() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.sql");
        let status = std::process::Command::new("mkfifo")
            .arg(&before_path)
            .status()
            .expect("run mkfifo");
        assert!(status.success());

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
        assert!(stderr.contains("not a regular file"), "stderr: {stderr}");
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

        let parsed = parse_json(&output.get_output().stdout);
        assert_eq!(parsed["summary"]["tables_modified"], 1);
        assert_eq!(parsed["summary"]["columns_changed"], 1);
        assert_eq!(parsed["added_tables"], serde_json::json!([]));
        assert_eq!(parsed["removed_tables"], serde_json::json!([]));
        let modified = parsed["modified_tables"]
            .as_array()
            .expect("modified_tables array");
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0]["table_name"], "users");
        assert_eq!(
            modified[0]["column_diffs"],
            serde_json::json!([{
                "column_name": "name",
                "change_kind": "added",
                "old_value": null,
                "new_value": {
                    "name": "name",
                    "data_type": "VARCHAR(255)",
                    "nullable": true,
                    "primary_key": false,
                },
            }])
        );
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

    #[test]
    fn diff_markdown_format() {
        let output = relune()
            .arg("diff")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));")
            .arg("--format")
            .arg("markdown")
            .output()
            .expect("command should run");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("## Schema Diff:"));
        assert!(stdout.contains("| Category | Added | Removed | Modified |"));
        assert!(stdout.contains("<details>"));
    }

    #[test]
    fn diff_exit_code_with_changes() {
        let output = relune()
            .arg("diff")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));")
            .arg("--exit-code")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(10));
    }

    #[test]
    fn diff_exit_code_no_changes() {
        let output = relune()
            .arg("diff")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--exit-code")
            .output()
            .expect("command should run");

        assert!(output.status.success());
    }
}

// ============================================================================
// Review Command Tests
// ============================================================================

mod review_tests {
    use super::*;

    const DROP_FK_BEFORE: &str = "
        CREATE TABLE users (id INT PRIMARY KEY, email TEXT);
        CREATE TABLE orders (
            id INT PRIMARY KEY,
            user_email TEXT REFERENCES users(email)
        );
    ";
    const DROP_FK_AFTER: &str = "
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE orders (
            id INT PRIMARY KEY,
            user_email TEXT REFERENCES users(email)
        );
    ";

    #[test]
    fn review_rejects_schema_with_duplicate_columns() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE t (id INT);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE t (id INT, id INT);")
            .output()
            .expect("command should run");

        failure_snapshot("review_rejects_schema_with_duplicate_columns", &output);
    }

    #[test]
    fn review_no_changes_reports_no_findings() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review with no changes should succeed"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Schema review"));
        assert!(stdout.contains("No risk findings"));
    }

    #[test]
    fn review_text_format_emits_buckets() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review should succeed without --deny"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("breaking"));
        assert!(stdout.contains("risk/drop-column-referenced"));
    }

    #[test]
    fn review_markdown_format_uses_bullet_lines() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .arg("-f")
            .arg("markdown")
            .output()
            .expect("command should run");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("## Schema review"));
        assert!(stdout.contains("- **`risk/drop-column-referenced`**"));
    }

    #[test]
    fn review_json_format_emits_structured_payload() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(output.status.success());
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("JSON output should parse");
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(findings.iter().any(|finding| {
            finding["rule_id"]
                == serde_json::Value::String("risk/drop-column-referenced".to_string())
        }));
        // The dropped column loses data and breaks the surviving FK.
        assert_eq!(parsed["summary"]["breaking"], serde_json::Value::from(2));
        assert_eq!(parsed["denied"], serde_json::Value::from(false));
    }

    #[test]
    fn review_quiet_suppresses_findings_body() {
        let output = relune()
            .arg("--quiet")
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "review --quiet should succeed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Schema review"));
        assert!(stdout.contains("breaking"));
        assert!(
            !stdout.contains("risk/drop-column-referenced"),
            "findings detail should be suppressed under --quiet, got: {stdout}"
        );
    }

    #[test]
    fn review_deny_breaking_returns_exit_code_10() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .arg("--deny")
            .arg("breaking")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(10));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--deny"));
    }

    #[test]
    fn review_exit_code_signals_findings() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY); CREATE TABLE orders (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY); CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));")
            .arg("--exit-code")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(10));
    }

    #[test]
    fn review_except_rule_suppresses_finding() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY); CREATE TABLE orders (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY); CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));")
            .arg("--except-rule")
            .arg("fk-without-index")
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review with --except-rule should succeed"
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("JSON output should parse");
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(
            !findings
                .iter()
                .any(|finding| finding["rule_id"] == "risk/fk-without-index"),
            "fk-without-index should be suppressed; got: {findings:?}"
        );
    }

    #[test]
    fn review_except_table_moves_finding_to_suppressed() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE audit_log (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE audit_log (id INT PRIMARY KEY, created_at TIMESTAMP NOT NULL);")
            .arg("--except-table")
            .arg("audit_*")
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review with --except-table should succeed"
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("JSON output should parse");
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(
            findings.is_empty(),
            "audit_log finding should be suppressed"
        );
        let suppressed = parsed["suppressed"].as_array().expect("suppressed array");
        assert!(
            !suppressed.is_empty(),
            "suppressed should contain the audit_log finding"
        );
    }

    #[test]
    fn review_writes_to_output_file() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let output_path = temp.path().join("review.json");

        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .arg("-f")
            .arg("json")
            .arg("--out")
            .arg(&output_path)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "review should succeed");
        assert!(
            output.stdout.is_empty(),
            "review should not write to stdout when --out is used"
        );
        let content = fs::read_to_string(&output_path).expect("Failed to read review output");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Output file should contain valid JSON");
        assert!(parsed["findings"].is_array());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Review report written to"));
    }

    #[test]
    fn review_uses_config_format() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");
        fs::write(&config_path, "[review]\nformat = \"markdown\"\n").unwrap();

        let output = relune()
            .arg("--config")
            .arg(&config_path)
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .output()
            .expect("command should run");

        assert!(output.status.success(), "review should succeed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("## Schema review"));
        assert!(stdout.contains("- **`risk/drop-column-referenced`**"));
    }

    #[test]
    fn review_severity_override_downgrades_via_config() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");
        fs::write(
            &config_path,
            "[review]\n\
             format = \"json\"\n\
             \n\
             [review.severity_overrides.\"risk/add-not-null-on-existing\"]\n\
             severity = \"info\"\n",
        )
        .unwrap();

        let output = relune()
            .arg("--config")
            .arg(&config_path)
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL);")
            .output()
            .expect("command should run");

        assert!(output.status.success(), "review should succeed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("review JSON should parse");
        assert_eq!(parsed["summary"]["warning"], 0);
        assert_eq!(parsed["summary"]["info"], 1);
        let findings = parsed["findings"].as_array().expect("findings array");
        let finding = findings
            .iter()
            .find(|f| f["rule_id"] == "risk/add-not-null-on-existing")
            .expect("override target finding");
        assert_eq!(finding["severity"], "info");
    }

    #[test]
    fn review_severity_override_unknown_rule_id_is_usage_error() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");
        fs::write(
            &config_path,
            "[review.severity_overrides.\"risk/does-not-exist\"]\n\
             severity = \"warning\"\n",
        )
        .unwrap();

        let output = relune()
            .arg("--config")
            .arg(&config_path)
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE t (id INT);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE t (id INT);")
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(2),
            "unknown rule_id in TOML override should map to usage exit code 2"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("review.severity_overrides has unknown rule_id 'risk/does-not-exist'"),
            "stderr should explain the unknown rule_id; got: {stderr}"
        );
    }

    #[test]
    fn review_severity_override_duplicate_rule_id_is_usage_error() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");
        fs::write(
            &config_path,
            "[review.severity_overrides.\"risk/fk-without-index\"]\n\
             severity = \"info\"\n\
             \n\
             [review.severity_overrides.\"RISK/FK-WITHOUT-INDEX\"]\n\
             severity = \"warning\"\n",
        )
        .unwrap();

        let output = relune()
            .arg("--config")
            .arg(&config_path)
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE t (id INT);")
            .arg("--after-sql-text")
            .arg("CREATE TABLE t (id INT);")
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(2),
            "duplicate rule_id in TOML override should map to usage exit code 2"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("review.severity_overrides has duplicate rule_id"),
            "stderr should explain the duplicate rule_id; got: {stderr}"
        );
    }

    #[test]
    fn review_list_rules_text_lists_every_rule() {
        let output = relune()
            .arg("review")
            .arg("--list-rules")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review --list-rules should succeed without before/after"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Each line is `<rule id> <default severity> <description>`; keep the
        // listing order-independent but exact, so a missing, extra, or
        // duplicated rule fails.
        let mut listed = stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut columns = line.split_whitespace();
                (
                    columns.next().expect("rule id column"),
                    columns.next().expect("severity column"),
                )
            })
            .collect::<Vec<_>>();
        listed.sort_unstable();
        let mut expected = vec![
            ("risk/drop-column", "breaking"),
            ("risk/drop-table", "breaking"),
            ("risk/drop-enum-value", "breaking"),
            ("risk/drop-column-referenced", "breaking"),
            ("risk/drop-table-referenced", "breaking"),
            ("risk/add-not-null-on-existing", "warning"),
            ("risk/type-narrow", "breaking"),
            ("risk/drop-pk-or-unique", "warning"),
            ("risk/add-unique-on-existing", "warning"),
            ("risk/add-check-on-existing", "warning"),
            ("risk/add-cascade-delete", "warning"),
            ("risk/fk-without-index", "info"),
            ("risk/add-index-on-large-table", "caution"),
            ("risk/add-fk-on-existing", "caution"),
            ("risk/alter-column-type", "caution"),
            ("risk/rewrite-table", "caution"),
        ];
        expected.sort_unstable();
        assert_eq!(listed, expected, "unexpected rule listing:\n{stdout}");
    }

    #[test]
    fn review_list_rules_json_emits_metadata_array() {
        let output = relune()
            .arg("review")
            .arg("--list-rules")
            .arg("--format")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review --list-rules --format json should succeed"
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("list-rules output should be valid JSON");
        let entries = parsed
            .as_array()
            .expect("--list-rules JSON should be an array");
        assert_eq!(entries.len(), 16, "expected metadata for every review rule");
        let first = &entries[0];
        assert_eq!(first["rule_id"], "risk/drop-column");
        assert_eq!(first["default_severity"], "breaking");
        assert!(
            first["description"].as_str().is_some_and(|d| !d.is_empty()),
            "description should be populated"
        );

        let by_id: std::collections::HashMap<&str, &serde_json::Value> = entries
            .iter()
            .map(|entry| {
                (
                    entry["rule_id"]
                        .as_str()
                        .expect("rule_id should be a string"),
                    entry,
                )
            })
            .collect();
        for (rule_id, expected_severity, expected_description) in [
            (
                "risk/add-index-on-large-table",
                "caution",
                "New index on existing table; non-CONCURRENT/INPLACE builds lock the table",
            ),
            (
                "risk/add-fk-on-existing",
                "caution",
                "New foreign key validates every existing row under a blocking lock",
            ),
            (
                "risk/alter-column-type",
                "caution",
                "Existing column's type change may rewrite the table under an exclusive lock",
            ),
            (
                "risk/rewrite-table",
                "caution",
                "Schema change forces a table rebuild on MySQL 5.7-compatible engines",
            ),
        ] {
            let entry = by_id
                .get(rule_id)
                .unwrap_or_else(|| panic!("metadata for {rule_id} should exist"));
            assert_eq!(
                entry["default_severity"], expected_severity,
                "{rule_id} default_severity"
            );
            assert_eq!(
                entry["description"], expected_description,
                "{rule_id} description"
            );
        }
    }

    #[test]
    fn review_list_rules_rejects_markdown_format() {
        let output = relune()
            .arg("review")
            .arg("--list-rules")
            .arg("--format")
            .arg("markdown")
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(2),
            "--list-rules with markdown should fail as a usage error"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("markdown is not a supported format for --list-rules"),
            "stderr should explain the rejection; got: {stderr}"
        );
    }

    #[test]
    fn review_emit_summary_writes_json_even_when_denied() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let summary_path = temp.path().join("review-summary.json");
        let main_out_path = temp.path().join("review-main.txt");

        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .arg("--deny")
            .arg("breaking")
            .arg("--emit-summary")
            .arg(&summary_path)
            .arg("--out")
            .arg(&main_out_path)
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(10),
            "--deny breaking should still trip rc=10 with --emit-summary"
        );
        assert!(
            main_out_path.exists(),
            "main --out file should be written before deny short-circuit"
        );
        assert!(
            summary_path.exists(),
            "--emit-summary file should be written even when --deny short-circuits"
        );
        let summary = fs::read_to_string(&summary_path).expect("Failed to read summary");
        let parsed: serde_json::Value =
            serde_json::from_str(&summary).expect("emit-summary file should contain valid JSON");
        assert_eq!(parsed["denied"], true);
        assert_eq!(parsed["summary"]["breaking"], 2);
        assert!(parsed["findings"].is_array());
        assert!(parsed["applied_rules"].is_array());
    }

    const UNSUPPORTED_AFTER: &str = "
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE SEQUENCE order_seq;
    ";

    #[test]
    fn review_skipped_constructs_pass_without_fail_on_warning() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg(UNSUPPORTED_AFTER)
            .arg("--deny")
            .arg("breaking")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Review coverage is incomplete"), "{stdout}");
        assert!(
            stdout.contains("1 unsupported SQL construct in the after input was skipped"),
            "{stdout}"
        );
    }

    #[test]
    fn review_fail_on_warning_rejects_skipped_constructs_after_writing_reports() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let summary_path = temp.path().join("review-summary.json");
        let main_out_path = temp.path().join("review-main.md");

        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--after-sql-text")
            .arg(UNSUPPORTED_AFTER)
            .arg("--format")
            .arg("markdown")
            .arg("--fail-on-warning")
            .arg("--emit-summary")
            .arg(&summary_path)
            .arg("--out")
            .arg(&main_out_path)
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let report = fs::read_to_string(&main_out_path).expect("report should be written");
        assert!(report.contains("> [!WARNING]"), "{report}");
        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&summary_path).expect("summary should be written"),
        )
        .expect("summary should be JSON");
        assert_eq!(summary["inputs"]["after"]["unsupported_constructs"], 1);
    }

    #[test]
    fn review_fail_on_warning_from_config_rejects_empty_before() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");
        fs::write(&config_path, "[review]\nfail_on_warning = true\n").unwrap();
        let before_path = temp.path().join("before.sql");
        fs::write(&before_path, "-- no tables yet\n").unwrap();

        let output = relune()
            .arg("--config")
            .arg(&config_path)
            .arg("review")
            .arg("--before")
            .arg(&before_path)
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("The before input produced no schema objects"),
            "{stdout}"
        );
    }

    #[test]
    fn review_fail_on_warning_rejects_empty_schema_json_input() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let before_path = temp.path().join("before.json");
        fs::write(
            &before_path,
            r#"{"version":"2.0.0","tables":[],"views":[],"enums":[]}"#,
        )
        .unwrap();

        let output = relune()
            .arg("review")
            .arg("--before-schema-json")
            .arg(&before_path)
            .arg("--after-sql-text")
            .arg("CREATE TABLE users (id INT PRIMARY KEY);")
            .arg("--fail-on-warning")
            .output()
            .expect("command should run");

        assert_eq!(output.status.code(), Some(3));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("The before input produced no schema objects"),
            "{stdout}"
        );
    }

    #[test]
    fn review_emit_summary_rejects_path_collision_with_out() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let shared_path = temp.path().join("shared.json");

        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .arg("--format")
            .arg("json")
            .arg("--out")
            .arg(&shared_path)
            .arg("--emit-summary")
            .arg(&shared_path)
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(2),
            "reusing --out as --emit-summary should be a usage error"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Cannot reuse --out path as --emit-summary"),
            "stderr should explain the path collision; got: {stderr}"
        );
    }

    #[test]
    fn review_emit_summary_rejects_path_collision_when_files_do_not_exist() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let absolute_path = temp.path().join("only.json");

        // Same logical target expressed two different ways. Neither file
        // exists yet, so the literal strings differ but the resolved path is
        // identical — the collision must still be rejected.
        let mut relative_path = PathBuf::from("./");
        relative_path.push(absolute_path.file_name().expect("file name"));

        let output = relune()
            .current_dir(temp.path())
            .arg("review")
            .arg("--before-sql-text")
            .arg(DROP_FK_BEFORE)
            .arg("--after-sql-text")
            .arg(DROP_FK_AFTER)
            .arg("--format")
            .arg("json")
            .arg("--out")
            .arg(&relative_path)
            .arg("--emit-summary")
            .arg(&absolute_path)
            .output()
            .expect("command should run");

        assert_eq!(
            output.status.code(),
            Some(2),
            "logical path collision should be detected even when neither file exists"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Cannot reuse --out path as --emit-summary"),
            "stderr should explain the path collision; got: {stderr}"
        );
    }

    // Lock-risk fixture used by the dialect-aware smoke tests below: a new
    // index is added on an existing table, which triggers
    // `risk/add-index-on-large-table` only when the effective dialect is
    // postgres or mysql.
    const LOCK_RISK_BEFORE: &str =
        "CREATE TABLE orders (\n    id BIGINT PRIMARY KEY,\n    user_id BIGINT NOT NULL\n);";
    const LOCK_RISK_AFTER: &str = "CREATE TABLE orders (\n    id BIGINT PRIMARY KEY,\n    user_id BIGINT NOT NULL\n);\n\nCREATE INDEX orders_user_id_idx ON orders (user_id);";

    #[test]
    fn review_dialect_postgres_emits_lock_risk_caution() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(LOCK_RISK_BEFORE)
            .arg("--after-sql-text")
            .arg(LOCK_RISK_AFTER)
            .arg("--dialect")
            .arg("postgres")
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review --dialect postgres should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("review JSON should parse");
        assert_eq!(parsed["summary"]["caution"], 1);
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(
            findings
                .iter()
                .any(|f| f["rule_id"] == "risk/add-index-on-large-table"
                    && f["severity"] == "caution"),
            "expected a caution finding for risk/add-index-on-large-table; got: {parsed}"
        );
        let diagnostics = parsed["diagnostics"].as_array().expect("diagnostics array");
        assert!(
            diagnostics.is_empty(),
            "no skip diagnostic should fire when lock-risk rules are active; got: {diagnostics:?}"
        );
    }

    #[test]
    fn review_dialect_auto_promotes_to_postgres_when_inputs_agree() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(LOCK_RISK_BEFORE)
            .arg("--after-sql-text")
            .arg(LOCK_RISK_AFTER)
            .arg("--rules")
            .arg("risk/add-index-on-large-table")
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review under --dialect auto with same-dialect inputs should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("review JSON should parse");
        assert_eq!(parsed["summary"]["caution"], 1);
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(
            findings
                .iter()
                .any(|f| f["rule_id"] == "risk/add-index-on-large-table"),
            "auto should promote to postgres and fire lock-risk; got: {findings:?}"
        );
        let diagnostics = parsed["diagnostics"].as_array().expect("diagnostics array");
        assert!(
            diagnostics.is_empty(),
            "promoted auto run should not emit any review diagnostic; got: {diagnostics:?}"
        );
    }

    #[test]
    fn review_dialect_auto_default_profile_runs_lock_risk_after_promotion() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(LOCK_RISK_BEFORE)
            .arg("--after-sql-text")
            .arg(LOCK_RISK_AFTER)
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review under --dialect auto with default profile should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("review JSON should parse");
        assert_eq!(parsed["summary"]["caution"], 1);
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(
            findings
                .iter()
                .any(|f| f["rule_id"] == "risk/add-index-on-large-table"),
            "default profile should fire lock-risk after auto promotion; got: {findings:?}"
        );
        let diagnostics = parsed["diagnostics"].as_array().expect("diagnostics array");
        assert!(
            diagnostics.is_empty(),
            "promoted auto run should not emit a skip diagnostic; got: {diagnostics:?}"
        );
    }

    #[test]
    fn review_dialect_sqlite_lock_risk_optin_emits_skip_diagnostic() {
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(LOCK_RISK_BEFORE)
            .arg("--after-sql-text")
            .arg(LOCK_RISK_AFTER)
            .arg("--dialect")
            .arg("sqlite")
            .arg("--rules")
            .arg("risk/add-index-on-large-table")
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review with explicit lock-risk opt-in under --dialect sqlite should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("review JSON should parse");
        assert_eq!(parsed["summary"]["caution"], 0);
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(
            findings.is_empty(),
            "lock-risk rule should be skipped under --dialect sqlite; got: {findings:?}"
        );
        let diagnostics = parsed["diagnostics"].as_array().expect("diagnostics array");
        assert_eq!(
            diagnostics.len(),
            1,
            "explicit opt-in should produce exactly one skip diagnostic; got: {diagnostics:?}"
        );
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic["severity"], "info");
        assert_eq!(diagnostic["code"]["prefix"], "REVIEW");
        assert_eq!(diagnostic["code"]["number"], 1);
        assert!(
            diagnostic["message"]
                .as_str()
                .is_some_and(|m| m.contains("Lock-risk review rules are not defined for sqlite")),
            "diagnostic message should explain the sqlite skip; got: {diagnostic}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Lock-risk review rules are not defined for sqlite"),
            "info diagnostic should also surface on stderr; got: {stderr}"
        );
    }

    #[test]
    fn review_dialect_auto_mismatch_emits_warning_diagnostic() {
        // Postgres-leaning SQL on the before side and MySQL-leaning SQL
        // on the after side: the parser resolves to two different
        // concrete dialects, so the review pipeline must keep `Auto`
        // and emit a `REVIEW002` warning so the operator can pin a
        // dialect explicitly.
        let before = "CREATE TABLE users (id SERIAL PRIMARY KEY);";
        let after = "CREATE TABLE `users` (id INT AUTO_INCREMENT PRIMARY KEY) ENGINE=InnoDB;";

        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(before)
            .arg("--after-sql-text")
            .arg(after)
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review under auto-mismatch should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("review JSON should parse");
        assert_eq!(parsed["requested_dialect"], "auto");
        assert_eq!(parsed["effective_dialect"], "auto");
        let diagnostics = parsed["diagnostics"].as_array().expect("diagnostics array");
        let warning = diagnostics
            .iter()
            .find(|d| d["severity"] == "warning")
            .expect("auto-mismatch should emit a warning diagnostic");
        assert_eq!(warning["code"]["prefix"], "REVIEW");
        assert_eq!(warning["code"]["number"], 2);
    }

    #[test]
    fn review_dialect_auto_promotion_surfaces_in_text_output() {
        // The text formatter should surface the resolved dialect when
        // the request was `auto` so operators can see why lock-risk
        // ran (or did not run) without inspecting the JSON payload.
        let output = relune()
            .arg("review")
            .arg("--before-sql-text")
            .arg(LOCK_RISK_BEFORE)
            .arg("--after-sql-text")
            .arg(LOCK_RISK_AFTER)
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review --format text should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("postgres"),
            "text output should announce the resolved dialect; got: {stdout}"
        );
    }

    #[test]
    fn review_dialect_from_config_drives_lock_risk() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let config_path = temp.path().join("relune.toml");
        fs::write(&config_path, "[review]\ndialect = \"postgres\"\n").unwrap();

        let output = relune()
            .arg("--config")
            .arg(&config_path)
            .arg("review")
            .arg("--before-sql-text")
            .arg(LOCK_RISK_BEFORE)
            .arg("--after-sql-text")
            .arg(LOCK_RISK_AFTER)
            .arg("-f")
            .arg("json")
            .output()
            .expect("command should run");

        assert!(
            output.status.success(),
            "review with [review.dialect] = postgres should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("review JSON should parse");
        // This SQL also auto-promotes to postgres, so the requested dialect is
        // what shows the config value was applied rather than auto-detection.
        assert_eq!(parsed["requested_dialect"], "postgres");
        assert_eq!(parsed["effective_dialect"], "postgres");
        assert_eq!(parsed["summary"]["caution"], 1);
        let findings = parsed["findings"].as_array().expect("findings array");
        assert!(
            findings
                .iter()
                .any(|f| f["rule_id"] == "risk/add-index-on-large-table"),
            "config-supplied dialect should drive lock-risk evaluation; got: {findings:?}"
        );
    }
}
