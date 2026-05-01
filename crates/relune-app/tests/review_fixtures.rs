//! Golden tests for the migration risk review pipeline.
//!
//! Each fixture directory under `fixtures/review/` provides
//! `before.sql` and `after.sql`. The runner walks the directory tree
//! recursively so per-rule fixture groups can live in subdirectories
//! (`fixtures/review/lock-risk/<rule>-<dialect>/...`).
//!
//! An optional per-fixture `meta.toml` configures the request; an
//! optional `expected_diagnostics.json` locks down the diagnostics
//! payload. Fixtures without `meta.toml` keep running with default
//! settings and must produce zero diagnostics.
//!
//! Set `UPDATE_FIXTURES=1` to regenerate `expected.json` (and any
//! `expected_diagnostics.json` already present) from the current
//! pipeline output.

use std::fs;
use std::path::{Path, PathBuf};

use relune_app::{InputSource, ReviewRequest, review};
use relune_core::SqlDialect;
use relune_testkit::workspace_root;
use serde::Deserialize;

/// Optional per-fixture configuration loaded from `meta.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct FixtureMeta {
    #[serde(default)]
    dialect: Option<SqlDialect>,
    #[serde(default)]
    rules: Option<Vec<String>>,
    #[serde(default)]
    except_rules: Option<Vec<String>>,
    #[serde(default)]
    except_tables: Option<Vec<String>>,
}

#[test]
fn review_golden_fixtures_match_expected_findings() {
    let root = workspace_root().join("fixtures").join("review");
    let mut fixtures = Vec::new();
    collect_fixtures(&root, &root, &mut fixtures);
    fixtures.sort_by(|a, b| a.relative.cmp(&b.relative));

    assert!(
        !fixtures.is_empty(),
        "expected at least one review fixture directory under {}",
        root.display(),
    );

    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    let mut failures = Vec::new();

    for fixture in fixtures {
        if let Err(err) = run_fixture(&fixture, update) {
            failures.push(err);
        }
    }

    assert!(
        failures.is_empty(),
        "{} fixture mismatch(es):\n{}",
        failures.len(),
        failures.join("\n---\n"),
    );
}

struct Fixture {
    /// Absolute path to the fixture directory.
    path: PathBuf,
    /// Path relative to `fixtures/review/`, used for stable ordering
    /// and human-readable error messages (`lock-risk/foo/bar`).
    relative: String,
}

fn collect_fixtures(root: &Path, dir: &Path, out: &mut Vec<Fixture>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read directory {}: {err}", dir.display()));
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }
        // A directory is treated as a fixture iff both before.sql and
        // after.sql exist; otherwise descend looking for leaf fixtures.
        if path.join("before.sql").exists() && path.join("after.sql").exists() {
            let relative = path.strip_prefix(root).map_or_else(
                |_| path.to_string_lossy().into_owned(),
                |p| p.to_string_lossy().into_owned(),
            );
            out.push(Fixture { path, relative });
        } else {
            collect_fixtures(root, &path, out);
        }
    }
}

fn run_fixture(fixture: &Fixture, update: bool) -> Result<(), String> {
    let before = read_required(&fixture.path.join("before.sql"));
    let after = read_required(&fixture.path.join("after.sql"));
    let meta = load_meta(&fixture.path.join("meta.toml"));

    let request = build_request(before, after, meta);
    let result = review(request).map_err(|err| {
        format!(
            "fixture `{name}` failed to run: {err:?}",
            name = fixture.relative
        )
    })?;

    check_findings(fixture, &result.review.findings, update)?;
    check_diagnostics(fixture, &result.diagnostics, update)?;

    Ok(())
}

fn build_request(before: String, after: String, meta: FixtureMeta) -> ReviewRequest {
    let mut request = ReviewRequest::from_sql(before, after);
    if let Some(dialect) = meta.dialect {
        request = request.with_dialect(dialect);
        request = with_parser_dialect(request, dialect);
    }
    if let Some(rules) = meta.rules {
        request = request.with_rules(rules);
    }
    if let Some(except_rules) = meta.except_rules {
        request = request.with_except_rules(except_rules);
    }
    if let Some(except_tables) = meta.except_tables {
        request = request.with_except_tables(except_tables);
    }
    request
}

fn check_findings<T: serde::Serialize>(
    fixture: &Fixture,
    findings: &T,
    update: bool,
) -> Result<(), String> {
    let actual = serde_json::to_value(findings).unwrap();
    let actual_pretty = serde_json::to_string_pretty(&actual).unwrap();
    let path = fixture.path.join("expected.json");

    if update {
        write_expected(&path, &actual_pretty);
        return Ok(());
    }

    let expected_pretty = fs::read_to_string(&path).map_err(|err| {
        format!(
            "fixture `{}` missing expected.json (set UPDATE_FIXTURES=1): {err}",
            fixture.relative,
        )
    })?;
    let expected: serde_json::Value = serde_json::from_str(&expected_pretty).map_err(|err| {
        format!(
            "fixture `{}` has invalid JSON in expected.json: {err}",
            fixture.relative,
        )
    })?;
    if actual != expected {
        return Err(format!(
            "fixture `{name}` findings mismatch.\n  expected: {expected_pretty}\n  actual:   {actual_pretty}",
            name = fixture.relative
        ));
    }
    Ok(())
}

fn check_diagnostics<T: serde::Serialize>(
    fixture: &Fixture,
    diagnostics: &[T],
    update: bool,
) -> Result<(), String> {
    let actual = serde_json::to_value(diagnostics).unwrap();
    let actual_pretty = serde_json::to_string_pretty(&actual).unwrap();
    let path = fixture.path.join("expected_diagnostics.json");

    if !path.exists() {
        if !diagnostics.is_empty() {
            return Err(format!(
                "fixture `{name}` produced unexpected diagnostics (add expected_diagnostics.json to lock them down): {actual_pretty}",
                name = fixture.relative
            ));
        }
        return Ok(());
    }

    if update {
        write_expected(&path, &actual_pretty);
        return Ok(());
    }

    let expected_pretty = fs::read_to_string(&path).map_err(|err| {
        format!(
            "fixture `{}` failed to read expected_diagnostics.json: {err}",
            fixture.relative,
        )
    })?;
    let expected: serde_json::Value = serde_json::from_str(&expected_pretty).map_err(|err| {
        format!(
            "fixture `{}` has invalid JSON in expected_diagnostics.json: {err}",
            fixture.relative,
        )
    })?;
    if actual != expected {
        return Err(format!(
            "fixture `{name}` diagnostics mismatch.\n  expected: {expected_pretty}\n  actual:   {actual_pretty}",
            name = fixture.relative
        ));
    }
    Ok(())
}

fn write_expected(path: &Path, pretty: &str) {
    fs::write(path, format!("{pretty}\n"))
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
}

fn read_required(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read fixture file {}: {err}", path.display()))
}

fn load_meta(path: &Path) -> FixtureMeta {
    if !path.exists() {
        return FixtureMeta::default();
    }
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    toml::from_str(&text)
        .unwrap_or_else(|err| panic!("invalid meta.toml at {}: {err}", path.display()))
}

/// Re-tag the parser dialect on every `SqlText` input so the lexer
/// uses the right reserved-word set when fixtures pin a dialect.
/// `SchemaJson` inputs are untouched.
fn with_parser_dialect(mut request: ReviewRequest, dialect: SqlDialect) -> ReviewRequest {
    request.before = retag_parser_dialect(request.before, dialect);
    request.after = retag_parser_dialect(request.after, dialect);
    request
}

fn retag_parser_dialect(input: InputSource, dialect: SqlDialect) -> InputSource {
    match input {
        InputSource::SqlText { sql, .. } => InputSource::sql_text_with_dialect(sql, dialect),
        InputSource::SqlFile { path, .. } => InputSource::sql_file_with_dialect(path, dialect),
        other => other,
    }
}
