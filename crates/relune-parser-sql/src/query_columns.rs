//! Column derivation from a `SELECT` query, shared by `CREATE VIEW` and
//! `CREATE TABLE ... AS SELECT`.

use relune_core::{Column, ColumnId, normalize_identifier};

/// Derive columns from the output projection of a query.
///
/// Data types are reported as `unknown` because deriving them would require
/// resolving the underlying tables and expressions. Parenthesized queries and
/// set operations (`UNION` etc.) resolve to the names of their left-most
/// `SELECT`, mirroring SQL's output-column naming. Wildcard-only projections
/// yield no names; callers that need columns there should declare them
/// explicitly.
pub(crate) fn columns_from_query(query: &sqlparser::ast::Query) -> Vec<Column> {
    let names = projection_names(query.body.as_ref());

    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| Column {
            id: ColumnId((index as u64) + 1),
            name,
            data_type: "unknown".to_string(),
            nullable: true,
            is_primary_key: false,
            comment: None,
            enum_values: None,
        })
        .collect()
}

/// Resolve the output column names of a query body, recursing through
/// parenthesized queries and into the left operand of set operations.
fn projection_names(set_expr: &sqlparser::ast::SetExpr) -> Vec<String> {
    use sqlparser::ast::{SelectItem, SetExpr};

    match set_expr {
        SetExpr::Select(select) => {
            // Fail closed: derive names only when every projection item yields
            // one. A wildcard (unresolvable without the source tables) or an
            // unnamed non-column expression (e.g. `count(*)`) makes the column
            // set incomplete, and recording the derivable subset would assert a
            // misleading partial schema. Returning empty lets CTAS warn instead.
            let mut names = Vec::new();
            for item in &select.projection {
                match item {
                    SelectItem::UnnamedExpr(expr) => match extract_expr_column_name(expr) {
                        Some(name) => names.push(name),
                        None => return Vec::new(),
                    },
                    SelectItem::ExprWithAlias { alias, .. } => {
                        names.push(normalize_identifier(&alias.value));
                    }
                    SelectItem::ExprWithAliases { aliases, .. } => {
                        names.extend(
                            aliases
                                .iter()
                                .map(|alias| normalize_identifier(&alias.value)),
                        );
                    }
                    SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _) => {
                        return Vec::new();
                    }
                }
            }
            names
        }
        SetExpr::Query(query) => projection_names(query.body.as_ref()),
        SetExpr::SetOperation { left, .. } => projection_names(left),
        _ => Vec::new(),
    }
}

/// Try to extract a column name from a simple projection expression.
fn extract_expr_column_name(expr: &sqlparser::ast::Expr) -> Option<String> {
    use sqlparser::ast::Expr;

    match expr {
        Expr::Identifier(ident) => Some(normalize_identifier(&ident.value)),
        Expr::CompoundIdentifier(parts) => {
            // Take the last part (e.g., "t.column_name" -> "column_name").
            parts.last().map(|ident| normalize_identifier(&ident.value))
        }
        _ => None,
    }
}
