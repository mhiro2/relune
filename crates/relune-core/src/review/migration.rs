//! Review of the migration SQL behind a schema change.
//!
//! The schema rules see only the before and after states. Given the
//! migration statements, this pass points each finding at the statements
//! that make the change, judges on `PostgreSQL` whether the way they run
//! avoids the risk the finding describes, and adds findings that only the
//! statement order shows: renames, `CONCURRENTLY` inside a transaction,
//! `SET NOT NULL` scans, and constraints left `NOT VALID`.

use std::collections::HashSet;

use super::{
    EffectiveDialect, ExecutionNote, ExecutionVerdict, ReviewRuleId, ReviewSeverity, RiskFinding,
};
use crate::migration::{
    ConstraintKind, MigrationOperation, MigrationScript, find_table, names_table,
};
use crate::model::{Schema, Table};

/// Reconciles `findings` with the migration `script` that produced the
/// change from `before` to `after`.
///
/// `before` must already have the script's renames applied (see
/// [`MigrationScript::apply_renames`]). Findings get the statements that
/// make their change as `sources`; on `PostgreSQL` they also get an
/// [`ExecutionNote`], and a finding whose risk the statements avoid is
/// lowered to `info`. Rules in `applied_rules` that read migration SQL
/// add their findings at the end.
pub fn assess_migration(
    findings: &mut Vec<RiskFinding>,
    script: &MigrationScript,
    before: &Schema,
    after: &Schema,
    applied_rules: &[ReviewRuleId],
    dialect: EffectiveDialect,
) {
    let context = Context {
        script,
        before,
        after,
    };
    for finding in findings.iter_mut() {
        context.assess(finding, dialect);
    }

    let active: HashSet<ReviewRuleId> = applied_rules
        .iter()
        .copied()
        .filter(|rule| match rule.dialect_scope() {
            super::DialectScope::Any => true,
            super::DialectScope::OneOf(_) => dialect == EffectiveDialect::Postgres,
        })
        .collect();
    if active.contains(&ReviewRuleId::RenameTable) || active.contains(&ReviewRuleId::RenameColumn) {
        context.renames(&active, findings);
    }
    if active.contains(&ReviewRuleId::ConcurrentlyInTransaction) {
        context.concurrently_in_transaction(findings);
    }
    if active.contains(&ReviewRuleId::SetNotNullScan) {
        context.set_not_null_scans(findings);
    }
    if active.contains(&ReviewRuleId::ConstraintNotValidated) {
        context.constraints_not_validated(findings);
    }
}

struct Context<'a> {
    script: &'a MigrationScript,
    before: &'a Schema,
    after: &'a Schema,
}

const fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn same_columns(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| same(x, y)))
}

fn mitigated(finding: &mut RiskFinding, detail: String) {
    finding.severity = ReviewSeverity::Info;
    finding.execution = Some(ExecutionNote {
        verdict: ExecutionVerdict::Mitigated,
        detail,
    });
}

fn note(finding: &mut RiskFinding, verdict: ExecutionVerdict, detail: String) {
    finding.execution = Some(ExecutionNote { verdict, detail });
}

/// A `CHECK` expression that only says `column IS NOT NULL`.
fn is_not_null_check(expression: &str, column: &str) -> bool {
    let compact: String = expression
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '(' && *ch != ')' && *ch != '"')
        .collect();
    compact == format!("{}isnotnull", column.to_ascii_lowercase())
}

impl<'a> Context<'a> {
    fn location(&self, statement: usize) -> String {
        self.script.statements[statement].location.to_string()
    }

    fn transaction(&self, statement: usize) -> Option<usize> {
        self.script.statements[statement].transaction
    }

    /// The table a finding is about, on either side of the change.
    fn finding_table(&self, finding: &RiskFinding) -> Option<&'a Table> {
        let id = finding.table_id.as_deref()?;
        self.after
            .tables
            .iter()
            .chain(&self.before.tables)
            .find(|table| table.stable_id == id)
    }

    /// Whether a table name written in the SQL refers to `table`, either
    /// directly or under a name the script later renames to it.
    fn names(&self, name: &str, table: &Table) -> bool {
        if names_table(name, table) {
            return true;
        }
        self.script.operations().any(|(_, operation)| {
            matches!(operation, MigrationOperation::RenameTable { from, to }
                if same(from, name) && names_table(to, table))
        })
    }

    /// Statements whose operation matches, in execution order.
    fn find(
        &self,
        matches: impl Fn(&MigrationOperation) -> bool,
    ) -> Vec<(usize, &'a MigrationOperation)> {
        self.script
            .operations()
            .filter(|(_, operation)| matches(operation))
            .collect()
    }

    /// `VALIDATE CONSTRAINT` of `name` on `table` after statement `after`.
    fn validation(&self, table: &Table, name: &str, after: usize) -> Option<usize> {
        self.find(|operation| {
            matches!(operation, MigrationOperation::ValidateConstraint { table: t, name: n }
                if same(n, name) && self.names(t, table))
        })
        .into_iter()
        .map(|(index, _)| index)
        .find(|index| *index > after)
    }

    // One match arm per rule that names its statements; splitting it would
    // only pass the shared matchers around.
    #[allow(clippy::too_many_lines)]
    fn assess(&self, finding: &mut RiskFinding, dialect: EffectiveDialect) {
        let Some(table) = self.finding_table(finding) else {
            return;
        };
        let columns: Vec<String> = if finding.columns.is_empty() {
            finding.column_name.iter().cloned().collect()
        } else {
            finding.columns.clone()
        };
        let names_key = |name: &Option<String>, op_columns: &[String]| {
            let named = finding
                .constraint_name
                .iter()
                .chain(&finding.fk_name)
                .any(|wanted| name.as_deref().is_some_and(|name| same(name, wanted)));
            named || (!columns.is_empty() && same_columns(op_columns, &columns))
        };
        let on_column = |op_table: &str, column: &str| {
            self.names(op_table, table)
                && finding
                    .column_name
                    .as_deref()
                    .is_some_and(|wanted| same(wanted, column))
        };

        let index_builds = || {
            self.find(|operation| match operation {
                MigrationOperation::CreateIndex {
                    table: t,
                    name,
                    columns,
                    ..
                }
                | MigrationOperation::AddConstraint {
                    table: t,
                    name,
                    kind: ConstraintKind::Unique | ConstraintKind::PrimaryKey,
                    columns,
                    ..
                } => self.names(t, table) && names_key(name, columns),
                _ => false,
            })
        };
        let foreign_keys = || {
            self.find(|operation| {
                matches!(operation, MigrationOperation::AddConstraint {
                    table: t, name, kind: ConstraintKind::ForeignKey, columns, ..
                } if self.names(t, table) && names_key(name, columns))
            })
        };

        let statements: Vec<(usize, &MigrationOperation)> = match finding.rule_id {
            ReviewRuleId::DropColumn
            | ReviewRuleId::DropColumnReferenced
            | ReviewRuleId::RewriteTable => self.find(|operation| {
                matches!(operation, MigrationOperation::DropColumn { table: t, column }
                    if on_column(t, column))
            }),
            ReviewRuleId::DropTable | ReviewRuleId::DropTableReferenced => self.find(|operation| {
                matches!(operation, MigrationOperation::DropTable { tables }
                    if tables.iter().any(|name| self.names(name, table)))
            }),
            ReviewRuleId::TypeNarrow | ReviewRuleId::AlterColumnType => self.find(|operation| {
                matches!(operation, MigrationOperation::AlterColumnType { table: t, column }
                    if on_column(t, column))
            }),
            ReviewRuleId::AddNotNullOnExisting => self.find(|operation| match operation {
                MigrationOperation::SetNotNull { table: t, column }
                | MigrationOperation::AddColumn { table: t, column } => on_column(t, column),
                _ => false,
            }),
            ReviewRuleId::AddUniqueOnExisting | ReviewRuleId::AddIndexOnLargeTable => {
                index_builds()
            }
            ReviewRuleId::AddFkOnExisting
            | ReviewRuleId::FkWithoutIndex
            | ReviewRuleId::AddCascadeDelete => foreign_keys(),
            ReviewRuleId::AddCheckOnExisting => self.find(|operation| {
                matches!(operation, MigrationOperation::AddConstraint {
                    table: t, name, kind: ConstraintKind::Check, ..
                } if self.names(t, table)
                    && finding.constraint_name.as_deref().is_none_or(|wanted| {
                        name.as_deref().is_some_and(|name| same(name, wanted))
                    }))
            }),
            ReviewRuleId::DropPkOrUnique => self.find(|operation| match operation {
                MigrationOperation::DropConstraint { table: t, name } => {
                    self.names(t, table)
                        && finding
                            .constraint_name
                            .as_deref()
                            .is_some_and(|wanted| same(wanted, name))
                }
                MigrationOperation::DropIndex { names, .. } => finding
                    .constraint_name
                    .as_deref()
                    .is_some_and(|wanted| names.iter().any(|name| same(name, wanted))),
                MigrationOperation::DropColumn { table: t, column } => on_column(t, column),
                _ => false,
            }),
            _ => Vec::new(),
        };
        let mut sources: Vec<usize> = statements.iter().map(|(index, _)| *index).collect();

        if dialect == EffectiveDialect::Postgres {
            match finding.rule_id {
                ReviewRuleId::AddIndexOnLargeTable => self.index_build(finding, table, &statements),
                ReviewRuleId::AddFkOnExisting => {
                    if let Some(validate) = self.foreign_key(finding, table, &statements) {
                        sources.push(validate);
                    }
                }
                ReviewRuleId::AddCheckOnExisting => {
                    if let Some(validate) = self.check(finding, table, &statements) {
                        sources.push(validate);
                    }
                }
                ReviewRuleId::AddNotNullOnExisting => self.backfill(finding, table, &statements),
                _ => {}
            }
        }

        sources.sort_unstable();
        sources.dedup();
        finding.sources = sources
            .into_iter()
            .map(|index| self.script.statements[index].location.clone())
            .collect();
    }

    fn index_build(
        &self,
        finding: &mut RiskFinding,
        table: &Table,
        statements: &[(usize, &MigrationOperation)],
    ) {
        let Some(&(index, operation)) = statements.last() else {
            note(
                finding,
                ExecutionVerdict::Unverified,
                "No statement in the migration SQL builds this index, so how it is built is unknown."
                    .to_string(),
            );
            return;
        };
        let at = self.location(index);
        match operation {
            MigrationOperation::CreateIndex {
                concurrently: true, ..
            } if self.transaction(index).is_none() => mitigated(
                finding,
                format!(
                    "Built with CREATE INDEX CONCURRENTLY outside a transaction block ({at}), so writes to {} continue during the build. A failed build leaves an INVALID index to drop before retrying.",
                    table.qualified_name()
                ),
            ),
            MigrationOperation::CreateIndex {
                concurrently: true, ..
            } => note(
                finding,
                ExecutionVerdict::Confirmed,
                format!(
                    "CREATE INDEX CONCURRENTLY ({at}) runs inside a transaction block, where PostgreSQL rejects it, so the migration fails (see risk/concurrently-in-transaction)."
                ),
            ),
            MigrationOperation::CreateIndex { .. } => note(
                finding,
                ExecutionVerdict::Confirmed,
                format!(
                    "Built with CREATE INDEX without CONCURRENTLY ({at}), so writes to {} wait until the build finishes.",
                    table.qualified_name()
                ),
            ),
            _ => note(
                finding,
                ExecutionVerdict::Confirmed,
                format!(
                    "Built by ALTER TABLE ... ADD CONSTRAINT ({at}), which blocks reads and writes on {} until the index is built. Build it with CREATE UNIQUE INDEX CONCURRENTLY and attach it with ADD CONSTRAINT ... USING INDEX.",
                    table.qualified_name()
                ),
            ),
        }
    }

    /// Judges how a foreign key is added; returns its validation statement.
    fn foreign_key(
        &self,
        finding: &mut RiskFinding,
        table: &Table,
        statements: &[(usize, &MigrationOperation)],
    ) -> Option<usize> {
        let Some(&(
            index,
            MigrationOperation::AddConstraint {
                name, not_valid, ..
            },
        )) = statements.last()
        else {
            note(
                finding,
                ExecutionVerdict::Unverified,
                "No statement in the migration SQL adds this foreign key, so how it is validated is unknown."
                    .to_string(),
            );
            return None;
        };
        let at = self.location(index);
        if !not_valid {
            note(
                finding,
                ExecutionVerdict::Confirmed,
                format!(
                    "Added without NOT VALID ({at}), so every existing row of {} is checked while the referencing and referenced tables are locked against writes.",
                    table.qualified_name()
                ),
            );
            return None;
        }
        let validate = name
            .as_deref()
            .and_then(|name| self.validation(table, name, index));
        match validate {
            Some(validate)
                if self.transaction(validate).is_some()
                    && self.transaction(validate) == self.transaction(index) =>
            {
                note(
                    finding,
                    ExecutionVerdict::Confirmed,
                    format!(
                        "Added NOT VALID ({at}) but validated in the same transaction ({}), so the lock taken by ADD CONSTRAINT is held while every row is checked. Commit between the two statements.",
                        self.location(validate)
                    ),
                );
            }
            Some(validate) => mitigated(
                finding,
                format!(
                    "Added NOT VALID ({at}) and validated in a separate transaction ({}). VALIDATE CONSTRAINT checks existing rows under a lock that lets writes continue.",
                    self.location(validate)
                ),
            ),
            None => mitigated(
                finding,
                format!(
                    "Added NOT VALID ({at}), so existing rows are not checked until VALIDATE CONSTRAINT runs (see risk/constraint-not-validated)."
                ),
            ),
        }
        validate
    }

    /// Judges how a `CHECK` is added; returns its validation statement.
    fn check(
        &self,
        finding: &mut RiskFinding,
        table: &Table,
        statements: &[(usize, &MigrationOperation)],
    ) -> Option<usize> {
        let &(
            index,
            MigrationOperation::AddConstraint {
                name, not_valid, ..
            },
        ) = statements.last()?
        else {
            return None;
        };
        let at = self.location(index);
        if !not_valid {
            note(
                finding,
                ExecutionVerdict::Confirmed,
                format!(
                    "Added without NOT VALID ({at}), so every existing row is checked while writes are blocked, and one violating row fails the migration."
                ),
            );
            return None;
        }
        let validate = name
            .as_deref()
            .and_then(|name| self.validation(table, name, index));
        match validate {
            Some(validate) => note(
                finding,
                ExecutionVerdict::Confirmed,
                format!(
                    "Added NOT VALID ({at}); VALIDATE CONSTRAINT ({}) checks existing rows and fails if any row violates the condition.",
                    self.location(validate)
                ),
            ),
            None => mitigated(
                finding,
                format!(
                    "Added NOT VALID ({at}) and not validated here, so existing rows cannot fail this migration; they stay unchecked (see risk/constraint-not-validated)."
                ),
            ),
        }
        validate
    }

    /// Judges whether existing NULLs are handled before `SET NOT NULL`.
    fn backfill(
        &self,
        finding: &mut RiskFinding,
        table: &Table,
        statements: &[(usize, &MigrationOperation)],
    ) {
        let Some(&(index, MigrationOperation::SetNotNull { column, .. })) = statements
            .iter()
            .find(|(_, operation)| matches!(operation, MigrationOperation::SetNotNull { .. }))
        else {
            return;
        };
        let at = self.location(index);
        let update = self
            .find(|operation| {
                matches!(operation, MigrationOperation::Update { table: t } if self.names(t, table))
            })
            .into_iter()
            .map(|(update, _)| update)
            .rfind(|update| *update < index);
        match update {
            // An UPDATE may cover only part of the table, so its presence
            // never counts as a finished backfill.
            Some(update) => note(
                finding,
                ExecutionVerdict::Unverified,
                format!(
                    "An UPDATE on {} ({}) runs before SET NOT NULL ({at}); Relune does not check that it fills every NULL in {column}.",
                    table.qualified_name(),
                    self.location(update)
                ),
            ),
            None => note(
                finding,
                ExecutionVerdict::Confirmed,
                format!(
                    "No UPDATE on {} runs before SET NOT NULL ({at}), so any existing NULL in {column} fails the migration.",
                    table.qualified_name()
                ),
            ),
        }
    }

    fn renames(&self, active: &HashSet<ReviewRuleId>, findings: &mut Vec<RiskFinding>) {
        for (index, operation) in self.script.operations() {
            let location = self.script.statements[index].location.clone();
            match operation {
                MigrationOperation::RenameTable { from, to }
                    if active.contains(&ReviewRuleId::RenameTable) =>
                {
                    let Some(table) = find_table(self.after, to) else {
                        continue;
                    };
                    let mut finding = RiskFinding::new(
                        ReviewRuleId::RenameTable,
                        ReviewSeverity::Warning,
                        format!(
                            "Table {from} is renamed to {} ({location}). Its rows are kept, but queries and application code that still use {from} fail.",
                            table.qualified_name()
                        ),
                    )
                    .with_table(&table.stable_id, table.qualified_name())
                    .with_mitigation(
                        "Ship code that uses the new name first, or keep a view under the old name until every reader has moved.",
                    );
                    finding.sources.push(location);
                    findings.push(finding);
                }
                MigrationOperation::RenameColumn {
                    table: name,
                    from,
                    to,
                } if active.contains(&ReviewRuleId::RenameColumn) => {
                    let Some(table) = self
                        .after
                        .tables
                        .iter()
                        .find(|table| self.names(name, table))
                    else {
                        continue;
                    };
                    let qname = table.qualified_name();
                    let mut finding = RiskFinding::new(
                        ReviewRuleId::RenameColumn,
                        ReviewSeverity::Warning,
                        format!(
                            "Column {qname}.{from} is renamed to {to} ({location}). Its data is kept, but queries and application code that still use {from} fail."
                        ),
                    )
                    .with_table(&table.stable_id, &qname)
                    .with_column(to)
                    .with_mitigation(
                        "Ship code that reads the new name first, or keep the old name available (a view or generated column) until every reader has moved.",
                    );
                    finding.sources.push(location);
                    findings.push(finding);
                }
                _ => {}
            }
        }
    }

    fn concurrently_in_transaction(&self, findings: &mut Vec<RiskFinding>) {
        for (index, operation) in self.script.operations() {
            if self.transaction(index).is_none() {
                continue;
            }
            let (label, table) = match operation {
                MigrationOperation::CreateIndex {
                    concurrently: true,
                    table,
                    name,
                    ..
                } => (
                    format!(
                        "CREATE INDEX CONCURRENTLY{}",
                        name.as_deref()
                            .map_or_else(String::new, |name| format!(" {name}"))
                    ),
                    find_table(self.after, table).or_else(|| find_table(self.before, table)),
                ),
                MigrationOperation::DropIndex {
                    concurrently: true,
                    names,
                } => (
                    format!("DROP INDEX CONCURRENTLY {}", names.join(", ")),
                    None,
                ),
                MigrationOperation::Reindex { concurrently: true } => {
                    ("REINDEX CONCURRENTLY".to_string(), None)
                }
                _ => continue,
            };
            let location = self.script.statements[index].location.clone();
            let mut finding = RiskFinding::new(
                ReviewRuleId::ConcurrentlyInTransaction,
                ReviewSeverity::Breaking,
                format!(
                    "{label} runs inside a transaction block ({location}). PostgreSQL cannot run CONCURRENTLY in a transaction, so the migration fails."
                ),
            )
            .with_mitigation(
                "Run the statement outside BEGIN/COMMIT; when the migration tool wraps each file in a transaction, move it to its own file and turn the wrapping off for that file.",
            );
            if let Some(table) = table {
                finding = finding.with_table(&table.stable_id, table.qualified_name());
            }
            finding.sources.push(location);
            findings.push(finding);
        }
    }

    fn set_not_null_scans(&self, findings: &mut Vec<RiskFinding>) {
        for (index, operation) in self.script.operations() {
            let MigrationOperation::SetNotNull {
                table: name,
                column,
            } = operation
            else {
                continue;
            };
            // A table created by the migration holds no rows to scan.
            let Some(existing) = find_table(self.before, name) else {
                continue;
            };
            if self.not_null_proven(existing, column, index) {
                continue;
            }
            let table = find_table(self.after, name).unwrap_or(existing);
            let location = self.script.statements[index].location.clone();
            let qname = table.qualified_name();
            let mut finding = RiskFinding::new(
                ReviewRuleId::SetNotNullScan,
                ReviewSeverity::Caution,
                format!(
                    "SET NOT NULL on {qname}.{column} ({location}) scans every row for NULLs while holding an ACCESS EXCLUSIVE lock that blocks reads and writes."
                ),
            )
            .with_table(&table.stable_id, &qname)
            .with_column(column.clone())
            .with_mitigation(format!(
                "Add CHECK ({column} IS NOT NULL) NOT VALID, VALIDATE CONSTRAINT in a separate transaction, then SET NOT NULL (PostgreSQL 12+ skips the scan) and drop the check."
            ));
            finding.sources.push(location);
            findings.push(finding);
        }
    }

    /// Whether a validated `CHECK (column IS NOT NULL)` exists on `table`
    /// before statement `before`, which lets `SET NOT NULL` skip its scan.
    fn not_null_proven(&self, table: &Table, column: &str, before: usize) -> bool {
        self.find(|operation| {
            matches!(operation, MigrationOperation::AddConstraint {
                table: t, kind: ConstraintKind::Check, expression: Some(expression), ..
            } if self.names(t, table) && is_not_null_check(expression, column))
        })
        .into_iter()
        .any(|(index, operation)| {
            let MigrationOperation::AddConstraint {
                name, not_valid, ..
            } = operation
            else {
                return false;
            };
            index < before
                && (!not_valid
                    || name.as_deref().is_some_and(|name| {
                        self.validation(table, name, index)
                            .is_some_and(|validate| validate < before)
                    }))
        })
    }

    fn constraints_not_validated(&self, findings: &mut Vec<RiskFinding>) {
        for (index, operation) in self.script.operations() {
            let MigrationOperation::AddConstraint {
                table: name,
                name: constraint,
                columns,
                not_valid: true,
                ..
            } = operation
            else {
                continue;
            };
            let Some(table) =
                find_table(self.after, name).or_else(|| find_table(self.before, name))
            else {
                continue;
            };
            if constraint
                .as_deref()
                .is_some_and(|constraint| self.validation(table, constraint, index).is_some())
            {
                continue;
            }
            let location = self.script.statements[index].location.clone();
            let label = constraint.as_deref().unwrap_or("(unnamed)");
            let mut finding = RiskFinding::new(
                ReviewRuleId::ConstraintNotValidated,
                ReviewSeverity::Info,
                format!(
                    "Constraint {label} on {} is added NOT VALID ({location}) and never validated in this migration. Existing rows are not checked until VALIDATE CONSTRAINT runs.",
                    table.qualified_name()
                ),
            )
            .with_table(&table.stable_id, table.qualified_name())
            .with_columns(columns.iter().cloned())
            .with_constraint_name_opt(constraint.as_deref())
            .with_mitigation(
                "Fix rows that violate it, then run VALIDATE CONSTRAINT in a follow-up migration.",
            );
            finding.sources.push(location);
            findings.push(finding);
        }
    }
}
