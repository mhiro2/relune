//! `CREATE VIEW` parsing and column extraction.

use crate::context::{LineOffsets, ParseContext};
use crate::names::{normalized_stable_id, split_object_name_with_diagnostics};
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
        extract_view_columns_from_query(query)
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
            }
        })
        .collect()
}

/// Extract column names from the top-level `SELECT` items in a view query.
///
/// Complex queries such as nested subqueries, set operations, or wildcard-only
/// projections may not yield derived column names here unless the view declares
/// them explicitly in `CREATE VIEW ... (col1, col2)`.
fn extract_view_columns_from_query(query: &sqlparser::ast::Query) -> Vec<Column> {
    use sqlparser::ast::{SelectItem, SetExpr};

    let SetExpr::Select(select) = query.body.as_ref() else {
        return Vec::new();
    };

    let mut columns = Vec::new();
    for (i, item) in select.projection.iter().enumerate() {
        let col_name = match item {
            SelectItem::UnnamedExpr(expr) => extract_expr_column_name(expr),
            SelectItem::ExprWithAlias { alias, .. } => Some(normalize_identifier(&alias.value)),
            SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _) => None,
        };
        if let Some(name) = col_name {
            columns.push(Column {
                id: ColumnId((i as u64) + 1),
                name,
                data_type: "unknown".to_string(),
                nullable: true,
                is_primary_key: false,
                comment: None,
                enum_values: None,
            });
        }
    }
    columns
}

/// Try to extract a column name from a simple expression.
fn extract_expr_column_name(expr: &sqlparser::ast::Expr) -> Option<String> {
    use sqlparser::ast::Expr;

    match expr {
        Expr::Identifier(ident) => Some(normalize_identifier(&ident.value)),
        Expr::CompoundIdentifier(parts) => {
            // Take the last part (e.g., "t.column_name" -> "column_name")
            parts.last().map(|ident| normalize_identifier(&ident.value))
        }
        _ => None,
    }
}
