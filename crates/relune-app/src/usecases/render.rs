//! Render use case implementation.

use std::time::Instant;

use relune_core::SchemaStats;
use relune_layout::{
    FocusExtractor, LayoutConfig, LayoutGraphBuilder, build_layout_from_graph_with_config,
};
use relune_render_html::{HtmlRenderOptions, Theme as HtmlTheme};
use relune_render_svg::{SvgRenderOptions, Theme as SvgTheme, render_svg_with_overlay};
use tracing::{debug, info, info_span};

use crate::error::AppError;
use crate::request::{OutputFormat, RenderOptions, RenderRequest, RenderTheme};
use crate::result::{RenderResult, RenderStats};
use crate::schema_input::schema_from_input;

enum RenderPath {
    Svg {
        positioned: relune_layout::PositionedGraph,
    },
    Html {
        graph: relune_layout::LayoutGraph,
        positioned: relune_layout::PositionedGraph,
    },
    GraphJson {
        graph: relune_layout::LayoutGraph,
    },
    SchemaJson {
        export: relune_core::export::SchemaExport,
    },
}

/// Execute a render request.
#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_lines)] // This usecase keeps parse, graph, layout, and render timing in one flow.
pub fn render(request: RenderRequest) -> Result<RenderResult, AppError> {
    let _total_span = info_span!("render_pipeline").entered();
    let _total_start = Instant::now();
    debug!(
        output_format = ?request.output_format,
        focus = ?request.focus,
        filter = ?request.filter,
        grouping = ?request.grouping,
        layout = ?request.layout,
        options = ?request.options,
        "accepted render request"
    );

    // Step 1: Parse input
    let parse_start = Instant::now();
    let (schema, diagnostics) = {
        let _span = info_span!("parse").entered();
        schema_from_input(&request.input)?
    };
    let mut diagnostics = diagnostics;
    let stats = schema.stats();
    let parse_time = parse_start.elapsed();
    debug!(
        tables = stats.table_count,
        columns = stats.column_count,
        "parse complete"
    );

    // Step 2-3: Prepare the render path and intermediate data.
    let graph_start = Instant::now();
    let layout_config = LayoutConfig::from(&request.layout);
    let (render_path, graph_time, layout_time) = match request.output_format {
        OutputFormat::Svg => {
            let (graph, graph_diagnostics) = {
                let _span = info_span!("graph_build").entered();
                build_graph(&request, &schema)?
            };
            diagnostics.extend(graph_diagnostics);
            let graph_time = graph_start.elapsed();
            let layout_start = Instant::now();
            let positioned = {
                let _span = info_span!("layout").entered();
                build_layout_from_graph_with_config(&graph, &layout_config)?
            };
            let layout_time = layout_start.elapsed();
            (RenderPath::Svg { positioned }, graph_time, layout_time)
        }
        OutputFormat::Html => {
            let (graph, graph_diagnostics) = {
                let _span = info_span!("graph_build").entered();
                build_graph(&request, &schema)?
            };
            diagnostics.extend(graph_diagnostics);
            let graph_time = graph_start.elapsed();
            let layout_start = Instant::now();
            let positioned = {
                let _span = info_span!("layout").entered();
                build_layout_from_graph_with_config(&graph, &layout_config)?
            };
            let layout_time = layout_start.elapsed();
            (
                RenderPath::Html { graph, positioned },
                graph_time,
                layout_time,
            )
        }
        OutputFormat::GraphJson => {
            let (graph, graph_diagnostics) = {
                let _span = info_span!("graph_build").entered();
                build_graph(&request, &schema)?
            };
            diagnostics.extend(graph_diagnostics);
            (
                RenderPath::GraphJson { graph },
                graph_start.elapsed(),
                std::time::Duration::ZERO,
            )
        }
        OutputFormat::SchemaJson => (
            RenderPath::SchemaJson {
                export: relune_core::export::export_schema(&schema),
            },
            std::time::Duration::ZERO,
            std::time::Duration::ZERO,
        ),
    };

    // Step 4: Render
    let render_start = Instant::now();
    let content = {
        let _span = info_span!("render", format = ?request.output_format).entered();
        match render_path {
            RenderPath::Svg { positioned } => render_svg_output(
                &positioned,
                &stats,
                request.options,
                request.overlay.as_ref(),
            )?,
            RenderPath::Html { graph, positioned } => render_html_output(
                &positioned,
                &graph,
                &stats,
                request.options,
                request.overlay.as_ref(),
            )?,
            RenderPath::GraphJson { graph } => serde_json::to_string_pretty(&graph)?,
            RenderPath::SchemaJson { export } => serde_json::to_string_pretty(&export)?,
        }
    };
    let render_time = render_start.elapsed();

    let render_stats =
        RenderStats::from_schema_stats(&stats, parse_time, graph_time, layout_time, render_time);
    info!(
        output_format = ?request.output_format,
        tables = render_stats.table_count,
        columns = render_stats.column_count,
        diagnostics = diagnostics.len(),
        parse_ms = parse_time.as_millis(),
        graph_ms = graph_time.as_millis(),
        layout_ms = layout_time.as_millis(),
        render_ms = render_time.as_millis(),
        "render complete"
    );

    Ok(RenderResult {
        content,
        diagnostics,
        stats: render_stats,
    })
}

fn build_graph(
    request: &RenderRequest,
    schema: &relune_core::Schema,
) -> Result<(relune_layout::LayoutGraph, Vec<relune_core::Diagnostic>), AppError> {
    let (mut graph, diagnostics) = LayoutGraphBuilder::new()
        .filter(request.filter.clone())
        .focus(request.focus.clone())
        .grouping(request.grouping)
        .build_with_diagnostics(schema);
    if let Some(ref focus) = request.focus {
        graph = FocusExtractor
            .extract(&graph, focus)
            .map_err(relune_layout::LayoutError::from)?;
    }
    Ok((graph, diagnostics))
}

/// Render to SVG format.
fn render_svg_output(
    positioned: &relune_layout::PositionedGraph,
    _stats: &SchemaStats,
    options: RenderOptions,
    overlay: Option<&relune_layout::DiagramOverlay>,
) -> Result<String, AppError> {
    let options = SvgRenderOptions {
        theme: map_svg_theme(options.theme),
        show_legend: options.show_legend,
        show_stats: options.show_stats,
        embed_css: true,
        compact: false,
        show_tooltips: true,
    };
    Ok(render_svg_with_overlay(positioned, options, overlay)?)
}

/// Render to HTML format.
fn render_html_output(
    positioned: &relune_layout::PositionedGraph,
    graph: &relune_layout::LayoutGraph,
    _stats: &SchemaStats,
    options: RenderOptions,
    overlay: Option<&relune_layout::DiagramOverlay>,
) -> Result<String, AppError> {
    // First render SVG (with overlay)
    let svg_options = SvgRenderOptions {
        theme: map_svg_theme(options.theme),
        show_legend: options.show_legend,
        show_stats: options.show_stats,
        embed_css: true,
        compact: false,
        show_tooltips: true,
    };
    let svg = render_svg_with_overlay(positioned, svg_options, overlay)?;

    // Then wrap in HTML (with overlay)
    let html_options = HtmlRenderOptions {
        theme: map_html_theme(options.theme),
        include_legend: options.show_legend || options.show_stats,
        ..Default::default()
    };
    let html = relune_render_html::render_html_with_overlay(graph, &svg, &html_options, overlay)?;

    Ok(html)
}

const fn map_svg_theme(theme: RenderTheme) -> SvgTheme {
    match theme {
        RenderTheme::Light => SvgTheme::Light,
        RenderTheme::Dark => SvgTheme::Dark,
    }
}

const fn map_html_theme(theme: RenderTheme) -> HtmlTheme {
    match theme {
        RenderTheme::Light => HtmlTheme::Light,
        RenderTheme::Dark => HtmlTheme::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_from_sql() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            );
        ";

        let request = RenderRequest::from_sql(sql).with_output_format(OutputFormat::Svg);

        let result = render(request);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert!(result.content.contains("<svg"));
        assert_eq!(result.stats.table_count, 1);
    }

    #[test]
    fn test_render_graph_json() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
        ";

        let request = RenderRequest::from_sql(sql).with_output_format(OutputFormat::GraphJson);

        let result = render(request);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert!(result.content.contains("\"nodes\""));
        assert!(result.content.contains("\"edges\""));
    }

    #[test]
    fn test_render_schema_json() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255)
            );
        ";

        let request = RenderRequest::from_sql(sql).with_output_format(OutputFormat::SchemaJson);

        let result = render(request);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert!(result.content.contains("\"tables\""));
        assert!(result.content.contains("\"users\""));
    }

    #[test]
    fn test_render_svg_and_html_include_views_and_enums() {
        let sql = r"
            CREATE TYPE status AS ENUM ('active', 'inactive');

            CREATE TABLE users (
                id INT PRIMARY KEY,
                status status NOT NULL
            );

            CREATE VIEW active_users AS
            SELECT id FROM users;
        ";

        let svg =
            render(RenderRequest::from_sql(sql).with_output_format(OutputFormat::Svg)).unwrap();
        assert!(svg.content.contains("data-node-kind=\"view\""));
        assert!(svg.content.contains("data-node-kind=\"enum\""));
        assert!(svg.content.contains("edge-kind-view-dependency"));
        assert!(svg.content.contains("edge-kind-enum-reference"));

        let html =
            render(RenderRequest::from_sql(sql).with_output_format(OutputFormat::Html)).unwrap();
        assert!(html.content.contains(r#""kind":"view""#));
        assert!(html.content.contains(r#""kind":"enum""#));
        assert!(html.content.contains(r#""kind":"view_dependency""#));
        assert!(html.content.contains("data-node-kind=\"view\""));
        assert!(html.content.contains("data-node-kind=\"enum\""));
    }
}
