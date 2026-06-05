//! Shared CLI input resolution helpers.

use std::fs;
use std::path::Path;

use anyhow::anyhow;
use relune_app::InputSource;
use relune_core::SqlDialect;

use crate::cli::{DiffArgs, DocArgs, ExportArgs, InspectArgs, LintArgs, RenderArgs, ReviewArgs};
use crate::error::{CliError, CliResult};

/// Input selection resolved from a CLI command.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InputSelection<'a> {
    sql: Option<&'a Path>,
    sql_text: Option<&'a str>,
    schema_json: Option<&'a Path>,
    db_url: Option<&'a str>,
}

impl<'a> InputSelection<'a> {
    /// Create a new selection from the available input fields.
    #[must_use]
    pub(crate) const fn new(
        sql: Option<&'a Path>,
        sql_text: Option<&'a str>,
        schema_json: Option<&'a Path>,
        db_url: Option<&'a str>,
    ) -> Self {
        Self {
            sql,
            sql_text,
            schema_json,
            db_url,
        }
    }

    /// Resolve the selected input into an app-level `InputSource`.
    pub(crate) fn resolve(
        self,
        dialect: SqlDialect,
        subject: &'static str,
    ) -> CliResult<InputSource> {
        // Fall back to the DATABASE_URL environment variable only when no input
        // flag was given. Passing the DSN via `--db-url` leaks it into argv
        // (`ps`) and shell history, so the env var is the safer default for
        // live-DB introspection.
        let env_db_url = std::env::var("DATABASE_URL").ok();
        let db_url = effective_db_url(
            self.db_url,
            self.has_explicit_input(),
            env_db_url.as_deref(),
        );

        let selected = present(self.sql.is_some())
            + present(self.sql_text.is_some())
            + present(self.schema_json.is_some())
            + present(db_url.is_some());
        if selected == 0 {
            return Err(CliError::usage(anyhow::anyhow!(
                "No input source was provided. Provide an input (e.g. --sql or --db-url) or set the DATABASE_URL environment variable."
            )));
        }
        if selected > 1 {
            return Err(CliError::usage(anyhow::anyhow!(
                "Only one input source can be specified."
            )));
        }

        if let Some(path) = self.sql {
            return read_sql_file(path, subject, dialect);
        }
        if let Some(text) = self.sql_text {
            return Ok(InputSource::sql_text_with_dialect(text.to_owned(), dialect));
        }
        if let Some(path) = self.schema_json {
            return read_schema_json_file(path, subject);
        }
        if let Some(url) = db_url {
            return Ok(InputSource::db_url(url.to_owned()));
        }

        unreachable!("validated input selection should always contain one item")
    }

    /// Whether any non-`db_url` input flag was provided.
    const fn has_explicit_input(&self) -> bool {
        self.sql.is_some() || self.sql_text.is_some() || self.schema_json.is_some()
    }

    /// Build a selection for `render`/`inspect`/`export`.
    #[must_use]
    pub(crate) fn from_render(args: &'a RenderArgs) -> Self {
        Self::new(
            args.sql.as_deref(),
            args.sql_text.as_deref(),
            args.schema_json.as_deref(),
            args.db_url.as_deref(),
        )
    }

    /// Build a selection for `inspect`.
    #[must_use]
    pub(crate) fn from_inspect(args: &'a InspectArgs) -> Self {
        Self::new(
            args.sql.as_deref(),
            args.sql_text.as_deref(),
            args.schema_json.as_deref(),
            args.db_url.as_deref(),
        )
    }

    /// Build a selection for `export`.
    #[must_use]
    pub(crate) fn from_export(args: &'a ExportArgs) -> Self {
        Self::new(
            args.sql.as_deref(),
            args.sql_text.as_deref(),
            args.schema_json.as_deref(),
            args.db_url.as_deref(),
        )
    }

    /// Build a selection for `doc`.
    #[must_use]
    pub(crate) fn from_doc(args: &'a DocArgs) -> Self {
        Self::new(
            args.sql.as_deref(),
            args.sql_text.as_deref(),
            args.schema_json.as_deref(),
            args.db_url.as_deref(),
        )
    }

    /// Build a selection for `lint`.
    #[must_use]
    pub(crate) fn from_lint(args: &'a LintArgs) -> Self {
        Self::new(
            args.sql.as_deref(),
            None,
            args.schema_json.as_deref(),
            args.db_url.as_deref(),
        )
    }
}

const fn present(value: bool) -> usize {
    if value { 1 } else { 0 }
}

/// Resolve the effective db-url.
///
/// The explicit `--db-url` flag always wins. Otherwise the `DATABASE_URL`
/// environment value is used only when no other input flag was provided, so a
/// stray `DATABASE_URL` never shadows an explicit `--sql`/`--schema-json`.
fn effective_db_url<'a>(
    flag: Option<&'a str>,
    has_explicit_input: bool,
    env_value: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(url) = flag {
        return Some(url);
    }
    if has_explicit_input {
        return None;
    }
    env_value.filter(|value| !value.trim().is_empty())
}

fn read_sql_file(path: &Path, _subject: &str, dialect: SqlDialect) -> CliResult<InputSource> {
    ensure_input_file_metadata(path, "Failed to read SQL file")?;
    Ok(InputSource::sql_file_with_dialect(path, dialect))
}

fn read_schema_json_file(path: &Path, _subject: &str) -> CliResult<InputSource> {
    ensure_input_file_metadata(path, "Failed to read schema JSON file")?;
    Ok(InputSource::schema_json_file(path))
}

/// Input selection resolved from a `diff` command side.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DiffInputSelection<'a> {
    file: Option<&'a Path>,
    sql_text: Option<&'a str>,
    schema_json: Option<&'a Path>,
}

impl<'a> DiffInputSelection<'a> {
    /// Create the `before` selection for `diff`.
    #[must_use]
    pub(crate) fn from_before(args: &'a DiffArgs) -> Self {
        Self {
            file: args.before.as_deref(),
            sql_text: args.before_sql_text.as_deref(),
            schema_json: args.before_schema_json.as_deref(),
        }
    }

    /// Create the `after` selection for `diff`.
    #[must_use]
    pub(crate) fn from_after(args: &'a DiffArgs) -> Self {
        Self {
            file: args.after.as_deref(),
            sql_text: args.after_sql_text.as_deref(),
            schema_json: args.after_schema_json.as_deref(),
        }
    }

    /// Create the `before` selection for `review`.
    #[must_use]
    pub(crate) fn from_review_before(args: &'a ReviewArgs) -> Self {
        Self {
            file: args.before.as_deref(),
            sql_text: args.before_sql_text.as_deref(),
            schema_json: args.before_schema_json.as_deref(),
        }
    }

    /// Create the `after` selection for `review`.
    #[must_use]
    pub(crate) fn from_review_after(args: &'a ReviewArgs) -> Self {
        Self {
            file: args.after.as_deref(),
            sql_text: args.after_sql_text.as_deref(),
            schema_json: args.after_schema_json.as_deref(),
        }
    }

    /// Resolve the selected input into an app-level `InputSource`.
    pub(crate) fn resolve(
        self,
        dialect: SqlDialect,
        subject: &'static str,
    ) -> CliResult<InputSource> {
        let selected = usize::from(self.file.is_some())
            + usize::from(self.sql_text.is_some())
            + usize::from(self.schema_json.is_some());
        if selected == 0 {
            return Err(CliError::usage(anyhow::anyhow!(
                "No {subject} input option was selected"
            )));
        }
        if selected > 1 {
            return Err(CliError::usage(anyhow::anyhow!(
                "Only one {subject} input option can be specified"
            )));
        }

        if let Some(path) = self.file {
            return read_sniffed_file(path, subject, dialect);
        }
        if let Some(text) = self.sql_text {
            return Ok(InputSource::sql_text_with_dialect(text.to_owned(), dialect));
        }
        if let Some(path) = self.schema_json {
            return read_schema_json_file(path, subject);
        }

        unreachable!("validated diff input selection should always contain one item")
    }
}

fn read_sniffed_file(path: &Path, subject: &str, dialect: SqlDialect) -> CliResult<InputSource> {
    ensure_input_file_metadata(path, &format!("Failed to read {subject} input file"))?;

    let content = fs::read_to_string(path).map_err(|error| {
        CliError::usage(anyhow::anyhow!(
            "Failed to read {subject} input file: {}: {error}",
            path.display()
        ))
    })?;

    if looks_like_schema_json(&content) {
        Ok(InputSource::schema_json(content))
    } else {
        Ok(InputSource::sql_text_with_dialect(content, dialect))
    }
}

fn looks_like_schema_json(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .is_some_and(|value| value.get("tables").is_some())
}

fn ensure_input_file_metadata(path: &Path, prefix: &str) -> CliResult<()> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| CliError::usage(anyhow!("{prefix}: {}: {error}", path.display())))?;

    if metadata.len() > relune_app::MAX_INPUT_FILE_SIZE_BYTES {
        return Err(CliError::usage(anyhow!(
            "Input file '{}' is too large: {} bytes exceeds the {} byte limit",
            path.display(),
            metadata.len(),
            relune_app::MAX_INPUT_FILE_SIZE_BYTES
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::effective_db_url;

    #[test]
    fn explicit_db_url_flag_wins_over_env() {
        assert_eq!(
            effective_db_url(Some("postgres://flag"), false, Some("postgres://env")),
            Some("postgres://flag")
        );
    }

    #[test]
    fn env_is_used_when_no_input_flag_is_given() {
        assert_eq!(
            effective_db_url(None, false, Some("postgres://env")),
            Some("postgres://env")
        );
    }

    #[test]
    fn explicit_non_db_input_suppresses_env_fallback() {
        // A stray DATABASE_URL must not shadow `--sql`/`--schema-json`.
        assert_eq!(effective_db_url(None, true, Some("postgres://env")), None);
    }

    #[test]
    fn blank_env_value_is_ignored() {
        assert_eq!(effective_db_url(None, false, Some("   ")), None);
        assert_eq!(effective_db_url(None, false, Some("")), None);
        assert_eq!(effective_db_url(None, false, None), None);
    }
}
