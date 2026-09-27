//! Property tests for the SQL parser.
//!
//! These check that arbitrary input never panics the parser in any dialect,
//! and that well-formed generated DDL survives `parse → export → JSON →
//! import` without losing tables, columns, keys, or indexes.

use std::fmt::Write as _;

use proptest::prelude::*;
use proptest::sample::select;
use relune_core::export::{SchemaExport, export_schema, import_schema};
use relune_core::{Schema, SqlDialect};
use relune_parser_sql::parse_sql_to_schema_with_diagnostics_and_dialect;

const DIALECTS: [SqlDialect; 4] = [
    SqlDialect::Auto,
    SqlDialect::Postgres,
    SqlDialect::Mysql,
    SqlDialect::Sqlite,
];

/// Tokens that steer random input into the parser's DDL paths instead of
/// failing at the first keyword.
#[rustfmt::skip]
const TOKENS: &[&str] = &[
    "CREATE", "TABLE", "VIEW", "INDEX", "UNIQUE", "TYPE", "AS", "ENUM", "ALTER", "ADD", "DROP",
    "COLUMN", "CONSTRAINT", "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "ON", "DELETE", "UPDATE",
    "CASCADE", "SET", "NULL", "NOT", "DEFAULT", "CHECK", "COMMENT", "IS", "RENAME", "TO", "IF",
    "EXISTS", "OR", "REPLACE", "SELECT", "FROM", "WHERE", "JOIN", "WITH", "GENERATED", "ALWAYS",
    "STORED", "INT", "BIGINT", "TEXT", "VARCHAR(10)", "NUMERIC(10,2)", "ENUM('a','b')", "SERIAL",
    "INTEGER", "ENGINE=InnoDB", "AUTO_INCREMENT", "WITHOUT ROWID", "STRICT", "PARTITION", "OF",
    "LIKE", "INCLUDE", "USING", "btree", "DELIMITER", ";;", "users", "posts", "id", "user_id",
    "public", "public.users", "\"Quoted\"", "`tick`", "[bracket]", "'str'", "''", "(", ")", ",",
    ";", ".", "=", "--", "/*", "*/", "\n", "$$", "é", "テーブル",
];

fn token_soup() -> impl Strategy<Value = String> {
    prop::collection::vec(select(TOKENS), 0..48).prop_map(|tokens| tokens.join(" "))
}

#[derive(Debug, Clone)]
struct GeneratedColumn {
    name: String,
    data_type: &'static str,
    nullable: bool,
}

#[derive(Debug, Clone)]
struct GeneratedTable {
    name: String,
    columns: Vec<GeneratedColumn>,
    /// Index of an earlier table referenced by the first non-key column.
    reference: Option<usize>,
    indexed: bool,
    /// Whether a view selecting the key column is created after the table.
    viewed: bool,
}

#[derive(Debug, Clone)]
struct GeneratedSchema {
    dialect: SqlDialect,
    tables: Vec<GeneratedTable>,
}

const COLUMN_TYPES: &[&str] = &[
    "INT",
    "BIGINT",
    "TEXT",
    "VARCHAR(40)",
    "BOOLEAN",
    "NUMERIC(10,2)",
    "DATE",
];

fn generated_schema() -> impl Strategy<Value = GeneratedSchema> {
    let dialect = select(&[SqlDialect::Postgres, SqlDialect::Mysql, SqlDialect::Sqlite][..]);
    let table = (
        prop::collection::vec((select(COLUMN_TYPES), any::<bool>()), 1..6),
        any::<Option<prop::sample::Index>>(),
        any::<bool>(),
        any::<bool>(),
    );
    (dialect, prop::collection::vec(table, 1..8)).prop_map(|(dialect, tables)| {
        let tables = tables
            .into_iter()
            .enumerate()
            .map(|(table_index, (columns, reference, indexed, viewed))| {
                let columns = std::iter::once(GeneratedColumn {
                    name: "id".to_string(),
                    data_type: "BIGINT",
                    nullable: false,
                })
                .chain(
                    columns
                        .into_iter()
                        .enumerate()
                        .map(|(i, (data_type, nullable))| GeneratedColumn {
                            name: format!("c{i}"),
                            data_type,
                            nullable,
                        }),
                )
                .collect();
                GeneratedTable {
                    name: format!("t{table_index}"),
                    columns,
                    reference: reference
                        .filter(|_| table_index > 0)
                        .map(|index| index.index(table_index)),
                    indexed,
                    viewed,
                }
            })
            .collect();
        GeneratedSchema { dialect, tables }
    })
}

impl GeneratedSchema {
    fn quote(&self, identifier: &str) -> String {
        match self.dialect {
            SqlDialect::Mysql => format!("`{identifier}`"),
            _ => format!("\"{identifier}\""),
        }
    }

    fn to_sql(&self) -> String {
        let mut sql = String::new();
        for table in &self.tables {
            let mut lines: Vec<String> = table
                .columns
                .iter()
                .map(|column| {
                    let null = if column.nullable { "" } else { " NOT NULL" };
                    format!("  {} {}{null}", self.quote(&column.name), column.data_type)
                })
                .collect();
            lines.push(format!("  PRIMARY KEY ({})", self.quote("id")));
            if let (Some(target), Some(column)) = (table.reference, table.columns.get(1)) {
                lines.push(format!(
                    "  FOREIGN KEY ({}) REFERENCES {} ({})",
                    self.quote(&column.name),
                    self.quote(&self.tables[target].name),
                    self.quote("id"),
                ));
            }
            writeln!(
                sql,
                "CREATE TABLE {} (\n{}\n);",
                self.quote(&table.name),
                lines.join(",\n")
            )
            .expect("writing to a String cannot fail");
            if table.indexed
                && let Some(column) = table.columns.last()
            {
                writeln!(
                    sql,
                    "CREATE INDEX {} ON {} ({});",
                    self.quote(&format!("idx_{}_{}", table.name, column.name)),
                    self.quote(&table.name),
                    self.quote(&column.name),
                )
                .expect("writing to a String cannot fail");
            }
            if table.viewed {
                writeln!(
                    sql,
                    "CREATE VIEW {} AS SELECT {} FROM {};",
                    self.quote(&format!("v_{}", table.name)),
                    self.quote("id"),
                    self.quote(&table.name),
                )
                .expect("writing to a String cannot fail");
            }
        }
        sql
    }

    fn assert_matches(&self, schema: &Schema) -> Result<(), TestCaseError> {
        prop_assert_eq!(schema.tables.len(), self.tables.len());
        for (expected, table) in self.tables.iter().zip(&schema.tables) {
            prop_assert_eq!(&table.name, &expected.name);
            prop_assert_eq!(table.columns.len(), expected.columns.len());
            for (expected_column, column) in expected.columns.iter().zip(&table.columns) {
                prop_assert_eq!(&column.name, &expected_column.name);
                prop_assert_eq!(
                    column.data_type.to_ascii_uppercase(),
                    expected_column.data_type
                );
                prop_assert_eq!(column.is_primary_key, expected_column.name == "id");
                prop_assert_eq!(
                    column.nullable,
                    expected_column.nullable && !column.is_primary_key
                );
            }

            let has_reference = expected.reference.is_some() && expected.columns.len() > 1;
            prop_assert_eq!(table.foreign_keys.len(), usize::from(has_reference));
            if let (Some(target), Some(foreign_key)) =
                (expected.reference, table.foreign_keys.first())
            {
                prop_assert_eq!(&foreign_key.to_table, &self.tables[target].name);
                prop_assert_eq!(&foreign_key.from_columns, &vec!["c0".to_string()]);
                prop_assert_eq!(&foreign_key.to_columns, &vec!["id".to_string()]);
            }
            prop_assert_eq!(table.indexes.len(), usize::from(expected.indexed));
        }
        let views: Vec<String> = schema.views.iter().map(|view| view.name.clone()).collect();
        let expected_views: Vec<String> = self
            .tables
            .iter()
            .filter(|table| table.viewed)
            .map(|table| format!("v_{}", table.name))
            .collect();
        prop_assert_eq!(views, expected_views);
        Ok(())
    }
}

/// Serializes the schema to JSON, reads it back, and checks the re-exported
/// form is unchanged.
fn assert_json_round_trip(schema: &Schema) -> Result<(), TestCaseError> {
    let exported = export_schema(schema);
    let json = serde_json::to_string(&exported).expect("schema export serializes");
    let parsed: SchemaExport = serde_json::from_str(&json).expect("schema export deserializes");
    let imported = import_schema(&parsed)
        .map_err(|error| TestCaseError::fail(format!("import failed: {error}")))?;
    prop_assert_eq!(export_schema(&imported), exported);
    Ok(())
}

/// Returns a copy of `sql` with the statement at `repeat` appended again (so
/// duplicate definitions are exercised), then `cut` characters removed at
/// `at` and `insert` spliced in, always on character boundaries.
fn mutate(
    sql: &str,
    repeat: prop::sample::Index,
    at: prop::sample::Index,
    cut: usize,
    insert: &str,
) -> String {
    let statements: Vec<&str> = sql.split_inclusive(";\n").collect();
    let repeated = format!("{sql}{}", statements[repeat.index(statements.len())]);
    let chars: Vec<char> = repeated.chars().collect();
    let start = at.index(chars.len() + 1);
    let end = (start + cut).min(chars.len());
    chars[..start]
        .iter()
        .copied()
        .chain(insert.chars())
        .chain(chars[end..].iter().copied())
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn arbitrary_text_never_panics_and_yields_importable_schemas(input in "\\PC{0,200}") {
        for dialect in DIALECTS {
            let output = parse_sql_to_schema_with_diagnostics_and_dialect(&input, dialect);
            if let Some(schema) = &output.schema {
                assert_json_round_trip(schema)?;
            }
        }
    }

    #[test]
    fn keyword_soup_never_panics_and_yields_importable_schemas(input in token_soup()) {
        for dialect in DIALECTS {
            let output = parse_sql_to_schema_with_diagnostics_and_dialect(&input, dialect);
            if let Some(schema) = &output.schema {
                assert_json_round_trip(schema)?;
            }
        }
    }

    #[test]
    fn mutated_ddl_never_panics_and_yields_importable_schemas(
        generated in generated_schema(),
        repeat in any::<prop::sample::Index>(),
        at in any::<prop::sample::Index>(),
        cut in 0usize..24,
        insert in select(TOKENS),
    ) {
        let input = mutate(&generated.to_sql(), repeat, at, cut, insert);
        for dialect in DIALECTS {
            let output = parse_sql_to_schema_with_diagnostics_and_dialect(&input, dialect);
            if let Some(schema) = &output.schema {
                assert_json_round_trip(schema)?;
            }
        }
    }

    #[test]
    fn generated_ddl_round_trips_through_schema_json(generated in generated_schema()) {
        let sql = generated.to_sql();
        let output = parse_sql_to_schema_with_diagnostics_and_dialect(&sql, generated.dialect);
        prop_assert!(
            !output.has_errors() && !output.has_warnings(),
            "unexpected diagnostics for\n{sql}\n{:?}",
            output.diagnostics
        );
        let schema = output.schema.expect("generated DDL produces a schema");
        generated.assert_matches(&schema)?;
        assert_json_round_trip(&schema)?;
    }
}
