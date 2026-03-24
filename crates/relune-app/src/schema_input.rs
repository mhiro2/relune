//! Resolve [`relune_core::Schema`] from [`crate::request::InputSource`].

use relune_core::Schema;
use relune_parser_sql::parse_sql_to_schema_with_dialect;

use crate::error::AppError;
use crate::request::InputSource;

/// Load a schema from the given input source.
pub(crate) fn schema_from_input(input: &InputSource) -> Result<Schema, AppError> {
    match input {
        InputSource::SqlText { sql, dialect } => {
            Ok(parse_sql_to_schema_with_dialect(sql, *dialect)?)
        }
        InputSource::SqlFile { path, dialect } => {
            let sql = std::fs::read_to_string(path)?;
            Ok(parse_sql_to_schema_with_dialect(&sql, *dialect)?)
        }
        InputSource::SchemaJson { json } => {
            let export: relune_core::export::SchemaExport = serde_json::from_str(json)?;
            Ok(relune_core::export::import_schema(&export))
        }
        InputSource::SchemaJsonFile { path } => {
            let json = std::fs::read_to_string(path)?;
            let export: relune_core::export::SchemaExport = serde_json::from_str(&json)?;
            Ok(relune_core::export::import_schema(&export))
        }
        #[cfg(feature = "introspect")]
        InputSource::DbUrl { url } => schema_from_db_url(url),
    }
}

#[cfg(feature = "introspect")]
fn schema_from_db_url(url: &str) -> Result<Schema, AppError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::input("Database URL is empty"));
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Other(format!("Failed to start async runtime: {e}")))?
        .block_on(relune_introspect::introspect_database(trimmed))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::InputSource;

    #[test]
    fn from_sql_text() {
        let input = InputSource::sql_text("CREATE TABLE t (id INT PRIMARY KEY);");
        let schema = schema_from_input(&input).expect("schema");
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
