//! Group rendering for SVG output.

use std::fmt::{self, Write};

use relune_layout::PositionedGroup;

use crate::theme::ThemeColors;

/// Renders a group as a rounded rectangle with a label.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `group` - The positioned group to render
/// * `colors` - The theme colors to use
pub fn render_group(
    out: &mut String,
    group: &PositionedGroup,
    colors: &ThemeColors,
) -> fmt::Result {
    render_group_background(out, group, colors)?;
    render_group_label(out, group, colors)
}

/// Renders the background shell for a group.
pub fn render_group_background(
    out: &mut String,
    group: &PositionedGroup,
    colors: &ThemeColors,
) -> fmt::Result {
    // Skip rendering if group has zero dimensions
    if group.width <= 0.0 || group.height <= 0.0 {
        return Ok(());
    }

    // Render the group box with a distinct background plane and accent band.
    write!(
        out,
        r#"<g class="group" data-group-id="{}"><rect class="group-box" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="20" ry="20" fill="{}" stroke="{}" stroke-width="1.5" stroke-dasharray="10,5" filter="url(#group-shadow)"/><rect class="group-band" x="{:.1}" y="{:.1}" width="{:.1}" height="34" rx="20" ry="20" fill="{}" fill-opacity="0.92"/><line class="group-divider" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-opacity="0.72"/></g>"#,
        escape_attribute(&group.id),
        group.x,
        group.y,
        group.width,
        group.height,
        colors.group_fill,
        colors.group_stroke,
        group.x,
        group.y,
        group.width,
        colors.group_band_fill,
        group.x + 12.0,
        group.y + 34.0,
        group.x + group.width - 12.0,
        group.y + 34.0,
        colors.group_stroke
    )?;

    Ok(())
}

/// Renders the foreground label for a group.
pub fn render_group_label(
    out: &mut String,
    group: &PositionedGroup,
    colors: &ThemeColors,
) -> fmt::Result {
    if group.width <= 0.0 || group.height <= 0.0 {
        return Ok(());
    }

    // Render the group label at top-left inside the group.
    if !group.label.is_empty() {
        write!(
            out,
            r#"<text class="group-label" x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="11" font-weight="700" letter-spacing="0.12em" fill="{}">{}</text>"#,
            group.x + 12.0,
            group.y + 22.0,
            colors.text_secondary,
            escape_text(&group.label)
        )?;
    }
    Ok(())
}

use crate::escape::{escape_attribute, escape_text};

#[cfg(test)]
mod tests {
    use super::*;

    fn render_group_ok(out: &mut String, group: &PositionedGroup, colors: &ThemeColors) {
        render_group(out, group, colors).expect("group rendering should succeed in tests");
    }

    fn render_group_background_ok(out: &mut String, group: &PositionedGroup, colors: &ThemeColors) {
        render_group_background(out, group, colors)
            .expect("group background rendering should succeed in tests");
    }

    fn render_group_label_ok(out: &mut String, group: &PositionedGroup, colors: &ThemeColors) {
        render_group_label(out, group, colors)
            .expect("group label rendering should succeed in tests");
    }

    fn test_colors() -> ThemeColors {
        ThemeColors {
            background: "#0c0f1a",
            canvas_base: "#0c0f1a",
            canvas_dot: "#151928",
            foreground: "#e2e8f0",
            node_fill: "#111827",
            node_stroke: "#334155",
            header_fill: "#1e293b",
            text_primary: "#e2e8f0",
            text_secondary: "#cbd5e1",
            text_muted: "#94a3b8",
            edge_stroke: "#64748b",
            arrow_fill: "#64748b",
            node_shadow: "rgba(0, 0, 0, 0.5)",
            group_fill: "#0f172acc",
            group_band_fill: "#172036",
            group_stroke: "#334155",
            glow_color: "#f59e0b",
            glow_particle: "#fbbf24",
            is_light: false,
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
        render_group_ok(&mut out, &group, &colors);

        assert!(out.contains("class=\"group-box\""));
        assert!(out.contains("data-group-id=\"group1\""));
        assert!(out.contains("class=\"group-band\""));
        assert!(out.contains("stroke-dasharray=\"10,5\""));
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

        assert!(out.contains("class=\"group-label\""));
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

        assert!(out.contains("rx=\"20\""));
        assert!(out.contains("ry=\"20\""));
    }
}
