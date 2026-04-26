//! Configuration file support for relune CLI.
//!
//! Config files are TOML format and support the following structure:
//! ```toml
//! [render]
//! format = "svg"  # svg, html, png, graph-json, schema-json
//! dialect = "auto" # auto, postgres, mysql, sqlite
//! theme = "light" # light, dark
//! layout = "hierarchical" # hierarchical, force-directed
//! edge_style = "straight" # straight, orthogonal, curved
//! direction = "top-to-bottom" # top-to-bottom, left-to-right, right-to-left, bottom-to-top
//! viewpoint = "billing"
//! group_by = "none" # none, schema, prefix
//! focus = "table_name"
//! depth = 1
//! include = ["table1", "table2"]
//! exclude = ["table3"]
//! show_legend = false
//! show_stats = false
//!
//! [inspect]
//! format = "text" # text, json
//! dialect = "auto"
//! fail_on_warning = false
//!
//! [export]
//! format = "schema-json" # schema-json, graph-json, layout-json, mermaid, d2, dot
//! dialect = "auto"
//! viewpoint = "billing"
//! group_by = "none"
//! layout = "hierarchical"
//! edge_style = "straight"
//! direction = "top-to-bottom"
//! focus = "table_name"
//! depth = 1
//! include = ["table1", "table2"]
//! exclude = ["table3"]
//! fail_on_warning = false
//!
//! [viewpoints.billing]
//! focus = "invoices"
//! depth = 2
//! include = ["billing_*", "invoices", "payments"]
//! exclude = ["audit_*"]
//! group_by = "schema"
//!
//! [doc]
//! dialect = "auto"
//! fail_on_warning = false
//!
//! [diff]
//! format = "text" # text, json, svg, html, markdown
//! dialect = "auto"
//! viewpoint = "billing"
//! group_by = "none"
//! layout = "hierarchical"
//! edge_style = "straight"
//! direction = "top-to-bottom"
//! focus = "table_name"
//! depth = 1
//! include = ["table1", "table2"]
//! exclude = ["table3"]
//! theme = "light"
//! show_legend = false
//! show_stats = false
//! fail_on_warning = false
//!
//! [lint]
//! dialect = "auto"
//! profile = "default" # default, strict
//! rules = ["missing-foreign-key-index"]
//! exclude_rules = ["missing-column-comment"]
//! categories = ["relationships"]
//! except_tables = ["schema_migrations"]
//! ```
//!
//! Config layering order (later overrides earlier):
//! 1. Built-in defaults
//! 2. Config file
//! 3. CLI flags

use std::path::Path;
use std::{collections::BTreeMap, fmt::Write as _};

use serde::{Deserialize, Serialize};

use crate::cli::{
    DialectArg, DiffFormat, DirectionArg, EdgeStyleArg, GroupByMode, LayoutAlgorithmArg,
    RenderFormat, Theme,
};

/// Root configuration structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReluneConfig {
    /// Render command configuration.
    #[serde(default)]
    pub render: RenderConfig,
    /// Inspect command configuration.
    #[serde(default)]
    pub inspect: InspectConfig,
    /// Export command configuration.
    #[serde(default)]
    pub export: ExportConfig,
    /// Doc command configuration.
    #[serde(default)]
    pub doc: DocConfig,
    /// Lint command configuration.
    #[serde(default)]
    pub lint: LintConfig,
    /// Diff command configuration.
    #[serde(default)]
    pub diff: DiffConfig,
    /// Named focus/filter/grouping presets shared across commands.
    #[serde(default)]
    pub viewpoints: BTreeMap<String, ViewpointConfig>,
}

/// Configuration for the render command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderConfig {
    /// Output format.
    #[serde(default)]
    pub format: Option<RenderFormat>,
    /// SQL dialect for parsing.
    #[serde(default)]
    pub dialect: Option<DialectArg>,
    /// Visual theme.
    #[serde(default)]
    pub theme: Option<Theme>,
    /// Layout algorithm.
    #[serde(default)]
    pub layout: Option<LayoutAlgorithmArg>,
    /// Edge routing style.
    #[serde(default)]
    pub edge_style: Option<EdgeStyleArg>,
    /// Layout direction.
    #[serde(default)]
    pub direction: Option<DirectionArg>,
    /// Grouping mode.
    #[serde(default)]
    pub group_by: Option<GroupByMode>,
    /// Focus table name.
    #[serde(default)]
    pub focus: Option<String>,
    /// Named viewpoint to apply by default.
    #[serde(default)]
    pub viewpoint: Option<String>,
    /// Focus depth.
    #[serde(default)]
    pub depth: Option<u32>,
    /// Tables to include.
    #[serde(default)]
    pub include: Vec<String>,
    /// Tables to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Show legend in output.
    #[serde(default)]
    pub show_legend: Option<bool>,
    /// Show statistics.
    #[serde(default)]
    pub show_stats: Option<bool>,
    /// Exit with non-zero code if warnings are emitted.
    #[serde(default)]
    pub fail_on_warning: Option<bool>,
}

/// Configuration for the inspect command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectConfig {
    /// Output format.
    #[serde(default)]
    pub format: Option<InspectFormatConfig>,
    /// SQL dialect for parsing.
    #[serde(default)]
    pub dialect: Option<DialectArg>,
    /// Exit with non-zero code if warnings are emitted.
    #[serde(default)]
    pub fail_on_warning: Option<bool>,
}

/// Configuration for the export command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportConfig {
    /// Export format.
    #[serde(default)]
    pub format: Option<ExportFormatConfig>,
    /// SQL dialect for parsing.
    #[serde(default)]
    pub dialect: Option<DialectArg>,
    /// Grouping mode.
    #[serde(default)]
    pub group_by: Option<GroupByMode>,
    /// Named viewpoint to apply by default.
    #[serde(default)]
    pub viewpoint: Option<String>,
    /// Layout algorithm.
    #[serde(default)]
    pub layout: Option<LayoutAlgorithmArg>,
    /// Edge routing style.
    #[serde(default)]
    pub edge_style: Option<EdgeStyleArg>,
    /// Layout direction.
    #[serde(default)]
    pub direction: Option<DirectionArg>,
    /// Focus table name.
    #[serde(default)]
    pub focus: Option<String>,
    /// Focus depth.
    #[serde(default)]
    pub depth: Option<u32>,
    /// Tables to include.
    #[serde(default)]
    pub include: Vec<String>,
    /// Tables to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Exit with non-zero code if warnings are emitted.
    #[serde(default)]
    pub fail_on_warning: Option<bool>,
}

/// Named focus/filter/grouping preset.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewpointConfig {
    /// Grouping mode.
    #[serde(default)]
    pub group_by: Option<GroupByMode>,
    /// Focus table name.
    #[serde(default)]
    pub focus: Option<String>,
    /// Focus depth.
    #[serde(default)]
    pub depth: Option<u32>,
    /// Tables to include.
    #[serde(default)]
    pub include: Vec<String>,
    /// Tables to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Configuration for the doc command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocConfig {
    /// SQL dialect for parsing.
    #[serde(default)]
    pub dialect: Option<DialectArg>,
    /// Exit with non-zero code if warnings are emitted.
    #[serde(default)]
    pub fail_on_warning: Option<bool>,
}

/// Configuration for the lint command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintConfig {
    /// Output format.
    #[serde(default)]
    pub format: Option<LintFormatConfig>,
    /// SQL dialect for parsing.
    #[serde(default)]
    pub dialect: Option<DialectArg>,
    /// Review profile used to seed the active rule set.
    #[serde(default)]
    pub profile: Option<LintProfileConfig>,
    /// Optional rule IDs to run.
    #[serde(default)]
    pub rules: Vec<String>,
    /// Optional rule IDs to exclude.
    #[serde(default)]
    pub exclude_rules: Vec<String>,
    /// Optional categories to keep.
    #[serde(default)]
    pub categories: Vec<LintRuleCategoryConfig>,
    /// Table patterns to suppress from the report.
    #[serde(default)]
    pub except_tables: Vec<String>,
    /// Minimum severity that causes non-zero exit.
    #[serde(default)]
    pub deny: Option<LintSeverityConfig>,
    /// Exit with non-zero code if warnings are emitted.
    #[serde(default)]
    pub fail_on_warning: Option<bool>,
}

/// Configuration for the diff command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffConfig {
    /// Output format.
    #[serde(default)]
    pub format: Option<DiffFormat>,
    /// SQL dialect for parsing.
    #[serde(default)]
    pub dialect: Option<DialectArg>,
    /// Visual theme for SVG/HTML output.
    #[serde(default)]
    pub theme: Option<Theme>,
    /// Layout algorithm for SVG/HTML output.
    #[serde(default)]
    pub layout: Option<LayoutAlgorithmArg>,
    /// Edge routing style for SVG/HTML output.
    #[serde(default)]
    pub edge_style: Option<EdgeStyleArg>,
    /// Layout direction for SVG/HTML output.
    #[serde(default)]
    pub direction: Option<DirectionArg>,
    /// Grouping mode for SVG/HTML output.
    #[serde(default)]
    pub group_by: Option<GroupByMode>,
    /// Named viewpoint to apply by default.
    #[serde(default)]
    pub viewpoint: Option<String>,
    /// Focus table name for SVG/HTML output.
    #[serde(default)]
    pub focus: Option<String>,
    /// Focus depth.
    #[serde(default)]
    pub depth: Option<u32>,
    /// Tables to include in SVG/HTML output.
    #[serde(default)]
    pub include: Vec<String>,
    /// Tables to exclude from SVG/HTML output.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Show legend in SVG/HTML output.
    #[serde(default)]
    pub show_legend: Option<bool>,
    /// Show statistics in SVG/HTML output.
    #[serde(default)]
    pub show_stats: Option<bool>,
    /// Exit with non-zero code if warnings are emitted.
    #[serde(default)]
    pub fail_on_warning: Option<bool>,
}

/// Inspect format configuration (mirrors CLI `InspectFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InspectFormatConfig {
    Text,
    Json,
}

/// Export format configuration (mirrors CLI `ExportFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(clippy::enum_variant_names)]
pub enum ExportFormatConfig {
    SchemaJson,
    GraphJson,
    LayoutJson,
    Mermaid,
    D2,
    Dot,
}

/// Lint format configuration (mirrors CLI `LintFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintFormatConfig {
    Text,
    Json,
}

/// Lint profile configuration (mirrors CLI `LintProfileArg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintProfileConfig {
    Default,
    Strict,
}

/// Lint category configuration (mirrors CLI `LintRuleCategoryArg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintRuleCategoryConfig {
    Structure,
    Relationships,
    Naming,
    Documentation,
}

/// Lint severity configuration (mirrors CLI `LintSeverity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintSeverityConfig {
    Error,
    Warning,
    Info,
    Hint,
}

impl From<InspectFormatConfig> for crate::cli::InspectFormat {
    fn from(value: InspectFormatConfig) -> Self {
        match value {
            InspectFormatConfig::Text => Self::Text,
            InspectFormatConfig::Json => Self::Json,
        }
    }
}

impl From<ExportFormatConfig> for crate::cli::ExportFormat {
    fn from(value: ExportFormatConfig) -> Self {
        match value {
            ExportFormatConfig::SchemaJson => Self::SchemaJson,
            ExportFormatConfig::GraphJson => Self::GraphJson,
            ExportFormatConfig::LayoutJson => Self::LayoutJson,
            ExportFormatConfig::Mermaid => Self::Mermaid,
            ExportFormatConfig::D2 => Self::D2,
            ExportFormatConfig::Dot => Self::Dot,
        }
    }
}

impl From<LintFormatConfig> for crate::cli::LintFormat {
    fn from(value: LintFormatConfig) -> Self {
        match value {
            LintFormatConfig::Text => Self::Text,
            LintFormatConfig::Json => Self::Json,
        }
    }
}

impl From<LintProfileConfig> for crate::cli::LintProfileArg {
    fn from(value: LintProfileConfig) -> Self {
        match value {
            LintProfileConfig::Default => Self::Default,
            LintProfileConfig::Strict => Self::Strict,
        }
    }
}

impl From<LintRuleCategoryConfig> for crate::cli::LintRuleCategoryArg {
    fn from(value: LintRuleCategoryConfig) -> Self {
        match value {
            LintRuleCategoryConfig::Structure => Self::Structure,
            LintRuleCategoryConfig::Relationships => Self::Relationships,
            LintRuleCategoryConfig::Naming => Self::Naming,
            LintRuleCategoryConfig::Documentation => Self::Documentation,
        }
    }
}

impl From<LintSeverityConfig> for crate::cli::LintSeverity {
    fn from(value: LintSeverityConfig) -> Self {
        match value {
            LintSeverityConfig::Error => Self::Error,
            LintSeverityConfig::Warning => Self::Warning,
            LintSeverityConfig::Info => Self::Info,
            LintSeverityConfig::Hint => Self::Hint,
        }
    }
}

/// Error type for config operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Invalid config value: {0}")]
    InvalidValue(String),
}

impl ReluneConfig {
    /// Load configuration from a file.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Merge CLI render args into this config.
    /// CLI args take precedence over config file values.
    pub fn merge_render_args(
        &self,
        args: &crate::cli::RenderArgs,
    ) -> Result<MergedRenderConfig, ConfigError> {
        let viewpoint_name = args
            .viewpoint
            .clone()
            .or_else(|| self.render.viewpoint.clone());
        let viewpoint = self.resolve_viewpoint(viewpoint_name.as_deref())?;

        Ok(MergedRenderConfig {
            format: args.format.or(self.render.format).unwrap_or_default(),
            dialect: args.dialect.or(self.render.dialect).unwrap_or_default(),
            theme: args.theme.or(self.render.theme).unwrap_or_default(),
            layout: args.layout.or(self.render.layout).unwrap_or_default(),
            edge_style: args
                .edge_style
                .or(self.render.edge_style)
                .unwrap_or_default(),
            direction: args.direction.or(self.render.direction).unwrap_or_default(),
            group_by: args
                .group_by
                .or_else(|| viewpoint.and_then(|entry| entry.group_by))
                .or(self.render.group_by),
            focus: args
                .focus
                .clone()
                .or_else(|| viewpoint.and_then(|entry| entry.focus.clone()))
                .or_else(|| self.render.focus.clone()),
            depth: args
                .depth
                .or_else(|| viewpoint.and_then(|entry| entry.depth))
                .or(self.render.depth)
                .unwrap_or(1),
            include: merge_table_filters(
                &args.include,
                viewpoint.map_or(&[], |entry| entry.include.as_slice()),
                &self.render.include,
            ),
            exclude: merge_table_filters(
                &args.exclude,
                viewpoint.map_or(&[], |entry| entry.exclude.as_slice()),
                &self.render.exclude,
            ),
            show_legend: self.render.show_legend.unwrap_or(false),
            show_stats: args.stats || self.render.show_stats.unwrap_or(false),
            fail_on_warning: args.fail_on_warning || self.render.fail_on_warning.unwrap_or(false),
        })
    }

    /// Merge CLI inspect args into this config.
    #[allow(clippy::unused_self)]
    #[allow(clippy::missing_const_for_fn)]
    pub fn merge_inspect_args(&self, args: &crate::cli::InspectArgs) -> MergedInspectConfig {
        MergedInspectConfig {
            format: args
                .format
                .or_else(|| self.inspect.format.map(Into::into))
                .unwrap_or_default(),
            dialect: args.dialect.or(self.inspect.dialect).unwrap_or_default(),
            fail_on_warning: args.fail_on_warning || self.inspect.fail_on_warning.unwrap_or(false),
        }
    }

    /// Merge CLI export args into this config.
    pub fn merge_export_args(
        &self,
        args: &crate::cli::ExportArgs,
    ) -> Result<MergedExportConfig, ConfigError> {
        let viewpoint_name = args
            .viewpoint
            .clone()
            .or_else(|| self.export.viewpoint.clone());
        let viewpoint = self.resolve_viewpoint(viewpoint_name.as_deref())?;
        let format = args
            .format
            .or_else(|| self.export.format.map(Into::into))
            .ok_or_else(|| {
                ConfigError::InvalidValue(
                    "Export format must be provided via --format or config export.format"
                        .to_string(),
                )
            })?;

        Ok(MergedExportConfig {
            format,
            dialect: args.dialect.or(self.export.dialect).unwrap_or_default(),
            group_by: args
                .group_by
                .or_else(|| viewpoint.and_then(|entry| entry.group_by))
                .or(self.export.group_by),
            layout: args.layout.or(self.export.layout).unwrap_or_default(),
            edge_style: args
                .edge_style
                .or(self.export.edge_style)
                .unwrap_or_default(),
            direction: args.direction.or(self.export.direction).unwrap_or_default(),
            focus: args
                .focus
                .clone()
                .or_else(|| viewpoint.and_then(|entry| entry.focus.clone()))
                .or_else(|| self.export.focus.clone()),
            depth: args
                .depth
                .or_else(|| viewpoint.and_then(|entry| entry.depth))
                .or(self.export.depth)
                .unwrap_or(1),
            include: merge_table_filters(
                &args.include,
                viewpoint.map_or(&[], |entry| entry.include.as_slice()),
                &self.export.include,
            ),
            exclude: merge_table_filters(
                &args.exclude,
                viewpoint.map_or(&[], |entry| entry.exclude.as_slice()),
                &self.export.exclude,
            ),
            fail_on_warning: args.fail_on_warning || self.export.fail_on_warning.unwrap_or(false),
        })
    }

    /// Merge CLI doc args into this config.
    #[allow(clippy::unused_self)]
    #[allow(clippy::missing_const_for_fn)]
    pub fn merge_doc_args(&self, args: &crate::cli::DocArgs) -> MergedDocConfig {
        MergedDocConfig {
            dialect: args.dialect.or(self.doc.dialect).unwrap_or_default(),
            fail_on_warning: args.fail_on_warning || self.doc.fail_on_warning.unwrap_or(false),
        }
    }

    /// Merge CLI lint args into this config.
    pub fn merge_lint_args(&self, args: &crate::cli::LintArgs) -> MergedLintConfig {
        MergedLintConfig {
            dialect: args.dialect.or(self.lint.dialect).unwrap_or_default(),
            format: args
                .format
                .or_else(|| self.lint.format.map(Into::into))
                .unwrap_or_default(),
            profile: args
                .profile
                .or_else(|| self.lint.profile.map(Into::into))
                .unwrap_or_default(),
            rules: merge_string_values(&args.rules, &self.lint.rules),
            exclude_rules: merge_string_values(&args.exclude_rules, &self.lint.exclude_rules),
            rule_categories: if args.rule_categories.is_empty() {
                self.lint
                    .categories
                    .iter()
                    .copied()
                    .map(Into::into)
                    .collect()
            } else {
                args.rule_categories.clone()
            },
            except_tables: merge_string_values(&args.except_tables, &self.lint.except_tables),
            deny: args.deny.or_else(|| self.lint.deny.map(Into::into)),
            fail_on_warning: args.fail_on_warning || self.lint.fail_on_warning.unwrap_or(false),
        }
    }

    /// Merge CLI diff args into this config.
    pub fn merge_diff_args(
        &self,
        args: &crate::cli::DiffArgs,
    ) -> Result<MergedDiffConfig, ConfigError> {
        let viewpoint = self.resolve_viewpoint(self.diff.viewpoint.as_deref())?;

        Ok(MergedDiffConfig {
            format: args.format.or(self.diff.format).unwrap_or_default(),
            dialect: args.dialect.or(self.diff.dialect).unwrap_or_default(),
            theme: self.diff.theme.unwrap_or_default(),
            layout: self.diff.layout.unwrap_or_default(),
            edge_style: self.diff.edge_style.unwrap_or_default(),
            direction: self.diff.direction.unwrap_or_default(),
            group_by: viewpoint
                .and_then(|entry| entry.group_by)
                .or(self.diff.group_by),
            focus: viewpoint
                .and_then(|entry| entry.focus.clone())
                .or_else(|| self.diff.focus.clone()),
            depth: viewpoint
                .and_then(|entry| entry.depth)
                .or(self.diff.depth)
                .unwrap_or(1),
            include: merge_table_filters(
                &[],
                viewpoint.map_or(&[], |entry| entry.include.as_slice()),
                &self.diff.include,
            ),
            exclude: merge_table_filters(
                &[],
                viewpoint.map_or(&[], |entry| entry.exclude.as_slice()),
                &self.diff.exclude,
            ),
            show_legend: self.diff.show_legend.unwrap_or(false),
            show_stats: self.diff.show_stats.unwrap_or(false),
            fail_on_warning: args.fail_on_warning || self.diff.fail_on_warning.unwrap_or(false),
        })
    }

    fn resolve_viewpoint(
        &self,
        raw_name: Option<&str>,
    ) -> Result<Option<&ViewpointConfig>, ConfigError> {
        let Some(raw_name) = raw_name else {
            return Ok(None);
        };

        let name = validate_named_value("viewpoint", raw_name)?;
        let viewpoint = self.viewpoints.get(name).ok_or_else(|| {
            let mut message = format!("unknown viewpoint '{name}'");
            if self.viewpoints.is_empty() {
                message.push_str("; no viewpoints are defined in the active config");
            } else {
                let mut available = String::new();
                for (index, candidate) in self.viewpoints.keys().enumerate() {
                    if index > 0 {
                        available.push_str(", ");
                    }
                    let _ = write!(&mut available, "{candidate}");
                }
                let _ = write!(&mut message, "; available viewpoints: {available}");
            }
            ConfigError::InvalidValue(message)
        })?;

        Ok(Some(viewpoint))
    }
}

fn merge_table_filters(cli: &[String], viewpoint: &[String], config: &[String]) -> Vec<String> {
    if !cli.is_empty() {
        cli.to_vec()
    } else if !viewpoint.is_empty() {
        viewpoint.to_vec()
    } else {
        config.to_vec()
    }
}

fn merge_string_values(cli: &[String], config: &[String]) -> Vec<String> {
    if cli.is_empty() {
        config.to_vec()
    } else {
        cli.to_vec()
    }
}

fn validate_named_value<'a>(label: &str, raw_value: &'a str) -> Result<&'a str, ConfigError> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() || trimmed != raw_value {
        return Err(ConfigError::InvalidValue(format!(
            "{label} must contain a non-empty value without surrounding whitespace"
        )));
    }

    Ok(trimmed)
}

fn validate_table_list(label: &str, values: &[String]) -> Result<(), ConfigError> {
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed != value {
            return Err(ConfigError::InvalidValue(format!(
                "{label} must contain non-empty table names without surrounding whitespace"
            )));
        }
    }

    Ok(())
}

fn validate_focus_filters(
    command: &str,
    focus: Option<&str>,
    depth: u32,
    include: &[String],
    exclude: &[String],
) -> Result<(), ConfigError> {
    validate_table_list(&format!("{command}.include"), include)?;
    validate_table_list(&format!("{command}.exclude"), exclude)?;

    if depth == 0 {
        return Err(ConfigError::InvalidValue(format!(
            "{command}.depth must be at least 1"
        )));
    }

    if let Some(conflict) = include.iter().find(|table| exclude.contains(*table)) {
        return Err(ConfigError::InvalidValue(format!(
            "{command}.include and {command}.exclude cannot both contain '{conflict}'"
        )));
    }

    let Some(raw_focus) = focus else {
        if depth != 1 {
            return Err(ConfigError::InvalidValue(format!(
                "{command}.depth can only be set when {command}.focus is provided"
            )));
        }

        return Ok(());
    };

    let focus = raw_focus.trim();
    if focus.is_empty() || focus != raw_focus {
        return Err(ConfigError::InvalidValue(format!(
            "{command}.focus must contain a non-empty table name without surrounding whitespace"
        )));
    }

    if !include.is_empty() && !include.iter().any(|table| table == focus) {
        return Err(ConfigError::InvalidValue(format!(
            "{command}.focus '{focus}' must be included when {command}.include is set"
        )));
    }

    if exclude.iter().any(|table| table == focus) {
        return Err(ConfigError::InvalidValue(format!(
            "{command}.focus '{focus}' cannot be excluded"
        )));
    }

    Ok(())
}

/// Merged render configuration after combining config file and CLI args.
#[derive(Debug, Clone)]
pub struct MergedRenderConfig {
    pub format: RenderFormat,
    pub dialect: DialectArg,
    pub theme: Theme,
    pub layout: LayoutAlgorithmArg,
    pub edge_style: EdgeStyleArg,
    pub direction: DirectionArg,
    pub group_by: Option<GroupByMode>,
    pub focus: Option<String>,
    pub depth: u32,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub show_legend: bool,
    pub show_stats: bool,
    pub fail_on_warning: bool,
}

/// Merged inspect configuration.
#[derive(Debug, Clone)]
pub struct MergedInspectConfig {
    pub format: crate::cli::InspectFormat,
    pub dialect: DialectArg,
    pub fail_on_warning: bool,
}

/// Merged export configuration.
#[derive(Debug, Clone)]
pub struct MergedExportConfig {
    pub format: crate::cli::ExportFormat,
    pub dialect: DialectArg,
    pub group_by: Option<GroupByMode>,
    pub layout: LayoutAlgorithmArg,
    pub edge_style: EdgeStyleArg,
    pub direction: DirectionArg,
    pub focus: Option<String>,
    pub depth: u32,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub fail_on_warning: bool,
}

/// Merged doc configuration.
#[derive(Debug, Clone)]
pub struct MergedDocConfig {
    pub dialect: DialectArg,
    pub fail_on_warning: bool,
}

/// Merged lint configuration.
#[derive(Debug, Clone)]
pub struct MergedLintConfig {
    pub dialect: DialectArg,
    pub format: crate::cli::LintFormat,
    pub profile: crate::cli::LintProfileArg,
    pub rules: Vec<String>,
    pub exclude_rules: Vec<String>,
    pub rule_categories: Vec<crate::cli::LintRuleCategoryArg>,
    pub except_tables: Vec<String>,
    pub deny: Option<crate::cli::LintSeverity>,
    pub fail_on_warning: bool,
}

/// Merged diff configuration.
#[derive(Debug, Clone)]
pub struct MergedDiffConfig {
    pub format: DiffFormat,
    pub dialect: DialectArg,
    pub theme: Theme,
    pub layout: LayoutAlgorithmArg,
    pub edge_style: EdgeStyleArg,
    pub direction: DirectionArg,
    pub group_by: Option<GroupByMode>,
    pub focus: Option<String>,
    pub depth: u32,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub show_legend: bool,
    pub show_stats: bool,
    pub fail_on_warning: bool,
}

impl MergedDiffConfig {
    /// Validates semantic constraints for diff configuration.
    pub fn validate_semantics(&self) -> Result<(), ConfigError> {
        validate_focus_filters(
            "diff",
            self.focus.as_deref(),
            self.depth,
            &self.include,
            &self.exclude,
        )
    }
}

impl MergedRenderConfig {
    /// Validates semantic constraints for render configuration.
    pub fn validate_semantics(&self) -> Result<(), ConfigError> {
        validate_focus_filters(
            "render",
            self.focus.as_deref(),
            self.depth,
            &self.include,
            &self.exclude,
        )
    }
}

impl MergedExportConfig {
    /// Validates semantic constraints for export configuration.
    pub fn validate_semantics(&self) -> Result<(), ConfigError> {
        validate_focus_filters(
            "export",
            self.focus.as_deref(),
            self.depth,
            &self.include,
            &self.exclude,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        DiffArgs, EdgeStyleArg, ExportFormat, InspectFormat, LayoutAlgorithmArg, LintFormat,
        RenderArgs,
    };
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("config")
    }

    #[test]
    fn test_load_valid_full_config() {
        let path = fixtures_dir().join("valid_full.toml");
        let config = ReluneConfig::from_file(&path).expect("Failed to load valid_full.toml");

        assert_eq!(config.render.format, Some(RenderFormat::Html));
        assert_eq!(config.render.theme, Some(Theme::Dark));
        assert_eq!(
            config.render.layout,
            Some(LayoutAlgorithmArg::ForceDirected)
        );
        assert_eq!(config.render.edge_style, Some(EdgeStyleArg::Orthogonal));
        assert_eq!(config.render.direction, Some(DirectionArg::LeftToRight));
        assert_eq!(config.render.group_by, Some(GroupByMode::Schema));
        assert_eq!(config.render.focus, Some("users".to_string()));
        assert_eq!(config.render.viewpoint, Some("billing".to_string()));
        assert_eq!(config.render.depth, Some(2));
        assert_eq!(config.render.include, vec!["users", "posts", "comments"]);
        assert_eq!(config.render.exclude, vec!["migrations", "audit_logs"]);
        assert_eq!(config.render.show_legend, Some(true));
        assert_eq!(config.render.show_stats, Some(true));

        // Check inspect config
        assert!(matches!(
            config.inspect.format,
            Some(InspectFormatConfig::Json)
        ));

        // Check export config
        assert!(matches!(
            config.export.format,
            Some(ExportFormatConfig::GraphJson)
        ));
        assert_eq!(config.export.viewpoint, Some("billing".to_string()));
        assert_eq!(config.export.layout, Some(LayoutAlgorithmArg::Hierarchical));
        assert_eq!(config.export.edge_style, Some(EdgeStyleArg::Curved));
        assert_eq!(config.export.direction, Some(DirectionArg::RightToLeft));
        assert_eq!(config.export.include, vec!["users", "posts"]);
        assert_eq!(config.export.exclude, vec!["audit_logs"]);
        assert_eq!(config.diff.format, Some(DiffFormat::Json));
        assert_eq!(config.diff.dialect, Some(DialectArg::Postgres));
        assert_eq!(config.viewpoints.len(), 2);
        assert_eq!(
            config.viewpoints["billing"].focus,
            Some("users".to_string())
        );
        assert_eq!(config.viewpoints["billing"].include, vec!["users", "posts"]);
    }

    #[test]
    fn test_load_partial_config() {
        let path = fixtures_dir().join("valid_partial.toml");
        let config = ReluneConfig::from_file(&path).expect("Failed to load valid_partial.toml");

        // Only some fields should be set
        assert_eq!(config.render.theme, Some(Theme::Light));
        assert_eq!(config.render.group_by, Some(GroupByMode::Schema));
        assert_eq!(config.render.exclude, vec!["temp_*"]);

        // Others should be None or default
        assert_eq!(config.render.format, None);
        assert_eq!(config.render.focus, None);
    }

    #[test]
    fn test_load_empty_config() {
        let path = fixtures_dir().join("empty.toml");
        let config = ReluneConfig::from_file(&path).expect("Failed to load empty.toml");

        // All values should be defaults
        assert_eq!(config.render.format, None);
        assert_eq!(config.render.theme, None);
    }

    #[test]
    fn test_load_invalid_syntax() {
        let path = fixtures_dir().join("invalid_syntax.toml");
        let result = ReluneConfig::from_file(&path);

        assert!(result.is_err());
        match result {
            Err(ConfigError::Parse(_)) => {}
            _ => panic!("Expected Parse error for invalid TOML syntax"),
        }
    }

    #[test]
    fn test_load_unknown_nested_key_fails() {
        let path = fixtures_dir().join("unknown_nested_key.toml");
        let result = ReluneConfig::from_file(&path);

        assert!(result.is_err());
        match result {
            Err(ConfigError::Parse(error)) => {
                assert!(error.to_string().contains("unknown field `tehme`"));
            }
            _ => panic!("Expected Parse error for unknown nested key"),
        }
    }

    #[test]
    fn test_load_unknown_root_key_fails() {
        let path = fixtures_dir().join("unknown_root_key.toml");
        let result = ReluneConfig::from_file(&path);

        assert!(result.is_err());
        match result {
            Err(ConfigError::Parse(error)) => {
                assert!(error.to_string().contains("unknown field `unknown_field`"));
            }
            _ => panic!("Expected Parse error for unknown root key"),
        }
    }

    #[test]
    fn test_merge_render_args_cli_overrides_config() {
        // Load config with specific values
        let config = ReluneConfig::default();

        // Create CLI args with different values
        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: Some(RenderFormat::Svg), // CLI specifies svg
            out: None,
            stdout: false,
            focus: Some("posts".to_string()), // CLI specifies different focus
            viewpoint: None,
            depth: Some(3),                      // CLI specifies different depth
            group_by: Some(GroupByMode::Prefix), // CLI specifies different group_by
            include: vec!["a".to_string()],      // CLI specifies different include
            exclude: vec!["b".to_string()],      // CLI specifies different exclude
            theme: Some(Theme::Light),           // CLI specifies light theme
            layout: Some(LayoutAlgorithmArg::Hierarchical),
            edge_style: Some(EdgeStyleArg::Straight),
            direction: None,
            stats: true,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");

        // CLI values should win
        assert_eq!(merged.format, RenderFormat::Svg);
        assert_eq!(merged.theme, Theme::Light);
        assert_eq!(merged.layout, LayoutAlgorithmArg::Hierarchical);
        assert_eq!(merged.edge_style, EdgeStyleArg::Straight);
        assert_eq!(merged.focus, Some("posts".to_string()));
        assert_eq!(merged.depth, 3);
        assert_eq!(merged.group_by, Some(GroupByMode::Prefix));
        assert_eq!(merged.include, vec!["a"]);
        assert_eq!(merged.exclude, vec!["b"]);
        assert!(!merged.show_legend);
        assert!(merged.show_stats);
    }

    #[test]
    fn test_merge_render_args_config_used_when_cli_not_specified() {
        // Create config with specific values
        let mut config = ReluneConfig::default();
        config.render.format = Some(RenderFormat::Html);
        config.render.theme = Some(Theme::Dark);
        config.render.focus = Some("config_table".to_string());
        config.render.viewpoint = Some("billing".to_string());
        config.render.depth = Some(5);
        config.render.group_by = Some(GroupByMode::Schema);
        config.render.include = vec!["config_include".to_string()];
        config.viewpoints.insert(
            "billing".to_string(),
            ViewpointConfig {
                focus: Some("viewpoint_table".to_string()),
                depth: Some(3),
                group_by: Some(GroupByMode::Prefix),
                include: vec!["viewpoint_include".to_string()],
                exclude: vec!["viewpoint_exclude".to_string()],
            },
        );

        // Create CLI args with minimal values (defaults)
        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            stdout: false,
            focus: None, // Not specified - should use config
            viewpoint: None,
            depth: None,
            group_by: None,  // Not specified - should use config
            include: vec![], // Empty - should use config
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            direction: None,
            stats: false,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");

        // Config values should be used when CLI uses defaults
        assert_eq!(merged.format, RenderFormat::Html);
        assert_eq!(merged.theme, Theme::Dark);
        assert_eq!(merged.focus, Some("viewpoint_table".to_string()));
        assert_eq!(merged.depth, 3);
        assert_eq!(merged.group_by, Some(GroupByMode::Prefix));
        assert_eq!(merged.include, vec!["viewpoint_include"]);
        assert_eq!(merged.exclude, vec!["viewpoint_exclude"]);
    }

    #[test]
    fn test_merge_render_args_cli_explicit_overrides_config() {
        // Create config with specific values
        let mut config = ReluneConfig::default();
        config.render.focus = Some("config_table".to_string());
        config.render.viewpoint = Some("billing".to_string());
        config.render.depth = Some(5);
        config.render.group_by = Some(GroupByMode::Schema);
        config.render.include = vec!["config_include".to_string()];
        config.render.exclude = vec!["config_exclude".to_string()];
        config.viewpoints.insert(
            "billing".to_string(),
            ViewpointConfig {
                focus: Some("viewpoint_table".to_string()),
                depth: Some(3),
                group_by: Some(GroupByMode::Prefix),
                include: vec!["viewpoint_include".to_string()],
                exclude: vec!["viewpoint_exclude".to_string()],
            },
        );

        // Create CLI args with explicit values (non-defaults)
        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: Some(RenderFormat::Html), // CLI explicitly specifies html
            out: None,
            stdout: false,
            focus: Some("cli_table".to_string()), // CLI explicitly specifies focus
            viewpoint: None,
            depth: Some(10),                     // CLI explicitly specifies depth
            group_by: Some(GroupByMode::Prefix), // CLI explicitly specifies group_by
            include: vec!["cli_include".to_string()], // CLI explicitly specifies include
            exclude: vec!["cli_exclude".to_string()], // CLI explicitly specifies exclude
            theme: Some(Theme::Dark),            // CLI explicitly specifies dark
            layout: Some(LayoutAlgorithmArg::ForceDirected),
            edge_style: Some(EdgeStyleArg::Orthogonal),
            direction: None,
            stats: true,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");

        // CLI values should always win when explicitly provided
        assert_eq!(merged.format, RenderFormat::Html);
        assert_eq!(merged.theme, Theme::Dark);
        assert_eq!(merged.layout, LayoutAlgorithmArg::ForceDirected);
        assert_eq!(merged.edge_style, EdgeStyleArg::Orthogonal);
        assert_eq!(merged.focus, Some("cli_table".to_string()));
        assert_eq!(merged.depth, 10);
        assert_eq!(merged.group_by, Some(GroupByMode::Prefix));
        assert_eq!(merged.include, vec!["cli_include"]);
        assert_eq!(merged.exclude, vec!["cli_exclude"]);
    }

    #[test]
    fn test_from_file_missing_returns_error() {
        let path = PathBuf::from("/nonexistent/path/config.toml");
        assert!(ReluneConfig::from_file(&path).is_err());
    }

    #[test]
    fn test_from_file_partial() {
        let path = fixtures_dir().join("valid_partial.toml");
        let config = ReluneConfig::from_file(&path).expect("Should load config");

        // Should have loaded values
        assert_eq!(config.render.theme, Some(Theme::Light));
        assert_eq!(config.render.group_by, Some(GroupByMode::Schema));
    }

    #[test]
    fn test_config_error_display() {
        let parse_error =
            ConfigError::Parse(toml::from_str::<ReluneConfig>("invalid").unwrap_err());
        assert!(parse_error.to_string().contains("Failed to parse"));

        let io_error = ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(io_error.to_string().contains("Failed to read"));

        let invalid_error = ConfigError::InvalidValue("bad value".to_string());
        assert!(invalid_error.to_string().contains("Invalid config value"));
    }

    #[test]
    fn test_merge_inspect_args() {
        let config = ReluneConfig::default();
        let args = crate::cli::InspectArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            table: None,
            summary: false,
            format: Some(InspectFormat::Json),
            out: None,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config.merge_inspect_args(&args);
        assert_eq!(merged.format, InspectFormat::Json);
        assert!(!merged.fail_on_warning);
    }

    #[test]
    fn test_merge_export_args() {
        let mut config = ReluneConfig::default();
        config.export.format = Some(ExportFormatConfig::GraphJson);
        config.export.focus = Some("config_focus".to_string());
        config.export.viewpoint = Some("billing".to_string());
        config.export.depth = Some(5);
        config.export.layout = Some(LayoutAlgorithmArg::ForceDirected);
        config.export.edge_style = Some(EdgeStyleArg::Orthogonal);
        config.export.include = vec!["config_include".to_string()];
        config.export.exclude = vec!["config_exclude".to_string()];
        config.viewpoints.insert(
            "billing".to_string(),
            ViewpointConfig {
                focus: Some("viewpoint_focus".to_string()),
                depth: Some(2),
                group_by: Some(GroupByMode::Schema),
                include: vec!["viewpoint_include".to_string()],
                exclude: vec!["viewpoint_exclude".to_string()],
            },
        );

        let args = crate::cli::ExportArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            focus: None, // Not specified - should use config
            viewpoint: None,
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            layout: None,
            edge_style: None,
            direction: None,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_export_args(&args)
            .expect("export format should be resolved");
        assert_eq!(merged.format, ExportFormat::GraphJson);
        assert_eq!(merged.focus, Some("viewpoint_focus".to_string()));
        assert_eq!(merged.depth, 2);
        assert_eq!(merged.group_by, Some(GroupByMode::Schema));
        assert_eq!(merged.include, vec!["viewpoint_include"]);
        assert_eq!(merged.exclude, vec!["viewpoint_exclude"]);
        assert_eq!(merged.layout, LayoutAlgorithmArg::ForceDirected);
        assert_eq!(merged.edge_style, EdgeStyleArg::Orthogonal);
        assert!(!merged.fail_on_warning);
    }

    #[test]
    fn test_merge_export_args_requires_format() {
        let config = ReluneConfig::default();
        let args = crate::cli::ExportArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            focus: None,
            viewpoint: None,
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            layout: None,
            edge_style: None,
            direction: None,
            fail_on_warning: false,
            dialect: None,
        };

        let error = config
            .merge_export_args(&args)
            .expect_err("missing export format should fail");
        assert!(
            error
                .to_string()
                .contains("Export format must be provided via --format or config export.format")
        );
    }

    #[test]
    fn test_merge_doc_args() {
        let mut config = ReluneConfig::default();
        config.doc.fail_on_warning = Some(true);

        let args = crate::cli::DocArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            out: None,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config.merge_doc_args(&args);
        assert!(merged.fail_on_warning);
    }

    #[test]
    fn test_validate_render_semantics_accepts_consistent_filters() {
        let config = MergedRenderConfig {
            format: RenderFormat::Svg,
            dialect: DialectArg::Auto,
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            direction: DirectionArg::default(),
            group_by: None,
            focus: Some("users".to_string()),
            depth: 2,
            include: vec!["users".to_string(), "posts".to_string()],
            exclude: vec!["comments".to_string()],
            show_legend: false,
            show_stats: false,
            fail_on_warning: false,
        };

        config
            .validate_semantics()
            .expect("consistent focus filters should be accepted");
    }

    #[test]
    fn test_validate_render_semantics_rejects_depth_without_focus() {
        let config = MergedRenderConfig {
            format: RenderFormat::Svg,
            dialect: DialectArg::Auto,
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            direction: DirectionArg::default(),
            group_by: None,
            focus: None,
            depth: 2,
            include: Vec::new(),
            exclude: Vec::new(),
            show_legend: false,
            show_stats: false,
            fail_on_warning: false,
        };

        let error = config
            .validate_semantics()
            .expect_err("depth without focus should be rejected");
        assert!(error.to_string().contains("render.depth can only be set"));
    }

    #[test]
    fn test_validate_render_semantics_rejects_conflicting_include_and_exclude() {
        let config = MergedRenderConfig {
            format: RenderFormat::Svg,
            dialect: DialectArg::Auto,
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            direction: DirectionArg::default(),
            group_by: None,
            focus: Some("users".to_string()),
            depth: 1,
            include: vec!["users".to_string(), "posts".to_string()],
            exclude: vec!["posts".to_string()],
            show_legend: false,
            show_stats: false,
            fail_on_warning: false,
        };

        let error = config
            .validate_semantics()
            .expect_err("overlapping filters should be rejected");
        assert!(
            error
                .to_string()
                .contains("render.include and render.exclude cannot both contain 'posts'")
        );
    }

    #[test]
    fn test_validate_render_semantics_rejects_focus_not_in_include() {
        let config = MergedRenderConfig {
            format: RenderFormat::Svg,
            dialect: DialectArg::Auto,
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            direction: DirectionArg::default(),
            group_by: None,
            focus: Some("users".to_string()),
            depth: 1,
            include: vec!["posts".to_string()],
            exclude: Vec::new(),
            show_legend: false,
            show_stats: false,
            fail_on_warning: false,
        };

        let error = config
            .validate_semantics()
            .expect_err("focus outside the include list should be rejected");
        assert!(
            error
                .to_string()
                .contains("render.focus 'users' must be included")
        );
    }

    #[test]
    fn test_validate_export_semantics_rejects_blank_focus() {
        let config = MergedExportConfig {
            format: crate::cli::ExportFormat::SchemaJson,
            dialect: DialectArg::Auto,
            group_by: None,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            direction: DirectionArg::default(),
            focus: Some("   ".to_string()),
            depth: 1,
            include: Vec::new(),
            exclude: Vec::new(),
            fail_on_warning: false,
        };

        let error = config
            .validate_semantics()
            .expect_err("blank export focus should be rejected");
        assert!(
            error
                .to_string()
                .contains("export.focus must contain a non-empty table name")
        );
    }

    #[test]
    fn test_validate_export_semantics_rejects_depth_without_focus() {
        let config = MergedExportConfig {
            format: crate::cli::ExportFormat::SchemaJson,
            dialect: DialectArg::Auto,
            group_by: None,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            direction: DirectionArg::default(),
            focus: None,
            depth: 2,
            include: Vec::new(),
            exclude: Vec::new(),
            fail_on_warning: false,
        };

        let error = config
            .validate_semantics()
            .expect_err("depth without focus should be rejected");
        assert!(error.to_string().contains("export.depth can only be set"));
    }

    #[test]
    fn test_merge_lint_args() {
        let mut config = ReluneConfig::default();
        config.lint.format = Some(LintFormatConfig::Json);
        config.lint.deny = Some(LintSeverityConfig::Warning);

        let args = crate::cli::LintArgs {
            sql: None,
            db_url: None,
            schema_json: None,
            dialect: None,
            format: None,
            out: None,
            profile: None,
            rules: vec![],
            exclude_rules: vec![],
            rule_categories: vec![],
            except_tables: vec![],
            deny: None, // Not specified - should use config
            fail_on_warning: false,
        };

        let merged = config.merge_lint_args(&args);
        assert_eq!(merged.format, LintFormat::Json);
        assert_eq!(merged.profile, crate::cli::LintProfileArg::Default);
        assert_eq!(merged.deny, Some(crate::cli::LintSeverity::Warning));
        assert!(!merged.fail_on_warning);
    }

    #[test]
    fn test_merge_render_args_uses_config_fail_on_warning() {
        let mut config = ReluneConfig::default();
        config.render.fail_on_warning = Some(true);

        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            stdout: false,
            focus: None,
            viewpoint: None,
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            direction: None,
            stats: false,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");
        assert!(merged.fail_on_warning);
    }

    #[test]
    fn test_merge_diff_args_config_used_when_cli_not_specified() {
        let mut config = ReluneConfig::default();
        config.diff.format = Some(DiffFormat::Json);
        config.diff.dialect = Some(DialectArg::Mysql);

        let args = DiffArgs {
            before: None,
            before_sql_text: None,
            before_schema_json: None,
            after: None,
            after_sql_text: None,
            after_schema_json: None,
            dialect: None,
            format: None,
            out: None,
            stdout: false,
            fail_on_warning: false,
            exit_code: false,
        };

        let merged = config.merge_diff_args(&args).expect("merge should succeed");
        assert_eq!(merged.format, DiffFormat::Json);
        assert_eq!(merged.dialect, DialectArg::Mysql);
        assert!(!merged.fail_on_warning);
    }

    #[test]
    fn test_merge_diff_args_cli_overrides_config() {
        let mut config = ReluneConfig::default();
        config.diff.format = Some(DiffFormat::Text);
        config.diff.dialect = Some(DialectArg::Auto);

        let args = DiffArgs {
            before: None,
            before_sql_text: None,
            before_schema_json: None,
            after: None,
            after_sql_text: None,
            after_schema_json: None,
            dialect: Some(DialectArg::Sqlite),
            format: Some(DiffFormat::Json),
            out: None,
            stdout: false,
            fail_on_warning: false,
            exit_code: false,
        };

        let merged = config.merge_diff_args(&args).expect("merge should succeed");
        assert_eq!(merged.format, DiffFormat::Json);
        assert_eq!(merged.dialect, DialectArg::Sqlite);
        assert!(!merged.fail_on_warning);
    }

    #[test]
    fn test_merge_diff_args_visual_settings_from_config() {
        let mut config = ReluneConfig::default();
        config.diff.layout = Some(LayoutAlgorithmArg::ForceDirected);
        config.diff.edge_style = Some(EdgeStyleArg::Curved);
        config.diff.direction = Some(DirectionArg::LeftToRight);
        config.diff.group_by = Some(GroupByMode::Schema);
        config.diff.focus = Some("orders".to_string());
        config.diff.depth = Some(2);
        config.diff.include = vec!["orders".to_string(), "items".to_string()];
        config.diff.exclude = vec!["audit".to_string()];
        config.diff.theme = Some(Theme::Light);
        config.diff.show_legend = Some(true);
        config.diff.show_stats = Some(true);

        let args = DiffArgs {
            before: None,
            before_sql_text: None,
            before_schema_json: None,
            after: None,
            after_sql_text: None,
            after_schema_json: None,
            dialect: None,
            format: None,
            out: None,
            stdout: false,
            fail_on_warning: false,
            exit_code: false,
        };

        let merged = config.merge_diff_args(&args).expect("merge should succeed");
        assert_eq!(merged.layout, LayoutAlgorithmArg::ForceDirected);
        assert_eq!(merged.edge_style, EdgeStyleArg::Curved);
        assert_eq!(merged.direction, DirectionArg::LeftToRight);
        assert_eq!(merged.group_by, Some(GroupByMode::Schema));
        assert_eq!(merged.focus.as_deref(), Some("orders"));
        assert_eq!(merged.depth, 2);
        assert_eq!(
            merged.include,
            vec!["orders".to_string(), "items".to_string()]
        );
        assert_eq!(merged.exclude, vec!["audit".to_string()]);
        assert_eq!(merged.theme, Theme::Light);
        assert!(merged.show_legend);
        assert!(merged.show_stats);
    }

    #[test]
    fn test_merge_diff_args_viewpoint_overrides_command_focus() {
        let mut config = ReluneConfig::default();
        config.diff.focus = Some("orders".to_string());
        config.diff.depth = Some(1);
        config.diff.viewpoint = Some("billing".to_string());
        config.viewpoints.insert(
            "billing".to_string(),
            ViewpointConfig {
                group_by: Some(GroupByMode::Schema),
                focus: Some("invoices".to_string()),
                depth: Some(3),
                include: vec!["invoices".to_string(), "payments".to_string()],
                exclude: vec![],
            },
        );

        let args = DiffArgs {
            before: None,
            before_sql_text: None,
            before_schema_json: None,
            after: None,
            after_sql_text: None,
            after_schema_json: None,
            dialect: None,
            format: None,
            out: None,
            stdout: false,
            fail_on_warning: false,
            exit_code: false,
        };

        let merged = config.merge_diff_args(&args).expect("merge should succeed");
        assert_eq!(merged.focus.as_deref(), Some("invoices"));
        assert_eq!(merged.depth, 3);
        assert_eq!(merged.group_by, Some(GroupByMode::Schema));
        assert_eq!(
            merged.include,
            vec!["invoices".to_string(), "payments".to_string()]
        );
    }

    fn default_render_args() -> RenderArgs {
        RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            stdout: false,
            focus: None,
            viewpoint: None,
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            direction: None,
            stats: false,
            fail_on_warning: false,
            dialect: None,
        }
    }

    fn default_inspect_args() -> crate::cli::InspectArgs {
        crate::cli::InspectArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            table: None,
            summary: false,
            format: None,
            out: None,
            fail_on_warning: false,
            dialect: None,
        }
    }

    fn default_export_args() -> crate::cli::ExportArgs {
        crate::cli::ExportArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: Some(crate::cli::ExportFormat::SchemaJson),
            out: None,
            focus: None,
            viewpoint: None,
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            layout: None,
            edge_style: None,
            direction: None,
            fail_on_warning: false,
            dialect: None,
        }
    }

    fn default_doc_args() -> crate::cli::DocArgs {
        crate::cli::DocArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            out: None,
            fail_on_warning: false,
            dialect: None,
        }
    }

    fn default_lint_args() -> crate::cli::LintArgs {
        crate::cli::LintArgs {
            sql: None,
            db_url: None,
            schema_json: None,
            dialect: None,
            format: None,
            out: None,
            profile: None,
            rules: vec![],
            exclude_rules: vec![],
            rule_categories: vec![],
            except_tables: vec![],
            deny: None,
            fail_on_warning: false,
        }
    }

    #[test]
    fn test_merge_render_args_dialect_from_config() {
        let mut config = ReluneConfig::default();
        config.render.dialect = Some(DialectArg::Postgres);
        let merged = config
            .merge_render_args(&default_render_args())
            .expect("merge should succeed");
        assert_eq!(merged.dialect, DialectArg::Postgres);
    }

    #[test]
    fn test_merge_render_args_dialect_cli_overrides_config() {
        let mut config = ReluneConfig::default();
        config.render.dialect = Some(DialectArg::Postgres);
        let args = RenderArgs {
            dialect: Some(DialectArg::Mysql),
            ..default_render_args()
        };
        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");
        assert_eq!(merged.dialect, DialectArg::Mysql);
    }

    #[test]
    fn test_merge_inspect_args_dialect_from_config() {
        let mut config = ReluneConfig::default();
        config.inspect.dialect = Some(DialectArg::Mysql);
        let merged = config.merge_inspect_args(&default_inspect_args());
        assert_eq!(merged.dialect, DialectArg::Mysql);
    }

    #[test]
    fn test_merge_inspect_args_dialect_cli_overrides_config() {
        let mut config = ReluneConfig::default();
        config.inspect.dialect = Some(DialectArg::Mysql);
        let args = crate::cli::InspectArgs {
            dialect: Some(DialectArg::Sqlite),
            ..default_inspect_args()
        };
        let merged = config.merge_inspect_args(&args);
        assert_eq!(merged.dialect, DialectArg::Sqlite);
    }

    #[test]
    fn test_merge_export_args_dialect_from_config() {
        let mut config = ReluneConfig::default();
        config.export.dialect = Some(DialectArg::Sqlite);
        let merged = config
            .merge_export_args(&default_export_args())
            .expect("merge should succeed");
        assert_eq!(merged.dialect, DialectArg::Sqlite);
    }

    #[test]
    fn test_merge_export_args_dialect_cli_overrides_config() {
        let mut config = ReluneConfig::default();
        config.export.dialect = Some(DialectArg::Sqlite);
        let args = crate::cli::ExportArgs {
            dialect: Some(DialectArg::Postgres),
            ..default_export_args()
        };
        let merged = config
            .merge_export_args(&args)
            .expect("merge should succeed");
        assert_eq!(merged.dialect, DialectArg::Postgres);
    }

    #[test]
    fn test_merge_doc_args_dialect_from_config() {
        let mut config = ReluneConfig::default();
        config.doc.dialect = Some(DialectArg::Postgres);
        let merged = config.merge_doc_args(&default_doc_args());
        assert_eq!(merged.dialect, DialectArg::Postgres);
    }

    #[test]
    fn test_merge_doc_args_dialect_cli_overrides_config() {
        let mut config = ReluneConfig::default();
        config.doc.dialect = Some(DialectArg::Postgres);
        let args = crate::cli::DocArgs {
            dialect: Some(DialectArg::Mysql),
            ..default_doc_args()
        };
        let merged = config.merge_doc_args(&args);
        assert_eq!(merged.dialect, DialectArg::Mysql);
    }

    #[test]
    fn test_merge_lint_args_dialect_from_config() {
        let mut config = ReluneConfig::default();
        config.lint.dialect = Some(DialectArg::Mysql);
        let merged = config.merge_lint_args(&default_lint_args());
        assert_eq!(merged.dialect, DialectArg::Mysql);
    }

    #[test]
    fn test_merge_lint_args_dialect_cli_overrides_config() {
        let mut config = ReluneConfig::default();
        config.lint.dialect = Some(DialectArg::Mysql);
        let args = crate::cli::LintArgs {
            dialect: Some(DialectArg::Sqlite),
            ..default_lint_args()
        };
        let merged = config.merge_lint_args(&args);
        assert_eq!(merged.dialect, DialectArg::Sqlite);
    }

    #[test]
    fn test_merge_render_direction_cli_overrides_toml() {
        let mut config = ReluneConfig::default();
        config.render.direction = Some(DirectionArg::LeftToRight);

        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            stdout: false,
            focus: None,
            viewpoint: None,
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            direction: Some(DirectionArg::BottomToTop),
            stats: false,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");
        assert_eq!(merged.direction, DirectionArg::BottomToTop);
    }

    #[test]
    fn test_merge_render_direction_falls_back_to_toml() {
        let mut config = ReluneConfig::default();
        config.render.direction = Some(DirectionArg::LeftToRight);

        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            stdout: false,
            focus: None,
            viewpoint: None,
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            direction: None,
            stats: false,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");
        assert_eq!(merged.direction, DirectionArg::LeftToRight);
    }

    #[test]
    fn test_toml_direction_kebab_case_parses() {
        let toml = r#"
[render]
direction = "left-to-right"
"#;
        let config: ReluneConfig = toml::from_str(toml).expect("should parse kebab-case direction");
        assert_eq!(config.render.direction, Some(DirectionArg::LeftToRight));
    }

    #[test]
    fn test_merge_render_args_cli_viewpoint_overrides_config_viewpoint() {
        let mut config = ReluneConfig::default();
        config.render.viewpoint = Some("billing".to_string());
        config.viewpoints.insert(
            "billing".to_string(),
            ViewpointConfig {
                focus: Some("invoices".to_string()),
                depth: Some(2),
                group_by: Some(GroupByMode::Schema),
                include: vec!["billing_*".to_string()],
                exclude: vec!["audit_*".to_string()],
            },
        );
        config.viewpoints.insert(
            "auth".to_string(),
            ViewpointConfig {
                focus: Some("users".to_string()),
                depth: Some(1),
                group_by: Some(GroupByMode::Prefix),
                include: vec!["users".to_string(), "sessions".to_string()],
                exclude: vec!["audit_logs".to_string()],
            },
        );

        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            stdout: false,
            focus: None,
            viewpoint: Some("auth".to_string()),
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            direction: None,
            stats: false,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_render_args(&args)
            .expect("merge should succeed");
        assert_eq!(merged.focus, Some("users".to_string()));
        assert_eq!(merged.depth, 1);
        assert_eq!(merged.group_by, Some(GroupByMode::Prefix));
        assert_eq!(merged.include, vec!["users", "sessions"]);
        assert_eq!(merged.exclude, vec!["audit_logs"]);
    }

    #[test]
    fn test_merge_render_args_rejects_unknown_viewpoint() {
        let config = ReluneConfig::default();
        let args = RenderArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            stdout: false,
            focus: None,
            viewpoint: Some("missing".to_string()),
            depth: None,
            group_by: None,
            include: vec![],
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            direction: None,
            stats: false,
            fail_on_warning: false,
            dialect: None,
        };

        let error = config
            .merge_render_args(&args)
            .expect_err("unknown viewpoint should fail");
        assert!(
            error
                .to_string()
                .contains("unknown viewpoint 'missing'; no viewpoints are defined")
        );
    }

    #[test]
    fn test_merge_export_args_cli_filters_override_viewpoint() {
        let mut config = ReluneConfig::default();
        config.export.format = Some(ExportFormatConfig::SchemaJson);
        config.viewpoints.insert(
            "billing".to_string(),
            ViewpointConfig {
                focus: Some("invoices".to_string()),
                depth: Some(2),
                group_by: Some(GroupByMode::Schema),
                include: vec!["billing_*".to_string()],
                exclude: vec!["audit_*".to_string()],
            },
        );

        let args = crate::cli::ExportArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            focus: None,
            viewpoint: Some("billing".to_string()),
            depth: None,
            group_by: None,
            include: vec!["invoices".to_string()],
            exclude: vec!["billing_audit".to_string()],
            layout: None,
            edge_style: None,
            direction: None,
            fail_on_warning: false,
            dialect: None,
        };

        let merged = config
            .merge_export_args(&args)
            .expect("merge should succeed");
        assert_eq!(merged.focus, Some("invoices".to_string()));
        assert_eq!(merged.depth, 2);
        assert_eq!(merged.include, vec!["invoices"]);
        assert_eq!(merged.exclude, vec!["billing_audit"]);
    }
}
