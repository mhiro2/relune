//! Card geometry shared by the layout engine and the renderers.
//!
//! Layout sizes nodes and anchors edges with these values, and renderers draw
//! nodes and group labels with the same values, so changing one here moves
//! both sides together.

use crate::ColumnFlags;
use unicode_width::UnicodeWidthChar;

/// Height of the node header band.
pub const NODE_HEADER_HEIGHT: f32 = 34.0;
/// Height of one column row.
pub const NODE_COLUMN_HEIGHT: f32 = 22.0;
/// Baseline of the table name, measured from the node top.
pub const NODE_HEADER_BASELINE: f32 = 22.0;
/// Top of the first column row, measured from the node top.
pub const NODE_FIRST_ROW_TOP: f32 = NODE_HEADER_HEIGHT + 4.0;
/// Baseline of column text, measured from the top of its row.
pub const NODE_ROW_BASELINE: f32 = 15.0;
/// Space kept below the last column row.
pub const NODE_ROWS_BOTTOM_PADDING: f32 = 6.0;
/// Horizontal inset of header and column content from the node edges.
pub const NODE_TEXT_INSET: f32 = 12.0;
/// Corner radius of the node card.
pub const NODE_CORNER_RADIUS: f32 = 6.0;
/// Side length of the square kind mark before the table name.
pub const NODE_KIND_MARK_SIZE: f32 = 8.0;
/// Distance from the header's left content edge to the table name, past
/// the kind mark.
pub const NODE_HEADER_NAME_OFFSET: f32 = NODE_KIND_MARK_SIZE + 7.0;
/// Space kept between the table-name clip and the node's right edge for the
/// right-aligned kind label ("TABLE"/"VIEW"/"ENUM").
pub const NODE_KIND_LABEL_RESERVE: f32 = 48.0;

/// Font size of the table name.
pub const NODE_HEADER_FONT_SIZE: f32 = 14.0;
/// Font size of column names.
pub const NODE_COLUMN_FONT_SIZE: f32 = 12.0;
/// Font size of column types and the nullable marker.
pub const NODE_DETAIL_FONT_SIZE: f32 = 11.0;

/// Width of one PK / FK / IX badge.
pub const COLUMN_BADGE_WIDTH: f32 = 18.0;
/// Height of one PK / FK / IX badge.
pub const COLUMN_BADGE_HEIGHT: f32 = 13.0;
/// Width of one badge slot, including the gap to the next slot.
pub const COLUMN_BADGE_PITCH: f32 = 20.0;
/// Gap between the key badges and the column name.
pub const COLUMN_KEY_GUTTER_GAP: f32 = 4.0;
/// Minimum gap between a column name and its right-aligned type.
pub const COLUMN_NAME_TYPE_GAP: f32 = 16.0;
/// Gap between the end of the type and the nullable marker.
pub const COLUMN_NULLABLE_GAP: f32 = 2.0;
/// Width of the nullable marker slot to the right of the type.
pub const COLUMN_NULLABLE_SLOT_WIDTH: f32 = 10.0;
/// Gap between the type (or nullable marker) and the IX badge.
pub const COLUMN_INDEX_GAP: f32 = 6.0;
/// Marker drawn after the type of a nullable column.
pub const COLUMN_NULLABLE_MARKER: &str = "?";
/// Separator between the previous and current type of a column a diff
/// changed, as in `varchar(500) → varchar(120)`.
pub const TYPE_CHANGE_SEPARATOR: &str = " → ";
/// Center of the diff change marker (`+` / `−` / `~`) from the card's left
/// edge. It sits in the text inset, so marking a row needs no extra width.
pub const CHANGE_MARKER_CENTER: f32 = NODE_TEXT_INSET / 2.0;

/// Advance width of one narrow character in the card's monospace font, in `em`.
const MONO_NARROW_ADVANCE_EM: f32 = 0.6;
/// Advance width of one wide (CJK) character, in `em`.
const MONO_WIDE_ADVANCE_EM: f32 = 1.0;

/// Horizontal inset of the group label from the group's left edge.
pub const GROUP_LABEL_INSET: f32 = 12.0;
/// Font size of the group label.
pub const GROUP_LABEL_FONT_SIZE: f32 = 11.0;
/// Letter spacing of the group label, in `em`.
pub const GROUP_LABEL_LETTER_SPACING_EM: f32 = 0.12;
/// Letter spacing of the group label, in pixels.
pub const GROUP_LABEL_LETTER_SPACING: f32 = GROUP_LABEL_FONT_SIZE * GROUP_LABEL_LETTER_SPACING_EM;

/// Height of the edge label pill.
pub const EDGE_LABEL_HEIGHT: f32 = 18.0;
/// Horizontal padding inside the edge label pill, both sides combined.
pub const EDGE_LABEL_PADDING: f32 = 18.0;

/// Whether a card ends with a row counting the columns it leaves out.
///
/// Only a card that lists some columns gets the row; a card that lists none
/// is a header alone, as in the overview density.
#[must_use]
pub const fn shows_omitted_columns_row(listed: usize, omitted: usize) -> bool {
    listed > 0 && omitted > 0
}

/// Label of the row counting the columns a card leaves out.
#[must_use]
pub fn omitted_columns_label(omitted: usize) -> String {
    if omitted == 1 {
        "+1 column".to_string()
    } else {
        format!("+{omitted} columns")
    }
}

/// Height of a card that lists `listed` columns and leaves out `omitted`.
#[must_use]
#[allow(clippy::cast_precision_loss)] // Row counts are small layout values.
pub fn node_height(listed: usize, omitted: usize) -> f32 {
    let rows = listed + usize::from(shows_omitted_columns_row(listed, omitted));
    if rows == 0 {
        return NODE_HEADER_HEIGHT;
    }
    (rows as f32)
        .mul_add(
            NODE_COLUMN_HEIGHT,
            NODE_FIRST_ROW_TOP + NODE_ROWS_BOTTOM_PADDING,
        )
        .ceil()
}

/// Vertical center of column row `index`, measured from the node top.
///
/// Edge ports attach here so a relationship line meets the row it refers to.
#[must_use]
#[allow(clippy::cast_precision_loss)] // Column indices are small layout values.
pub fn column_row_center(index: usize) -> f32 {
    (index as f32).mul_add(NODE_COLUMN_HEIGHT, NODE_FIRST_ROW_TOP) + NODE_COLUMN_HEIGHT / 2.0
}

/// Column marks a node reserves a fixed slot for in every row.
///
/// A slot is reserved when any column of the node needs it, so PK / FK
/// badges, types, nullable markers, and IX badges line up down the card.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // One independent flag per slot.
pub struct ColumnSlots {
    /// Some column is part of the primary key.
    pub primary_key: bool,
    /// Some column participates in a foreign key.
    pub foreign_key: bool,
    /// Some column appears in an index.
    pub indexed: bool,
    /// Some column is nullable.
    pub nullable: bool,
}

impl ColumnSlots {
    /// Collects the slots needed by every column in `columns`.
    #[must_use]
    pub fn of<'a>(columns: impl IntoIterator<Item = &'a ColumnFlags>) -> Self {
        columns
            .into_iter()
            .fold(Self::default(), |slots, flags| Self {
                primary_key: slots.primary_key || flags.relation.is_primary_key,
                foreign_key: slots.foreign_key || flags.relation.is_foreign_key,
                indexed: slots.indexed || flags.relation.is_indexed,
                nullable: slots.nullable || flags.nullable,
            })
    }

    /// Offset of the FK badge from the row's left content edge; the PK badge
    /// always sits at that edge.
    #[must_use]
    pub const fn foreign_key_offset(self) -> f32 {
        if self.primary_key {
            COLUMN_BADGE_PITCH
        } else {
            0.0
        }
    }

    /// Width taken by the key badges before the column name.
    #[must_use]
    pub fn key_gutter(self) -> f32 {
        let slots = u8::from(self.primary_key) + u8::from(self.foreign_key);
        if slots == 0 {
            0.0
        } else {
            f32::from(slots).mul_add(COLUMN_BADGE_PITCH, COLUMN_KEY_GUTTER_GAP)
        }
    }

    /// Width taken right of the type by the nullable marker and IX badge.
    #[must_use]
    pub fn trailing_reserve(self) -> f32 {
        let nullable = if self.nullable {
            COLUMN_NULLABLE_SLOT_WIDTH
        } else {
            0.0
        };
        let indexed = if self.indexed {
            COLUMN_INDEX_GAP + COLUMN_BADGE_WIDTH
        } else {
            0.0
        };
        nullable + indexed
    }
}

/// Content width of one column row: key gutter, name, type, and trailing
/// marks, excluding the node's horizontal insets.
///
/// `previous_type` is the type before a diff changed it; the type slot then
/// holds `previous → data_type`.
#[must_use]
pub fn column_row_width(
    slots: ColumnSlots,
    name: &str,
    previous_type: Option<&str>,
    data_type: &str,
) -> f32 {
    let name_width = estimate_mono_text_width(name, NODE_COLUMN_FONT_SIZE);
    let change_width = previous_type.map_or(0.0, |previous| {
        estimate_mono_text_width(previous, NODE_DETAIL_FONT_SIZE)
            + estimate_mono_text_width(TYPE_CHANGE_SEPARATOR, NODE_DETAIL_FONT_SIZE)
    });
    let type_width = if data_type.is_empty() {
        0.0
    } else {
        COLUMN_NAME_TYPE_GAP
            + change_width
            + estimate_mono_text_width(data_type, NODE_DETAIL_FONT_SIZE)
    };
    slots.key_gutter() + name_width + type_width + slots.trailing_reserve()
}

/// Estimates the rendered width of `text` in the card's monospace font.
///
/// Narrow characters advance by a fixed fraction of the font size, wide
/// (CJK) characters by a full em, and zero-width characters not at all.
#[must_use]
pub fn estimate_mono_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| match ch.width_cjk().or_else(|| ch.width()) {
            Some(0) => 0.0,
            Some(1) | None => MONO_NARROW_ADVANCE_EM,
            Some(_) => MONO_WIDE_ADVANCE_EM,
        })
        .sum::<f32>()
        * font_size
}

/// Estimates the rendered width of proportional (sans-serif) `text` at
/// `font_size`.
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

/// Estimates the width of an edge label pill, including its padding.
///
/// The renderer sizes the pill with this value and the layout engine uses
/// it for label obstacles, so wide (CJK) and zero-width characters count
/// the same on both sides.
#[must_use]
pub fn estimate_edge_label_width(text: &str) -> f32 {
    text.chars()
        .map(|ch| match ch.width_cjk().or_else(|| ch.width()) {
            Some(0) | None => 0.0,
            Some(1) => 6.4,
            Some(_) => 12.8,
        })
        .sum::<f32>()
        + EDGE_LABEL_PADDING
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(primary_key: bool, foreign_key: bool, nullable: bool) -> ColumnFlags {
        ColumnFlags {
            nullable,
            relation: crate::ColumnRelationFlags {
                is_primary_key: primary_key,
                is_foreign_key: foreign_key,
                is_indexed: false,
            },
        }
    }

    #[test]
    fn column_slots_reserve_only_marks_some_column_uses() {
        let columns = [flags(true, false, false), flags(false, false, true)];
        let slots = ColumnSlots::of(&columns);

        assert!(slots.primary_key && slots.nullable);
        assert!(!slots.foreign_key && !slots.indexed);
        assert!((slots.key_gutter() - 24.0).abs() < f32::EPSILON);
        assert!((slots.trailing_reserve() - COLUMN_NULLABLE_SLOT_WIDTH).abs() < f32::EPSILON);
        assert!(ColumnSlots::default().key_gutter().abs() < f32::EPSILON);
    }

    #[test]
    fn foreign_key_slot_follows_primary_key_slot() {
        let fk_only = ColumnSlots::of(&[flags(false, true, false)]);
        let both = ColumnSlots::of(&[flags(true, false, false), flags(false, true, false)]);

        assert!(fk_only.foreign_key_offset().abs() < f32::EPSILON);
        assert!((both.foreign_key_offset() - COLUMN_BADGE_PITCH).abs() < f32::EPSILON);
        assert!((both.key_gutter() - 44.0).abs() < f32::EPSILON);
    }

    #[test]
    fn column_row_width_separates_name_and_type() {
        let slots = ColumnSlots::default();
        let name_only = column_row_width(slots, "id", None, "");
        let typed = column_row_width(slots, "id", None, "int");

        assert!((name_only - 14.4).abs() < 0.01);
        assert!((typed - (14.4 + COLUMN_NAME_TYPE_GAP + 19.8)).abs() < 0.01);
    }

    #[test]
    fn column_row_width_makes_room_for_a_type_change() {
        let slots = ColumnSlots::default();
        let typed = column_row_width(slots, "id", None, "int");
        let changed = column_row_width(slots, "id", Some("bigint"), "int");

        let expected = estimate_mono_text_width("bigint", NODE_DETAIL_FONT_SIZE)
            + estimate_mono_text_width(TYPE_CHANGE_SEPARATOR, NODE_DETAIL_FONT_SIZE);
        assert!((changed - typed - expected).abs() < 0.01);
    }

    #[test]
    fn node_height_adds_a_row_only_for_cards_listing_some_columns() {
        let listed = node_height(2, 0);

        assert!((node_height(0, 0) - NODE_HEADER_HEIGHT).abs() < f32::EPSILON);
        assert!((node_height(0, 5) - NODE_HEADER_HEIGHT).abs() < f32::EPSILON);
        assert!((node_height(2, 3) - listed - NODE_COLUMN_HEIGHT).abs() < f32::EPSILON);
        assert_eq!(omitted_columns_label(1), "+1 column");
        assert_eq!(omitted_columns_label(4), "+4 columns");
    }

    #[test]
    fn mono_text_width_counts_wide_characters_as_one_em() {
        assert!((estimate_mono_text_width("ab", 10.0) - 12.0).abs() < 0.01);
        assert!((estimate_mono_text_width("顧客", 10.0) - 20.0).abs() < 0.01);
        assert!((estimate_mono_text_width("e\u{301}", 10.0) - 6.0).abs() < 0.01);
    }

    #[test]
    fn edge_label_width_counts_wide_characters_double() {
        let ascii = estimate_edge_label_width("ab");
        let cjk = estimate_edge_label_width("漢");
        assert!((ascii - cjk).abs() < f32::EPSILON);
        assert!(estimate_edge_label_width("users_テーブル") > estimate_edge_label_width("users"));
    }

    #[test]
    fn edge_label_width_ignores_zero_width_characters() {
        let plain = estimate_edge_label_width("e");
        let combined = estimate_edge_label_width("e\u{301}");
        assert!((plain - combined).abs() < f32::EPSILON);
    }
}
