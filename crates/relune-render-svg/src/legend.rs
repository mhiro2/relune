//! Legend and statistics rendering for schema diagrams.

use std::fmt::{self, Write};

use relune_core::model::SchemaStats;

use relune_layout::metrics::{COLUMN_BADGE_HEIGHT, COLUMN_NULLABLE_MARKER};

use crate::node::{ColumnBadge, render_column_badge};
use crate::theme::ThemeColors;

/// Renders a legend block showing the PK, FK, and IX badges and the nullable
/// marker exactly as node cards draw them.
/// Optionally includes statistics if provided.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `theme` - The theme colors to use
/// * `stats` - Optional schema statistics to display
/// * `svg_width` - Width of the SVG canvas (for positioning)
/// * `svg_height` - Height of the SVG canvas (for positioning)
#[allow(clippy::too_many_lines)]
pub fn render_legend(
    out: &mut String,
    theme: &ThemeColors,
    stats: Option<&SchemaStats>,
    svg_width: f32,
    svg_height: f32,
) -> fmt::Result {
    let bar_height = 42.0;
    let side_padding = 18.0;
    let legend_width = (svg_width - side_padding * 2.0).clamp(320.0, 640.0);
    let legend_x = (svg_width - legend_width) * 0.5;
    let legend_y = svg_height - bar_height - 18.0;
    let mut cursor_x = legend_x + 16.0;
    let baseline_y = legend_y + 25.0;

    out.push_str(r#"<g class="legend">"#);

    write!(
        out,
        r#"<rect class="legend-background" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="18" fill="{}" fill-opacity="0.94" stroke="{}" stroke-opacity="0.82" stroke-width="1.1"/>"#,
        legend_x, legend_y, legend_width, bar_height, theme.group_fill, theme.group_stroke
    )?;
    write!(
        out,
        r#"<text class="legend-title" x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="11" font-weight="700" letter-spacing="0.12em" fill="{}">LEGEND</text>"#,
        cursor_x, baseline_y, theme.text_primary
    )?;
    cursor_x += 84.0;
    let badge_y = baseline_y - COLUMN_BADGE_HEIGHT + 3.5;
    for (badge, label, advance) in [
        (ColumnBadge::PrimaryKey, "Primary key", 104.0),
        (ColumnBadge::ForeignKey, "Foreign key", 104.0),
        (ColumnBadge::Index, "Indexed", 84.0),
    ] {
        render_column_badge(out, cursor_x, badge_y, badge, theme)?;
        render_legend_item_label(out, cursor_x + 24.0, baseline_y, label, theme)?;
        cursor_x += advance;
    }
    write!(
        out,
        r#"<text class="legend-nullable" x="{:.1}" y="{baseline_y:.1}" text-anchor="middle" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="12" font-weight="700" fill="{}">{COLUMN_NULLABLE_MARKER}</text>"#,
        cursor_x + 9.0,
        theme.text_muted
    )?;
    render_legend_item_label(out, cursor_x + 24.0, baseline_y, "Nullable", theme)?;

    if let Some(stats) = stats {
        let stats_x = legend_x + legend_width - 144.0;
        write!(
            out,
            r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-opacity="0.72"/><text class="legend-stats" x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="11" fill="{}">{} tables · {} columns · {} FKs</text>"#,
            stats_x - 16.0,
            legend_y + 10.0,
            stats_x - 16.0,
            legend_y + bar_height - 10.0,
            theme.group_stroke,
            stats_x,
            baseline_y,
            theme.text_secondary,
            stats.table_count,
            stats.column_count,
            stats.foreign_key_count
        )?;
    }

    out.push_str("</g>");
    Ok(())
}

fn render_legend_item_label(
    out: &mut String,
    x: f32,
    y: f32,
    label: &str,
    theme: &ThemeColors,
) -> fmt::Result {
    write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="11" fill="{}">{}</text>"#,
        x, y, theme.text_secondary, label
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_legend_ok(
        out: &mut String,
        colors: &ThemeColors,
        stats: Option<&SchemaStats>,
        width: f32,
        height: f32,
    ) {
        render_legend(out, colors, stats, width, height)
            .expect("legend rendering should succeed in tests");
    }

    fn test_theme_colors() -> ThemeColors {
        crate::theme::get_colors(crate::theme::Theme::Dark)
    }

    #[test]
    fn test_render_legend_without_stats() {
        let colors = test_theme_colors();
        let mut out = String::new();

        render_legend_ok(&mut out, &colors, None, 800.0, 600.0);

        assert!(out.contains("class=\"legend\""));
        assert!(out.contains("class=\"legend-background\""));
        assert!(out.contains("LEGEND"));
        assert!(out.contains("Primary key"));
        assert!(out.contains("Foreign key"));
        assert!(out.contains("Indexed"));
        assert!(out.contains("Nullable"));
        assert_eq!(out.matches(r#"class="col-badge""#).count(), 3);
        assert!(!out.contains("tables ·"));
    }

    #[test]
    fn test_render_legend_with_stats() {
        let colors = test_theme_colors();
        let stats = SchemaStats {
            table_count: 5,
            column_count: 42,
            foreign_key_count: 8,
            view_count: 0,
        };
        let mut out = String::new();

        render_legend_ok(&mut out, &colors, Some(&stats), 800.0, 600.0);

        assert!(out.contains("class=\"legend\""));
        assert!(out.contains("5 tables · 42 columns · 8 FKs"));
    }

    #[test]
    fn test_legend_positioning() {
        let colors = test_theme_colors();
        let mut out = String::new();

        render_legend_ok(&mut out, &colors, None, 800.0, 600.0);

        // Bottom bar layout centers the legend horizontally near the canvas edge.
        assert!(out.contains("x=\"80.0\""));
        assert!(out.contains("y=\"540.0\""));
    }
}
