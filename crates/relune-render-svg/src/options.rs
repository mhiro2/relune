use serde::{Deserialize, Serialize};

use crate::theme::Theme;

/// Options for configuring SVG rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct SvgRenderOptions {
    /// Color theme for the SVG output.
    #[serde(default)]
    pub theme: Theme,

    /// Whether to include a legend in the output.
    #[serde(default)]
    pub show_legend: bool,

    /// Whether to include statistics in the output.
    #[serde(default)]
    pub show_stats: bool,

    /// Whether to embed CSS styles directly in the SVG.
    /// If false, CSS classes are used instead.
    #[serde(default = "default_embed_css")]
    pub embed_css: bool,

    /// Whether to use compact output (reduced whitespace).
    #[serde(default)]
    pub compact: bool,

    /// Whether to show tooltips on hover for nodes and edges.
    /// Tooltips display metadata like table name, column count, and foreign key info.
    #[serde(default)]
    pub show_tooltips: bool,
}

const fn default_embed_css() -> bool {
    true
}

impl Default for SvgRenderOptions {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            show_legend: false,
            show_stats: false,
            embed_css: true,
            compact: false,
            show_tooltips: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = SvgRenderOptions::default();
        assert_eq!(opts.theme, Theme::Dark);
        assert!(!opts.show_legend);
        assert!(!opts.show_stats);
        assert!(opts.embed_css);
        assert!(!opts.compact);
        assert!(!opts.show_tooltips);
    }

    #[test]
    fn test_default_theme() {
        let opts = SvgRenderOptions::default();
        assert_eq!(opts.theme, Theme::Dark);
    }

    #[test]
    fn test_default_show_legend() {
        let opts = SvgRenderOptions::default();
        assert!(!opts.show_legend);
    }

    #[test]
    fn test_default_show_stats() {
        let opts = SvgRenderOptions::default();
        assert!(!opts.show_stats);
    }

    #[test]
    fn test_default_embed_css() {
        let opts = SvgRenderOptions::default();
        assert!(opts.embed_css);
    }

    #[test]
    fn test_default_compact() {
        let opts = SvgRenderOptions::default();
        assert!(!opts.compact);
    }

    #[test]
    fn test_default_show_tooltips() {
        let opts = SvgRenderOptions::default();
        assert!(!opts.show_tooltips);
    }

    #[test]
    fn test_options_with_light_theme() {
        let opts = SvgRenderOptions {
            theme: Theme::Light,
            ..Default::default()
        };
        assert_eq!(opts.theme, Theme::Light);
        assert!(opts.embed_css); // Should still have default for other fields
    }

    #[test]
    fn test_options_with_legend() {
        let opts = SvgRenderOptions {
            show_legend: true,
            ..Default::default()
        };
        assert!(opts.show_legend);
    }

    #[test]
    fn test_options_with_stats() {
        let opts = SvgRenderOptions {
            show_stats: true,
            ..Default::default()
        };
        assert!(opts.show_stats);
    }

    #[test]
    fn test_options_without_embed_css() {
        let opts = SvgRenderOptions {
            embed_css: false,
            ..Default::default()
        };
        assert!(!opts.embed_css);
    }

    #[test]
    fn test_options_with_compact() {
        let opts = SvgRenderOptions {
            compact: true,
            ..Default::default()
        };
        assert!(opts.compact);
    }

    #[test]
    fn test_options_all_custom() {
        let opts = SvgRenderOptions {
            theme: Theme::Light,
            show_legend: true,
            show_stats: true,
            embed_css: false,
            compact: true,
            show_tooltips: true,
        };
        assert_eq!(opts.theme, Theme::Light);
        assert!(opts.show_legend);
        assert!(opts.show_stats);
        assert!(!opts.embed_css);
        assert!(opts.compact);
        assert!(opts.show_tooltips);
    }

    #[test]
    fn test_options_clone() {
        let opts = SvgRenderOptions {
            theme: Theme::Light,
            show_legend: true,
            show_stats: false,
            embed_css: true,
            compact: false,
            show_tooltips: true,
        };
        let cloned = opts;
        assert_eq!(opts, cloned);
    }

    #[test]
    fn test_options_debug() {
        let opts = SvgRenderOptions::default();
        let debug_str = format!("{opts:?}");
        assert!(debug_str.contains("SvgRenderOptions"));
        assert!(debug_str.contains("theme"));
        assert!(debug_str.contains("show_legend"));
    }

    #[test]
    fn test_options_serialization() {
        let opts = SvgRenderOptions {
            theme: Theme::Light,
            show_legend: true,
            show_stats: false,
            embed_css: true,
            compact: false,
            show_tooltips: true,
        };
        let json = serde_json::to_string(&opts).unwrap();
        assert!(json.contains("\"theme\":\"light\""));
        assert!(json.contains("\"show_legend\":true"));
        assert!(json.contains("\"show_tooltips\":true"));
    }

    #[test]
    fn test_options_deserialization() {
        let json = r#"{"theme":"dark","show_legend":false,"show_stats":false,"embed_css":true,"compact":false,"show_tooltips":false}"#;
        let opts: SvgRenderOptions = serde_json::from_str(json).unwrap();
        assert_eq!(opts.theme, Theme::Dark);
        assert!(!opts.show_legend);
        assert!(!opts.show_stats);
        assert!(opts.embed_css);
        assert!(!opts.compact);
        assert!(!opts.show_tooltips);
    }

    #[test]
    fn test_options_deserialization_partial() {
        // Test that missing fields get defaults
        let json = r"{}";
        let opts: SvgRenderOptions = serde_json::from_str(json).unwrap();
        assert_eq!(opts.theme, Theme::Dark); // Default theme
        assert!(!opts.show_legend);
        assert!(!opts.show_stats);
        assert!(opts.embed_css);
        assert!(!opts.compact);
        assert!(!opts.show_tooltips);
    }

    #[test]
    fn test_options_with_tooltips() {
        let opts = SvgRenderOptions {
            show_tooltips: true,
            ..Default::default()
        };
        assert!(opts.show_tooltips);
    }
}
