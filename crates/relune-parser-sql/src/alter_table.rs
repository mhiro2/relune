//! `ALTER TABLE` operation handling.

use crate::context::{LineOffsets, ParseContext, WithSpanOpt, span_from_ident, span_from_spanned};
use crate::create_table::{
    canonicalize_data_type, column_attributes_from_options, parsed_column_from_column_def,
    push_unique_index,
};
use crate::names::{
    build_foreign_key, normalized_stable_id, normalized_stable_id_for_object_name_with_diagnostics,
    split_object_name_with_diagnostics,
};
use relune_core::{
    Column, ColumnId, Diagnostic, ForeignKey, Table, diagnostic::codes, normalize_identifier,
};
use sqlparser::ast::{
    AlterColumnOperation, AlterTableOperation, ColumnOption, DataType, ObjectName, TableConstraint,
};
use std::collections::{HashMap, HashSet};

/// Build `stable_id` the same way as `parse_create_table` so `ALTER TABLE` resolves targets.
fn stable_id_for_alter_target(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table_name: &ObjectName,
) -> String {
    normalized_stable_id_for_object_name_with_diagnostics(
        ctx,
        input,
        offsets,
        table_name,
        "ALTER TABLE",
    )
}

fn table_name_matches_reference(table: &Table, target_table: &str) -> bool {
    table.name.eq_ignore_ascii_case(target_table)
        || table.stable_id.eq_ignore_ascii_case(target_table)
}

fn table_schema_matches(table: &Table, target_schema: Option<&str>) -> bool {
    match target_schema {
        Some(target_schema) => table
            .schema_name
            .as_deref()
            .is_some_and(|schema_name| schema_name.eq_ignore_ascii_case(target_schema)),
        None => table.schema_name.is_none(),
    }
}

fn single_matching_table_index(
    tables: &[Table],
    target_table: &str,
    target_schema: Option<&str>,
) -> Option<usize> {
    let mut found = None;
    for (table_idx, table) in tables.iter().enumerate() {
        if !table_schema_matches(table, target_schema)
            || !table_name_matches_reference(table, target_table)
        {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(table_idx);
    }
    found
}

fn single_matching_table_index_any_schema(tables: &[Table], target_table: &str) -> Option<usize> {
    let mut found = None;
    for (table_idx, table) in tables.iter().enumerate() {
        if !table_name_matches_reference(table, target_table) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(table_idx);
    }
    found
}

fn foreign_key_resolves_to_table(
    tables: &[Table],
    source_idx: usize,
    fk: &ForeignKey,
    target_idx: usize,
) -> bool {
    if let Some(target_schema) = fk.to_schema.as_deref() {
        return single_matching_table_index(tables, &fk.to_table, Some(target_schema))
            == Some(target_idx);
    }

    if let Some(source_schema) = tables[source_idx].schema_name.as_deref()
        && let Some(match_idx) =
            single_matching_table_index(tables, &fk.to_table, Some(source_schema))
    {
        return match_idx == target_idx;
    }

    single_matching_table_index(tables, &fk.to_table, None)
        .or_else(|| single_matching_table_index_any_schema(tables, &fk.to_table))
        == Some(target_idx)
}

fn foreign_keys_referencing_table(tables: &[Table], target_idx: usize) -> Vec<(usize, usize)> {
    let mut references = Vec::new();
    for (table_idx, table) in tables.iter().enumerate() {
        for (fk_idx, fk) in table.foreign_keys.iter().enumerate() {
            if foreign_key_resolves_to_table(tables, table_idx, fk, target_idx) {
                references.push((table_idx, fk_idx));
            }
        }
    }
    references
}

#[allow(clippy::too_many_lines)]
pub(crate) fn apply_alter_table_operations(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    tables: &mut [Table],
    table_map: &mut HashMap<String, usize>,
    table_name: &ObjectName,
    operations: &[AlterTableOperation],
) {
    let stable_id = stable_id_for_alter_target(ctx, input, offsets, table_name);
    let Some(&idx) = table_map.get(&stable_id) else {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_table(),
                format!("ALTER TABLE references unknown table: {stable_id}"),
            )
            .with_span_opt(span_from_spanned(input, offsets, table_name)),
        );
        return;
    };

    for op in operations {
        apply_single_alter_operation(ctx, input, offsets, tables, table_map, idx, op);
    }
}

#[allow(clippy::too_many_lines)]
fn apply_single_alter_operation(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    tables: &mut [Table],
    table_map: &mut HashMap<String, usize>,
    idx: usize,
    op: &AlterTableOperation,
) {
    match op {
        AlterTableOperation::AddColumn {
            column_def,
            if_not_exists,
            ..
        } => {
            add_column_from_alter(
                ctx,
                input,
                offsets,
                &mut tables[idx],
                column_def,
                *if_not_exists,
            );
        }
        AlterTableOperation::DropColumn {
            column_names,
            if_exists,
            ..
        } => {
            let stable = tables[idx].stable_id.clone();
            for ident in column_names {
                let col_name = normalize_identifier(&ident.value);
                let pos = tables[idx].columns.iter().position(|c| c.name == col_name);
                if let Some(p) = pos {
                    let incoming_fks_to_remove: HashSet<(usize, usize)> =
                        foreign_keys_referencing_table(tables, idx)
                            .into_iter()
                            .filter(|(table_idx, fk_idx)| {
                                tables[*table_idx].foreign_keys[*fk_idx]
                                    .to_columns
                                    .contains(&col_name)
                            })
                            .collect();

                    let table = &mut tables[idx];
                    table.columns.remove(p);
                    table.indexes.retain(|ix| !ix.columns.contains(&col_name));

                    for (table_idx, table) in tables.iter_mut().enumerate() {
                        let mut fk_idx = 0usize;
                        table.foreign_keys.retain(|fk| {
                            let remove = (table_idx == idx && fk.from_columns.contains(&col_name))
                                || incoming_fks_to_remove.contains(&(table_idx, fk_idx));
                            fk_idx += 1;
                            !remove
                        });
                    }

                    // A named primary key is meaningless once none of its
                    // columns remain, so clear the dangling constraint name.
                    if !tables[idx].columns.iter().any(|c| c.is_primary_key) {
                        tables[idx].primary_key_name = None;
                    }
                } else if !if_exists {
                    ctx.diagnostics.push(
                        Diagnostic::warning(
                            codes::schema_unknown_column(),
                            format!(
                                "ALTER TABLE DROP COLUMN: unknown column `{col_name}` on `{stable}`"
                            ),
                        )
                        .with_span_opt(span_from_ident(input, offsets, ident)),
                    );
                }
            }
        }
        AlterTableOperation::AddConstraint { constraint, .. } => {
            apply_add_table_constraint(ctx, input, offsets, &mut tables[idx], constraint);
        }
        AlterTableOperation::DropConstraint {
            if_exists, name, ..
        } => {
            let cname = name.value.clone();
            let cname_norm = normalize_identifier(&cname);
            let stable = tables[idx].stable_id.clone();
            let table = &mut tables[idx];
            let before_fk = table.foreign_keys.len();
            let before_ix = table.indexes.len();
            let pk_match = table
                .primary_key_name
                .as_ref()
                .is_some_and(|n| n == &cname_norm);
            table.foreign_keys.retain(|fk| {
                fk.name.as_ref().map(|n| normalize_identifier(n)) != Some(cname_norm.clone())
            });
            table.indexes.retain(|ix| {
                ix.name.as_ref().map(|n| normalize_identifier(n)) != Some(cname_norm.clone())
            });
            if pk_match {
                for column in &mut table.columns {
                    column.is_primary_key = false;
                }
                table.primary_key_name = None;
            }
            if table.foreign_keys.len() == before_fk
                && table.indexes.len() == before_ix
                && !pk_match
                && !if_exists
            {
                ctx.diagnostics.push(Diagnostic::warning(
                    codes::parse_unsupported(),
                    format!(
                        "ALTER TABLE DROP CONSTRAINT: no constraint named `{cname}` on `{stable}`"
                    ),
                )
                .with_span_opt(span_from_ident(input, offsets,name)));
            }
        }
        AlterTableOperation::RenameColumn {
            old_column_name,
            new_column_name,
        } => {
            let old = normalize_identifier(&old_column_name.value);
            let new = normalize_identifier(&new_column_name.value);
            if !rename_column_in_tables(tables, idx, &old, &new) {
                let stable = tables[idx].stable_id.clone();
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_column(),
                        format!("ALTER TABLE RENAME COLUMN: unknown `{old}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(
                        input,
                        offsets,
                        old_column_name,
                    )),
                );
            }
        }
        AlterTableOperation::AlterColumn { column_name, op } => {
            apply_alter_column(ctx, input, offsets, &mut tables[idx], column_name, op);
        }
        AlterTableOperation::ModifyColumn {
            col_name,
            data_type,
            options,
            ..
        } => {
            let name = normalize_identifier(&col_name.value);
            if tables[idx].columns.iter().any(|c| c.name == name) {
                redefine_column_in_table(
                    ctx,
                    input,
                    offsets,
                    &mut tables[idx],
                    &name,
                    data_type,
                    options,
                );
            } else {
                let stable = tables[idx].stable_id.clone();
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_column(),
                        format!("ALTER TABLE MODIFY COLUMN: unknown column `{name}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(input, offsets, col_name)),
                );
            }
        }
        AlterTableOperation::ChangeColumn {
            old_name,
            new_name,
            data_type,
            options,
            ..
        } => {
            let old = normalize_identifier(&old_name.value);
            let new = normalize_identifier(&new_name.value);
            if rename_column_in_tables(tables, idx, &old, &new) {
                redefine_column_in_table(
                    ctx,
                    input,
                    offsets,
                    &mut tables[idx],
                    &new,
                    data_type,
                    options,
                );
            } else {
                let stable = tables[idx].stable_id.clone();
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::schema_unknown_column(),
                        format!("ALTER TABLE CHANGE COLUMN: unknown column `{old}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(input, offsets, old_name)),
                );
            }
        }
        AlterTableOperation::RenameConstraint { old_name, new_name } => {
            apply_rename_constraint(ctx, input, offsets, &mut tables[idx], old_name, new_name);
        }
        AlterTableOperation::RenameTable {
            table_name: new_table,
        } => {
            let referencing_fks = foreign_keys_referencing_table(tables, idx);
            let old_stable = tables[idx].stable_id.clone();
            let old_schema = tables[idx].schema_name.clone();
            let renamed_target = match new_table {
                sqlparser::ast::RenameTableNameKind::As(name)
                | sqlparser::ast::RenameTableNameKind::To(name) => name,
            };
            let (new_schema_raw, new_name_raw) = split_object_name_with_diagnostics(
                ctx,
                input,
                offsets,
                renamed_target,
                "ALTER TABLE RENAME TO",
            );
            let new_schema = new_schema_raw
                .map(|schema_name| normalize_identifier(&schema_name))
                .or_else(|| old_schema.clone());
            let new_name = normalize_identifier(&new_name_raw);
            let renamed_stable_id = normalized_stable_id(new_schema.as_deref(), &new_name);
            let table = &mut tables[idx];
            table.schema_name.clone_from(&new_schema);
            table.name.clone_from(&new_name);
            table.stable_id.clone_from(&renamed_stable_id);
            table_map.remove(&old_stable);
            table_map.insert(renamed_stable_id.clone(), idx);
            ctx.seen_tables.remove(&old_stable);
            ctx.seen_tables.insert(renamed_stable_id.clone());

            for (table_idx, fk_idx) in referencing_fks {
                if let Some(fk) = tables
                    .get_mut(table_idx)
                    .and_then(|table| table.foreign_keys.get_mut(fk_idx))
                {
                    fk.to_table.clone_from(&new_name);
                    if fk.to_schema.is_some() || old_schema != new_schema {
                        fk.to_schema.clone_from(&new_schema);
                    }
                }
            }
        }
        AlterTableOperation::DropPrimaryKey { .. } => {
            let table = &mut tables[idx];
            for col in &mut table.columns {
                col.is_primary_key = false;
            }
            table.primary_key_name = None;
        }
        AlterTableOperation::DropForeignKey { name, .. } => {
            let sym = normalize_identifier(&name.value);
            let stable = tables[idx].stable_id.clone();
            let table = &mut tables[idx];
            let before = table.foreign_keys.len();
            table.foreign_keys.retain(|fk| {
                fk.name.as_ref().map(|n| normalize_identifier(n)) != Some(sym.clone())
            });
            if table.foreign_keys.len() == before {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!("ALTER TABLE DROP FOREIGN KEY: no FK named `{sym}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(input, offsets, name)),
                );
            }
        }
        AlterTableOperation::DropIndex { name } => {
            let n = normalize_identifier(&name.value);
            let stable = tables[idx].stable_id.clone();
            let table = &mut tables[idx];
            let before = table.indexes.len();
            table.indexes.retain(|ix| {
                ix.name.as_ref().map(|nm| normalize_identifier(nm)) != Some(n.clone())
            });
            if table.indexes.len() == before {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        codes::parse_unsupported(),
                        format!("ALTER TABLE DROP INDEX: no index named `{n}` on `{stable}`"),
                    )
                    .with_span_opt(span_from_ident(input, offsets, name)),
                );
            }
        }
        other => {
            ctx.warn_unsupported(
                &format!("ALTER TABLE operation (unsupported): {other:?}"),
                span_from_spanned(input, offsets, op),
            );
        }
    }
}

/// Rename `old` to `new` on `tables[idx]`, propagating the change to the
/// column's local FK `from_columns`, local index columns, and the `to_columns`
/// of every FK that references this table. Returns `false` if no such column
/// exists (the caller emits the unknown-column diagnostic).
fn rename_column_in_tables(tables: &mut [Table], idx: usize, old: &str, new: &str) -> bool {
    if !tables[idx].columns.iter().any(|c| c.name == old) {
        return false;
    }
    let referencing_fks = foreign_keys_referencing_table(tables, idx);
    let table = &mut tables[idx];
    if let Some(col) = table.columns.iter_mut().find(|c| c.name == old) {
        new.clone_into(&mut col.name);
    }
    for fk in &mut table.foreign_keys {
        for c in &mut fk.from_columns {
            if *c == old {
                new.clone_into(c);
            }
        }
    }
    for ix in &mut table.indexes {
        for c in &mut ix.columns {
            if *c == old {
                new.clone_into(c);
            }
        }
    }
    for (table_idx, fk_idx) in referencing_fks {
        if let Some(fk) = tables
            .get_mut(table_idx)
            .and_then(|table| table.foreign_keys.get_mut(fk_idx))
        {
            for c in &mut fk.to_columns {
                if *c == old {
                    new.clone_into(c);
                }
            }
        }
    }
    true
}

/// Apply a `PostgreSQL` `ALTER COLUMN <col> <op>` to the matching column.
fn apply_alter_column(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table: &mut Table,
    column_name: &sqlparser::ast::Ident,
    op: &AlterColumnOperation,
) {
    let name = normalize_identifier(&column_name.value);
    let stable = table.stable_id.clone();
    let Some(column) = table.columns.iter_mut().find(|c| c.name == name) else {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::schema_unknown_column(),
                format!("ALTER TABLE ALTER COLUMN: unknown column `{name}` on `{stable}`"),
            )
            .with_span_opt(span_from_ident(input, offsets, column_name)),
        );
        return;
    };

    match op {
        AlterColumnOperation::SetNotNull => column.nullable = false,
        AlterColumnOperation::DropNotNull => {
            // Primary-key columns remain implicitly NOT NULL.
            if !column.is_primary_key {
                column.nullable = true;
            }
        }
        AlterColumnOperation::SetDataType { data_type, .. } => {
            set_column_data_type(column, data_type);
        }
        // The model tracks neither DEFAULT values nor identity/generated
        // metadata, so these operations have no observable schema effect.
        AlterColumnOperation::SetDefault { .. }
        | AlterColumnOperation::DropDefault
        | AlterColumnOperation::AddGenerated { .. } => {}
    }
}

/// Fully redefine a column from a `MySQL` `MODIFY`/`CHANGE` clause: replace the
/// data type, re-derive nullability from the options, and apply any inline
/// schema-affecting options (`UNIQUE`, `FOREIGN KEY`) the same way `ADD COLUMN`
/// does. `MySQL` treats these as complete column redefinitions, so an omitted
/// `NOT NULL` makes the column nullable again. Primary-key membership is
/// table-level, so it is only added (never cleared) here, and PK columns stay
/// NOT NULL regardless.
///
/// The caller must have verified that `col_name` exists on `table`.
fn redefine_column_in_table(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table: &mut Table,
    col_name: &str,
    data_type: &DataType,
    options: &[ColumnOption],
) {
    let attrs = column_attributes_from_options(options);
    if let Some(column) = table.columns.iter_mut().find(|c| c.name == col_name) {
        column.nullable = attrs.nullable;
        if attrs.is_primary_key {
            column.is_primary_key = true;
        }
        if column.is_primary_key {
            column.nullable = false;
        }
        column.comment = attrs.comment;
        set_column_data_type(column, data_type);
    }

    // Inline UNIQUE / FOREIGN KEY constraints carry no constraint name in this
    // position, so they are recorded anonymously, mirroring `ADD COLUMN`.
    for option in options {
        match option {
            ColumnOption::Unique(_) => {
                push_unique_index(&mut table.indexes, None, vec![col_name.to_owned()]);
            }
            ColumnOption::ForeignKey(constraint) => {
                table.foreign_keys.push(build_foreign_key(
                    ctx,
                    input,
                    offsets,
                    None,
                    vec![col_name.to_owned()],
                    &constraint.foreign_table,
                    &constraint.referred_columns,
                    constraint.on_delete,
                    constraint.on_update,
                    "ALTER TABLE MODIFY/CHANGE COLUMN inline FOREIGN KEY",
                ));
            }
            _ => {}
        }
    }
}

/// Replace a column's data type, clearing any cached inline enum/set values so
/// they are re-derived by the `MySQL` enum pass over the final type string.
fn set_column_data_type(column: &mut Column, data_type: &DataType) {
    column.data_type = canonicalize_data_type(data_type);
    column.enum_values = None;
}

/// Apply `RENAME CONSTRAINT <old> TO <new>`, matching named primary keys,
/// foreign keys, and indexes (constraint names are compared case-insensitively).
fn apply_rename_constraint(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table: &mut Table,
    old_name: &sqlparser::ast::Ident,
    new_name: &sqlparser::ast::Ident,
) {
    let old = normalize_identifier(&old_name.value);
    let new = normalize_identifier(&new_name.value);
    let stable = table.stable_id.clone();

    let pk_renamed = table.primary_key_name.as_deref() == Some(old.as_str());
    if pk_renamed {
        table.primary_key_name = Some(new.clone());
    }
    let mut renamed = pk_renamed;
    for fk in &mut table.foreign_keys {
        if fk.name.as_deref().map(normalize_identifier).as_deref() == Some(old.as_str()) {
            fk.name = Some(new.clone());
            renamed = true;
        }
    }
    for ix in &mut table.indexes {
        if ix.name.as_deref().map(normalize_identifier).as_deref() == Some(old.as_str()) {
            ix.name = Some(new.clone());
            renamed = true;
        }
    }

    if !renamed {
        ctx.diagnostics.push(
            Diagnostic::warning(
                codes::parse_unsupported(),
                format!("ALTER TABLE RENAME CONSTRAINT: no constraint named `{old}` on `{stable}`"),
            )
            .with_span_opt(span_from_ident(input, offsets, old_name)),
        );
    }
}

fn add_column_from_alter(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table: &mut Table,
    column_def: &sqlparser::ast::ColumnDef,
    if_not_exists: bool,
) {
    let col_name = normalize_identifier(&column_def.name.value);
    if table.columns.iter().any(|c| c.name == col_name) {
        if !if_not_exists {
            ctx.diagnostics.push(
                Diagnostic::warning(
                    codes::parse_unsupported(),
                    format!(
                        "ALTER TABLE ADD COLUMN: duplicate column `{col_name}` on `{}`",
                        table.stable_id
                    ),
                )
                .with_span_opt(span_from_spanned(input, offsets, column_def)),
            );
        }
        return;
    }

    let next_id = table.columns.iter().map(|c| c.id.0).max().unwrap_or(0) + 1;
    let col = parsed_column_from_column_def(column_def).into_column(ColumnId(next_id));
    table.columns.push(col);

    for option in &column_def.options {
        match &option.option {
            ColumnOption::ForeignKey(constraint) => {
                let from_column = col_name.clone();
                table.foreign_keys.push(build_foreign_key(
                    ctx,
                    input,
                    offsets,
                    option.name.as_ref(),
                    vec![from_column],
                    &constraint.foreign_table,
                    &constraint.referred_columns,
                    constraint.on_delete,
                    constraint.on_update,
                    "ALTER TABLE ADD COLUMN inline FOREIGN KEY",
                ));
            }
            ColumnOption::PrimaryKey(_) => {
                if let Some(constraint_name) = &option.name {
                    table.primary_key_name = Some(normalize_identifier(&constraint_name.value));
                }
            }
            ColumnOption::Unique(_) => {
                let constraint_name = option.name.as_ref().map(|n| normalize_identifier(&n.value));
                push_unique_index(&mut table.indexes, constraint_name, vec![col_name.clone()]);
            }
            _ => {}
        }
    }
}

fn apply_add_table_constraint(
    ctx: &mut ParseContext,
    input: &str,
    offsets: &LineOffsets,
    table: &mut Table,
    constraint: &TableConstraint,
) {
    match constraint {
        TableConstraint::PrimaryKey(primary_key) => {
            for pk_col in &primary_key.columns {
                let col_name = crate::create_table::extract_column_name(pk_col);
                if let Some(column) = table.columns.iter_mut().find(|c| c.name == col_name) {
                    column.is_primary_key = true;
                    column.nullable = false;
                }
            }
            if let Some(constraint_name) = &primary_key.name {
                table.primary_key_name = Some(normalize_identifier(&constraint_name.value));
            }
        }
        TableConstraint::Unique(unique) => {
            let col_names: Vec<String> = unique
                .columns
                .iter()
                .map(crate::create_table::extract_column_name)
                .collect();
            let index_name = unique
                .name
                .as_ref()
                .map(|ident| normalize_identifier(&ident.value));
            push_unique_index(&mut table.indexes, index_name, col_names);
        }
        TableConstraint::ForeignKey(foreign_key) => {
            let from_cols: Vec<String> = foreign_key
                .columns
                .iter()
                .map(|c| normalize_identifier(&c.value))
                .collect();
            table.foreign_keys.push(build_foreign_key(
                ctx,
                input,
                offsets,
                foreign_key.name.as_ref(),
                from_cols,
                &foreign_key.foreign_table,
                &foreign_key.referred_columns,
                foreign_key.on_delete,
                foreign_key.on_update,
                "ALTER TABLE ADD CONSTRAINT FOREIGN KEY",
            ));
        }
        TableConstraint::Check(_) | TableConstraint::Index(_) => {}
        TableConstraint::FulltextOrSpatial(_) => {
            ctx.warn_unsupported(
                "FULLTEXT/SPATIAL constraint",
                span_from_spanned(input, offsets, constraint),
            );
        }
        TableConstraint::PrimaryKeyUsingIndex(_) | TableConstraint::UniqueUsingIndex(_) => {
            ctx.warn_unsupported(
                "PRIMARY KEY/UNIQUE USING INDEX constraint",
                span_from_spanned(input, offsets, constraint),
            );
        }
    }
}
