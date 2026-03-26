//! Configuration file support for relune CLI.
//!
//! Config files are TOML format and support the following structure:
//! ```toml
//! [render]
//! format = "svg"  # svg, html, graph-json, schema-json
//! theme = "light" # light, dark
//! layout = "hierarchical" # hierarchical, force-directed
//! edge_style = "straight" # straight, orthogonal, curved
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
//!
//! [export]
//! format = "schema-json" # schema-json, graph-json, layout-json, mermaid, d2, dot
//! group_by = "none"
//! layout = "hierarchical"
//! edge_style = "straight"
//! focus = "table_name"
//! depth = 1
//!
//! [diff]
//! format = "text" # text, json
//! dialect = "auto" # auto, postgres, mysql, sqlite
//! ```
//!
//! Config layering order (later overrides earlier):
//! 1. Built-in defaults
//! 2. Config file
//! 3. CLI flags

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cli::{
    DialectArg, DiffFormat, EdgeStyleArg, GroupByMode, LayoutAlgorithmArg, RenderFormat, Theme,
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
    /// Lint command configuration.
    #[serde(default)]
    pub lint: LintConfig,
    /// Diff command configuration.
    #[serde(default)]
    pub diff: DiffConfig,
}

/// Configuration for the render command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderConfig {
    /// Output format.
    #[serde(default)]
    pub format: Option<RenderFormat>,
    /// Visual theme.
    #[serde(default)]
    pub theme: Option<Theme>,
    /// Layout algorithm.
    #[serde(default)]
    pub layout: Option<LayoutAlgorithmArg>,
    /// Edge routing style.
    #[serde(default)]
    pub edge_style: Option<EdgeStyleArg>,
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
    /// Show legend in output.
    #[serde(default)]
    pub show_legend: Option<bool>,
    /// Show statistics.
    #[serde(default)]
    pub show_stats: Option<bool>,
}

/// Configuration for the inspect command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectConfig {
    /// Output format.
    #[serde(default)]
    pub format: Option<InspectFormatConfig>,
}

/// Configuration for the export command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportConfig {
    /// Export format.
    #[serde(default)]
    pub format: Option<ExportFormatConfig>,
    /// Grouping mode.
    #[serde(default)]
    pub group_by: Option<GroupByMode>,
    /// Layout algorithm.
    #[serde(default)]
    pub layout: Option<LayoutAlgorithmArg>,
    /// Edge routing style.
    #[serde(default)]
    pub edge_style: Option<EdgeStyleArg>,
    /// Focus table name.
    #[serde(default)]
    pub focus: Option<String>,
    /// Focus depth.
    #[serde(default)]
    pub depth: Option<u32>,
}

/// Configuration for the lint command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintConfig {
    /// Output format.
    #[serde(default)]
    pub format: Option<LintFormatConfig>,
    /// Minimum severity that causes non-zero exit.
    #[serde(default)]
    pub deny: Option<LintSeverityConfig>,
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
    #[allow(dead_code)]
    InvalidValue(String),
}

impl ReluneConfig {
    /// Load configuration from a file.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration from a file, returning default if file doesn't exist.
    #[allow(dead_code)]
    pub fn from_file_or_default(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            Self::from_file(path)
        } else {
            Ok(Self::default())
        }
    }

    /// Merge CLI render args into this config.
    /// CLI args take precedence over config file values.
    pub fn merge_render_args(&self, args: &crate::cli::RenderArgs) -> MergedRenderConfig {
        MergedRenderConfig {
            format: args.format.or(self.render.format).unwrap_or_default(),
            theme: args.theme.or(self.render.theme).unwrap_or_default(),
            layout: args.layout.or(self.render.layout).unwrap_or_default(),
            edge_style: args
                .edge_style
                .or(self.render.edge_style)
                .unwrap_or_default(),
            group_by: args.group_by.or(self.render.group_by),
            focus: args.focus.clone().or_else(|| self.render.focus.clone()),
            depth: args.depth.or(self.render.depth).unwrap_or(1),
            include: if args.include.is_empty() {
                self.render.include.clone()
            } else {
                args.include.clone()
            },
            exclude: if args.exclude.is_empty() {
                self.render.exclude.clone()
            } else {
                args.exclude.clone()
            },
            show_legend: self.render.show_legend.unwrap_or(false),
            show_stats: args.stats || self.render.show_stats.unwrap_or(false),
        }
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
        }
    }

    /// Merge CLI export args into this config.
    pub fn merge_export_args(
        &self,
        args: &crate::cli::ExportArgs,
    ) -> Result<MergedExportConfig, ConfigError> {
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
            group_by: args.group_by.or(self.export.group_by),
            layout: args.layout.or(self.export.layout).unwrap_or_default(),
            edge_style: args
                .edge_style
                .or(self.export.edge_style)
                .unwrap_or_default(),
            focus: args.focus.clone().or_else(|| self.export.focus.clone()),
            depth: args.depth.or(self.export.depth).unwrap_or(1),
        })
    }

    /// Merge CLI lint args into this config.
    pub fn merge_lint_args(&self, args: &crate::cli::LintArgs) -> MergedLintConfig {
        MergedLintConfig {
            format: args
                .format
                .or_else(|| self.lint.format.map(Into::into))
                .unwrap_or_default(),
            deny: args.deny.or_else(|| self.lint.deny.map(Into::into)),
        }
    }

    /// Merge CLI diff args into this config.
    pub fn merge_diff_args(&self, args: &crate::cli::DiffArgs) -> MergedDiffConfig {
        MergedDiffConfig {
            format: args.format.or(self.diff.format).unwrap_or_default(),
            dialect: args.dialect.or(self.diff.dialect).unwrap_or_default(),
        }
    }
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
    pub theme: Theme,
    pub layout: LayoutAlgorithmArg,
    pub edge_style: EdgeStyleArg,
    pub group_by: Option<GroupByMode>,
    pub focus: Option<String>,
    pub depth: u32,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    #[allow(dead_code)]
    pub show_legend: bool,
    pub show_stats: bool,
}

/// Merged inspect configuration.
#[derive(Debug, Clone)]
pub struct MergedInspectConfig {
    pub format: crate::cli::InspectFormat,
}

/// Merged export configuration.
#[derive(Debug, Clone)]
pub struct MergedExportConfig {
    pub format: crate::cli::ExportFormat,
    pub group_by: Option<GroupByMode>,
    pub layout: LayoutAlgorithmArg,
    pub edge_style: EdgeStyleArg,
    pub focus: Option<String>,
    pub depth: u32,
}

/// Merged lint configuration.
#[derive(Debug, Clone)]
pub struct MergedLintConfig {
    pub format: crate::cli::LintFormat,
    pub deny: Option<crate::cli::LintSeverity>,
}

/// Merged diff configuration.
#[derive(Debug, Clone)]
pub struct MergedDiffConfig {
    pub format: DiffFormat,
    pub dialect: DialectArg,
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
        validate_focus_filters("export", self.focus.as_deref(), self.depth, &[], &[])
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
        assert_eq!(config.render.group_by, Some(GroupByMode::Schema));
        assert_eq!(config.render.focus, Some("users".to_string()));
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
        assert_eq!(config.export.layout, Some(LayoutAlgorithmArg::Hierarchical));
        assert_eq!(config.export.edge_style, Some(EdgeStyleArg::Curved));
        assert_eq!(config.diff.format, Some(DiffFormat::Json));
        assert_eq!(config.diff.dialect, Some(DialectArg::Postgres));
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
            depth: Some(3),                   // CLI specifies different depth
            group_by: Some(GroupByMode::Prefix), // CLI specifies different group_by
            include: vec!["a".to_string()],   // CLI specifies different include
            exclude: vec!["b".to_string()],   // CLI specifies different exclude
            theme: Some(Theme::Light),        // CLI specifies light theme
            layout: Some(LayoutAlgorithmArg::Hierarchical),
            edge_style: Some(EdgeStyleArg::Straight),
            stats: true,
            fail_on_warning: false,
            dialect: crate::cli::DialectArg::Auto,
        };

        let merged = config.merge_render_args(&args);

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
        config.render.depth = Some(5);
        config.render.group_by = Some(GroupByMode::Schema);
        config.render.include = vec!["config_include".to_string()];

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
            depth: None,
            group_by: None,  // Not specified - should use config
            include: vec![], // Empty - should use config
            exclude: vec![],
            theme: None,
            layout: None,
            edge_style: None,
            stats: false,
            fail_on_warning: false,
            dialect: crate::cli::DialectArg::Auto,
        };

        let merged = config.merge_render_args(&args);

        // Config values should be used when CLI uses defaults
        assert_eq!(merged.format, RenderFormat::Html);
        assert_eq!(merged.theme, Theme::Dark);
        assert_eq!(merged.focus, Some("config_table".to_string()));
        assert_eq!(merged.depth, 5); // Config value, not CLI default
        assert_eq!(merged.group_by, Some(GroupByMode::Schema));
        assert_eq!(merged.include, vec!["config_include"]);
    }

    #[test]
    fn test_merge_render_args_cli_explicit_overrides_config() {
        // Create config with specific values
        let mut config = ReluneConfig::default();
        config.render.focus = Some("config_table".to_string());
        config.render.depth = Some(5);
        config.render.group_by = Some(GroupByMode::Schema);
        config.render.include = vec!["config_include".to_string()];
        config.render.exclude = vec!["config_exclude".to_string()];

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
            depth: Some(10),                      // CLI explicitly specifies depth
            group_by: Some(GroupByMode::Prefix),  // CLI explicitly specifies group_by
            include: vec!["cli_include".to_string()], // CLI explicitly specifies include
            exclude: vec!["cli_exclude".to_string()], // CLI explicitly specifies exclude
            theme: Some(Theme::Dark),             // CLI explicitly specifies dark
            layout: Some(LayoutAlgorithmArg::ForceDirected),
            edge_style: Some(EdgeStyleArg::Orthogonal),
            stats: true,
            fail_on_warning: false,
            dialect: crate::cli::DialectArg::Auto,
        };

        let merged = config.merge_render_args(&args);

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
    fn test_from_file_or_default_missing_file() {
        let path = PathBuf::from("/nonexistent/path/config.toml");
        let config = ReluneConfig::from_file_or_default(&path).expect("Should return default");

        // Should be all defaults
        assert_eq!(config.render.format, None);
        assert_eq!(config.render.theme, None);
        assert_eq!(config.render.focus, None);
    }

    #[test]
    fn test_from_file_or_default_existing_file() {
        let path = fixtures_dir().join("valid_partial.toml");
        let config = ReluneConfig::from_file_or_default(&path).expect("Should load config");

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
            dialect: crate::cli::DialectArg::Auto,
        };

        let merged = config.merge_inspect_args(&args);
        assert_eq!(merged.format, InspectFormat::Json);
    }

    #[test]
    fn test_merge_export_args() {
        let mut config = ReluneConfig::default();
        config.export.format = Some(ExportFormatConfig::GraphJson);
        config.export.focus = Some("config_focus".to_string());
        config.export.depth = Some(5);
        config.export.layout = Some(LayoutAlgorithmArg::ForceDirected);
        config.export.edge_style = Some(EdgeStyleArg::Orthogonal);

        let args = crate::cli::ExportArgs {
            sql: None,
            sql_text: None,
            schema_json: None,
            db_url: None,
            format: None,
            out: None,
            focus: None, // Not specified - should use config
            depth: None,
            group_by: None,
            layout: None,
            edge_style: None,
            dialect: crate::cli::DialectArg::Auto,
        };

        let merged = config
            .merge_export_args(&args)
            .expect("export format should be resolved");
        assert_eq!(merged.format, ExportFormat::GraphJson);
        assert_eq!(merged.focus, Some("config_focus".to_string()));
        assert_eq!(merged.depth, 5);
        assert_eq!(merged.layout, LayoutAlgorithmArg::ForceDirected);
        assert_eq!(merged.edge_style, EdgeStyleArg::Orthogonal);
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
            depth: None,
            group_by: None,
            layout: None,
            edge_style: None,
            dialect: crate::cli::DialectArg::Auto,
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
    fn test_validate_render_semantics_accepts_consistent_filters() {
        let config = MergedRenderConfig {
            format: RenderFormat::Svg,
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            group_by: None,
            focus: Some("users".to_string()),
            depth: 2,
            include: vec!["users".to_string(), "posts".to_string()],
            exclude: vec!["comments".to_string()],
            show_legend: false,
            show_stats: false,
        };

        config
            .validate_semantics()
            .expect("consistent focus filters should be accepted");
    }

    #[test]
    fn test_validate_render_semantics_rejects_depth_without_focus() {
        let config = MergedRenderConfig {
            format: RenderFormat::Svg,
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            group_by: None,
            focus: None,
            depth: 2,
            include: Vec::new(),
            exclude: Vec::new(),
            show_legend: false,
            show_stats: false,
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
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            group_by: None,
            focus: Some("users".to_string()),
            depth: 1,
            include: vec!["users".to_string(), "posts".to_string()],
            exclude: vec!["posts".to_string()],
            show_legend: false,
            show_stats: false,
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
            theme: Theme::Light,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            group_by: None,
            focus: Some("users".to_string()),
            depth: 1,
            include: vec!["posts".to_string()],
            exclude: Vec::new(),
            show_legend: false,
            show_stats: false,
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
            group_by: None,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            focus: Some("   ".to_string()),
            depth: 1,
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
            group_by: None,
            layout: LayoutAlgorithmArg::Hierarchical,
            edge_style: EdgeStyleArg::Straight,
            focus: None,
            depth: 2,
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
            format: None,
            rules: vec![],
            deny: None, // Not specified - should use config
            dialect: crate::cli::DialectArg::Auto,
        };

        let merged = config.merge_lint_args(&args);
        assert_eq!(merged.format, LintFormat::Json);
        assert_eq!(merged.deny, Some(crate::cli::LintSeverity::Warning));
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
        };

        let merged = config.merge_diff_args(&args);
        assert_eq!(merged.format, DiffFormat::Json);
        assert_eq!(merged.dialect, DialectArg::Mysql);
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
        };

        let merged = config.merge_diff_args(&args);
        assert_eq!(merged.format, DiffFormat::Json);
        assert_eq!(merged.dialect, DialectArg::Sqlite);
    }
}
