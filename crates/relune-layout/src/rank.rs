//! Rank/layer assignment for hierarchical layout
//!
//! Foreign keys point from a child table to the parent it references, and the
//! layout places parents above (before) their children. Cyclic references are
//! first broken with a greedy feedback arc set so the remaining graph is a DAG,
//! then every node is assigned the length of the longest parent chain above it.

use std::collections::VecDeque;

use crate::graph::LayoutGraph;
use tracing::{debug, warn};

/// Result of rank assignment.
#[derive(Debug, Clone)]
pub struct RankAssignment {
    /// Map from node index to rank (layer).
    pub node_rank: Vec<usize>,
    /// Total number of ranks.
    pub num_ranks: usize,
    /// Nodes grouped by rank.
    pub nodes_by_rank: Vec<Vec<usize>>,
}

/// Assign ranks to nodes in the graph.
///
/// Referenced tables get lower ranks than the tables referencing them. Edges
/// that close a cycle are reversed (for ranking only) using the Eades–Lin–Smyth
/// greedy feedback arc set heuristic, so a strongly connected component spreads
/// over as few layers as its acyclic skeleton needs instead of one per node.
#[must_use]
pub fn assign_ranks(graph: &LayoutGraph) -> RankAssignment {
    let n = graph.nodes.len();
    let ranking_graph = RankingGraph::from_layout_graph(graph);
    let order = ranking_graph.greedy_acyclic_order();

    let mut position = vec![0usize; n];
    for (pos, &node_idx) in order.iter().enumerate() {
        position[node_idx] = pos;
    }

    // Orient every edge forward along `order`; edges pointing backward form
    // the feedback arc set and are reversed for ranking purposes.
    let mut forward = vec![Vec::new(); n];
    let mut reversed = Vec::new();
    for (upper, lowers) in ranking_graph.lowers.iter().enumerate() {
        for &lower in lowers {
            if position[upper] < position[lower] {
                forward[upper].push(lower);
            } else {
                forward[lower].push(upper);
                reversed.push((upper, lower));
            }
        }
    }
    warn_if_cycles(graph, &reversed);

    // `order` is a topological order of the oriented graph, so a single
    // forward sweep yields longest-path ranks.
    let mut node_rank = vec![0usize; n];
    for &node_idx in &order {
        let next_rank = node_rank[node_idx] + 1;
        for &lower in &forward[node_idx] {
            node_rank[lower] = node_rank[lower].max(next_rank);
        }
    }

    build_rank_assignment(node_rank)
}

fn warn_if_cycles(graph: &LayoutGraph, reversed: &[(usize, usize)]) {
    if reversed.is_empty() {
        return;
    }

    warn!(
        reversed_edges = reversed.len(),
        "Foreign-key cycle detected; reversing edges that close cycles for rank assignment"
    );
    for &(upper, lower) in reversed {
        debug!(
            parent = %graph.nodes[upper].id,
            child = %graph.nodes[lower].id,
            "Reversed cyclic edge for rank assignment"
        );
    }
}

/// Deduplicated parent → child adjacency used for ranking.
struct RankingGraph {
    /// `lowers[parent]` lists the children that must be ranked below `parent`.
    lowers: Vec<Vec<usize>>,
    /// `uppers[child]` lists the parents that must be ranked above `child`.
    uppers: Vec<Vec<usize>>,
}

impl RankingGraph {
    fn from_layout_graph(graph: &LayoutGraph) -> Self {
        let n = graph.nodes.len();
        let mut lowers = vec![Vec::new(); n];
        let mut uppers = vec![Vec::new(); n];

        for edge in &graph.edges {
            if edge.is_self_loop {
                continue;
            }
            if let (Some(&child), Some(&parent)) = (
                graph.node_index.get(&edge.from),
                graph.node_index.get(&edge.to),
            ) && child != parent
            {
                lowers[parent].push(child);
                uppers[child].push(parent);
            }
        }
        for list in lowers.iter_mut().chain(uppers.iter_mut()) {
            list.sort_unstable();
            list.dedup();
        }

        Self { lowers, uppers }
    }

    /// Eades–Lin–Smyth greedy ordering.
    ///
    /// Sinks are peeled off to the end and sources to the front; when neither
    /// exists, the node with the largest `out - in` degree is moved to the
    /// front. Edges pointing backward in the resulting order form a small
    /// feedback arc set. Ties break on the lowest node index for determinism.
    fn greedy_acyclic_order(&self) -> Vec<usize> {
        let n = self.lowers.len();
        let mut state = PeelState {
            graph: self,
            out_degree: self.lowers.iter().map(Vec::len).collect(),
            in_degree: self.uppers.iter().map(Vec::len).collect(),
            removed: vec![false; n],
            sinks: VecDeque::new(),
            sources: VecDeque::new(),
        };
        state.sinks = (0..n).filter(|&v| state.out_degree[v] == 0).collect();
        state.sources = (0..n)
            .filter(|&v| state.in_degree[v] == 0 && state.out_degree[v] > 0)
            .collect();

        let mut front = Vec::with_capacity(n);
        let mut back = Vec::new();
        let mut remaining = n;
        while remaining > 0 {
            if let Some(v) = state.sinks.pop_front() {
                if state.remove(v) {
                    back.push(v);
                    remaining -= 1;
                }
                continue;
            }
            if let Some(v) = state.sources.pop_front() {
                if state.remove(v) {
                    front.push(v);
                    remaining -= 1;
                }
                continue;
            }

            // Only cycles remain: move the node with the most outgoing surplus
            // to the front so it breaks as few edges as possible.
            let pick = (0..n)
                .filter(|&v| !state.removed[v])
                .max_by_key(|&v| {
                    let delta =
                        state.out_degree[v].cast_signed() - state.in_degree[v].cast_signed();
                    (delta, std::cmp::Reverse(v))
                })
                .expect("remaining > 0 implies an unremoved node");
            state.remove(pick);
            front.push(pick);
            remaining -= 1;
        }

        front.extend(back.into_iter().rev());
        front
    }
}

/// Mutable bookkeeping for [`RankingGraph::greedy_acyclic_order`].
struct PeelState<'a> {
    graph: &'a RankingGraph,
    out_degree: Vec<usize>,
    in_degree: Vec<usize>,
    removed: Vec<bool>,
    sinks: VecDeque<usize>,
    sources: VecDeque<usize>,
}

impl PeelState<'_> {
    /// Removes `v` from the remaining graph, queueing neighbors that become
    /// sinks or sources. Returns `false` when `v` was already removed.
    fn remove(&mut self, v: usize) -> bool {
        if self.removed[v] {
            return false;
        }
        self.removed[v] = true;
        for &lower in &self.graph.lowers[v] {
            if !self.removed[lower] {
                self.in_degree[lower] -= 1;
                if self.in_degree[lower] == 0 {
                    self.sources.push_back(lower);
                }
            }
        }
        for &upper in &self.graph.uppers[v] {
            if !self.removed[upper] {
                self.out_degree[upper] -= 1;
                if self.out_degree[upper] == 0 {
                    self.sinks.push_back(upper);
                }
            }
        }
        true
    }
}

fn build_rank_assignment(node_rank: Vec<usize>) -> RankAssignment {
    let num_ranks = node_rank.iter().copied().max().map_or(0, |r| r + 1);

    let mut nodes_by_rank = vec![Vec::new(); num_ranks];
    for (idx, &rank) in node_rank.iter().enumerate() {
        nodes_by_rank[rank].push(idx);
    }

    RankAssignment {
        node_rank,
        num_ranks,
        nodes_by_rank,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::graph::LayoutGraphBuilder;
    use relune_core::{Column, ColumnId, ForeignKey, ReferentialAction, Schema, Table, TableId};

    /// Builds a schema where each `(table, references)` entry declares one
    /// foreign key from `table` to every referenced table.
    fn schema_with_references(tables: &[(&str, &[&str])]) -> Schema {
        let tables = tables
            .iter()
            .enumerate()
            .map(|(index, (name, references))| {
                let id = u64::try_from(index + 1).unwrap();
                Table {
                    id: TableId(id),
                    stable_id: (*name).to_string(),
                    schema_name: None,
                    name: (*name).to_string(),
                    columns: vec![Column {
                        id: ColumnId(id),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
                    }],
                    foreign_keys: references
                        .iter()
                        .map(|target| ForeignKey {
                            name: None,
                            from_columns: vec![format!("{target}_id")],
                            to_schema: None,
                            to_table: (*target).to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        })
                        .collect(),
                    indexes: vec![],
                    primary_key_name: None,
                    check_constraints: Vec::new(),
                    comment: None,
                }
            })
            .collect();
        Schema {
            tables,
            views: vec![],
            enums: vec![],
        }
    }

    fn ranks_by_id(tables: &[(&str, &[&str])]) -> (BTreeMap<String, usize>, usize) {
        let graph = LayoutGraphBuilder::new().build(&schema_with_references(tables));
        let ranks = assign_ranks(&graph);
        let by_id = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(idx, node)| (node.id.clone(), ranks.node_rank[idx]))
            .collect();
        (by_id, ranks.num_ranks)
    }

    #[test]
    fn test_assign_ranks_places_parents_above_children() {
        let (ranks, num_ranks) = ranks_by_id(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);

        assert_eq!(ranks["a"], 0);
        assert_eq!(ranks["b"], 1);
        assert_eq!(ranks["c"], 2);
        assert_eq!(num_ranks, 3);
    }

    #[test]
    fn test_assign_ranks_uses_longest_parent_chain() {
        let (ranks, _) =
            ranks_by_id(&[("a", &[]), ("b", &["a"]), ("c", &["b"]), ("d", &["a", "c"])]);

        assert_eq!(ranks["d"], 3);
    }

    #[test]
    fn test_assign_ranks_spreads_simple_cycle_across_layers() {
        let (ranks, num_ranks) = ranks_by_id(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"])]);

        let mut cycle_ranks: Vec<usize> = ranks.into_values().collect();
        cycle_ranks.sort_unstable();
        assert_eq!(cycle_ranks, vec![0, 1, 2]);
        assert_eq!(num_ranks, 3);
    }

    #[test]
    fn test_assign_ranks_keeps_dense_cycle_compact() {
        // Every spoke references the hub and the hub references every spoke.
        // Ranking one node per layer would need nine layers; reversing the
        // hub's outgoing references leaves a two-layer star.
        let spokes = ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8"];
        let mut tables: Vec<(&str, &[&str])> = vec![("hub", &spokes)];
        tables.extend(spokes.iter().map(|spoke| (*spoke, &["hub"] as &[&str])));

        let (ranks, num_ranks) = ranks_by_id(&tables);

        assert_eq!(num_ranks, 2);
        let hub_rank = ranks["hub"];
        assert!(spokes.iter().all(|spoke| ranks[*spoke] != hub_rank));
    }

    #[test]
    fn test_assign_ranks_keeps_acyclic_edges_downward_around_cycle() {
        // `root` is referenced by the cycle, and `leaf` references the cycle.
        let (ranks, _) = ranks_by_id(&[
            ("root", &[]),
            ("a", &["root", "b"]),
            ("b", &["a"]),
            ("leaf", &["b"]),
        ]);

        assert!(ranks["root"] < ranks["a"].min(ranks["b"]));
        assert!(ranks["leaf"] > ranks["b"]);
    }

    #[test]
    fn test_assign_ranks_ignores_self_loops() {
        let (ranks, num_ranks) = ranks_by_id(&[("a", &["a"]), ("b", &["a"])]);

        assert_eq!(ranks["a"], 0);
        assert_eq!(ranks["b"], 1);
        assert_eq!(num_ranks, 2);
    }

    #[test]
    fn test_assign_ranks_handles_empty_graph() {
        let (ranks, num_ranks) = ranks_by_id(&[]);

        assert!(ranks.is_empty());
        assert_eq!(num_ranks, 0);
    }
}
