//! CLI definition for relune.
//!
//! This module defines the command-line interface using clap derive macros.

use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use relune_core::{LayoutDirection, RouteStyle, SqlDialect};
use serde::{Deserialize, Serialize};

/// Render, inspect, export, lint, and diff database schemas
#[derive(Debug, Parser)]
#[command(name = "relune")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file (TOML; merges with CLI flags).
    #[arg(short = 'c', long = "config", global = true)]
    pub config: Option<PathBuf>,

    /// Colorize output.
    #[arg(long = "color", value_enum, global = true, default_value = "auto")]
    pub color: ColorWhen,

    /// Increase log verbosity (can be repeated).
    #[arg(short = 'v', long = "verbose", global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Reduce non-error output.
    #[arg(short = 'q', long = "quiet", global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// When to colorize output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    /// Colorize if output is a terminal.
    #[default]
    Auto,
    /// Always colorize output.
    Always,
    /// Never colorize output.
    Never,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Render an ERD from SQL, schema JSON, or database metadata.
    Render(RenderArgs),

    /// Inspect schema metadata or details for a specific table.
    Inspect(InspectArgs),

    /// Export schema JSON, graph JSON, layout JSON, or diagram text (Mermaid / D2 / DOT).
    Export(ExportArgs),

    /// Lint schema for issues and anti-patterns.
    Lint(LintArgs),

    /// Generate schema documentation as Markdown.
    Doc(DocArgs),

    /// Compare two schemas and show differences.
    Diff(DiffArgs),
}

// ============================================================================
// Render Command
// ============================================================================

/// Render an ERD from SQL, schema JSON, or database metadata.
#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("input")
            .args(["sql", "sql_text", "schema_json", "db_url"])
            .required(true)
            .multiple(false)
    )
)]
pub struct RenderArgs {
    // -------------------------------------------------------------------------
    // Input options (at least one required)
    // -------------------------------------------------------------------------
    /// Read SQL DDL from a file.
    #[arg(long = "sql", value_name = "FILE")]
    pub sql: Option<PathBuf>,

    /// Read SQL DDL directly from a string.
    #[arg(long = "sql-text", value_name = "TEXT")]
    pub sql_text: Option<String>,

    /// Read a normalized schema JSON file.
    #[arg(long = "schema-json", value_name = "FILE")]
    pub schema_json: Option<PathBuf>,

    /// Database URL for live introspection: `postgres://`, `mysql://`, `mariadb://`, or `sqlite:` (no SQL file).
    #[arg(long = "db-url", value_name = "URL")]
    pub db_url: Option<String>,

    /// SQL dialect for parsing (auto-detected if omitted).
    #[arg(long = "dialect", value_enum, default_value = "auto")]
    pub dialect: DialectArg,

    // -------------------------------------------------------------------------
    // Output options
    // -------------------------------------------------------------------------
    /// Output format. Defaults to `svg` after config merge.
    #[arg(short = 'f', long = "format", value_enum)]
    pub format: Option<RenderFormat>,

    /// Output file path; stdout if omitted.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Allow raw SVG/HTML output on stdout even when stdout is a terminal.
    #[arg(long = "stdout", conflicts_with = "out")]
    pub stdout: bool,

    // -------------------------------------------------------------------------
    // View options
    // -------------------------------------------------------------------------
    /// Center the graph on a table.
    #[arg(long = "focus", value_name = "TABLE")]
    pub focus: Option<String>,

    /// Traversal depth for focus mode. Defaults to `1` after config merge.
    #[arg(long = "depth", value_name = "N")]
    pub depth: Option<u32>,

    /// Grouping mode.
    #[arg(long = "group-by", value_enum)]
    pub group_by: Option<GroupByMode>,

    /// Explicitly include only these tables.
    #[arg(long = "include", value_name = "TABLE")]
    pub include: Vec<String>,

    /// Exclude tables from output.
    #[arg(long = "exclude", value_name = "TABLE")]
    pub exclude: Vec<String>,

    /// Visual theme. Defaults to `light` after config merge.
    #[arg(long = "theme", value_enum)]
    pub theme: Option<Theme>,

    /// Layout algorithm.
    #[arg(long = "layout", value_enum)]
    pub layout: Option<LayoutAlgorithmArg>,

    /// Edge routing style.
    #[arg(long = "edge-style", value_enum)]
    pub edge_style: Option<EdgeStyleArg>,

    /// Layout direction.
    #[arg(long = "direction", value_enum)]
    pub direction: Option<DirectionArg>,

    // -------------------------------------------------------------------------
    // Other options
    // -------------------------------------------------------------------------
    /// Print render statistics to stderr.
    #[arg(long = "stats")]
    pub stats: bool,

    /// Exit with non-zero code if warnings are emitted.
    #[arg(long = "fail-on-warning")]
    pub fail_on_warning: bool,
}

/// Output format for render command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderFormat {
    /// SVG output.
    #[default]
    Svg,
    /// Self-contained HTML with embedded SVG.
    Html,
    /// PNG raster image (rendered via resvg).
    Png,
    /// Graph JSON (intermediate representation).
    GraphJson,
    /// Schema JSON (normalized export).
    SchemaJson,
}

/// Grouping mode for tables.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupByMode {
    /// No grouping.
    #[default]
    None,
    /// Group by schema name.
    Schema,
    /// Group by table name prefix.
    Prefix,
}

/// Visual theme for rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Light theme.
    #[default]
    Light,
    /// Dark theme.
    Dark,
}

/// SQL dialect for parsing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DialectArg {
    /// Automatically detect dialect from SQL content.
    #[default]
    Auto,
    /// `PostgreSQL` dialect.
    Postgres,
    /// `MySQL` dialect.
    Mysql,
    /// `SQLite` dialect.
    Sqlite,
}

impl From<DialectArg> for SqlDialect {
    fn from(arg: DialectArg) -> Self {
        match arg {
            DialectArg::Auto => Self::Auto,
            DialectArg::Postgres => Self::Postgres,
            DialectArg::Mysql => Self::Mysql,
            DialectArg::Sqlite => Self::Sqlite,
        }
    }
}

/// Layout algorithm for the graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutAlgorithmArg {
    /// Standard hierarchical layout.
    #[default]
    Hierarchical,
    /// Force-directed layout.
    ForceDirected,
}

impl From<LayoutAlgorithmArg> for relune_core::LayoutAlgorithm {
    fn from(value: LayoutAlgorithmArg) -> Self {
        match value {
            LayoutAlgorithmArg::Hierarchical => Self::Hierarchical,
            LayoutAlgorithmArg::ForceDirected => Self::ForceDirected,
        }
    }
}

/// Edge routing style for the graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeStyleArg {
    /// Single straight segment between nodes.
    #[default]
    Straight,
    /// Axis-aligned orthogonal polyline.
    Orthogonal,
    /// Cubic curved edge.
    Curved,
}

impl From<EdgeStyleArg> for RouteStyle {
    fn from(value: EdgeStyleArg) -> Self {
        match value {
            EdgeStyleArg::Straight => Self::Straight,
            EdgeStyleArg::Orthogonal => Self::Orthogonal,
            EdgeStyleArg::Curved => Self::Curved,
        }
    }
}

/// Layout direction for the graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirectionArg {
    /// Top to bottom (default).
    #[default]
    TopToBottom,
    /// Left to right.
    LeftToRight,
    /// Right to left.
    RightToLeft,
    /// Bottom to top.
    BottomToTop,
}

impl From<DirectionArg> for LayoutDirection {
    fn from(value: DirectionArg) -> Self {
        match value {
            DirectionArg::TopToBottom => Self::TopToBottom,
            DirectionArg::LeftToRight => Self::LeftToRight,
            DirectionArg::RightToLeft => Self::RightToLeft,
            DirectionArg::BottomToTop => Self::BottomToTop,
        }
    }
}

// ============================================================================
// Inspect Command
// ============================================================================

/// Inspect schema metadata or details for a specific table.
#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("input")
            .args(["sql", "sql_text", "schema_json", "db_url"])
            .required(true)
            .multiple(false)
    )
)]
pub struct InspectArgs {
    // -------------------------------------------------------------------------
    // Input options (at least one required)
    // -------------------------------------------------------------------------
    /// Read SQL DDL from a file.
    #[arg(long = "sql", value_name = "FILE")]
    pub sql: Option<PathBuf>,

    /// Read SQL DDL directly from a string.
    #[arg(long = "sql-text", value_name = "TEXT")]
    pub sql_text: Option<String>,

    /// Read a normalized schema JSON file.
    #[arg(long = "schema-json", value_name = "FILE")]
    pub schema_json: Option<PathBuf>,

    /// Database URL for live introspection: `postgres://`, `mysql://`, `mariadb://`, or `sqlite:` (no SQL file).
    #[arg(long = "db-url", value_name = "URL")]
    pub db_url: Option<String>,

    /// SQL dialect for parsing (auto-detected if omitted).
    #[arg(long = "dialect", value_enum, default_value = "auto")]
    pub dialect: DialectArg,

    // -------------------------------------------------------------------------
    // Inspect options
    // -------------------------------------------------------------------------
    /// Table to inspect.
    #[arg(long = "table", value_name = "NAME")]
    pub table: Option<String>,

    /// Print schema summary (default behavior when --table not specified).
    #[arg(long = "summary")]
    pub summary: bool,

    /// Output format. Defaults to `text` after config merge.
    #[arg(long = "format", value_enum)]
    pub format: Option<InspectFormat>,

    /// Output file path; stdout if omitted.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,
}

/// Output format for inspect command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InspectFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
}

// ============================================================================
// Export Command
// ============================================================================

/// Export normalized schema, graph data, or review-oriented diagram text.
#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("input")
            .args(["sql", "sql_text", "schema_json", "db_url"])
            .required(true)
            .multiple(false)
    )
)]
pub struct ExportArgs {
    // -------------------------------------------------------------------------
    // Input options (at least one required)
    // -------------------------------------------------------------------------
    /// Read SQL DDL from a file.
    #[arg(long = "sql", value_name = "FILE")]
    pub sql: Option<PathBuf>,

    /// Read SQL DDL directly from a string.
    #[arg(long = "sql-text", value_name = "TEXT")]
    pub sql_text: Option<String>,

    /// Read a normalized schema JSON file.
    #[arg(long = "schema-json", value_name = "FILE")]
    pub schema_json: Option<PathBuf>,

    /// Database URL for live introspection: `postgres://`, `mysql://`, `mariadb://`, or `sqlite:` (no SQL file).
    #[arg(long = "db-url", value_name = "URL")]
    pub db_url: Option<String>,

    /// SQL dialect for parsing (auto-detected if omitted).
    #[arg(long = "dialect", value_enum, default_value = "auto")]
    pub dialect: DialectArg,

    // -------------------------------------------------------------------------
    // Export options
    // -------------------------------------------------------------------------
    /// Export format. Required unless provided by `export.format` in config.
    #[arg(long = "format", value_enum)]
    pub format: Option<ExportFormat>,

    /// Output file path; stdout if omitted.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Optional focus table.
    #[arg(long = "focus", value_name = "TABLE")]
    pub focus: Option<String>,

    /// Focus depth. Defaults to `1` after config merge.
    #[arg(long = "depth", value_name = "N")]
    pub depth: Option<u32>,

    /// Grouping mode.
    #[arg(long = "group-by", value_enum)]
    pub group_by: Option<GroupByMode>,

    /// Layout algorithm for positioned output.
    #[arg(long = "layout", value_enum)]
    pub layout: Option<LayoutAlgorithmArg>,

    /// Edge routing style for positioned output.
    #[arg(long = "edge-style", value_enum)]
    pub edge_style: Option<EdgeStyleArg>,

    /// Layout direction for positioned output.
    #[arg(long = "direction", value_enum)]
    pub direction: Option<DirectionArg>,
}

/// Export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(clippy::enum_variant_names)]
pub enum ExportFormat {
    /// Normalized schema JSON.
    SchemaJson,
    /// Graph JSON (intermediate representation).
    GraphJson,
    /// Layout JSON (positioned graph).
    LayoutJson,
    /// Mermaid ER diagram (`erDiagram`).
    Mermaid,
    /// D2 diagram source.
    D2,
    /// Graphviz DOT (`digraph`).
    Dot,
}

// ============================================================================
// Doc Command
// ============================================================================

/// Generate schema documentation as Markdown.
#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("input")
            .args(["sql", "sql_text", "schema_json", "db_url"])
            .required(true)
            .multiple(false)
    )
)]
pub struct DocArgs {
    // -------------------------------------------------------------------------
    // Input options (at least one required)
    // -------------------------------------------------------------------------
    /// Read SQL DDL from a file.
    #[arg(long = "sql", value_name = "FILE")]
    pub sql: Option<PathBuf>,

    /// Read SQL DDL directly from a string.
    #[arg(long = "sql-text", value_name = "TEXT")]
    pub sql_text: Option<String>,

    /// Read a normalized schema JSON file.
    #[arg(long = "schema-json", value_name = "FILE")]
    pub schema_json: Option<PathBuf>,

    /// Database URL for live introspection: `postgres://`, `mysql://`, `mariadb://`, or `sqlite:` (no SQL file).
    #[arg(long = "db-url", value_name = "URL")]
    pub db_url: Option<String>,

    /// SQL dialect for parsing (auto-detected if omitted).
    #[arg(long = "dialect", value_enum, default_value = "auto")]
    pub dialect: DialectArg,

    // -------------------------------------------------------------------------
    // Output options
    // -------------------------------------------------------------------------
    /// Output file path; stdout if omitted.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,
}

// ============================================================================
// Lint Command
// ============================================================================

/// Lint schema for issues and anti-patterns.
#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("input")
            .args(["sql", "db_url", "schema_json"])
            .required(true)
            .multiple(false)
    )
)]
pub struct LintArgs {
    // -------------------------------------------------------------------------
    // Input options (at least one required)
    // -------------------------------------------------------------------------
    /// Read SQL DDL from a file.
    #[arg(long = "sql", value_name = "FILE")]
    pub sql: Option<PathBuf>,

    /// Database URL for live introspection: `postgres://`, `mysql://`, `mariadb://`, or `sqlite:` (no SQL file).
    #[arg(long = "db-url", value_name = "URL")]
    pub db_url: Option<String>,

    /// Read a normalized schema JSON file.
    #[arg(long = "schema-json", value_name = "FILE")]
    pub schema_json: Option<PathBuf>,

    /// SQL dialect for parsing (auto-detected if omitted).
    #[arg(long = "dialect", value_enum, default_value = "auto")]
    pub dialect: DialectArg,

    // -------------------------------------------------------------------------
    // Lint options
    // -------------------------------------------------------------------------
    /// Output format. Defaults to `text` after config merge.
    #[arg(long = "format", value_enum)]
    pub format: Option<LintFormat>,

    /// Output file path; stdout if omitted.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Restrict execution to specific rules (can be repeated).
    #[arg(long = "rules", value_name = "RULE")]
    pub rules: Vec<String>,

    /// Minimum severity that causes non-zero exit.
    #[arg(long = "deny", value_name = "SEVERITY", value_enum)]
    pub deny: Option<LintSeverity>,
}

/// Output format for lint command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
}

/// Severity level for lint --deny option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintSeverity {
    /// Error severity.
    Error,
    /// Warning severity.
    Warning,
    /// Info severity.
    Info,
    /// Hint severity (lowest).
    Hint,
}

// ============================================================================
// Diff Command
// ============================================================================

/// Compare two schemas and show differences.
#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("before_input")
            .args(["before", "before_sql_text", "before_schema_json"])
            .required(true)
            .multiple(false)
    ),
    group(
        ArgGroup::new("after_input")
            .args(["after", "after_sql_text", "after_schema_json"])
            .required(true)
            .multiple(false)
    )
)]
pub struct DiffArgs {
    // -------------------------------------------------------------------------
    // Before input options (at least one required)
    // -------------------------------------------------------------------------
    /// Baseline SQL or schema JSON file.
    #[arg(long = "before", value_name = "FILE")]
    pub before: Option<PathBuf>,

    /// Baseline SQL DDL directly from a string.
    #[arg(long = "before-sql-text", value_name = "TEXT")]
    pub before_sql_text: Option<String>,

    /// Baseline normalized schema JSON file.
    #[arg(long = "before-schema-json", value_name = "FILE")]
    pub before_schema_json: Option<PathBuf>,

    // -------------------------------------------------------------------------
    // After input options (at least one required)
    // -------------------------------------------------------------------------
    /// Updated SQL or schema JSON file.
    #[arg(long = "after", value_name = "FILE")]
    pub after: Option<PathBuf>,

    /// Updated SQL DDL directly from a string.
    #[arg(long = "after-sql-text", value_name = "TEXT")]
    pub after_sql_text: Option<String>,

    /// Updated normalized schema JSON file.
    #[arg(long = "after-schema-json", value_name = "FILE")]
    pub after_schema_json: Option<PathBuf>,

    /// SQL dialect for parsing (auto-detected if omitted).
    #[arg(long = "dialect", value_enum)]
    pub dialect: Option<DialectArg>,

    // -------------------------------------------------------------------------
    // Output options
    // -------------------------------------------------------------------------
    /// Output format.
    #[arg(short = 'f', long = "format", value_enum)]
    pub format: Option<DiffFormat>,

    /// Output file path; stdout if omitted.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Allow raw SVG/HTML output on stdout even when stdout is a terminal.
    #[arg(long = "stdout", conflicts_with = "out")]
    pub stdout: bool,
}

/// Output format for diff command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
    /// SVG diagram with diff overlay.
    Svg,
    /// Self-contained HTML with diff overlay.
    Html,
}
