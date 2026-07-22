//! `CREATE VIEW` parsing and column extraction.

use crate::context::{LineOffsets, ParseContext};
use crate::names::{normalized_stable_id, split_object_name_with_diagnostics};
use crate::query_columns::columns_from_query;
use relune_core::{Column, ColumnId, View, normalize_identifier};
use sqlparser::ast::ObjectName;

/// Parse a CREATE VIEW statement into a View.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn parse_create_view(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    view_columns: &[sqlparser::ast::ViewColumnDef],
    query: &sqlparser::ast::Query,
) -> Option<View> {
    let (schema_name, view_name) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "CREATE VIEW");

    // Generate a stable ID for the view
    let id = normalized_stable_id(schema_name.as_deref(), &view_name);

    // Get the query definition as a string
    let definition = query.to_string();

    // Normalize names
    let normalized_schema = schema_name.map(|s| normalize_identifier(&s));
    let normalized_name = normalize_identifier(&view_name);

    // Extract columns: prefer explicit column list, fall back to SELECT items
    let columns = if view_columns.is_empty() {
        columns_from_query(query)
    } else {
        extract_view_columns_from_defs(view_columns)
    };

    Some(View {
        id,
        schema_name: normalized_schema,
        name: normalized_name,
        columns,
        definition: Some(definition),
    })
}

/// Extract columns from explicit VIEW column definitions.
fn extract_view_columns_from_defs(defs: &[sqlparser::ast::ViewColumnDef]) -> Vec<Column> {
    defs.iter()
        .enumerate()
        .map(|(i, def)| {
            let data_type = def
                .data_type
                .as_ref()
                .map_or_else(|| "unknown".to_string(), std::string::ToString::to_string);
            Column {
                id: ColumnId((i as u64) + 1),
                name: normalize_identifier(&def.name.value),
                data_type,
                nullable: true,
                is_primary_key: false,
                comment: None,
                enum_values: None,
                semantics: relune_core::ColumnSemantics::default(),
            }
        })
        .collect()
}
