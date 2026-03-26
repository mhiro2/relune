//! Enhanced node rendering with CSS classes and visual indicators.

use std::fmt::Write;

use relune_core::NodeKind;
use relune_layout::PositionedNode;

use crate::geometry::compute_column_y;
use crate::theme::{Theme, get_colors};

/// PK badge background color (green)
const PK_BG: &str = "#22c55e";
/// PK badge text color (white)
const PK_TEXT: &str = "#ffffff";
/// FK indicator color (blue)
const FK_COLOR: &str = "#3b82f6";
/// IDX indicator color (amber)
const IDX_COLOR: &str = "#f59e0b";
/// Nullable text color (muted)
const NULLABLE_TEXT: &str = "#64748b";

/// Options for enhanced node rendering.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct NodeRenderOptions {
    /// The height of each column row in pixels.
    pub column_height: f32,
    /// Whether to show primary key indicators.
    pub show_pk: bool,
    /// Whether to show foreign key indicators.
    pub show_fk: bool,
    /// Whether to show index indicators.
    pub show_idx: bool,
    /// Whether to style nullable columns differently.
    pub style_nullable: bool,
    /// The theme to use for rendering.
    pub theme: Theme,
    /// Whether to show tooltips on hover.
    pub show_tooltips: bool,
    /// Optional foreign key count for tooltip display.
    pub foreign_key_count: Option<usize>,
}

impl Default for NodeRenderOptions {
    fn default() -> Self {
        Self {
            column_height: 18.0,
            show_pk: true,
            show_fk: true,
            show_idx: true,
            style_nullable: true,
            theme: Theme::default(),
            show_tooltips: false,
            foreign_key_count: None,
        }
    }
}

/// Parsed column information for enhanced rendering.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ColumnInfo {
    /// The column name
    pub name: String,
    /// The data type
    pub data_type: String,
    /// Whether the column is nullable
    pub nullable: bool,
    /// Whether the column is a primary key
    pub is_pk: bool,
    /// Whether the column is a foreign key
    pub is_fk: bool,
    /// Whether the column is indexed
    pub is_idx: bool,
}

impl ColumnInfo {
    /// Parse a column string in the format "name: type[?][ PK]"
    /// Examples: "id: uuid PK", "name: text?", "`user_id`: uuid? FK"
    #[must_use]
    pub fn parse(column_str: &str) -> Self {
        let mut nullable = false;
        let mut is_pk = false;

        // Check for PK marker at the end
        let remaining = if let Some(stripped) = column_str.strip_suffix(" PK") {
            is_pk = true;
            stripped
        } else {
            column_str
        };

        // Check for nullable marker
        let remaining = if let Some(stripped) = remaining.strip_suffix('?') {
            nullable = true;
            stripped
        } else {
            remaining
        };

        // Split name and type
        let parts: Vec<&str> = remaining.splitn(2, ": ").collect();
        let name = parts.first().unwrap_or(&"").to_string();
        let data_type = parts.get(1).unwrap_or(&"").to_string();

        // Note: FK and IDX detection would require additional metadata
        // For now, we'll set them to false and rely on the schema data
        Self {
            name,
            data_type,
            nullable,
            is_pk,
            is_fk: false,
            is_idx: false,
        }
    }
}

/// Renders a node with enhanced styling and CSS classes for interactivity.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `node` - The positioned node to render
/// * `options` - The rendering options
#[allow(clippy::too_many_lines)] // Keeps SVG node markup generation in one sequential block.
pub fn render_node(out: &mut String, node: &PositionedNode, options: &NodeRenderOptions) {
    let colors = get_colors(options.theme);
    let kind = match node.kind {
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::Enum => "enum",
    };
    let tooltip_label = match node.kind {
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::Enum => "enum",
    };

    // Build column info from positioned columns
    let columns: Vec<ColumnInfo> = node
        .columns
        .iter()
        .map(|c| ColumnInfo {
            name: c.name.clone(),
            data_type: c.data_type.clone(),
            nullable: c.nullable,
            is_pk: c.is_primary_key,
            is_fk: false,
            is_idx: false,
        })
        .collect();

    // Opening group with data attribute for interactivity
    let _ = write!(
        out,
        r#"<g class="table-node node node-kind-{}" data-table-id="{}" data-id="{}" data-node-kind="{}">"#,
        kind,
        escape_attribute(&node.id),
        escape_attribute(&node.id),
        kind
    );

    // Add tooltip if enabled
    if options.show_tooltips {
        let tooltip_text = generate_node_tooltip(
            &node.label,
            tooltip_label,
            &columns,
            options.foreign_key_count,
        );
        let _ = write!(out, r"<title>{}</title>", escape_text(&tooltip_text));
    }

    // Main node rectangle
    let _ = write!(
        out,
        r#"<rect class="table-body" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="12" ry="12" fill="{}" stroke="{}" stroke-width="1.5"/>"#,
        node.x, node.y, node.width, node.height, colors.node_fill, colors.node_stroke
    );

    // Header rectangle
    let _ = write!(
        out,
        r#"<rect class="table-header" x="{:.1}" y="{:.1}" width="{:.1}" height="34" rx="12" ry="12" fill="{}"/>"#,
        node.x, node.y, node.width, colors.header_fill
    );

    // Header text
    let _ = write!(
        out,
        r#"<text class="table-name" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="14" font-weight="700" fill="{}">{}</text>"#,
        node.x + 12.0,
        node.y + 22.0,
        colors.text_primary,
        escape_text(&node.label)
    );

    // Column rows
    let start_y = node.y + 52.0;
    for (i, column) in columns.iter().enumerate() {
        let line_y = compute_column_y(start_y, i, options.column_height);

        // Determine text color based on nullable status
        let text_color = if options.style_nullable && column.nullable {
            NULLABLE_TEXT
        } else {
            colors.text_secondary
        };

        // Add italic style for nullable columns
        let font_style = if options.style_nullable && column.nullable {
            r#" font-style="italic""#
        } else {
            ""
        };

        // Column group for interactivity
        let _ = write!(
            out,
            r#"<g class="column-row" data-column-name="{}">"#,
            escape_attribute(&column.name)
        );

        // Column text with name and type
        let column_text = if node.kind == NodeKind::Enum {
            format!("• {}", column.name)
        } else {
            format!("{}: {}", column.name, column.data_type)
        };
        let _ = write!(
            out,
            r#"<text class="column-name" x="{:.1}" y="{:.1}" font-family="ui-monospace, SFMono-Regular, Menlo, monospace" font-size="12" fill="{}"{}>{}</text>"#,
            node.x + 12.0,
            line_y,
            text_color,
            font_style,
            escape_text(&column_text)
        );

        // Primary key badge
        if options.show_pk && column.is_pk {
            render_pk_badge(out, node.x + node.width - 36.0, line_y - 10.0);
        }

        // Foreign key indicator
        if options.show_fk && column.is_fk {
            render_fk_indicator(out, node.x + node.width - 28.0, line_y - 10.0);
        }

        // Index indicator
        if options.show_idx && column.is_idx {
            render_idx_indicator(out, node.x + node.width - 28.0, line_y - 10.0);
        }

        out.push_str("</g>");
    }

    out.push_str("</g>");
}

/// Generates tooltip text for a table node.
fn generate_node_tooltip(
    table_name: &str,
    kind_label: &str,
    columns: &[ColumnInfo],
    fk_count: Option<usize>,
) -> String {
    let column_count = columns.len();
    let pk_count = columns.iter().filter(|c| c.is_pk).count();

    let mut lines = vec![format!("{table_name} {kind_label}")];

    // Add column count
    lines.push(format!(
        "{} column{}",
        column_count,
        if column_count == 1 { "" } else { "s" }
    ));

    // Add PK count if any
    if pk_count > 0 {
        lines.push(format!(
            "{} primary key{}",
            pk_count,
            if pk_count == 1 { "" } else { "s" }
        ));
    }

    // Add FK count if provided
    if let Some(count) = fk_count
        && count > 0
    {
        lines.push(format!(
            "{} foreign key{}",
            count,
            if count == 1 { "" } else { "s" }
        ));
    }

    lines.join("\n")
}

/// Renders a primary key badge with a "PK" label.
fn render_pk_badge(out: &mut String, x: f32, y: f32) {
    // Badge background
    let _ = write!(
        out,
        r#"<rect class="pk-badge" x="{x:.1}" y="{y:.1}" width="24" height="14" rx="4" fill="{PK_BG}"/>"#
    );

    // "PK" text
    let _ = write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="9" font-weight="700" fill="{PK_TEXT}">PK</text>"#,
        x + 4.0,
        y + 10.0
    );
}

/// Renders a foreign key indicator with an arrow icon.
fn render_fk_indicator(out: &mut String, x: f32, y: f32) {
    // Arrow icon path
    let _ = write!(
        out,
        r#"<path class="fk-indicator" d="M{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1} M{:.1} {:.1} L{:.1} {:.1}" stroke="{FK_COLOR}" stroke-width="1.5" fill="none"/>"#,
        x,
        y + 7.0, // start point
        x + 10.0,
        y + 7.0, // arrow body
        x + 6.0,
        y + 3.0, // arrow head top
        x + 10.0,
        y + 7.0, // arrow head connection
        x + 6.0,
        y + 11.0, // arrow head bottom
    );
}

/// Renders an index indicator with "IDX" label.
fn render_idx_indicator(out: &mut String, x: f32, y: f32) {
    // "IDX" text
    let _ = write!(
        out,
        r#"<text class="idx-indicator" x="{x:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="8" font-weight="600" fill="{IDX_COLOR}">IDX</text>"#,
        y + 10.0
    );
}

use crate::escape::{escape_attribute, escape_text};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_info_parse_pk() {
        let info = ColumnInfo::parse("id: uuid PK");
        assert_eq!(info.name, "id");
        assert_eq!(info.data_type, "uuid");
        assert!(info.is_pk);
        assert!(!info.nullable);
    }

    #[test]
    fn test_column_info_parse_nullable() {
        let info = ColumnInfo::parse("description: text?");
        assert_eq!(info.name, "description");
        assert_eq!(info.data_type, "text");
        assert!(info.nullable);
        assert!(!info.is_pk);
    }

    #[test]
    fn test_column_info_parse_nullable_pk() {
        let info = ColumnInfo::parse("override_id: uuid? PK");
        assert_eq!(info.name, "override_id");
        assert_eq!(info.data_type, "uuid");
        assert!(info.nullable);
        assert!(info.is_pk);
    }

    #[test]
    fn test_render_node_produces_valid_svg() {
        use relune_core::NodeKind;
        use relune_layout::PositionedColumn;

        let node = PositionedNode {
            id: "users".to_string(),
            label: "users".to_string(),
            kind: NodeKind::Table,
            columns: vec![
                PositionedColumn {
                    name: "id".to_string(),
                    data_type: "uuid".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    is_foreign_key: false,
                    is_indexed: false,
                },
                PositionedColumn {
                    name: "name".to_string(),
                    data_type: "text".to_string(),
                    nullable: true,
                    is_primary_key: false,
                    is_foreign_key: false,
                    is_indexed: false,
                },
            ],
            x: 10.0,
            y: 10.0,
            width: 260.0,
            height: 100.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        };

        let options = NodeRenderOptions::default();

        let mut out = String::new();
        render_node(&mut out, &node, &options);

        assert!(out.contains("class=\"table-node node node-kind-table\""));
        assert!(out.contains("data-table-id=\"users\""));
        assert!(out.contains("class=\"table-header\""));
        assert!(out.contains("class=\"column-row\""));
        assert!(out.contains("pk-badge"));
    }
}
