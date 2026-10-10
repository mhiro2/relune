//! Legend and statistics rendering for schema diagrams.

use std::fmt::{self, Write};

use relune_core::model::SchemaStats;

use relune_layout::metrics::{COLUMN_BADGE_HEIGHT, COLUMN_NULLABLE_MARKER};

use relune_core::{ChangeKind, ReviewSeverity};

use crate::diff::{render_change_marker, render_risk_pill};
use crate::node::{ColumnBadge, render_column_badge};
use crate::theme::ThemeColors;

/// Width taken by the title, badges, and nullable marker, including padding.
const LEGEND_ITEMS_WIDTH: f32 = 488.0;
/// Width added for the diff change kinds, the risk label, and their divider.
const LEGEND_DIFF_WIDTH: f32 = 392.0;
/// Width added for the statistics block and its divider.
const LEGEND_STATS_WIDTH: f32 = 160.0;

/// Renders a legend block explaining the marks node cards draw.
///
/// The PK, FK, and IX badges and the nullable marker are drawn exactly as on
/// the cards. Diff diagrams add the change kind markers and the risk label;
/// statistics are included if provided.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `theme` - The theme colors to use
/// * `stats` - Optional schema statistics to display
/// * `diff` - Whether to explain the diff change kinds and risk label
/// * `svg_width` - Width of the SVG canvas (for positioning)
/// * `svg_height` - Height of the SVG canvas (for positioning)
#[allow(clippy::too_many_lines)]
pub fn render_legend(
    out: &mut String,
    theme: &ThemeColors,
    stats: Option<&SchemaStats>,
    diff: bool,
    svg_width: f32,
    svg_height: f32,
) -> fmt::Result {
    let bar_height = 42.0;
    let side_padding = 18.0;
    let available_width = svg_width - side_padding * 2.0;
    // The legend never shrinks below its items; on narrower canvases the
    // whole bar is scaled down instead of letting items run off the edge.
    let content_width = LEGEND_ITEMS_WIDTH
        + if diff { LEGEND_DIFF_WIDTH } else { 0.0 }
        + if stats.is_some() {
            LEGEND_STATS_WIDTH
        } else {
            0.0
        };
    let legend_width = available_width.clamp(content_width, content_width.max(640.0));
    let legend_x = (svg_width - legend_width) * 0.5;
    let legend_y = svg_height - bar_height - 18.0;
    let mut cursor_x = legend_x + 16.0;
    let baseline_y = legend_y + 25.0;

    if legend_width > available_width && available_width > 0.0 {
        let scale = available_width / legend_width;
        let (cx, cy) = (svg_width * 0.5, legend_y + bar_height);
        write!(
            out,
            r#"<g class="legend" transform="translate({cx:.1} {cy:.1}) scale({scale:.3}) translate({:.1} {:.1})">"#,
            -cx, -cy
        )?;
    } else {
        out.push_str(r#"<g class="legend">"#);
    }

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

    if diff {
        cursor_x += 88.0;
        render_legend_divider(out, cursor_x, legend_y, bar_height, theme)?;
        cursor_x += 16.0;
        for (kind, label, advance) in [
            (ChangeKind::Added, "Added", 70.0),
            (ChangeKind::Removed, "Removed", 84.0),
            (ChangeKind::Modified, "Modified", 88.0),
        ] {
            render_change_marker(out, cursor_x + 5.0, baseline_y, 12.0, kind, theme)?;
            render_legend_item_label(out, cursor_x + 16.0, baseline_y, label, theme)?;
            cursor_x += advance;
        }
        let pill_width = render_risk_pill(
            out,
            cursor_x,
            baseline_y - 4.0,
            "breaking",
            ReviewSeverity::Breaking,
            theme,
        )?;
        render_legend_item_label(
            out,
            cursor_x + pill_width + 8.0,
            baseline_y,
            "Review risk",
            theme,
        )?;
    }

    if let Some(stats) = stats {
        let stats_x = legend_x + legend_width - LEGEND_STATS_WIDTH + 16.0;
        render_legend_divider(out, stats_x - 16.0, legend_y, bar_height, theme)?;
        write!(
            out,
            r#"<text class="legend-stats" x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="11" fill="{}">{} tables · {} columns · {} FKs</text>"#,
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

fn render_legend_divider(
    out: &mut String,
    x: f32,
    legend_y: f32,
    bar_height: f32,
    theme: &ThemeColors,
) -> fmt::Result {
    write!(
        out,
        r#"<line x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{:.1}" stroke="{}" stroke-opacity="0.72"/>"#,
        legend_y + 10.0,
        legend_y + bar_height - 10.0,
        theme.group_stroke,
    )
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
        render_legend(out, colors, stats, false, width, height)
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
    fn test_render_legend_explains_diff_marks() {
        let colors = test_theme_colors();
        let mut plain = String::new();
        let mut diff = String::new();

        render_legend_ok(&mut plain, &colors, None, 1200.0, 600.0);
        render_legend(&mut diff, &colors, None, true, 1200.0, 600.0).unwrap();

        assert!(!plain.contains("diff-marker"));
        assert!(!plain.contains("Review risk"));
        assert_eq!(diff.matches(r#"class="diff-marker""#).count(), 3);
        for label in ["Added", "Removed", "Modified", "Review risk"] {
            assert!(diff.contains(&format!(">{label}<")), "missing {label}");
        }
        assert!(diff.contains(r#"<g class="risk-label risk-breaking">"#));
        // The diff items widen the bar instead of overflowing it.
        assert!(diff.contains(r#"width="880.0""#));
    }

    #[test]
    fn test_legend_scales_down_on_narrow_canvas() {
        let colors = test_theme_colors();
        let mut wide = String::new();
        let mut narrow = String::new();

        render_legend_ok(&mut wide, &colors, None, 800.0, 600.0);
        render_legend_ok(&mut narrow, &colors, None, 400.0, 600.0);

        assert!(wide.starts_with(r#"<g class="legend">"#));
        assert!(narrow.starts_with(r#"<g class="legend" transform="#));
        assert!(narrow.contains("scale(0.746)"));
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
