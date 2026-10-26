//! Group bounding-box positioning.

use tracing::warn;

use super::{GROUP_PADDING, GROUP_TOP_PADDING, PositionedGroup, PositionedNode};
use crate::metrics::{
    GROUP_LABEL_BASELINE, GROUP_LABEL_FONT_SIZE, GROUP_LABEL_INSET, group_label_width,
};
use crate::route::Rect;

/// Space kept clear around a group heading, so lines and labels never touch it.
const GROUP_HEADING_MARGIN: f32 = 4.0;

/// Area a group's heading text covers, with a small margin, or `None` for a
/// group without a heading or without cards.
pub(super) fn group_heading_rect(group: &PositionedGroup) -> Option<Rect> {
    if group.label.is_empty() || group.width <= 0.0 || group.height <= 0.0 {
        return None;
    }
    // Cap height plus descenders of the heading font around its baseline.
    let top = GROUP_LABEL_BASELINE - GROUP_LABEL_FONT_SIZE;
    Some(Rect {
        x: group.x + GROUP_LABEL_INSET - GROUP_HEADING_MARGIN,
        y: group.y + top - GROUP_HEADING_MARGIN,
        w: GROUP_HEADING_MARGIN.mul_add(2.0, group_label_width(&group.label)),
        h: GROUP_HEADING_MARGIN.mul_add(2.0, GROUP_LABEL_FONT_SIZE + 3.0),
    })
}

/// Calculate positions for groups.
pub(super) fn position_groups(
    groups: &[crate::graph::LayoutGroup],
    positioned_nodes: &[PositionedNode],
) -> Vec<PositionedGroup> {
    if groups.is_empty() {
        return Vec::new();
    }

    groups
        .iter()
        .map(|group| {
            let mut invalid_indices = Vec::new();
            let group_nodes: Vec<&PositionedNode> = group
                .node_indices
                .iter()
                .filter_map(|&idx| {
                    positioned_nodes.get(idx).or_else(|| {
                        invalid_indices.push(idx);
                        None
                    })
                })
                .collect();

            if !invalid_indices.is_empty() {
                warn!(
                    group = %group.id,
                    invalid_indices = ?invalid_indices,
                    "Skipping invalid group node indices"
                );
            }

            if group_nodes.is_empty() {
                return PositionedGroup {
                    id: group.id.clone(),
                    label: group.label.clone(),
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                };
            }

            let min_x = group_nodes.iter().map(|n| n.x).fold(f32::MAX, f32::min);
            let min_y = group_nodes.iter().map(|n| n.y).fold(f32::MAX, f32::min);
            let max_x = group_nodes
                .iter()
                .map(|n| n.x + n.width)
                .fold(f32::MIN, f32::max);
            let max_y = group_nodes
                .iter()
                .map(|n| n.y + n.height)
                .fold(f32::MIN, f32::max);

            PositionedGroup {
                id: group.id.clone(),
                label: group.label.clone(),
                x: min_x - GROUP_PADDING,
                y: min_y - GROUP_TOP_PADDING,
                width: GROUP_PADDING.mul_add(2.0, max_x - min_x),
                height: max_y - min_y + GROUP_TOP_PADDING + GROUP_PADDING,
            }
        })
        .collect()
}
