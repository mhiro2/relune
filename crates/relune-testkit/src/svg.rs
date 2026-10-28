//! Structural queries over rendered SVG markup.
//!
//! These helpers locate tables, columns, groups, and edges by the identifiers
//! the renderer writes into `data-*` attributes, so tests can assert on a
//! specific element instead of searching the whole document for a substring.

use std::collections::{BTreeMap, BTreeSet};

/// Axis-aligned rectangle read from a `<rect>` element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgRect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl SvgRect {
    /// Returns whether `other` lies entirely inside this rectangle.
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }
}

/// A relationship edge rendered as `<g class="edge ...">`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SvgEdge {
    /// Source node ID (`data-from`).
    pub from: String,
    /// Target node ID (`data-to`).
    pub to: String,
    /// Classes on the edge group, such as `diff-added`.
    pub classes: BTreeSet<String>,
}

/// Returns every start tag that begins with `prefix` (for example
/// `<g class="table-node`), paired with its byte offset.
fn start_tags<'a>(svg: &'a str, prefix: &'a str) -> impl Iterator<Item = (usize, &'a str)> + 'a {
    svg.match_indices(prefix).map(move |(start, _)| {
        let end = svg[start..]
            .find('>')
            .map_or(svg.len(), |end| start + end + 1);
        (start, &svg[start..end])
    })
}

/// Returns the raw value of attribute `name` in a start tag.
#[must_use]
pub fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// Undoes the renderer's XML escaping.
fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Returns attribute `name` with the renderer's XML escaping undone.
fn text_attr(tag: &str, name: &str) -> Option<String> {
    attr(tag, name).map(unescape)
}

fn classes(tag: &str) -> BTreeSet<String> {
    attr(tag, "class")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn rect(tag: &str) -> SvgRect {
    let number = |name| {
        attr(tag, name)
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("rect without numeric {name}: {tag}"))
    };
    SvgRect {
        x: number("x"),
        y: number("y"),
        width: number("width"),
        height: number("height"),
    }
}

/// Returns the first `<rect class="{class}"` inside `markup`.
fn rect_in(markup: &str, class: &str) -> SvgRect {
    let prefix = format!("<rect class=\"{class}\"");
    let (_, tag) = start_tags(markup, &prefix)
        .next()
        .unwrap_or_else(|| panic!("missing {class} rect in {markup}"));
    rect(tag)
}

/// Yields `(start tag, markup)` for every node, where the markup runs from
/// the start tag up to the next node (or the end of the document), so a
/// query never reads into a neighbouring card.
fn table_nodes(svg: &str) -> impl Iterator<Item = (&str, &str)> {
    let starts = start_tags(svg, "<g class=\"table-node").collect::<Vec<_>>();
    let ends = starts
        .iter()
        .skip(1)
        .map(|(start, _)| *start)
        .chain(std::iter::once(svg.len()))
        .collect::<Vec<_>>();
    starts
        .into_iter()
        .zip(ends)
        .map(move |((start, tag), end)| (tag, &svg[start..end]))
}

/// Returns the markup of the node with `table_id`.
fn table_node_markup<'a>(svg: &'a str, table_id: &str) -> Option<&'a str> {
    table_nodes(svg)
        .find(|(tag, _)| text_attr(tag, "data-table-id").as_deref() == Some(table_id))
        .map(|(_, markup)| markup)
}

/// Returns the IDs of all rendered nodes: tables, views, and enums.
#[must_use]
pub fn node_ids(svg: &str) -> BTreeSet<String> {
    start_tags(svg, "<g class=\"table-node")
        .filter_map(|(_, tag)| text_attr(tag, "data-table-id"))
        .collect()
}

/// Returns the IDs of rendered nodes whose kind is `table`.
#[must_use]
pub fn table_ids(svg: &str) -> BTreeSet<String> {
    start_tags(svg, "<g class=\"table-node")
        .filter(|(_, tag)| attr(tag, "data-node-kind") == Some("table"))
        .filter_map(|(_, tag)| text_attr(tag, "data-table-id"))
        .collect()
}

/// Returns the classes of the table node with `table_id`.
#[must_use]
pub fn table_classes(svg: &str, table_id: &str) -> Option<BTreeSet<String>> {
    table_node_markup(svg, table_id).map(classes)
}

/// Returns the classes of column `column` in table `table_id`.
#[must_use]
pub fn column_classes(svg: &str, table_id: &str, column: &str) -> Option<BTreeSet<String>> {
    let node = table_node_markup(svg, table_id)?;
    start_tags(node, "<g class=\"column-row")
        .find(|(_, tag)| text_attr(tag, "data-column-name").as_deref() == Some(column))
        .map(|(_, tag)| classes(tag))
}

/// Returns the card rectangle of every table node, keyed by table ID.
#[must_use]
pub fn table_bodies(svg: &str) -> BTreeMap<String, SvgRect> {
    table_nodes(svg)
        .filter_map(|(tag, markup)| {
            let id = text_attr(tag, "data-table-id")?;
            Some((id, rect_in(markup, "table-body")))
        })
        .collect()
}

/// Returns the tables enclosed by each group box, keyed by the group label.
#[must_use]
pub fn group_members(svg: &str) -> BTreeMap<String, BTreeSet<String>> {
    let labels: BTreeMap<&str, String> = start_tags(svg, "<text class=\"group-label\"")
        .filter_map(|(start, tag)| {
            let id = attr(tag, "data-group-id")?;
            let text_start = start + tag.len();
            let text_len = svg[text_start..].find("</text>")?;
            Some((id, unescape(&svg[text_start..text_start + text_len])))
        })
        .collect();
    let bodies = table_bodies(svg);

    start_tags(svg, "<g class=\"group\"")
        .map(|(start, tag)| {
            let id = attr(tag, "data-group-id").expect("group without data-group-id");
            let label = labels
                .get(id)
                .unwrap_or_else(|| panic!("group {id} without label"));
            let end = svg[start..]
                .find("</g>")
                .map_or(svg.len(), |end| start + end);
            let area = rect_in(&svg[start..end], "group-box");
            let members = bodies
                .iter()
                .filter(|(_, body)| area.contains(body))
                .map(|(table, _)| table.clone())
                .collect();
            (label.clone(), members)
        })
        .collect()
}

/// Returns every relationship edge with its endpoints and classes.
#[must_use]
pub fn edges(svg: &str) -> Vec<SvgEdge> {
    start_tags(svg, "<g id=\"edge-")
        .filter_map(|(_, tag)| {
            Some(SvgEdge {
                from: text_attr(tag, "data-from")?,
                to: text_attr(tag, "data-to")?,
                classes: classes(tag),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = concat!(
        r#"<g class="group" data-group-id="prefix_0"><rect class="group-box" x="0.0" y="0.0" width="100.0" height="100.0"/></g>"#,
        r#"<g class="table-node node diff-modified" data-table-id="app_users" data-id="app_users" data-node-kind="table">"#,
        r#"<rect class="table-body" x="10.0" y="10.0" width="50.0" height="50.0"/>"#,
        r#"<g class="column-row diff-removed" data-column-name="legacy" data-nullable="true"></g>"#,
        r#"<g class="column-row diff-added" data-column-name="a&amp;b" data-nullable="true"></g></g>"#,
        r#"<g class="table-node node" data-table-id="cats" data-id="cats" data-node-kind="table">"#,
        r#"<rect class="table-body" x="200.0" y="10.0" width="50.0" height="50.0"/></g>"#,
        r#"<text class="group-label" data-group-id="prefix_0" x="1" y="1">app</text>"#,
        r#"<g id="edge-0" class="edge diff-removed" data-from="app_users" data-to="cats"></g>"#,
        r#"<g class="table-node node" data-table-id="enum(&#39;a&#39;)" data-node-kind="enum">"#,
        r#"<rect class="table-body" x="300.0" y="10.0" width="50.0" height="50.0"/></g>"#,
    );

    #[test]
    fn queries_identify_elements_by_their_ids() {
        assert_eq!(
            table_ids(SVG),
            ["app_users", "cats"].map(str::to_string).into()
        );
        assert_eq!(
            node_ids(SVG),
            ["app_users", "cats", "enum('a')"]
                .map(str::to_string)
                .into()
        );
        assert!(
            table_classes(SVG, "app_users")
                .unwrap()
                .contains("diff-modified")
        );
        assert!(
            !table_classes(SVG, "cats")
                .unwrap()
                .contains("diff-modified")
        );
        assert!(
            column_classes(SVG, "app_users", "legacy")
                .unwrap()
                .contains("diff-removed")
        );
        assert!(
            column_classes(SVG, "app_users", "a&b")
                .unwrap()
                .contains("diff-added")
        );
        assert_eq!(column_classes(SVG, "cats", "legacy"), None);
        assert_eq!(
            group_members(SVG),
            BTreeMap::from([("app".to_string(), BTreeSet::from(["app_users".to_string()]))])
        );
        assert_eq!(
            edges(SVG),
            vec![SvgEdge {
                from: "app_users".to_string(),
                to: "cats".to_string(),
                classes: ["edge", "diff-removed"].map(str::to_string).into(),
            }]
        );
    }

    #[test]
    #[should_panic(expected = "missing table-body rect")]
    fn a_node_without_a_card_does_not_borrow_the_next_one() {
        let svg = concat!(
            r#"<g class="table-node node" data-table-id="ghost" data-node-kind="table"></g>"#,
            r#"<g class="table-node node" data-table-id="cats" data-node-kind="table">"#,
            r#"<rect class="table-body" x="0.0" y="0.0" width="50.0" height="50.0"/></g>"#,
        );
        let _ = table_bodies(svg);
    }
}
