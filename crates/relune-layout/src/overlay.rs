//! Overlay annotations for positioned graph elements.
//!
//! Overlays provide a way to attach visual annotations (lint warnings, diff
//! status, etc.) to nodes and edges without modifying the core positioned
//! graph types. Renderers consume overlays optionally — when no overlay is
//! present, the diagram renders normally.

use std::collections::BTreeMap;

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Severity level for an overlay annotation.
///
/// Maps conceptually to lint severity but is renderer-agnostic so that
/// diff status and other future annotation sources can reuse the same
/// visual weight scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlaySeverity {
    /// Lowest visual weight — informational hint.
    Hint,
    /// Informational annotation.
    Info,
    /// Warning — something that likely deserves attention.
    Warning,
    /// Error — a definite problem.
    Error,
}

/// A single annotation attached to a node or edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    /// Visual severity level.
    pub severity: OverlaySeverity,
    /// Short description shown in tooltips / badges.
    pub message: String,
    /// Optional hint for how to resolve the issue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Optional identifier of the rule that produced this annotation
    /// (e.g. `"no-primary-key"`). Useful for filtering and styling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

/// Overlay data for a single node, identified by stable ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeOverlay {
    /// Annotations on this node.
    pub annotations: Vec<Annotation>,
}

impl NodeOverlay {
    /// Returns the highest severity among all annotations, if any.
    #[must_use]
    pub fn max_severity(&self) -> Option<OverlaySeverity> {
        self.annotations
            .iter()
            .map(|a| a.severity)
            .max_by_key(|s| severity_rank(*s))
    }
}

/// Overlay data for a single edge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeOverlay {
    /// Annotations on this edge.
    pub annotations: Vec<Annotation>,
}

impl EdgeOverlay {
    /// Returns the highest severity among all annotations, if any.
    #[must_use]
    pub fn max_severity(&self) -> Option<OverlaySeverity> {
        self.annotations
            .iter()
            .map(|a| a.severity)
            .max_by_key(|s| severity_rank(*s))
    }
}

/// Stable key identifying an edge in [`DiagramOverlay::edges`].
///
/// Uses an explicit `(from, to)` tuple instead of a `"from->to"` string so
/// that arbitrary identifiers (which can themselves contain `->`) cannot
/// collide — for example `("a", "b->c")` and `("a->b", "c")` would both
/// flatten to the same string key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeKey {
    /// Source node stable ID.
    pub from: String,
    /// Destination node stable ID.
    pub to: String,
}

impl EdgeKey {
    /// Build an [`EdgeKey`] from string slices.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// Collection of all overlay annotations for a diagram.
///
/// Keyed by stable identifiers that match `PositionedNode::id` and the
/// `PositionedEdge::from` / `PositionedEdge::to` pair respectively.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramOverlay {
    /// Per-node overlays, keyed by `PositionedNode::id` (stable ID).
    pub nodes: BTreeMap<String, NodeOverlay>,
    /// Per-edge overlays, keyed by an explicit `(from, to)` pair (see
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
        let key = EdgeKey::new(entry.from, entry.to);
        if map.insert(key.clone(), entry.overlay).is_some() {
            return Err(D::Error::custom(format!(
                "duplicate edge overlay entry for ({}, {})",
                key.from, key.to
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
    pub fn edge(&self, from: &str, to: &str) -> Option<&EdgeOverlay> {
        self.edges.get(&EdgeKey::new(from, to))
    }

    /// Add an annotation to a node, creating the entry if needed.
    pub fn add_node_annotation(&mut self, node_id: impl Into<String>, annotation: Annotation) {
        self.nodes
            .entry(node_id.into())
            .or_default()
            .annotations
            .push(annotation);
    }

    /// Add an annotation to an edge, creating the entry if needed.
    pub fn add_edge_annotation(&mut self, from_id: &str, to_id: &str, annotation: Annotation) {
        self.edges
            .entry(EdgeKey::new(from_id, to_id))
            .or_default()
            .annotations
            .push(annotation);
    }
}

/// Returns a numeric rank for severity ordering (higher = more severe).
const fn severity_rank(s: OverlaySeverity) -> u8 {
    match s {
        OverlaySeverity::Hint => 0,
        OverlaySeverity::Info => 1,
        OverlaySeverity::Warning => 2,
        OverlaySeverity::Error => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warning_annotation(msg: &str) -> Annotation {
        Annotation {
            severity: OverlaySeverity::Warning,
            message: msg.to_string(),
            hint: None,
            rule_id: None,
        }
    }

    fn error_annotation(msg: &str) -> Annotation {
        Annotation {
            severity: OverlaySeverity::Error,
            message: msg.to_string(),
            hint: Some("Fix this".to_string()),
            rule_id: Some("test-rule".to_string()),
        }
    }

    #[test]
    fn empty_overlay_is_empty() {
        let overlay = DiagramOverlay::new();
        assert!(overlay.is_empty());
        assert!(overlay.node("foo").is_none());
        assert!(overlay.edge("foo", "bar").is_none());
    }

    #[test]
    fn add_node_annotation() {
        let mut overlay = DiagramOverlay::new();
        overlay.add_node_annotation("users", warning_annotation("No primary key"));

        assert!(!overlay.is_empty());
        let node = overlay.node("users").unwrap();
        assert_eq!(node.annotations.len(), 1);
        assert_eq!(node.annotations[0].message, "No primary key");
        assert_eq!(node.max_severity(), Some(OverlaySeverity::Warning));
    }

    #[test]
    fn add_edge_annotation() {
        let mut overlay = DiagramOverlay::new();
        overlay.add_edge_annotation("posts", "users", warning_annotation("Missing index"));

        let edge = overlay.edge("posts", "users").unwrap();
        assert_eq!(edge.annotations.len(), 1);
        assert!(overlay.edge("users", "posts").is_none());
    }

    #[test]
    fn max_severity_picks_highest() {
        let mut overlay = DiagramOverlay::new();
        overlay.add_node_annotation("users", warning_annotation("warn"));
        overlay.add_node_annotation("users", error_annotation("err"));

        let node = overlay.node("users").unwrap();
        assert_eq!(node.max_severity(), Some(OverlaySeverity::Error));
    }

    #[test]
    fn edge_key_round_trips_components() {
        let key = EdgeKey::new("posts", "users");
        assert_eq!(key.from, "posts");
        assert_eq!(key.to, "users");
    }

    #[test]
    fn ambiguous_arrow_ids_do_not_collide() {
        // Without an explicit tuple key, the two annotations below would have
        // collapsed to a single `"a->b->c"` entry.
        let mut overlay = DiagramOverlay::new();
        overlay.add_edge_annotation("a", "b->c", warning_annotation("first"));
        overlay.add_edge_annotation("a->b", "c", warning_annotation("second"));

        assert_eq!(overlay.edges.len(), 2);
        assert_eq!(
            overlay
                .edge("a", "b->c")
                .expect("first edge present")
                .annotations[0]
                .message,
            "first"
        );
        assert_eq!(
            overlay
                .edge("a->b", "c")
                .expect("second edge present")
                .annotations[0]
                .message,
            "second"
        );
    }

    #[test]
    fn serialization_round_trip() {
        let mut overlay = DiagramOverlay::new();
        overlay.add_node_annotation(
            "users",
            Annotation {
                severity: OverlaySeverity::Warning,
                message: "No primary key".to_string(),
                hint: Some("Add a PK column".to_string()),
                rule_id: Some("no-primary-key".to_string()),
            },
        );
        overlay.add_edge_annotation("posts", "users", warning_annotation("Missing index"));

        let json = serde_json::to_string(&overlay).unwrap();
        let deserialized: DiagramOverlay = serde_json::from_str(&json).unwrap();
        assert_eq!(overlay, deserialized);
    }

    #[test]
    fn duplicate_edge_entries_are_rejected() {
        let json = r#"{
            "nodes": {},
            "edges": [
                {"from": "posts", "to": "users", "overlay": {"annotations": []}},
                {"from": "posts", "to": "users", "overlay": {"annotations": []}}
            ]
        }"#;
        let err = serde_json::from_str::<DiagramOverlay>(json)
            .expect_err("duplicate (from, to) entries should fail to deserialize");
        assert!(err.to_string().contains("duplicate"));
    }
}
