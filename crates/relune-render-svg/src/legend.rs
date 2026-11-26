//! Legend rendering for schema diagrams.

use std::fmt::{self, Write};

use relune_layout::metrics::{COLUMN_BADGE_HEIGHT, COLUMN_NULLABLE_MARKER};

use relune_core::{ChangeKind, ReviewSeverity};

use crate::diff::{render_change_marker, render_risk_pill};
use crate::node::{ColumnBadge, render_column_badge};
use crate::theme::ThemeColors;

/// Height of the legend bar before any scaling.
const LEGEND_BAR_HEIGHT: f32 = 42.0;
/// Space kept between the legend and the canvas sides and bottom.
const LEGEND_MARGIN: f32 = 18.0;
/// Width taken by the title, badges, and nullable marker, including padding.
const LEGEND_ITEMS_WIDTH: f32 = 488.0;
/// Width added for the diff change kinds, the risk label, and their divider.
const LEGEND_DIFF_WIDTH: f32 = 392.0;

/// Horizontal placement of the legend bar on a canvas.
struct LegendFrame {
    x: f32,
    width: f32,
    /// Factor applied to the whole bar when the canvas is narrower than its items.
    scale: f32,
}

impl LegendFrame {
    fn new(svg_width: f32, diff: bool) -> Self {
        let available_width = LEGEND_MARGIN.mul_add(-2.0, svg_width);
        // The legend never shrinks below its items; on narrower canvases the
        // whole bar is scaled down instead of letting items run off the edge.
        let content_width = LEGEND_ITEMS_WIDTH + if diff { LEGEND_DIFF_WIDTH } else { 0.0 };
        let width = available_width.clamp(content_width, content_width.max(640.0));
        let scale = if width > available_width && available_width > 0.0 {
            available_width / width
        } else {
            1.0
        };
        Self {
            x: (svg_width - width) * 0.5,
            width,
            scale,
        }
    }
}

/// Returns the canvas height the legend needs below the graph.
///
/// The legend sits in a footer of this height under the graph bounds, so it
/// never covers a card, line, or label. A legend scaled down for a narrow
/// canvas needs a correspondingly shorter footer.
#[must_use]
pub fn legend_footer_height(svg_width: f32, diff: bool) -> f32 {
    let frame = LegendFrame::new(svg_width, diff);
    LEGEND_BAR_HEIGHT.mul_add(frame.scale, LEGEND_MARGIN)
}

/// Renders a legend block explaining the marks node cards draw.
///
/// The PK, FK, and IX badges and the nullable marker are drawn exactly as on
/// the cards. Diff diagrams add the change kind markers and the risk label.
/// The bar is drawn from `top` down;
/// reserve [`legend_footer_height`] below the graph for it.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `theme` - The theme colors to use
/// * `diff` - Whether to explain the diff change kinds and risk label
/// * `svg_width` - Width of the SVG canvas (for positioning)
/// * `top` - Y coordinate of the legend's top edge, usually the graph height
#[allow(clippy::too_many_lines)]
pub fn render_legend(
    out: &mut String,
    theme: &ThemeColors,
    diff: bool,
    svg_width: f32,
    top: f32,
) -> fmt::Result {
    let bar_height = LEGEND_BAR_HEIGHT;
    let LegendFrame {
        x: legend_x,
        width: legend_width,
        scale,
    } = LegendFrame::new(svg_width, diff);
    let legend_y = top;
    let mut cursor_x = legend_x + 16.0;
    let baseline_y = legend_y + 25.0;

    if scale < 1.0 {
        // Scale about the top centre so the bar stays inside its footer.
        let (cx, cy) = (svg_width * 0.5, legend_y);
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

    fn render_legend_ok(out: &mut String, colors: &ThemeColors, width: f32, height: f32) {
        render_legend(out, colors, false, width, height)
            .expect("legend rendering should succeed in tests");
    }

    fn test_theme_colors() -> ThemeColors {
        crate::theme::get_colors(crate::theme::Theme::Dark)
    }

    #[test]
    fn test_render_legend_explains_card_marks() {
        let colors = test_theme_colors();
        let mut out = String::new();

        render_legend_ok(&mut out, &colors, 800.0, 600.0);

        assert!(out.contains("class=\"legend\""));
        assert!(out.contains("class=\"legend-background\""));
        assert!(out.contains("LEGEND"));
        assert!(out.contains("Primary key"));
        assert!(out.contains("Foreign key"));
        assert!(out.contains("Indexed"));
        assert!(out.contains("Nullable"));
        assert_eq!(out.matches(r#"class="col-badge""#).count(), 3);
    }

    #[test]
    fn test_render_legend_explains_diff_marks() {
        let colors = test_theme_colors();
        let mut plain = String::new();
        let mut diff = String::new();

        render_legend_ok(&mut plain, &colors, 1200.0, 600.0);
        render_legend(&mut diff, &colors, true, 1200.0, 600.0).unwrap();

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

        render_legend_ok(&mut wide, &colors, 800.0, 600.0);
        render_legend_ok(&mut narrow, &colors, 400.0, 600.0);

        assert!(wide.starts_with(r#"<g class="legend">"#));
        assert!(narrow.starts_with(r#"<g class="legend" transform="#));
        assert!(narrow.contains("scale(0.746)"));
    }

    #[test]
    fn test_legend_positioning() {
        let colors = test_theme_colors();
        let mut out = String::new();

        render_legend_ok(&mut out, &colors, 800.0, 600.0);

        // The bar is centered horizontally and starts at the given top edge.
        assert!(out.contains(r#"class="legend-background" x="80.0" y="600.0""#));
    }

    #[test]
    fn test_legend_footer_fits_the_scaled_bar() {
        // A full-size bar plus the bottom margin.
        assert!((legend_footer_height(800.0, false) - 60.0).abs() < 0.01);
        // A bar scaled to 0.746 needs a shorter footer.
        assert!((legend_footer_height(400.0, false) - 42.0f32.mul_add(0.746, 18.0)).abs() < 0.05);
        // Diff marks widen the bar, so the same canvas scales it further.
        assert!(legend_footer_height(400.0, true) < legend_footer_height(400.0, false));
    }

    #[test]
    fn test_scaled_legend_stays_below_its_top() {
        let colors = test_theme_colors();
        let mut out = String::new();

        render_legend_ok(&mut out, &colors, 400.0, 600.0);

        // Scaling about the top centre keeps the bar from rising into the graph.
        assert!(out.starts_with(
            r#"<g class="legend" transform="translate(200.0 600.0) scale(0.746) translate(-200.0 -600.0)">"#
        ));
    }
}
