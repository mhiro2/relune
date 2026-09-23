//! Doc use case implementation.

use std::fmt::Write;

use relune_core::{ReferentialAction, Schema};

use crate::error::AppError;
use crate::markdown;
use crate::request::DocRequest;
use crate::result::DocResult;
use crate::schema_input::schema_from_input;

/// Execute a doc generation request.
#[allow(clippy::needless_pass_by_value)] // Owned request matches other usecases and CLI call sites.
pub fn doc(request: DocRequest) -> Result<DocResult, AppError> {
    let (schema, diagnostics) = schema_from_input(&request.input)?;
    let stats = schema.stats();
    let content = format_doc_markdown(&schema);

    Ok(DocResult {
        content,
        diagnostics,
        stats,
    })
}

/// Format a schema as Markdown documentation.
#[must_use]
pub fn format_doc_markdown(schema: &Schema) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Schema Documentation\n");

    write_overview(&mut out, schema);
    write_tables(&mut out, schema);
    write_views(&mut out, schema);
    write_enums(&mut out, schema);

    out
}

fn write_overview(out: &mut String, schema: &Schema) {
    let _ = writeln!(out, "## Overview\n");
    let _ = writeln!(out, "| Item | Count |");
    let _ = writeln!(out, "|------|-------|");
    let _ = writeln!(out, "| Tables | {} |", schema.tables.len());
    if !schema.views.is_empty() {
        let _ = writeln!(out, "| Views | {} |", schema.views.len());
    }
    if !schema.enums.is_empty() {
        let _ = writeln!(out, "| Enums | {} |", schema.enums.len());
    }
    let total_cols: usize = schema.tables.iter().map(|t| t.columns.len()).sum();
    let _ = writeln!(out, "| Columns | {total_cols} |");
    let total_fks: usize = schema.tables.iter().map(|t| t.foreign_keys.len()).sum();
    let _ = writeln!(out, "| Foreign Keys | {total_fks} |");
    let _ = writeln!(out);
}

fn write_tables(out: &mut String, schema: &Schema) {
    if schema.tables.is_empty() {
        return;
    }
    let _ = writeln!(out, "## Tables\n");
    for table in &schema.tables {
        let _ = writeln!(out, "### {}\n", markdown::text(&table.qualified_name()));

        if let Some(ref comment) = table.comment {
            let _ = writeln!(out, "{}\n", markdown::text(comment));
        }

        // Columns
        let _ = writeln!(out, "| Column | Type | Nullable | Key |");
        let _ = writeln!(out, "|--------|------|----------|-----|");
        for col in &table.columns {
            let nullable = if col.nullable { "YES" } else { "NO" };
            let mut keys = Vec::new();
            if col.is_primary_key {
                keys.push("PK");
            }
            if table
                .foreign_keys
                .iter()
                .any(|fk| fk.from_columns.contains(&col.name))
            {
                keys.push("FK");
            }
            let key_str = keys.join(", ");
            let comment_suffix = col
                .comment
                .as_ref()
                .map_or(String::new(), |c| format!(" — {}", markdown::text(c)));
            let _ = writeln!(
                out,
                "| {}{comment_suffix} | {} | {nullable} | {key_str} |",
                markdown::text(&col.name),
                markdown::text(&col.data_type)
            );
        }
        let _ = writeln!(out);

        // Foreign keys
        if !table.foreign_keys.is_empty() {
            let _ = writeln!(out, "**Foreign Keys**\n");
            let _ = writeln!(out, "| Columns | References | On Delete | On Update |");
            let _ = writeln!(out, "|---------|------------|-----------|-----------|");
            for fk in &table.foreign_keys {
                let from = markdown::text(&fk.from_columns.join(", "));
                let target = markdown::text(&format_fk_target(fk));
                let on_delete = format_action(fk.on_delete);
                let on_update = format_action(fk.on_update);
                let _ = writeln!(out, "| {from} | {target} | {on_delete} | {on_update} |");
            }
            let _ = writeln!(out);
        }

        // Indexes
        if !table.indexes.is_empty() {
            let _ = writeln!(out, "**Indexes**\n");
            let _ = writeln!(out, "| Name | Columns | Unique |");
            let _ = writeln!(out, "|------|---------|--------|");
            for idx in &table.indexes {
                let name = markdown::text(idx.name.as_deref().unwrap_or("(unnamed)"));
                let cols = markdown::text(&idx.key_labels().join(", "));
                let unique = if idx.is_unique { "YES" } else { "NO" };
                let _ = writeln!(out, "| {name} | {cols} | {unique} |");
            }
            let _ = writeln!(out);
        }
    }
}

fn write_views(out: &mut String, schema: &Schema) {
    if schema.views.is_empty() {
        return;
    }
    let _ = writeln!(out, "## Views\n");
    for view in &schema.views {
        let _ = writeln!(out, "### {}\n", markdown::text(&view.qualified_name()));

        if !view.columns.is_empty() {
            let _ = writeln!(out, "| Column | Type |");
            let _ = writeln!(out, "|--------|------|");
            for col in &view.columns {
                let _ = writeln!(
                    out,
                    "| {} | {} |",
                    markdown::text(&col.name),
                    markdown::text(&col.data_type)
                );
            }
            let _ = writeln!(out);
        }

        if let Some(ref def) = view.definition {
            let _ = writeln!(out, "<details>\n<summary>Definition</summary>\n");
            let _ = writeln!(out, "{}\n", markdown::fenced_code("sql", def));
            let _ = writeln!(out, "</details>\n");
        }
    }
}

fn write_enums(out: &mut String, schema: &Schema) {
    if schema.enums.is_empty() {
        return;
    }
    let _ = writeln!(out, "## Enums\n");
    for enum_ in &schema.enums {
        let _ = writeln!(out, "### {}\n", markdown::text(&enum_.qualified_name()));
        let _ = writeln!(out, "| Value |");
        let _ = writeln!(out, "|-------|");
        for value in &enum_.values {
            let _ = writeln!(out, "| {} |", markdown::text(value));
        }
        let _ = writeln!(out);
    }
}

fn format_fk_target(fk: &relune_core::ForeignKey) -> String {
    let target_table = relune_core::qualified_identifier(fk.to_schema.as_deref(), &fk.to_table);
    let to_cols = fk.to_columns.join(", ");
    format!("{target_table}({to_cols})")
}

const fn format_action(action: ReferentialAction) -> &'static str {
    match action {
        ReferentialAction::NoAction => "NO ACTION",
        ReferentialAction::Restrict => "RESTRICT",
        ReferentialAction::Cascade => "CASCADE",
        ReferentialAction::SetNull => "SET NULL",
        ReferentialAction::SetDefault => "SET DEFAULT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::DocFormat;

    #[test]
    fn test_doc_basic_table() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL,
                email VARCHAR(255)
            );
        ";

        let request = DocRequest::from_sql(sql);
        let result = doc(request).unwrap();

        assert!(result.content.contains("# Schema Documentation"));
        assert!(result.content.contains("### users"));
        assert!(result.content.contains("| id |"));
        assert!(result.content.contains("PK"));
        assert!(result.content.contains("Tables | 1"));
    }

    #[test]
    fn test_doc_with_foreign_keys() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT REFERENCES users(id)
            );
        ";

        let request = DocRequest::from_sql(sql);
        let result = doc(request).unwrap();

        assert!(result.content.contains("**Foreign Keys**"));
        assert!(result.content.contains("users(id)"));
        assert!(result.content.contains("FK"));
        assert_eq!(result.stats.table_count, 2);
    }

    #[test]
    fn test_doc_with_indexes() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                email VARCHAR(255)
            );
            CREATE UNIQUE INDEX idx_users_email ON users(email);
        ";

        let request = DocRequest::from_sql(sql);
        let result = doc(request).unwrap();

        assert!(result.content.contains("**Indexes**"));
        assert!(result.content.contains("idx_users_email"));
        assert!(result.content.contains("YES"));
    }

    #[test]
    fn test_doc_markdown_format() {
        let sql = "CREATE TABLE test (id INT PRIMARY KEY);";
        let request = DocRequest::from_sql(sql).with_format(DocFormat::Markdown);
        let result = doc(request).unwrap();
        assert!(result.content.starts_with("# Schema Documentation"));
    }

    #[test]
    fn test_doc_empty_schema() {
        let sql = "SELECT 1;";
        let request = DocRequest::from_sql(sql);
        let result = doc(request).unwrap();
        assert!(result.content.contains("Tables | 0"));
    }

    #[test]
    fn test_doc_overview_counts() {
        let sql = r"
            CREATE TABLE a (id INT PRIMARY KEY);
            CREATE TABLE b (id INT PRIMARY KEY, a_id INT REFERENCES a(id));
            CREATE TABLE c (id INT PRIMARY KEY, x TEXT, y TEXT);
        ";

        let request = DocRequest::from_sql(sql);
        let result = doc(request).unwrap();

        assert!(result.content.contains("Tables | 3"));
        assert!(result.content.contains("Columns | 6"));
        assert!(result.content.contains("Foreign Keys | 1"));
    }

    #[test]
    fn test_doc_escapes_user_controlled_markdown() {
        let sql = r#"
            CREATE TABLE "a|b" (
                "col|x" TEXT
            );
            COMMENT ON TABLE "a|b" IS '# heading
<details>';
            COMMENT ON COLUMN "a|b"."col|x" IS 'pipe | here';
            CREATE VIEW v AS SELECT '```' AS fence FROM "a|b";
        "#;

        let result = doc(DocRequest::from_sql(sql)).unwrap();
        let md = &result.content;

        assert!(md.contains("### a\\|b\n"), "{md}");
        assert!(md.contains("\\# heading \\<details\\>\n"), "{md}");
        assert!(md.contains("| col\\|x — pipe \\| here | TEXT |"), "{md}");
        assert!(md.contains("````sql\n"), "{md}");
        assert!(md.contains("\n````\n"), "{md}");
    }
}
