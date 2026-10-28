use serde::{Deserialize, Serialize};

use crate::theme::Theme;

/// Options for configuring SVG rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvgRenderOptions {
    /// Color theme for the SVG output.
    #[serde(default)]
    pub theme: Theme,

    /// Whether to include a legend in the output.
    #[serde(default)]
    pub show_legend: bool,

    /// Whether to show tooltips on hover for nodes and edges.
    /// Tooltips display metadata like table name, column count, and foreign key info.
    #[serde(default)]
    pub show_tooltips: bool,

    /// Whether edge labels are drawn above every line and card, so no line
    /// cuts through a label. Off by default, which keeps each label inside
    /// its edge group, where the HTML viewer's hover and selection styles
    /// reach it; standalone SVG output turns it on.
    #[serde(default)]
    pub labels_on_top: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = SvgRenderOptions::default();
        assert_eq!(opts.theme, Theme::Dark);
        assert!(!opts.show_legend);
        assert!(!opts.show_tooltips);
        assert!(!opts.labels_on_top);
    }

    #[test]
    fn test_options_serialization() {
        let opts = SvgRenderOptions {
            theme: Theme::Light,
            show_legend: true,
            show_tooltips: true,
            labels_on_top: true,
        };
        let json = serde_json::to_string(&opts).unwrap();
        assert!(json.contains("\"theme\":\"light\""));
        assert!(json.contains("\"show_legend\":true"));
        assert!(json.contains("\"show_tooltips\":true"));
    }

    #[test]
    fn test_options_deserialization() {
        let json = r#"{"theme":"dark","show_legend":false,"show_tooltips":false}"#;
        let opts: SvgRenderOptions = serde_json::from_str(json).unwrap();
        assert_eq!(opts.theme, Theme::Dark);
        assert!(!opts.show_legend);
        assert!(!opts.show_tooltips);
    }

    #[test]
    fn test_options_deserialization_partial() {
        // Test that missing fields get defaults
        let json = r"{}";
        let opts: SvgRenderOptions = serde_json::from_str(json).unwrap();
        assert_eq!(opts.theme, Theme::Dark); // Default theme
        assert!(!opts.show_legend);
        assert!(!opts.show_tooltips);
    }
}
