//! Inspect command implementation.

use anyhow::{Context, Result, bail};

use crate::cli::{ColorWhen, InspectArgs, InspectFormat};
use crate::config::ReluneConfig;
use crate::output::{DiagnosticPrinter, OutputWriter};
use relune_app::{
    InputSource, InspectFormat as AppInspectFormat, InspectRequest, format_inspect_text, inspect,
};

/// Run the inspect command.
pub fn run_inspect(args: &InspectArgs, color: ColorWhen, config: &ReluneConfig) -> Result<()> {
    // Merge config file with CLI args
    let merged = config.merge_inspect_args(args);

    // Resolve input source
    let input = resolve_input(args)?;

    // Build request using merged config
    let request = InspectRequest {
        input,
        table: args.table.clone(),
        format: match merged.format {
            InspectFormat::Text => AppInspectFormat::Text,
            InspectFormat::Json => AppInspectFormat::Json,
        },
    };

    // Execute inspect
    let result = inspect(request).context("Failed to inspect schema")?;

    // Print diagnostics
    let diag_printer = DiagnosticPrinter::new(color);
    diag_printer.print_all(&result.diagnostics);

    // Check for errors
    if DiagnosticPrinter::has_errors(&result.diagnostics) {
        bail!("Errors were encountered during inspection");
    }

    // Format output using merged config
    let output = match merged.format {
        InspectFormat::Json => {
            serde_json::to_string_pretty(&result).context("Failed to serialize result to JSON")?
        }
        InspectFormat::Text => format_inspect_text(&result),
    };

    // Write output (always to stdout for inspect)
    let mut writer = OutputWriter::new(None, color).context("Failed to create output writer")?;
    writer.write(&output).context("Failed to write output")?;

    Ok(())
}

/// Resolve input source from CLI arguments.
fn resolve_input(args: &InspectArgs) -> Result<InputSource> {
    let count = args.sql.iter().count()
        + args.sql_text.iter().count()
        + args.schema_json.iter().count()
        + args.db_url.iter().count();

    if count == 0 {
        bail!(
            "At least one input option is required: --sql, --sql-text, --schema-json, or --db-url"
        );
    }

    if count > 1 {
        bail!(
            "Only one input option can be specified: --sql, --sql-text, --schema-json, or --db-url"
        );
    }

    let dialect = args.dialect.into();

    if let Some(ref path) = args.sql {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read SQL file: {}", path.display()))?;
        return Ok(InputSource::sql_text_with_dialect(content, dialect));
    }

    if let Some(ref text) = args.sql_text {
        return Ok(InputSource::sql_text_with_dialect(text.clone(), dialect));
    }

    if let Some(ref path) = args.schema_json {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read schema JSON file: {}", path.display()))?;
        return Ok(InputSource::schema_json(content));
    }

    if let Some(ref url) = args.db_url {
        return Ok(InputSource::db_url(url.clone()));
    }

    unreachable!()
}
