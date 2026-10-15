//! A diff diagram shows what changed apart from how risky it is.
//!
//! `fixtures/diff/change-risk` mixes added, removed, and modified tables and
//! columns, some risky and some safe. Each card and row must expose its
//! change kind and its review risk as independent signals.

use std::path::PathBuf;

use relune_app::{DiffFormat, DiffRequest, InputSource, diff};

fn render(format: DiffFormat) -> String {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/diff/change-risk");
    let request = DiffRequest {
        before: InputSource::sql_file(fixture.join("before.sql")),
        after: InputSource::sql_file(fixture.join("after.sql")),
        format,
        ..Default::default()
    };
    diff(request)
        .expect("diff should succeed")
        .rendered
        .expect("visual output expected")
}

/// Opening `<g>` tag of the card with `id`.
fn card_tag<'a>(svg: &'a str, id: &str) -> &'a str {
    let anchor = svg
        .find(&format!(r#"data-table-id="{id}""#))
        .unwrap_or_else(|| panic!("card {id} missing"));
    let start = svg[..anchor].rfind("<g ").expect("card tag start");
    let end = anchor + svg[anchor..].find('>').expect("card tag end");
    &svg[start..end]
}

fn row_class<'a>(svg: &'a str, column: &str) -> &'a str {
    let anchor = svg
        .find(&format!(r#"data-column-name="{column}""#))
        .unwrap_or_else(|| panic!("row {column} missing"));
    let start = svg[..anchor].rfind("class=\"").expect("row class") + 7;
    &svg[start..anchor - 2]
}

#[test]
fn cards_show_change_kind_and_risk_independently() {
    let svg = render(DiffFormat::Svg);

    // Modified with breaking changes inside.
    let customers = card_tag(&svg, "customers");
    assert!(customers.contains("diff-modified"), "{customers}");
    assert!(customers.contains("risk-breaking"), "{customers}");
    assert!(svg.contains(">breaking 2</text>"));

    // Modified, but nothing risky.
    let orders = card_tag(&svg, "orders");
    assert!(orders.contains("diff-modified"), "{orders}");
    assert!(!orders.contains("risk-"), "{orders}");

    // Added and safe; removed and breaking.
    let coupons = card_tag(&svg, "coupons");
    assert!(
        coupons.contains("diff-added") && !coupons.contains("risk-"),
        "{coupons}"
    );
    let audit_log = card_tag(&svg, "audit_log");
    assert!(audit_log.contains("diff-removed"), "{audit_log}");
    assert!(audit_log.contains("risk-breaking"), "{audit_log}");
    assert!(svg.contains(">breaking 1</text>"));
}

#[test]
fn rows_show_change_kind_and_type_changes() {
    let svg = render(DiffFormat::Svg);

    assert_eq!(row_class(&svg, "status"), "column-row diff-added");
    assert_eq!(row_class(&svg, "legacy_code"), "column-row diff-removed");
    assert_eq!(row_class(&svg, "total"), "column-row diff-modified");
    assert_eq!(row_class(&svg, "shipped_at"), "column-row diff-added");
    // Unchanged rows stay plain so the card still reads as context.
    assert_eq!(row_class(&svg, "code"), "column-row");

    // Widened (safe) and narrowed (breaking) types both read `before → after`.
    for (before, after) in [
        ("VARCHAR(120)", "VARCHAR(500)"),
        ("VARCHAR(500)", "VARCHAR(120)"),
    ] {
        let change = format!(
            r#"<tspan class="column-type-previous">{before}</tspan> → <tspan class="column-type-current""#
        );
        assert!(svg.contains(&change), "missing {before} → {after}");
    }
}

#[test]
fn html_metadata_keeps_risk_apart_from_change_kind() {
    let html = render(DiffFormat::Html);

    assert!(html.contains(r#""diff_kind":"modified""#));
    assert!(html.contains(r#""previous_data_type":"VARCHAR(500)""#));
    assert!(html.contains(r#""severity":"breaking""#));
    assert!(html.contains(r#""rule_id":"risk/add-not-null-on-existing""#));
}
