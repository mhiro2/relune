//! Group rendering for SVG output.

use std::fmt::Write;

use relune_layout::PositionedGroup;

use crate::theme::ThemeColors;

/// Renders a group as a rounded rectangle with a label.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `group` - The positioned group to render
/// * `colors` - The theme colors to use
pub fn render_group(out: &mut String, group: &PositionedGroup, colors: &ThemeColors) {
    // Skip rendering if group has zero dimensions
    if group.width <= 0.0 || group.height <= 0.0 {
        return;
    }

    // Render the group box with dashed stroke and semi-transparent fill
    let _ = write!(
        out,
        r#"<rect class="group-box" data-group-id="{}" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="12" ry="12" fill="{}" fill-opacity="0.3" stroke="{}" stroke-width="1.5" stroke-dasharray="8,4"/>"#,
        escape_attribute(&group.id),
        group.x,
        group.y,
        group.width,
        group.height,
        colors.node_fill,
        colors.node_stroke
    );

    // Render the group label at top-left inside the group
    if !group.label.is_empty() {
        let _ = write!(
            out,
            r#"<text class="group-label" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="12" font-weight="600" fill="{}">{}</text>"#,
            group.x + 12.0,
            group.y + 20.0,
            colors.text_secondary,
            escape_text(&group.label)
        );
    }
}

use crate::escape::{escape_attribute, escape_text};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_colors() -> ThemeColors {
        ThemeColors {
            background: "#0f172a",
            foreground: "#e2e8f0",
            node_fill: "#111827",
            node_stroke: "#334155",
            header_fill: "#1e293b",
            text_primary: "#e2e8f0",
            text_secondary: "#cbd5e1",
            text_muted: "#94a3b8",
            edge_stroke: "#64748b",
            arrow_fill: "#64748b",
        }
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
        render_group(&mut out, &group, &colors);

        assert!(out.contains("class=\"group-box\""));
        assert!(out.contains("data-group-id=\"group1\""));
        assert!(out.contains("stroke-dasharray=\"8,4\""));
        assert!(out.contains("fill-opacity=\"0.3\""));
        assert!(out.contains("class=\"group-label\""));
        assert!(out.contains(">User Tables<"));
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
        render_group(&mut out, &group, &colors);

        assert!(out.contains("class=\"group-box\""));
        assert!(!out.contains("class=\"group-label\""));
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
        render_group(&mut out, &group, &colors);

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
        render_group(&mut out, &group, &colors);

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
        render_group(&mut out, &group, &colors);

        assert!(out.contains("rx=\"12\""));
        assert!(out.contains("ry=\"12\""));
    }
}
