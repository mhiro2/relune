//! `DROP` statement handling.
//!
//! Applies `DROP TABLE`/`VIEW`/`TYPE`/`INDEX` to the in-progress schema so
//! that migration SQL inputs (e.g. `CREATE TABLE users; DROP TABLE users;`)
//! produce an accurate final schema instead of silently retaining the
//! created object behind a `parse_unsupported` warning.

use crate::context::{LineOffsets, ParseContext, span_from_spanned};
use crate::names::{normalized_stable_id, split_object_name_with_diagnostics};
use relune_core::{Diagnostic, Enum, Table, View, diagnostic::codes};
use sqlparser::ast::{ObjectName, ObjectType};
use std::collections::HashMap;

/// Apply a `DROP` statement to the current schema, mutating the relevant
/// collections in place. Returns silently for object kinds that are not
/// modelled (e.g. `DROP SCHEMA`, `DROP DATABASE`); the outer caller still
/// emits an `Unsupported SQL construct` warning for those.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_drop_statement(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    object_type: ObjectType,
    if_exists: bool,
    names: &[ObjectName],
    table: Option<&ObjectName>,
    tables: &mut Vec<Table>,
    views: &mut Vec<View>,
    enums: &mut Vec<Enum>,
    table_map: &mut HashMap<String, usize>,
) -> bool {
    match object_type {
        ObjectType::Table => {
            for name in names {
                drop_table(ctx, input, offsets, name, if_exists, tables, table_map);
            }
            true
        }
        ObjectType::View | ObjectType::MaterializedView => {
            for name in names {
                drop_view(ctx, input, offsets, name, if_exists, views);
            }
            true
        }
        ObjectType::Type => {
            for name in names {
                drop_enum(ctx, input, offsets, name, if_exists, enums);
            }
            true
        }
        ObjectType::Index => {
            for name in names {
                drop_index(ctx, input, offsets, name, if_exists, table, tables);
            }
            true
        }
        _ => false,
    }
}

fn drop_table(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    if_exists: bool,
    tables: &mut Vec<Table>,
    table_map: &mut HashMap<String, usize>,
) {
    let (schema_name, target) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "DROP TABLE");
    let stable_id = normalized_stable_id(schema_name.as_deref(), &target);

    let Some(idx) = table_map.remove(&stable_id) else {
        if !if_exists {
            ctx.diagnostics.push(
                Diagnostic::warning(
                    codes::schema_unknown_table(),
                    format!("DROP TABLE references unknown table: {stable_id}"),
                )
                .with_span_opt(span_from_spanned(input, offsets, name)),
            );
        }
        return;
    };

    let dropped = tables.remove(idx);
    ctx.seen_tables.remove(&stable_id);

    // Indices into `tables` shift after the removal — rebuild lookup entries
    // for the trailing tables so subsequent statements (ALTER, COMMENT, etc.)
    // continue to resolve correctly.
    for entry in table_map.values_mut() {
        if *entry > idx {
            *entry -= 1;
        }
    }

    // Prune FKs in remaining tables that referenced the dropped one — leaving
    // them would yield a Schema with FK targets that no longer exist, which
    // Schema::validate flags as broken. The DDL effectively cascades for the
    // purposes of this in-memory model.
    let dropped_name_lower = dropped.name.to_lowercase();
    let dropped_schema_lower = dropped.schema_name.as_deref().map(str::to_lowercase);
    let dropped_stable_id_lower = dropped.stable_id.to_lowercase();
    for table in tables.iter_mut() {
        let source_schema_lower = table.schema_name.as_deref().map(str::to_lowercase);
        table.foreign_keys.retain(|fk| {
            !fk_targets_dropped_table(
                fk,
                &dropped_name_lower,
                dropped_schema_lower.as_deref(),
                &dropped_stable_id_lower,
                source_schema_lower.as_deref(),
            )
        });
    }
}

fn fk_targets_dropped_table(
    fk: &relune_core::ForeignKey,
    dropped_name_lower: &str,
    dropped_schema_lower: Option<&str>,
    dropped_stable_id_lower: &str,
    source_schema_lower: Option<&str>,
) -> bool {
    let to_table_lower = fk.to_table.to_lowercase();
    if to_table_lower != dropped_name_lower && to_table_lower != dropped_stable_id_lower {
        return false;
    }
    let fk_to_schema_lower = fk.to_schema.as_deref().map(str::to_lowercase);
    match (fk_to_schema_lower.as_deref(), dropped_schema_lower) {
        (Some(fk_schema), Some(dropped_schema)) => fk_schema == dropped_schema,
        // An explicit `fk.to_schema = Some(s)` only resolves to tables whose
        // schema_name matches; a schemaless dropped table cannot satisfy it.
        (Some(_), None) => false,
        // Unqualified FK targets resolve against the source table's schema
        // first, so prune only when the source shares the dropped schema.
        (None, Some(dropped_schema)) => source_schema_lower == Some(dropped_schema),
        (None, None) => true,
    }
}

fn drop_view(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    if_exists: bool,
    views: &mut Vec<View>,
) {
    let (schema_name, target) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "DROP VIEW");
    let stable_id = normalized_stable_id(schema_name.as_deref(), &target);

    let position = views.iter().position(|view| view.id == stable_id);
    if let Some(p) = position {
        views.remove(p);
    } else if !if_exists {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_table(),
                format!("DROP VIEW references unknown view: {stable_id}"),
            )
            .with_span_opt(span_from_spanned(input, offsets, name)),
        );
    }
}

fn drop_enum(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    if_exists: bool,
    enums: &mut Vec<Enum>,
) {
    let (schema_name, target) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "DROP TYPE");
    let stable_id = normalized_stable_id(schema_name.as_deref(), &target);

    let position = enums.iter().position(|enum_def| enum_def.id == stable_id);
    if let Some(p) = position {
        enums.remove(p);
    } else if !if_exists {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_table(),
                format!("DROP TYPE references unknown type: {stable_id}"),
            )
            .with_span_opt(span_from_spanned(input, offsets, name)),
        );
    }
}

fn drop_index(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    name: &ObjectName,
    if_exists: bool,
    table: Option<&ObjectName>,
    tables: &mut [Table],
) {
    let (index_schema, index_name) =
        split_object_name_with_diagnostics(ctx, input, offsets, name, "DROP INDEX");
    let normalized_index_name = relune_core::normalize_identifier(&index_name);
    let index_schema_lower = index_schema.as_deref().map(str::to_lowercase);

    let mut removed = false;
    if let Some(table_ref) = table {
        // MySQL syntax: DROP INDEX name ON table_name
        let (schema_name, target) =
            split_object_name_with_diagnostics(ctx, input, offsets, table_ref, "DROP INDEX ON");
        let stable_id = normalized_stable_id(schema_name.as_deref(), &target);
        if let Some(table) = tables.iter_mut().find(|t| t.stable_id == stable_id) {
            let before = table.indexes.len();
            table.indexes.retain(|ix| {
                ix.name
                    .as_deref()
                    .is_none_or(|n| n != normalized_index_name)
            });
            removed = table.indexes.len() != before;
        }
    } else {
        // PostgreSQL/SQLite syntax: DROP INDEX [schema.]name. The index
        // belongs to whatever schema its parent table lives in, so a
        // schema-qualified drop must only touch matching tables.
        for table in tables.iter_mut() {
            if let Some(target_schema) = index_schema_lower.as_deref() {
                let table_schema_lower = table.schema_name.as_deref().map(str::to_lowercase);
                if table_schema_lower.as_deref() != Some(target_schema) {
                    continue;
                }
            }
            let before = table.indexes.len();
            table.indexes.retain(|ix| {
                ix.name
                    .as_deref()
                    .is_none_or(|n| n != normalized_index_name)
            });
            if table.indexes.len() != before {
                removed = true;
            }
        }
    }

    if !removed && !if_exists {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_table(),
                format!("DROP INDEX references unknown index: {normalized_index_name}"),
            )
            .with_span_opt(span_from_spanned(input, offsets, name)),
        );
    }
}
