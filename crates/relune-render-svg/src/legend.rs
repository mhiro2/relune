//! Legend and statistics rendering for schema diagrams.

use std::fmt::Write;

use relune_core::model::SchemaStats;

use crate::theme::ThemeColors;

// Legend indicator colors (matching node.rs)
const PK_BG: &str = "#22c55e";
const PK_TEXT: &str = "#ffffff";
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
) {
    // Calculate legend dimensions
    let legend_width = 180.0;
    let legend_height = if stats.is_some() { 160.0 } else { 100.0 };
    let padding = 16.0;
    let line_height = 20.0;

    // Position in bottom-right corner with some margin
    let legend_x = svg_width - legend_width - padding;
    let legend_y = svg_height - legend_height - padding;

    // Legend container
    out.push_str(r#"<g class="legend">"#);

    // Legend background
    let _ = write!(
        out,
        r#"<rect class="legend-background" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="8" fill="{}" stroke="{}" stroke-width="1"/>"#,
        legend_x, legend_y, legend_width, legend_height, theme.node_fill, theme.node_stroke
    );

    // Legend title
    let _ = write!(
        out,
        r#"<text class="legend-title" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="12" font-weight="600" fill="{}">Legend</text>"#,
        legend_x + 12.0,
        legend_y + 20.0,
        theme.text_primary
    );

    // Separator line
    let _ = write!(
        out,
        r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="1"/>"#,
        legend_x + 12.0,
        legend_y + 28.0,
        legend_x + legend_width - 12.0,
        legend_y + 28.0,
        theme.node_stroke
    );

    let mut current_y = legend_y + 44.0;

    // PK indicator
    render_legend_pk_indicator(out, legend_x + 12.0, current_y - 4.0, theme);
    let _ = write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="11" fill="{}">Primary Key</text>"#,
        legend_x + 44.0,
        current_y,
        theme.text_secondary
    );
    current_y += line_height;

    // FK indicator
    render_legend_fk_indicator(out, legend_x + 12.0, current_y - 4.0);
    let _ = write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="11" fill="{}">Foreign Key</text>"#,
        legend_x + 44.0,
        current_y,
        theme.text_secondary
    );
    current_y += line_height;

    // IDX indicator
    let _ = write!(
        out,
        r#"<text class="legend-idx" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="9" font-weight="600" fill="{}">IDX</text>"#,
        legend_x + 16.0,
        current_y,
        IDX_COLOR
    );
    let _ = write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="11" fill="{}">Indexed</text>"#,
        legend_x + 44.0,
        current_y,
        theme.text_secondary
    );
    current_y += line_height;

    // Nullable indicator
    let _ = write!(
        out,
        r#"<text class="legend-nullable" x="{:.1}" y="{:.1}" font-family="ui-monospace, monospace" font-size="11" font-style="italic" fill="{}">nullable</text>"#,
        legend_x + 12.0,
        current_y,
        NULLABLE_TEXT
    );

    // Statistics section (if provided)
    if let Some(stats) = stats {
        current_y += line_height + 4.0;

        // Separator line
        let _ = write!(
            out,
            r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="1"/>"#,
            legend_x + 12.0,
            current_y - 8.0,
            legend_x + legend_width - 12.0,
            current_y - 8.0,
            theme.node_stroke
        );

        // Stats title
        let _ = write!(
            out,
            r#"<text class="legend-stats-title" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="11" font-weight="600" fill="{}">Statistics</text>"#,
            legend_x + 12.0,
            current_y,
            theme.text_primary
        );
        current_y += line_height;

        // Table count
        let _ = write!(
            out,
            r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="10" fill="{}">Tables: {}</text>"#,
            legend_x + 12.0,
            current_y,
            theme.text_muted,
            stats.table_count
        );
        current_y += 14.0;

        // Column count
        let _ = write!(
            out,
            r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="10" fill="{}">Columns: {}</text>"#,
            legend_x + 12.0,
            current_y,
            theme.text_muted,
            stats.column_count
        );
        current_y += 14.0;

        // Foreign key count
        let _ = write!(
            out,
            r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="10" fill="{}">Foreign Keys: {}</text>"#,
            legend_x + 12.0,
            current_y,
            theme.text_muted,
            stats.foreign_key_count
        );
    }

    out.push_str("</g>");
}

/// Renders a small PK badge for the legend.
fn render_legend_pk_indicator(out: &mut String, x: f32, y: f32, _theme: &ThemeColors) {
    let _ = write!(
        out,
        r#"<rect class="legend-pk-badge" x="{x:.1}" y="{y:.1}" width="24" height="14" rx="4" fill="{PK_BG}"/>"#
    );
    let _ = write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="9" font-weight="700" fill="{PK_TEXT}">PK</text>"#,
        x + 4.0,
        y + 10.0
    );
}

/// Renders a small FK indicator for the legend.
fn render_legend_fk_indicator(out: &mut String, x: f32, y: f32) {
    let _ = write!(
        out,
        r#"<path class="legend-fk-indicator" d="M{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1} M{:.1} {:.1} L{:.1} {:.1}" stroke="{FK_COLOR}" stroke-width="1.5" fill="none"/>"#,
        x,
        y + 7.0,
        x + 18.0,
        y + 7.0,
        x + 14.0,
        y + 3.0,
        x + 18.0,
        y + 7.0,
        x + 14.0,
        y + 11.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme_colors() -> ThemeColors {
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
    fn test_render_legend_without_stats() {
        let colors = test_theme_colors();
        let mut out = String::new();

        render_legend(&mut out, &colors, None, 800.0, 600.0);

        assert!(out.contains("class=\"legend\""));
        assert!(out.contains("class=\"legend-background\""));
        assert!(out.contains("Legend"));
        assert!(out.contains("Primary Key"));
        assert!(out.contains("Foreign Key"));
        assert!(out.contains("Indexed"));
        assert!(out.contains("nullable"));
        assert!(!out.contains("Statistics"));
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

        render_legend(&mut out, &colors, Some(&stats), 800.0, 600.0);

        assert!(out.contains("class=\"legend\""));
        assert!(out.contains("Statistics"));
        assert!(out.contains("Tables: 5"));
        assert!(out.contains("Columns: 42"));
        assert!(out.contains("Foreign Keys: 8"));
    }

    #[test]
    fn test_legend_positioning() {
        let colors = test_theme_colors();
        let mut out = String::new();

        render_legend(&mut out, &colors, None, 800.0, 600.0);

        // Should be positioned in bottom-right corner
        // legend_x = svg_width - legend_width - padding = 800 - 180 - 16 = 604
        // legend_y = svg_height - legend_height - padding = 600 - 100 - 16 = 484
        assert!(out.contains("x=\"604.0\""));
        assert!(out.contains("y=\"484.0\""));
    }
}
