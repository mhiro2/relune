//! Shared CLI input resolution helpers.

use std::fs;
use std::path::Path;

use relune_app::InputSource;
use relune_core::SqlDialect;

use crate::cli::{DiffArgs, DocArgs, ExportArgs, InspectArgs, LintArgs, RenderArgs};
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
        let selected = self.selected_count();
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

        if let Some(path) = self.sql {
            return read_sql_file(path, subject, dialect);
        }
        if let Some(text) = self.sql_text {
            return Ok(InputSource::sql_text_with_dialect(text.to_owned(), dialect));
        }
        if let Some(path) = self.schema_json {
            return read_schema_json_file(path, subject);
        }
        if let Some(url) = self.db_url {
            return Ok(InputSource::db_url(url.to_owned()));
        }

        unreachable!("validated input selection should always contain one item")
    }

    const fn selected_count(&self) -> usize {
        present(self.sql.is_some())
            + present(self.sql_text.is_some())
            + present(self.schema_json.is_some())
            + present(self.db_url.is_some())
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
    let content = fs::read_to_string(path).map_err(|error| {
        CliError::usage(anyhow::anyhow!(
            "Failed to read {subject} input file: {}: {error}",
            path.display()
        ))
    })?;

    if looks_like_schema_json(&content) {
        return ensure_input_file_metadata(path, "Failed to read schema JSON file")
            .map(|()| InputSource::schema_json_file(path));
    }

    ensure_input_file_metadata(path, &format!("Failed to read {subject} input file"))
        .map(|()| InputSource::sql_file_with_dialect(path, dialect))
}

fn looks_like_schema_json(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .is_some_and(|value| value.get("tables").is_some())
}

fn ensure_input_file_metadata(path: &Path, prefix: &str) -> CliResult<()> {
    std::fs::metadata(path)
        .map(|_| ())
        .map_err(|error| CliError::usage(anyhow::anyhow!("{prefix}: {}: {error}", path.display())))
}
