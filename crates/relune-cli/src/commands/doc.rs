//! Doc command implementation.

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, DocArgs};
use crate::config::ReluneConfig;
use crate::error::CliResult;
use crate::output::{check_diagnostics, print_success, write_output};
use relune_app::{DocRequest, doc};

/// Run the doc command.
pub fn run_doc(
    args: &DocArgs,
    color: ColorWhen,
    quiet: bool,
    _config: &ReluneConfig,
) -> CliResult<()> {
    // Resolve input source
    let input = InputSelection::from_doc(args).resolve(args.dialect.into(), "input")?;

    // Build request
    let request = DocRequest {
        input,
        output_path: args.out.clone(),
        ..Default::default()
    };

    // Execute doc generation
    let result = doc(request).context("Failed to generate documentation")?;

    check_diagnostics(&result.diagnostics, color, false)?;
    write_output(&result.content, args.out.as_deref(), color)?;

    // Print success message (unless quiet)
    if !quiet && let Some(ref out_path) = args.out {
        print_success(&format!("Generated {}", out_path.display()), color);
    }

    Ok(())
}
