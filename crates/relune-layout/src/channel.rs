//! Deterministic candidate selection policy for obstacle-aware channels.

use std::cmp::Ordering;

/// Candidate family for channel search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelCandidateClass {
    /// Candidate inside the gap between two different ranks.
    InterRank,
    /// Candidate used when the endpoints stay on the same rank.
    SameRank,
    /// Candidate used when the edge flows against the assigned rank order.
    ReverseEdge,
}

impl ChannelCandidateClass {
    /// Returns the signed offsets probed around the baseline channel center.
    #[must_use]
    pub const fn search_offsets(self) -> &'static [f32] {
        match self {
            Self::InterRank => INTER_RANK_SEARCH_OFFSETS,
            Self::SameRank => SAME_RANK_SEARCH_OFFSETS,
            Self::ReverseEdge => REVERSE_EDGE_SEARCH_OFFSETS,
        }
    }
}

/// Search offsets for inter-rank channel candidates.
pub const INTER_RANK_SEARCH_OFFSETS: &[f32] = &[0.0, -24.0, 24.0, -48.0, 48.0];
/// Search offsets for same-rank channel candidates.
pub const SAME_RANK_SEARCH_OFFSETS: &[f32] = &[0.0, -32.0, 32.0, -64.0, 64.0, -96.0, 96.0];
/// Search offsets for reverse-flow channel candidates.
pub const REVERSE_EDGE_SEARCH_OFFSETS: &[f32] = &[0.0, -24.0, 24.0, -48.0, 48.0, -72.0, 72.0];

/// Relative weights for soft candidate costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelCostWeights {
    /// Weight for node clearance loss.
    pub clearance_penalty: u32,
    /// Weight for route length.
    pub total_length: u32,
    /// Weight for extra bends.
    pub bend_penalty: u32,
    /// Weight for drifting away from the baseline channel center.
    pub center_deviation: u32,
    /// Weight for candidate reuse on already busy channels.
    pub congestion_penalty: u32,
}

impl Default for ChannelCostWeights {
    fn default() -> Self {
        Self {
            clearance_penalty: 16,
            total_length: 1,
            bend_penalty: 48,
            center_deviation: 2,
            congestion_penalty: 40,
        }
    }
}

/// Scalar metrics collected for one obstacle-aware channel candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelCandidateScore {
    /// Number of hard-constraint violations.
    pub hard_constraint_violations: u16,
    /// Penalty accumulated from missing obstacle clearance.
    pub clearance_penalty: u32,
    /// Total routed path length in pixels.
    pub total_length: u32,
    /// Number of bends in the candidate route.
    pub bend_count: u16,
    /// Absolute offset from the baseline channel center in pixels.
    pub center_deviation: u32,
    /// Penalty for placing another edge on a crowded channel.
    pub congestion_penalty: u32,
    /// Stable input order for final deterministic tie-breaks.
    pub stable_order: u32,
}

impl ChannelCandidateScore {
    /// Returns the weighted soft cost for the candidate.
    #[must_use]
    #[allow(clippy::suspicious_operation_groupings)] // Weighted scoring intentionally sums unrelated penalty terms.
    pub fn weighted_soft_cost(self, weights: ChannelCostWeights) -> u64 {
        (u64::from(self.clearance_penalty) * u64::from(weights.clearance_penalty))
            + (u64::from(self.total_length) * u64::from(weights.total_length))
            + (u64::from(self.bend_count) * u64::from(weights.bend_penalty))
            + (u64::from(self.center_deviation) * u64::from(weights.center_deviation))
            + (u64::from(self.congestion_penalty) * u64::from(weights.congestion_penalty))
    }
}

/// Compares two candidate scores with a fixed, deterministic tie-break order.
#[must_use]
pub fn compare_channel_candidate_scores(
    left: ChannelCandidateScore,
    right: ChannelCandidateScore,
    weights: ChannelCostWeights,
) -> Ordering {
    left.hard_constraint_violations
        .cmp(&right.hard_constraint_violations)
        .then_with(|| {
            left.weighted_soft_cost(weights)
                .cmp(&right.weighted_soft_cost(weights))
        })
        .then_with(|| left.clearance_penalty.cmp(&right.clearance_penalty))
        .then_with(|| left.total_length.cmp(&right.total_length))
        .then_with(|| left.bend_count.cmp(&right.bend_count))
        .then_with(|| left.center_deviation.cmp(&right.center_deviation))
        .then_with(|| left.congestion_penalty.cmp(&right.congestion_penalty))
        .then_with(|| left.stable_order.cmp(&right.stable_order))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_search_offsets_start_at_center_and_stay_symmetric() {
        for class in [
            ChannelCandidateClass::InterRank,
            ChannelCandidateClass::SameRank,
            ChannelCandidateClass::ReverseEdge,
        ] {
            let offsets = class.search_offsets();
            assert_eq!(offsets.first().copied(), Some(0.0));

            for pair in offsets[1..].chunks_exact(2) {
                assert!((pair[0] + pair[1]).abs() <= f32::EPSILON);
            }
        }
    }

    #[test]
    fn candidate_score_prefers_feasible_route_before_soft_cost() {
        let weights = ChannelCostWeights::default();
        let feasible = ChannelCandidateScore {
            hard_constraint_violations: 0,
            clearance_penalty: 500,
            total_length: 500,
            bend_count: 6,
            center_deviation: 96,
            congestion_penalty: 10,
            stable_order: 3,
        };
        let infeasible = ChannelCandidateScore {
            hard_constraint_violations: 1,
            ..feasible
        };

        assert_eq!(
            compare_channel_candidate_scores(feasible, infeasible, weights),
            Ordering::Less
        );
    }

    #[test]
    fn candidate_score_uses_weighted_soft_cost_before_component_tie_breaks() {
        let weights = ChannelCostWeights::default();
        let shorter = ChannelCandidateScore {
            hard_constraint_violations: 0,
            clearance_penalty: 5,
            total_length: 120,
            bend_count: 2,
            center_deviation: 24,
            congestion_penalty: 1,
            stable_order: 1,
        };
        let safer = ChannelCandidateScore {
            hard_constraint_violations: 0,
            clearance_penalty: 2,
            total_length: 150,
            bend_count: 2,
            center_deviation: 24,
            congestion_penalty: 1,
            stable_order: 2,
        };

        assert_eq!(
            compare_channel_candidate_scores(safer, shorter, weights),
            Ordering::Less
        );
    }

    #[test]
    fn candidate_score_uses_fixed_component_order_for_equal_weighted_cost() {
        let weights = ChannelCostWeights {
            clearance_penalty: 1,
            total_length: 1,
            bend_penalty: 1,
            center_deviation: 1,
            congestion_penalty: 1,
        };
        let first = ChannelCandidateScore {
            hard_constraint_violations: 0,
            clearance_penalty: 4,
            total_length: 100,
            bend_count: 2,
            center_deviation: 12,
            congestion_penalty: 0,
            stable_order: 5,
        };
        let second = ChannelCandidateScore {
            hard_constraint_violations: 0,
            clearance_penalty: 5,
            total_length: 99,
            bend_count: 2,
            center_deviation: 12,
            congestion_penalty: 0,
            stable_order: 1,
        };

        assert_eq!(
            first.weighted_soft_cost(weights),
            second.weighted_soft_cost(weights)
        );
        assert_eq!(
            compare_channel_candidate_scores(first, second, weights),
            Ordering::Less
        );
    }
}
