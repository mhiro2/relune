//! `CREATE INDEX` parsing and attachment to existing tables.

use crate::context::{LineOffsets, ParseContext, WithSpanOpt, span_from_spanned};
use crate::create_table::extract_column_name;
use crate::names::{
    normalized_stable_id_for_object_name_with_diagnostics, object_name_part_to_string,
};
use relune_core::{Diagnostic, Index, Table, diagnostic::codes, normalize_identifier};
use sqlparser::ast::CreateIndex;
use std::collections::HashMap;

/// Parse a CREATE INDEX statement and attach it to the appropriate table.
pub(crate) fn parse_create_index(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    create_index: &CreateIndex,
    tables: &mut [Table],
    table_map: &HashMap<String, usize>,
) {
    // Get the table name
    let stable_id = normalized_stable_id_for_object_name_with_diagnostics(
        ctx,
        input,
        offsets,
        &create_index.table_name,
        "CREATE INDEX",
    );

    // Find the table
    let Some(&table_idx) = table_map.get(&stable_id) else {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_table(),
                format!("CREATE INDEX references unknown table: {stable_id}"),
            )
            .with_span_opt(span_from_spanned(input, offsets, create_index)),
        );
        return;
    };

    // Extract index columns
    let index_columns: Vec<String> = create_index
        .columns
        .iter()
        .map(extract_column_name)
        .collect();

    let index = Index {
        name: create_index.name.as_ref().map(|ident| {
            // ObjectName is a wrapper around Vec<ObjectNamePart>.
            // Use the last part as the actual index name (earlier parts are schema qualifiers).
            ident
                .0
                .last()
                .map(|part| normalize_identifier(&object_name_part_to_string(part)))
                .unwrap_or_default()
        }),
        columns: index_columns,
        is_unique: create_index.unique,
    };

    tables[table_idx].indexes.push(index);
}
