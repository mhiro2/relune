//! Overlay annotations for positioned graph elements.
//!
//! Overlays attach diff state and risk annotations to nodes and edges
//! without modifying the core positioned graph types. Renderers consume
//! overlays optionally — when no overlay is present, the diagram renders
//! normally.
//!
//! What changed and how risky it is are kept apart: a node's `change` and
//! `column_changes` record the change kind (added / removed / modified),
//! while `annotations` carry review findings ranked by [`ReviewSeverity`].
//! An added column can therefore be breaking, and a modified one safe.

use std::collections::BTreeMap;

use relune_core::{ChangeKind, ReviewSeverity};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::graph::LayoutGraph;

/// A single risk annotation attached to a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    /// How risky the annotated change is.
    pub severity: ReviewSeverity,
    /// Short description shown in tooltips and detail panels.
    pub message: String,
    /// Optional hint for how to resolve the issue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Optional identifier of the rule that produced this annotation
    /// (e.g. `"risk/drop-column"`). Useful for filtering and styling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

/// How a whole node changed in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeChange {
    /// Change kind of the node itself.
    pub kind: ChangeKind,
    /// One-line summary, e.g. `"Modified (3 changes)"`.
    pub summary: String,
    /// Individual changes, e.g. `"+ email"` or `"~ users_email_idx"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
}

/// How a single column (or enum value) changed in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnChange {
    /// Change kind of the column.
    pub kind: ChangeKind,
    /// Type the column had before, when the diff changed its type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_type: Option<String>,
}

impl ColumnChange {
    /// A change that leaves the column's type as it was.
    #[must_use]
    pub const fn new(kind: ChangeKind) -> Self {
        Self {
            kind,
            previous_type: None,
        }
    }
}

/// Overlay data for a single node, identified by stable ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeOverlay {
    /// Diff change of the node, if it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<NodeChange>,
    /// Risk annotations on this node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    /// Diff changes of individual columns, keyed by column name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub column_changes: BTreeMap<String, ColumnChange>,
}

impl NodeOverlay {
    /// Change kind of the node itself, if it changed.
    #[must_use]
    pub fn change_kind(&self) -> Option<ChangeKind> {
        self.change.as_ref().map(|change| change.kind)
    }

    /// Returns the highest severity among all annotations, if any.
    #[must_use]
    pub fn max_severity(&self) -> Option<ReviewSeverity> {
        self.annotations.iter().map(|a| a.severity).max()
    }

    /// Returns the highest severity together with how many annotations
    /// carry it, e.g. `(Breaking, 1)` for a `breaking 1` label.
    #[must_use]
    pub fn top_risk(&self) -> Option<(ReviewSeverity, usize)> {
        let severity = self.max_severity()?;
        let count = self
            .annotations
            .iter()
            .filter(|a| a.severity == severity)
            .count();
        Some((severity, count))
    }
}

/// Overlay data for a single edge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeOverlay {
    /// Diff change kind of the relationship, if it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<ChangeKind>,
}

/// Stable key identifying an edge in [`DiagramOverlay::edges`].
///
/// Uses explicit fields instead of a `"from->to"` string so that arbitrary
/// identifiers (which can themselves contain `->`) cannot collide — for
/// example `("a", "b->c")` and `("a->b", "c")` would both flatten to the same
/// string key. The column lists tell apart several relationships between
/// the same two tables, such as `author_id` and `editor_id` both
/// referencing `users`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeKey {
    /// Source node stable ID.
    pub from: String,
    /// Destination node stable ID.
    pub to: String,
    /// Source columns of the relationship, matching
    /// `PositionedEdge::from_columns`.
    pub from_columns: Vec<String>,
    /// Referenced columns, matching `PositionedEdge::to_columns`.
    pub to_columns: Vec<String>,
}

impl EdgeKey {
    /// Build an [`EdgeKey`] from its parts.
    #[must_use]
    pub fn new<F: AsRef<str>, T: AsRef<str>>(
        from: impl Into<String>,
        to: impl Into<String>,
        from_columns: &[F],
        to_columns: &[T],
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            from_columns: owned_columns(from_columns),
            to_columns: owned_columns(to_columns),
        }
    }
}

fn owned_columns<C: AsRef<str>>(columns: &[C]) -> Vec<String> {
    columns
        .iter()
        .map(|column| column.as_ref().to_string())
        .collect()
}

/// Collection of all overlay annotations for a diagram.
///
/// Keyed by stable identifiers that match `PositionedNode::id` and the
/// `PositionedEdge::from` / `PositionedEdge::to` pair respectively.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramOverlay {
    /// Per-node overlays, keyed by `PositionedNode::id` (stable ID).
    pub nodes: BTreeMap<String, NodeOverlay>,
    /// Per-edge overlays, keyed by source, target, and both column lists (see
    /// [`EdgeKey`]).
    ///
    /// Serialized as a list of entries because JSON map keys must be
    /// strings, and we deliberately keep the key as an explicit struct
    /// to avoid `from->to` collisions on identifiers that contain `->`.
    #[serde(
        serialize_with = "serialize_edges",
        deserialize_with = "deserialize_edges"
    )]
    pub edges: BTreeMap<EdgeKey, EdgeOverlay>,
}

#[derive(Serialize, Deserialize)]
struct EdgeEntry {
    from: String,
    to: String,
    #[serde(default)]
    from_columns: Vec<String>,
    #[serde(default)]
    to_columns: Vec<String>,
    overlay: EdgeOverlay,
}

fn serialize_edges<S>(
    edges: &BTreeMap<EdgeKey, EdgeOverlay>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let entries: Vec<EdgeEntry> = edges
        .iter()
        .map(|(key, overlay)| EdgeEntry {
            from: key.from.clone(),
            to: key.to.clone(),
            from_columns: key.from_columns.clone(),
            to_columns: key.to_columns.clone(),
            overlay: overlay.clone(),
        })
        .collect();
    entries.serialize(serializer)
}

fn deserialize_edges<'de, D>(deserializer: D) -> Result<BTreeMap<EdgeKey, EdgeOverlay>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries: Vec<EdgeEntry> = Vec::deserialize(deserializer)?;
    let mut map = BTreeMap::new();
    for entry in entries {
        let key = EdgeKey {
            from: entry.from,
            to: entry.to,
            from_columns: entry.from_columns,
            to_columns: entry.to_columns,
        };
        if map.insert(key.clone(), entry.overlay).is_some() {
            return Err(D::Error::custom(format!(
                "duplicate edge overlay entry for {}({}) -> {}({})",
                key.from,
                key.from_columns.join(", "),
                key.to,
                key.to_columns.join(", ")
            )));
        }
    }
    Ok(map)
}

impl DiagramOverlay {
    /// Creates an empty overlay.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if this overlay contains no annotations at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// Look up the overlay for a specific node.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&NodeOverlay> {
        self.nodes.get(id)
    }

    /// Look up the overlay for a specific edge.
    #[must_use]
    pub fn edge<F: AsRef<str>, T: AsRef<str>>(
        &self,
        from: &str,
        to: &str,
        from_columns: &[F],
        to_columns: &[T],
    ) -> Option<&EdgeOverlay> {
        self.edges
            .get(&EdgeKey::new(from, to, from_columns, to_columns))
    }

    /// Record how a node changed, creating the entry if needed.
    pub fn set_node_change(&mut self, node_id: impl Into<String>, change: NodeChange) {
        self.nodes.entry(node_id.into()).or_default().change = Some(change);
    }

    /// Add a risk annotation to a node, creating the entry if needed.
    pub fn add_node_annotation(&mut self, node_id: impl Into<String>, annotation: Annotation) {
        self.nodes
            .entry(node_id.into())
            .or_default()
            .annotations
            .push(annotation);
    }

    /// Record the diff status of a node's column, creating the entry if needed.
    pub fn set_column_change(
        &mut self,
        node_id: impl Into<String>,
        column: impl Into<String>,
        change: ColumnChange,
    ) {
        self.nodes
            .entry(node_id.into())
            .or_default()
            .column_changes
            .insert(column.into(), change);
    }

    /// Copies each column's previous type onto `graph`, so cards are sized
    /// for and draw `previous → current` in the type slot.
    pub fn apply_type_changes(&self, graph: &mut LayoutGraph) {
        for node in &mut graph.nodes {
            let Some(node_overlay) = self.nodes.get(&node.id) else {
                continue;
            };
            for column in &mut node.columns {
                column.previous_data_type = node_overlay
                    .column_changes
                    .get(&column.name)
                    .and_then(|change| change.previous_type.clone());
            }
        }
    }

    /// Record how a relationship changed, creating the entry if needed.
    pub fn set_edge_change<F: AsRef<str>, T: AsRef<str>>(
        &mut self,
        from_id: &str,
        to_id: &str,
        from_columns: &[F],
        to_columns: &[T],
        change: ChangeKind,
    ) {
        self.edges
            .entry(EdgeKey::new(from_id, to_id, from_columns, to_columns))
            .or_default()
            .change = Some(change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(severity: ReviewSeverity, msg: &str) -> Annotation {
        Annotation {
            severity,
            message: msg.to_string(),
            hint: None,
            rule_id: None,
        }
    }

    #[test]
    fn empty_overlay_is_empty() {
        let overlay = DiagramOverlay::new();
        assert!(overlay.is_empty());
        assert!(overlay.node("foo").is_none());
        assert!(overlay.edge("foo", "bar", &["id"], &["id"]).is_none());
    }

    #[test]
    fn add_node_annotation() {
        let mut overlay = DiagramOverlay::new();
        overlay.add_node_annotation(
            "users",
            annotation(ReviewSeverity::Warning, "No primary key"),
        );

        assert!(!overlay.is_empty());
        let node = overlay.node("users").unwrap();
        assert_eq!(node.annotations.len(), 1);
        assert_eq!(node.annotations[0].message, "No primary key");
        assert_eq!(node.max_severity(), Some(ReviewSeverity::Warning));
        assert_eq!(node.change_kind(), None);
    }

    #[test]
    fn change_kind_and_risk_are_independent() {
        let mut overlay = DiagramOverlay::new();
        overlay.set_node_change(
            "users",
            NodeChange {
                kind: ChangeKind::Added,
                summary: "Added table".to_string(),
                details: Vec::new(),
            },
        );
        overlay.add_node_annotation("users", annotation(ReviewSeverity::Breaking, "risky"));

        let node = overlay.node("users").unwrap();
        assert_eq!(node.change_kind(), Some(ChangeKind::Added));
        assert_eq!(node.max_severity(), Some(ReviewSeverity::Breaking));
    }

    #[test]
    fn set_edge_change() {
        let mut overlay = DiagramOverlay::new();
        overlay.set_edge_change(
            "posts",
            "users",
            &["author_id"],
            &["id"],
            ChangeKind::Removed,
        );
        overlay.set_edge_change("posts", "users", &["editor_id"], &["id"], ChangeKind::Added);

        let removed = overlay
            .edge("posts", "users", &["author_id"], &["id"])
            .unwrap();
        assert_eq!(removed.change, Some(ChangeKind::Removed));
        // A second relationship between the same tables keeps its own change.
        let added = overlay
            .edge("posts", "users", &["editor_id"], &["id"])
            .unwrap();
        assert_eq!(added.change, Some(ChangeKind::Added));
        assert!(
            overlay
                .edge("users", "posts", &["author_id"], &["id"])
                .is_none()
        );
    }

    #[test]
    fn top_risk_counts_only_the_highest_severity() {
        let mut overlay = DiagramOverlay::new();
        overlay.add_node_annotation("users", annotation(ReviewSeverity::Warning, "warn"));
        overlay.add_node_annotation("users", annotation(ReviewSeverity::Breaking, "drop"));
        overlay.add_node_annotation("users", annotation(ReviewSeverity::Info, "info"));

        let node = overlay.node("users").unwrap();
        assert_eq!(node.max_severity(), Some(ReviewSeverity::Breaking));
        assert_eq!(node.top_risk(), Some((ReviewSeverity::Breaking, 1)));
    }

    #[test]
    fn edge_key_round_trips_components() {
        let key = EdgeKey::new("posts", "users", &["user_id"], &["id"]);
        assert_eq!(key.from, "posts");
        assert_eq!(key.to, "users");
        assert_eq!(key.from_columns, vec!["user_id".to_string()]);
        assert_eq!(key.to_columns, vec!["id".to_string()]);
    }

    #[test]
    fn ambiguous_arrow_ids_do_not_collide() {
        // Without an explicit tuple key, the two changes below would have
        // collapsed to a single `"a->b->c"` entry.
        let mut overlay = DiagramOverlay::new();
        overlay.set_edge_change("a", "b->c", &["id"], &["id"], ChangeKind::Added);
        overlay.set_edge_change("a->b", "c", &["id"], &["id"], ChangeKind::Removed);

        assert_eq!(overlay.edges.len(), 2);
        assert_eq!(
            overlay
                .edge("a", "b->c", &["id"], &["id"])
                .expect("first edge present")
                .change,
            Some(ChangeKind::Added)
        );
        assert_eq!(
            overlay
                .edge("a->b", "c", &["id"], &["id"])
                .expect("second edge present")
                .change,
            Some(ChangeKind::Removed)
        );
    }

    #[test]
    fn serialization_round_trip() {
        let mut overlay = DiagramOverlay::new();
        overlay.set_node_change(
            "users",
            NodeChange {
                kind: ChangeKind::Modified,
                summary: "Modified (1 changes)".to_string(),
                details: vec!["+ email".to_string()],
            },
        );
        overlay.set_column_change("users", "email", ColumnChange::new(ChangeKind::Added));
        overlay.set_column_change(
            "users",
            "name",
            ColumnChange {
                kind: ChangeKind::Modified,
                previous_type: Some("varchar(500)".to_string()),
            },
        );
        overlay.add_node_annotation(
            "users",
            Annotation {
                severity: ReviewSeverity::Warning,
                message: "NOT NULL column added".to_string(),
                hint: Some("Backfill first".to_string()),
                rule_id: Some("risk/add-not-null-on-existing".to_string()),
            },
        );
        overlay.set_edge_change("posts", "users", &["user_id"], &["id"], ChangeKind::Added);

        let json = serde_json::to_string(&overlay).unwrap();
        let deserialized: DiagramOverlay = serde_json::from_str(&json).unwrap();
        assert_eq!(overlay, deserialized);
    }

    #[test]
    fn duplicate_edge_entries_are_rejected() {
        let json = r#"{
            "nodes": {},
            "edges": [
                {"from": "posts", "to": "users", "from_columns": ["user_id"], "to_columns": ["id"], "overlay": {}},
                {"from": "posts", "to": "users", "from_columns": ["user_id"], "to_columns": ["id"], "overlay": {}}
            ]
        }"#;
        let err = serde_json::from_str::<DiagramOverlay>(json)
            .expect_err("duplicate (from, to) entries should fail to deserialize");
        assert!(err.to_string().contains("duplicate"));
    }
}
