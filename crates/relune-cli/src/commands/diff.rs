//! Diff command implementation.

use anyhow::{Context, Result, bail};

use crate::cli::{ColorWhen, DiffArgs, DiffFormat};
use crate::config::ReluneConfig;
use crate::output::{DiagnosticPrinter, OutputWriter, print_success};
use relune_app::{DiffRequest, InputSource, diff, format_diff_text};

/// Run the diff command.
pub fn run_diff(
    args: &DiffArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> Result<()> {
    let merged = config.merge_diff_args(args);
    let dialect = merged.dialect.into();

    // Resolve before input source
    let before = resolve_input(
        args.before.as_ref(),
        args.before_sql_text.as_ref(),
        args.before_schema_json.as_ref(),
        "before",
        dialect,
    )?;

    // Resolve after input source
    let after = resolve_input(
        args.after.as_ref(),
        args.after_sql_text.as_ref(),
        args.after_schema_json.as_ref(),
        "after",
        dialect,
    )?;

    // Build request
    let request = DiffRequest {
        before,
        after,
        format: match merged.format {
            DiffFormat::Text => relune_app::DiffFormat::Text,
            DiffFormat::Json => relune_app::DiffFormat::Json,
        },
        output_path: args.out.clone(),
    };

    // Execute diff
    let result = diff(request).context("Failed to compute schema diff")?;

    // Print diagnostics
    let diag_printer = DiagnosticPrinter::new(color);
    diag_printer.print_all(&result.diagnostics);

    // Check for errors
    if DiagnosticPrinter::has_errors(&result.diagnostics) {
        bail!("Errors were encountered during diff computation");
    }

    // Format output
    let content = match merged.format {
        DiffFormat::Text => format_diff_text(&result),
        DiffFormat::Json => serde_json::to_string_pretty(&result.diff)
            .context("Failed to serialize diff to JSON")?,
    };

    // Write output
    let mut writer =
        OutputWriter::new(args.out.as_deref(), color).context("Failed to create output writer")?;
    writer.write(&content).context("Failed to write output")?;
    writer.finish().context("Failed to finalize output")?;

    // Print success message (unless quiet)
    if !quiet && let Some(ref out_path) = args.out {
        let change_summary = if result.has_changes() {
            let s = &result.diff.summary;
            format!(
                "{} added, {} removed, {} modified",
                s.tables_added, s.tables_removed, s.tables_modified
            )
        } else {
            "no changes".to_string()
        };
        print_success(
            &format!(
                "Diff output written to {} ({})",
                out_path.display(),
                change_summary
            ),
            color,
        );
    }

    Ok(())
}

/// Resolve input source from CLI arguments.
fn resolve_input(
    file_path: Option<&std::path::PathBuf>,
    sql_text: Option<&String>,
    schema_json: Option<&std::path::PathBuf>,
    context: &str,
    dialect: relune_core::SqlDialect,
) -> Result<InputSource> {
    let count = file_path.iter().count() + sql_text.iter().count() + schema_json.iter().count();

    if count == 0 {
        bail!(
            "At least one {context} input option is required: --{context}, --{context}-sql-text, or --{context}-schema-json"
        );
    }

    if count > 1 {
        bail!("Only one {context} input option can be specified");
    }

    if let Some(path) = file_path {
        // Determine if it's SQL or JSON based on extension, or try to parse
        let file_content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {} file: {}", context, path.display()))?;

        // Check file extension to determine type
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if extension == "json" {
            return Ok(InputSource::schema_json(file_content));
        }
        // Default to SQL for .sql files and unknown extensions
        return Ok(InputSource::sql_text_with_dialect(file_content, dialect));
    }

    if let Some(text) = sql_text {
        return Ok(InputSource::sql_text_with_dialect(text.clone(), dialect));
    }

    if let Some(path) = schema_json {
        let schema_content = std::fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read {} schema JSON file: {}",
                context,
                path.display()
            )
        })?;
        return Ok(InputSource::schema_json(schema_content));
    }

    unreachable!()
}
