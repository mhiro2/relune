//! Graph construction from Schema
//!
//! This module builds a layout-oriented graph from a normalized Schema,
//! with support for filtering, focus, and grouping.

use std::collections::{BTreeMap, BTreeSet};

use relune_core::{
    EdgeKind, Enum, FilterSpec, FocusSpec, GroupingSpec, GroupingStrategy, NodeKind, Schema, Table,
    View,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Check whether `needle` appears in `haystack` as a whole SQL identifier,
/// i.e. not as a substring of a longer identifier.  Both strings must already
/// be lowercased.  The check treats any character that is **not** alphanumeric,
/// `_`, or `.` (for schema-qualified names) as an identifier boundary, which
/// also covers quoted identifiers (`"user"` → boundary is `"`).
fn contains_identifier(haystack: &str, needle: &str) -> bool {
    fn is_ident_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '.'
    }

    let h = haystack.as_bytes();
    let n = needle.len();
    if n == 0 || n > h.len() {
        return false;
    }

    for (start, _) in haystack.match_indices(needle) {
        let end = start + n;
        let left_ok = start == 0 || !is_ident_char(haystack[..start].chars().next_back().unwrap());
        let right_ok = end == h.len() || !is_ident_char(haystack[end..].chars().next().unwrap());
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

#[derive(Debug, Default)]
struct EnumIndex {
    exact: BTreeMap<String, String>,
    by_name: BTreeMap<String, Vec<(Option<String>, String)>>,
}

impl EnumIndex {
    fn insert(&mut self, enum_type: &Enum) {
        let name = enum_type.name.to_lowercase();
        let schema_name = enum_type
            .schema_name
            .as_ref()
            .map(|schema| schema.to_lowercase());

        self.by_name
            .entry(name.clone())
            .or_default()
            .push((schema_name.clone(), enum_type.id.clone()));

        if let Some(schema_name) = schema_name {
            self.exact
                .insert(format!("{schema_name}.{name}"), enum_type.id.clone());
        }
    }

    fn resolve(&self, table: &Table, data_type: &str) -> Option<String> {
        let data_type = data_type.to_lowercase();
        if data_type.contains('.')
            && let Some(enum_id) = self.exact.get(&data_type)
        {
            return Some(enum_id.clone());
        }

        let candidates = self.by_name.get(&data_type)?;
        if candidates.len() == 1 {
            return Some(candidates[0].1.clone());
        }

        if let Some(table_schema) = table.schema_name.as_deref().map(str::to_lowercase) {
            let matching: Vec<&(Option<String>, String)> = candidates
                .iter()
                .filter(|(schema_name, _)| schema_name.as_deref() == Some(table_schema.as_str()))
                .collect();
            if let [(_, enum_id)] = matching.as_slice() {
                return Some((*enum_id).clone());
            }
        }

        warn!(
            table = %table.qualified_name(),
            data_type = %data_type,
            "Ambiguous enum reference skipped"
        );
        None
    }
}

/// Request to build a layout graph from a schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutRequest {
    /// Filter specification for including/excluding tables.
    pub filter: FilterSpec,
    /// Optional focus specification.
    pub focus: Option<FocusSpec>,
    /// Grouping specification.
    pub grouping: GroupingSpec,
    /// Whether to collapse join tables into many-to-many edges.
    /// When enabled, join table candidates are hidden and direct edges
    /// are drawn between the tables they connect.
    #[serde(default)]
    pub collapse_join_tables: bool,
}

/// A graph suitable for layout algorithms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutGraph {
    /// All nodes in the graph.
    pub nodes: Vec<LayoutNode>,
    /// All edges in the graph.
    pub edges: Vec<LayoutEdge>,
    /// Groups for clustered layout.
    pub groups: Vec<LayoutGroup>,
    /// Map from table `stable_id` to node index.
    #[serde(skip)]
    pub node_index: BTreeMap<String, usize>,
    /// Map from node index to table `stable_id`.
    #[serde(skip)]
    pub reverse_index: BTreeMap<usize, String>,
}

/// A node in the layout graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutNode {
    /// Stable ID of the table.
    pub id: String,
    /// Qualified name (schema.table or just table).
    pub label: String,
    /// Schema name, if any.
    pub schema_name: Option<String>,
    /// Table name.
    pub table_name: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Column information for rendering.
    pub columns: Vec<LayoutColumn>,
    /// Number of inbound foreign keys.
    pub inbound_count: usize,
    /// Number of outbound foreign keys.
    pub outbound_count: usize,
    /// Whether this node has a self-referential FK.
    pub has_self_loop: bool,
    /// Whether this is likely a join table.
    pub is_join_table_candidate: bool,
    /// Index of the group this node belongs to (if any).
    pub group_index: Option<usize>,
}

/// Column information for layout nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutColumn {
    /// Column name.
    pub name: String,
    /// Column data type.
    pub data_type: String,
    /// Whether the column can be null.
    pub nullable: bool,
    /// Whether this column is part of the primary key.
    pub is_primary_key: bool,
}

/// An edge in the layout graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutEdge {
    /// Source table stable ID.
    pub from: String,
    /// Target table stable ID.
    pub to: String,
    /// Foreign key name (if any).
    pub name: Option<String>,
    /// Source columns.
    pub from_columns: Vec<String>,
    /// Target columns.
    pub to_columns: Vec<String>,
    /// Edge kind.
    pub kind: EdgeKind,
    /// Whether this edge is a self-loop.
    pub is_self_loop: bool,
    /// Whether the FK columns are nullable.
    pub nullable: bool,
    /// Whether this edge represents a collapsed join table (many-to-many relationship).
    #[serde(default)]
    pub is_collapsed_join: bool,
    /// If this is a collapsed join edge, contains information about the join table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed_join_table: Option<CollapsedJoinTable>,
}

/// Information about a collapsed join table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollapsedJoinTable {
    /// The ID of the join table that was collapsed.
    pub table_id: String,
    /// The label/name of the join table.
    pub table_label: String,
}

/// A group of nodes for clustered layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutGroup {
    /// Group identifier.
    pub id: String,
    /// Group label (schema name, prefix, etc.).
    pub label: String,
    /// Indices of nodes in this group.
    pub node_indices: Vec<usize>,
}

/// Builder for constructing layout graphs from schemas.
#[derive(Debug, Default)]
pub struct LayoutGraphBuilder {
    request: LayoutRequest,
}

impl LayoutGraphBuilder {
    /// Create a new builder with default request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the filter specification.
    #[must_use]
    pub fn filter(mut self, filter: FilterSpec) -> Self {
        self.request.filter = filter;
        self
    }

    /// Set the focus specification.
    #[must_use]
    pub fn focus(mut self, focus: Option<FocusSpec>) -> Self {
        self.request.focus = focus;
        self
    }

    /// Set the grouping specification.
    #[must_use]
    pub const fn grouping(mut self, grouping: GroupingSpec) -> Self {
        self.request.grouping = grouping;
        self
    }

    /// Set whether to collapse join tables.
    #[must_use]
    pub const fn collapse_join_tables(mut self, collapse: bool) -> Self {
        self.request.collapse_join_tables = collapse;
        self
    }

    /// Set the full request.
    #[must_use]
    pub fn request(mut self, request: LayoutRequest) -> Self {
        self.request = request;
        self
    }

    /// Build the layout graph from a schema.
    #[must_use]
    pub fn build(&self, schema: &Schema) -> LayoutGraph {
        // Step 1: Filter schema objects
        let filtered_tables = self.filter_tables(&schema.tables);
        let filtered_views = self.filter_views(&schema.views);
        let filtered_enums = self.filter_enums(&schema.enums);

        // Step 2: Build nodes with relationship counts
        let (mut nodes, mut edges) =
            self.build_nodes_and_edges(&filtered_tables, &filtered_views, &filtered_enums);

        // Step 3: Compute relationship counts
        self.compute_relationship_counts(&mut nodes, &edges);

        // Step 4: Mark join table candidates
        self.mark_join_table_candidates(&mut nodes, &edges);

        // Step 5: Collapse join tables if requested
        if self.request.collapse_join_tables {
            self.do_collapse_join_tables(&mut nodes, &mut edges);
        }

        // Step 6: Build groups
        let groups = self.build_groups(&nodes);

        // Step 7: Build indices
        let mut node_index = BTreeMap::new();
        let mut reverse_index = BTreeMap::new();
        for (i, node) in nodes.iter().enumerate() {
            node_index.insert(node.id.clone(), i);
            reverse_index.insert(i, node.id.clone());
        }

        // Step 8: Assign group indices to nodes
        for (group_idx, group) in groups.iter().enumerate() {
            for &node_idx in &group.node_indices {
                if let Some(node) = nodes.get_mut(node_idx) {
                    node.group_index = Some(group_idx);
                }
            }
        }

        LayoutGraph {
            nodes,
            edges,
            groups,
            node_index,
            reverse_index,
        }
    }

    /// Filter tables based on include/exclude patterns.
    fn filter_tables<'a>(&self, tables: &'a [Table]) -> Vec<&'a Table> {
        tables
            .iter()
            .filter(|table| self.matches_filter(&table.qualified_name(), &table.name))
            .collect()
    }

    fn filter_views<'a>(&self, views: &'a [View]) -> Vec<&'a View> {
        views
            .iter()
            .filter(|view| self.matches_filter(&view.qualified_name(), &view.name))
            .collect()
    }

    fn filter_enums<'a>(&self, enums: &'a [Enum]) -> Vec<&'a Enum> {
        enums
            .iter()
            .filter(|enum_type| self.matches_filter(&enum_type.qualified_name(), &enum_type.name))
            .collect()
    }

    fn matches_filter(&self, qualified_name: &str, short_name: &str) -> bool {
        let include_patterns = &self.request.filter.include;
        let exclude_patterns = &self.request.filter.exclude;

        if exclude_patterns.iter().any(|pattern| {
            matches_pattern(pattern, qualified_name) || matches_pattern(pattern, short_name)
        }) {
            return false;
        }

        include_patterns.is_empty()
            || include_patterns.iter().any(|pattern| {
                matches_pattern(pattern, qualified_name) || matches_pattern(pattern, short_name)
            })
    }

    /// Build nodes and edges from filtered schema objects.
    #[allow(clippy::unused_self)]
    #[allow(clippy::too_many_lines)] // Keeps node/edge wiring for all schema object kinds in one place.
    fn build_nodes_and_edges(
        &self,
        tables: &[&Table],
        views: &[&View],
        enums: &[&Enum],
    ) -> (Vec<LayoutNode>, Vec<LayoutEdge>) {
        let table_ids: BTreeSet<&str> = tables.iter().map(|t| t.stable_id.as_str()).collect();
        let table_names: BTreeSet<String> = tables
            .iter()
            .flat_map(|table| [table.name.to_lowercase(), table.stable_id.to_lowercase()])
            .collect();
        let view_ids: BTreeSet<&str> = views.iter().map(|view| view.id.as_str()).collect();
        let view_names: BTreeSet<String> = views
            .iter()
            .flat_map(|view| {
                [
                    view.name.to_lowercase(),
                    view.id.to_lowercase(),
                    view.qualified_name().to_lowercase(),
                ]
            })
            .collect();
        let mut enum_index = EnumIndex::default();
        for enum_type in enums {
            enum_index.insert(enum_type);
        }

        let mut nodes = Vec::with_capacity(tables.len() + views.len() + enums.len());
        let mut edges = Vec::new();

        for table in tables {
            let node = LayoutNode {
                id: table.stable_id.clone(),
                label: table.qualified_name(),
                schema_name: table.schema_name.clone(),
                table_name: table.name.clone(),
                kind: NodeKind::Table,
                columns: table
                    .columns
                    .iter()
                    .map(|c| LayoutColumn {
                        name: c.name.clone(),
                        data_type: c.data_type.clone(),
                        nullable: c.nullable,
                        is_primary_key: c.is_primary_key,
                    })
                    .collect(),
                inbound_count: 0,
                outbound_count: 0,
                has_self_loop: false,
                is_join_table_candidate: false,
                group_index: None,
            };
            nodes.push(node);

            // Build edges for foreign keys
            for fk in &table.foreign_keys {
                let is_self_loop = fk.to_table == table.stable_id || fk.to_table == table.name;

                // Determine if FK is nullable by checking source columns
                let fk_nullable = fk.from_columns.iter().all(|col_name| {
                    table
                        .columns
                        .iter()
                        .find(|c| &c.name == col_name)
                        .is_some_and(|c| c.nullable)
                });

                // Only include edges where both endpoints are in our filtered set
                // For self-loops, we always include them
                if is_self_loop
                    || table_ids.contains(fk.to_table.as_str())
                    || tables.iter().any(|t| t.name == fk.to_table)
                {
                    edges.push(LayoutEdge {
                        from: table.stable_id.clone(),
                        to: fk.to_table.clone(),
                        name: fk.name.clone(),
                        from_columns: fk.from_columns.clone(),
                        to_columns: fk.to_columns.clone(),
                        kind: EdgeKind::ForeignKey,
                        is_self_loop,
                        nullable: fk_nullable,
                        is_collapsed_join: false,
                        collapsed_join_table: None,
                    });
                }
            }

            for column in &table.columns {
                if let Some(enum_id) = enum_index.resolve(table, &column.data_type) {
                    edges.push(LayoutEdge {
                        from: table.stable_id.clone(),
                        to: enum_id,
                        name: Some(format!("{} ({})", column.name, column.data_type)),
                        from_columns: vec![column.name.clone()],
                        to_columns: vec![],
                        kind: EdgeKind::EnumReference,
                        is_self_loop: false,
                        nullable: column.nullable,
                        is_collapsed_join: false,
                        collapsed_join_table: None,
                    });
                }
            }
        }

        for view in views {
            nodes.push(LayoutNode {
                id: view.id.clone(),
                label: view.qualified_name(),
                schema_name: view.schema_name.clone(),
                table_name: view.name.clone(),
                kind: NodeKind::View,
                columns: view
                    .columns
                    .iter()
                    .map(|column| LayoutColumn {
                        name: column.name.clone(),
                        data_type: column.data_type.clone(),
                        nullable: column.nullable,
                        is_primary_key: false,
                    })
                    .collect(),
                inbound_count: 0,
                outbound_count: 0,
                has_self_loop: false,
                is_join_table_candidate: false,
                group_index: None,
            });

            if let Some(definition) = &view.definition {
                let definition = definition.to_lowercase();
                let mut seen_targets = BTreeSet::new();

                for table in tables {
                    let stable_id = table.stable_id.to_lowercase();
                    let table_name = table.name.to_lowercase();
                    if (table_names.contains(&stable_id)
                        && contains_identifier(&definition, &stable_id))
                        || (table_names.contains(&table_name)
                            && contains_identifier(&definition, &table_name))
                    {
                        seen_targets.insert(table.stable_id.clone());
                    }
                }

                for dependency_view in views {
                    if dependency_view.id == view.id {
                        continue;
                    }

                    let view_id = dependency_view.id.to_lowercase();
                    let view_name = dependency_view.name.to_lowercase();
                    let view_label = dependency_view.qualified_name().to_lowercase();
                    if (view_ids.contains(dependency_view.id.as_str())
                        && contains_identifier(&definition, &view_id))
                        || (view_names.contains(&view_name)
                            && contains_identifier(&definition, &view_name))
                        || (view_names.contains(&view_label)
                            && contains_identifier(&definition, &view_label))
                    {
                        seen_targets.insert(dependency_view.id.clone());
                    }
                }

                for target_id in seen_targets {
                    edges.push(LayoutEdge {
                        from: target_id,
                        to: view.id.clone(),
                        name: Some("view dep".to_string()),
                        from_columns: vec![],
                        to_columns: vec![],
                        kind: EdgeKind::ViewDependency,
                        is_self_loop: false,
                        nullable: false,
                        is_collapsed_join: false,
                        collapsed_join_table: None,
                    });
                }
            }
        }

        for enum_type in enums {
            nodes.push(LayoutNode {
                id: enum_type.id.clone(),
                label: enum_type.qualified_name(),
                schema_name: enum_type.schema_name.clone(),
                table_name: enum_type.name.clone(),
                kind: NodeKind::Enum,
                columns: enum_type
                    .values
                    .iter()
                    .map(|value| LayoutColumn {
                        name: value.clone(),
                        data_type: String::new(),
                        nullable: false,
                        is_primary_key: false,
                    })
                    .collect(),
                inbound_count: 0,
                outbound_count: 0,
                has_self_loop: false,
                is_join_table_candidate: false,
                group_index: None,
            });
        }

        (nodes, edges)
    }

    /// Compute inbound/outbound relationship counts for each node.
    #[allow(clippy::unused_self)]
    fn compute_relationship_counts(&self, nodes: &mut [LayoutNode], edges: &[LayoutEdge]) {
        // Internal enum for tracking update types
        enum UpdateKind {
            SelfLoop,
            Inbound,
            Outbound,
        }

        // First, collect all updates needed
        let mut updates: Vec<(usize, UpdateKind)> = Vec::new();

        let node_ids: BTreeMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();

        for edge in edges {
            if edge.is_self_loop {
                if let Some(&idx) = node_ids.get(edge.from.as_str()) {
                    updates.push((idx, UpdateKind::SelfLoop));
                }
                continue;
            }

            if let Some(&from_idx) = node_ids.get(edge.from.as_str()) {
                updates.push((from_idx, UpdateKind::Outbound));
            }
            if let Some(&to_idx) = node_ids.get(edge.to.as_str()) {
                updates.push((to_idx, UpdateKind::Inbound));
            }
        }

        // Apply updates
        for (idx, kind) in updates {
            match kind {
                UpdateKind::SelfLoop => nodes[idx].has_self_loop = true,
                UpdateKind::Inbound => nodes[idx].inbound_count += 1,
                UpdateKind::Outbound => nodes[idx].outbound_count += 1,
            }
        }
    }

    fn is_join_table_metadata_column(column: &LayoutColumn) -> bool {
        column.is_primary_key
            || matches!(
                column.name.as_str(),
                "id" | "created_at"
                    | "updated_at"
                    | "created_on"
                    | "updated_on"
                    | "deleted_at"
                    | "sort_order"
                    | "position"
            )
    }

    /// Mark nodes that are likely join tables.
    #[allow(clippy::unused_self)]
    fn mark_join_table_candidates(&self, nodes: &mut [LayoutNode], edges: &[LayoutEdge]) {
        // Collect join table candidates first
        let mut candidates: Vec<usize> = Vec::new();

        for (idx, node) in nodes.iter().enumerate() {
            if node.kind != NodeKind::Table {
                continue;
            }

            let outbound_fks: Vec<_> = edges
                .iter()
                .filter(|e| e.from == node.id && !e.is_self_loop && e.kind == EdgeKind::ForeignKey)
                .collect();

            if outbound_fks.len() != 2 {
                continue;
            }

            let target_tables: BTreeSet<&str> =
                outbound_fks.iter().map(|e| e.to.as_str()).collect();
            if target_tables.len() != 2 {
                continue;
            }

            let fk_columns: BTreeSet<&str> = outbound_fks
                .iter()
                .flat_map(|e| e.from_columns.iter().map(String::as_str))
                .collect();

            let fk_column_count = node
                .columns
                .iter()
                .filter(|c| fk_columns.contains(c.name.as_str()))
                .count();
            if fk_column_count < 2 {
                continue;
            }

            if node.columns.iter().all(|column| {
                fk_columns.contains(column.name.as_str())
                    || Self::is_join_table_metadata_column(column)
            }) {
                candidates.push(idx);
            }
        }

        // Apply markings
        for idx in candidates {
            nodes[idx].is_join_table_candidate = true;
        }
    }

    /// Collapse join tables, removing them from the graph and creating
    /// direct edges between the tables they connect.
    #[allow(clippy::similar_names)]
    #[allow(clippy::unused_self)]
    fn do_collapse_join_tables(&self, nodes: &mut Vec<LayoutNode>, edges: &mut Vec<LayoutEdge>) {
        use tracing::debug;

        // Find all join table candidates to collapse
        let join_table_ids: BTreeSet<String> = nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Table && n.is_join_table_candidate)
            .map(|n| n.id.clone())
            .collect();

        if join_table_ids.is_empty() {
            debug!("No join table candidates to collapse");
            return;
        }

        debug!("Collapsing {} join table candidates", join_table_ids.len());

        // For each join table, find the two tables it connects and create a direct edge
        let mut new_edges: Vec<LayoutEdge> = Vec::new();
        let mut edges_to_remove: BTreeSet<usize> = BTreeSet::new();

        for join_table_id in &join_table_ids {
            // Find edges from this join table to other tables
            let outgoing_edges: Vec<(usize, &LayoutEdge)> = edges
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    &e.from == join_table_id && !e.is_self_loop && e.kind == EdgeKind::ForeignKey
                })
                .collect();

            // A join table should have exactly 2 outgoing edges
            if outgoing_edges.len() != 2 {
                debug!(
                    "Join table {} has {} outgoing edges, skipping",
                    join_table_id,
                    outgoing_edges.len()
                );
                continue;
            }

            let (idx_a, edge1) = outgoing_edges[0];
            let (idx_b, edge2) = outgoing_edges[1];

            // Find the join table node to get its label
            let join_table_label = nodes
                .iter()
                .find(|n| &n.id == join_table_id)
                .map_or_else(|| join_table_id.clone(), |n| n.label.clone());

            // Create a new edge connecting the two tables directly
            // The edge goes from edge1.to to edge2.to (both targets of the join table's FKs)
            let collapsed_edge = LayoutEdge {
                from: edge1.to.clone(),
                to: edge2.to.clone(),
                name: Some(format!("m2m:{join_table_label}")),
                from_columns: edge1.to_columns.clone(),
                to_columns: edge2.to_columns.clone(),
                kind: EdgeKind::ForeignKey,
                is_self_loop: edge1.to == edge2.to,
                nullable: edge1.nullable && edge2.nullable,
                is_collapsed_join: true,
                collapsed_join_table: Some(CollapsedJoinTable {
                    table_id: join_table_id.clone(),
                    table_label: join_table_label,
                }),
            };

            new_edges.push(collapsed_edge);
            edges_to_remove.insert(idx_a);
            edges_to_remove.insert(idx_b);

            // Also mark any incoming edges to the join table for removal
            for (idx, edge) in edges.iter().enumerate() {
                if &edge.to == join_table_id {
                    edges_to_remove.insert(idx);
                }
            }
        }

        // Remove the collapsed join table nodes
        nodes.retain(|n| !join_table_ids.contains(&n.id));

        // Remove edges that were replaced or connected to collapsed tables
        // and add the new collapsed edges
        let mut retained_edges: Vec<LayoutEdge> = Vec::new();
        for (idx, edge) in edges.drain(..).enumerate() {
            if !edges_to_remove.contains(&idx) {
                // Also skip edges that connect to/from collapsed tables
                if !join_table_ids.contains(&edge.from) && !join_table_ids.contains(&edge.to) {
                    retained_edges.push(edge);
                }
            }
        }
        retained_edges.extend(new_edges);
        *edges = retained_edges;

        debug!(
            "After collapse: {} nodes, {} edges",
            nodes.len(),
            edges.len()
        );
    }

    /// Build groups based on grouping strategy.
    fn build_groups(&self, nodes: &[LayoutNode]) -> Vec<LayoutGroup> {
        match self.request.grouping.strategy {
            GroupingStrategy::None => Vec::new(),
            GroupingStrategy::BySchema => self.group_by_schema(nodes),
            GroupingStrategy::ByPrefix => self.group_by_prefix(nodes),
        }
    }

    /// Group nodes by schema name.
    #[allow(clippy::unused_self)]
    fn group_by_schema(&self, nodes: &[LayoutNode]) -> Vec<LayoutGroup> {
        let mut schema_groups: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();

        for (idx, node) in nodes.iter().enumerate() {
            schema_groups
                .entry(node.schema_name.clone())
                .or_default()
                .push(idx);
        }

        schema_groups
            .into_iter()
            .enumerate()
            .map(|(group_idx, (schema_name, node_indices))| LayoutGroup {
                id: format!("schema_{group_idx}"),
                label: schema_name.unwrap_or_else(|| "public".to_string()),
                node_indices,
            })
            .collect()
    }

    /// Group nodes by table name prefix.
    #[allow(clippy::unused_self)]
    fn group_by_prefix(&self, nodes: &[LayoutNode]) -> Vec<LayoutGroup> {
        // Extract common prefixes (e.g., "user_", "order_", etc.)
        let mut prefix_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();

        for (idx, node) in nodes.iter().enumerate() {
            let prefix = extract_prefix(&node.table_name);
            prefix_groups.entry(prefix).or_default().push(idx);
        }

        prefix_groups
            .into_iter()
            .enumerate()
            .map(|(group_idx, (prefix, node_indices))| LayoutGroup {
                id: format!("prefix_{group_idx}"),
                label: prefix,
                node_indices,
            })
            .collect()
    }
}

/// Check if a string matches a glob pattern (simple implementation).
fn matches_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.starts_with('*') && pattern.ends_with('*') {
        let middle = &pattern[1..pattern.len() - 1];
        return value.contains(middle);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return value.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    value == pattern
}

/// Extract a prefix from a table name (e.g., "`user_profile`" -> "user_").
fn extract_prefix(name: &str) -> String {
    // Try to find common delimiter patterns
    if let Some(underscore_pos) = name.find('_')
        && underscore_pos > 0
    {
        return format!("{}_", &name[..underscore_pos]);
    }

    // Fall back to first 3 characters or full name
    if name.len() > 6 {
        format!("{}...", &name[..3])
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::{Column, ColumnId, TableId};

    #[test]
    fn test_matches_pattern() {
        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("*_test", "my_test"));
        assert!(matches_pattern("test_*", "test_value"));
        assert!(matches_pattern("*user*", "my_user_table"));
        assert!(matches_pattern("exact", "exact"));
        assert!(!matches_pattern("exact", "not_exact"));
    }

    #[test]
    fn test_extract_prefix() {
        assert_eq!(extract_prefix("user_profile"), "user_");
        assert_eq!(extract_prefix("order_items"), "order_");
        assert_eq!(extract_prefix("abc"), "abc");
    }

    #[test]
    fn test_build_includes_view_and_enum_nodes() {
        let schema = Schema {
            tables: vec![Table {
                id: TableId(1),
                stable_id: "users".to_string(),
                schema_name: None,
                name: "users".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    },
                    Column {
                        id: ColumnId(2),
                        name: "status".to_string(),
                        data_type: "status".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                    },
                ],
                foreign_keys: vec![],
                indexes: vec![],
                comment: None,
            }],
            views: vec![View {
                id: "active_users".to_string(),
                schema_name: None,
                name: "active_users".to_string(),
                columns: vec![Column {
                    id: ColumnId(3),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: false,
                    comment: None,
                }],
                definition: Some("SELECT id FROM users".to_string()),
            }],
            enums: vec![Enum {
                id: "status".to_string(),
                schema_name: None,
                name: "status".to_string(),
                values: vec!["active".to_string(), "inactive".to_string()],
            }],
        };

        let graph = LayoutGraphBuilder::new().build(&schema);

        assert_eq!(graph.nodes.len(), 3);
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "users" && node.kind == NodeKind::Table)
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "active_users" && node.kind == NodeKind::View)
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "status" && node.kind == NodeKind::Enum)
        );

        assert!(graph.edges.iter().any(|edge| {
            edge.from == "users" && edge.to == "status" && edge.kind == EdgeKind::EnumReference
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.from == "users"
                && edge.to == "active_users"
                && edge.kind == EdgeKind::ViewDependency
        }));
    }

    #[test]
    fn test_ambiguous_enum_reference_is_skipped() {
        let schema = Schema {
            tables: vec![Table {
                id: TableId(1),
                stable_id: "accounts".to_string(),
                schema_name: None,
                name: "accounts".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    },
                    Column {
                        id: ColumnId(2),
                        name: "status".to_string(),
                        data_type: "status".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                    },
                ],
                foreign_keys: vec![],
                indexes: vec![],
                comment: None,
            }],
            views: vec![],
            enums: vec![
                Enum {
                    id: "public.status".to_string(),
                    schema_name: Some("public".to_string()),
                    name: "status".to_string(),
                    values: vec!["active".to_string()],
                },
                Enum {
                    id: "auth.status".to_string(),
                    schema_name: Some("auth".to_string()),
                    name: "status".to_string(),
                    values: vec!["pending".to_string()],
                },
            ],
        };

        let graph = LayoutGraphBuilder::new().build(&schema);

        assert_eq!(graph.nodes.len(), 3);
        assert!(
            graph
                .edges
                .iter()
                .all(|edge| edge.kind != EdgeKind::EnumReference)
        );
    }

    #[test]
    fn test_collapse_join_tables_option() {
        // Test that collapse_join_tables defaults to false
        let request = LayoutRequest::default();
        assert!(!request.collapse_join_tables);

        // Test that it can be set to true
        let request = LayoutRequest {
            collapse_join_tables: true,
            ..Default::default()
        };
        assert!(request.collapse_join_tables);
    }

    #[test]
    fn test_builder_collapse_join_tables() {
        let builder = LayoutGraphBuilder::new().collapse_join_tables(true);
        assert!(builder.request.collapse_join_tables);

        let builder = LayoutGraphBuilder::new().collapse_join_tables(false);
        assert!(!builder.request.collapse_join_tables);
    }
}

#[cfg(test)]
mod collapse_tests {
    use super::*;
    use relune_core::{Column, ColumnId, ForeignKey, ReferentialAction, Table, TableId};

    /// Create a schema with a classic many-to-many relationship:
    /// users <-> `user_roles` <-> roles
    #[allow(clippy::too_many_lines)]
    fn make_many_to_many_schema() -> Schema {
        Schema {
            tables: vec![
                // users table
                Table {
                    id: TableId(1),
                    stable_id: "users".to_string(),
                    schema_name: None,
                    name: "users".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(1),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(2),
                            name: "name".to_string(),
                            data_type: "varchar".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                // roles table
                Table {
                    id: TableId(2),
                    stable_id: "roles".to_string(),
                    schema_name: None,
                    name: "roles".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(3),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(4),
                            name: "name".to_string(),
                            data_type: "varchar".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                // user_roles join table
                Table {
                    id: TableId(3),
                    stable_id: "user_roles".to_string(),
                    schema_name: None,
                    name: "user_roles".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(5),
                            name: "user_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(6),
                            name: "role_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![
                        ForeignKey {
                            name: Some("fk_user_roles_user".to_string()),
                            from_columns: vec!["user_id".to_string()],
                            to_schema: None,
                            to_table: "users".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                        ForeignKey {
                            name: Some("fk_user_roles_role".to_string()),
                            from_columns: vec!["role_id".to_string()],
                            to_schema: None,
                            to_table: "roles".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                    ],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        }
    }

    #[test]
    fn test_join_table_detection() {
        let schema = make_many_to_many_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);

        // Find the user_roles node
        let user_roles = graph.nodes.iter().find(|n| n.id == "user_roles");
        assert!(user_roles.is_some());
        let user_roles = user_roles.unwrap();

        // It should be marked as a join table candidate
        assert!(user_roles.is_join_table_candidate);

        // users and roles should NOT be join table candidates
        let users = graph.nodes.iter().find(|n| n.id == "users").unwrap();
        let roles = graph.nodes.iter().find(|n| n.id == "roles").unwrap();
        assert!(!users.is_join_table_candidate);
        assert!(!roles.is_join_table_candidate);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn test_join_table_detection_allows_metadata_and_inbound_edges() {
        let schema = Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "users".to_string(),
                    schema_name: None,
                    name: "users".to_string(),
                    columns: vec![Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(2),
                    stable_id: "roles".to_string(),
                    schema_name: None,
                    name: "roles".to_string(),
                    columns: vec![Column {
                        id: ColumnId(2),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(3),
                    stable_id: "user_roles".to_string(),
                    schema_name: None,
                    name: "user_roles".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(3),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(4),
                            name: "user_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(5),
                            name: "role_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(6),
                            name: "created_at".to_string(),
                            data_type: "timestamp".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![
                        ForeignKey {
                            name: Some("fk_user_roles_user".to_string()),
                            from_columns: vec!["user_id".to_string()],
                            to_schema: None,
                            to_table: "users".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                        ForeignKey {
                            name: Some("fk_user_roles_role".to_string()),
                            from_columns: vec!["role_id".to_string()],
                            to_schema: None,
                            to_table: "roles".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                    ],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(4),
                    stable_id: "audit_logs".to_string(),
                    schema_name: None,
                    name: "audit_logs".to_string(),
                    columns: vec![Column {
                        id: ColumnId(7),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![ForeignKey {
                        name: Some("fk_audit_logs_user_roles".to_string()),
                        from_columns: vec!["id".to_string()],
                        to_schema: None,
                        to_table: "user_roles".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        };

        let graph = LayoutGraphBuilder::new().build(&schema);
        let user_roles = graph.nodes.iter().find(|n| n.id == "user_roles").unwrap();
        assert!(user_roles.is_join_table_candidate);
    }

    #[test]
    fn test_collapse_join_tables_removes_node() {
        let schema = make_many_to_many_schema();

        // Without collapsing
        let graph_no_collapse = LayoutGraphBuilder::new()
            .collapse_join_tables(false)
            .build(&schema);
        assert_eq!(graph_no_collapse.nodes.len(), 3);
        assert_eq!(graph_no_collapse.edges.len(), 2); // user_roles -> users, user_roles -> roles

        // With collapsing
        let graph_collapsed = LayoutGraphBuilder::new()
            .collapse_join_tables(true)
            .build(&schema);

        // Should have 2 nodes (users and roles, user_roles removed)
        assert_eq!(graph_collapsed.nodes.len(), 2);

        // user_roles should not be in the nodes
        assert!(graph_collapsed.nodes.iter().all(|n| n.id != "user_roles"));

        // Should have 1 edge (users <-> roles)
        assert_eq!(graph_collapsed.edges.len(), 1);
    }

    #[test]
    fn test_collapsed_edge_properties() {
        let schema = make_many_to_many_schema();

        let graph = LayoutGraphBuilder::new()
            .collapse_join_tables(true)
            .build(&schema);

        assert_eq!(graph.edges.len(), 1);
        let edge = &graph.edges[0];

        // The edge should be marked as a collapsed join
        assert!(edge.is_collapsed_join);

        // Should have collapsed join table info
        assert!(edge.collapsed_join_table.is_some());
        let collapsed = edge.collapsed_join_table.as_ref().unwrap();
        assert_eq!(collapsed.table_id, "user_roles");
        assert_eq!(collapsed.table_label, "user_roles");

        // Label should indicate many-to-many
        assert!(edge.name.as_ref().unwrap().starts_with("m2m:"));
    }

    #[test]
    fn test_non_join_tables_not_collapsed() {
        // Create a schema where tables are not join table candidates
        let schema = Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "users".to_string(),
                    schema_name: None,
                    name: "users".to_string(),
                    columns: vec![Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(2),
                    stable_id: "posts".to_string(),
                    schema_name: None,
                    name: "posts".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(2),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(3),
                            name: "user_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![ForeignKey {
                        name: Some("fk_posts_user".to_string()),
                        from_columns: vec!["user_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        };

        let graph = LayoutGraphBuilder::new()
            .collapse_join_tables(true)
            .build(&schema);

        // No tables should be collapsed (posts has only 1 outbound FK)
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        assert!(!graph.edges[0].is_collapsed_join);
    }
}
