//! Card geometry shared by the layout engine and the renderers.
//!
//! Layout sizes nodes and anchors edges with these values, and renderers draw
//! nodes and group labels with the same values, so changing one here moves
//! both sides together.

use relune_core::NodeKind;
use unicode_width::UnicodeWidthChar;

/// Height of the node header band.
pub const NODE_HEADER_HEIGHT: f32 = 32.0;
/// Height of one column row.
pub const NODE_COLUMN_HEIGHT: f32 = 18.0;
/// Baseline of the table name, measured from the node top.
pub const NODE_HEADER_BASELINE: f32 = 21.0;
/// Baseline of the first column row, measured from the node top.
pub const NODE_FIRST_COLUMN_BASELINE: f32 = 46.0;
/// Horizontal inset of header and column text from the node edges.
pub const NODE_TEXT_INSET: f32 = 10.0;
/// Corner radius of the node body and header.
pub const NODE_CORNER_RADIUS: f32 = 16.0;
/// Space kept between the table-name clip and the node's right edge for the
/// right-aligned kind label ("TABLE"/"VIEW"/"ENUM").
pub const NODE_KIND_LABEL_RESERVE: f32 = 44.0;

/// Font size of the table name.
pub const NODE_HEADER_FONT_SIZE: f32 = 13.0;
/// Font size of column rows.
pub const NODE_COLUMN_FONT_SIZE: f32 = 11.5;

/// Width of one PK / FK / IX badge.
pub const COLUMN_BADGE_WIDTH: f32 = 20.0;
/// Height of one PK / FK / IX badge.
pub const COLUMN_BADGE_HEIGHT: f32 = 13.0;
/// Distance between the left edges of neighbouring badges.
pub const COLUMN_BADGE_PITCH: f32 = 24.0;
/// Distance from the node's right edge to the rightmost badge's left edge.
pub const COLUMN_BADGE_RIGHT_INSET: f32 = 22.0;
/// Gap kept between column text and the leftmost badge.
pub const COLUMN_BADGE_TEXT_GAP: f32 = 6.0;

/// Horizontal inset of the group label from the group's left edge.
pub const GROUP_LABEL_INSET: f32 = 12.0;
/// Font size of the group label.
pub const GROUP_LABEL_FONT_SIZE: f32 = 11.0;
/// Letter spacing of the group label, in `em`.
pub const GROUP_LABEL_LETTER_SPACING_EM: f32 = 0.12;
/// Letter spacing of the group label, in pixels.
pub const GROUP_LABEL_LETTER_SPACING: f32 = GROUP_LABEL_FONT_SIZE * GROUP_LABEL_LETTER_SPACING_EM;

/// Horizontal space a column row reserves at its right end for `badge_count`
/// badges, including the gap before the leftmost one.
#[must_use]
#[allow(clippy::cast_precision_loss)] // Badge counts are tiny layout values.
pub fn column_badge_reserve(badge_count: usize) -> f32 {
    if badge_count == 0 {
        return 0.0;
    }
    (badge_count as f32 - 1.0).mul_add(
        COLUMN_BADGE_PITCH,
        COLUMN_BADGE_RIGHT_INSET + COLUMN_BADGE_TEXT_GAP,
    )
}

/// Text drawn for one column row: enum values get a bullet, and table or view
/// columns append their data type when it is known.
#[must_use]
pub fn column_display_text(kind: NodeKind, name: &str, data_type: &str) -> String {
    if kind == NodeKind::Enum {
        format!("• {name}")
    } else if data_type.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {data_type}")
    }
}

/// Estimates the rendered width of `text` at `font_size`.
///
/// East Asian wide characters count wider than ASCII so CJK identifiers are
/// not clipped.
#[must_use]
pub fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            let width_factor = match ch {
                'A'..='Z' => 0.72,
                'a'..='z' | '0'..='9' => 0.62,
                '_' | '-' | '.' | ':' | ',' | '(' | ')' | '[' | ']' | ' ' => 0.38,
                _ if ch.is_ascii_punctuation() => 0.52,
                _ if ch.is_ascii() => 0.62,
                _ => match ch.width_cjk().or_else(|| ch.width()) {
                    Some(0) => 0.0,
                    Some(1) => 0.94,
                    Some(_) => 1.12,
                    None => 1.0,
                },
            };
            font_size * width_factor
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_badge_reserve_grows_by_pitch() {
        assert!(column_badge_reserve(0).abs() < f32::EPSILON);
        assert!((column_badge_reserve(1) - 28.0).abs() < f32::EPSILON);
        assert!((column_badge_reserve(3) - 76.0).abs() < f32::EPSILON);
    }
}
