//! Core library for relune - database schema analysis and visualization.
//!
//! This crate provides data structures and algorithms for parsing,
//! analyzing, and visualizing database schemas.
//!
//! ## Modules
//!
//! - [`config`] - Configuration types for filtering and layout hints
//! - [`diagnostic`] - Diagnostic messages and error codes
//! - [`diff`] - Schema diff engine for comparing schemas
//! - [`export`] - Stable export format for JSON serialization
//! - [`graph`] - Graph representation of schema relationships
//! - [`layout`] - Layout type definitions for positioned nodes
//! - [`lint`] - Lint engine for schema analysis
//! - [`model`] - Core data model types
//!
//! ## Feature Flags
//!
//! - `std` (default) - Standard library support
//! - `serde` (default) - Serialization support via serde
//!
//! ## SQL Dialect Support
//!
//! Use [`SqlDialect`] to specify which SQL dialect to parse:
//! - `Auto` (default) - Automatically detect from SQL content
//! - `Postgres` - `PostgreSQL` dialect
//! - `Mysql` - `MySQL` dialect
//! - `Sqlite` - `SQLite` dialect

/// Configuration types for filtering and layout hints.
pub mod config;
/// Diagnostic messages and error codes.
pub mod diagnostic;
/// Schema diff engine for comparing schemas.
pub mod diff;
/// Stable export format for JSON serialization.
pub mod export;
/// Graph representation of schema relationships.
pub mod graph;
/// Layout type definitions for positioned nodes.
pub mod layout;
/// Lint engine for schema analysis.
pub mod lint;
/// Core data model types.
pub mod model;

// Re-exports for convenience
pub use config::{
    FilterSpec, FocusSpec, GroupingSpec, GroupingStrategy, LayoutAlgorithm, LayoutCompactionSpec,
    LayoutDirection, LayoutSpec,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
pub use diff::{ChangeKind, SchemaDiff, diff_schemas};
pub use graph::{EdgeKind, GraphBuildError, GraphEdge, GraphNode, NodeKind, SchemaGraph};
pub use layout::{Cardinality, EdgeRoute, RouteStyle};
pub use lint::{LintIssue, LintResult, LintRuleId, LintStats, lint_schema};
pub use model::{
    Column, ColumnId, Enum, ForeignKey, Index, ReferentialAction, Schema, SchemaStats, SqlDialect,
    Table, TableId, ValidationError, View, normalize_identifier,
};
