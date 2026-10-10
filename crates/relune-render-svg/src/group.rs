//! Group rendering for SVG output.
//!
//! A group is a faint surface with a small heading. Its boundary is drawn
//! only where membership would otherwise be ambiguous: when another group or
//! a card outside the group overlaps its surface.

use std::fmt::{self, Write};

use relune_layout::metrics::{
    GROUP_CORNER_RADIUS, GROUP_LABEL_BASELINE, GROUP_LABEL_FONT_SIZE, GROUP_LABEL_INSET,
    GROUP_LABEL_LETTER_SPACING_EM,
};
use relune_layout::{PositionedGraph, PositionedGroup};

use crate::theme::ThemeColors;

/// Renders a group surface and its label.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `group` - The positioned group to render
/// * `outlined` - Whether to draw the group's boundary
/// * `colors` - The theme colors to use
pub fn render_group(
    out: &mut String,
    group: &PositionedGroup,
    outlined: bool,
    colors: &ThemeColors,
) -> fmt::Result {
    render_group_background(out, group, outlined, colors)?;
    render_group_label(out, group, colors)
}

/// Renders the surface of a group, with its boundary when `outlined`.
pub fn render_group_background(
    out: &mut String,
    group: &PositionedGroup,
    outlined: bool,
    colors: &ThemeColors,
) -> fmt::Result {
    if !has_area(group) {
        return Ok(());
    }

    write!(
        out,
        r#"<g class="group" data-group-id="{}"><rect class="group-box" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{GROUP_CORNER_RADIUS}" ry="{GROUP_CORNER_RADIUS}" fill="{}""#,
        escape_attribute(&group.id),
        group.x,
        group.y,
        group.width,
        group.height,
        colors.group_fill,
    )?;
    if outlined {
        write!(out, r#" stroke="{}" stroke-width="1""#, colors.group_stroke)?;
    }
    out.push_str("/></g>");
    Ok(())
}

/// Renders the foreground label for a group.
pub fn render_group_label(
    out: &mut String,
    group: &PositionedGroup,
    colors: &ThemeColors,
) -> fmt::Result {
    if !has_area(group) || group.label.is_empty() {
        return Ok(());
    }

    write!(
        out,
        r#"<text class="group-label" data-group-id="{}" x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="{GROUP_LABEL_FONT_SIZE}" font-weight="700" letter-spacing="{GROUP_LABEL_LETTER_SPACING_EM}em" fill="{}">{}</text>"#,
        escape_attribute(&group.id),
        group.x + GROUP_LABEL_INSET,
        group.y + GROUP_LABEL_BASELINE,
        colors.text_secondary,
        escape_text(&group.label)
    )
}

/// Whether each group of `graph` needs its boundary drawn: another group or
/// a card outside the group overlaps its surface, so the surface alone would
/// not show which cards belong to it.
#[must_use]
pub fn ambiguous_groups(graph: &PositionedGraph) -> Vec<bool> {
    graph
        .groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            has_area(group)
                && (graph.groups.iter().enumerate().any(|(other_index, other)| {
                    other_index != index && has_area(other) && overlaps(group, rect_of(other))
                }) || graph.nodes.iter().any(|node| {
                    node.group_index != Some(index)
                        && overlaps(group, (node.x, node.y, node.width, node.height))
                }))
        })
        .collect()
}

const fn has_area(group: &PositionedGroup) -> bool {
    group.width > 0.0 && group.height > 0.0
}

const fn rect_of(group: &PositionedGroup) -> (f32, f32, f32, f32) {
    (group.x, group.y, group.width, group.height)
}

/// Whether `group` and the `(x, y, width, height)` rectangle share any area.
fn overlaps(group: &PositionedGroup, (x, y, width, height): (f32, f32, f32, f32)) -> bool {
    group.x < x + width
        && x < group.x + group.width
        && group.y < y + height
        && y < group.y + group.height
}

use crate::escape::{escape_attribute, escape_text};

#[cfg(test)]
mod tests {
    use super::*;

    fn render_group_ok(out: &mut String, group: &PositionedGroup, colors: &ThemeColors) {
        render_group(out, group, false, colors).expect("group rendering should succeed in tests");
    }

    fn render_group_background_ok(out: &mut String, group: &PositionedGroup, colors: &ThemeColors) {
        render_group_background(out, group, false, colors)
            .expect("group background rendering should succeed in tests");
    }

    fn render_group_label_ok(out: &mut String, group: &PositionedGroup, colors: &ThemeColors) {
        render_group_label(out, group, colors)
            .expect("group label rendering should succeed in tests");
    }

    fn test_colors() -> ThemeColors {
        crate::theme::get_colors(crate::theme::Theme::Dark)
    }

    #[test]
    fn test_render_group_basic() {
        let group = PositionedGroup {
            id: "group1".to_string(),
            label: "User Tables".to_string(),
            x: 50.0,
            y: 50.0,
            width: 300.0,
            height: 200.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_ok(&mut out, &group, &colors);

        assert!(out.contains("class=\"group-box\""));
        assert!(out.contains("data-group-id=\"group1\""));
        assert!(out.contains("class=\"group-label\""));
        assert!(out.contains(">User Tables<"));
        // A plain surface: no frame, band, divider, or shadow.
        assert!(!out.contains("stroke="));
        assert!(!out.contains("stroke-dasharray"));
        assert!(!out.contains("group-band"));
        assert!(!out.contains("filter="));
    }

    #[test]
    fn test_render_group_outlined_draws_a_solid_boundary() {
        let group = PositionedGroup {
            id: "outlined".to_string(),
            label: "Outlined".to_string(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_background(&mut out, &group, true, &colors).unwrap();

        assert!(out.contains(&format!(
            r#"stroke="{}" stroke-width="1""#,
            colors.group_stroke
        )));
        assert!(!out.contains("stroke-dasharray"));
    }

    fn group_at(id: &str, x: f32, y: f32, width: f32, height: f32) -> PositionedGroup {
        PositionedGroup {
            id: id.to_string(),
            label: id.to_string(),
            x,
            y,
            width,
            height,
        }
    }

    fn node_at(id: &str, x: f32, group_index: Option<usize>) -> relune_layout::PositionedNode {
        relune_layout::PositionedNode {
            id: id.to_string(),
            label: id.to_string(),
            kind: relune_core::NodeKind::Table,
            columns: Vec::new(),
            omitted_columns: 0,
            x,
            y: 40.0,
            width: 80.0,
            height: 40.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index,
        }
    }

    fn graph_of(
        groups: Vec<PositionedGroup>,
        nodes: Vec<relune_layout::PositionedNode>,
    ) -> PositionedGraph {
        PositionedGraph {
            nodes,
            edges: Vec::new(),
            groups,
            width: 1000.0,
            height: 1000.0,
            routing_debug: None,
        }
    }

    #[test]
    fn test_separate_groups_are_not_outlined() {
        let graph = graph_of(
            vec![
                group_at("a", 0.0, 0.0, 200.0, 120.0),
                group_at("b", 300.0, 0.0, 200.0, 120.0),
            ],
            vec![node_at("a1", 20.0, Some(0)), node_at("b1", 320.0, Some(1))],
        );

        assert_eq!(ambiguous_groups(&graph), [false, false]);
    }

    #[test]
    fn test_overlapping_groups_are_outlined() {
        let graph = graph_of(
            vec![
                group_at("a", 0.0, 0.0, 200.0, 120.0),
                group_at("b", 150.0, 0.0, 200.0, 120.0),
                group_at("c", 600.0, 0.0, 200.0, 120.0),
            ],
            vec![],
        );

        assert_eq!(ambiguous_groups(&graph), [true, true, false]);
    }

    #[test]
    fn test_group_overlapping_an_outside_card_is_outlined() {
        let graph = graph_of(
            vec![
                group_at("a", 0.0, 0.0, 200.0, 120.0),
                group_at("b", 300.0, 0.0, 200.0, 120.0),
            ],
            // `loose` is in no group but lies on group `a`'s surface.
            vec![node_at("a1", 20.0, Some(0)), node_at("loose", 100.0, None)],
        );

        assert_eq!(ambiguous_groups(&graph), [true, false]);
    }

    #[test]
    fn test_render_group_empty_label() {
        let group = PositionedGroup {
            id: "empty_label".to_string(),
            label: String::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_ok(&mut out, &group, &colors);

        assert!(out.contains("class=\"group-box\""));
        assert!(!out.contains("class=\"group-label\""));
    }

    #[test]
    fn test_render_group_background_omits_label() {
        let group = PositionedGroup {
            id: "background_only".to_string(),
            label: "Background Only".to_string(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_background_ok(&mut out, &group, &colors);

        assert!(out.contains("class=\"group-box\""));
        assert!(!out.contains("class=\"group-label\""));
    }

    #[test]
    fn test_render_group_label_only_emits_text() {
        let group = PositionedGroup {
            id: "label_only".to_string(),
            label: "Label Only".to_string(),
            x: 10.0,
            y: 10.0,
            width: 120.0,
            height: 80.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_label_ok(&mut out, &group, &colors);

        assert!(out.contains(r#"class="group-label" data-group-id="label_only""#));
        assert!(!out.contains("class=\"group-box\""));
    }

    #[test]
    fn test_render_group_zero_dimensions() {
        let group = PositionedGroup {
            id: "zero".to_string(),
            label: "Zero".to_string(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_ok(&mut out, &group, &colors);

        // Should not render anything
        assert!(out.is_empty());
    }

    #[test]
    fn test_render_group_escapes_special_characters() {
        let group = PositionedGroup {
            id: "test & <group>".to_string(),
            label: "Label & <test>".to_string(),
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_ok(&mut out, &group, &colors);

        // Should contain escaped characters
        assert!(out.contains("&amp;"));
        assert!(out.contains("&lt;"));
        assert!(out.contains("&gt;"));
        // Should not contain raw special characters (except in SVG syntax)
        assert!(!out.contains("test & <group>"));
        assert!(!out.contains("Label & <test>"));
    }

    #[test]
    fn test_render_group_rounded_corners() {
        let group = PositionedGroup {
            id: "rounded".to_string(),
            label: "Rounded".to_string(),
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
        };

        let colors = test_colors();
        let mut out = String::new();
        render_group_ok(&mut out, &group, &colors);

        assert!(out.contains("rx=\"10\""));
        assert!(out.contains("ry=\"10\""));
    }
}
