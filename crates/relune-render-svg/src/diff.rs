//! Diff overlay styling: change kinds and risk labels.
//!
//! A diff diagram keeps two signals apart. The change kind (added / removed /
//! modified) is shown as a `+` / `−` / `~` marker and a faint tint in the
//! kind's hue; the risk of a change is a separate `breaking 1`-style label
//! taken from the review rules. Unchanged cards keep their normal colors so
//! they still read as context.

use std::fmt::{self, Write};

use relune_core::{ChangeKind, ReviewSeverity};
use relune_layout::metrics::estimate_text_width;

use crate::escape::escape_text;
use crate::is_light_theme;
use crate::theme::ThemeColors;

/// Height of the risk label pill.
const RISK_LABEL_HEIGHT: f32 = 18.0;
/// Font size of the risk label text.
const RISK_LABEL_FONT_SIZE: f32 = 10.0;
/// Horizontal padding inside the risk label pill.
const RISK_LABEL_PADDING: f32 = 7.0;
/// Gap between the risk label and the card's right edge.
const RISK_LABEL_INSET: f32 = 8.0;

/// CSS class naming a change kind, e.g. `diff-added`.
pub(crate) const fn change_class(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "diff-added",
        ChangeKind::Removed => "diff-removed",
        ChangeKind::Modified => "diff-modified",
    }
}

/// Marker drawn before a changed row or card name.
pub(crate) const fn change_marker(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "\u{2212}",
        ChangeKind::Modified => "~",
    }
}

/// Color of a change kind's marker, outline, and tint.
pub(crate) const fn change_color(kind: ChangeKind, colors: &ThemeColors) -> &'static str {
    match kind {
        ChangeKind::Added => colors.diff.added,
        ChangeKind::Removed => colors.diff.removed,
        ChangeKind::Modified => colors.diff.modified,
    }
}

/// Opacity of the tint behind a changed row or card, light enough that the
/// text on top keeps its normal contrast.
pub(crate) const fn change_tint_opacity(colors: &ThemeColors) -> f32 {
    if is_light_theme(colors) { 0.08 } else { 0.12 }
}

/// Dash pattern for the outline of a removed card or relationship.
pub(crate) const REMOVED_DASHARRAY: &str = "5,3";

/// CSS class naming a risk level, e.g. `risk-breaking`.
pub(crate) const fn risk_class(severity: ReviewSeverity) -> &'static str {
    match severity {
        ReviewSeverity::Breaking => "risk-breaking",
        ReviewSeverity::Caution => "risk-caution",
        ReviewSeverity::Warning => "risk-warning",
        ReviewSeverity::Info => "risk-info",
    }
}

/// Fill of a risk label.
const fn risk_fill(severity: ReviewSeverity, colors: &ThemeColors) -> &'static str {
    match severity {
        ReviewSeverity::Breaking => colors.risk.breaking,
        ReviewSeverity::Caution => colors.risk.caution,
        ReviewSeverity::Warning => colors.risk.warning,
        ReviewSeverity::Info => colors.risk.info,
    }
}

/// Text of a risk label, e.g. `breaking 1`.
pub(crate) fn risk_label(severity: ReviewSeverity, count: usize) -> String {
    format!("{severity} {count}")
}

/// Draws the risk label straddling the top border, right-aligned at
/// `right_x`.
pub(crate) fn render_risk_label(
    out: &mut String,
    right_x: f32,
    top_y: f32,
    severity: ReviewSeverity,
    count: usize,
    colors: &ThemeColors,
) -> fmt::Result {
    let label = risk_label(severity, count);
    let width = RISK_LABEL_PADDING.mul_add(2.0, estimate_text_width(&label, RISK_LABEL_FONT_SIZE));
    let x = right_x - RISK_LABEL_INSET - width;
    let y = top_y - RISK_LABEL_HEIGHT / 2.0;
    let (fill, text) = (risk_fill(severity, colors), colors.risk.text);
    write!(
        out,
        r#"<g class="risk-label {}"><rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{RISK_LABEL_HEIGHT}" rx="{:.1}" fill="{fill}"/><text x="{:.1}" y="{:.1}" font-family="'Inter', system-ui, sans-serif" font-size="{RISK_LABEL_FONT_SIZE}" font-weight="700" text-anchor="middle" fill="{text}">{}</text></g>"#,
        risk_class(severity),
        RISK_LABEL_HEIGHT / 2.0,
        x + width / 2.0,
        y + 12.5,
        escape_text(&label),
    )
}

/// Draws a change marker centered at (`center_x`, `baseline`).
pub(crate) fn render_change_marker(
    out: &mut String,
    center_x: f32,
    baseline: f32,
    font_size: f32,
    kind: ChangeKind,
    colors: &ThemeColors,
) -> fmt::Result {
    write!(
        out,
        r#"<text class="diff-marker" x="{center_x:.1}" y="{baseline:.1}" text-anchor="middle" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{font_size}" font-weight="700" fill="{}">{}</text>"#,
        change_color(kind, colors),
        change_marker(kind),
    )
}

/// Draws a faint tint of a change kind's hue over a rectangle.
pub(crate) fn render_change_tint(
    out: &mut String,
    rect: (f32, f32, f32, f32),
    clip_path: Option<&str>,
    kind: ChangeKind,
    colors: &ThemeColors,
) -> fmt::Result {
    let (x, y, width, height) = rect;
    write!(
        out,
        r#"<rect class="diff-tint" x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}""#
    )?;
    if let Some(clip_path) = clip_path {
        write!(out, r#" clip-path="url(#{clip_path})""#)?;
    }
    write!(
        out,
        r#" fill="{}" fill-opacity="{}"/>"#,
        change_color(kind, colors),
        change_tint_opacity(colors),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_label_names_the_severity_and_count() {
        assert_eq!(risk_label(ReviewSeverity::Breaking, 1), "breaking 1");
        assert_eq!(risk_label(ReviewSeverity::Info, 3), "info 3");
    }
}
