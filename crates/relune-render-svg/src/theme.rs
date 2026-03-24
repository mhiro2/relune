use serde::{Deserialize, Serialize};

/// Theme selection for SVG rendering.
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
    /// Background color for the SVG canvas.
    pub background: &'static str,
    /// Primary foreground/text color.
    pub foreground: &'static str,
    /// Node background fill color.
    pub node_fill: &'static str,
    /// Node border stroke color.
    pub node_stroke: &'static str,
    /// Node header background color.
    pub header_fill: &'static str,
    /// Primary text color (used for headers).
    pub text_primary: &'static str,
    /// Secondary text color (used for columns).
    pub text_secondary: &'static str,
    /// Muted text color (used for edge labels).
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
    fn test_default_theme_is_dark() {
        let theme = Theme::default();
        assert_eq!(theme, Theme::Dark);
    }

    #[test]
    fn test_dark_theme_colors() {
        let colors = get_colors(Theme::Dark);
        assert_eq!(colors.background, "#0f172a");
        assert_eq!(colors.node_fill, "#111827");
        assert_eq!(colors.header_fill, "#1e293b");
        assert_eq!(colors.text_primary, "#e2e8f0");
    }

    #[test]
    fn test_light_theme_colors() {
        let colors = get_colors(Theme::Light);
        assert_eq!(colors.background, "#ffffff");
        assert_eq!(colors.node_fill, "#f8fafc");
        assert_eq!(colors.header_fill, "#f1f5f9");
        assert_eq!(colors.text_primary, "#1e293b");
    }

    #[test]
    fn test_get_colors_dark_theme() {
        let colors = get_colors(Theme::Dark);

        // Verify all dark theme colors are set
        assert_eq!(colors.background, "#0f172a");
        assert_eq!(colors.foreground, "#e2e8f0");
        assert_eq!(colors.node_fill, "#111827");
        assert_eq!(colors.node_stroke, "#334155");
        assert_eq!(colors.header_fill, "#1e293b");
        assert_eq!(colors.text_primary, "#e2e8f0");
        assert_eq!(colors.text_secondary, "#cbd5e1");
        assert_eq!(colors.text_muted, "#94a3b8");
        assert_eq!(colors.edge_stroke, "#64748b");
        assert_eq!(colors.arrow_fill, "#64748b");
    }

    #[test]
    fn test_get_colors_light_theme() {
        let colors = get_colors(Theme::Light);

        // Verify all light theme colors are set
        assert_eq!(colors.background, "#ffffff");
        assert_eq!(colors.foreground, "#1e293b");
        assert_eq!(colors.node_fill, "#f8fafc");
        assert_eq!(colors.node_stroke, "#cbd5e1");
        assert_eq!(colors.header_fill, "#f1f5f9");
        assert_eq!(colors.text_primary, "#1e293b");
        assert_eq!(colors.text_secondary, "#475569");
        assert_eq!(colors.text_muted, "#64748b");
        assert_eq!(colors.edge_stroke, "#94a3b8");
        assert_eq!(colors.arrow_fill, "#94a3b8");
    }

    #[test]
    fn test_dark_theme_has_dark_background() {
        let colors = get_colors(Theme::Dark);
        // Dark theme should have a dark background (starts with #0 or #1 typically)
        assert!(
            colors.background.starts_with("#0") || colors.background.starts_with("#1"),
            "Dark theme background should be dark"
        );
        // Light text on dark background
        assert!(
            colors.text_primary.starts_with("#e")
                || colors.text_primary.starts_with("#f")
                || colors.text_primary.starts_with("#c")
                || colors.text_primary.starts_with("#d"),
            "Dark theme text should be light"
        );
    }

    #[test]
    fn test_light_theme_has_light_background() {
        let colors = get_colors(Theme::Light);
        // Light theme should have a light background
        assert!(
            colors.background.starts_with("#f") || colors.background.starts_with("#e"),
            "Light theme background should be light"
        );
        // Dark text on light background
        assert!(
            colors.text_primary.starts_with("#1") || colors.text_primary.starts_with("#2"),
            "Light theme text should be dark"
        );
    }

    #[test]
    fn test_theme_serialization() {
        // Test that themes serialize to lowercase
        let dark_json = serde_json::to_string(&Theme::Dark).unwrap();
        assert_eq!(dark_json, "\"dark\"");

        let light_json = serde_json::to_string(&Theme::Light).unwrap();
        assert_eq!(light_json, "\"light\"");
    }

    #[test]
    fn test_theme_deserialization() {
        // Test that themes deserialize from lowercase
        let dark: Theme = serde_json::from_str("\"dark\"").unwrap();
        assert_eq!(dark, Theme::Dark);

        let light: Theme = serde_json::from_str("\"light\"").unwrap();
        assert_eq!(light, Theme::Light);
    }

    #[test]
    fn test_theme_colors_debug() {
        let colors = get_colors(Theme::Dark);
        // ThemeColors should implement Debug
        let debug_str = format!("{colors:?}");
        assert!(debug_str.contains("ThemeColors"));
        assert!(debug_str.contains("background"));
    }

    #[test]
    fn test_theme_colors_clone() {
        let colors = get_colors(Theme::Dark);
        let cloned = colors.clone();
        assert_eq!(colors, cloned);
    }
}
