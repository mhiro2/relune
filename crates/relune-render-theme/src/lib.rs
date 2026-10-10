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

/// Colors of the diff change kinds: markers, outlines, and faint tints.
///
/// Each color keeps AA text contrast against the card surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffColors {
    /// Added tables, columns, and relationships.
    pub added: &'static str,
    /// Removed tables, columns, and relationships.
    pub removed: &'static str,
    /// Modified tables and columns.
    pub modified: &'static str,
}

/// Solid fills of the review risk labels, with one text color for all.
///
/// Solid fills keep risk labels apart from the faint change tints; every
/// fill meets AA contrast with `text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskColors {
    /// `breaking` findings.
    pub breaking: &'static str,
    /// `caution` findings.
    pub caution: &'static str,
    /// `warning` findings.
    pub warning: &'static str,
    /// `info` findings.
    pub info: &'static str,
    /// Label text drawn on every fill.
    pub text: &'static str,
}

/// Colors of one PK / FK / IX column badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BadgeColor {
    /// Chip fill, drawn at `fill_opacity_percent`.
    pub fill: &'static str,
    /// Opacity of the chip fill, in percent.
    pub fill_opacity_percent: u8,
    /// Label text color.
    pub text: &'static str,
}

/// Colors of the column badges shared by node cards and the viewer.
///
/// PK is a solid neutral chip, FK a blue tint, IX a quiet grey tint, so the
/// badges step down in weight without borrowing kind or state hues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// Holds no strings itself, so serde cannot infer the `'static` bound.
#[serde(bound(deserialize = "'de: 'static"))]
pub struct BadgeColors {
    /// Primary key badge.
    pub primary_key: BadgeColor,
    /// Foreign key badge.
    pub foreign_key: BadgeColor,
    /// Index badge.
    pub index: BadgeColor,
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
    /// Faint surface of a group, kept apart from the canvas and the cards.
    pub group_fill: &'static str,
    /// Group boundary, drawn only where membership would be ambiguous.
    pub group_stroke: &'static str,
    /// Accent color for viewer controls and edge hover feedback.
    pub accent_color: &'static str,
    /// Blue-grey outline for selected and highlighted diagram elements, kept
    /// apart from kind marks and review severities.
    pub selection_color: &'static str,
    /// Column badge colors.
    pub badges: BadgeColors,
    /// Diff change kind colors.
    pub diff: DiffColors,
    /// Review risk label colors.
    pub risk: RiskColors,
    /// Whether this is a light theme (used for conditional rendering).
    pub is_light: bool,
}

/// Returns the color palette for the given theme.
#[must_use]
#[allow(clippy::too_many_lines)] // Two literal palettes read best side by side.
pub const fn get_colors(theme: Theme) -> ThemeColors {
    match theme {
        Theme::Dark => ThemeColors {
            background: "#0c0f1a",
            canvas_base: "#0c0f1a",
            canvas_dot: "#151928",
            foreground: "#e2e8f0",
            node_fill: "#161b26",
            node_stroke: "#566175",
            header_fill: "#1c2230",
            text_primary: "#e2e8f0",
            text_secondary: "#cbd5e1",
            text_muted: "#94a3b8",
            edge_stroke: "#56627a",
            arrow_fill: "#56627a",
            group_fill: "#121725",
            group_stroke: "#475569",
            accent_color: "#f59e0b",
            selection_color: "#93a8c9",
            badges: BadgeColors {
                primary_key: BadgeColor {
                    fill: "#cbd5e1",
                    fill_opacity_percent: 100,
                    text: "#0f172a",
                },
                foreign_key: BadgeColor {
                    fill: "#38bdf8",
                    fill_opacity_percent: 18,
                    text: "#7dd3fc",
                },
                index: BadgeColor {
                    fill: "#94a3b8",
                    fill_opacity_percent: 16,
                    text: "#cbd5e1",
                },
            },
            diff: DiffColors {
                added: "#4ade80",
                removed: "#f87171",
                modified: "#facc15",
            },
            risk: RiskColors {
                breaking: "#f87171",
                caution: "#fb923c",
                warning: "#facc15",
                info: "#94a3b8",
                text: "#0c0f1a",
            },
            is_light: false,
        },
        Theme::Light => ThemeColors {
            background: "#f7f8fc",
            canvas_base: "#f7f8fc",
            canvas_dot: "#e8eaf0",
            foreground: "#1e293b",
            node_fill: "#ffffff",
            node_stroke: "#7b879a",
            header_fill: "#f8fafc",
            text_primary: "#1e293b",
            text_secondary: "#334155",
            text_muted: "#64748b",
            edge_stroke: "#8390a3",
            arrow_fill: "#8390a3",
            group_fill: "#eef1f6",
            group_stroke: "#a3afbf",
            accent_color: "#d97706",
            selection_color: "#4a6285",
            badges: BadgeColors {
                primary_key: BadgeColor {
                    fill: "#334155",
                    fill_opacity_percent: 100,
                    text: "#ffffff",
                },
                foreign_key: BadgeColor {
                    fill: "#0284c7",
                    fill_opacity_percent: 12,
                    text: "#075985",
                },
                index: BadgeColor {
                    fill: "#64748b",
                    fill_opacity_percent: 12,
                    text: "#334155",
                },
            },
            diff: DiffColors {
                added: "#15803d",
                removed: "#b91c1c",
                modified: "#a16207",
            },
            risk: RiskColors {
                breaking: "#b91c1c",
                caution: "#c2410c",
                warning: "#a16207",
                info: "#475569",
                text: "#ffffff",
            },
            is_light: true,
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
        assert_eq!(colors.node_fill, "#161b26");
        assert_eq!(colors.header_fill, "#1c2230");
        assert_eq!(colors.text_primary, "#e2e8f0");
    }

    #[test]
    fn light_theme_colors_match_expected_palette() {
        let colors = get_colors(Theme::Light);
        assert_eq!(colors.background, "#f7f8fc");
        assert_eq!(colors.canvas_base, "#f7f8fc");
        assert_eq!(colors.canvas_dot, "#e8eaf0");
        assert_eq!(colors.node_fill, "#ffffff");
        assert_eq!(colors.header_fill, "#f8fafc");
        assert_eq!(colors.text_primary, "#1e293b");
    }

    fn relative_luminance(hex: &str) -> f64 {
        let channel = |index: usize| {
            let value = f64::from(u8::from_str_radix(&hex[index..index + 2], 16).unwrap()) / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.0722f64.mul_add(
            channel(5),
            0.2126f64.mul_add(channel(1), 0.7152 * channel(3)),
        )
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (la, lb) = (relative_luminance(a), relative_luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn diff_colors_meet_text_contrast_on_cards() {
        for theme in [Theme::Light, Theme::Dark] {
            let colors = get_colors(theme);
            for color in [colors.diff.added, colors.diff.removed, colors.diff.modified] {
                let ratio = contrast(color, colors.node_fill);
                assert!(ratio >= 4.5, "{theme:?} {color}: {ratio:.2}");
            }
        }
    }

    /// Hex color of `fill` drawn at `percent` opacity over `base`.
    fn blend(fill: &str, percent: u8, base: &str) -> String {
        use std::fmt::Write;

        let alpha = f64::from(percent) / 100.0;
        let mut out = String::from("#");
        for index in [1, 3, 5] {
            let channel =
                |hex: &str| f64::from(u8::from_str_radix(&hex[index..index + 2], 16).unwrap());
            let value = alpha.mul_add(channel(fill), (1.0 - alpha) * channel(base));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let value = value.round() as u8;
            write!(out, "{value:02x}").unwrap();
        }
        out
    }

    #[test]
    fn group_labels_meet_text_contrast_on_group_surfaces() {
        for theme in [Theme::Light, Theme::Dark] {
            let colors = get_colors(theme);
            let ratio = contrast(colors.text_secondary, colors.group_fill);
            assert!(ratio >= 4.5, "{theme:?}: {ratio:.2}");
        }
    }

    #[test]
    fn column_badges_meet_text_contrast_on_cards() {
        for theme in [Theme::Light, Theme::Dark] {
            let colors = get_colors(theme);
            let badges = colors.badges;
            for badge in [badges.primary_key, badges.foreign_key, badges.index] {
                let chip = blend(badge.fill, badge.fill_opacity_percent, colors.node_fill);
                let ratio = contrast(badge.text, &chip);
                assert!(ratio >= 4.5, "{theme:?} {badge:?}: {ratio:.2}");
            }
        }
    }

    #[test]
    fn risk_labels_meet_text_contrast() {
        for theme in [Theme::Light, Theme::Dark] {
            let colors = get_colors(theme);
            let risk = &colors.risk;
            for fill in [risk.breaking, risk.caution, risk.warning, risk.info] {
                let ratio = contrast(fill, risk.text);
                assert!(ratio >= 4.5, "{theme:?} {fill}: {ratio:.2}");
            }
        }
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
