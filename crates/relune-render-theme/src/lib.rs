//! Shared theme definitions for Relune renderers.

use serde::{Deserialize, Serialize};

/// Theme selection for render output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Dark theme with dark background.
    #[default]
    Dark,
    /// Light theme with white background.
    Light,
}

/// Color palette for a specific theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeColors {
    /// Background color for the canvas.
    pub background: &'static str,
    /// Primary foreground/text color.
    pub foreground: &'static str,
    /// Node background fill color.
    pub node_fill: &'static str,
    /// Node border stroke color.
    pub node_stroke: &'static str,
    /// Node header background color.
    pub header_fill: &'static str,
    /// Primary text color.
    pub text_primary: &'static str,
    /// Secondary text color.
    pub text_secondary: &'static str,
    /// Muted text color.
    pub text_muted: &'static str,
    /// Edge stroke color.
    pub edge_stroke: &'static str,
    /// Arrow marker color.
    pub arrow_fill: &'static str,
}

/// Returns the color palette for the given theme.
#[must_use]
pub const fn get_colors(theme: Theme) -> ThemeColors {
    match theme {
        Theme::Dark => ThemeColors {
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
        },
        Theme::Light => ThemeColors {
            background: "#ffffff",
            foreground: "#1e293b",
            node_fill: "#f8fafc",
            node_stroke: "#cbd5e1",
            header_fill: "#f1f5f9",
            text_primary: "#1e293b",
            text_secondary: "#475569",
            text_muted: "#64748b",
            edge_stroke: "#94a3b8",
            arrow_fill: "#94a3b8",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark() {
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn dark_theme_colors_match_expected_palette() {
        let colors = get_colors(Theme::Dark);
        assert_eq!(colors.background, "#0f172a");
        assert_eq!(colors.node_fill, "#111827");
        assert_eq!(colors.header_fill, "#1e293b");
        assert_eq!(colors.text_primary, "#e2e8f0");
    }

    #[test]
    fn light_theme_colors_match_expected_palette() {
        let colors = get_colors(Theme::Light);
        assert_eq!(colors.background, "#ffffff");
        assert_eq!(colors.node_fill, "#f8fafc");
        assert_eq!(colors.header_fill, "#f1f5f9");
        assert_eq!(colors.text_primary, "#1e293b");
    }

    #[test]
    fn theme_serializes_to_lowercase() {
        let dark_json = serde_json::to_string(&Theme::Dark).unwrap();
        let light_json = serde_json::to_string(&Theme::Light).unwrap();

        assert_eq!(dark_json, "\"dark\"");
        assert_eq!(light_json, "\"light\"");
    }

    #[test]
    fn theme_deserializes_from_lowercase() {
        let dark: Theme = serde_json::from_str("\"dark\"").unwrap();
        let light: Theme = serde_json::from_str("\"light\"").unwrap();

        assert_eq!(dark, Theme::Dark);
        assert_eq!(light, Theme::Light);
    }
}
