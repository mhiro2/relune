//! Review report embedded in the HTML review: findings with the reasoning
//! and the table definitions a reader needs to judge them.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use relune_core::{
    CheckConstraint, Column, ForeignKey, Index, MigrationScript, RiskFinding, Schema, Table,
};
use relune_render_html::{
    DefinitionDiff, DefinitionLine, LineChange, ReportStatement, ReviewReport, ReviewReportFinding,
};

use crate::result::ReviewResult;

/// Builds the review report of `result` over the aligned schemas it compared.
pub(crate) fn build_review_report(
    result: &ReviewResult,
    before: &Schema,
    after: &Schema,
    script: Option<&MigrationScript>,
    dialect_note: Option<String>,
    coverage_notes: Vec<String>,
) -> ReviewReport {
    let before_tables = tables_by_id(before);
    let after_tables = tables_by_id(after);

    let mut keyed: BTreeMap<String, Vec<KeyedLine>> = BTreeMap::new();
    let mut findings: Vec<&RiskFinding> = result.review.findings.iter().collect();
    // Most severe first; the rules' own order breaks ties.
    findings.sort_by_key(|finding| std::cmp::Reverse(finding.severity));

    let findings = findings
        .into_iter()
        .map(|finding| {
            let focus_lines = finding.table_id.as_ref().map_or_else(Vec::new, |id| {
                let lines = keyed.entry(id.clone()).or_insert_with(|| {
                    definition_diff(
                        before_tables.get(id.as_str()).copied(),
                        after_tables.get(id.as_str()).copied(),
                    )
                });
                focus_lines(finding, lines)
            });
            ReviewReportFinding {
                finding: finding.clone(),
                rule_description: finding.rule_id.description().to_string(),
                focus_lines,
            }
        })
        .collect();

    let statements = script.map_or_else(Vec::new, |script| {
        script
            .statements
            .iter()
            .filter(|statement| {
                result
                    .review
                    .findings
                    .iter()
                    .any(|finding| finding.sources.contains(&statement.location))
            })
            .map(|statement| ReportStatement {
                location: statement.location.clone(),
                sql: statement.sql.clone(),
                in_transaction: statement.transaction.is_some(),
            })
            .collect()
    });
    let migration_note = result.migration.as_ref().map(|migration| {
        format!(
            "Migration SQL: {} file(s), {} statement(s); transactions: {}",
            migration.files.len(),
            migration.statements,
            migration.transaction_mode
        )
    });

    ReviewReport {
        summary: result.review.summary.clone(),
        migration_note,
        statements,
        findings,
        suppressed: result.review.suppressed.len(),
        dialect_note,
        coverage_notes,
        definitions: keyed
            .into_iter()
            .filter(|(_, lines)| !lines.is_empty())
            .map(|(id, lines)| {
                let lines = lines
                    .into_iter()
                    .map(|line| DefinitionLine {
                        change: line.change,
                        text: line.text,
                    })
                    .collect();
                (id, DefinitionDiff { lines })
            })
            .collect(),
    }
}

fn tables_by_id(schema: &Schema) -> HashMap<&str, &Table> {
    schema
        .tables
        .iter()
        .map(|table| (table.stable_id.as_str(), table))
        .collect()
}

/// What a definition line declares, so findings can point at it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LineKey {
    Header,
    Column(String),
    PrimaryKey {
        name: Option<String>,
        columns: Vec<String>,
    },
    ForeignKey {
        name: Option<String>,
        columns: Vec<String>,
    },
    Check {
        name: Option<String>,
    },
    Index {
        name: Option<String>,
        columns: Vec<String>,
    },
}

#[derive(Debug, Clone)]
struct KeyedLine {
    key: LineKey,
    text: String,
    change: LineChange,
}

/// One table's definition as SQL-like lines: the table heading, then
/// columns, primary key, foreign keys, checks, and indexes. Lines carry no
/// separators, so adding a column changes no neighboring line.
fn table_definition(table: &Table) -> Vec<(LineKey, String)> {
    let mut lines = vec![(LineKey::Header, format!("TABLE {}", table.qualified_name()))];
    for column in &table.columns {
        lines.push((
            LineKey::Column(column.name.to_ascii_lowercase()),
            column_line(column),
        ));
    }

    let primary_key: Vec<String> = table
        .columns
        .iter()
        .filter(|column| column.is_primary_key)
        .map(|column| column.name.clone())
        .collect();
    if !primary_key.is_empty() {
        lines.push((
            LineKey::PrimaryKey {
                name: table.primary_key_name.clone(),
                columns: lowercase(&primary_key),
            },
            format!(
                "  {}PRIMARY KEY ({})",
                constraint_prefix(table.primary_key_name.as_deref()),
                primary_key.join(", ")
            ),
        ));
    }
    for fk in &table.foreign_keys {
        lines.push((
            LineKey::ForeignKey {
                name: fk.name.clone(),
                columns: lowercase(&fk.from_columns),
            },
            foreign_key_line(fk),
        ));
    }
    for check in &table.check_constraints {
        lines.push((
            LineKey::Check {
                name: check.name.clone(),
            },
            format!("  {}", check_text(check)),
        ));
    }
    for index in &table.indexes {
        lines.push((
            LineKey::Index {
                name: index.name.clone(),
                columns: index
                    .column_names()
                    .iter()
                    .map(|name| name.to_ascii_lowercase())
                    .collect(),
            },
            index_line(index),
        ));
    }
    lines
}

fn column_line(column: &Column) -> String {
    let mut line = format!("  {} {}", column.name, column.data_type);
    let semantics = &column.semantics;
    if let Some(values) = &column.enum_values {
        let _ = write!(
            line,
            "({})",
            values
                .iter()
                .map(|value| format!("'{value}'"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(identity) = &semantics.identity {
        let mode = if identity.always {
            "ALWAYS"
        } else {
            "BY DEFAULT"
        };
        let _ = write!(line, " GENERATED {mode} AS IDENTITY");
    }
    if semantics.auto_increment {
        line.push_str(" AUTO_INCREMENT");
    }
    if let Some(generated) = &semantics.generated {
        let storage = if generated.stored {
            "STORED"
        } else {
            "VIRTUAL"
        };
        let _ = write!(
            line,
            " GENERATED ALWAYS AS ({}) {storage}",
            generated.expression
        );
    }
    if let Some(collation) = &semantics.collation {
        let _ = write!(line, " COLLATE {collation}");
    }
    if !column.nullable {
        line.push_str(" NOT NULL");
    }
    if let Some(default) = &semantics.default_expression {
        let _ = write!(line, " DEFAULT {default}");
    }
    if let Some(on_update) = &semantics.on_update {
        let _ = write!(line, " ON UPDATE {on_update}");
    }
    for check in &semantics.check_constraints {
        let _ = write!(line, " {}", check_text(check));
    }
    line
}

fn constraint_prefix(name: Option<&str>) -> String {
    name.map_or_else(String::new, |name| format!("CONSTRAINT {name} "))
}

fn check_text(check: &CheckConstraint) -> String {
    format!(
        "{}CHECK ({})",
        constraint_prefix(check.name.as_deref()),
        check.expression
    )
}

fn foreign_key_line(fk: &ForeignKey) -> String {
    let target = fk.to_schema.as_ref().map_or_else(
        || fk.to_table.clone(),
        |schema| format!("{schema}.{}", fk.to_table),
    );
    let mut line = format!(
        "  {}FOREIGN KEY ({}) REFERENCES {target} ({})",
        constraint_prefix(fk.name.as_deref()),
        fk.from_columns.join(", "),
        fk.to_columns.join(", "),
    );
    for (clause, action) in [("DELETE", fk.on_delete), ("UPDATE", fk.on_update)] {
        if action != relune_core::ReferentialAction::NoAction {
            let _ = write!(line, " ON {clause} {action}");
        }
    }
    line
}

fn index_line(index: &Index) -> String {
    let mut line = String::from("  ");
    if index.is_unique {
        line.push_str("UNIQUE ");
    }
    line.push_str("INDEX");
    if let Some(name) = &index.name {
        let _ = write!(line, " {name}");
    }
    if let Some(method) = &index.method {
        let _ = write!(line, " USING {method}");
    }
    let _ = write!(line, " ({})", index.key_labels().join(", "));
    if !index.included_columns.is_empty() {
        let _ = write!(line, " INCLUDE ({})", index.included_columns.join(", "));
    }
    if let Some(predicate) = &index.predicate {
        let _ = write!(line, " WHERE {predicate}");
    }
    line
}

fn lowercase(columns: &[String]) -> Vec<String> {
    columns
        .iter()
        .map(|column| column.to_ascii_lowercase())
        .collect()
}

/// Line diff of a table's definition before and after the change. A table
/// only on one side is entirely removed or added.
fn definition_diff(before: Option<&Table>, after: Option<&Table>) -> Vec<KeyedLine> {
    let old = before.map(table_definition).unwrap_or_default();
    let new = after.map(table_definition).unwrap_or_default();

    // Longest common subsequence over line text; definitions are a few
    // dozen lines, so the quadratic table stays small.
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i].1 == new[j].1 {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let keyed = |(key, text): &(LineKey, String), change| KeyedLine {
        key: key.clone(),
        text: text.clone(),
        change,
    };
    let mut lines = Vec::with_capacity(n.max(m));
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i].1 == new[j].1 {
            lines.push(keyed(&new[j], LineChange::Unchanged));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            lines.push(keyed(&old[i], LineChange::Removed));
            i += 1;
        } else {
            lines.push(keyed(&new[j], LineChange::Added));
            j += 1;
        }
    }
    lines.extend(old[i..].iter().map(|line| keyed(line, LineChange::Removed)));
    lines.extend(new[j..].iter().map(|line| keyed(line, LineChange::Added)));
    lines
}

/// Indexes of the definition lines a finding is about: its columns, and the
/// constraint or key it names. A table-level finding points at the heading.
fn focus_lines(finding: &RiskFinding, lines: &[KeyedLine]) -> Vec<usize> {
    let columns: Vec<String> = finding
        .columns
        .iter()
        .chain(finding.column_name.iter())
        .map(|column| column.to_ascii_lowercase())
        .collect();
    let names: Vec<String> = finding
        .fk_name
        .iter()
        .chain(finding.constraint_name.iter())
        .map(|name| name.to_ascii_lowercase())
        .collect();
    let key_columns = lowercase(&finding.columns);
    let has_name = |name: &Option<String>| {
        name.as_ref()
            .is_some_and(|name| names.contains(&name.to_ascii_lowercase()))
    };
    // A key finding without a constraint name still points at the key over
    // the same columns. `fk_name` does not count: a dropped key's finding
    // names the foreign key that still references it, not the key itself.
    let same_key = |line_columns: &[String]| {
        finding.constraint_name.is_none()
            && !key_columns.is_empty()
            && line_columns.len() == key_columns.len()
            && line_columns
                .iter()
                .all(|column| key_columns.contains(column))
    };

    let mut focus: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| match &line.key {
            LineKey::Header => false,
            LineKey::Column(name) => columns.contains(name),
            LineKey::Check { name } => has_name(name),
            LineKey::PrimaryKey { name, columns }
            | LineKey::ForeignKey { name, columns }
            | LineKey::Index { name, columns } => has_name(name) || same_key(columns),
        })
        .map(|(index, _)| index)
        .collect();
    if focus.is_empty() && columns.is_empty() && names.is_empty() && !lines.is_empty() {
        focus.push(0);
    }
    focus
}

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::{ReviewRuleId, ReviewSeverity};
    use relune_parser_sql::parse_sql_to_schema;

    fn table(sql: &str) -> Table {
        parse_sql_to_schema(sql)
            .expect("fixture parses")
            .tables
            .remove(0)
    }

    fn texts(lines: &[KeyedLine]) -> Vec<(LineChange, &str)> {
        lines
            .iter()
            .map(|line| (line.change, line.text.as_str()))
            .collect()
    }

    #[test]
    fn definition_lists_columns_keys_checks_and_indexes() {
        let users = table(
            "CREATE TABLE app.users (
                id BIGINT PRIMARY KEY,
                org_id INT NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
                email VARCHAR(255) NOT NULL DEFAULT 'x',
                CONSTRAINT email_present CHECK (email <> '')
            );
            CREATE UNIQUE INDEX users_email_key ON app.users (lower(email)) WHERE org_id > 0;",
        );
        let lines: Vec<String> = table_definition(&users)
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(
            lines,
            [
                "TABLE app.users",
                "  id BIGINT NOT NULL",
                "  org_id INT NOT NULL",
                "  email VARCHAR(255) NOT NULL DEFAULT 'x'",
                "  PRIMARY KEY (id)",
                "  FOREIGN KEY (org_id) REFERENCES orgs (id) ON DELETE CASCADE",
                "  CONSTRAINT email_present CHECK (email <> '')",
                "  UNIQUE INDEX users_email_key (lower(email)) WHERE org_id > 0",
            ]
        );
    }

    #[test]
    fn definition_diff_marks_changed_lines_in_place() {
        let before = table("CREATE TABLE users (id INT PRIMARY KEY, email TEXT, name TEXT);");
        let after = table(
            "CREATE TABLE users (id INT PRIMARY KEY, email VARCHAR(120) NOT NULL, name TEXT);",
        );
        let lines = definition_diff(Some(&before), Some(&after));
        assert_eq!(
            texts(&lines),
            [
                (LineChange::Unchanged, "TABLE users"),
                (LineChange::Unchanged, "  id INT NOT NULL"),
                (LineChange::Removed, "  email TEXT"),
                (LineChange::Added, "  email VARCHAR(120) NOT NULL"),
                (LineChange::Unchanged, "  name TEXT"),
                (LineChange::Unchanged, "  PRIMARY KEY (id)"),
            ]
        );

        let dropped = definition_diff(Some(&before), None);
        assert!(
            dropped
                .iter()
                .all(|line| line.change == LineChange::Removed)
        );
    }

    #[test]
    fn focus_points_at_columns_named_keys_and_unnamed_keys() {
        let before =
            table("CREATE TABLE accounts (id INT PRIMARY KEY, tenant_id INT, email TEXT);");
        let after = table(
            "CREATE TABLE accounts (id INT PRIMARY KEY, tenant_id INT, email TEXT);
             CREATE UNIQUE INDEX ON accounts (tenant_id, email);",
        );
        let lines = definition_diff(Some(&before), Some(&after));
        let unique = RiskFinding::new(
            ReviewRuleId::AddUniqueOnExisting,
            ReviewSeverity::Warning,
            "",
        )
        .with_table("accounts", "accounts")
        .with_columns(["tenant_id", "email"]);
        let focused: Vec<&str> = focus_lines(&unique, &lines)
            .into_iter()
            .map(|index| lines[index].text.as_str())
            .collect();
        assert_eq!(
            focused,
            [
                "  tenant_id INT",
                "  email TEXT",
                "  UNIQUE INDEX (tenant_id, email)"
            ]
        );

        let dropped_pk =
            RiskFinding::new(ReviewRuleId::DropPkOrUnique, ReviewSeverity::Breaking, "")
                .with_table("accounts", "accounts")
                .with_columns(["id"])
                .with_fk_name("orders_account_fk");
        let without_pk = table("CREATE TABLE accounts (id INT, tenant_id INT, email TEXT);");
        let pk_lines = definition_diff(Some(&before), Some(&without_pk));
        let focused: Vec<&str> = focus_lines(&dropped_pk, &pk_lines)
            .into_iter()
            .map(|index| pk_lines[index].text.as_str())
            .collect();
        assert!(focused.contains(&"  PRIMARY KEY (id)"), "{focused:?}");

        let table_level = RiskFinding::new(ReviewRuleId::DropTable, ReviewSeverity::Breaking, "")
            .with_table("accounts", "accounts");
        assert_eq!(focus_lines(&table_level, &lines), [0]);
    }
}
