//! Graph representation of a database schema.

use std::collections::HashSet;
use std::ops::ControlFlow;

use serde::{Deserialize, Serialize};
use sqlparser::ast::{ObjectName, ObjectNamePart, Query, Visit, Visitor};
use sqlparser::dialect::{Dialect, GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use thiserror::Error;

use crate::model::{Table, View, normalize_identifier};

/// The kind of node in the schema graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// A database table.
    Table,
    /// A database view.
    View,
    /// An enum type.
    Enum,
}

/// The kind of edge in the schema graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// A foreign key relationship between tables.
    ForeignKey,
    /// A column references an enum type.
    EnumReference,
    /// A view depends on a table or another view.
    ViewDependency,
}

/// A normalized relation reference extracted from SQL.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SqlRelation {
    /// Referenced schema name, if qualified in SQL.
    pub schema_name: Option<String>,
    /// Referenced relation name.
    pub name: String,
}

impl SqlRelation {
    /// Returns `true` if this SQL relation reference resolves to `table`.
    ///
    /// Schema-qualified references require the schema to match. Bare
    /// references match by name (or stable id) and are filtered by
    /// `referencing_schema`: if provided, only tables in the same schema or
    /// tables with no schema qualifier are considered, which avoids over-
    /// linking when the same name exists in multiple schemas.
    #[must_use]
    pub fn matches_table(&self, table: &Table, referencing_schema: Option<&str>) -> bool {
        let table_name = table.name.to_lowercase();
        let stable_id = table.stable_id.to_lowercase();

        if let Some(reference_schema) = self.schema_name.as_deref() {
            table.schema_name.as_deref().is_some_and(|table_schema| {
                table_schema.to_lowercase() == reference_schema && table_name == self.name
            })
        } else {
            let name_matches = self.name == table_name || self.name == stable_id;
            if !name_matches {
                return false;
            }
            schema_visible_from(table.schema_name.as_deref(), referencing_schema)
        }
    }

    /// Returns `true` if this SQL relation reference resolves to `view`.
    ///
    /// Schema-qualified references require the schema to match. Bare
    /// references match by name, id, or qualified name and are filtered by
    /// `referencing_schema` so a view does not depend on a same-named view
    /// in an unrelated schema.
    #[must_use]
    pub fn matches_view(&self, view: &View, referencing_schema: Option<&str>) -> bool {
        let view_name = view.name.to_lowercase();
        let view_id = view.id.to_lowercase();
        let view_label = view.qualified_name().to_lowercase();

        if let Some(reference_schema) = self.schema_name.as_deref() {
            view.schema_name.as_deref().is_some_and(|view_schema| {
                view_schema.to_lowercase() == reference_schema && view_name == self.name
            })
        } else {
            let name_matches =
                self.name == view_name || self.name == view_id || self.name == view_label;
            if !name_matches {
                return false;
            }
            schema_visible_from(view.schema_name.as_deref(), referencing_schema)
        }
    }
}

/// Decides whether a relation in `target_schema` is visible to a bare
/// reference written from a relation in `referencing_schema`.
///
/// A target with no schema qualifier is treated as visible from anywhere;
/// a qualified target is only visible when the schemas match. When the
/// caller has no schema context, all targets are considered visible.
const fn schema_visible_from(
    target_schema: Option<&str>,
    referencing_schema: Option<&str>,
) -> bool {
    match (target_schema, referencing_schema) {
        (None, _) | (_, None) => true,
        (Some(target), Some(reference)) => target.eq_ignore_ascii_case(reference),
    }
}

#[derive(Debug, Default)]
struct RelationCollector {
    cte_scopes: Vec<HashSet<String>>,
    references: HashSet<SqlRelation>,
}

impl Visitor for RelationCollector {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        let cte_names = query.with.as_ref().map_or_else(HashSet::new, |with| {
            with.cte_tables
                .iter()
                .map(|cte| normalize_identifier(&cte.alias.name.value))
                .collect()
        });
        self.cte_scopes.push(cte_names);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        let _ = self.cte_scopes.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<Self::Break> {
        if let Some(reference) = object_name_to_relation(relation)
            && !self.is_cte_reference(&reference)
        {
            self.references.insert(reference);
        }
        ControlFlow::Continue(())
    }
}

impl RelationCollector {
    fn is_cte_reference(&self, reference: &SqlRelation) -> bool {
        reference.schema_name.is_none()
            && self
                .cte_scopes
                .iter()
                .rev()
                .any(|scope| scope.contains(&reference.name))
    }
}

fn object_name_to_relation(name: &ObjectName) -> Option<SqlRelation> {
    let parts: Vec<String> = name
        .0
        .iter()
        .filter_map(object_name_identifier)
        .map(normalize_identifier)
        .collect();
    match parts.as_slice() {
        [name] => Some(SqlRelation {
            schema_name: None,
            name: name.clone(),
        }),
        [.., schema_name, name] => Some(SqlRelation {
            schema_name: Some(schema_name.clone()),
            name: name.clone(),
        }),
        [] => None,
    }
}

const fn object_name_identifier(part: &ObjectNamePart) -> Option<&str> {
    match part {
        ObjectNamePart::Identifier(ident) => Some(ident.value.as_str()),
        ObjectNamePart::Function(_) => None,
    }
}

/// Error returned when no supported SQL dialect can parse a relation
/// definition (e.g. a view's SQL body) for dependency extraction.
///
/// The error preserves a short snippet of the failing source so callers can
/// surface a useful diagnostic instead of silently dropping the view's
/// dependency edges.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "no supported SQL dialect could parse the relation definition for dependency extraction: {snippet}"
)]
pub struct SqlRelationParseError {
    /// Truncated snippet of the definition that failed to parse.
    pub snippet: String,
}

const SQL_RELATION_PARSE_SNIPPET_LIMIT: usize = 80;

fn snippet_for_definition(definition: &str) -> String {
    let trimmed = definition.trim();
    let mut buffer = String::with_capacity(SQL_RELATION_PARSE_SNIPPET_LIMIT + 1);
    for ch in trimmed.chars() {
        if buffer.len() + ch.len_utf8() > SQL_RELATION_PARSE_SNIPPET_LIMIT {
            buffer.push('…');
            break;
        }
        buffer.push(ch);
    }
    buffer
}

/// Collects normalized table/view references from a SQL fragment.
///
/// The result excludes CTE aliases so callers can reason about actual
/// relation dependencies without comment or alias false positives.
///
/// # Errors
///
/// Returns [`SqlRelationParseError`] when no supported dialect can parse
/// `definition`. Callers should surface this as a diagnostic rather than
/// silently treat the view as having no dependencies, otherwise an
/// unparsable view becomes indistinguishable from a genuinely orphan view.
pub fn collect_sql_relations(
    definition: &str,
) -> Result<HashSet<SqlRelation>, SqlRelationParseError> {
    let generic = GenericDialect {};
    let postgres = PostgreSqlDialect {};
    let mysql = MySqlDialect {};
    let sqlite = SQLiteDialect {};
    let dialects: [&dyn Dialect; 4] = [&generic, &postgres, &mysql, &sqlite];

    for dialect in dialects {
        let Ok(statements) = Parser::parse_sql(dialect, definition) else {
            continue;
        };
        let mut collector = RelationCollector::default();
        let _ = statements.visit(&mut collector);
        return Ok(collector.references);
    }

    Err(SqlRelationParseError {
        snippet: snippet_for_definition(definition),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_sql_relations_returns_err_when_no_dialect_can_parse() {
        let definition = "this is not valid sql ;;;;";
        let error = collect_sql_relations(definition).expect_err("definition should not parse");
        assert!(!error.snippet.is_empty());
        assert!(definition.starts_with(error.snippet.trim_end_matches('…')));
    }

    #[test]
    fn collect_sql_relations_returns_ok_for_parseable_definition() {
        let references = collect_sql_relations("select id from public.users")
            .expect("definition should parse with at least one supported dialect");
        assert!(references.iter().any(|reference| reference.name == "users"));
    }
}
