//! Golden tests for the migration risk review pipeline.
//!
//! Each subdirectory under `fixtures/review/` provides `before.sql` and
//! `after.sql`. The test runs the review pipeline and asserts the
//! resulting `findings` array matches the persisted `expected.json`.
//!
//! Set `UPDATE_FIXTURES=1` to regenerate `expected.json` from the
//! current pipeline output.

use std::fs;
use std::path::Path;

use relune_app::{ReviewRequest, review};
use relune_testkit::workspace_root;

#[test]
fn review_golden_fixtures_match_expected_findings() {
    let root = workspace_root().join("fixtures").join("review");
    let mut entries: Vec<_> = fs::read_dir(&root)
        .expect("fixtures/review directory should exist")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    assert!(
        !entries.is_empty(),
        "expected at least one review fixture directory"
    );

    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    let mut failures = Vec::new();

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        let before_path = path.join("before.sql");
        let after_path = path.join("after.sql");
        let expected_path = path.join("expected.json");

        let before = read_or_skip(&before_path);
        let after = read_or_skip(&after_path);

        let result = review(ReviewRequest::from_sql(before, after))
            .unwrap_or_else(|err| panic!("review failed for fixture {name}: {err:?}"));
        let actual = serde_json::to_value(&result.review.findings).unwrap();
        let actual_pretty = serde_json::to_string_pretty(&actual).unwrap();

        if update {
            fs::write(&expected_path, format!("{actual_pretty}\n"))
                .unwrap_or_else(|err| panic!("failed to write {}: {err}", expected_path.display()));
            continue;
        }

        let expected_pretty = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
            panic!(
                "failed to read {} (set UPDATE_FIXTURES=1): {err}",
                expected_path.display()
            )
        });
        let expected: serde_json::Value = serde_json::from_str(&expected_pretty)
            .unwrap_or_else(|err| panic!("invalid JSON in {}: {err}", expected_path.display()));

        if actual != expected {
            failures.push(format!(
                "fixture `{name}` mismatched.\n  expected: {expected_pretty}\n  actual:   {actual_pretty}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} fixture mismatch(es):\n{}",
        failures.len(),
        failures.join("\n---\n"),
    );
}

fn read_or_skip(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read fixture file {}: {err}", path.display()))
}
