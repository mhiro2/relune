//! HTML rendering options.

use serde::{Deserialize, Serialize};

/// Visual theme for HTML output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    /// Light theme with white background.
    #[default]
    Light,
    /// Dark theme with dark background.
    Dark,
}

/// Options for HTML rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct HtmlRenderOptions {
    /// Visual theme (light or dark).
    pub theme: Theme,

    /// Optional title for the HTML document.
    /// If provided, appears in both the `title` tag and as an H1 heading.
    pub title: Option<String>,

    /// Whether to include a legend/stats section.
    /// Default: false (not implemented in MVP)
    pub include_legend: bool,

    /// Whether to enable pan/zoom interaction.
    /// Default: true
    pub enable_pan_zoom: bool,

    /// Whether to enable search functionality for filtering/highlighting tables.
    /// Default: true
    pub enable_search: bool,

    /// When search UI is enabled, whether to show column data-type filters in the same panel.
    /// Default: true
    pub enable_column_type_filter: bool,

    /// Whether to enable group toggle UI.
    /// When enabled, shows a panel with checkboxes to show/hide groups.
    /// Default: true
    pub enable_group_toggles: bool,

    /// Whether to enable table collapse/expand functionality.
    /// When enabled, allows clicking on table headers to collapse/expand column lists.
    /// Default: true
    pub enable_collapse: bool,

    /// Whether to enable neighbor highlighting on hover/click.
    /// When enabled, highlights connected tables when hovering or clicking on a node.
    /// Default: true
    pub enable_highlight: bool,
}

impl Default for HtmlRenderOptions {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            title: None,
            include_legend: false,
            enable_pan_zoom: true,
            enable_search: true,
            enable_column_type_filter: true,
            enable_group_toggles: true,
            enable_collapse: true,
            enable_highlight: true,
        }
    }
}

impl HtmlRenderOptions {
    /// Create new options with dark theme.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            theme: Theme::Dark,
            title: None,
            include_legend: false,
            enable_pan_zoom: true,
            enable_search: true,
            enable_column_type_filter: true,
            enable_group_toggles: true,
            enable_collapse: true,
            enable_highlight: true,
        }
    }

    /// Create new options with light theme.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            theme: Theme::Light,
            title: None,
            include_legend: false,
            enable_pan_zoom: true,
            enable_search: true,
            enable_column_type_filter: true,
            enable_group_toggles: true,
            enable_collapse: true,
            enable_highlight: true,
        }
    }

    /// Set the title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the theme.
    #[must_use]
    pub const fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Enable or disable search.
    #[must_use]
    pub const fn with_search(mut self, enable: bool) -> Self {
        self.enable_search = enable;
        self
    }

    /// Enable or disable the column type filter block (requires search UI).
    #[must_use]
    pub const fn with_column_type_filter(mut self, enable: bool) -> Self {
        self.enable_column_type_filter = enable;
        self
    }

    /// Enable or disable group toggles.
    #[must_use]
    pub const fn with_group_toggles(mut self, enable: bool) -> Self {
        self.enable_group_toggles = enable;
        self
    }

    /// Enable or disable collapse functionality.
    #[must_use]
    pub const fn with_collapse(mut self, enable: bool) -> Self {
        self.enable_collapse = enable;
        self
    }

    /// Enable or disable highlight functionality.
    #[must_use]
    pub const fn with_highlight(mut self, enable: bool) -> Self {
        self.enable_highlight = enable;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let options = HtmlRenderOptions::default();
        assert_eq!(options.theme, Theme::Light);
        assert!(options.title.is_none());
        assert!(!options.include_legend);
        assert!(options.enable_pan_zoom);
        assert!(options.enable_search);
        assert!(options.enable_column_type_filter);
        assert!(options.enable_group_toggles);
        assert!(options.enable_collapse);
        assert!(options.enable_highlight);
    }

    #[test]
    fn test_dark_preset() {
        let options = HtmlRenderOptions::dark();
        assert_eq!(options.theme, Theme::Dark);
    }

    #[test]
    fn test_light_preset() {
        let options = HtmlRenderOptions::light();
        assert_eq!(options.theme, Theme::Light);
    }

    #[test]
    fn test_builder_pattern() {
        let options = HtmlRenderOptions::default()
            .with_title("Test Schema")
            .with_theme(Theme::Dark);

        assert_eq!(options.title, Some("Test Schema".to_string()));
        assert_eq!(options.theme, Theme::Dark);
    }

    #[test]
    fn test_with_group_toggles() {
        let options = HtmlRenderOptions::default().with_group_toggles(false);
        assert!(!options.enable_group_toggles);

        let options = HtmlRenderOptions::default().with_group_toggles(true);
        assert!(options.enable_group_toggles);
    }

    #[test]
    fn test_with_collapse() {
        let options = HtmlRenderOptions::default().with_collapse(false);
        assert!(!options.enable_collapse);

        let options = HtmlRenderOptions::default().with_collapse(true);
        assert!(options.enable_collapse);
    }

    #[test]
    fn test_with_search() {
        let options = HtmlRenderOptions::default().with_search(false);
        assert!(!options.enable_search);

        let options = HtmlRenderOptions::default().with_search(true);
        assert!(options.enable_search);
    }

    #[test]
    fn test_with_column_type_filter() {
        let options = HtmlRenderOptions::default().with_column_type_filter(false);
        assert!(!options.enable_column_type_filter);

        let options = HtmlRenderOptions::default().with_column_type_filter(true);
        assert!(options.enable_column_type_filter);
    }

    #[test]
    fn test_with_highlight() {
        let options = HtmlRenderOptions::default().with_highlight(false);
        assert!(!options.enable_highlight);

        let options = HtmlRenderOptions::default().with_highlight(true);
        assert!(options.enable_highlight);
    }
}
