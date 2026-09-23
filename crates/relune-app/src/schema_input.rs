//! Resolve [`relune_core::Schema`] from [`crate::request::InputSource`].

use std::fs::File;
use std::io::Read;
use std::path::Path;

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
    /// Concrete dialect resolved from the input, if any.
    ///
    /// Populated for SQL inputs (where the parser auto-detects when the
    /// caller passes `Auto`) and for DB URL inputs (derived from the URL
    /// scheme). `None` for schema-JSON inputs, which carry no parser
    /// dialect signal. Used by the review pipeline to promote `Auto` to
    /// the concrete dialect (`Postgres`, `Mysql`, or `Sqlite`) when both
    /// sides agree.
    pub resolved_dialect: Option<SqlDialect>,
}

/// Load a schema from the given input source.
pub(crate) fn schema_from_input(
    input: &InputSource,
) -> Result<(Schema, Vec<Diagnostic>), AppError> {
    let (schema, diagnostics, _context) = schema_from_input_with_context(input)?;
    Ok((schema, diagnostics))
}

/// Load a schema together with the resolved parser dialect.
///
/// Returns the same `(schema, diagnostics)` pair as
/// [`schema_from_input`], plus the concrete `SqlDialect` resolved by the
/// SQL parser (or derived from a DB URL scheme). Returns `None` for
/// schema-JSON inputs, which carry no parser dialect signal.
pub(crate) fn schema_from_input_with_dialect(
    input: &InputSource,
) -> Result<(Schema, Vec<Diagnostic>, Option<SqlDialect>), AppError> {
    let (schema, diagnostics, context) = schema_from_input_with_context(input)?;
    Ok((schema, diagnostics, context.resolved_dialect))
}

/// Load a schema plus resolved input capabilities.
pub(crate) fn schema_from_input_with_context(
    input: &InputSource,
) -> Result<(Schema, Vec<Diagnostic>, SchemaInputContext), AppError> {
    let (schema, mut diagnostics, context) = load_schema(input)?;
    diagnostics.extend(schema_validation_diagnostics(&schema));
    Ok((schema, diagnostics, context))
}

/// Convert structural validation errors from [`Schema::validate`] into
/// warning diagnostics so downstream consumers (CLI, WASM, action) can
/// surface them alongside parse-time issues.
fn schema_validation_diagnostics(schema: &Schema) -> Vec<Diagnostic> {
    schema
        .validate()
        .into_iter()
        .map(|error| {
            Diagnostic::warning(
                relune_core::diagnostic::codes::schema_validation(),
                error.to_string(),
            )
        })
        .collect()
}

fn load_schema(
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
                        resolved_dialect: Some(output.dialect),
                    },
                )),
                None => Err(AppError::input_with_type(
                    "sql_text",
                    "Failed to parse SQL: no schema produced",
                )),
            }
        }
        InputSource::SqlFile { path, dialect } => {
            let sql = read_input_file(path)?;
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
                        resolved_dialect: Some(output.dialect),
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
                    resolved_dialect: None,
                },
            ))
        }
        InputSource::SchemaJsonFile { path } => {
            let json = read_input_file(path)?;
            let export: relune_core::export::SchemaExport = serde_json::from_str(&json)?;
            let schema = relune_core::export::import_schema(&export)
                .map_err(|e| AppError::input_with_type("schema_json_file", e.to_string()))?;
            Ok((
                schema,
                vec![],
                SchemaInputContext {
                    supports_comment_review: false,
                    resolved_dialect: None,
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
                    resolved_dialect: dialect,
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

/// Read a SQL or schema JSON input file as UTF-8, enforcing
/// [`MAX_INPUT_FILE_SIZE_BYTES`] on the bytes actually read.
///
/// The file is opened once and its metadata is taken from the open handle,
/// so the checks apply to the same file that is read. Anything other than a
/// regular file (FIFOs, device files, directories) is rejected, and the read
/// itself is capped so a file that grows after the size check still cannot
/// exceed the limit.
pub fn read_input_file(path: &Path) -> Result<String, AppError> {
    let file = open_input_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(AppError::input_with_type(
            "file",
            format!("Input file '{}' is not a regular file", path.display()),
        ));
    }
    if metadata.len() > MAX_INPUT_FILE_SIZE_BYTES {
        return Err(file_too_large_error(path, metadata.len()));
    }

    let mut bytes = Vec::new();
    file.take(MAX_INPUT_FILE_SIZE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let read = bytes.len() as u64;
    if read > MAX_INPUT_FILE_SIZE_BYTES {
        return Err(file_too_large_error(path, read));
    }

    String::from_utf8(bytes).map_err(|_| {
        AppError::input_with_type(
            "file",
            format!("Input file '{}' is not valid UTF-8", path.display()),
        )
    })
}

/// Open without blocking on FIFOs so they can be rejected by the
/// regular-file check instead of hanging until a writer appears.
#[cfg(unix)]
fn open_input_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_input_file(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

fn file_too_large_error(path: &Path, size: u64) -> AppError {
    AppError::input_with_type(
        "file",
        format!(
            "Input file '{}' is too large: {} bytes exceeds the {} byte limit",
            path.display(),
            size,
            MAX_INPUT_FILE_SIZE_BYTES
        ),
    )
}

#[cfg(feature = "introspect")]
fn schema_from_db_url(url: &str) -> Result<Schema, AppError> {
    let trimmed = normalized_db_url(url)?;

    // This path is only used by the synchronous CLI entry point, which never
    // runs inside an existing Tokio runtime. Async callers must use
    // [`schema_from_db_url_async`] directly; the previous `block_in_place`
    // fallback panicked on current-thread runtimes and is therefore avoided.
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

        let err = read_input_file(temp.path()).expect_err("file size should be rejected");
        assert!(matches!(err, AppError::Input { .. }));
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn reads_input_file_at_the_size_limit() {
        let temp = tempfile::NamedTempFile::new().expect("create temp file");
        temp.as_file()
            .set_len(MAX_INPUT_FILE_SIZE_BYTES)
            .expect("sparse temp file");

        let content = read_input_file(temp.path()).expect("file at the limit should be read");
        assert_eq!(content.len() as u64, MAX_INPUT_FILE_SIZE_BYTES);
    }

    #[test]
    fn sql_file_input_is_read_through_the_bounded_reader() {
        let temp = tempfile::NamedTempFile::new().expect("create temp file");
        std::fs::write(temp.path(), "CREATE TABLE t (id INT PRIMARY KEY);").expect("write SQL");

        let input = InputSource::sql_file(temp.path());
        let (schema, _diagnostics) = schema_from_input(&input).expect("schema");
        assert_eq!(schema.tables.len(), 1);
    }

    #[test]
    fn rejects_non_utf8_input_files() {
        let temp = tempfile::NamedTempFile::new().expect("create temp file");
        std::fs::write(temp.path(), [0xff, 0xfe, 0x00]).expect("write bytes");

        let err = read_input_file(temp.path()).expect_err("invalid UTF-8 should be rejected");
        assert!(err.to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn rejects_directories_as_input_files() {
        let dir = tempfile::tempdir().expect("create temp dir");

        let err = read_input_file(dir.path()).expect_err("directory should be rejected");
        assert!(err.to_string().contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_fifo_input_files_without_blocking() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let fifo = dir.path().join("input.sql");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(status.success());

        let err = read_input_file(&fifo).expect_err("FIFO should be rejected");
        assert!(err.to_string().contains("not a regular file"));
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
