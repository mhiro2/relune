//! Group bounding-box positioning.

use tracing::warn;

use super::{GROUP_PADDING, GROUP_TOP_PADDING, PositionedGroup, PositionedNode};

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
