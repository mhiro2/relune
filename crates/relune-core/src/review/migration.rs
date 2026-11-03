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
use crate::migration::{ConstraintKind, MigrationOperation, MigrationScript, find_table};
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

/// SQL text without whitespace, parentheses, or double quotes, lowercased,
/// so differently formatted copies of one expression compare equal.
fn compact(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '(' && *ch != ')' && *ch != '"')
        .flat_map(char::to_lowercase)
        .collect()
}

/// A `CHECK` expression that only says `column IS NOT NULL`.
fn is_not_null_check(expression: &str, column: &str) -> bool {
    compact(expression) == format!("{}isnotnull", column.to_ascii_lowercase())
}

/// What a migration shows about validating a `NOT VALID` constraint.
enum Validation {
    /// Validated by this statement.
    At(usize),
    /// Never validated.
    None,
    /// An unnamed constraint and a validation that may or may not be it.
    Unknown,
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

    /// The name a table written as `name` in statement `at` ends up with,
    /// following the renames that run after it.
    fn final_name(&self, name: &str, at: usize) -> String {
        let mut current = name.to_string();
        for (index, operation) in self.script.operations() {
            if let MigrationOperation::RenameTable { from, to } = operation
                && index > at
                && same(from, &current)
            {
                current.clone_from(to);
            }
        }
        current
    }

    /// The table statement `at` names, in `schema` (whose renames are
    /// already applied). An ambiguous name resolves to nothing.
    fn resolve_in(&self, schema: &'a Schema, name: &str, at: usize) -> Option<&'a Table> {
        find_table(schema, &self.final_name(name, at))
    }

    /// The table statement `at` names, preferring the schema after the change.
    fn resolve(&self, name: &str, at: usize) -> Option<&'a Table> {
        self.resolve_in(self.after, name, at)
            .or_else(|| self.resolve_in(self.before, name, at))
    }

    /// Whether `name` in statement `at` refers to `table`.
    fn names(&self, name: &str, at: usize, table: &Table) -> bool {
        self.resolve(name, at)
            .is_some_and(|resolved| resolved.stable_id == table.stable_id)
    }

    /// Statements whose operation matches, in execution order.
    fn find(
        &self,
        matches: impl Fn(usize, &MigrationOperation) -> bool,
    ) -> Vec<(usize, &'a MigrationOperation)> {
        self.script
            .operations()
            .filter(|(index, operation)| matches(*index, operation))
            .collect()
    }

    /// What is known about the validation of a constraint added `NOT VALID`
    /// by statement `added`: `names` are the names it may have (the written
    /// one, or the one `PostgreSQL` generates for an unnamed foreign key).
    fn validation(&self, table: &Table, names: &[String], added: usize) -> Validation {
        let validations: Vec<(usize, String)> = self
            .find(|index, operation| {
                index > added
                    && matches!(operation, MigrationOperation::ValidateConstraint { table: t, .. }
                        if self.names(t, index, table))
            })
            .into_iter()
            .filter_map(|(index, operation)| match operation {
                MigrationOperation::ValidateConstraint { name, .. } => Some((index, name.clone())),
                _ => None,
            })
            .collect();
        if let Some((index, _)) = validations
            .iter()
            .find(|(_, name)| names.iter().any(|candidate| same(candidate, name)))
        {
            return Validation::At(*index);
        }
        // Without a written name the validated constraint cannot be told
        // apart, so a validation of any constraint on the table may be it.
        if !validations.is_empty() && !self.named(added) {
            return Validation::Unknown;
        }
        Validation::None
    }

    /// `ADD ... CHECK` statements that add the constraint a finding names.
    /// An unnamed finding is paired by the expression its message quotes.
    fn check_statements(
        &self,
        finding: &RiskFinding,
        table: &Table,
    ) -> Vec<(usize, &'a MigrationOperation)> {
        let message = compact(&finding.message);
        self.find(|at, operation| {
            let MigrationOperation::AddConstraint {
                table: t,
                name,
                kind: ConstraintKind::Check,
                expression,
                ..
            } = operation
            else {
                return false;
            };
            if !self.names(t, at, table) {
                return false;
            }
            match (finding.constraint_name.as_deref(), name.as_deref()) {
                (Some(wanted), Some(name)) => same(wanted, name),
                _ => expression
                    .as_deref()
                    .is_some_and(|expression| message.contains(&compact(expression))),
            }
        })
    }

    /// Whether the constraint added by statement `at` has a written name.
    fn named(&self, at: usize) -> bool {
        self.script.statements[at]
            .operations
            .iter()
            .any(|operation| {
                matches!(
                    operation,
                    MigrationOperation::AddConstraint { name: Some(_), .. }
                )
            })
    }

    /// Names a constraint added by `operation` may have.
    fn constraint_names(table: &Table, operation: &MigrationOperation) -> Vec<String> {
        match operation {
            MigrationOperation::AddConstraint {
                name: Some(name), ..
            } => vec![name.clone()],
            MigrationOperation::AddConstraint {
                name: None,
                kind: ConstraintKind::ForeignKey,
                columns,
                ..
            } => vec![format!("{}_{}_fkey", table.name, columns.join("_"))],
            _ => Vec::new(),
        }
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
        let wanted: Vec<&String> = finding
            .constraint_name
            .iter()
            .chain(&finding.fk_name)
            .collect();
        // Two names decide on their own; columns only pair a key when one
        // side has no name.
        let names_key = |name: &Option<String>, op_columns: &[String]| match name {
            Some(name) if !wanted.is_empty() => wanted.iter().any(|wanted| same(name, wanted)),
            _ => !columns.is_empty() && same_columns(op_columns, &columns),
        };
        let on_column = |op_table: &str, at: usize, column: &str| {
            self.names(op_table, at, table)
                && finding
                    .column_name
                    .as_deref()
                    .is_some_and(|wanted| same(wanted, column))
        };

        let index_builds = || {
            self.find(|at, operation| match operation {
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
                } => self.names(t, at, table) && names_key(name, columns),
                _ => false,
            })
        };
        let foreign_keys = || {
            self.find(|at, operation| {
                matches!(operation, MigrationOperation::AddConstraint {
                    table: t, name, kind: ConstraintKind::ForeignKey, columns, ..
                } if self.names(t, at, table) && names_key(name, columns))
            })
        };

        let statements: Vec<(usize, &MigrationOperation)> = match finding.rule_id {
            ReviewRuleId::DropColumn
            | ReviewRuleId::DropColumnReferenced
            | ReviewRuleId::RewriteTable => self.find(|at, operation| {
                matches!(operation, MigrationOperation::DropColumn { table: t, column }
                    if on_column(t, at, column))
            }),
            ReviewRuleId::DropTable | ReviewRuleId::DropTableReferenced => {
                self.find(|at, operation| {
                    matches!(operation, MigrationOperation::DropTable { tables }
                        if tables.iter().any(|name| self.names(name, at, table)))
                })
            }
            ReviewRuleId::TypeNarrow | ReviewRuleId::AlterColumnType => {
                self.find(|at, operation| {
                    matches!(operation, MigrationOperation::AlterColumnType { table: t, column }
                        if on_column(t, at, column))
                })
            }
            ReviewRuleId::AddNotNullOnExisting => self.find(|at, operation| match operation {
                MigrationOperation::SetNotNull { table: t, column }
                | MigrationOperation::AddColumn { table: t, column } => on_column(t, at, column),
                _ => false,
            }),
            ReviewRuleId::AddUniqueOnExisting | ReviewRuleId::AddIndexOnLargeTable => {
                index_builds()
            }
            ReviewRuleId::AddFkOnExisting
            | ReviewRuleId::FkWithoutIndex
            | ReviewRuleId::AddCascadeDelete => foreign_keys(),
            ReviewRuleId::AddCheckOnExisting => self.check_statements(finding, table),
            ReviewRuleId::DropPkOrUnique => self.find(|at, operation| match operation {
                MigrationOperation::DropConstraint { table: t, name } => {
                    self.names(t, at, table)
                        && finding
                            .constraint_name
                            .as_deref()
                            .is_some_and(|wanted| same(wanted, name))
                }
                MigrationOperation::DropIndex { names, .. } => finding
                    .constraint_name
                    .as_deref()
                    .is_some_and(|wanted| names.iter().any(|name| same(name, wanted))),
                MigrationOperation::DropColumn { table: t, column } => on_column(t, at, column),
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
        let Some(&(mut index, mut operation)) = statements.last() else {
            note(
                finding,
                ExecutionVerdict::Unverified,
                "No statement in the migration SQL builds this index, so how it is built is unknown."
                    .to_string(),
            );
            return;
        };
        // `ADD CONSTRAINT ... USING INDEX` attaches an index built earlier;
        // that build decides the lock.
        if let MigrationOperation::AddConstraint {
            using_index: Some(used),
            ..
        } = operation
        {
            let Some(&(build_at, build)) = self
                .find(|at, candidate| {
                    at < index
                        && matches!(candidate, MigrationOperation::CreateIndex { name: Some(name), table: t, .. }
                            if same(name, used) && self.names(t, at, table))
                })
                .last()
            else {
                note(
                    finding,
                    ExecutionVerdict::Unverified,
                    format!(
                        "ADD CONSTRAINT ... USING INDEX {used} ({}) attaches an index no statement here builds, so how it was built is unknown.",
                        self.location(index)
                    ),
                );
                return;
            };
            index = build_at;
            operation = build;
        }
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
        let Some(&(index, MigrationOperation::AddConstraint { not_valid, .. })) = statements.last()
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
        let names = Self::constraint_names(table, statements.last()?.1);
        let validate = match self.validation(table, &names, index) {
            Validation::At(validate) => Some(validate),
            Validation::None => None,
            Validation::Unknown => {
                note(
                    finding,
                    ExecutionVerdict::Unverified,
                    format!(
                        "Added NOT VALID ({at}) without a name, and a later VALIDATE CONSTRAINT on {} may validate it, so whether existing rows are checked under the same lock is unknown. Name the constraint.",
                        table.qualified_name()
                    ),
                );
                return None;
            }
        };
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
        if statements.len() > 1 {
            note(
                finding,
                ExecutionVerdict::Unverified,
                format!(
                    "Several statements may add this CHECK to {}, so which one does, and how, is unknown.",
                    table.qualified_name()
                ),
            );
            return None;
        }
        let &(index, operation @ MigrationOperation::AddConstraint { not_valid, .. }) =
            statements.last()?
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
        let validate = match self.validation(
            table,
            &Self::constraint_names(table, operation),
            index,
        ) {
            Validation::At(validate) => Some(validate),
            Validation::None => None,
            Validation::Unknown => {
                note(
                    finding,
                    ExecutionVerdict::Unverified,
                    format!(
                        "Added NOT VALID ({at}) without a name, and a later VALIDATE CONSTRAINT on {} may validate it, so whether existing rows are checked is unknown. Name the constraint.",
                        table.qualified_name()
                    ),
                );
                return None;
            }
        };
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
            .find(|at, operation| {
                matches!(operation, MigrationOperation::Update { table: t } if self.names(t, at, table))
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
                    let Some(table) = self.resolve_in(self.after, to, index) else {
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
                    let Some(table) = self.resolve_in(self.after, name, index) else {
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
                    self.resolve(table, index),
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
            let Some(existing) = self.resolve_in(self.before, name, index) else {
                continue;
            };
            if self.not_null_proven(existing, column, index) {
                continue;
            }
            let table = self.resolve_in(self.after, name, index).unwrap_or(existing);
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

    /// Whether a validated `CHECK (column IS NOT NULL)` is in place on
    /// `table` when statement `before` runs, which lets `SET NOT NULL` skip
    /// its scan. A check dropped first, or by that same statement, no
    /// longer proves anything.
    fn not_null_proven(&self, table: &Table, column: &str, before: usize) -> bool {
        self.find(|at, operation| {
            at < before
                && matches!(operation, MigrationOperation::AddConstraint {
                    table: t, kind: ConstraintKind::Check, expression: Some(expression), ..
                } if self.names(t, at, table) && is_not_null_check(expression, column))
        })
        .into_iter()
        .any(|(index, operation)| {
            let MigrationOperation::AddConstraint {
                name, not_valid, ..
            } = operation
            else {
                return false;
            };
            let validated = !not_valid
                || matches!(
                    self.validation(table, &Self::constraint_names(table, operation), index),
                    Validation::At(validate) if validate < before
                );
            let dropped = name.as_deref().is_some_and(|name| {
                !self
                    .find(|at, candidate| {
                        at > index
                            && at <= before
                            && matches!(candidate, MigrationOperation::DropConstraint { table: t, name: dropped }
                                if same(dropped, name) && self.names(t, at, table))
                    })
                    .is_empty()
            });
            validated && !dropped
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
            let Some(table) = self.resolve(name, index) else {
                continue;
            };
            if !matches!(
                self.validation(table, &Self::constraint_names(table, operation), index),
                Validation::None
            ) {
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
