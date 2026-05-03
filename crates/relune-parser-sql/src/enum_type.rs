//! `CREATE TYPE ... AS ENUM` parsing.

use crate::context::{LineOffsets, ParseContext};
use crate::names::{normalized_stable_id, split_object_name_with_diagnostics};
use relune_core::{Enum, normalize_identifier};
use sqlparser::ast::ObjectName;

/// Parse a CREATE TYPE ... AS ENUM statement into an Enum.
pub(crate) fn parse_create_type_enum(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    labels: &[sqlparser::ast::Ident],
) -> Enum {
    let (schema_name, type_name) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "CREATE TYPE");

    // Generate a stable ID for the enum
    let id = normalized_stable_id(schema_name.as_deref(), &type_name);

    // Extract enum values
    let values: Vec<String> = labels.iter().map(|l| l.value.clone()).collect();

    // Normalize names
    let normalized_schema = schema_name.map(|s| normalize_identifier(&s));
    let normalized_name = normalize_identifier(&type_name);

    Enum {
        id,
        schema_name: normalized_schema,
        name: normalized_name,
        values,
    }
}
