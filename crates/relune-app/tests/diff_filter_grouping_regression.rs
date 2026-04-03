//! Regression tests for diff visualization with filter, grouping, and overlay.
//!
//! These integration tests verify that the interaction between diff overlays
//! and filter/grouping specifications does not silently drop changes from
//! the rendered output.

use relune_app::{
    DiffFormat, DiffRequest, FilterSpec, GroupingSpec, GroupingStrategy, InputSource, diff,
};

/// Helper: run a diff request and return (`diff_result`, `svg_content`).
fn diff_svg(
    before: &str,
    after: &str,
    filter: FilterSpec,
    grouping: GroupingSpec,
) -> (relune_app::DiffResult, String) {
    let request = DiffRequest {
        before: InputSource::sql_text(before),
        after: InputSource::sql_text(after),
        format: DiffFormat::Svg,
        filter,
        grouping,
        ..Default::default()
    };
    let result = diff(request).expect("diff should succeed");
    let svg = result.rendered.clone().expect("SVG output expected");
    (result, svg)
}

// ---------------------------------------------------------------------------
// Regression: removed table + removed FK in a multi-table schema
// ---------------------------------------------------------------------------

#[test]
fn regression_removed_table_with_fk_visible_in_svg() {
    let before = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE profiles (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        CREATE TABLE sessions (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
    ";
    let after = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE profiles (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
    ";

    let (result, svg) = diff_svg(
        before,
        after,
        FilterSpec::default(),
        GroupingSpec::default(),
    );

    assert!(!result.diff.removed_tables.is_empty());
    assert!(
        svg.contains("sessions"),
        "removed table 'sessions' must appear in default (unfiltered) SVG"
    );
    assert!(svg.contains("overlay-error"), "removed table overlay");
}

#[test]
fn regression_removed_table_hidden_by_include_filter() {
    let before = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE profiles (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        CREATE TABLE sessions (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
    ";
    let after = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE profiles (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
    ";

    let filter = FilterSpec {
        include: vec!["users".to_string(), "profiles".to_string()],
        exclude: vec![],
    };
    let (result, svg) = diff_svg(before, after, filter, GroupingSpec::default());

    // Diff data always reflects reality
    assert!(result.diff.removed_tables.contains(&"sessions".to_string()));
    // But the visual excludes it per the filter
    assert!(
        !svg.contains("sessions"),
        "filtered-out table should not render"
    );
}

// ---------------------------------------------------------------------------
// Regression: modified table with removed columns + grouping
// ---------------------------------------------------------------------------

#[test]
fn regression_modified_table_removed_columns_with_grouping() {
    let before = r"
        CREATE TABLE app_users (
            id INT PRIMARY KEY,
            name VARCHAR(255),
            legacy_field VARCHAR(100)
        );
        CREATE TABLE app_posts (
            id INT PRIMARY KEY,
            user_id INT REFERENCES app_users(id),
            title VARCHAR(255)
        );
    ";
    let after = r"
        CREATE TABLE app_users (
            id INT PRIMARY KEY,
            name VARCHAR(255)
        );
        CREATE TABLE app_posts (
            id INT PRIMARY KEY,
            user_id INT REFERENCES app_users(id),
            title VARCHAR(255),
            body TEXT
        );
    ";

    let grouping = GroupingSpec {
        strategy: GroupingStrategy::ByPrefix,
    };
    let (result, svg) = diff_svg(before, after, FilterSpec::default(), grouping);

    assert_eq!(result.diff.modified_tables.len(), 2);
    assert!(svg.contains("overlay-warning"), "modified tables overlay");
    // Both tables must appear
    assert!(svg.contains("app_users"));
    assert!(svg.contains("app_posts"));
}

// ---------------------------------------------------------------------------
// Regression: added table that only has FK to excluded table
// ---------------------------------------------------------------------------

#[test]
fn regression_added_table_with_fk_to_excluded_target() {
    let before = r"
        CREATE TABLE users (id INT PRIMARY KEY);
    ";
    let after = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
    ";

    // Include only posts, exclude users (the FK target)
    let filter = FilterSpec {
        include: vec!["posts".to_string()],
        exclude: vec![],
    };
    let (_result, svg) = diff_svg(before, after, filter, GroupingSpec::default());

    // Should render without panic
    assert!(svg.contains("<svg"));
    assert!(svg.contains("overlay-info"), "added table overlay");
}

// ---------------------------------------------------------------------------
// Regression: all tables removed
// ---------------------------------------------------------------------------

#[test]
fn regression_all_tables_removed() {
    let before = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE posts (id INT PRIMARY KEY);
    ";
    let after = "";

    let (result, svg) = diff_svg(
        before,
        after,
        FilterSpec::default(),
        GroupingSpec::default(),
    );

    assert_eq!(result.diff.removed_tables.len(), 2);
    assert!(svg.contains("overlay-error"), "removed table overlay");
    assert!(svg.contains("users"));
    assert!(svg.contains("posts"));
}

// ---------------------------------------------------------------------------
// Regression: all tables added
// ---------------------------------------------------------------------------

#[test]
fn regression_all_tables_added() {
    let before = "";
    let after = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
    ";

    let (result, svg) = diff_svg(
        before,
        after,
        FilterSpec::default(),
        GroupingSpec::default(),
    );

    assert_eq!(result.diff.added_tables.len(), 2);
    assert!(svg.contains("overlay-info"), "added table overlay");
}

// ---------------------------------------------------------------------------
// Regression: filter + grouping + multi-schema combined
// ---------------------------------------------------------------------------

#[test]
fn regression_filter_grouping_multi_schema_combined() {
    let before = r"
        CREATE SCHEMA public;
        CREATE TABLE public.users (id INT PRIMARY KEY);
        CREATE TABLE public.posts (id INT PRIMARY KEY, user_id INT REFERENCES public.users(id));
        CREATE SCHEMA analytics;
        CREATE TABLE analytics.events (id INT PRIMARY KEY);
    ";
    let after = r"
        CREATE SCHEMA public;
        CREATE TABLE public.users (id INT PRIMARY KEY, email VARCHAR(255));
        CREATE TABLE public.posts (id INT PRIMARY KEY, user_id INT REFERENCES public.users(id));
        CREATE SCHEMA analytics;
        CREATE TABLE analytics.events (id INT PRIMARY KEY, ts TIMESTAMP);
        CREATE TABLE analytics.metrics (id INT PRIMARY KEY);
    ";

    let filter = FilterSpec {
        include: vec!["public.*".to_string()],
        exclude: vec![],
    };
    let grouping = GroupingSpec {
        strategy: GroupingStrategy::BySchema,
    };
    let (result, svg) = diff_svg(before, after, filter, grouping);

    // Diff data includes all changes
    assert!(result.diff.modified_tables.len() >= 2);
    assert_eq!(result.diff.added_tables.len(), 1);

    // Only public schema tables should render
    assert!(svg.contains("users"));
    assert!(svg.contains("posts"));
    assert!(
        !svg.contains("analytics"),
        "analytics schema should be filtered out"
    );
}

// ---------------------------------------------------------------------------
// Regression: renamed FK (old removed, new added) on same table
// ---------------------------------------------------------------------------

#[test]
fn regression_fk_replacement_on_modified_table() {
    let before = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE categories (id INT PRIMARY KEY);
        CREATE TABLE posts (
            id INT PRIMARY KEY,
            author_id INT REFERENCES users(id)
        );
    ";
    let after = r"
        CREATE TABLE users (id INT PRIMARY KEY);
        CREATE TABLE categories (id INT PRIMARY KEY);
        CREATE TABLE posts (
            id INT PRIMARY KEY,
            category_id INT REFERENCES categories(id)
        );
    ";

    let (result, svg) = diff_svg(
        before,
        after,
        FilterSpec::default(),
        GroupingSpec::default(),
    );

    // posts should be modified with FK changes
    let posts_diff = result
        .diff
        .modified_tables
        .iter()
        .find(|t| t.table_name == "posts");
    assert!(posts_diff.is_some(), "posts should be in modified_tables");
    let posts_diff = posts_diff.unwrap();
    assert!(
        posts_diff.fk_diffs.len() >= 2,
        "should have both removed and added FK diffs"
    );

    assert!(svg.contains("overlay-warning"), "modified table overlay");
    // All three tables visible
    assert!(svg.contains("users"));
    assert!(svg.contains("categories"));
    assert!(svg.contains("posts"));
}
