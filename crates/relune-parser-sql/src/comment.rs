//! `COMMENT ON` parsing for tables and columns.

use crate::context::{LineOffsets, ParseContext, span_from_spanned};
use crate::names::{
    normalized_stable_id, normalized_stable_id_for_object_name_with_diagnostics,
    split_object_name_parts, warn_truncated_object_name,
};
use relune_core::{Diagnostic, Table, diagnostic::codes, normalize_identifier};
use sqlparser::ast::ObjectName;
use std::collections::HashMap;

/// Parse a COMMENT ON statement and apply it to the appropriate table or column.
///
/// `None` for `comment` represents `COMMENT ... IS NULL`, which deletes any
/// existing comment per SQL semantics — earlier versions silently skipped this
/// case and left a stale comment behind.
#[allow(clippy::ref_option)]
#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
pub(crate) fn parse_comment(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    object_type: sqlparser::ast::CommentObject,
    object_name: &ObjectName,
    comment: Option<&String>,
    tables: &mut [Table],
    table_map: &HashMap<String, usize>,
) {
    let new_comment = comment.cloned();

    match object_type {
        sqlparser::ast::CommentObject::Table => {
            let stable_id = normalized_stable_id_for_object_name_with_diagnostics(
                ctx,
                input,
                offsets,
                object_name,
                "COMMENT ON TABLE",
            );

            if let Some(&table_idx) = table_map.get(&stable_id) {
                tables[table_idx].comment = new_comment;
            } else {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_table(),
                        format!("COMMENT ON TABLE references unknown table: {stable_id}"),
                    )
                    .with_span_opt(span_from_spanned(
                        input,
                        offsets,
                        object_name,
                    )),
                );
            }
        }
        sqlparser::ast::CommentObject::Column => {
            // For columns, object_name is typically "table.column" or "schema.table.column"
            let parts = split_object_name_parts(object_name);

            // Extract column name (last part) and table name (remaining parts)
            if parts.len() < 2 {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::parse_unsupported(),
                        "Invalid COMMENT ON COLUMN syntax: expected table.column".to_string(),
                    )
                    .with_span_opt(span_from_spanned(
                        input,
                        offsets,
                        object_name,
                    )),
                );
                return;
            }

            warn_truncated_object_name(ctx, input, offsets, object_name, 3, "COMMENT ON COLUMN");

            let column_name = normalize_identifier(&parts[parts.len() - 1]);
            let table_parts = &parts[..parts.len() - 1];

            let stable_id = match table_parts {
                [table] => normalize_identifier(table),
                [schema, table] | [.., schema, table] => normalized_stable_id(Some(schema), table),
                [] => {
                    ctx.diagnostics.push(
                        Diagnostic::warning(
                            codes::parse_unsupported(),
                            "Unsupported COMMENT ON COLUMN syntax: missing table qualifier"
                                .to_string(),
                        )
                        .with_span_opt(span_from_spanned(
                            input,
                            offsets,
                            object_name,
                        )),
                    );
                    return;
                }
            };

            if let Some(&table_idx) = table_map.get(&stable_id) {
                if let Some(column) = tables[table_idx]
                    .columns
                    .iter_mut()
                    .find(|c| c.name == column_name)
                {
                    column.comment = new_comment;
                } else {
                    ctx.diagnostics.push(Diagnostic::warning(
                        codes::schema_unknown_column(),
                        format!(
                            "COMMENT ON COLUMN references unknown column: {stable_id}.{column_name}"
                        ),
                    )
                    .with_span_opt(span_from_spanned(input, offsets,object_name)));
                }
            } else {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_table(),
                        format!("COMMENT ON COLUMN references unknown table: {stable_id}"),
                    )
                    .with_span_opt(span_from_spanned(
                        input,
                        offsets,
                        object_name,
                    )),
                );
            }
        }
        _ => {
            // Other comment types (view, function, etc.) are not supported
            ctx.warn_unsupported(
                &format!("COMMENT ON {object_type:?}"),
                span_from_spanned(input, offsets, object_name),
            );
        }
    }
}
