//! Routing debug metadata helpers.

use crate::port::RegularPortAssignment;
use crate::route::{AttachmentSide, ChannelAxis};

use super::PositionedEdgeRoutingDebug;

pub(super) fn build_regular_edge_debug(
    assignment: RegularPortAssignment,
) -> PositionedEdgeRoutingDebug {
    PositionedEdgeRoutingDebug {
        source_side: Some(attachment_side_name(assignment.source_side).to_string()),
        target_side: Some(attachment_side_name(assignment.target_side).to_string()),
        source_slot_index: Some(assignment.source_slot_index),
        source_slot_count: Some(assignment.source_slot_count),
        target_slot_index: Some(assignment.target_slot_index),
        target_slot_count: Some(assignment.target_slot_count),
        source_row_offset: Some(assignment.source_row_offset),
        target_row_offset: Some(assignment.target_row_offset),
        channel_axis: None,
        channel_coordinate: None,
        detour_activation_counted: false,
        self_loop_radius_offset: None,
    }
}

pub(super) const fn attachment_side_name(side: AttachmentSide) -> &'static str {
    match side {
        AttachmentSide::North => "north",
        AttachmentSide::South => "south",
        AttachmentSide::East => "east",
        AttachmentSide::West => "west",
    }
}

pub(super) const fn channel_axis_name(axis: ChannelAxis) -> &'static str {
    match axis {
        ChannelAxis::X => "x",
        ChannelAxis::Y => "y",
    }
}
