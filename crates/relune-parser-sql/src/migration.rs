//! Migration SQL → ordered [`MigrationScript`].
//!
//! Statements are split on top-level semicolons from the token stream, so
//! each one keeps its file and line even when the parser cannot read it,
//! and transaction blocks are tracked from `BEGIN` / `COMMIT` in order.
//! Forms the SQL parser misreads or rejects (`DROP INDEX CONCURRENTLY`,
//! `REINDEX`) are classified from their leading keywords instead.

use relune_core::{
    ConstraintKind, Diagnostic, MigrationOperation, MigrationScript, MigrationStatement,
    SourceLocation, SqlDialect, TransactionMode, diagnostic::codes, normalize_identifier,
};
use sqlparser::ast::{
    AlterColumnOperation, AlterTableOperation, Ident, ObjectName, RenameTableNameKind, Statement,
    TableConstraint, TableFactor,
};
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer};

use crate::context::LineOffsets;
use crate::dialect::{dialect_impl, resolve_dialect};
use crate::names::{normalized_stable_id, split_object_name};

/// One migration file, in the order it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFile {
    /// Name shown in source locations, usually the file path.
    pub name: String,
    /// File contents.
    pub sql: String,
}

/// Parsed migration and the diagnostics collected while reading it.
#[derive(Debug, Clone)]
pub struct MigrationParseOutput {
    /// Statements in execution order.
    pub script: MigrationScript,
    /// Warnings for statements that could not be analyzed.
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses migration files, in the given order, into one script.
///
/// `dialect` selects the SQL grammar; `Auto` detects it from all files.
/// Statements that cannot be read are kept as
/// [`MigrationOperation::Unanalyzed`] with a `REVIEW003` warning, so a
/// review never treats them as harmless.
#[must_use]
pub fn parse_migration(
    files: &[MigrationFile],
    dialect: SqlDialect,
    mode: TransactionMode,
) -> MigrationParseOutput {
    let all_sql: String = files.iter().map(|file| file.sql.as_str()).collect();
    let resolved = resolve_dialect(dialect, &all_sql);
    let grammar = dialect_impl(resolved);

    let mut statements = Vec::new();
    let mut diagnostics = Vec::new();
    let mut next_transaction = 0usize;
    let mut open_transaction: Option<usize> = None;

    for file in files {
        let file_transaction = (mode == TransactionMode::PerFile).then(|| {
            next_transaction += 1;
            next_transaction
        });
        for raw in split_statements(grammar.as_ref(), file) {
            let operations = classify(grammar.as_ref(), &raw);
            let transaction = match mode {
                TransactionMode::PerFile => file_transaction,
                TransactionMode::Explicit => {
                    let ends = operations.iter().any(|operation| {
                        matches!(
                            operation,
                            MigrationOperation::Commit | MigrationOperation::Rollback
                        )
                    });
                    let begins = operations
                        .iter()
                        .any(|operation| matches!(operation, MigrationOperation::Begin));
                    // The statement belongs to the transaction it ends.
                    let current = if ends {
                        open_transaction.take()
                    } else {
                        open_transaction
                    };
                    // `BEGIN` inside an open transaction only warns in
                    // PostgreSQL; the transaction and its locks carry on.
                    // `COMMIT AND CHAIN` starts the next one right away.
                    if begins && open_transaction.is_none() {
                        next_transaction += 1;
                        open_transaction = Some(next_transaction);
                    }
                    if ends { current } else { open_transaction }
                }
            };
            let statement = MigrationStatement {
                location: SourceLocation {
                    file: file.name.clone(),
                    line: raw.line,
                    column: raw.column,
                },
                sql: raw.sql,
                transaction,
                operations,
            };
            if let Some(MigrationOperation::Unanalyzed { reason }) = statement
                .operations
                .iter()
                .find(|operation| matches!(operation, MigrationOperation::Unanalyzed { .. }))
            {
                diagnostics.push(
                    Diagnostic::warning(
                        codes::review_migration_unanalyzed(),
                        format!(
                            "Migration statement at {} was not analyzed ({reason}); changes it makes are not reviewed.",
                            statement.location
                        ),
                    )
                    .with_source(&statement.location.file),
                );
            }
            statements.push(statement);
        }
        // A transaction left open at the end of a file does not carry over:
        // migration tools run each file as its own session.
        if mode == TransactionMode::Explicit {
            open_transaction = None;
        }
    }

    MigrationParseOutput {
        script: MigrationScript {
            transaction_mode: mode,
            statements,
        },
        diagnostics,
    }
}

/// Statement text with the line it starts on and its significant tokens.
struct RawStatement {
    line: usize,
    column: usize,
    sql: String,
    tokens: Vec<Token>,
}

/// Splits a file on top-level semicolons. A file the tokenizer rejects is
/// one unanalyzable statement.
fn split_statements(
    grammar: &dyn sqlparser::dialect::Dialect,
    file: &MigrationFile,
) -> Vec<RawStatement> {
    let offsets = LineOffsets::new(&file.sql);
    let tokens = match Tokenizer::new(grammar, &file.sql).tokenize_with_location() {
        Ok(tokens) => tokens,
        Err(error) => {
            return vec![RawStatement {
                line: usize::try_from(error.location.line).unwrap_or(1).max(1),
                column: 1,
                sql: file.sql.trim().to_string(),
                tokens: vec![Token::Word(sqlparser::tokenizer::Word {
                    value: format!("tokenizer error: {}", error.message),
                    quote_style: None,
                    keyword: Keyword::NoKeyword,
                })],
            }];
        }
    };

    let mut statements = Vec::new();
    let mut current: Vec<&TokenWithSpan> = Vec::new();
    for token in tokens
        .iter()
        .chain(std::iter::once(&TokenWithSpan::wrap(Token::SemiColon)))
    {
        if token.token == Token::SemiColon {
            if let (Some(first), Some(last)) = (current.first(), current.last()) {
                let start = offsets.location_to_offset(&file.sql, first.span.start);
                let end = offsets.location_to_offset(&file.sql, last.span.end);
                if let (Some(start), Some(end)) = (start, end) {
                    statements.push(RawStatement {
                        line: usize::try_from(first.span.start.line).unwrap_or(1),
                        column: usize::try_from(first.span.start.column).unwrap_or(1),
                        sql: file.sql[start..end.max(start)].to_string(),
                        tokens: current.iter().map(|token| token.token.clone()).collect(),
                    });
                }
            }
            current.clear();
        } else if !matches!(token.token, Token::Whitespace(_) | Token::EOF) {
            current.push(token);
        }
    }
    statements
}

fn keyword_at(tokens: &[Token], index: usize) -> Option<Keyword> {
    match tokens.get(index) {
        Some(Token::Word(word)) => Some(word.keyword),
        _ => None,
    }
}

fn unanalyzed(reason: impl Into<String>) -> Vec<MigrationOperation> {
    vec![MigrationOperation::Unanalyzed {
        reason: reason.into(),
    }]
}

/// What a statement does, from its leading keywords or its parsed form.
fn classify(
    grammar: &dyn sqlparser::dialect::Dialect,
    raw: &RawStatement,
) -> Vec<MigrationOperation> {
    let tokens = &raw.tokens;
    if let Some(Token::Word(word)) = tokens.first()
        && word.value.starts_with("tokenizer error")
    {
        return unanalyzed(word.value.clone());
    }
    match (keyword_at(tokens, 0), keyword_at(tokens, 1)) {
        // The SQL parser reads `DROP INDEX CONCURRENTLY name` as an index
        // named `CONCURRENTLY`, so the form is taken from the keywords.
        (Some(Keyword::DROP), Some(Keyword::INDEX)) => {
            let concurrently = keyword_at(tokens, 2) == Some(Keyword::CONCURRENTLY);
            let names = tokens
                .iter()
                .skip(if concurrently { 3 } else { 2 })
                .filter_map(|token| match token {
                    Token::Word(word)
                        if !matches!(
                            word.keyword,
                            Keyword::IF | Keyword::EXISTS | Keyword::CASCADE | Keyword::RESTRICT
                        ) =>
                    {
                        Some(normalize_identifier(&word.value))
                    }
                    _ => None,
                })
                .collect();
            return vec![MigrationOperation::DropIndex {
                names,
                concurrently,
            }];
        }
        (Some(Keyword::REINDEX), _) => {
            return vec![MigrationOperation::Reindex {
                concurrently: tokens.iter().any(|token| {
                    matches!(token, Token::Word(word) if word.keyword == Keyword::CONCURRENTLY)
                }),
            }];
        }
        _ => {}
    }

    let statement = Parser::new(grammar)
        .try_with_sql(&raw.sql)
        .and_then(|mut parser| parser.parse_statement());
    match statement {
        Ok(statement) => statement_operations(&statement),
        Err(error) => unanalyzed(format!("SQL parse error: {error}")),
    }
}

fn table_name(name: &ObjectName) -> String {
    let (schema, table) = split_object_name(name);
    normalized_stable_id(schema.as_deref(), &table)
}

fn ident(ident: &Ident) -> String {
    normalize_identifier(&ident.value)
}

fn idents(idents: &[Ident]) -> Vec<String> {
    idents.iter().map(ident).collect()
}

fn index_columns(columns: &[sqlparser::ast::IndexColumn]) -> Vec<String> {
    columns
        .iter()
        .filter_map(|column| match &column.column.expr {
            sqlparser::ast::Expr::Identifier(name) => Some(ident(name)),
            sqlparser::ast::Expr::CompoundIdentifier(parts) => parts.last().map(ident),
            _ => None,
        })
        .collect()
}

#[allow(clippy::too_many_lines)] // One arm per statement form review tracks.
fn statement_operations(statement: &Statement) -> Vec<MigrationOperation> {
    match statement {
        Statement::StartTransaction { statements, .. } if statements.is_empty() => {
            vec![MigrationOperation::Begin]
        }
        Statement::Commit { chain: false, .. } => vec![MigrationOperation::Commit],
        Statement::Commit { chain: true, .. } => {
            vec![MigrationOperation::Commit, MigrationOperation::Begin]
        }
        Statement::Rollback {
            savepoint: None,
            chain,
        } => {
            if *chain {
                vec![MigrationOperation::Rollback, MigrationOperation::Begin]
            } else {
                vec![MigrationOperation::Rollback]
            }
        }
        Statement::CreateIndex(create) => vec![MigrationOperation::CreateIndex {
            table: table_name(&create.table_name),
            name: create.name.as_ref().map(table_name),
            columns: index_columns(&create.columns),
            unique: create.unique,
            concurrently: create.concurrently,
        }],
        Statement::CreateTable(create) => vec![MigrationOperation::CreateTable {
            table: table_name(&create.name),
        }],
        Statement::Drop {
            object_type: sqlparser::ast::ObjectType::Table,
            names,
            ..
        } => vec![MigrationOperation::DropTable {
            tables: names.iter().map(table_name).collect(),
        }],
        Statement::RenameTable(renames) => renames
            .iter()
            .map(|rename| MigrationOperation::RenameTable {
                from: table_name(&rename.old_name),
                to: table_name(&rename.new_name),
            })
            .collect(),
        Statement::Update(update) => match &update.table.relation {
            TableFactor::Table { name, .. } => vec![MigrationOperation::Update {
                table: table_name(name),
            }],
            _ => vec![MigrationOperation::Other],
        },
        Statement::AlterTable(alter) => {
            let table = table_name(&alter.name);
            alter
                .operations
                .iter()
                .map(|operation| alter_operation(&table, operation))
                .collect()
        }
        _ => vec![MigrationOperation::Other],
    }
}

fn alter_operation(table: &str, operation: &AlterTableOperation) -> MigrationOperation {
    let table = table.to_string();
    match operation {
        AlterTableOperation::AddConstraint {
            constraint,
            not_valid,
        } => add_constraint(table, constraint, *not_valid),
        AlterTableOperation::ValidateConstraint { name } => {
            MigrationOperation::ValidateConstraint {
                table,
                name: ident(name),
            }
        }
        AlterTableOperation::DropConstraint { name, .. } => MigrationOperation::DropConstraint {
            table,
            name: ident(name),
        },
        AlterTableOperation::AlterColumn { column_name, op } => match op {
            AlterColumnOperation::SetNotNull => MigrationOperation::SetNotNull {
                table,
                column: ident(column_name),
            },
            AlterColumnOperation::SetDataType { .. } => MigrationOperation::AlterColumnType {
                table,
                column: ident(column_name),
            },
            _ => MigrationOperation::Other,
        },
        AlterTableOperation::AddColumn { column_def, .. } => MigrationOperation::AddColumn {
            table,
            column: ident(&column_def.name),
        },
        AlterTableOperation::DropColumn { column_names, .. } => match column_names.as_slice() {
            [column] => MigrationOperation::DropColumn {
                table,
                column: ident(column),
            },
            _ => MigrationOperation::Unanalyzed {
                reason: "DROP COLUMN with several columns".to_string(),
            },
        },
        AlterTableOperation::RenameColumn {
            old_column_name,
            new_column_name,
        } => MigrationOperation::RenameColumn {
            table,
            from: ident(old_column_name),
            to: ident(new_column_name),
        },
        AlterTableOperation::ChangeColumn {
            old_name, new_name, ..
        } if !old_name.value.eq_ignore_ascii_case(&new_name.value) => {
            MigrationOperation::RenameColumn {
                table,
                from: ident(old_name),
                to: ident(new_name),
            }
        }
        AlterTableOperation::RenameTable { table_name } => {
            let (RenameTableNameKind::As(name) | RenameTableNameKind::To(name)) = table_name;
            // An unqualified new name stays in the table's schema.
            let (_, new_name) = split_object_name(name);
            let (schema, _) = relune_core::migration::split_table_name(&table);
            MigrationOperation::RenameTable {
                from: table.clone(),
                to: normalized_stable_id(schema, &new_name),
            }
        }
        _ => MigrationOperation::Other,
    }
}

fn add_constraint(
    table: String,
    constraint: &TableConstraint,
    not_valid: bool,
) -> MigrationOperation {
    let (name, kind, columns, expression, using_index) = match constraint {
        TableConstraint::ForeignKey(fk) => (
            fk.name.as_ref().map(ident),
            ConstraintKind::ForeignKey,
            idents(&fk.columns),
            None,
            None,
        ),
        TableConstraint::Check(check) => (
            check.name.as_ref().map(ident),
            ConstraintKind::Check,
            Vec::new(),
            Some(check.expr.to_string().to_ascii_lowercase()),
            None,
        ),
        TableConstraint::Unique(unique) => (
            unique.name.as_ref().map(ident),
            ConstraintKind::Unique,
            index_columns(&unique.columns),
            None,
            None,
        ),
        TableConstraint::PrimaryKey(primary) => (
            primary.name.as_ref().map(ident),
            ConstraintKind::PrimaryKey,
            index_columns(&primary.columns),
            None,
            None,
        ),
        TableConstraint::UniqueUsingIndex(using) => (
            using.name.as_ref().map(ident),
            ConstraintKind::Unique,
            Vec::new(),
            None,
            Some(ident(&using.index_name)),
        ),
        TableConstraint::PrimaryKeyUsingIndex(using) => (
            using.name.as_ref().map(ident),
            ConstraintKind::PrimaryKey,
            Vec::new(),
            None,
            Some(ident(&using.index_name)),
        ),
        _ => return MigrationOperation::Other,
    };
    MigrationOperation::AddConstraint {
        table,
        name,
        kind,
        columns,
        expression,
        using_index,
        not_valid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(files: &[(&str, &str)], mode: TransactionMode) -> MigrationParseOutput {
        let files: Vec<MigrationFile> = files
            .iter()
            .map(|(name, sql)| MigrationFile {
                name: (*name).to_string(),
                sql: (*sql).to_string(),
            })
            .collect();
        parse_migration(&files, SqlDialect::Postgres, mode)
    }

    #[test]
    fn statements_keep_their_file_line_and_transaction() {
        let output = parse(
            &[
                (
                    "001.sql",
                    "-- add an index\nCREATE INDEX CONCURRENTLY idx ON orders (user_id);\n\nBEGIN;\nALTER TABLE orders\n  ADD CONSTRAINT fk FOREIGN KEY (user_id) REFERENCES users (id) NOT VALID;\nCOMMIT;\n",
                ),
                ("002.sql", "ALTER TABLE orders VALIDATE CONSTRAINT fk;"),
            ],
            TransactionMode::Explicit,
        );
        let statements = &output.script.statements;
        let lines: Vec<(&str, usize, Option<usize>)> = statements
            .iter()
            .map(|statement| {
                (
                    statement.location.file.as_str(),
                    statement.location.line,
                    statement.transaction,
                )
            })
            .collect();
        assert_eq!(
            lines,
            [
                ("001.sql", 2, None),
                ("001.sql", 4, Some(1)),
                ("001.sql", 5, Some(1)),
                ("001.sql", 7, Some(1)),
                ("002.sql", 1, None),
            ]
        );
        assert_eq!(
            statements[0].sql,
            "CREATE INDEX CONCURRENTLY idx ON orders (user_id)"
        );
        assert_eq!(
            statements[0].operations,
            [MigrationOperation::CreateIndex {
                table: "orders".to_string(),
                name: Some("idx".to_string()),
                columns: vec!["user_id".to_string()],
                unique: false,
                concurrently: true,
            }]
        );
        assert!(matches!(
            &statements[2].operations[0],
            MigrationOperation::AddConstraint {
                kind: ConstraintKind::ForeignKey,
                not_valid: true,
                ..
            }
        ));
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn chained_commits_and_nested_begins_keep_statements_in_a_transaction() {
        let output = parse(
            &[(
                "001.sql",
                "BEGIN; COMMIT AND CHAIN; CREATE INDEX CONCURRENTLY a ON t (x); COMMIT;
                 BEGIN; SELECT 1; BEGIN; SELECT 2; COMMIT; SELECT 3;",
            )],
            TransactionMode::Explicit,
        );
        let transactions: Vec<Option<usize>> = output
            .script
            .statements
            .iter()
            .map(|statement| statement.transaction)
            .collect();
        assert_eq!(
            transactions,
            [
                Some(1),
                Some(1),
                Some(2),
                Some(2),
                Some(3),
                Some(3),
                Some(3),
                Some(3),
                Some(3),
                None
            ]
        );
        let columns: Vec<usize> = output.script.statements[..3]
            .iter()
            .map(|statement| statement.location.column)
            .collect();
        assert_eq!(columns, [1, 8, 26]);
    }

    #[test]
    fn per_file_mode_runs_each_file_in_one_transaction() {
        let output = parse(
            &[
                ("001.sql", "CREATE INDEX CONCURRENTLY a ON t (x); SELECT 1;"),
                ("002.sql", "SELECT 2;"),
            ],
            TransactionMode::PerFile,
        );
        let transactions: Vec<Option<usize>> = output
            .script
            .statements
            .iter()
            .map(|statement| statement.transaction)
            .collect();
        assert_eq!(transactions, [Some(1), Some(1), Some(2)]);
    }

    #[test]
    fn keyword_forms_the_parser_misreads_are_classified() {
        let output = parse(
            &[(
                "001.sql",
                "DROP INDEX CONCURRENTLY IF EXISTS old_idx; REINDEX INDEX CONCURRENTLY idx;",
            )],
            TransactionMode::Explicit,
        );
        assert_eq!(
            output.script.statements[0].operations,
            [MigrationOperation::DropIndex {
                names: vec!["old_idx".to_string()],
                concurrently: true,
            }]
        );
        assert_eq!(
            output.script.statements[1].operations,
            [MigrationOperation::Reindex { concurrently: true }]
        );
    }

    #[test]
    fn alter_table_operations_and_renames_are_recognized() {
        let output = parse(
            &[(
                "001.sql",
                "ALTER TABLE public.users RENAME COLUMN name TO full_name;
                 ALTER TABLE public.users RENAME TO members;
                 ALTER TABLE members ALTER COLUMN email SET NOT NULL;
                 ALTER TABLE members ADD CONSTRAINT email_nn CHECK (email IS NOT NULL) NOT VALID;
                 UPDATE members SET email = '' WHERE email IS NULL;",
            )],
            TransactionMode::Explicit,
        );
        let operations: Vec<&MigrationOperation> = output
            .script
            .statements
            .iter()
            .flat_map(|statement| &statement.operations)
            .collect();
        assert_eq!(
            operations,
            [
                &MigrationOperation::RenameColumn {
                    table: "public.users".to_string(),
                    from: "name".to_string(),
                    to: "full_name".to_string(),
                },
                &MigrationOperation::RenameTable {
                    from: "public.users".to_string(),
                    to: "public.members".to_string(),
                },
                &MigrationOperation::SetNotNull {
                    table: "members".to_string(),
                    column: "email".to_string(),
                },
                &MigrationOperation::AddConstraint {
                    table: "members".to_string(),
                    name: Some("email_nn".to_string()),
                    kind: ConstraintKind::Check,
                    columns: vec![],
                    expression: Some("email is not null".to_string()),
                    using_index: None,
                    not_valid: true,
                },
                &MigrationOperation::Update {
                    table: "members".to_string(),
                },
            ]
        );
    }

    #[test]
    fn unreadable_statements_stay_unanalyzed_with_a_warning() {
        let output = parse(
            &[(
                "001.sql",
                "DO $$ BEGIN PERFORM 1; END $$;\nCREATE INDEX idx ON t (x);",
            )],
            TransactionMode::Explicit,
        );
        assert!(output.script.statements[0].is_unanalyzed());
        assert!(!output.script.statements[1].is_unanalyzed());
        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(
            output.diagnostics[0].code,
            codes::review_migration_unanalyzed()
        );
        assert!(output.diagnostics[0].message.contains("001.sql:1"));
    }
}
