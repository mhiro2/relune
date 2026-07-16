use super::*;
use crate::context::ParseContext;
use crate::diagnostics::{MAX_UNSUPPORTED_DEBUG_LEN, truncate_unsupported_debug};
use crate::mysql_enum::{
    MySqlEnumLikeParseError, parse_mysql_enum_like_type, populate_inline_enum_columns,
};
use proptest::prelude::*;
use relune_core::{Column, ColumnId, ReferentialAction, Table, TableId};
use relune_testkit::read_sql_fixture;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Location, Span as SqlSpan};

fn snapshot_data(output: &ParseOutput) -> serde_json::Value {
    serde_json::json!({
        "dialect": output.dialect,
        "schema": output.schema,
        "diagnostics": output.diagnostics.iter().map(|d| serde_json::json!({
            "severity": format!("{}", d.severity),
            "code": d.code.full_code(),
            "message": d.message,
        })).collect::<Vec<_>>(),
    })
}

#[test]
fn parses_primary_keys_and_foreign_keys() {
    let sql = r"
    CREATE TABLE public.users (
      id BIGINT PRIMARY KEY,
      name TEXT NOT NULL
    );

    CREATE TABLE posts (
      id BIGINT PRIMARY KEY,
      user_id BIGINT NOT NULL REFERENCES public.users(id),
      title TEXT NOT NULL,
      CONSTRAINT fk_posts_user FOREIGN KEY (user_id) REFERENCES public.users(id)
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 2);

    let users = &schema.tables[0];
    assert_eq!(users.stable_id, "public.users");
    assert!(users.columns[0].is_primary_key);
    assert_eq!(users.columns[0].name, "id");

    let posts = &schema.tables[1];
    assert!(posts.columns[0].is_primary_key);
    // Should have two foreign keys: one inline, one table-level
    assert_eq!(posts.foreign_keys.len(), 2);
}

#[test]
fn captures_column_and_table_semantics() {
    let sql = r"
    CREATE TABLE accounts (
      id BIGINT PRIMARY KEY,
      enabled BOOLEAN NOT NULL DEFAULT false,
      amount INTEGER NOT NULL CHECK (amount >= 0),
      full_name TEXT GENERATED ALWAYS AS (id) STORED,
      CONSTRAINT amount_and_id CHECK (amount < id)
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let accounts = &schema.tables[0];

    let enabled = accounts
        .columns
        .iter()
        .find(|c| c.name == "enabled")
        .unwrap();
    assert_eq!(
        enabled.semantics.default_expression.as_deref(),
        Some("false")
    );

    let amount = accounts
        .columns
        .iter()
        .find(|c| c.name == "amount")
        .unwrap();
    assert_eq!(amount.semantics.check_constraints.len(), 1);
    assert_eq!(
        amount.semantics.check_constraints[0].expression,
        "amount >= 0"
    );

    let full_name = accounts
        .columns
        .iter()
        .find(|c| c.name == "full_name")
        .unwrap();
    let generated = full_name.semantics.generated.as_ref().unwrap();
    assert!(generated.stored);
    assert_eq!(generated.expression, "id");

    assert_eq!(accounts.check_constraints.len(), 1);
    assert_eq!(
        accounts.check_constraints[0].name.as_deref(),
        Some("amount_and_id")
    );
    assert_eq!(accounts.check_constraints[0].expression, "amount < id");
}

#[test]
fn alter_column_default_change_is_captured() {
    let sql = r"
    CREATE TABLE t (id INT PRIMARY KEY, enabled BOOLEAN DEFAULT false);
    ALTER TABLE t ALTER COLUMN enabled SET DEFAULT true;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let enabled = schema.tables[0]
        .columns
        .iter()
        .find(|c| c.name == "enabled")
        .unwrap();
    assert_eq!(
        enabled.semantics.default_expression.as_deref(),
        Some("true")
    );
}

#[test]
fn parses_target_schema_for_create_table_foreign_keys() {
    let sql = r"
    CREATE TABLE auth.accounts (
      id BIGINT PRIMARY KEY
    );

    CREATE TABLE auth.orgs (
      id BIGINT PRIMARY KEY
    );

    CREATE TABLE public.users (
      id BIGINT PRIMARY KEY,
      account_id BIGINT REFERENCES auth.accounts(id),
      org_id BIGINT,
      CONSTRAINT fk_users_org FOREIGN KEY (org_id) REFERENCES auth.orgs(id)
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema
        .tables
        .iter()
        .find(|table| table.stable_id == "public.users")
        .expect("users table should exist");

    assert_eq!(users.foreign_keys.len(), 2);
    assert_eq!(users.foreign_keys[0].to_schema.as_deref(), Some("auth"));
    assert_eq!(users.foreign_keys[0].to_table, "accounts");
    assert_eq!(users.foreign_keys[1].to_schema.as_deref(), Some("auth"));
    assert_eq!(users.foreign_keys[1].to_table, "orgs");
}

#[test]
fn parses_create_index() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      email TEXT NOT NULL
    );

    CREATE INDEX idx_users_email ON users (email);
    CREATE UNIQUE INDEX idx_users_id ON users (id);
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(users.indexes.len(), 2);

    let email_idx = &users.indexes[0];
    assert_eq!(email_idx.name, Some("idx_users_email".to_string()));
    assert_eq!(email_idx.column_names(), vec!["email"]);
    assert!(!email_idx.is_unique);

    let id_idx = &users.indexes[1];
    assert_eq!(id_idx.name, Some("idx_users_id".to_string()));
    assert_eq!(id_idx.column_names(), vec!["id"]);
    assert!(id_idx.is_unique);
}

#[test]
fn create_index_keeps_expression_only_index() {
    use relune_core::IndexKey;

    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);
    CREATE INDEX idx_lower_email ON users (lower(email));
    ";

    let schema = parse_sql_to_schema(sql).expect("schema should exist");
    let users = &schema.tables[0];

    // The expression index is retained (not dropped) so it counts toward
    // coverage/uniqueness and is detected on removal, but exposes no plain
    // column.
    assert_eq!(users.indexes.len(), 1);
    let idx = &users.indexes[0];
    assert!(idx.has_expression());
    assert!(idx.column_names().is_empty());
    assert!(matches!(idx.key_parts[0], IndexKey::Expression(_)));
}

#[test]
fn create_index_keeps_mixed_expression_index_in_order() {
    use relune_core::IndexKey;

    let sql = r"
    CREATE TABLE users (tenant_id BIGINT, email TEXT NOT NULL);
    CREATE INDEX idx_mixed ON users (tenant_id, lower(email));
    ";

    let schema = parse_sql_to_schema(sql).expect("schema should exist");
    let users = &schema.tables[0];

    assert_eq!(users.indexes.len(), 1);
    let idx = &users.indexes[0];
    // The leading column stays a plain column; the trailing part is recorded as
    // an expression, preserving order for prefix-coverage checks.
    assert_eq!(idx.key_slots(), vec![Some("tenant_id"), None]);
    assert!(matches!(idx.key_parts[1], IndexKey::Expression(_)));
}

#[test]
fn unique_constraint_with_expression_is_dropped_not_narrowed() {
    // `UNIQUE (tenant_id, lower(email))` must not collapse to `UNIQUE
    // (tenant_id)`, which would assert uniqueness the DDL never declared.
    let sql = r"
    CREATE TABLE users (
        tenant_id BIGINT,
        email TEXT NOT NULL,
        UNIQUE (tenant_id, lower(email))
    );
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should exist");
    let users = &schema.tables[0];

    assert!(
        !users.indexes.iter().any(|ix| ix.is_unique),
        "an expression unique constraint must not be recorded as a narrower unique index"
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning
                && d.message.contains("functional/expression"))
    );
}

#[test]
fn handles_schema_qualified_names() {
    let sql = r"
    CREATE TABLE public.users (
      id BIGINT PRIMARY KEY
    );

    CREATE TABLE app.posts (
      id BIGINT PRIMARY KEY
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 2);

    assert_eq!(schema.tables[0].schema_name, Some("public".to_string()));
    assert_eq!(schema.tables[0].name, "users");
    assert_eq!(schema.tables[1].schema_name, Some("app".to_string()));
    assert_eq!(schema.tables[1].name, "posts");
}

#[test]
fn warns_when_object_names_have_more_than_two_parts() {
    let sql = r"
    CREATE TABLE db.public.users (
      id BIGINT PRIMARY KEY
    );
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should exist");

    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].stable_id, "public.users");
    let warning = output
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == codes::parse_unsupported()
                && diagnostic.message.contains("db.public.users")
        })
        .expect("warning should exist");
    assert!(warning.message.contains("ignoring leading qualifier"));
}

#[test]
fn normalizes_identifiers() {
    let sql = r"
    CREATE TABLE Users (
      ID BIGINT PRIMARY KEY,
      Name TEXT NOT NULL
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];

    assert_eq!(table.name, "users");
    assert_eq!(table.columns[0].name, "id");
    assert_eq!(table.columns[1].name, "name");
}

#[test]
fn normalizes_stable_ids_for_lookups() {
    let sql = r"
    CREATE TABLE Public.Users (
      id BIGINT PRIMARY KEY
    );

    CREATE INDEX idx_users_id ON public.users (id);
    COMMENT ON TABLE PUBLIC.USERS IS 'User accounts';
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];

    assert_eq!(table.stable_id, "public.users");
    assert_eq!(table.comment, Some("User accounts".to_string()));
    assert_eq!(table.indexes.len(), 1);
    assert_eq!(table.indexes[0].name, Some("idx_users_id".to_string()));
}

#[test]
fn handles_table_level_primary_key() {
    let sql = r"
    CREATE TABLE order_items (
      order_id BIGINT NOT NULL,
      product_id BIGINT NOT NULL,
      quantity INTEGER NOT NULL,
      PRIMARY KEY (order_id, product_id)
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];

    assert!(table.columns[0].is_primary_key);
    assert!(table.columns[1].is_primary_key);
    assert!(!table.columns[2].is_primary_key);
}

#[test]
fn returns_diagnostics_for_unsupported_constructs() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE VIEW user_view AS SELECT * FROM users;
    CREATE FUNCTION noop() RETURNS void AS $$ BEGIN END; $$ LANGUAGE plpgsql;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);

    assert!(output.schema.is_some());
    assert!(output.has_warnings());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|d| d.code == codes::parse_unsupported())
            .count(),
        1
    );
}

#[test]
fn truncates_unsupported_debug_output_on_utf8_boundaries() {
    let debug = "絵文字🙂".repeat(20);
    let truncated = truncate_unsupported_debug(&debug);

    assert!(truncated.ends_with("..."));
    assert!(truncated.is_char_boundary(truncated.len() - 3));
    assert!(truncated.len() <= MAX_UNSUPPORTED_DEBUG_LEN);
}

#[test]
fn alter_table_add_column_before_create_index() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users ADD COLUMN email TEXT;
    CREATE INDEX idx_users_email ON users (email);
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(table.columns.iter().any(|c| c.name == "email"));
    assert!(
        table
            .indexes
            .iter()
            .any(|i| i.column_names().contains(&"email"))
    );
}

#[test]
fn alter_table_add_foreign_key_constraint() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users ADD COLUMN org_id BIGINT;
    ALTER TABLE users ADD CONSTRAINT fk_users_org
      FOREIGN KEY (org_id) REFERENCES orgs (id);
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        users
            .foreign_keys
            .iter()
            .any(|fk| fk.name.as_deref() == Some("fk_users_org"))
    );
}

#[test]
fn parses_target_schema_for_alter_table_foreign_keys() {
    let sql = r"
    CREATE TABLE auth.orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE auth.accounts (id BIGINT PRIMARY KEY);
    CREATE TABLE public.users (
      id BIGINT PRIMARY KEY,
      org_id BIGINT
    );

    ALTER TABLE public.users ADD COLUMN account_id BIGINT REFERENCES auth.accounts(id);
    ALTER TABLE public.users ADD CONSTRAINT fk_users_org
      FOREIGN KEY (org_id) REFERENCES auth.orgs(id);
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema
        .tables
        .iter()
        .find(|table| table.stable_id == "public.users")
        .expect("users table should exist");

    assert_eq!(users.foreign_keys.len(), 2);
    assert_eq!(users.foreign_keys[0].to_schema.as_deref(), Some("auth"));
    assert_eq!(users.foreign_keys[0].to_table, "accounts");
    assert_eq!(users.foreign_keys[1].name.as_deref(), Some("fk_users_org"));
    assert_eq!(users.foreign_keys[1].to_schema.as_deref(), Some("auth"));
    assert_eq!(users.foreign_keys[1].to_table, "orgs");
}

#[test]
fn handles_duplicate_tables() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);

    assert!(output.schema.is_some());
    assert_eq!(output.schema.as_ref().unwrap().tables.len(), 1);

    assert!(output.has_warnings());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|d| d.code == codes::schema_duplicate_table())
            .count(),
        1
    );
    assert!(
        output
            .diagnostics
            .iter()
            .find(|d| d.code == codes::schema_duplicate_table())
            .and_then(|d| d.span)
            .is_some()
    );
}

#[test]
fn handles_invalid_sql() {
    let sql = "THIS IS NOT VALID SQL";

    let output = parse_sql_to_schema_with_diagnostics(sql);

    assert!(output.schema.is_none());
    assert!(output.has_errors());
}

#[test]
fn recovery_continues_after_semicolon_terminated_error() {
    // A malformed statement that is semicolon-terminated lets the parser
    // recover and still parse the surrounding valid statements.
    let sql = r"
    CREATE TABLE before (id BIGINT PRIMARY KEY);
    THIS IS NOT VALID SQL;
    CREATE TABLE after (id BIGINT PRIMARY KEY);
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(
        output.has_errors(),
        "the malformed statement should be reported"
    );

    let schema = output.schema.expect("valid statements should still parse");
    let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"before"));
    assert!(names.contains(&"after"));
}

#[test]
fn recovery_is_semicolon_delimited_and_consumes_unterminated_errors() {
    // Documents a known limitation: recovery skips to the next semicolon, so a
    // malformed statement without its own terminating semicolon swallows the
    // following statement up to the next one.
    let sql = "THIS IS NOT VALID SQL\nCREATE TABLE swallowed (id BIGINT PRIMARY KEY);";

    let output = parse_sql_to_schema_with_diagnostics(sql);

    assert!(output.has_errors());
    let parsed_swallowed = output
        .schema
        .is_some_and(|schema| schema.tables.iter().any(|t| t.name == "swallowed"));
    assert!(
        !parsed_swallowed,
        "an unterminated malformed statement consumes the following statement"
    );
}

#[test]
fn strict_parse_rejects_error_diagnostics() {
    let sql = "THIS IS NOT VALID SQL";

    let err = parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres)
        .expect_err("strict parsing should reject error diagnostics");
    assert!(err.to_string().contains("error diagnostics"));
}

#[test]
fn parse_error_wraps_sqlparser_errors_as_strings() {
    let parser_error = Parser::new(&PostgreSqlDialect {})
        .try_with_sql("CREATE TABLE")
        .expect("tokenization should succeed")
        .parse_statement()
        .expect_err("statement should fail");

    let error = ParseError::from(parser_error);

    match error {
        ParseError::Sql(message) => {
            assert!(!message.is_empty());
            assert!(message.contains("sql parser error"));
        }
        ParseError::Schema(message) => panic!("unexpected schema error: {message}"),
    }
}

#[test]
fn normalizes_constraint_and_index_names_on_storage() {
    let sql = r"
    CREATE TABLE orgs (
        id BIGINT PRIMARY KEY
    );

    CREATE TABLE users (
        id BIGINT PRIMARY KEY,
        org_id BIGINT,
        email TEXT,
        CONSTRAINT FK_USERS_ORG FOREIGN KEY (org_id) REFERENCES orgs(id)
    );

    CREATE INDEX IDX_USERS_EMAIL ON users (email);
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema
        .tables
        .iter()
        .find(|table| table.name == "users")
        .unwrap();

    assert_eq!(users.foreign_keys[0].name.as_deref(), Some("fk_users_org"));
    assert_eq!(users.indexes[0].name.as_deref(), Some("idx_users_email"));
}

#[test]
fn parse_output_helpers() {
    let output = ParseOutput {
        dialect: SqlDialect::Postgres,
        schema: Some(Schema {
            tables: vec![],
            views: vec![],
            enums: vec![],
        }),
        diagnostics: vec![Diagnostic::warning(codes::parse_unsupported(), "test")],
    };

    assert!(!output.has_errors());
    assert!(output.has_warnings());

    let output_with_errors = ParseOutput {
        dialect: SqlDialect::Postgres,
        schema: None,
        diagnostics: vec![Diagnostic::error(codes::parse_error(), "test")],
    };

    assert!(output_with_errors.has_errors());
}

#[test]
fn warns_when_input_produces_empty_schema() {
    let output = parse_sql_to_schema_with_diagnostics("  -- comments only\n");

    assert!(output.schema.is_some());
    assert!(output.has_warnings());
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == codes::parse_empty_schema()
            && diagnostic
                .message
                .contains("No schema objects were produced")
    }));
}

#[test]
fn handles_composite_foreign_keys() {
    let sql = r"
    CREATE TABLE orders (
      id BIGINT PRIMARY KEY
    );

    CREATE TABLE order_items (
      order_id BIGINT NOT NULL,
      line_num INTEGER NOT NULL,
      product_id BIGINT NOT NULL,
      PRIMARY KEY (order_id, line_num),
      CONSTRAINT fk_order FOREIGN KEY (order_id) REFERENCES orders(id)
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 2);

    let order_items = &schema.tables[1];
    assert_eq!(order_items.foreign_keys.len(), 1);
    assert_eq!(
        order_items.foreign_keys[0].name,
        Some("fk_order".to_string())
    );
    assert_eq!(order_items.foreign_keys[0].from_columns, vec!["order_id"]);
    assert_eq!(order_items.foreign_keys[0].to_table, "orders");
    assert_eq!(order_items.foreign_keys[0].to_columns, vec!["id"]);
}

#[test]
fn generates_sequential_ids() {
    let sql = r"
    CREATE TABLE first (id BIGINT PRIMARY KEY);
    CREATE TABLE second (id BIGINT PRIMARY KEY);
    CREATE TABLE third (id BIGINT PRIMARY KEY);
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 3);

    assert_eq!(schema.tables[0].id, TableId(1));
    assert_eq!(schema.tables[1].id, TableId(2));
    assert_eq!(schema.tables[2].id, TableId(3));
}

#[test]
fn generates_column_ids_per_table() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      name TEXT NOT NULL,
      email TEXT
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];

    assert_eq!(table.columns[0].id, ColumnId(1));
    assert_eq!(table.columns[1].id, ColumnId(2));
    assert_eq!(table.columns[2].id, ColumnId(3));
}

#[test]
fn generates_column_ids_for_alter_table_add_column() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY
    );

    ALTER TABLE users ADD COLUMN name TEXT NOT NULL;
    ALTER TABLE users ADD COLUMN email TEXT;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];

    assert_eq!(table.columns[0].id, ColumnId(1));
    assert_eq!(table.columns[1].id, ColumnId(2));
    assert_eq!(table.columns[2].id, ColumnId(3));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "sql span end must not precede start")]
fn rejects_reversed_sql_spans_in_debug_builds() {
    let span = SqlSpan::new(Location::new(1, 5), Location::new(1, 3));
    let offsets = LineOffsets::new("abcd");

    let _ = source_span_from_sql_span("abcd", &offsets, span);
}

#[cfg(not(debug_assertions))]
#[test]
fn ignores_reversed_sql_spans_in_release_builds() {
    let span = SqlSpan::new(Location::new(1, 5), Location::new(1, 3));
    let offsets = LineOffsets::new("abcd");

    assert_eq!(source_span_from_sql_span("abcd", &offsets, span), None);
}

#[test]
fn handles_index_on_unknown_table() {
    let sql = r"
    CREATE INDEX idx_missing ON nonexistent_table (id);
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);

    // Schema should be Some but empty (no tables, but no errors)
    assert!(output.schema.is_some());
    assert_eq!(output.schema.as_ref().unwrap().tables.len(), 0);

    // Should have warning about unknown table
    assert!(output.has_warnings());
}

#[test]
fn unknown_table_warnings_include_spans() {
    let sql = r"
    CREATE INDEX idx_missing ON nonexistent_table (id);
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let warning = output
        .diagnostics
        .iter()
        .find(|d| d.code == codes::schema_unknown_table())
        .expect("warning should exist");

    assert!(warning.span.is_some());
}

#[test]
fn parses_create_type_as_enum() {
    let sql = r"
    CREATE TYPE status AS ENUM ('active', 'inactive', 'pending');
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.enums.len(), 1);

    let status_enum = &schema.enums[0];
    assert_eq!(status_enum.id, "status");
    assert_eq!(status_enum.schema_name, None);
    assert_eq!(status_enum.name, "status");
    assert_eq!(status_enum.values, vec!["active", "inactive", "pending"]);
}

#[test]
fn parses_schema_qualified_enum() {
    let sql = r"
    CREATE TYPE public.user_role AS ENUM ('admin', 'user', 'guest');
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.enums.len(), 1);

    let role_enum = &schema.enums[0];
    assert_eq!(role_enum.id, "public.user_role");
    assert_eq!(role_enum.schema_name, Some("public".to_string()));
    assert_eq!(role_enum.name, "user_role");
    assert_eq!(role_enum.values, vec!["admin", "user", "guest"]);
}

#[test]
fn handles_tables_and_enums_together() {
    let sql = r"
    CREATE TYPE status AS ENUM ('active', 'inactive');

    CREATE TABLE users (
        id BIGINT PRIMARY KEY,
        status TEXT NOT NULL
    );
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.enums.len(), 1);

    assert_eq!(schema.enums[0].name, "status");
    assert_eq!(schema.tables[0].name, "users");
}

#[test]
fn parses_comment_on_table() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      name TEXT NOT NULL
    );

    COMMENT ON TABLE users IS 'Stores user information';
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(users.comment, Some("Stores user information".to_string()));
}

#[test]
fn parses_comment_on_column() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      email TEXT NOT NULL
    );

    COMMENT ON COLUMN users.email IS 'User email address';
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(users.columns[0].comment, None);
    assert_eq!(
        users.columns[1].comment,
        Some("User email address".to_string())
    );
}

#[test]
fn parses_comment_on_schema_qualified_table() {
    let sql = r"
    CREATE TABLE public.users (
      id BIGINT PRIMARY KEY
    );

    COMMENT ON TABLE public.users IS 'Public users table';
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(users.comment, Some("Public users table".to_string()));
}

#[test]
fn parses_comment_on_schema_qualified_column() {
    let sql = r"
    CREATE TABLE public.users (
      id BIGINT PRIMARY KEY,
      created_at TIMESTAMP
    );

    COMMENT ON COLUMN public.users.created_at IS 'Record creation timestamp';
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(
        users.columns[1].comment,
        Some("Record creation timestamp".to_string())
    );
}

#[test]
fn handles_comment_on_unknown_table() {
    let sql = r"
    COMMENT ON TABLE nonexistent IS 'This table does not exist';
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);

    assert!(output.schema.is_some());
    assert!(output.has_warnings());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|d| d.code == codes::schema_unknown_table())
            .count(),
        1
    );
}

#[test]
fn handles_comment_on_unknown_column() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY
    );

    COMMENT ON COLUMN users.nonexistent IS 'This column does not exist';
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);

    assert!(output.schema.is_some());
    assert!(output.has_warnings());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|d| d.code == codes::schema_unknown_column())
            .count(),
        1
    );
}

#[test]
fn handles_null_comment() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY
    );

    COMMENT ON TABLE users IS NULL;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(users.comment, None);
}

#[test]
fn parses_create_view() {
    let sql = r"
    CREATE TABLE users (
        id BIGINT PRIMARY KEY,
        name TEXT NOT NULL
    );

    CREATE VIEW user_view AS
        SELECT id, name FROM users WHERE id > 0;

    CREATE VIEW public.active_users AS
        SELECT id, name FROM users WHERE name IS NOT NULL;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.views.len(), 2);

    let user_view = &schema.views[0];
    assert_eq!(user_view.id, "user_view");
    assert_eq!(user_view.schema_name, None);
    assert_eq!(user_view.name, "user_view");
    assert!(user_view.definition.is_some());
    assert!(
        user_view
            .definition
            .as_ref()
            .unwrap()
            .contains("SELECT id, name FROM users")
    );
    // View columns are extracted from the SELECT items
    assert_eq!(user_view.columns.len(), 2);
    assert_eq!(user_view.columns[0].name, "id");
    assert_eq!(user_view.columns[1].name, "name");

    let active_users = &schema.views[1];
    assert_eq!(active_users.id, "public.active_users");
    assert_eq!(active_users.schema_name, Some("public".to_string()));
    assert_eq!(active_users.name, "active_users");
    assert!(active_users.definition.is_some());
}

#[test]
fn create_table_as_select_derives_columns_from_projection() {
    let sql = "CREATE TABLE summary AS SELECT id, name AS label FROM users;";
    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should be produced");

    let summary = schema
        .tables
        .iter()
        .find(|t| t.name == "summary")
        .expect("CREATE TABLE AS SELECT should produce a table");
    assert_eq!(
        summary
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "label"],
        "columns should be derived from the SELECT projection"
    );
    assert!(
        summary.columns.iter().all(|c| c.data_type == "unknown"),
        "derived columns have no resolvable data type"
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning),
        "deriving named projection columns should not warn"
    );
}

#[test]
fn create_table_as_select_wildcard_warns_without_columns() {
    let sql = "CREATE TABLE summary AS SELECT * FROM users;";
    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should be produced");

    let summary = schema
        .tables
        .iter()
        .find(|t| t.name == "summary")
        .expect("CREATE TABLE AS SELECT should still produce a table");
    assert!(
        summary.columns.is_empty(),
        "wildcard projection cannot yield named columns"
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning
                && d.message.contains("could not derive columns")),
        "a column-less CREATE TABLE AS SELECT must be diagnosed, not silent"
    );
}

#[test]
fn create_table_as_select_derives_columns_through_set_operation() {
    // UNION output column names come from the left-most SELECT.
    let sql = "CREATE TABLE merged AS SELECT id, name FROM a UNION SELECT id, name FROM b;";
    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should be produced");

    let merged = schema
        .tables
        .iter()
        .find(|t| t.name == "merged")
        .expect("CREATE TABLE AS SELECT should produce a table");
    assert_eq!(
        merged
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "name"],
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning),
        "a set-operation projection with named columns should not warn"
    );
}

#[test]
fn create_table_as_select_derives_columns_through_parenthesized_query() {
    let sql = "CREATE TABLE wrapped AS (SELECT id FROM src);";
    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should be produced");

    let wrapped = schema
        .tables
        .iter()
        .find(|t| t.name == "wrapped")
        .expect("CREATE TABLE AS SELECT should produce a table");
    assert_eq!(
        wrapped
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id"],
    );
}

#[test]
fn create_table_as_select_with_wildcard_and_named_item_warns() {
    // `SELECT *, id` cannot be fully enumerated; recording only `id` would be a
    // misleading partial schema, so it is treated as underivable and warned.
    let sql = "CREATE TABLE summary AS SELECT *, id FROM users;";
    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should be produced");

    let summary = schema
        .tables
        .iter()
        .find(|t| t.name == "summary")
        .expect("CREATE TABLE AS SELECT should still produce a table");
    assert!(
        summary.columns.is_empty(),
        "a wildcard mixed with named items cannot be faithfully enumerated"
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning
                && d.message.contains("could not derive columns")),
    );
}

#[test]
fn create_table_as_select_with_unnamed_expression_warns() {
    // An unnamed non-column expression (`count(*)`) has no derivable name, so
    // the projection fails closed rather than recording only `id`.
    let sql = "CREATE TABLE summary AS SELECT id, count(*) FROM users GROUP BY id;";
    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("schema should be produced");

    let summary = schema
        .tables
        .iter()
        .find(|t| t.name == "summary")
        .expect("CREATE TABLE AS SELECT should still produce a table");
    assert!(
        summary.columns.is_empty(),
        "an unnamed expression makes the column set incomplete"
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning
                && d.message.contains("could not derive columns")),
    );
}

// Snapshot tests for all fixtures
mod snapshot_tests {
    use super::*;

    fn snapshot_fixture(name: &str, sql: &str) {
        let output = parse_sql_to_schema_with_diagnostics(sql);

        insta::assert_json_snapshot!(
            format!("fixture_{}", name.replace('.', "_")),
            snapshot_data(&output)
        );
    }

    #[test]
    fn snapshot_simple_blog() {
        let sql = read_sql_fixture("simple_blog.sql");
        snapshot_fixture("simple_blog", &sql);
    }

    #[test]
    fn snapshot_ecommerce() {
        let sql = read_sql_fixture("ecommerce.sql");
        snapshot_fixture("ecommerce", &sql);
    }

    #[test]
    fn snapshot_multi_schema() {
        let sql = read_sql_fixture("multi_schema.sql");
        snapshot_fixture("multi_schema", &sql);
    }

    #[test]
    fn snapshot_broken_input() {
        let sql = read_sql_fixture("broken_input.sql");
        snapshot_fixture("broken_input", &sql);
    }

    #[test]
    fn snapshot_cyclic_fk() {
        let sql = read_sql_fixture("cyclic_fk.sql");
        snapshot_fixture("cyclic_fk", &sql);
    }

    #[test]
    fn snapshot_join_heavy() {
        let sql = read_sql_fixture("join_heavy.sql");
        snapshot_fixture("join_heavy", &sql);
    }

    fn snapshot_fixture_with_dialect(name: &str, sql: &str, dialect: SqlDialect) {
        let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, dialect);

        insta::assert_json_snapshot!(
            format!("fixture_{}", name.replace('.', "_")),
            snapshot_data(&output)
        );
    }

    #[test]
    fn snapshot_mysql_ecommerce() {
        let sql = read_sql_fixture("mysql_ecommerce.sql");
        snapshot_fixture_with_dialect("mysql_ecommerce", &sql, SqlDialect::Mysql);
    }

    #[test]
    fn snapshot_sqlite_blog() {
        let sql = read_sql_fixture("sqlite_blog.sql");
        snapshot_fixture_with_dialect("sqlite_blog", &sql, SqlDialect::Sqlite);
    }
}

#[test]
fn test_detect_dialect_mysql() {
    let sql = r"
        CREATE TABLE `users` (
            `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
            `name` VARCHAR(255) NOT NULL,
            PRIMARY KEY (`id`)
        ) ENGINE=InnoDB;
    ";
    assert_eq!(detect_dialect(sql), SqlDialect::Mysql);
}

#[test]
fn test_detect_dialect_sqlite() {
    let sql = r"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL
        );
    ";
    assert_eq!(detect_dialect(sql), SqlDialect::Sqlite);
}

#[test]
fn test_detect_dialect_mysql_with_single_signal() {
    let sql = "CREATE TABLE `users` (`id` INT PRIMARY KEY);";
    assert_eq!(detect_dialect(sql), SqlDialect::Mysql);
}

#[test]
fn test_detect_dialect_sqlite_with_single_signal() {
    let sql = r"
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
    ";
    assert_eq!(detect_dialect(sql), SqlDialect::Sqlite);
}

#[test]
fn test_detect_dialect_postgres() {
    let sql = r"
        CREATE TYPE status AS ENUM ('active', 'inactive');
        CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL
        );
        COMMENT ON TABLE users IS 'User accounts';
    ";
    assert_eq!(detect_dialect(sql), SqlDialect::Postgres);
}

#[test]
fn test_detect_dialect_default_postgres() {
    let sql = r"
        CREATE TABLE users (
            id INT PRIMARY KEY,
            name TEXT NOT NULL
        );
    ";
    // Generic SQL should default to Postgres
    assert_eq!(detect_dialect(sql), SqlDialect::Postgres);
}

#[test]
fn test_detect_dialect_ignores_comment_markers() {
    let sql = r"
        -- ENGINE=InnoDB AUTO_INCREMENT
        /* PRAGMA foreign_keys = ON */
        CREATE TABLE users (
            id INT PRIMARY KEY
        );
    ";

    assert_eq!(detect_dialect(sql), SqlDialect::Postgres);
}

#[test]
fn test_detect_dialect_inline_enum_is_mysql() {
    // Column-position inline ENUM(...) is MySQL-specific syntax.
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, status ENUM('a', 'b'));";
    assert_eq!(detect_dialect(sql), SqlDialect::Mysql);
}

#[test]
fn test_detect_dialect_inline_enum_outweighs_integer_primary_key() {
    // An inline enum/set is definitively MySQL and must win over SQLite's weak
    // `INTEGER PRIMARY KEY` heuristic when both appear.
    let sql = "CREATE TABLE t (id INTEGER PRIMARY KEY, status ENUM('a', 'b'));";
    assert_eq!(detect_dialect(sql), SqlDialect::Mysql);
}

#[test]
fn test_detect_dialect_named_enum_type_is_not_mysql() {
    // PostgreSQL's `CREATE TYPE ... AS ENUM` must not be read as a MySQL signal.
    let sql = "CREATE TYPE mood AS ENUM ('happy', 'sad');";
    assert_eq!(detect_dialect(sql), SqlDialect::Postgres);
}

#[test]
fn test_detect_dialect_set_storage_parameter_is_not_mysql() {
    // PostgreSQL `SET (...)` storage parameters open with an identifier, not a
    // quoted value, so they must not be mistaken for a MySQL `SET(...)` type.
    let sql = "CREATE TABLE t (id INT PRIMARY KEY);\nALTER TABLE t SET (fillfactor = 70);";
    assert_eq!(detect_dialect(sql), SqlDialect::Postgres);
}

proptest! {
    #[test]
    fn prop_detect_dialect_mysql_from_backticks(table in "[a-z][a-z0-9_]{0,15}") {
        let sql = format!("CREATE TABLE `{table}` (`id` INT PRIMARY KEY);");
        prop_assert_eq!(detect_dialect(&sql), SqlDialect::Mysql);
    }

    #[test]
    fn prop_detect_dialect_sqlite_from_integer_primary_key(table in "[a-z][a-z0-9_]{0,15}") {
        let sql = format!(
            "CREATE TABLE {table} (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"
        );
        prop_assert_eq!(detect_dialect(&sql), SqlDialect::Sqlite);
    }
}

#[test]
fn auto_detection_matches_explicit_dialect_for_fixture_corpus() {
    let cases = [
        ("simple_blog.sql", SqlDialect::Postgres),
        ("ecommerce.sql", SqlDialect::Postgres),
        ("multi_schema.sql", SqlDialect::Postgres),
        ("cyclic_fk.sql", SqlDialect::Postgres),
        ("join_heavy.sql", SqlDialect::Postgres),
        ("mysql_ecommerce.sql", SqlDialect::Mysql),
        ("sqlite_blog.sql", SqlDialect::Sqlite),
    ];

    for (fixture, dialect) in cases {
        let sql = read_sql_fixture(fixture);
        let auto = parse_sql_to_schema_with_diagnostics(&sql);
        let explicit = parse_sql_to_schema_with_diagnostics_and_dialect(&sql, dialect);

        assert_eq!(
            auto.dialect, dialect,
            "auto-detected wrong dialect for {fixture}"
        );
        assert_eq!(
            snapshot_data(&auto),
            snapshot_data(&explicit),
            "auto-detected parse output diverged for {fixture}"
        );
    }
}

#[test]
fn test_parse_mysql_basic() {
    let sql = r"
        CREATE TABLE `users` (
            `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
            `name` VARCHAR(255) NOT NULL,
            `email` VARCHAR(255) NOT NULL,
            PRIMARY KEY (`id`),
            UNIQUE KEY `idx_email` (`email`)
        ) ENGINE=InnoDB;
    ";
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(users.name, "users");
    assert_eq!(users.columns.len(), 3);
    assert_eq!(users.columns[0].name, "id");
    assert!(!users.columns[0].nullable);
}

#[test]
fn test_parse_mysql_foreign_keys() {
    let sql = r"
        CREATE TABLE `users` (
            `id` BIGINT NOT NULL AUTO_INCREMENT,
            PRIMARY KEY (`id`)
        ) ENGINE=InnoDB;

        CREATE TABLE `posts` (
            `id` BIGINT NOT NULL AUTO_INCREMENT,
            `user_id` BIGINT NOT NULL,
            PRIMARY KEY (`id`),
            CONSTRAINT `fk_posts_user` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
        ) ENGINE=InnoDB;
    ";
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 2);

    let posts = &schema.tables[1];
    assert_eq!(posts.foreign_keys.len(), 1);
    assert_eq!(posts.foreign_keys[0].to_table, "users");
    assert_eq!(posts.foreign_keys[0].on_delete, ReferentialAction::Cascade);
}

#[test]
fn test_parse_mysql_enum_and_set_types_populate_column_enum_values() {
    let sql = r"
        CREATE TABLE `users` (
            `id` BIGINT NOT NULL AUTO_INCREMENT,
            `status` ENUM('draft', 'published') NOT NULL,
            `flags` SET('featured', 'archived') NULL,
            PRIMARY KEY (`id`)
        ) ENGINE=InnoDB;
    ";
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");

    assert!(
        schema.enums.is_empty(),
        "inline enum/set columns must not synthesize Schema-level enum types"
    );

    let users = &schema.tables[0];
    assert_eq!(users.columns[1].data_type, "enum('draft','published')");
    assert_eq!(
        users.columns[1].enum_values,
        Some(vec!["draft".to_string(), "published".to_string()])
    );

    assert_eq!(users.columns[2].data_type, "set('featured','archived')");
    assert_eq!(
        users.columns[2].enum_values,
        Some(vec!["featured".to_string(), "archived".to_string()])
    );

    assert!(users.columns[0].enum_values.is_none());
}

#[test]
fn test_parse_mysql_enum_like_type_rejects_reversed_parentheses() {
    assert_eq!(parse_mysql_enum_like_type(")enum("), Ok(None));
}

#[test]
fn test_parse_mysql_enum_like_type_preserves_trailing_backslash() {
    assert_eq!(
        parse_mysql_enum_like_type("enum('back\\\\')"),
        Ok(Some(("enum".to_string(), vec!["back\\".to_string()])))
    );
}

#[test]
fn test_parse_mysql_enum_like_type_preserves_unknown_backslash_sequences() {
    assert_eq!(
        parse_mysql_enum_like_type(r"enum('line\nbreak')"),
        Ok(Some(("enum".to_string(), vec![r"line\nbreak".to_string()])))
    );
}

#[test]
fn test_parse_mysql_enum_like_type_rejects_incomplete_escape_sequences() {
    assert_eq!(
        parse_mysql_enum_like_type("enum('bad\\)"),
        Err(MySqlEnumLikeParseError::TrailingEscapeSequence)
    );
}

#[test]
fn test_populate_mysql_enum_columns_warns_on_malformed_definitions() {
    let mut ctx = ParseContext::new();
    ctx.dialect = SqlDialect::Mysql;

    let mut tables = vec![Table {
        id: TableId(1),
        stable_id: "users".to_string(),
        schema_name: None,
        name: "users".to_string(),
        columns: vec![Column {
            id: ColumnId(1),
            name: "status".to_string(),
            data_type: "enum('bad\\')".to_string(),
            nullable: false,
            is_primary_key: false,
            comment: None,
            enum_values: None,
            semantics: relune_core::ColumnSemantics::default(),
        }],
        foreign_keys: vec![],
        indexes: vec![],
        primary_key_name: None,
        check_constraints: Vec::new(),
        comment: None,
    }];

    populate_inline_enum_columns(&mut ctx, &mut tables);

    assert!(
        tables[0].columns[0].enum_values.is_none(),
        "malformed enum/set definitions must not populate column enum_values"
    );
    assert!(
        ctx.diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.severity == Severity::Warning })
    );
    assert!(ctx.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Malformed inline enum/set definition")
    }));
}

#[test]
fn test_parse_mysql_inline_enum_does_not_create_schema_enum_entries_across_tables() {
    let sql = r"
        CREATE TABLE `users` (
            `id` BIGINT NOT NULL AUTO_INCREMENT,
            `status` ENUM('draft', 'published') NOT NULL,
            PRIMARY KEY (`id`)
        ) ENGINE=InnoDB;

        CREATE TABLE `posts` (
            `id` BIGINT NOT NULL AUTO_INCREMENT,
            `status` ENUM('draft', 'published') NOT NULL,
            PRIMARY KEY (`id`)
        ) ENGINE=InnoDB;
    ";
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");

    assert!(schema.enums.is_empty());

    let expected_values = Some(vec!["draft".to_string(), "published".to_string()]);
    for table in &schema.tables {
        let column = table
            .columns
            .iter()
            .find(|c| c.name == "status")
            .expect("status column should exist");
        assert_eq!(column.enum_values, expected_values);
    }
}

#[test]
fn inline_enum_values_recovered_under_non_mysql_dialect() {
    // A MySQL dump misclassified as another dialect must still recover its
    // inline enum values; the type string is canonicalized regardless of
    // dialect, so the values must be too.
    let sql = "CREATE TABLE t (id INT PRIMARY KEY, status ENUM('draft', 'published'));";
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres).expect("parse should succeed");

    let status = schema.tables[0]
        .columns
        .iter()
        .find(|c| c.name == "status")
        .expect("status column should exist");
    assert_eq!(
        status.enum_values,
        Some(vec!["draft".to_string(), "published".to_string()]),
        "inline enum values must be recovered even when not parsed as MySQL"
    );
}

#[test]
fn test_parse_sqlite_basic() {
    let sql = r"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE
        );
    ";
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Sqlite).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);

    let users = &schema.tables[0];
    assert_eq!(users.name, "users");
    assert_eq!(users.columns.len(), 3);
    assert_eq!(users.columns[0].name, "id");
    assert!(users.columns[0].is_primary_key);
}

#[test]
fn test_parse_sqlite_foreign_keys() {
    let sql = r"
        CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL
        );

        CREATE TABLE posts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            author_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE CASCADE
        );
    ";
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Sqlite).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 2);

    let posts = &schema.tables[1];
    assert_eq!(posts.foreign_keys.len(), 1);
    assert_eq!(posts.foreign_keys[0].to_table, "users");
}

#[test]
fn test_parse_auto_detect_mysql() {
    let sql = r"
        CREATE TABLE `users` (
            `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
            `name` VARCHAR(255) NOT NULL,
            PRIMARY KEY (`id`)
        ) ENGINE=InnoDB;
    ";
    // Auto dialect should detect MySQL and parse correctly
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Auto).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].name, "users");
}

#[test]
fn test_parse_auto_detect_sqlite() {
    let sql = r"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL
        );
    ";
    // Auto dialect should detect SQLite and parse correctly
    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Auto).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].name, "users");
}

#[test]
fn test_sql_dialect_from_str() {
    assert_eq!(
        "postgres".parse::<SqlDialect>().unwrap(),
        SqlDialect::Postgres
    );
    assert_eq!(
        "postgresql".parse::<SqlDialect>().unwrap(),
        SqlDialect::Postgres
    );
    assert_eq!("pg".parse::<SqlDialect>().unwrap(), SqlDialect::Postgres);
    assert_eq!("mysql".parse::<SqlDialect>().unwrap(), SqlDialect::Mysql);
    assert_eq!("sqlite".parse::<SqlDialect>().unwrap(), SqlDialect::Sqlite);
    assert_eq!("sqlite3".parse::<SqlDialect>().unwrap(), SqlDialect::Sqlite);
    assert_eq!("auto".parse::<SqlDialect>().unwrap(), SqlDialect::Auto);
    assert!("unknown".parse::<SqlDialect>().is_err());
}

// ========================================================================
// ALTER TABLE operation tests
// ========================================================================

#[test]
fn alter_table_drop_column_removes_column() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      name TEXT NOT NULL,
      email TEXT
    );
    ALTER TABLE users DROP COLUMN email;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];
    assert_eq!(table.columns.len(), 2);
    assert!(!table.columns.iter().any(|c| c.name == "email"));
}

#[test]
fn alter_table_drop_column_cascades_to_fk_and_index() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      org_id BIGINT,
      CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
    );
    CREATE INDEX idx_org ON users (org_id);
    ALTER TABLE users DROP COLUMN org_id;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(!users.columns.iter().any(|c| c.name == "org_id"));
    assert!(
        users.foreign_keys.is_empty(),
        "FK referencing dropped column should be removed"
    );
    assert!(
        users.indexes.is_empty(),
        "index referencing dropped column should be removed"
    );
}

#[test]
fn alter_table_drop_column_clears_dangling_primary_key_name() {
    let sql = r"
    CREATE TABLE t (
      id BIGINT,
      name TEXT,
      CONSTRAINT pk_t PRIMARY KEY (id)
    );
    ALTER TABLE t DROP COLUMN id;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];
    assert!(!table.columns.iter().any(|c| c.name == "id"));
    assert!(
        !table.columns.iter().any(|c| c.is_primary_key),
        "no primary-key columns should remain"
    );
    assert_eq!(
        table.primary_key_name, None,
        "primary key name must not dangle after its only column is dropped"
    );
}

#[test]
fn alter_table_drop_column_keeps_primary_key_name_for_remaining_columns() {
    let sql = r"
    CREATE TABLE t (
      tenant_id BIGINT,
      id BIGINT,
      name TEXT,
      CONSTRAINT pk_t PRIMARY KEY (tenant_id, id)
    );
    ALTER TABLE t DROP COLUMN name;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];
    assert_eq!(
        table.primary_key_name,
        Some("pk_t".to_string()),
        "dropping a non-key column must not clear the primary key name"
    );
}

#[test]
fn alter_table_drop_column_removes_incoming_fk_to_column() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE TABLE orders (
      id BIGINT PRIMARY KEY,
      user_id BIGINT,
      CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
    );
    ALTER TABLE users DROP COLUMN id;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
    assert!(
        orders.foreign_keys.is_empty(),
        "FK referencing dropped target column should be removed"
    );
}

#[test]
fn alter_table_drop_unknown_column_warns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users DROP COLUMN ghost;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(output.schema.is_some());
    assert!(output.has_warnings());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == codes::schema_unknown_column() && d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_rename_column() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      name TEXT NOT NULL
    );
    ALTER TABLE users RENAME COLUMN name TO full_name;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];
    assert!(!table.columns.iter().any(|c| c.name == "name"));
    assert!(table.columns.iter().any(|c| c.name == "full_name"));
}

#[test]
fn alter_table_rename_column_updates_fk_and_index() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      org_id BIGINT,
      CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
    );
    CREATE INDEX idx_org ON users (org_id);
    ALTER TABLE users RENAME COLUMN org_id TO organization_id;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(users.columns.iter().any(|c| c.name == "organization_id"));
    assert!(
        users.foreign_keys[0]
            .from_columns
            .contains(&"organization_id".to_string()),
        "FK from_columns should be updated after rename"
    );
    assert!(
        users.indexes[0].column_names().contains(&"organization_id"),
        "index columns should be updated after rename"
    );
}

#[test]
fn alter_table_rename_column_updates_referring_fk_to_columns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE TABLE orders (
      id BIGINT PRIMARY KEY,
      user_id BIGINT,
      CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
    );
    ALTER TABLE users RENAME COLUMN id TO user_id;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
    assert_eq!(
        orders.foreign_keys[0].to_columns,
        vec!["user_id".to_string()],
        "FK to_columns should be updated when referenced column is renamed"
    );
}

#[test]
fn alter_table_rename_column_updates_unqualified_same_schema_referring_fk() {
    let sql = r"
    CREATE TABLE public.users (id BIGINT PRIMARY KEY);
    CREATE TABLE public.orders (
      id BIGINT PRIMARY KEY,
      user_id BIGINT,
      CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
    );
    ALTER TABLE public.users RENAME COLUMN id TO user_id;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
    assert_eq!(
        orders.foreign_keys[0].to_columns,
        vec!["user_id".to_string()],
        "same-schema unqualified FK to_columns should be updated"
    );
}

#[test]
fn alter_table_rename_column_updates_self_referencing_fk() {
    let sql = r"
    CREATE TABLE employees (
      id BIGINT PRIMARY KEY,
      manager_id BIGINT,
      CONSTRAINT fk_manager FOREIGN KEY (manager_id) REFERENCES employees(id)
    );
    ALTER TABLE employees RENAME COLUMN id TO employee_id;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let emp = &schema.tables[0];
    assert!(emp.columns.iter().any(|c| c.name == "employee_id"));
    assert_eq!(
        emp.foreign_keys[0].to_columns,
        vec!["employee_id".to_string()],
        "self-referencing FK to_columns should be updated"
    );
}

#[test]
fn alter_table_rename_unknown_column_warns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users RENAME COLUMN ghost TO phantom;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(output.schema.is_some());
    assert!(output.has_warnings());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == codes::schema_unknown_column() && d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_rename_table() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users RENAME TO accounts;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].name, "accounts");
    assert_eq!(schema.tables[0].stable_id, "accounts");
}

#[test]
fn alter_table_rename_table_preserves_schema_when_new_name_is_unqualified() {
    let sql = r"
    CREATE TABLE public.users (id BIGINT PRIMARY KEY);
    ALTER TABLE public.users RENAME TO accounts;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].schema_name.as_deref(), Some("public"));
    assert_eq!(schema.tables[0].name, "accounts");
    assert_eq!(schema.tables[0].stable_id, "public.accounts");
}

#[test]
fn alter_table_rename_table_updates_referring_fk() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE TABLE orders (
      id BIGINT PRIMARY KEY,
      user_id BIGINT,
      CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
    );
    ALTER TABLE users RENAME TO accounts;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
    assert_eq!(
        orders.foreign_keys[0].to_table, "accounts",
        "FK to_table should be updated when referenced table is renamed"
    );
}

#[test]
fn alter_table_rename_table_updates_unqualified_same_schema_referring_fk() {
    let sql = r"
    CREATE TABLE public.users (id BIGINT PRIMARY KEY);
    CREATE TABLE public.orders (
      id BIGINT PRIMARY KEY,
      user_id BIGINT,
      CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id)
    );
    ALTER TABLE public.users RENAME TO accounts;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
    assert_eq!(
        orders.foreign_keys[0].to_table, "accounts",
        "same-schema unqualified FK to_table should be updated"
    );
    assert_eq!(
        orders.foreign_keys[0].to_schema, None,
        "unqualified FK should remain unqualified when schema did not change"
    );
}

#[test]
fn alter_table_rename_table_allows_reusing_old_name() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users RENAME TO accounts;
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table_names: std::collections::HashSet<&str> = schema
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect();
    assert_eq!(schema.tables.len(), 2);
    assert!(table_names.contains("accounts"));
    assert!(table_names.contains("users"));
}

#[test]
fn alter_table_drop_constraint_removes_fk() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      org_id BIGINT,
      CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
    );
    ALTER TABLE users DROP CONSTRAINT fk_org;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        users.foreign_keys.is_empty(),
        "FK should be removed by DROP CONSTRAINT"
    );
}

#[test]
fn alter_table_drop_constraint_removes_index() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      email TEXT
    );
    CREATE INDEX idx_email ON users (email);
    ALTER TABLE users DROP CONSTRAINT idx_email;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        users.indexes.is_empty(),
        "index should be removed by DROP CONSTRAINT"
    );
}

#[test]
fn alter_table_drop_constraint_removes_named_primary_key() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT,
      name TEXT NOT NULL,
      CONSTRAINT pk_users PRIMARY KEY (id)
    );
    ALTER TABLE users DROP CONSTRAINT pk_users;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        !users.columns.iter().any(|c| c.is_primary_key),
        "primary key should be cleared after DROP CONSTRAINT"
    );
    assert!(
        users.primary_key_name.is_none(),
        "primary key name should be cleared"
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("pk_users")),
        "no warning should be emitted for valid PK drop"
    );
}

#[test]
fn alter_table_drop_constraint_removes_column_level_named_primary_key() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT CONSTRAINT pk_users PRIMARY KEY,
      name TEXT NOT NULL
    );
    ALTER TABLE users DROP CONSTRAINT pk_users;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        !users.columns.iter().any(|c| c.is_primary_key),
        "primary key should be cleared after DROP CONSTRAINT"
    );
    assert!(users.primary_key_name.is_none());
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("pk_users")),
        "no warning should be emitted for valid PK drop"
    );
}

#[test]
fn alter_table_drop_primary_key_clears_constraint_name() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT,
      name TEXT NOT NULL,
      CONSTRAINT pk_users PRIMARY KEY (id)
    );
    ALTER TABLE users DROP PRIMARY KEY;
    ALTER TABLE users DROP CONSTRAINT pk_users;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(users.primary_key_name.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("pk_users")),
        "second DROP CONSTRAINT should warn that pk_users no longer exists"
    );
}

#[test]
fn alter_table_drop_constraint_named_pk_added_via_alter() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT NOT NULL,
      name TEXT NOT NULL
    );
    ALTER TABLE users ADD CONSTRAINT pk_users PRIMARY KEY (id);
    ALTER TABLE users DROP CONSTRAINT pk_users;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        !users.columns.iter().any(|c| c.is_primary_key),
        "primary key should be cleared after DROP CONSTRAINT"
    );
    assert!(users.primary_key_name.is_none());
}

#[test]
fn alter_table_drop_unknown_constraint_warns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users DROP CONSTRAINT ghost;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(output.schema.is_some());
    assert!(output.has_warnings());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_drop_primary_key() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      name TEXT NOT NULL
    );
    ALTER TABLE users DROP PRIMARY KEY;
    ";

    let schema = parse_sql_to_schema(sql).expect("parse should succeed");
    let table = &schema.tables[0];
    assert!(
        !table.columns.iter().any(|c| c.is_primary_key),
        "all PK flags should be cleared after DROP PRIMARY KEY"
    );
}

#[test]
fn alter_table_drop_foreign_key_mysql_style() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      org_id BIGINT,
      CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
    );
    ALTER TABLE users DROP FOREIGN KEY fk_org;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        users.foreign_keys.is_empty(),
        "FK should be removed by DROP FOREIGN KEY"
    );
}

#[test]
fn alter_table_drop_unknown_foreign_key_warns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users DROP FOREIGN KEY ghost;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(output.schema.is_some());
    assert!(output.has_warnings());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_drop_index() {
    let sql = r"
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      email TEXT
    );
    CREATE INDEX idx_email ON users (email);
    ALTER TABLE users DROP INDEX idx_email;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    let schema = output.schema.expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        users.indexes.is_empty(),
        "index should be removed by DROP INDEX"
    );
}

#[test]
fn alter_table_drop_unknown_index_warns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users DROP INDEX ghost;
    ";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(output.schema.is_some());
    assert!(output.has_warnings());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_unsupported_operation_truncates_debug_output() {
    let sql = r"
    CREATE TABLE t (id BIGINT PRIMARY KEY);
    ALTER TABLE t OWNER TO some_role;
    ";

    let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, SqlDialect::Postgres);
    let message = output
        .diagnostics
        .iter()
        .find(|d| d.message.contains("ALTER TABLE operation (unsupported)"))
        .map(|d| d.message.as_str())
        .expect("an unsupported ALTER operation should be diagnosed");

    assert!(
        message.contains("..."),
        "the operation's debug rendering should be truncated, got: {message}"
    );
}

#[test]
fn alter_table_alter_column_set_data_type() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, bio TEXT);
    ALTER TABLE users ALTER COLUMN bio TYPE VARCHAR(10);
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres).expect("parse should succeed");
    let bio = schema.tables[0]
        .columns
        .iter()
        .find(|c| c.name == "bio")
        .unwrap();
    assert_eq!(
        bio.data_type, "VARCHAR(10)",
        "ALTER COLUMN TYPE should update the column's data type"
    );
}

#[test]
fn alter_table_alter_column_set_and_drop_not_null() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT);
    ALTER TABLE users ALTER COLUMN email SET NOT NULL;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres).expect("parse should succeed");
    let email = schema.tables[0]
        .columns
        .iter()
        .find(|c| c.name == "email")
        .unwrap();
    assert!(
        !email.nullable,
        "SET NOT NULL should make the column NOT NULL"
    );

    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);
    ALTER TABLE users ALTER COLUMN email DROP NOT NULL;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres).expect("parse should succeed");
    let email = schema.tables[0]
        .columns
        .iter()
        .find(|c| c.name == "email")
        .unwrap();
    assert!(
        email.nullable,
        "DROP NOT NULL should make the column nullable"
    );
}

#[test]
fn alter_table_alter_column_drop_not_null_keeps_primary_key_not_null() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users ALTER COLUMN id DROP NOT NULL;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres).expect("parse should succeed");
    let id = &schema.tables[0].columns[0];
    assert!(
        !id.nullable,
        "primary-key columns stay NOT NULL even after DROP NOT NULL"
    );
}

#[test]
fn alter_table_alter_column_unknown_warns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users ALTER COLUMN ghost SET NOT NULL;
    ";

    let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, SqlDialect::Postgres);
    assert!(output.schema.is_some());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == codes::schema_unknown_column() && d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_modify_column_changes_type_and_nullability() {
    let sql = r"
    CREATE TABLE t (a INT);
    ALTER TABLE t MODIFY COLUMN a BIGINT NOT NULL;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let a = &schema.tables[0].columns[0];
    assert_eq!(a.data_type, "BIGINT", "MODIFY should update the data type");
    assert!(
        !a.nullable,
        "MODIFY ... NOT NULL should make the column NOT NULL"
    );
}

#[test]
fn alter_table_modify_column_without_not_null_is_nullable() {
    let sql = r"
    CREATE TABLE t (a INT NOT NULL);
    ALTER TABLE t MODIFY COLUMN a INT;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let a = &schema.tables[0].columns[0];
    assert!(
        a.nullable,
        "MySQL MODIFY redefines the column, so an omitted NOT NULL makes it nullable"
    );
}

#[test]
fn alter_table_modify_column_preserves_primary_key() {
    let sql = r"
    CREATE TABLE t (id INT PRIMARY KEY);
    ALTER TABLE t MODIFY COLUMN id BIGINT;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let id = &schema.tables[0].columns[0];
    assert_eq!(id.data_type, "BIGINT");
    assert!(
        id.is_primary_key,
        "MODIFY must not drop table-level primary-key membership"
    );
    assert!(!id.nullable, "primary-key columns remain NOT NULL");
}

#[test]
fn alter_table_modify_column_to_enum_populates_values() {
    let sql = r"
    CREATE TABLE t (status VARCHAR(10));
    ALTER TABLE t MODIFY COLUMN status ENUM('open', 'closed') NOT NULL;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let status = &schema.tables[0].columns[0];
    assert_eq!(
        status.enum_values.as_deref(),
        Some(["open".to_string(), "closed".to_string()].as_slice()),
        "MODIFY to ENUM should populate inline enum values"
    );
}

#[test]
fn alter_table_modify_unknown_column_warns() {
    let sql = r"
    CREATE TABLE t (a INT);
    ALTER TABLE t MODIFY COLUMN ghost BIGINT;
    ";

    let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, SqlDialect::Mysql);
    assert!(output.schema.is_some());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == codes::schema_unknown_column() && d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_change_column_renames_and_retypes() {
    let sql = r"
    CREATE TABLE t (a INT);
    ALTER TABLE t CHANGE COLUMN a b BIGINT NOT NULL;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let table = &schema.tables[0];
    assert!(!table.columns.iter().any(|c| c.name == "a"));
    let b = table.columns.iter().find(|c| c.name == "b").unwrap();
    assert_eq!(b.data_type, "BIGINT");
    assert!(!b.nullable);
}

#[test]
fn alter_table_change_column_updates_fk_and_index() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      org_id BIGINT,
      CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
    );
    CREATE INDEX idx_org ON users (org_id);
    ALTER TABLE users CHANGE COLUMN org_id organization_id BIGINT;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(users.columns.iter().any(|c| c.name == "organization_id"));
    assert!(
        users.foreign_keys[0]
            .from_columns
            .contains(&"organization_id".to_string()),
        "CHANGE COLUMN should update the local FK's from_columns"
    );
    assert!(
        users.indexes[0].column_names().contains(&"organization_id"),
        "CHANGE COLUMN should update index columns"
    );
}

#[test]
fn alter_table_modify_column_with_unique_adds_index() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, email VARCHAR(100));
    ALTER TABLE users MODIFY COLUMN email VARCHAR(255) UNIQUE;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let users = &schema.tables[0];
    let email = users.columns.iter().find(|c| c.name == "email").unwrap();
    assert_eq!(email.data_type, "VARCHAR(255)");
    assert!(
        users
            .indexes
            .iter()
            .any(|ix| ix.is_unique && ix.column_names() == vec!["email"]),
        "MODIFY ... UNIQUE should add a unique index on the column"
    );
}

#[test]
fn alter_table_change_column_with_inline_foreign_key_adds_fk() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (id BIGINT PRIMARY KEY, org BIGINT);
    ALTER TABLE users CHANGE COLUMN org org_id BIGINT REFERENCES orgs(id);
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Mysql).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert!(users.columns.iter().any(|c| c.name == "org_id"));
    assert!(
        users
            .foreign_keys
            .iter()
            .any(|fk| fk.from_columns == vec!["org_id".to_string()] && fk.to_table == "orgs"),
        "CHANGE ... REFERENCES should record the foreign key on the renamed column"
    );
}

#[test]
fn alter_table_change_unknown_column_warns() {
    let sql = r"
    CREATE TABLE t (a INT);
    ALTER TABLE t CHANGE COLUMN ghost phantom BIGINT;
    ";

    let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, SqlDialect::Mysql);
    assert!(output.schema.is_some());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == codes::schema_unknown_column() && d.message.contains("ghost"))
    );
}

#[test]
fn alter_table_rename_constraint_renames_foreign_key() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (
      id BIGINT PRIMARY KEY,
      org_id BIGINT,
      CONSTRAINT fk_org FOREIGN KEY (org_id) REFERENCES orgs(id)
    );
    ALTER TABLE users RENAME CONSTRAINT fk_org TO fk_organization;
    ";

    let schema =
        parse_sql_to_schema_with_dialect(sql, SqlDialect::Postgres).expect("parse should succeed");
    let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
    assert_eq!(
        users.foreign_keys[0].name.as_deref(),
        Some("fk_organization"),
        "RENAME CONSTRAINT should rename the foreign key"
    );
}

#[test]
fn alter_table_rename_unknown_constraint_warns() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    ALTER TABLE users RENAME CONSTRAINT ghost TO phantom;
    ";

    let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, SqlDialect::Postgres);
    assert!(output.schema.is_some());
    assert!(output.has_warnings());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ghost"))
    );
}

#[test]
fn drop_table_removes_table_from_schema() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE TABLE orders (id BIGINT PRIMARY KEY);
    DROP TABLE users;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].name, "orders");
}

#[test]
fn drop_table_recreate_works_after_drop() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    DROP TABLE users;
    CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT);
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    assert_eq!(schema.tables.len(), 1);
    assert!(
        schema.tables[0].columns.iter().any(|c| c.name == "email"),
        "second definition should be the active one"
    );
}

#[test]
fn drop_table_unknown_warns_without_if_exists() {
    let sql = "DROP TABLE missing;";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("DROP TABLE references unknown table"))
    );
}

#[test]
fn drop_table_if_exists_is_silent() {
    let sql = "DROP TABLE IF EXISTS missing;";

    let output = parse_sql_to_schema_with_diagnostics(sql);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("DROP TABLE references unknown table"))
    );
}

#[test]
fn drop_view_removes_view_from_schema() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE VIEW user_view AS SELECT id FROM users;
    DROP VIEW user_view;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    assert!(schema.views.is_empty(), "view should be dropped");
    assert_eq!(schema.tables.len(), 1);
}

#[test]
fn drop_type_removes_enum_from_schema() {
    let sql = r"
    CREATE TYPE status AS ENUM ('active', 'inactive');
    DROP TYPE status;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    assert!(schema.enums.is_empty(), "enum type should be dropped");
}

#[test]
fn drop_index_removes_named_index_from_table() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT);
    CREATE INDEX users_email_idx ON users (email);
    DROP INDEX users_email_idx;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    assert!(
        schema.tables[0].indexes.is_empty(),
        "named index should be removed"
    );
}

#[test]
fn drop_index_mysql_with_table_qualifier_removes_index() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, email VARCHAR(255));
    CREATE INDEX users_email_idx ON users (email);
    DROP INDEX users_email_idx ON users;
    ";

    let output =
        parse_sql_to_schema_with_diagnostics_and_dialect(sql, relune_core::SqlDialect::Mysql);
    let schema = output.schema.expect("schema parse");
    assert!(
        schema.tables[0].indexes.is_empty(),
        "MySQL DROP INDEX ... ON table should remove the index"
    );
}

#[test]
fn drop_index_schema_qualifier_only_drops_matching_schema() {
    let sql = r"
    CREATE TABLE public.users (id BIGINT PRIMARY KEY, email TEXT);
    CREATE TABLE analytics.users (id BIGINT PRIMARY KEY, email TEXT);
    CREATE INDEX users_email_idx ON public.users (email);
    CREATE INDEX users_email_idx ON analytics.users (email);
    DROP INDEX public.users_email_idx;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    let public_users = schema
        .tables
        .iter()
        .find(|t| t.schema_name.as_deref() == Some("public"))
        .expect("public.users present");
    let analytics_users = schema
        .tables
        .iter()
        .find(|t| t.schema_name.as_deref() == Some("analytics"))
        .expect("analytics.users present");
    assert!(
        public_users.indexes.is_empty(),
        "public.users index should be dropped"
    );
    assert_eq!(
        analytics_users.indexes.len(),
        1,
        "analytics.users index should be untouched"
    );
}

#[test]
fn drop_table_removes_inbound_foreign_keys() {
    let sql = r"
    CREATE TABLE orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE users (
        id BIGINT PRIMARY KEY,
        org_id BIGINT REFERENCES orgs (id)
    );
    DROP TABLE orgs;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    let users = schema
        .tables
        .iter()
        .find(|t| t.name == "users")
        .expect("users present");
    assert!(
        users.foreign_keys.is_empty(),
        "FK pointing at the dropped table should be removed; got {:?}",
        users.foreign_keys
    );
    let validation_errors = schema.validate();
    assert!(
        validation_errors.is_empty(),
        "schema should remain valid: {validation_errors:?}"
    );
}

#[test]
fn drop_schema_qualified_table_removes_qualified_inbound_fks() {
    let sql = r"
    CREATE TABLE auth.orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE public.orgs (id BIGINT PRIMARY KEY);
    CREATE TABLE app.users (
        id BIGINT PRIMARY KEY,
        org_id BIGINT REFERENCES auth.orgs (id)
    );
    CREATE TABLE app.members (
        id BIGINT PRIMARY KEY,
        org_id BIGINT REFERENCES public.orgs (id)
    );
    DROP TABLE auth.orgs;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    let users = schema
        .tables
        .iter()
        .find(|t| t.name == "users")
        .expect("users present");
    let members = schema
        .tables
        .iter()
        .find(|t| t.name == "members")
        .expect("members present");
    assert!(
        users.foreign_keys.is_empty(),
        "FK to auth.orgs should be removed"
    );
    assert_eq!(
        members.foreign_keys.len(),
        1,
        "FK to public.orgs should remain"
    );
}

#[test]
fn drop_unqualified_table_keeps_explicitly_qualified_inbound_fks() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    CREATE TABLE public.users (id BIGINT PRIMARY KEY);
    CREATE TABLE public.orders (
        id BIGINT PRIMARY KEY,
        user_id BIGINT REFERENCES public.users (id)
    );
    DROP TABLE users;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    let orders = schema
        .tables
        .iter()
        .find(|t| t.name == "orders")
        .expect("orders present");
    assert_eq!(
        orders.foreign_keys.len(),
        1,
        "FK explicitly targeting public.users must survive a DROP of bare users"
    );
}

#[test]
fn comment_on_table_with_null_clears_existing_comment() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY);
    COMMENT ON TABLE users IS 'old';
    COMMENT ON TABLE users IS NULL;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    assert_eq!(schema.tables[0].comment, None);
}

#[test]
fn comment_on_column_with_null_clears_existing_comment() {
    let sql = r"
    CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT);
    COMMENT ON COLUMN users.email IS 'old';
    COMMENT ON COLUMN users.email IS NULL;
    ";

    let schema = parse_sql_to_schema(sql).expect("schema parse");
    let email = schema.tables[0]
        .columns
        .iter()
        .find(|c| c.name == "email")
        .expect("email column");
    assert_eq!(email.comment, None);
}

#[test]
fn mysql_inline_column_comment_is_captured() {
    let sql = r"
    CREATE TABLE users (
        id BIGINT PRIMARY KEY,
        email VARCHAR(255) COMMENT 'login email'
    );
    ";

    let output =
        parse_sql_to_schema_with_diagnostics_and_dialect(sql, relune_core::SqlDialect::Mysql);
    let schema = output.schema.expect("schema parse");
    let email = schema.tables[0]
        .columns
        .iter()
        .find(|c| c.name == "email")
        .expect("email column");
    assert_eq!(email.comment.as_deref(), Some("login email"));
}
