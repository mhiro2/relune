//! Resolve [`relune_core::Schema`] from [`crate::request::InputSource`].

use relune_core::{Diagnostic, Schema, SqlDialect};
use relune_parser_sql::parse_sql_to_schema_with_diagnostics_and_dialect;
use tracing::info;

use crate::error::AppError;
use crate::request::InputSource;

/// Maximum size for file-based SQL and schema JSON inputs.
pub const MAX_INPUT_FILE_SIZE_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum size for direct text/JSON input (same limit as file input for consistency).
const MAX_TEXT_INPUT_SIZE_BYTES: usize = 8 * 1024 * 1024;

/// Extra context resolved while loading schema input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SchemaInputContext {
    /// Whether table/column comments can be reviewed reliably for this input.
    pub supports_comment_review: bool,
}

/// Load a schema from the given input source.
pub(crate) fn schema_from_input(
    input: &InputSource,
) -> Result<(Schema, Vec<Diagnostic>), AppError> {
    let (schema, diagnostics, _context) = schema_from_input_with_context(input)?;
    Ok((schema, diagnostics))
}

/// Load a schema plus resolved input capabilities.
pub(crate) fn schema_from_input_with_context(
    input: &InputSource,
) -> Result<(Schema, Vec<Diagnostic>, SchemaInputContext), AppError> {
    match input {
        InputSource::SqlText { sql, dialect } => {
            ensure_text_size_within_limit(sql.len(), "SQL text")?;
            let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, *dialect);
            info!(
                requested_dialect = %dialect,
                resolved_dialect = %output.dialect,
                diagnostics = output.diagnostics.len(),
                tables = output.schema.as_ref().map_or(0, |schema| schema.tables.len()),
                "parsed SQL text input"
            );
            match output.schema {
                Some(schema) => Ok((
                    schema,
                    output.diagnostics,
                    SchemaInputContext {
                        supports_comment_review: supports_comment_review_for_sql(output.dialect),
                    },
                )),
                None => Err(AppError::input_with_type(
                    "sql_text",
                    "Failed to parse SQL: no schema produced",
                )),
            }
        }
        InputSource::SqlFile { path, dialect } => {
            ensure_file_size_within_limit(path)?;
            let sql = std::fs::read_to_string(path)?;
            let output = parse_sql_to_schema_with_diagnostics_and_dialect(&sql, *dialect);
            info!(
                path = %path.display(),
                requested_dialect = %dialect,
                resolved_dialect = %output.dialect,
                diagnostics = output.diagnostics.len(),
                tables = output.schema.as_ref().map_or(0, |schema| schema.tables.len()),
                "parsed SQL file input"
            );
            match output.schema {
                Some(schema) => Ok((
                    schema,
                    output.diagnostics,
                    SchemaInputContext {
                        supports_comment_review: supports_comment_review_for_sql(output.dialect),
                    },
                )),
                None => Err(AppError::input_with_type(
                    "sql_file",
                    "Failed to parse SQL: no schema produced",
                )),
            }
        }
        InputSource::SchemaJson { json } => {
            ensure_text_size_within_limit(json.len(), "Schema JSON")?;
            let export: relune_core::export::SchemaExport = serde_json::from_str(json)?;
            let schema = relune_core::export::import_schema(&export)
                .map_err(|e| AppError::input_with_type("schema_json", e.to_string()))?;
            Ok((
                schema,
                vec![],
                SchemaInputContext {
                    supports_comment_review: false,
                },
            ))
        }
        InputSource::SchemaJsonFile { path } => {
            ensure_file_size_within_limit(path)?;
            let json = std::fs::read_to_string(path)?;
            let export: relune_core::export::SchemaExport = serde_json::from_str(&json)?;
            let schema = relune_core::export::import_schema(&export)
                .map_err(|e| AppError::input_with_type("schema_json_file", e.to_string()))?;
            Ok((
                schema,
                vec![],
                SchemaInputContext {
                    supports_comment_review: false,
                },
            ))
        }
        #[cfg(feature = "introspect")]
        InputSource::DbUrl { url } => {
            let schema = schema_from_db_url(url)?;
            let dialect = dialect_from_db_url(url);
            Ok((
                schema,
                vec![],
                SchemaInputContext {
                    supports_comment_review: dialect.is_some_and(supports_comment_review_for_db),
                },
            ))
        }
    }
}

const fn supports_comment_review_for_sql(dialect: SqlDialect) -> bool {
    matches!(dialect, SqlDialect::Postgres)
}

#[cfg(feature = "introspect")]
const fn supports_comment_review_for_db(dialect: SqlDialect) -> bool {
    matches!(dialect, SqlDialect::Postgres | SqlDialect::Mysql)
}

#[cfg(feature = "introspect")]
fn dialect_from_db_url(url: &str) -> Option<SqlDialect> {
    let trimmed = url.trim().to_ascii_lowercase();
    if trimmed.starts_with("postgres://") || trimmed.starts_with("postgresql://") {
        Some(SqlDialect::Postgres)
    } else if trimmed.starts_with("mysql://") || trimmed.starts_with("mariadb://") {
        Some(SqlDialect::Mysql)
    } else if trimmed.starts_with("sqlite:") {
        Some(SqlDialect::Sqlite)
    } else {
        None
    }
}

fn ensure_text_size_within_limit(size: usize, input_type: &str) -> Result<(), AppError> {
    if size > MAX_TEXT_INPUT_SIZE_BYTES {
        return Err(AppError::input_with_type(
            input_type,
            format!(
                "{input_type} is too large: {size} bytes exceeds the {MAX_TEXT_INPUT_SIZE_BYTES} byte limit"
            ),
        ));
    }
    Ok(())
}

fn ensure_file_size_within_limit(path: &std::path::Path) -> Result<(), AppError> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_INPUT_FILE_SIZE_BYTES {
        return Err(AppError::input_with_type(
            "file",
            format!(
                "Input file '{}' is too large: {} bytes exceeds the {} byte limit",
                path.display(),
                size,
                MAX_INPUT_FILE_SIZE_BYTES
            ),
        ));
    }

    Ok(())
}

#[cfg(feature = "introspect")]
fn schema_from_db_url(url: &str) -> Result<Schema, AppError> {
    let trimmed = normalized_db_url(url)?;

    // If we're already inside a Tokio runtime, use it directly via
    // spawn_blocking → block_in_place fallback instead of creating a
    // second runtime (which would panic).
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return tokio::task::block_in_place(|| handle.block_on(schema_from_db_url_impl(trimmed)));
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            AppError::other(
                "async runtime",
                format!("Failed to start async runtime: {e}"),
            )
        })?
        .block_on(schema_from_db_url_impl(trimmed))
}

/// Async version of database introspection.
///
/// Use this when you already have a Tokio runtime (e.g. in a server or
/// worker context). The synchronous [`schema_from_input`] calls this
/// internally and creates a runtime only when one is not already active.
#[cfg(feature = "introspect")]
pub async fn schema_from_db_url_async(url: &str) -> Result<Schema, AppError> {
    let trimmed = normalized_db_url(url)?;

    schema_from_db_url_impl(trimmed).await
}

#[cfg(feature = "introspect")]
async fn schema_from_db_url_impl(url: &str) -> Result<Schema, AppError> {
    relune_introspect::introspect_database(url)
        .await
        .map_err(|error| sanitized_introspect_error(url, error))
}

#[cfg(feature = "introspect")]
fn normalized_db_url(url: &str) -> Result<&str, AppError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::input_with_type("db_url", "Database URL is empty"));
    }

    Ok(trimmed)
}

#[cfg(feature = "introspect")]
fn sanitized_introspect_error(url: &str, error: relune_introspect::IntrospectError) -> AppError {
    AppError::Introspect(error.sanitized_for_url(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::request::InputSource;

    #[test]
    fn from_sql_text() {
        let input = InputSource::sql_text("CREATE TABLE t (id INT PRIMARY KEY);");
        let (schema, _diagnostics) = schema_from_input(&input).expect("schema");
        assert_eq!(schema.tables.len(), 1);
    }

    #[test]
    fn rejects_invalid_sql_text() {
        let input = InputSource::sql_text("THIS IS NOT VALID SQL");
        let err = schema_from_input(&input).expect_err("invalid SQL should fail");
        assert!(matches!(err, AppError::Input { .. } | AppError::Parse(_)));
    }

    #[test]
    fn comment_only_sql_returns_empty_schema_warning() {
        let input = InputSource::sql_text("-- comments only");
        let (schema, diagnostics) = schema_from_input(&input).expect("schema");

        assert!(schema.tables.is_empty());
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == relune_core::diagnostic::codes::parse_empty_schema()
        }));
    }

    #[test]
    fn rejects_malformed_schema_json() {
        let input = InputSource::schema_json("{\"tables\":");
        let err = schema_from_input(&input).expect_err("malformed JSON should fail");
        assert!(matches!(err, AppError::Json(_)));
    }

    #[test]
    fn rejects_oversized_input_files() {
        let temp = tempfile::Builder::new()
            .prefix("relune-schema-input-")
            .suffix(".sql")
            .tempfile()
            .expect("create temp file");
        temp.as_file()
            .set_len(MAX_INPUT_FILE_SIZE_BYTES + 1)
            .expect("sparse temp file");

        let err =
            ensure_file_size_within_limit(temp.path()).expect_err("file size should be rejected");
        assert!(matches!(err, AppError::Input { .. }));
    }

    #[cfg(feature = "introspect")]
    #[test]
    fn db_url_rejects_unknown_scheme_without_network() {
        let input = InputSource::db_url("ftp://example/db");
        let err = schema_from_input(&input).expect_err("expected invalid URL");
        match err {
            AppError::Introspect(relune_introspect::IntrospectError::InvalidDatabaseUrl(_)) => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "introspect")]
    #[test]
    fn db_url_sync_path_rejects_empty_after_trim() {
        let input = InputSource::db_url("   ");
        let err = schema_from_input(&input).expect_err("empty database URL should fail");

        match err {
            AppError::Input {
                input_type,
                message,
            } => {
                assert_eq!(input_type, "db_url");
                assert!(message.contains("empty"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "introspect")]
    #[test]
    fn db_url_errors_are_sanitized_before_reaching_app_error() {
        let url = "postgres://user:secret@localhost/db?password=hunter2&token=abc";
        let err = sanitized_introspect_error(
            url,
            relune_introspect::IntrospectError::connection(format!("failed to connect to {url}")),
        );

        let message = err.to_string();
        assert!(message.contains("postgres://***:***@localhost/db?password=***&token=***"));
        assert!(!message.contains("secret"));
        assert!(!message.contains("hunter2"));
        assert!(!message.contains("token=abc"));
    }
}
