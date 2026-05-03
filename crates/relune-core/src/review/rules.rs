//! Review rule implementations.
//!
//! Each rule consumes the `SchemaDiff` plus the `before` and `after`
//! schemas and emits zero or more `RiskFinding`s. Rules are intentionally
//! pure: suppression, configuration, and ordering happen in the caller.

use std::collections::{HashMap, HashSet};

use super::{DialectScope, EffectiveDialect, ReviewRuleId, ReviewSeverity, RiskFinding};
use crate::SqlDialect;
use crate::diff::{ChangeKind, ColumnDiff, ForeignKeyDiff, IndexDiff, SchemaDiff, TableDiff};
use crate::model::{ForeignKey, Schema, Table};

/// Runs every rule in `applied_rules` against the diff.
///
/// `dialect` is the effective dialect resolved by the caller (CLI, wasm,
/// or a direct library user). It gates the lock-risk rules that only
/// fire on a specific dialect via `ReviewRuleId::dialect_scope`; other
/// rules ignore it. The caller is responsible for filtering out
/// suppressed rules and suppressing per-table afterwards; this function
/// only enforces "did the user select this rule" and "does the dialect
/// scope match".
//
// The dispatcher hits 12 rule arms; splitting it would only force the
// shared `selected` / `context` state through helper signatures.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn run_rules(
    diff: &SchemaDiff,
    before: &Schema,
    after: &Schema,
    applied_rules: &[ReviewRuleId],
    dialect: EffectiveDialect,
) -> Vec<RiskFinding> {
    let context = RuleContext::build(diff, before, after, dialect);
    let mut findings = Vec::new();
    let selected: HashSet<ReviewRuleId> = applied_rules.iter().copied().collect();

    for table_diff in &diff.modified_tables {
        let Some(after_table) = context.find_after_table(&table_diff.table_name) else {
            continue;
        };
        let before_table = context.find_before_table(&table_diff.table_name);

        for column_diff in &table_diff.column_diffs {
            if context.rule_active(ReviewRuleId::DropColumnReferenced, &selected) {
                check_drop_column_referenced(
                    table_diff,
                    column_diff,
                    before_table,
                    &context,
                    &mut findings,
                );
            }
            if context.rule_active(ReviewRuleId::AddNotNullOnExisting, &selected) {
                check_add_not_null_on_existing(
                    table_diff,
                    column_diff,
                    before_table,
                    after_table,
                    &mut findings,
                );
            }
            if context.rule_active(ReviewRuleId::TypeNarrow, &selected) {
                check_type_narrow(table_diff, column_diff, after_table, &mut findings);
            }
            if context.rule_active(ReviewRuleId::DropPkOrUnique, &selected) {
                check_drop_pk_or_unique_column(
                    table_diff,
                    column_diff,
                    before_table,
                    after_table,
                    &context,
                    &mut findings,
                );
            }
            if context.rule_active(ReviewRuleId::AlterColumnType, &selected) {
                check_alter_column_type_lock(
                    table_diff,
                    column_diff,
                    after_table,
                    &context,
                    &mut findings,
                );
            }
        }

        for index_diff in &table_diff.index_diffs {
            if context.rule_active(ReviewRuleId::DropPkOrUnique, &selected) {
                check_drop_pk_or_unique_index(
                    table_diff,
                    index_diff,
                    before_table,
                    after_table,
                    &context,
                    &mut findings,
                );
            }
            if context.rule_active(ReviewRuleId::AddUniqueOnExisting, &selected) {
                check_add_unique_on_existing(
                    table_diff,
                    index_diff,
                    before_table,
                    after_table,
                    &mut findings,
                );
            }
            if context.rule_active(ReviewRuleId::AddIndexOnLargeTable, &selected) {
                check_add_index_on_large_table(
                    table_diff,
                    index_diff,
                    after_table,
                    &context,
                    &mut findings,
                );
            }
        }

        if context.rule_active(ReviewRuleId::DropPkOrUnique, &selected) {
            check_drop_pk_or_unique_widened(
                table_diff,
                before_table,
                after_table,
                &context,
                &mut findings,
            );
        }

        for fk_diff in &table_diff.fk_diffs {
            if context.rule_active(ReviewRuleId::AddCascadeDelete, &selected) {
                check_add_cascade_delete(table_diff, fk_diff, after_table, &context, &mut findings);
            }
            if context.rule_active(ReviewRuleId::FkWithoutIndex, &selected) {
                check_fk_without_index(table_diff, fk_diff, after_table, &mut findings);
            }
            if context.rule_active(ReviewRuleId::AddFkOnExisting, &selected) {
                check_add_fk_on_existing(table_diff, fk_diff, after_table, &context, &mut findings);
            }
        }

        if context.rule_active(ReviewRuleId::RewriteTable, &selected) {
            check_rewrite_table(
                table_diff,
                before_table,
                after_table,
                &context,
                &mut findings,
            );
        }
    }

    if context.rule_active(ReviewRuleId::DropTableReferenced, &selected) {
        check_drop_table_referenced(diff, &context, &mut findings);
    }

    findings
}

struct RuleContext<'a> {
    before: &'a Schema,
    after: &'a Schema,
    /// Lower-cased `schema.table` (or bare `table`) → table in `before`.
    /// Replaces the per-rule linear scan over `before.tables`. Built
    /// once at `RuleContext::build` and shared across every rule.
    before_by_qname: HashMap<String, &'a Table>,
    /// Lower-cased qualified name → table in `after`. Same shape as
    /// `before_by_qname`.
    after_by_qname: HashMap<String, &'a Table>,
    /// Set of FK names + (table, columns) that are removed by this diff.
    /// Used to decide whether a referenced column / table is being
    /// "intentionally" disconnected at the same time.
    removed_fks: HashSet<RemovedFk>,
    /// Set of table qualified names removed by this diff (lower-cased).
    removed_tables: HashSet<String>,
    /// Effective dialect used to gate lock-risk rules.
    dialect: EffectiveDialect,
}

/// Build a lower-cased `qualified_name` → `&Table` lookup for one side of
/// the diff. Schemas may legally contain multiple tables that lower-case
/// to the same key (e.g. `Users` and `users` under `PostgreSQL`'s
/// case-folding); the first occurrence wins to mirror the prior linear
/// `find` semantics.
fn index_tables_by_qname(schema: &Schema) -> HashMap<String, &Table> {
    let mut map = HashMap::with_capacity(schema.tables.len());
    for table in &schema.tables {
        map.entry(table.qualified_name().to_ascii_lowercase())
            .or_insert(table);
    }
    map
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RemovedFk {
    /// Lower-cased qualified name of the table that owns the FK.
    table_name: String,
    /// Optional FK constraint name (lower-cased).
    fk_name: Option<String>,
    /// Lower-cased ordered tuple of `(from_column, to_column)` pairs.
    column_pairs: Vec<(String, String)>,
    /// Lower-cased target schema, normalized to the owner schema when
    /// the FK was unqualified. Lets two homonym targets (e.g.
    /// `auth.users` and `public.users`) be tracked separately.
    to_schema: Option<String>,
    /// Lower-cased target table bare name.
    to_table: String,
}

impl<'a> RuleContext<'a> {
    fn build(
        diff: &SchemaDiff,
        before: &'a Schema,
        after: &'a Schema,
        dialect: EffectiveDialect,
    ) -> Self {
        let before_by_qname = index_tables_by_qname(before);
        let after_by_qname = index_tables_by_qname(after);
        let lookup_owner_schema = |qname: &str| -> Option<String> {
            let key = qname.to_ascii_lowercase();
            before_by_qname
                .get(&key)
                .or_else(|| after_by_qname.get(&key))
                .and_then(|t| t.schema_name.clone())
        };

        let mut removed_fks: HashSet<RemovedFk> = HashSet::new();
        for table_diff in &diff.modified_tables {
            // Owner schema lets us treat `REFERENCES users` and
            // `REFERENCES <owner_schema>.users` as the same shape.
            let owner_schema = lookup_owner_schema(&table_diff.table_name);
            for fk_diff in &table_diff.fk_diffs {
                // The "old shape" of an FK is gone in `after` only when its
                // column pairs or target table actually moved. A `Modified`
                // diff that only flips `ON DELETE` / `ON UPDATE` keeps the
                // same reference, so we must NOT add it to `removed_fks` —
                // doing so would silence drop-column-referenced and friends
                // even though the FK still depends on the dropped column.
                let removed_shape = match fk_diff.change_kind {
                    ChangeKind::Removed => fk_diff.old_value.as_ref(),
                    ChangeKind::Modified => {
                        match (fk_diff.old_value.as_ref(), fk_diff.new_value.as_ref()) {
                            (Some(old), Some(new))
                                if fk_shape_changed(
                                    old,
                                    new,
                                    owner_schema.as_deref(),
                                    before,
                                    after,
                                ) =>
                            {
                                Some(old)
                            }
                            _ => None,
                        }
                    }
                    ChangeKind::Added => None,
                };
                if let Some(old) = removed_shape {
                    // If the after side still has an FK on the same owner
                    // table that resolves to the same target and column
                    // pairs, the reference survives (e.g. an FK rename
                    // shows up as Removed + Added). Skip — otherwise
                    // cross-table rules would be silenced for the
                    // surviving reference.
                    if fk_shape_survives_in_after(
                        old,
                        &table_diff.table_name,
                        owner_schema.as_deref(),
                        before,
                        after,
                    ) {
                        continue;
                    }
                    removed_fks.insert(RemovedFk {
                        table_name: table_diff.table_name.to_lowercase(),
                        fk_name: old.name.as_ref().map(|n| n.to_lowercase()),
                        column_pairs: old
                            .from_columns
                            .iter()
                            .zip(old.to_columns.iter())
                            .map(|(f, t)| (f.to_lowercase(), t.to_lowercase()))
                            .collect(),
                        to_schema: old
                            .to_schema
                            .as_deref()
                            .or(owner_schema.as_deref())
                            .map(str::to_ascii_lowercase),
                        to_table: old.to_table.to_lowercase(),
                    });
                }
            }
        }
        // FKs that vanish because their owning table is removed should
        // also be considered "removed" for cross-table accounting.
        let removed_table_qnames: HashSet<String> = diff
            .removed_tables
            .iter()
            .map(|t| t.to_lowercase())
            .collect();
        for table in &before.tables {
            if removed_table_qnames.contains(&table.qualified_name().to_lowercase()) {
                for fk in &table.foreign_keys {
                    removed_fks.insert(RemovedFk {
                        table_name: table.qualified_name().to_lowercase(),
                        fk_name: fk.name.as_ref().map(|n| n.to_lowercase()),
                        column_pairs: fk
                            .from_columns
                            .iter()
                            .zip(fk.to_columns.iter())
                            .map(|(f, t)| (f.to_lowercase(), t.to_lowercase()))
                            .collect(),
                        to_schema: fk
                            .to_schema
                            .as_deref()
                            .or(table.schema_name.as_deref())
                            .map(str::to_ascii_lowercase),
                        to_table: fk.to_table.to_lowercase(),
                    });
                }
            }
        }

        Self {
            before,
            after,
            before_by_qname,
            after_by_qname,
            removed_fks,
            removed_tables: removed_table_qnames,
            dialect,
        }
    }

    /// Returns true when a rule is selected by the caller and its
    /// `dialect_scope` includes the effective dialect.
    ///
    /// Lock-risk rules with `DialectScope::OneOf(...)` silently skip
    /// when the dialect is `Auto` or outside the rule's scope; the
    /// caller does not need to pre-filter `applied_rules`.
    fn rule_active(&self, rule: ReviewRuleId, selected: &HashSet<ReviewRuleId>) -> bool {
        if !selected.contains(&rule) {
            return false;
        }
        match rule.dialect_scope() {
            DialectScope::Any => true,
            DialectScope::OneOf(scopes) => match self.dialect {
                EffectiveDialect::Auto => false,
                EffectiveDialect::Postgres => scopes.contains(&SqlDialect::Postgres),
                EffectiveDialect::Mysql => scopes.contains(&SqlDialect::Mysql),
                EffectiveDialect::Sqlite => scopes.contains(&SqlDialect::Sqlite),
            },
        }
    }

    fn find_before_table(&self, qualified_name: &str) -> Option<&'a Table> {
        self.before_by_qname
            .get(&qualified_name.to_ascii_lowercase())
            .copied()
    }

    fn find_after_table(&self, qualified_name: &str) -> Option<&'a Table> {
        self.after_by_qname
            .get(&qualified_name.to_ascii_lowercase())
            .copied()
    }

    /// Returns true if the FK on `table` is removed in this diff (either
    /// directly or because the owning table was dropped).
    fn fk_is_removed(&self, owning_table_qname: &str, fk: &ForeignKey) -> bool {
        // Normalize the FK's target schema against the owner's schema so
        // an unqualified `REFERENCES users` from `public.orders` only
        // matches the corresponding `RemovedFk` for `public.users`, not a
        // homonym in another schema.
        let owner_key = owning_table_qname.to_ascii_lowercase();
        let owner_schema = self
            .before_by_qname
            .get(&owner_key)
            .or_else(|| self.after_by_qname.get(&owner_key))
            .and_then(|t| t.schema_name.as_deref());
        let key = RemovedFk {
            table_name: owning_table_qname.to_lowercase(),
            fk_name: fk.name.as_ref().map(|n| n.to_lowercase()),
            column_pairs: fk
                .from_columns
                .iter()
                .zip(fk.to_columns.iter())
                .map(|(f, t)| (f.to_lowercase(), t.to_lowercase()))
                .collect(),
            to_schema: fk
                .to_schema
                .as_deref()
                .or(owner_schema)
                .map(str::to_ascii_lowercase),
            to_table: fk.to_table.to_lowercase(),
        };
        self.removed_fks.contains(&key)
    }
}

/// Returns true when at least one FK with `removed`'s shape survives
/// in `after` *over and above* what already existed in `before`. In
/// other words, `after_count(shape) >= before_count(shape)`. Used to
/// suppress `RemovedFk` registration when a FK is just renamed or
/// dropped-and-recreated with the same shape — without
/// over-suppressing when `before` had multiple same-shape FKs and only
/// some of them are dropped (in which case `after_count < before_count`
/// and the dropped FK is a genuine loss).
fn fk_shape_survives_in_after(
    removed: &crate::export::ForeignKeyExport,
    owner_qname: &str,
    owner_schema: Option<&str>,
    before: &Schema,
    after: &Schema,
) -> bool {
    let Some(after_owner) = after
        .tables
        .iter()
        .find(|t| t.qualified_name().eq_ignore_ascii_case(owner_qname))
    else {
        return false;
    };
    let removed_target = resolve_table_id(
        before,
        removed.to_schema.as_deref(),
        &removed.to_table,
        owner_schema,
    );
    let removed_pairs: Vec<(String, String)> = removed
        .from_columns
        .iter()
        .zip(removed.to_columns.iter())
        .map(|(f, t)| (f.to_ascii_lowercase(), t.to_ascii_lowercase()))
        .collect();
    let shape_matches = |fk: &ForeignKey, schema: &Schema, target: &Option<String>| {
        let candidate_target =
            resolve_table_id(schema, fk.to_schema.as_deref(), &fk.to_table, owner_schema);
        let target_eq = match (target, &candidate_target) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (None, None) => {
                let normalize = |s: Option<&str>| s.or(owner_schema).map(str::to_ascii_lowercase);
                normalize(removed.to_schema.as_deref()) == normalize(fk.to_schema.as_deref())
                    && removed.to_table.eq_ignore_ascii_case(&fk.to_table)
            }
            _ => false,
        };
        if !target_eq {
            return false;
        }
        if fk.from_columns.len() != removed_pairs.len() {
            return false;
        }
        fk.from_columns
            .iter()
            .zip(fk.to_columns.iter())
            .zip(removed_pairs.iter())
            .all(|((f, t), (rf, rt))| f.eq_ignore_ascii_case(rf) && t.eq_ignore_ascii_case(rt))
    };
    let before_count = before
        .tables
        .iter()
        .find(|t| t.qualified_name().eq_ignore_ascii_case(owner_qname))
        .map_or(0, |t| {
            t.foreign_keys
                .iter()
                .filter(|fk| shape_matches(fk, before, &removed_target))
                .count()
        });
    let after_count = after_owner
        .foreign_keys
        .iter()
        .filter(|fk| shape_matches(fk, after, &removed_target))
        .count();
    after_count >= before_count
}

/// Returns true when an FK's reference shape (target table + column
/// pairs) differs between `old` and `new`. Used to distinguish a true
/// retarget (old shape is gone in `after`) from a referential-action
/// only change (`ON DELETE` / `ON UPDATE`) that preserves the
/// reference.
///
/// Comparison goes through [`resolve_table_id`] using the same
/// owner-schema-preferred + bare-name fallback policy that
/// [`fk_targets_table`] uses, so an unqualified `REFERENCES users`
/// that resolves to `auth.users` via fallback is treated as the same
/// shape when the FK is later spelled `REFERENCES auth.users`. The
/// `before` / `after` schemas are used to resolve the old/new sides
/// respectively, since a same-name homonym may have appeared between
/// states.
fn fk_shape_changed(
    old: &crate::export::ForeignKeyExport,
    new: &crate::export::ForeignKeyExport,
    owner_schema: Option<&str>,
    before: &Schema,
    after: &Schema,
) -> bool {
    let old_target = resolve_table_id(
        before,
        old.to_schema.as_deref(),
        &old.to_table,
        owner_schema,
    );
    let new_target = resolve_table_id(after, new.to_schema.as_deref(), &new.to_table, owner_schema);
    let target_eq = match (&old_target, &new_target) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => {
            // Neither side resolves to a known table — fall back to the
            // raw `(schema, table)` pair, normalizing `None` against the
            // owner schema.
            let resolve_raw = |s: Option<&str>| s.or(owner_schema).map(str::to_ascii_lowercase);
            resolve_raw(old.to_schema.as_deref()) == resolve_raw(new.to_schema.as_deref())
                && old.to_table.eq_ignore_ascii_case(&new.to_table)
        }
        _ => false,
    };
    let columns_eq = old.from_columns.len() == new.from_columns.len()
        && old.to_columns.len() == new.to_columns.len()
        && old
            .from_columns
            .iter()
            .zip(new.from_columns.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        && old
            .to_columns
            .iter()
            .zip(new.to_columns.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b));
    !(target_eq && columns_eq)
}

fn fk_label(fk: &ForeignKey, owner_qname: &str) -> String {
    fk.name.clone().unwrap_or_else(|| {
        format!(
            "{}({})->{}({})",
            owner_qname,
            fk.from_columns.join(","),
            fk.to_table,
            fk.to_columns.join(",")
        )
    })
}

fn fk_label_short(fk: &ForeignKey) -> String {
    fk.name
        .clone()
        .unwrap_or_else(|| format!("{}({})", fk.to_table, fk.from_columns.join(",")))
}

/// `risk/drop-column-referenced` — column being dropped is still
/// referenced by some FK.
fn check_drop_column_referenced(
    table_diff: &TableDiff,
    column_diff: &ColumnDiff,
    before_table: Option<&Table>,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    if column_diff.change_kind != ChangeKind::Removed {
        return;
    }
    let Some(before_table) = before_table else {
        return;
    };

    let column_lower = column_diff.column_name.to_ascii_lowercase();
    let before_qname = before_table.qualified_name();

    // Outgoing FKs: the dropped column appears in `from_columns`.
    for fk in &before_table.foreign_keys {
        if !fk
            .from_columns
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&column_diff.column_name))
        {
            continue;
        }
        if context.fk_is_removed(&before_qname, fk) {
            continue;
        }
        let related = resolve_table_id(
            context.before,
            fk.to_schema.as_deref(),
            &fk.to_table,
            before_table.schema_name.as_deref(),
        );
        let mut finding = RiskFinding::new(
            ReviewRuleId::DropColumnReferenced,
            ReviewSeverity::Breaking,
            format!(
                "Column {}.{} is referenced by FK {}. Dropping it will fail.",
                before_qname,
                column_diff.column_name,
                fk_label(fk, &before_qname),
            ),
        )
        .with_table(&before_table.stable_id, &before_qname)
        .with_column(&column_diff.column_name)
        .with_mitigation("Drop or update the referencing FK in the same migration.");
        if let Some(name) = &fk.name {
            finding = finding.with_fk_name(name);
        }
        if let Some(related_id) = related {
            finding = finding.with_related_table(related_id);
        }
        findings.push(finding);
    }

    // Incoming FKs across the schema (including self-references on the
    // same table): something references the dropped column as a FK target.
    for other_table in &context.before.tables {
        let is_same_table = other_table
            .qualified_name()
            .eq_ignore_ascii_case(&before_qname);
        for fk in &other_table.foreign_keys {
            // Resolve target name to detect references to before_table.
            if !fk_targets_table(
                fk,
                before_table,
                context.before,
                other_table.schema_name.as_deref(),
            ) {
                continue;
            }
            if !fk
                .to_columns
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&column_lower))
            {
                continue;
            }
            // For same-table FKs, the outgoing-FK loop above already
            // emits a finding when the dropped column is in
            // `from_columns`. Avoid double-reporting that case here.
            if is_same_table
                && fk
                    .from_columns
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case(&column_diff.column_name))
            {
                continue;
            }
            let other_qname = other_table.qualified_name();
            if context.fk_is_removed(&other_qname, fk) {
                continue;
            }
            let mut finding = RiskFinding::new(
                ReviewRuleId::DropColumnReferenced,
                ReviewSeverity::Breaking,
                format!(
                    "Column {}.{} is referenced by FK {} on {}. Dropping it will fail.",
                    before_qname,
                    column_diff.column_name,
                    fk_label_short(fk),
                    other_qname,
                ),
            )
            .with_table(&before_table.stable_id, &before_qname)
            .with_column(&column_diff.column_name)
            .with_related_table(&other_table.stable_id)
            .with_mitigation("Drop or update the referencing FK in the same migration.");
            if let Some(name) = &fk.name {
                finding = finding.with_fk_name(name);
            }
            findings.push(finding);
        }
    }
    let _ = table_diff;
}

/// `risk/drop-table-referenced` — table being dropped is still
/// referenced by some FK that survives the migration.
fn check_drop_table_referenced(
    diff: &SchemaDiff,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    for removed in &diff.removed_tables {
        let Some(removed_table) = context
            .before
            .tables
            .iter()
            .find(|t| t.qualified_name().eq_ignore_ascii_case(removed))
        else {
            continue;
        };
        let removed_qname = removed_table.qualified_name();

        for other_table in &context.before.tables {
            if other_table
                .qualified_name()
                .eq_ignore_ascii_case(&removed_qname)
            {
                continue;
            }
            // Skip if the referencing table is itself being dropped.
            if context
                .removed_tables
                .contains(&other_table.qualified_name().to_lowercase())
            {
                continue;
            }
            for fk in &other_table.foreign_keys {
                if !fk_targets_table(
                    fk,
                    removed_table,
                    context.before,
                    other_table.schema_name.as_deref(),
                ) {
                    continue;
                }
                let other_qname = other_table.qualified_name();
                if context.fk_is_removed(&other_qname, fk) {
                    continue;
                }
                let mut finding = RiskFinding::new(
                    ReviewRuleId::DropTableReferenced,
                    ReviewSeverity::Breaking,
                    format!(
                        "Table {} is referenced by FK {} on {}. Dropping it will fail.",
                        removed_qname,
                        fk_label_short(fk),
                        other_qname,
                    ),
                )
                .with_table(&removed_table.stable_id, &removed_qname)
                .with_related_table(&other_table.stable_id)
                .with_mitigation("Drop or repoint the referencing FKs in the same migration.");
                if let Some(name) = &fk.name {
                    finding = finding.with_fk_name(name);
                }
                findings.push(finding);
            }
        }
    }
}

/// `risk/add-not-null-on-existing` — NOT NULL added to a column on an
/// existing table.
fn check_add_not_null_on_existing(
    table_diff: &TableDiff,
    column_diff: &ColumnDiff,
    before_table: Option<&Table>,
    after_table: &Table,
    findings: &mut Vec<RiskFinding>,
) {
    // Skip new tables entirely (handled by SchemaDiff.added_tables).
    if before_table.is_none() {
        return;
    }

    let triggered = match column_diff.change_kind {
        ChangeKind::Added => column_diff.new_value.as_ref().is_some_and(|v| !v.nullable),
        ChangeKind::Modified => {
            let was_nullable = column_diff.old_value.as_ref().is_some_and(|v| v.nullable);
            let is_now_not_nullable = column_diff.new_value.as_ref().is_some_and(|v| !v.nullable);
            was_nullable && is_now_not_nullable
        }
        ChangeKind::Removed => false,
    };
    if !triggered {
        return;
    }

    findings.push(
        RiskFinding::new(
            ReviewRuleId::AddNotNullOnExisting,
            ReviewSeverity::Warning,
            format!(
                "NOT NULL column {}.{} was added to an existing table. Existing rows may fail the constraint.",
                table_diff.table_name, column_diff.column_name,
            ),
        )
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_column(&column_diff.column_name)
        .with_mitigation("Add as nullable, backfill, then ALTER to NOT NULL."),
    );
}

/// `risk/type-narrow` — column data type is being narrowed in a way
/// that may reject existing data.
fn check_type_narrow(
    table_diff: &TableDiff,
    column_diff: &ColumnDiff,
    after_table: &Table,
    findings: &mut Vec<RiskFinding>,
) {
    if column_diff.change_kind != ChangeKind::Modified {
        return;
    }
    let (Some(old), Some(new)) = (
        column_diff.old_value.as_ref(),
        column_diff.new_value.as_ref(),
    ) else {
        return;
    };
    let Some((old_label, new_label)) = detect_type_narrowing(&old.data_type, &new.data_type) else {
        return;
    };

    findings.push(
        RiskFinding::new(
            ReviewRuleId::TypeNarrow,
            ReviewSeverity::Breaking,
            format!(
                "Column {}.{} is being narrowed from {} to {}. Existing data may be truncated or rejected.",
                table_diff.table_name, column_diff.column_name, old_label, new_label,
            ),
        )
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_column(&column_diff.column_name)
        .with_mitigation("Verify all existing values fit the new type before applying."),
    );
}

/// Detect a narrowing transition between two data type strings. Returns
/// the canonicalized old/new labels for use in the finding message.
fn detect_type_narrowing(old: &str, new: &str) -> Option<(String, String)> {
    let old_norm = old.trim().to_ascii_uppercase();
    let new_norm = new.trim().to_ascii_uppercase();
    if old_norm == new_norm {
        return None;
    }

    // VARCHAR / CHARACTER VARYING / CHAR with width.
    if let (Some((old_kind, old_n)), Some((new_kind, new_n))) =
        (parse_char_type(&old_norm), parse_char_type(&new_norm))
        && old_kind == new_kind
        && new_n < old_n
    {
        return Some((
            format!("{old_kind}({old_n})"),
            format!("{new_kind}({new_n})"),
        ));
    }

    // NUMERIC(P, S).
    if let (Some((old_p, old_s)), Some((new_p, new_s))) =
        (parse_numeric(&old_norm), parse_numeric(&new_norm))
        && (new_p < old_p || new_s < old_s)
    {
        return Some((
            format!("NUMERIC({old_p},{old_s})"),
            format!("NUMERIC({new_p},{new_s})"),
        ));
    }

    // Integer width shrink.
    if let (Some(old_rank), Some(new_rank)) = (int_rank(&old_norm), int_rank(&new_norm))
        && new_rank < old_rank
    {
        return Some((old_norm, new_norm));
    }

    None
}

fn parse_char_type(value: &str) -> Option<(&'static str, u32)> {
    let (kind, rest) = if let Some(rest) = value.strip_prefix("CHARACTER VARYING") {
        ("VARCHAR", rest)
    } else if let Some(rest) = value.strip_prefix("VARCHAR") {
        ("VARCHAR", rest)
    } else if let Some(rest) = value.strip_prefix("CHAR") {
        ("CHAR", rest)
    } else {
        return None;
    };
    let trimmed = rest.trim();
    if !trimmed.starts_with('(') {
        return None;
    }
    let inner = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    inner.trim().parse::<u32>().ok().map(|n| (kind, n))
}

fn parse_numeric(value: &str) -> Option<(u32, u32)> {
    let rest = value
        .strip_prefix("NUMERIC")
        .or_else(|| value.strip_prefix("DECIMAL"))?
        .trim();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = inner.split(',');
    let p = parts.next()?.trim().parse::<u32>().ok()?;
    let s = parts.next()?.trim().parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((p, s))
}

const fn int_rank(value: &str) -> Option<u32> {
    match value.as_bytes() {
        b"BIGINT" | b"INT8" => Some(64),
        b"INT" | b"INTEGER" | b"INT4" => Some(32),
        b"SMALLINT" | b"INT2" => Some(16),
        b"TINYINT" => Some(8),
        _ => None,
    }
}

/// `risk/drop-pk-or-unique` (column path) — primary key column going
/// away by either column drop or losing its `is_primary_key` flag.
fn check_drop_pk_or_unique_column(
    table_diff: &TableDiff,
    column_diff: &ColumnDiff,
    before_table: Option<&Table>,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    let Some(before_table) = before_table else {
        return;
    };

    let was_pk = column_diff
        .old_value
        .as_ref()
        .is_some_and(|v| v.primary_key);
    let is_pk_now = column_diff
        .new_value
        .as_ref()
        .is_some_and(|v| v.primary_key);
    let lost_pk = match column_diff.change_kind {
        ChangeKind::Removed => was_pk,
        ChangeKind::Modified => was_pk && !is_pk_now,
        ChangeKind::Added => false,
    };
    if !lost_pk {
        return;
    }

    let before_pk_cols: Vec<String> = before_table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.to_ascii_lowercase())
        .collect();
    let after_pk_cols: Vec<String> = after_table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.to_ascii_lowercase())
        .collect();
    if same_column_set(&after_pk_cols, &before_pk_cols) {
        return;
    }
    if covered_by_unique_index(after_table, &before_pk_cols) {
        return;
    }

    let (severity, related_fk) = evaluate_drop_pk_severity(before_table, &before_pk_cols, context);

    let message = match (severity, &related_fk) {
        (ReviewSeverity::Breaking, Some((other_qname, fk_label))) => format!(
            "Primary key on {} ({}) is being dropped while FK {} on {} still references it. Migration will fail.",
            table_diff.table_name,
            before_pk_cols.join(","),
            fk_label,
            other_qname,
        ),
        _ => format!(
            "Primary key on {} ({}) is being dropped without replacement. Application logic relying on it may break.",
            table_diff.table_name,
            before_pk_cols.join(","),
        ),
    };

    let mitigation = match severity {
        ReviewSeverity::Breaking => "Drop or repoint the referencing FKs in the same migration.",
        _ => "Add a replacement PRIMARY KEY in the same migration if uniqueness is still required.",
    };

    let mut finding = RiskFinding::new(ReviewRuleId::DropPkOrUnique, severity, message)
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_column(&column_diff.column_name)
        .with_mitigation(mitigation);
    if let Some((_, fk_text)) = related_fk {
        finding = finding.with_fk_name(fk_text);
    }
    findings.push(finding);
}

/// `risk/drop-pk-or-unique` (index path) — UNIQUE index dropped or
/// losing its uniqueness invariant.
fn check_drop_pk_or_unique_index(
    table_diff: &TableDiff,
    index_diff: &IndexDiff,
    before_table: Option<&Table>,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    let lost_unique = match index_diff.change_kind {
        ChangeKind::Removed => index_diff.old_value.as_ref().is_some_and(|v| v.unique),
        ChangeKind::Modified => {
            let was_unique = index_diff.old_value.as_ref().is_some_and(|v| v.unique);
            let is_unique_now = index_diff.new_value.as_ref().is_some_and(|v| v.unique);
            was_unique && !is_unique_now
        }
        ChangeKind::Added => false,
    };
    if !lost_unique {
        return;
    }
    let Some(old) = index_diff.old_value.as_ref() else {
        return;
    };
    let lower_cols: Vec<String> = old.columns.iter().map(|c| c.to_ascii_lowercase()).collect();
    if covered_by_unique_index(after_table, &lower_cols) {
        return;
    }

    let (severity, related_fk) = before_table.map_or((ReviewSeverity::Warning, None), |bt| {
        evaluate_unique_loss_severity(bt, &lower_cols, context)
    });

    let label = old
        .name
        .clone()
        .unwrap_or_else(|| format!("({})", old.columns.join(",")));
    let message = match (severity, &related_fk) {
        (ReviewSeverity::Breaking, Some((other_qname, fk_label))) => format!(
            "UNIQUE index {}.{} is being dropped while FK {} on {} still references the column set ({}). Migration may fail.",
            table_diff.table_name,
            label,
            fk_label,
            other_qname,
            old.columns.join(","),
        ),
        _ => format!(
            "UNIQUE index {}.{} is being dropped without replacement. Application logic relying on uniqueness may break.",
            table_diff.table_name, label,
        ),
    };
    let mitigation = match severity {
        ReviewSeverity::Breaking => "Drop or repoint the referencing FKs in the same migration.",
        _ => "Add a replacement UNIQUE in the same migration if uniqueness is still required.",
    };
    let mut finding = RiskFinding::new(ReviewRuleId::DropPkOrUnique, severity, message)
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_mitigation(mitigation);
    if let Some((_, fk_text)) = related_fk {
        finding = finding.with_fk_name(fk_text);
    }
    findings.push(finding);
}

/// `risk/drop-pk-or-unique` (PK widening path) — every original PK column
/// remains a PK column in `after`, but the PK has been widened with extra
/// columns. The original column set is no longer guaranteed unique, so an
/// FK that still references just the original set will not resolve.
///
/// The column-path check only fires when a column loses its PK status;
/// it cannot see this case because no column actually drops out.
fn check_drop_pk_or_unique_widened(
    table_diff: &TableDiff,
    before_table: Option<&Table>,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    let Some(before_table) = before_table else {
        return;
    };

    let before_pk_cols: Vec<String> = before_table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.to_ascii_lowercase())
        .collect();
    if before_pk_cols.is_empty() {
        return;
    }
    let after_pk_cols: Vec<String> = after_table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.to_ascii_lowercase())
        .collect();

    if same_column_set(&after_pk_cols, &before_pk_cols) {
        return;
    }
    // Only the strict-superset case is unique to this check; if any
    // before-PK column is missing from after, the column-path check
    // already fires for it.
    if !covers(&after_pk_cols, &before_pk_cols) {
        return;
    }
    if covered_by_unique_index(after_table, &before_pk_cols) {
        return;
    }

    let (severity, related_fk) = evaluate_drop_pk_severity(before_table, &before_pk_cols, context);

    let message = match (severity, &related_fk) {
        (ReviewSeverity::Breaking, Some((other_qname, fk_label))) => format!(
            "Primary key on {} is being widened from ({}) to ({}). FK {} on {} still references ({}), which is no longer guaranteed unique. Migration will fail.",
            table_diff.table_name,
            before_pk_cols.join(","),
            after_pk_cols.join(","),
            fk_label,
            other_qname,
            before_pk_cols.join(","),
        ),
        _ => format!(
            "Primary key on {} is being widened from ({}) to ({}). The original column set is no longer guaranteed unique.",
            table_diff.table_name,
            before_pk_cols.join(","),
            after_pk_cols.join(","),
        ),
    };

    let mitigation = match severity {
        ReviewSeverity::Breaking => {
            "Add a UNIQUE index on the original PK columns or repoint the referencing FKs in the same migration."
        }
        _ => {
            "Add a UNIQUE index on the original PK columns if the uniqueness guarantee is still required."
        }
    };

    let mut finding = RiskFinding::new(ReviewRuleId::DropPkOrUnique, severity, message)
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_mitigation(mitigation);
    if let Some((_, fk_text)) = related_fk {
        finding = finding.with_fk_name(fk_text);
    }
    findings.push(finding);
}

/// Determine whether dropping the UNIQUE index over `unique_cols`
/// breaks any incoming FK that references that exact column set.
fn evaluate_unique_loss_severity(
    before_table: &Table,
    unique_cols: &[String],
    context: &RuleContext<'_>,
) -> (ReviewSeverity, Option<(String, String)>) {
    if unique_cols.is_empty() {
        return (ReviewSeverity::Warning, None);
    }
    let mut sorted_unique = unique_cols.to_vec();
    sorted_unique.sort();

    for other_table in &context.before.tables {
        if context
            .removed_tables
            .contains(&other_table.qualified_name().to_lowercase())
        {
            continue;
        }
        for fk in &other_table.foreign_keys {
            if !fk_targets_table(
                fk,
                before_table,
                context.before,
                other_table.schema_name.as_deref(),
            ) {
                continue;
            }
            let other_qname = other_table.qualified_name();
            if context.fk_is_removed(&other_qname, fk) {
                continue;
            }
            let mut sorted_to: Vec<String> = fk
                .to_columns
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            sorted_to.sort();
            if sorted_to == sorted_unique {
                return (
                    ReviewSeverity::Breaking,
                    Some((other_qname, fk_label_short(fk))),
                );
            }
        }
    }
    (ReviewSeverity::Warning, None)
}

fn evaluate_drop_pk_severity(
    before_table: &Table,
    before_pk_cols: &[String],
    context: &RuleContext<'_>,
) -> (ReviewSeverity, Option<(String, String)>) {
    if before_pk_cols.is_empty() {
        return (ReviewSeverity::Warning, None);
    }
    let mut sorted_pk = before_pk_cols.to_vec();
    sorted_pk.sort();

    for other_table in &context.before.tables {
        if other_table
            .qualified_name()
            .eq_ignore_ascii_case(&before_table.qualified_name())
        {
            continue;
        }
        if context
            .removed_tables
            .contains(&other_table.qualified_name().to_lowercase())
        {
            continue;
        }
        for fk in &other_table.foreign_keys {
            if !fk_targets_table(
                fk,
                before_table,
                context.before,
                other_table.schema_name.as_deref(),
            ) {
                continue;
            }
            let other_qname = other_table.qualified_name();
            if context.fk_is_removed(&other_qname, fk) {
                continue;
            }
            let mut sorted_to: Vec<String> = fk
                .to_columns
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            sorted_to.sort();
            if sorted_to == sorted_pk {
                return (
                    ReviewSeverity::Breaking,
                    Some((other_qname, fk_label_short(fk))),
                );
            }
        }
    }
    (ReviewSeverity::Warning, None)
}

/// `risk/add-unique-on-existing` — UNIQUE index added to an existing table.
fn check_add_unique_on_existing(
    table_diff: &TableDiff,
    index_diff: &IndexDiff,
    before_table: Option<&Table>,
    after_table: &Table,
    findings: &mut Vec<RiskFinding>,
) {
    if index_diff.change_kind != ChangeKind::Added {
        return;
    }
    let Some(new) = index_diff.new_value.as_ref() else {
        return;
    };
    if !new.unique {
        return;
    }
    // Suppress when the column set is already guaranteed unique in
    // `before`: e.g. replacing PRIMARY KEY(id) with UNIQUE(id), or
    // adding UNIQUE(id, tenant_id) on a table that already declared
    // PRIMARY KEY(id). No new uniqueness invariant is being introduced,
    // so existing rows cannot fail the new constraint.
    if let Some(before) = before_table
        && already_unique_in(before, &new.columns)
    {
        return;
    }

    let label = new
        .name
        .clone()
        .unwrap_or_else(|| format!("({})", new.columns.join(",")));
    findings.push(
        RiskFinding::new(
            ReviewRuleId::AddUniqueOnExisting,
            ReviewSeverity::Warning,
            format!(
                "UNIQUE index {} on ({}) is being added to existing table {}. Existing duplicate rows will fail the constraint.",
                label,
                new.columns.join(","),
                table_diff.table_name,
            ),
        )
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_mitigation("Verify no duplicates exist or deduplicate before applying."),
    );
}

/// Returns true when `columns` are already guaranteed unique in `table`
/// — i.e. there exists a primary key or unique index whose column set
/// is a subset of `columns`. Subset suffices because uniqueness on
/// `(a)` implies uniqueness on `(a, b)`.
fn already_unique_in(table: &Table, columns: &[String]) -> bool {
    if columns.is_empty() {
        return false;
    }
    let new_cols: HashSet<String> = columns.iter().map(|c| c.to_ascii_lowercase()).collect();

    let pk_cols: Vec<String> = table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.to_ascii_lowercase())
        .collect();
    if !pk_cols.is_empty() && pk_cols.iter().all(|c| new_cols.contains(c)) {
        return true;
    }

    table.indexes.iter().any(|idx| {
        if !idx.is_unique || idx.columns.is_empty() {
            return false;
        }
        idx.columns
            .iter()
            .all(|c| new_cols.contains(&c.to_ascii_lowercase()))
    })
}

/// `risk/add-cascade-delete` — FK gains `ON DELETE CASCADE`.
fn check_add_cascade_delete(
    table_diff: &TableDiff,
    fk_diff: &ForeignKeyDiff,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    let triggered = match fk_diff.change_kind {
        ChangeKind::Added => fk_diff
            .new_value
            .as_ref()
            .is_some_and(|v| v.on_delete.as_deref() == Some("CASCADE")),
        ChangeKind::Modified => {
            let old = fk_diff.old_value.as_ref();
            let new = fk_diff.new_value.as_ref();
            let was_cascade = old.is_some_and(|v| v.on_delete.as_deref() == Some("CASCADE"));
            let is_cascade = new.is_some_and(|v| v.on_delete.as_deref() == Some("CASCADE"));
            !was_cascade && is_cascade
        }
        ChangeKind::Removed => false,
    };
    if !triggered {
        return;
    }
    let Some(new) = fk_diff.new_value.as_ref() else {
        return;
    };

    let related_table_id = resolve_table_id(
        context.after,
        new.to_schema.as_deref(),
        &new.to_table,
        after_table.schema_name.as_deref(),
    );
    let label = new
        .name
        .clone()
        .unwrap_or_else(|| format!("{}({})", new.to_table, new.from_columns.join(",")));
    let mut finding = RiskFinding::new(
        ReviewRuleId::AddCascadeDelete,
        ReviewSeverity::Warning,
        format!(
            "FK {} on {} now uses ON DELETE CASCADE. Deleting {} rows will cascade.",
            label, table_diff.table_name, new.to_table,
        ),
    )
    .with_table(&after_table.stable_id, &table_diff.table_name)
    .with_mitigation("Confirm the cascade scope is intended; consider RESTRICT/SET NULL if not.");
    if let Some(name) = &new.name {
        finding = finding.with_fk_name(name);
    }
    if let Some(id) = related_table_id {
        finding = finding.with_related_table(id);
    }
    findings.push(finding);
}

/// `risk/fk-without-index` — newly added FK lacks a supporting index.
fn check_fk_without_index(
    table_diff: &TableDiff,
    fk_diff: &ForeignKeyDiff,
    after_table: &Table,
    findings: &mut Vec<RiskFinding>,
) {
    if fk_diff.change_kind != ChangeKind::Added {
        return;
    }
    let Some(new) = fk_diff.new_value.as_ref() else {
        return;
    };
    if fk_columns_are_indexed(after_table, &new.from_columns) {
        return;
    }

    let label = new
        .name
        .clone()
        .unwrap_or_else(|| format!("({})", new.from_columns.join(",")));
    let mut finding = RiskFinding::new(
        ReviewRuleId::FkWithoutIndex,
        ReviewSeverity::Info,
        format!(
            "New FK {} on {} ({}) has no supporting index.",
            label,
            table_diff.table_name,
            new.from_columns.join(","),
        ),
    )
    .with_table(&after_table.stable_id, &table_diff.table_name)
    .with_mitigation(format!(
        "Add an index on ({}) to avoid full-table scans on JOIN.",
        new.from_columns.join(","),
    ));
    if let Some(name) = &new.name {
        finding = finding.with_fk_name(name);
    }
    if new.from_columns.len() == 1 {
        finding = finding.with_column(new.from_columns[0].clone());
    }
    findings.push(finding);
}

/// `risk/add-index-on-large-table` — index added on an existing table;
/// non-CONCURRENT / non-INPLACE builds block writes for the duration of
/// the rebuild.
fn check_add_index_on_large_table(
    table_diff: &TableDiff,
    index_diff: &IndexDiff,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    if index_diff.change_kind != ChangeKind::Added {
        return;
    }
    // Skip newly added tables: an empty table cannot incur a problematic
    // long lock during index build.
    if context.find_before_table(&table_diff.table_name).is_none() {
        return;
    }
    let Some(new) = index_diff.new_value.as_ref() else {
        return;
    };

    let label = new
        .name
        .clone()
        .unwrap_or_else(|| format!("({})", new.columns.join(",")));
    let dialect = dialect_word(context.dialect);
    let (message, mitigation) = match context.dialect {
        EffectiveDialect::Postgres => (
            format!(
                "New index {label} on existing table {} ({}) ({dialect}). A non-CONCURRENT CREATE INDEX takes a SHARE lock that blocks writes for the duration of the build.",
                table_diff.table_name,
                new.columns.join(","),
            ),
            "Use CREATE INDEX CONCURRENTLY (and DROP INDEX CONCURRENTLY for rollback) in postgres.",
        ),
        EffectiveDialect::Mysql => (
            format!(
                "New index {label} on existing table {} ({}) ({dialect}). Default ALGORITHM=INPLACE may still block writes during rebuild on large tables.",
                table_diff.table_name,
                new.columns.join(","),
            ),
            "Use ALGORITHM=INPLACE, LOCK=NONE explicitly (5.6+) and verify the column type supports it.",
        ),
        // Dispatcher gates lock-risk rules by dialect; these branches are unreachable in practice.
        EffectiveDialect::Auto | EffectiveDialect::Sqlite => return,
    };

    let mut finding = RiskFinding::new(
        ReviewRuleId::AddIndexOnLargeTable,
        ReviewSeverity::Caution,
        message,
    )
    .with_table(&after_table.stable_id, &table_diff.table_name)
    .with_mitigation(mitigation);
    if new.columns.len() == 1 {
        finding = finding.with_column(new.columns[0].clone());
    }
    findings.push(finding);
}

/// `risk/add-fk-on-existing` — FK added between two tables that both
/// already existed; validation locks the referencing table while every
/// existing row is checked.
fn check_add_fk_on_existing(
    table_diff: &TableDiff,
    fk_diff: &ForeignKeyDiff,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    if fk_diff.change_kind != ChangeKind::Added {
        return;
    }
    // Owner table must exist in `before`; modified_tables guarantees this
    // because newly created tables go to `added_tables` instead.
    if context.find_before_table(&table_diff.table_name).is_none() {
        return;
    }
    let Some(new) = fk_diff.new_value.as_ref() else {
        return;
    };
    // Target table must also exist in `before`. Skip the finding when the
    // FK points at a table that is created in the same migration; an empty
    // table has nothing to validate against. Resolve the FK target on the
    // `after` side (where the FK lives) so unqualified `REFERENCES users`
    // picks up `public.users` first when added in the same migration, then
    // verify the resolved table id existed in `before`.
    let Some(target_id) = resolve_table_id(
        context.after,
        new.to_schema.as_deref(),
        &new.to_table,
        after_table.schema_name.as_deref(),
    ) else {
        return;
    };
    if !context
        .before
        .tables
        .iter()
        .any(|t| t.stable_id == target_id)
    {
        return;
    }

    let label = new
        .name
        .clone()
        .unwrap_or_else(|| format!("({})", new.from_columns.join(",")));
    let dialect = dialect_word(context.dialect);
    let (message, mitigation) = match context.dialect {
        EffectiveDialect::Postgres => (
            format!(
                "New FK {label} on existing table {} ({dialect}). Adding a FK validates all existing rows under SHARE ROW EXCLUSIVE lock.",
                table_diff.table_name,
            ),
            "Use ADD CONSTRAINT ... NOT VALID, then VALIDATE CONSTRAINT in a separate transaction.",
        ),
        EffectiveDialect::Mysql => (
            format!(
                "New FK {label} on existing table {} ({dialect}). FK creation locks the referencing table while every existing row is checked against the parent.",
                table_diff.table_name,
            ),
            "Schedule during low-traffic windows, or stage referencing rows so validation is fast.",
        ),
        EffectiveDialect::Auto | EffectiveDialect::Sqlite => return,
    };

    let related_table_id = resolve_table_id(
        context.after,
        new.to_schema.as_deref(),
        &new.to_table,
        after_table.schema_name.as_deref(),
    );
    let mut finding = RiskFinding::new(
        ReviewRuleId::AddFkOnExisting,
        ReviewSeverity::Caution,
        message,
    )
    .with_table(&after_table.stable_id, &table_diff.table_name)
    .with_mitigation(mitigation);
    if let Some(name) = &new.name {
        finding = finding.with_fk_name(name);
    }
    if new.from_columns.len() == 1 {
        finding = finding.with_column(new.from_columns[0].clone());
    }
    if let Some(id) = related_table_id {
        finding = finding.with_related_table(id);
    }
    findings.push(finding);
}

/// `risk/alter-column-type` — existing column's data type was changed;
/// many type changes rewrite the entire table under an exclusive lock.
//
// The name carries `_lock` to keep it independent from
// `check_type_narrow`, which fires on the same `ColumnDiff::Modified`
// from a different (data-correctness) angle.
fn check_alter_column_type_lock(
    table_diff: &TableDiff,
    column_diff: &ColumnDiff,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    if column_diff.change_kind != ChangeKind::Modified {
        return;
    }
    let (Some(old), Some(new)) = (
        column_diff.old_value.as_ref(),
        column_diff.new_value.as_ref(),
    ) else {
        return;
    };
    if old.data_type.eq_ignore_ascii_case(&new.data_type) {
        return;
    }

    let dialect = dialect_word(context.dialect);
    let (message, mitigation) = match context.dialect {
        EffectiveDialect::Postgres => (
            format!(
                "Column {}.{} changed type from {} to {} ({dialect}). Many type changes rewrite the entire table under ACCESS EXCLUSIVE lock.",
                table_diff.table_name, column_diff.column_name, old.data_type, new.data_type,
            ),
            "Add a new column, backfill, swap, drop the old column; or verify the USING clause is no-rewrite.",
        ),
        EffectiveDialect::Mysql => (
            format!(
                "Column {}.{} changed type from {} to {} ({dialect}). Many type changes fall back to ALGORITHM=COPY and rewrite the entire table.",
                table_diff.table_name, column_diff.column_name, old.data_type, new.data_type,
            ),
            "Verify ALGORITHM=INPLACE, LOCK=NONE applies for this transition (5.6+); otherwise stage with a new-column / backfill / swap.",
        ),
        EffectiveDialect::Auto | EffectiveDialect::Sqlite => return,
    };

    findings.push(
        RiskFinding::new(
            ReviewRuleId::AlterColumnType,
            ReviewSeverity::Caution,
            message,
        )
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_column(&column_diff.column_name)
        .with_mitigation(mitigation),
    );
}

/// `risk/rewrite-table` — schema change forces a full table rebuild on
/// `MySQL` 5.7-compatible engines (PK rotation or existing column drop).
//
// Operates at table-diff granularity rather than per-column /
// per-index, so the dispatcher invokes it once per modified table.
fn check_rewrite_table(
    table_diff: &TableDiff,
    before_table: Option<&Table>,
    after_table: &Table,
    context: &RuleContext<'_>,
    findings: &mut Vec<RiskFinding>,
) {
    // Dispatcher gates this rule to MySQL via dialect_scope, but be
    // explicit so direct callers cannot mis-use the helper.
    if context.dialect != EffectiveDialect::Mysql {
        return;
    }
    // Table must exist in `before`; modified_tables guarantees this and
    // newly created tables flow through `added_tables` instead.
    if before_table.is_none() {
        return;
    }

    let mitigation =
        "Schedule a maintenance window or use a tool such as gh-ost / pt-online-schema-change.";

    // PK rotation: any column whose primary-key flag flipped on Modified.
    let mut pk_rotation_columns: Vec<&str> = Vec::new();
    for diff in &table_diff.column_diffs {
        if diff.change_kind != ChangeKind::Modified {
            continue;
        }
        if let (Some(old), Some(new)) = (diff.old_value.as_ref(), diff.new_value.as_ref())
            && old.primary_key != new.primary_key
        {
            pk_rotation_columns.push(diff.column_name.as_str());
        }
    }

    if !pk_rotation_columns.is_empty() {
        // column_diffs iteration order can be non-deterministic; sort to
        // keep the message stable across runs and golden fixtures.
        pk_rotation_columns.sort_unstable();
        let columns_label = pk_rotation_columns.join(",");
        let mut finding = RiskFinding::new(
            ReviewRuleId::RewriteTable,
            ReviewSeverity::Caution,
            format!(
                "Primary key on {} is being rotated on column(s) ({columns_label}) (mysql). Pre-8.0 MySQL rebuilds the entire table; 8.0+ may still copy under ALGORITHM=COPY.",
                table_diff.table_name,
            ),
        )
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_mitigation(mitigation);
        if pk_rotation_columns.len() == 1 {
            finding = finding.with_column(pk_rotation_columns[0]);
        }
        findings.push(finding);
    }

    // Column drops on an existing table.
    for diff in &table_diff.column_diffs {
        if diff.change_kind != ChangeKind::Removed {
            continue;
        }
        let finding = RiskFinding::new(
            ReviewRuleId::RewriteTable,
            ReviewSeverity::Caution,
            format!(
                "Dropping column {}.{} forces a table rebuild (mysql). Pre-8.0 MySQL copies the full table; 8.0+ supports INSTANT only for trailing columns.",
                table_diff.table_name, diff.column_name,
            ),
        )
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_column(&diff.column_name)
        .with_mitigation(mitigation);
        findings.push(finding);
    }
}

/// Lower-cased dialect label used by lock-risk finding messages.
const fn dialect_word(dialect: EffectiveDialect) -> &'static str {
    match dialect {
        EffectiveDialect::Auto => "auto",
        EffectiveDialect::Postgres => "postgres",
        EffectiveDialect::Mysql => "mysql",
        EffectiveDialect::Sqlite => "sqlite",
    }
}

fn covers(set: &[String], expected: &[String]) -> bool {
    if expected.is_empty() {
        return true;
    }
    let set_lookup: HashSet<&String> = set.iter().collect();
    expected.iter().all(|c| set_lookup.contains(c))
}

fn same_column_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && covers(a, b)
}

fn covered_by_unique_index(table: &Table, expected: &[String]) -> bool {
    if expected.is_empty() {
        return false;
    }
    let mut sorted_expected: Vec<String> =
        expected.iter().map(|c| c.to_ascii_lowercase()).collect();
    sorted_expected.sort();
    table.indexes.iter().any(|idx| {
        if !idx.is_unique {
            return false;
        }
        let mut sorted_cols: Vec<String> =
            idx.columns.iter().map(|c| c.to_ascii_lowercase()).collect();
        sorted_cols.sort();
        sorted_cols.iter().all(|c| sorted_expected.contains(c))
            && sorted_cols.len() >= sorted_expected.len()
    })
}

fn fk_columns_are_indexed(table: &Table, fk_cols: &[String]) -> bool {
    if fk_cols.is_empty() {
        return true;
    }
    let pk_cols: Vec<&String> = table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| &c.name)
        .collect();
    if column_list_has_prefix(
        &pk_cols.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        fk_cols,
    ) {
        return true;
    }
    table.indexes.iter().any(|idx| {
        column_list_has_prefix(
            &idx.columns.iter().map(String::as_str).collect::<Vec<_>>(),
            fk_cols,
        )
    })
}

fn column_list_has_prefix(index_cols: &[&str], fk_cols: &[String]) -> bool {
    if index_cols.len() < fk_cols.len() {
        return false;
    }
    index_cols
        .iter()
        .zip(fk_cols.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Decide whether `fk` (owned by a table in `owner_schema`) references
/// `target`.
///
/// Resolution policy for unqualified FKs (`fk.to_schema = None`):
/// 1. If `owner_schema` is provided and any same-name table exists in
///    that schema, treat the bare name as resolving to the
///    owner-schema homonym only — matching the disambiguation in
///    [`resolve_table_id`].
/// 2. Otherwise (no owner-schema homonym, or owner has no schema),
///    fall back to a unique bare-name match across the whole schema
///    set. This approximates Postgres' `search_path` lookup when the
///    owner schema does not own a homonym.
///
/// Note: this approximation does not model an arbitrary `search_path`
/// chain, so dialect-specific resolution that would point at a
/// different schema cannot be expressed here.
fn fk_targets_table(
    fk: &ForeignKey,
    target: &Table,
    schema: &Schema,
    owner_schema: Option<&str>,
) -> bool {
    let target_qname_lower = target.qualified_name().to_lowercase();
    if let Some(fk_schema) = fk.to_schema.as_deref() {
        let qname = format!(
            "{}.{}",
            fk_schema.to_lowercase(),
            fk.to_table.to_lowercase()
        );
        return qname == target_qname_lower;
    }
    if !fk.to_table.eq_ignore_ascii_case(&target.name) {
        return false;
    }
    if let Some(owner) = owner_schema {
        let owner_has_homonym = schema.tables.iter().any(|t| {
            t.name.eq_ignore_ascii_case(&target.name)
                && t.schema_name
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(owner))
        });
        if owner_has_homonym {
            return target
                .schema_name
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(owner));
        }
    }
    let same_name_tables = schema
        .tables
        .iter()
        .filter(|t| t.name.eq_ignore_ascii_case(&target.name))
        .count();
    same_name_tables == 1
}

/// Resolve a stable id for the table named `table_name` in `schema`.
///
/// `schema_name` carries the FK's explicit schema qualifier when
/// present. When the FK is unqualified, `default_schema` (typically the
/// owner table's schema) is consulted as the search-path head so that
/// `REFERENCES users` from `public.orders` resolves to `public.users`
/// when one exists.
///
/// Resolution policy, in order:
/// 1. Explicit qualifier (`schema_name = Some`): match only that schema.
/// 2. Owner schema preferred (`default_schema = Some`): if a same-name
///    table exists in that schema, return it. Otherwise fall through
///    to (3) so an unqualified FK can still resolve to the only
///    homonym (Postgres `search_path`-style behavior).
/// 3. Bare-name lookup: return the unique match, or None when more
///    than one table shares the name. Mirrors `fk_targets_table`'s
///    disambiguation policy.
fn resolve_table_id(
    schema: &Schema,
    schema_name: Option<&str>,
    table_name: &str,
    default_schema: Option<&str>,
) -> Option<String> {
    if let Some(s) = schema_name {
        return schema
            .tables
            .iter()
            .find(|t| {
                t.name.eq_ignore_ascii_case(table_name)
                    && t.schema_name
                        .as_deref()
                        .is_some_and(|a| a.eq_ignore_ascii_case(s))
            })
            .map(|t| t.stable_id.clone());
    }

    if let Some(s) = default_schema
        && let Some(t) = schema.tables.iter().find(|t| {
            t.name.eq_ignore_ascii_case(table_name)
                && t.schema_name
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(s))
        })
    {
        return Some(t.stable_id.clone());
    }

    let mut matches = schema
        .tables
        .iter()
        .filter(|t| t.name.eq_ignore_ascii_case(table_name));
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first.stable_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff_schemas;
    use crate::model::{
        Column, ColumnId, Index as ModelIndex, ReferentialAction, Schema, Table, TableId,
    };

    fn col(name: &str, dtype: &str, nullable: bool, pk: bool) -> Column {
        Column {
            id: ColumnId(0),
            name: name.into(),
            data_type: dtype.into(),
            nullable,
            is_primary_key: pk,
            comment: None,
            enum_values: None,
        }
    }

    fn fk(
        name: &str,
        from: &[&str],
        to_table: &str,
        to: &[&str],
        on_delete: ReferentialAction,
    ) -> ForeignKey {
        ForeignKey {
            name: Some(name.into()),
            from_columns: from.iter().map(|c| (*c).to_string()).collect(),
            to_schema: None,
            to_table: to_table.into(),
            to_columns: to.iter().map(|c| (*c).to_string()).collect(),
            on_delete,
            on_update: ReferentialAction::NoAction,
        }
    }

    fn index(name: &str, columns: &[&str], unique: bool) -> ModelIndex {
        ModelIndex {
            name: Some(name.into()),
            columns: columns.iter().map(|c| (*c).to_string()).collect(),
            is_unique: unique,
        }
    }

    fn table(
        name: &str,
        columns: Vec<Column>,
        foreign_keys: Vec<ForeignKey>,
        indexes: Vec<ModelIndex>,
    ) -> Table {
        Table {
            id: TableId(name.len() as u64),
            stable_id: name.into(),
            schema_name: None,
            name: name.into(),
            columns,
            foreign_keys,
            indexes,
            primary_key_name: None,
            comment: None,
        }
    }

    fn table_in(
        schema: &str,
        name: &str,
        columns: Vec<Column>,
        foreign_keys: Vec<ForeignKey>,
        indexes: Vec<ModelIndex>,
    ) -> Table {
        Table {
            id: TableId(format!("{schema}.{name}").len() as u64),
            stable_id: format!("{schema}.{name}"),
            schema_name: Some(schema.into()),
            name: name.into(),
            columns,
            foreign_keys,
            indexes,
            primary_key_name: None,
            comment: None,
        }
    }

    fn run_all(before: &Schema, after: &Schema) -> Vec<RiskFinding> {
        let diff = diff_schemas(before, after);
        run_rules(
            &diff,
            before,
            after,
            ReviewRuleId::all_rules(),
            EffectiveDialect::Auto,
        )
    }

    fn run_with_dialect(
        before: &Schema,
        after: &Schema,
        dialect: EffectiveDialect,
    ) -> Vec<RiskFinding> {
        let diff = diff_schemas(before, after);
        run_rules(&diff, before, after, ReviewRuleId::all_rules(), dialect)
    }

    #[test]
    fn drop_column_referenced_breaking_when_fk_remains() {
        let users = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::NoAction,
            )],
            vec![index("orders_user_email_idx", &["user_email"], false)],
        );
        let before = Schema {
            tables: vec![users, orders.clone()],
            ..Default::default()
        };
        let users_after = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let after = Schema {
            tables: vec![users_after, orders],
            ..Default::default()
        };

        let findings = run_all(&before, &after);
        let drop = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::DropColumnReferenced)
            .expect("expected DropColumnReferenced finding");
        assert_eq!(drop.severity, ReviewSeverity::Breaking);
        assert_eq!(drop.column_name.as_deref(), Some("email"));
    }

    #[test]
    fn drop_column_referenced_silenced_when_fk_also_dropped() {
        let users = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders],
            ..Default::default()
        };
        let users_after = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let after = Schema {
            tables: vec![users_after, orders_after],
            ..Default::default()
        };

        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::DropColumnReferenced),
            "FK is dropped in same migration; rule should not fire"
        );
    }

    #[test]
    fn drop_column_referenced_silenced_when_fk_modified_to_other_column() {
        // Same migration: drop users.email, add users.new_email, and
        // retarget orders.user_email_fkey from email to new_email.
        // The Modified FK's old shape (referencing email) is gone, so the
        // drop is safe and the rule should not fire.
        let users = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders],
            ..Default::default()
        };
        let users_after = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("new_email", "TEXT", false, false),
            ],
            vec![],
            vec![index("users_new_email_key", &["new_email"], true)],
        );
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["new_email"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let after = Schema {
            tables: vec![users_after, orders_after],
            ..Default::default()
        };

        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::DropColumnReferenced),
            "FK is retargeted in same migration; drop-column-referenced should not fire, got: {findings:?}"
        );
    }

    #[test]
    fn drop_column_referenced_breaking_when_fk_only_changes_action() {
        // FK is `Modified` (on_delete flips NoAction → Cascade) but still
        // references the same dropped column. The reference shape did not
        // move, so the FK must NOT count as "removed" — drop-column-referenced
        // must still fire on the dangerous drop.
        let users = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders],
            ..Default::default()
        };
        let users_after = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::Cascade,
            )],
            vec![],
        );
        let after = Schema {
            tables: vec![users_after, orders_after],
            ..Default::default()
        };

        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropColumnReferenced),
            "FK still references dropped column; drop-column-referenced must fire, got: {findings:?}"
        );
    }

    #[test]
    fn drop_table_referenced_breaking_when_fk_remains() {
        let users = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders.clone()],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![orders],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let drop = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::DropTableReferenced)
            .expect("expected DropTableReferenced finding");
        assert_eq!(drop.severity, ReviewSeverity::Breaking);
        assert_eq!(drop.table_name.as_deref(), Some("users"));
    }

    #[test]
    fn add_not_null_on_existing_warns() {
        let before = Schema {
            tables: vec![table(
                "orders",
                vec![col("id", "BIGINT", false, true)],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("tenant_id", "BIGINT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::AddNotNullOnExisting)
            .expect("expected AddNotNullOnExisting finding");
        assert_eq!(f.severity, ReviewSeverity::Warning);
        assert_eq!(f.column_name.as_deref(), Some("tenant_id"));
    }

    #[test]
    fn add_not_null_on_new_table_does_not_fire() {
        let before = Schema::default();
        let after = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("tenant_id", "BIGINT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AddNotNullOnExisting)
        );
    }

    #[test]
    fn type_narrow_breaking_for_varchar_shrink() {
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("username", "VARCHAR(100)", true, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("username", "VARCHAR(50)", true, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::TypeNarrow)
            .expect("expected TypeNarrow finding");
        assert_eq!(f.severity, ReviewSeverity::Breaking);
    }

    #[test]
    fn type_narrow_does_not_fire_for_widen() {
        let before = Schema {
            tables: vec![table(
                "users",
                vec![col("username", "VARCHAR(50)", true, false)],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![col("username", "VARCHAR(100)", true, false)],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::TypeNarrow)
        );
    }

    #[test]
    fn drop_pk_breaking_with_referencing_fk() {
        let users_before = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users_before, orders.clone()],
            ..Default::default()
        };
        let users_after = table(
            "users",
            vec![col("id", "BIGINT", false, false)],
            vec![],
            vec![],
        );
        let after = Schema {
            tables: vec![users_after, orders],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::DropPkOrUnique)
            .expect("expected DropPkOrUnique finding");
        assert_eq!(f.severity, ReviewSeverity::Breaking);
    }

    #[test]
    fn drop_unique_index_warns_without_replacement() {
        let users_before = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", false, false),
            ],
            vec![],
            vec![index("users_email_key", &["email"], true)],
        );
        let users_after = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", false, false),
            ],
            vec![],
            vec![],
        );
        let before = Schema {
            tables: vec![users_before],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users_after],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::DropPkOrUnique)
            .expect("expected DropPkOrUnique finding for index");
        assert_eq!(f.severity, ReviewSeverity::Warning);
    }

    #[test]
    fn pk_widened_to_composite_breaks_existing_fk() {
        // Before: users(id PK, tenant_id) — orders.user_id REFERENCES users(id).
        // After: users(id, tenant_id, PRIMARY KEY (id, tenant_id)) — composite PK.
        // The original `(id)` is no longer guaranteed unique, so the existing
        // FK that targets just `(id)` is unsupportable.
        let users_before = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("tenant_id", "BIGINT", false, false),
            ],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let users_after = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("tenant_id", "BIGINT", false, true),
            ],
            vec![],
            vec![],
        );
        let before = Schema {
            tables: vec![users_before, orders.clone()],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users_after, orders],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::DropPkOrUnique)
            .expect("expected DropPkOrUnique finding for PK widening");
        assert_eq!(f.severity, ReviewSeverity::Breaking);
        assert_eq!(f.table_name.as_deref(), Some("users"));
    }

    #[test]
    fn pk_widened_suppressed_when_unique_on_original_columns() {
        // Same as above but after schema retains a UNIQUE index on the
        // original `(id)` column set, so the FK still resolves.
        let users_before = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("tenant_id", "BIGINT", false, false),
            ],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let users_after = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("tenant_id", "BIGINT", false, true),
            ],
            vec![],
            vec![index("users_id_key", &["id"], true)],
        );
        let before = Schema {
            tables: vec![users_before, orders.clone()],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users_after, orders],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            !findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropPkOrUnique),
            "PK widening should be suppressed when a UNIQUE index covers the original PK columns, got: {findings:?}"
        );
    }

    #[test]
    fn drop_unique_index_breaking_when_fk_references_unique_columns() {
        let users_before = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", false, false),
            ],
            vec![],
            vec![index("users_email_key", &["email"], true)],
        );
        let users_after = table(
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", false, false),
            ],
            vec![],
            vec![],
        );
        let orders = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", false, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::NoAction,
            )],
            vec![index("orders_user_email_idx", &["user_email"], false)],
        );
        let before = Schema {
            tables: vec![users_before, orders.clone()],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users_after, orders],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::DropPkOrUnique)
            .expect("expected DropPkOrUnique finding for index drop with referencing FK");
        assert_eq!(f.severity, ReviewSeverity::Breaking);
    }

    #[test]
    fn drop_column_referenced_by_self_fk_target_is_breaking() {
        let categories_before = table(
            "categories",
            vec![
                col("id", "BIGINT", false, true),
                col("code", "TEXT", false, false),
                col("parent_code", "TEXT", true, false),
            ],
            vec![fk(
                "categories_parent_fkey",
                &["parent_code"],
                "categories",
                &["code"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let categories_after = table(
            "categories",
            vec![
                col("id", "BIGINT", false, true),
                col("parent_code", "TEXT", true, false),
            ],
            vec![fk(
                "categories_parent_fkey",
                &["parent_code"],
                "categories",
                &["code"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![categories_before],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![categories_after],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| {
                f.rule_id == ReviewRuleId::DropColumnReferenced
                    && f.column_name.as_deref() == Some("code")
            })
            .expect("expected DropColumnReferenced finding for self-FK target column");
        assert_eq!(f.severity, ReviewSeverity::Breaking);
    }

    #[test]
    fn add_unique_on_existing_warns() {
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("email", "TEXT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("email", "TEXT", false, false),
                ],
                vec![],
                vec![index("users_email_key", &["email"], true)],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::AddUniqueOnExisting)
            .expect("expected AddUniqueOnExisting finding");
        assert_eq!(f.severity, ReviewSeverity::Warning);
    }

    #[test]
    fn add_unique_suppressed_when_column_set_already_pk() {
        // Replacing PRIMARY KEY(id) with UNIQUE(id) introduces no new
        // uniqueness invariant — the PK already guarantees no duplicates,
        // so no row can fail the new constraint.
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("email", "TEXT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, false),
                    col("email", "TEXT", false, false),
                ],
                vec![],
                vec![index("users_id_key", &["id"], true)],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AddUniqueOnExisting),
            "PK->UNIQUE on the same column set must not warn; got {findings:?}"
        );
    }

    #[test]
    fn add_unique_suppressed_when_subset_already_unique() {
        // UNIQUE(id) already exists; adding UNIQUE(id, tenant_id) does
        // not introduce a new invariant because uniqueness on (id)
        // implies uniqueness on (id, tenant_id).
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, false),
                    col("tenant_id", "BIGINT", false, false),
                ],
                vec![],
                vec![index("users_id_key", &["id"], true)],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, false),
                    col("tenant_id", "BIGINT", false, false),
                ],
                vec![],
                vec![
                    index("users_id_key", &["id"], true),
                    index("users_id_tenant_key", &["id", "tenant_id"], true),
                ],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AddUniqueOnExisting),
            "UNIQUE(id, tenant_id) on top of existing UNIQUE(id) must not warn; got {findings:?}"
        );
    }

    #[test]
    fn add_unique_still_warns_when_no_existing_uniqueness_covers_columns() {
        // Existing UNIQUE(id, tenant_id) does NOT imply uniqueness on
        // just (id) alone, so adding UNIQUE(id) can still fail.
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, false),
                    col("tenant_id", "BIGINT", false, false),
                ],
                vec![],
                vec![index("users_id_tenant_key", &["id", "tenant_id"], true)],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, false),
                    col("tenant_id", "BIGINT", false, false),
                ],
                vec![],
                vec![
                    index("users_id_tenant_key", &["id", "tenant_id"], true),
                    index("users_id_key", &["id"], true),
                ],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::AddUniqueOnExisting),
            "narrowing UNIQUE column set must still warn; got {findings:?}"
        );
    }

    #[test]
    fn add_cascade_delete_on_modified_fk_warns() {
        let users = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_before = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![index("orders_user_idx", &["user_id"], false)],
        );
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::Cascade,
            )],
            vec![index("orders_user_idx", &["user_id"], false)],
        );
        let before = Schema {
            tables: vec![users.clone(), orders_before],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users, orders_after],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::AddCascadeDelete)
            .expect("expected AddCascadeDelete finding");
        assert_eq!(f.severity, ReviewSeverity::Warning);
    }

    #[test]
    fn fk_without_index_info_only_for_new_fk() {
        let users = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_before = table(
            "orders",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users.clone(), orders_before],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users, orders_after],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::FkWithoutIndex)
            .expect("expected FkWithoutIndex finding");
        assert_eq!(f.severity, ReviewSeverity::Info);
    }

    #[test]
    fn fk_without_index_silenced_when_indexed() {
        let users = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_before = table(
            "orders",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![index("orders_user_idx", &["user_id"], false)],
        );
        let before = Schema {
            tables: vec![users.clone(), orders_before],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users, orders_after],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::FkWithoutIndex)
        );
    }

    #[test]
    fn add_index_on_large_table_caution_under_postgres() {
        let before = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("user_id", "BIGINT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("user_id", "BIGINT", false, false),
                ],
                vec![],
                vec![index("orders_user_id_idx", &["user_id"], false)],
            )],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::AddIndexOnLargeTable)
            .expect("expected AddIndexOnLargeTable finding");
        assert_eq!(f.severity, ReviewSeverity::Caution);
        assert_eq!(f.table_name.as_deref(), Some("orders"));
        assert_eq!(f.column_name.as_deref(), Some("user_id"));
        assert!(f.message.contains("CREATE INDEX"));
    }

    #[test]
    fn add_index_on_large_table_does_not_fire_for_new_table() {
        let before = Schema::default();
        let after = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("user_id", "BIGINT", false, false),
                ],
                vec![],
                vec![index("orders_user_id_idx", &["user_id"], false)],
            )],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AddIndexOnLargeTable)
        );
    }

    #[test]
    fn add_index_on_large_table_skipped_under_auto_dialect() {
        let before = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("user_id", "BIGINT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("user_id", "BIGINT", false, false),
                ],
                vec![],
                vec![index("orders_user_id_idx", &["user_id"], false)],
            )],
            ..Default::default()
        };
        let findings = run_all(&before, &after);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AddIndexOnLargeTable),
            "Auto dialect must not fire lock-risk rules; got {findings:?}"
        );
    }

    #[test]
    fn add_fk_on_existing_caution_under_postgres() {
        let users = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_before = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![],
            vec![],
        );
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users.clone(), orders_before],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![users, orders_after],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::AddFkOnExisting)
            .expect("expected AddFkOnExisting finding");
        assert_eq!(f.severity, ReviewSeverity::Caution);
        assert_eq!(f.fk_name.as_deref(), Some("orders_user_fkey"));
        assert_eq!(f.related_table_id.as_deref(), Some("users"));
    }

    #[test]
    fn add_fk_on_existing_skipped_when_referencing_table_is_new() {
        let users = table(
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let before = Schema {
            tables: vec![users.clone()],
            ..Default::default()
        };
        let orders_after = table(
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let after = Schema {
            tables: vec![users, orders_after],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AddFkOnExisting),
            "FK on a brand-new table is creation cost, not lock risk; got {findings:?}"
        );
    }

    #[test]
    fn add_fk_on_existing_resolves_unqualified_target_in_owner_schema() {
        // before:  auth.users exists; public.orders has no FK.
        // after:   auth.users + public.users (new) + public.orders with
        //          FK REFERENCES users (unqualified).
        // The unqualified `users` should resolve to `public.users` (the
        // owner's schema), which is freshly added in this migration. The
        // rule must NOT fire because the target is new.
        let auth_users = table_in(
            "auth",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_before = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![],
            vec![],
        );
        let before = Schema {
            tables: vec![auth_users.clone(), orders_before],
            ..Default::default()
        };
        let public_users = table_in(
            "public",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let after = Schema {
            tables: vec![auth_users, public_users, orders_after],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AddFkOnExisting),
            "Unqualified `users` from public.orders must resolve to public.users (new) and skip the rule; got {findings:?}"
        );
        // The cascade-delete / drop-column-referenced rules should also
        // not see auth.users as the related target.
        for f in &findings {
            if let Some(rel) = f.related_table_id.as_deref() {
                assert_ne!(
                    rel, "auth.users",
                    "unqualified FK target must not resolve cross-schema to auth.users; got {f:?}"
                );
            }
        }
    }

    #[test]
    fn drop_column_referenced_silenced_when_all_same_shape_fks_dropped() {
        // before: public.orders has TWO FKs with identical shape
        //         (user_id -> public.users.id), differing only by name.
        // after:  both FKs are dropped, and public.users itself is dropped.
        // No FK survives that references public.users — drop-table-referenced
        // must NOT fire (the survival check accounts for the multiset of
        // same-shape FKs and recognizes both were dropped).
        let users = table_in(
            "public",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let mut fk_a = fk(
            "orders_user_a_fkey",
            &["user_id"],
            "users",
            &["id"],
            ReferentialAction::NoAction,
        );
        fk_a.to_schema = Some("public".into());
        let mut fk_b = fk(
            "orders_user_b_fkey",
            &["user_id"],
            "users",
            &["id"],
            ReferentialAction::NoAction,
        );
        fk_b.to_schema = Some("public".into());
        let orders_before = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk_a, fk_b],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders_before],
            ..Default::default()
        };
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![],
            vec![],
        );
        let after = Schema {
            tables: vec![orders_after],
            ..Default::default()
        };

        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::DropTableReferenced),
            "All same-shape FKs were dropped; drop-table-referenced should not fire, got: {findings:?}"
        );
    }

    #[test]
    fn drop_table_referenced_breaking_when_only_one_of_same_shape_fks_dropped() {
        // before: public.orders has TWO FKs with identical shape
        //         (user_id -> public.users.id) named A and B.
        // after:  FK A is dropped; FK B survives. public.users is dropped.
        // The reference still exists (FK B), so drop-table-referenced must
        // fire — and the survival check must not over-suppress just because
        // *one* FK with that shape exists in after.
        let users = table_in(
            "public",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let mut fk_a = fk(
            "orders_user_a_fkey",
            &["user_id"],
            "users",
            &["id"],
            ReferentialAction::NoAction,
        );
        fk_a.to_schema = Some("public".into());
        let mut fk_b = fk(
            "orders_user_b_fkey",
            &["user_id"],
            "users",
            &["id"],
            ReferentialAction::NoAction,
        );
        fk_b.to_schema = Some("public".into());
        let orders_before = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk_a, fk_b.clone()],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders_before],
            ..Default::default()
        };
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk_b],
            vec![],
        );
        let after = Schema {
            tables: vec![orders_after],
            ..Default::default()
        };

        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropTableReferenced),
            "Surviving FK B still references public.users; drop-table-referenced must fire, got: {findings:?}"
        );
    }

    #[test]
    fn drop_column_referenced_breaking_when_modified_fk_qualifies_fallback_target() {
        // before: auth.users (id, email) + public.orders with FK
        //         REFERENCES users(email) — bare name, resolves to
        //         auth.users via search-path fallback (no public.users).
        // after:  auth.users (id) — email dropped.
        //         public.orders with same FK, now spelled
        //         REFERENCES auth.users(email).
        // The FK Modified: schema went None -> "auth", but the resolved
        // target is the same auth.users in both states. Shape is
        // semantically unchanged, so the FK must NOT be in removed_fks
        // and drop-column-referenced must still fire on the dropped
        // auth.users.email.
        let auth_users = table_in(
            "auth",
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let bare_fk = fk(
            "orders_user_email_fkey",
            &["user_email"],
            "users",
            &["email"],
            ReferentialAction::NoAction,
        );
        let orders_before = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![bare_fk],
            vec![],
        );
        let before = Schema {
            tables: vec![auth_users, orders_before],
            ..Default::default()
        };
        let auth_users_after = table_in(
            "auth",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let mut qualified_fk = fk(
            "orders_user_email_fkey",
            &["user_email"],
            "users",
            &["email"],
            ReferentialAction::NoAction,
        );
        qualified_fk.to_schema = Some("auth".into());
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![qualified_fk],
            vec![],
        );
        let after = Schema {
            tables: vec![auth_users_after, orders_after],
            ..Default::default()
        };

        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropColumnReferenced),
            "FK semantically unchanged after qualifying the fallback target; drop-column-referenced must still fire, got: {findings:?}"
        );
    }

    #[test]
    fn drop_column_referenced_breaking_when_fk_renamed_with_same_shape() {
        // public.orders has FK orders_user_email_fkey REFERENCES
        // public.users(email). The migration renames the constraint
        // (Removed orders_user_email_fkey + Added orders_users_email_fk)
        // and drops public.users.email. The reference still exists in
        // after, so drop-column-referenced must still fire.
        let users = table_in(
            "public",
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let mut old_named_fk = fk(
            "orders_user_email_fkey",
            &["user_email"],
            "users",
            &["email"],
            ReferentialAction::NoAction,
        );
        old_named_fk.to_schema = Some("public".into());
        let orders_before = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![old_named_fk],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders_before],
            ..Default::default()
        };
        let users_after = table_in(
            "public",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let mut renamed_fk = fk(
            "orders_users_email_fk",
            &["user_email"],
            "users",
            &["email"],
            ReferentialAction::NoAction,
        );
        renamed_fk.to_schema = Some("public".into());
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![renamed_fk],
            vec![],
        );
        let after = Schema {
            tables: vec![users_after, orders_after],
            ..Default::default()
        };

        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropColumnReferenced),
            "FK was only renamed; the reference survives in after, drop-column-referenced must fire, got: {findings:?}"
        );
    }

    #[test]
    fn add_fk_on_existing_fires_for_search_path_fallback_target() {
        // before: auth.users + public.orders (no FK to users).
        // after:  auth.users + public.orders with new FK REFERENCES users.
        // No `public.users` exists in either side, so the unqualified FK
        // resolves to `auth.users` via bare-name uniqueness fallback (the
        // search-path-style behavior). Because that target existed in
        // `before`, `add-fk-on-existing` must fire.
        let auth_users = table_in(
            "auth",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_before = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![],
            vec![],
        );
        let before = Schema {
            tables: vec![auth_users.clone(), orders_before],
            ..Default::default()
        };
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
            ],
            vec![fk(
                "orders_user_fkey",
                &["user_id"],
                "users",
                &["id"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let after = Schema {
            tables: vec![auth_users, orders_after],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::AddFkOnExisting),
            "FK targets the only `users` (auth.users) which existed in before; rule must fire, got: {findings:?}"
        );
    }

    #[test]
    fn drop_column_referenced_not_silenced_when_other_homonym_fk_dropped() {
        // public.orders has two FKs referencing homonym tables:
        //   - orders_auth_user_fkey  (user_id)   -> auth.users(id)
        //   - orders_pub_user_fkey   (user_email)-> public.users(email)
        // The first is dropped; the second stays.
        // public.users.email is dropped at the same time.
        // The dropped FK targets auth.users; the remaining FK targets
        // public.users. drop-column-referenced for public.users.email
        // must still fire — the schema-aware RemovedFk key prevents the
        // dropped auth.users FK from masking the surviving public.users FK.
        let auth_users = table_in(
            "auth",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let public_users = table_in(
            "public",
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let mut auth_fk = fk(
            "orders_auth_user_fkey",
            &["user_id"],
            "users",
            &["id"],
            ReferentialAction::NoAction,
        );
        auth_fk.to_schema = Some("auth".into());
        let mut pub_fk = fk(
            "orders_pub_user_fkey",
            &["user_email"],
            "users",
            &["email"],
            ReferentialAction::NoAction,
        );
        pub_fk.to_schema = Some("public".into());
        let orders_before = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
                col("user_email", "TEXT", true, false),
            ],
            vec![auth_fk, pub_fk.clone()],
            vec![],
        );
        let before = Schema {
            tables: vec![auth_users.clone(), public_users, orders_before],
            ..Default::default()
        };
        let public_users_after = table_in(
            "public",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_id", "BIGINT", false, false),
                col("user_email", "TEXT", true, false),
            ],
            vec![pub_fk],
            vec![],
        );
        let after = Schema {
            tables: vec![auth_users, public_users_after, orders_after],
            ..Default::default()
        };

        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropColumnReferenced),
            "Surviving FK to public.users.email must still trigger drop-column-referenced; got: {findings:?}"
        );
    }

    #[test]
    fn drop_column_referenced_breaking_when_modified_fk_only_qualifies_target() {
        // Modified FK where to_schema flips None -> Some(owner_schema)
        // but target table and column pairs are unchanged. The "shape"
        // is semantically identical, so the FK still references the
        // dropped column and drop-column-referenced must fire.
        let users = table_in(
            "public",
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let orders = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![users, orders],
            ..Default::default()
        };
        let users_after = table_in(
            "public",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let mut qualified_fk = fk(
            "orders_user_email_fkey",
            &["user_email"],
            "users",
            &["email"],
            ReferentialAction::NoAction,
        );
        qualified_fk.to_schema = Some("public".into());
        let orders_after = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![qualified_fk],
            vec![],
        );
        let after = Schema {
            tables: vec![users_after, orders_after],
            ..Default::default()
        };

        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropColumnReferenced),
            "Bare->qualified-but-equivalent FK is not a true retarget; drop-column-referenced must still fire, got: {findings:?}"
        );
    }

    #[test]
    fn drop_column_referenced_detects_homonym_target_in_owner_schema() {
        // before: auth.users (id) + public.users (id, email) + public.orders
        //         with FK REFERENCES users(email) — unqualified, owner is
        //         public, so the FK targets public.users.
        // after:  public.users.email is dropped; FK is unchanged.
        // Expectation: drop-column-referenced fires Breaking. The
        // pre-fix bare-name uniqueness check would silence this because
        // `same_name_tables == 2` (auth.users + public.users).
        let auth_users = table_in(
            "auth",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let public_users = table_in(
            "public",
            "users",
            vec![
                col("id", "BIGINT", false, true),
                col("email", "TEXT", true, false),
            ],
            vec![],
            vec![],
        );
        let public_orders = table_in(
            "public",
            "orders",
            vec![
                col("id", "BIGINT", false, true),
                col("user_email", "TEXT", true, false),
            ],
            vec![fk(
                "orders_user_email_fkey",
                &["user_email"],
                "users",
                &["email"],
                ReferentialAction::NoAction,
            )],
            vec![],
        );
        let before = Schema {
            tables: vec![auth_users.clone(), public_users, public_orders.clone()],
            ..Default::default()
        };
        let public_users_after = table_in(
            "public",
            "users",
            vec![col("id", "BIGINT", false, true)],
            vec![],
            vec![],
        );
        let after = Schema {
            tables: vec![auth_users, public_users_after, public_orders],
            ..Default::default()
        };

        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropColumnReferenced),
            "FK from public.orders should resolve to public.users via owner schema; drop-column-referenced must fire, got: {findings:?}"
        );
    }

    #[test]
    fn alter_column_type_caution_under_postgres() {
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("external_id", "VARCHAR(64)", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("external_id", "UUID", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::AlterColumnType)
            .expect("expected AlterColumnType finding");
        assert_eq!(f.severity, ReviewSeverity::Caution);
        assert_eq!(f.column_name.as_deref(), Some("external_id"));
    }

    #[test]
    fn alter_column_type_does_not_fire_for_nullability_only_change() {
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("nickname", "VARCHAR(255)", true, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("nickname", "VARCHAR(255)", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::AlterColumnType),
            "Same-type nullability flip should not raise alter-column-type lock risk; got {findings:?}"
        );
    }

    #[test]
    fn rewrite_table_pk_rotation_caution_under_mysql() {
        let before = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("public_id", "BIGINT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, false),
                    col("public_id", "BIGINT", false, true),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Mysql);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::RewriteTable)
            .expect("expected RewriteTable finding for PK rotation");
        assert_eq!(f.severity, ReviewSeverity::Caution);
        assert!(f.message.contains("Primary key on orders is being rotated"));
        assert!(f.message.contains("(id,public_id)"));
    }

    #[test]
    fn rewrite_table_column_drop_caution_under_mysql() {
        let before = Schema {
            tables: vec![table(
                "users",
                vec![
                    col("id", "BIGINT", false, true),
                    col("legacy_handle", "VARCHAR(64)", true, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "users",
                vec![col("id", "BIGINT", false, true)],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Mysql);
        let f = findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::RewriteTable)
            .expect("expected RewriteTable finding for column drop");
        assert_eq!(f.severity, ReviewSeverity::Caution);
        assert_eq!(f.column_name.as_deref(), Some("legacy_handle"));
    }

    #[test]
    fn rewrite_table_skipped_under_postgres() {
        let before = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, true),
                    col("public_id", "BIGINT", false, false),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let after = Schema {
            tables: vec![table(
                "orders",
                vec![
                    col("id", "BIGINT", false, false),
                    col("public_id", "BIGINT", false, true),
                ],
                vec![],
                vec![],
            )],
            ..Default::default()
        };
        let findings = run_with_dialect(&before, &after, EffectiveDialect::Postgres);
        assert!(
            findings
                .iter()
                .all(|f| f.rule_id != ReviewRuleId::RewriteTable),
            "rewrite-table is MySQL-only; got {findings:?}"
        );
    }
}
