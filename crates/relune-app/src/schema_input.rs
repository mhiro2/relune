//! Resolve [`relune_core::Schema`] from [`crate::request::InputSource`].

use relune_core::{Diagnostic, Schema};
use relune_parser_sql::parse_sql_to_schema_with_diagnostics_and_dialect;
use tracing::info;

use crate::error::AppError;
use crate::request::InputSource;

/// Load a schema from the given input source.
pub(crate) fn schema_from_input(
    input: &InputSource,
) -> Result<(Schema, Vec<Diagnostic>), AppError> {
    match input {
        InputSource::SqlText { sql, dialect } => {
            let output = parse_sql_to_schema_with_diagnostics_and_dialect(sql, *dialect);
            info!(
                requested_dialect = %dialect,
                resolved_dialect = %output.dialect,
                diagnostics = output.diagnostics.len(),
                tables = output.schema.as_ref().map_or(0, |schema| schema.tables.len()),
                "parsed SQL text input"
            );
            match output.schema {
                Some(schema) => Ok((schema, output.diagnostics)),
                None => Err(AppError::input("Failed to parse SQL: no schema produced")),
            }
        }
        InputSource::SqlFile { path, dialect } => {
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
                Some(schema) => Ok((schema, output.diagnostics)),
                None => Err(AppError::input("Failed to parse SQL: no schema produced")),
            }
        }
        InputSource::SchemaJson { json } => {
            let export: relune_core::export::SchemaExport = serde_json::from_str(json)?;
            Ok((relune_core::export::import_schema(&export), vec![]))
        }
        InputSource::SchemaJsonFile { path } => {
            let json = std::fs::read_to_string(path)?;
            let export: relune_core::export::SchemaExport = serde_json::from_str(&json)?;
            Ok((relune_core::export::import_schema(&export), vec![]))
        }
        #[cfg(feature = "introspect")]
        InputSource::DbUrl { url } => {
            let schema = schema_from_db_url(url)?;
            Ok((schema, vec![]))
        }
    }
}

#[cfg(feature = "introspect")]
fn schema_from_db_url(url: &str) -> Result<Schema, AppError> {
    // If we're already inside a Tokio runtime, use it directly via
    // spawn_blocking → block_in_place fallback instead of creating a
    // second runtime (which would panic).
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return tokio::task::block_in_place(|| handle.block_on(schema_from_db_url_async(url)));
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Other(format!("Failed to start async runtime: {e}")))?
        .block_on(schema_from_db_url_async(url))
}

/// Async version of database introspection.
///
/// Use this when you already have a Tokio runtime (e.g. in a server or
/// worker context). The synchronous [`schema_from_input`] calls this
/// internally and creates a runtime only when one is not already active.
#[cfg(feature = "introspect")]
pub async fn schema_from_db_url_async(url: &str) -> Result<Schema, AppError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::input("Database URL is empty"));
    }

    relune_introspect::introspect_database(trimmed)
        .await
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::InputSource;

    #[test]
    fn from_sql_text() {
        let input = InputSource::sql_text("CREATE TABLE t (id INT PRIMARY KEY);");
        let (schema, _diagnostics) = schema_from_input(&input).expect("schema");
        assert_eq!(schema.tables.len(), 1);
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
}
