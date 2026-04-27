//! Review rule implementations.
//!
//! Each rule consumes the `SchemaDiff` plus the `before` and `after`
//! schemas and emits zero or more `RiskFinding`s. Rules are intentionally
//! pure: suppression, configuration, and ordering happen in the caller.

use std::collections::HashSet;

use super::{ReviewRuleId, ReviewSeverity, RiskFinding};
use crate::diff::{ChangeKind, ColumnDiff, ForeignKeyDiff, IndexDiff, SchemaDiff, TableDiff};
use crate::model::{ForeignKey, Schema, Table};

/// Runs every rule in `applied_rules` against the diff.
///
/// The caller is responsible for filtering out suppressed rules and
/// suppressing per-table afterwards; this function only enforces "did
/// the user select this rule".
#[must_use]
pub fn run_rules(
    diff: &SchemaDiff,
    before: &Schema,
    after: &Schema,
    applied_rules: &[ReviewRuleId],
) -> Vec<RiskFinding> {
    let context = RuleContext::build(diff, before, after);
    let mut findings = Vec::new();
    let selected: HashSet<ReviewRuleId> = applied_rules.iter().copied().collect();

    for table_diff in &diff.modified_tables {
        let Some(after_table) = context.find_after_table(&table_diff.table_name) else {
            continue;
        };
        let before_table = context.find_before_table(&table_diff.table_name);

        for column_diff in &table_diff.column_diffs {
            if selected.contains(&ReviewRuleId::DropColumnReferenced) {
                check_drop_column_referenced(
                    table_diff,
                    column_diff,
                    before_table,
                    &context,
                    &mut findings,
                );
            }
            if selected.contains(&ReviewRuleId::AddNotNullOnExisting) {
                check_add_not_null_on_existing(
                    table_diff,
                    column_diff,
                    before_table,
                    after_table,
                    &mut findings,
                );
            }
            if selected.contains(&ReviewRuleId::TypeNarrow) {
                check_type_narrow(table_diff, column_diff, after_table, &mut findings);
            }
            if selected.contains(&ReviewRuleId::DropPkOrUnique) {
                check_drop_pk_or_unique_column(
                    table_diff,
                    column_diff,
                    before_table,
                    after_table,
                    &context,
                    &mut findings,
                );
            }
        }

        for index_diff in &table_diff.index_diffs {
            if selected.contains(&ReviewRuleId::DropPkOrUnique) {
                check_drop_pk_or_unique_index(table_diff, index_diff, after_table, &mut findings);
            }
            if selected.contains(&ReviewRuleId::AddUniqueOnExisting) {
                check_add_unique_on_existing(table_diff, index_diff, after_table, &mut findings);
            }
        }

        for fk_diff in &table_diff.fk_diffs {
            if selected.contains(&ReviewRuleId::AddCascadeDelete) {
                check_add_cascade_delete(table_diff, fk_diff, after_table, &context, &mut findings);
            }
            if selected.contains(&ReviewRuleId::FkWithoutIndex) {
                check_fk_without_index(table_diff, fk_diff, after_table, &mut findings);
            }
        }
    }

    if selected.contains(&ReviewRuleId::DropTableReferenced) {
        check_drop_table_referenced(diff, &context, &mut findings);
    }

    findings
}

struct RuleContext<'a> {
    before: &'a Schema,
    after: &'a Schema,
    /// Set of FK names + (table, columns) that are removed by this diff.
    /// Used to decide whether a referenced column / table is being
    /// "intentionally" disconnected at the same time.
    removed_fks: HashSet<RemovedFk>,
    /// Set of table qualified names removed by this diff (lower-cased).
    removed_tables: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RemovedFk {
    /// Lower-cased qualified name of the table that owns the FK.
    table_name: String,
    /// Optional FK constraint name (lower-cased).
    fk_name: Option<String>,
    /// Lower-cased ordered tuple of `(from_column, to_column)` pairs.
    column_pairs: Vec<(String, String)>,
    /// Lower-cased qualified target table name.
    to_table: String,
}

impl<'a> RuleContext<'a> {
    fn build(diff: &SchemaDiff, before: &'a Schema, after: &'a Schema) -> Self {
        let mut removed_fks: HashSet<RemovedFk> = HashSet::new();
        for table_diff in &diff.modified_tables {
            for fk_diff in &table_diff.fk_diffs {
                if fk_diff.change_kind == ChangeKind::Removed
                    && let Some(old) = fk_diff.old_value.as_ref()
                {
                    removed_fks.insert(RemovedFk {
                        table_name: table_diff.table_name.to_lowercase(),
                        fk_name: old.name.as_ref().map(|n| n.to_lowercase()),
                        column_pairs: old
                            .from_columns
                            .iter()
                            .zip(old.to_columns.iter())
                            .map(|(f, t)| (f.to_lowercase(), t.to_lowercase()))
                            .collect(),
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
                        to_table: fk.to_table.to_lowercase(),
                    });
                }
            }
        }

        Self {
            before,
            after,
            removed_fks,
            removed_tables: removed_table_qnames,
        }
    }

    fn find_before_table(&self, qualified_name: &str) -> Option<&'a Table> {
        self.before
            .tables
            .iter()
            .find(|t| t.qualified_name().eq_ignore_ascii_case(qualified_name))
    }

    fn find_after_table(&self, qualified_name: &str) -> Option<&'a Table> {
        self.after
            .tables
            .iter()
            .find(|t| t.qualified_name().eq_ignore_ascii_case(qualified_name))
    }

    /// Returns true if the FK on `table` is removed in this diff (either
    /// directly or because the owning table was dropped).
    fn fk_is_removed(&self, owning_table_qname: &str, fk: &ForeignKey) -> bool {
        let key = RemovedFk {
            table_name: owning_table_qname.to_lowercase(),
            fk_name: fk.name.as_ref().map(|n| n.to_lowercase()),
            column_pairs: fk
                .from_columns
                .iter()
                .zip(fk.to_columns.iter())
                .map(|(f, t)| (f.to_lowercase(), t.to_lowercase()))
                .collect(),
            to_table: fk.to_table.to_lowercase(),
        };
        self.removed_fks.contains(&key)
    }
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
        let related = resolve_table_id(context.before, fk.to_schema.as_deref(), &fk.to_table);
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

    // Incoming FKs across the schema: another table references this column.
    for other_table in &context.before.tables {
        if other_table
            .qualified_name()
            .eq_ignore_ascii_case(&before_qname)
        {
            continue;
        }
        for fk in &other_table.foreign_keys {
            // Resolve target name to detect references to before_table.
            if !fk_targets_table(fk, before_table, context.before) {
                continue;
            }
            if !fk
                .to_columns
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&column_lower))
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
                if !fk_targets_table(fk, removed_table, context.before) {
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
    if covers(&after_pk_cols, &before_pk_cols) {
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
    after_table: &Table,
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

    let label = old
        .name
        .clone()
        .unwrap_or_else(|| format!("({})", old.columns.join(",")));
    findings.push(
        RiskFinding::new(
            ReviewRuleId::DropPkOrUnique,
            ReviewSeverity::Warning,
            format!(
                "UNIQUE index {}.{} is being dropped without replacement. Application logic relying on uniqueness may break.",
                table_diff.table_name, label,
            ),
        )
        .with_table(&after_table.stable_id, &table_diff.table_name)
        .with_mitigation(
            "Add a replacement UNIQUE in the same migration if uniqueness is still required.",
        ),
    );
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
            if !fk_targets_table(fk, before_table, context.before) {
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

    let related_table_id = resolve_table_id(context.after, new.to_schema.as_deref(), &new.to_table);
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

fn covers(set: &[String], expected: &[String]) -> bool {
    if expected.is_empty() {
        return true;
    }
    let set_lookup: HashSet<&String> = set.iter().collect();
    expected.iter().all(|c| set_lookup.contains(c))
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

fn fk_targets_table(fk: &ForeignKey, target: &Table, schema: &Schema) -> bool {
    let target_qname_lower = target.qualified_name().to_lowercase();
    if let Some(fk_schema) = fk.to_schema.as_deref() {
        let qname = format!(
            "{}.{}",
            fk_schema.to_lowercase(),
            fk.to_table.to_lowercase()
        );
        if qname == target_qname_lower {
            return true;
        }
    }
    if fk.to_table.eq_ignore_ascii_case(&target.name) {
        // Consider unqualified FKs that refer to the target table by
        // bare name when no other table in the schema shares that name.
        if fk.to_schema.is_none() {
            let same_name_tables = schema
                .tables
                .iter()
                .filter(|t| t.name.eq_ignore_ascii_case(&target.name))
                .count();
            return same_name_tables == 1;
        }
    }
    false
}

fn resolve_table_id(
    schema: &Schema,
    schema_name: Option<&str>,
    table_name: &str,
) -> Option<String> {
    schema
        .tables
        .iter()
        .find(|t| {
            let name_match = t.name.eq_ignore_ascii_case(table_name);
            let schema_match = match (t.schema_name.as_deref(), schema_name) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (None, None) => true,
                _ => false,
            };
            name_match && (schema_name.is_none() || schema_match)
        })
        .map(|t| t.stable_id.clone())
}
