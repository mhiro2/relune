//! Object-name normalization, span-aware diagnostics, and foreign-key building.

use crate::context::{LineOffsets, ParseContext, span_from_spanned};
use relune_core::{
    Diagnostic, ForeignKey, ReferentialAction, diagnostic::codes, normalize_identifier,
};
use sqlparser::ast::{ObjectName, ObjectNamePart};

pub(crate) fn normalized_stable_id(schema_name: Option<&str>, name: &str) -> String {
    let norm_name = normalize_identifier(name);
    match schema_name {
        Some(schema_name) => {
            let norm_schema = normalize_identifier(schema_name);
            // Quote components that contain '.' so that ("a.b", "c") produces
            // "a.b".c instead of the ambiguous a.b.c.
            let s = if norm_schema.contains('.') {
                format!("\"{norm_schema}\"")
            } else {
                norm_schema
            };
            let n = if norm_name.contains('.') {
                format!("\"{norm_name}\"")
            } else {
                norm_name
            };
            format!("{s}.{n}")
        }
        None => norm_name,
    }
}

pub(crate) fn split_object_name_parts(name: &ObjectName) -> Vec<String> {
    name.0.iter().map(object_name_part_to_string).collect()
}

pub(crate) fn warn_truncated_object_name(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    max_parts: usize,
    context: &str,
) {
    if name.0.len() <= max_parts {
        return;
    }

    let parts = split_object_name_parts(name);
    let ignored = parts[..parts.len() - max_parts].join(".");
    let retained = parts[parts.len() - max_parts..].join(".");
    ctx.diagnostics.push(
        Diagnostic::warning(
            codes::parse_unsupported(),
            format!(
                "{context}: object name `{name}` has more than {max_parts} parts; ignoring leading qualifier(s) `{ignored}` and using `{retained}`"
            ),
        )
        .with_span_opt(span_from_spanned(input, offsets, name)),
    );
}

pub(crate) fn split_object_name_with_diagnostics(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    context: &str,
) -> (Option<String>, String) {
    warn_truncated_object_name(ctx, input, offsets, name, 2, context);
    split_object_name(name)
}

pub(crate) fn normalized_stable_id_for_object_name_with_diagnostics(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    context: &str,
) -> String {
    let (schema_name, object_name) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, context);
    normalized_stable_id(schema_name.as_deref(), &object_name)
}

pub(crate) fn foreign_key_target(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    target: &ObjectName,
    context: &str,
) -> (Option<String>, String) {
    split_object_name_with_diagnostics(ctx, input, offsets, target, context)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_foreign_key(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    constraint_name: Option<&sqlparser::ast::Ident>,
    from_columns: Vec<String>,
    foreign_table: &ObjectName,
    referred_columns: &[sqlparser::ast::Ident],
    on_delete: Option<sqlparser::ast::ReferentialAction>,
    on_update: Option<sqlparser::ast::ReferentialAction>,
    context: &str,
) -> ForeignKey {
    let (to_schema, to_table) = foreign_key_target(ctx, input, offsets, foreign_table, context);
    let to_columns = referred_columns
        .iter()
        .map(|column| normalize_identifier(&column.value))
        .collect();

    ForeignKey {
        name: constraint_name.map(|ident| normalize_identifier(&ident.value)),
        from_columns,
        to_schema: to_schema.map(|schema| normalize_identifier(&schema)),
        to_table: normalize_identifier(&to_table),
        to_columns,
        on_delete: convert_referential_action(on_delete),
        on_update: convert_referential_action(on_update),
    }
}

/// Split an `ObjectName` into (`schema_name`, `table_name`).
pub(crate) fn split_object_name(name: &ObjectName) -> (Option<String>, String) {
    let len = name.0.len();
    if len == 0 {
        return (None, String::new());
    }
    let table = object_name_part_to_string(&name.0[len - 1]);
    let schema = (len >= 2).then(|| object_name_part_to_string(&name.0[len - 2]));
    (schema, table)
}

/// Convert an `ObjectNamePart` to a string.
pub(crate) fn object_name_part_to_string(part: &ObjectNamePart) -> String {
    match part {
        ObjectNamePart::Identifier(ident) => ident.value.clone(),
        ObjectNamePart::Function(func) => func.to_string(),
    }
}

/// Convert sqlparser's `ReferentialAction` to our model type.
pub(crate) const fn convert_referential_action(
    action: Option<sqlparser::ast::ReferentialAction>,
) -> ReferentialAction {
    match action {
        Some(sqlparser::ast::ReferentialAction::Cascade) => ReferentialAction::Cascade,
        Some(sqlparser::ast::ReferentialAction::SetNull) => ReferentialAction::SetNull,
        Some(sqlparser::ast::ReferentialAction::SetDefault) => ReferentialAction::SetDefault,
        Some(sqlparser::ast::ReferentialAction::Restrict) => ReferentialAction::Restrict,
        Some(sqlparser::ast::ReferentialAction::NoAction) | None => ReferentialAction::NoAction,
    }
}
