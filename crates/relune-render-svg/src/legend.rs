//! Legend and statistics rendering for schema diagrams.

use std::fmt::{self, Write};

use relune_core::model::SchemaStats;

use crate::theme::ThemeColors;

// Legend indicator colors (matching node cards)
const PK_STROKE: &str = "#fbbf24";
const FK_COLOR: &str = "#3b82f6";
const IDX_COLOR: &str = "#f59e0b";
const NULLABLE_TEXT: &str = "#64748b";

/// Renders a legend block showing PK, FK, IDX indicators and nullable style.
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
    render_legend_pk_indicator(out, cursor_x, baseline_y - 8.0)?;
    cursor_x += 16.0;
    render_legend_item_label(out, cursor_x, baseline_y, "Primary key", theme)?;
    cursor_x += 82.0;
    render_legend_fk_indicator(out, cursor_x, baseline_y - 7.0)?;
    cursor_x += 22.0;
    render_legend_item_label(out, cursor_x, baseline_y, "Foreign key", theme)?;
    cursor_x += 84.0;
    render_legend_idx_indicator(out, cursor_x, baseline_y - 8.0)?;
    cursor_x += 16.0;
    render_legend_item_label(out, cursor_x, baseline_y, "Indexed", theme)?;
    cursor_x += 62.0;
    write!(
        out,
        r#"<text class="legend-nullable" x="{cursor_x:.1}" y="{baseline_y:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="11" font-style="italic" fill="{NULLABLE_TEXT}">nullable</text>"#
    )?;

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

/// Renders a small key icon for the legend.
fn render_legend_pk_indicator(out: &mut String, x: f32, y: f32) -> fmt::Result {
    write!(
        out,
        r#"<path class="legend-pk-indicator" d="M{:.1} {:.1}a3.2 3.2 0 1 0 0.01 0M{:.1} {:.1}h7m-2.4 0v2.1m-2.2 -2.1v3.4" fill="none" stroke="{PK_STROKE}" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,
        x,
        y + 3.2,
        x + 3.2,
        y + 3.2
    )
}

/// Renders a small link indicator for the legend.
fn render_legend_fk_indicator(out: &mut String, x: f32, y: f32) -> fmt::Result {
    write!(
        out,
        r#"<path class="legend-fk-indicator" d="M{:.1} {:.1}c0 -1.9 1.5 -3.4 3.4 -3.4h2.5c1.9 0 3.4 1.5 3.4 3.4s-1.5 3.4 -3.4 3.4h-2.5c-1.9 0 -3.4 1.5 -3.4 3.4s1.5 3.4 3.4 3.4h2.5c1.9 0 3.4 -1.5 3.4 -3.4" stroke="{FK_COLOR}" stroke-width="1.5" fill="none" stroke-linecap="round"/>"#,
        x,
        y + 3.4
    )
}

/// Renders a small bolt indicator for the legend.
fn render_legend_idx_indicator(out: &mut String, x: f32, y: f32) -> fmt::Result {
    write!(
        out,
        r#"<path class="legend-idx-indicator" d="M{x:.1} {y:.1}h4.2l-2.2 4.4h3.8l-6.4 7.2 2.1-5h-3.5z" fill="{IDX_COLOR}"/>"#
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
        render_legend(out, colors, stats, width, height)
            .expect("legend rendering should succeed in tests");
    }

    fn test_theme_colors() -> ThemeColors {
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
        assert!(out.contains("nullable"));
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
