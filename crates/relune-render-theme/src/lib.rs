//! Shared theme definitions for Relune renderers.

mod xml_escape;

use serde::{Deserialize, Serialize};

pub use xml_escape::{escape_xml_attribute, escape_xml_text};

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
    /// Base canvas color used by SVG patterns.
    pub canvas_base: &'static str,
    /// Dot color used by SVG patterns.
    pub canvas_dot: &'static str,
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
    /// Soft shadow color used under node cards.
    pub node_shadow: &'static str,
    /// Group background fill.
    pub group_fill: &'static str,
    /// Group accent band fill.
    pub group_band_fill: &'static str,
    /// Group border stroke.
    pub group_stroke: &'static str,
    /// Accent glow color used for hover/highlight effects on edges and nodes.
    pub glow_color: &'static str,
    /// Secondary glow particle color (slightly lighter than `glow_color`).
    pub glow_particle: &'static str,
}

/// Returns the color palette for the given theme.
#[must_use]
pub const fn get_colors(theme: Theme) -> ThemeColors {
    match theme {
        Theme::Dark => ThemeColors {
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
        },
        Theme::Light => ThemeColors {
            background: "#f7f8fc",
            canvas_base: "#f7f8fc",
            canvas_dot: "#e8eaf0",
            foreground: "#1e293b",
            node_fill: "#f8fafc",
            node_stroke: "#cbd5e1",
            header_fill: "#f1f5f9",
            text_primary: "#1e293b",
            text_secondary: "#475569",
            text_muted: "#64748b",
            edge_stroke: "#94a3b8",
            arrow_fill: "#94a3b8",
            node_shadow: "rgba(15, 23, 42, 0.08)",
            group_fill: "#ffffffd9",
            group_band_fill: "#eef2ff",
            group_stroke: "#cbd5e1",
            glow_color: "#d97706",
            glow_particle: "#f59e0b",
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
        assert_eq!(colors.background, "#0c0f1a");
        assert_eq!(colors.canvas_base, "#0c0f1a");
        assert_eq!(colors.canvas_dot, "#151928");
        assert_eq!(colors.node_fill, "#111827");
        assert_eq!(colors.header_fill, "#1e293b");
        assert_eq!(colors.text_primary, "#e2e8f0");
    }

    #[test]
    fn light_theme_colors_match_expected_palette() {
        let colors = get_colors(Theme::Light);
        assert_eq!(colors.background, "#f7f8fc");
        assert_eq!(colors.canvas_base, "#f7f8fc");
        assert_eq!(colors.canvas_dot, "#e8eaf0");
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
