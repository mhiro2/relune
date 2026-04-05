//! Relune CLI entry point.
//!
//! This crate provides the command-line interface for relune.

use std::process::ExitCode;

use clap::Parser;

mod cli;
mod commands;
mod config;
mod error;
mod output;
mod png;

use cli::{Cli, Command};
use commands::{run_diff, run_doc, run_export, run_inspect, run_lint, run_render};
use config::ReluneConfig;
use error::{CliError, CliResult};

fn main() -> ExitCode {
    // Parse command line arguments
    let cli = Cli::parse();

    // Configure logging based on verbosity
    setup_logging(cli.verbose, cli.quiet);

    // Run the command
    match run_command(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Print error to stderr
            eprintln!("Error: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}

/// Load configuration file if specified, otherwise return default config.
fn load_config(config_path: Option<&std::path::Path>) -> CliResult<ReluneConfig> {
    match config_path {
        Some(path) => ReluneConfig::from_file(path).map_err(|e| {
            CliError::usage(anyhow::anyhow!(
                "Failed to load config file '{}': {}",
                path.display(),
                e
            ))
        }),
        None => Ok(ReluneConfig::default()),
    }
}

/// Run the specified command.
fn run_command(cli: Cli) -> CliResult<()> {
    // Load config file if specified
    let config = load_config(cli.config.as_deref())?;

    match cli.command {
        Command::Render(args) => {
            run_render(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Inspect(args) => {
            run_inspect(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Doc(args) => {
            run_doc(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Export(args) => {
            run_export(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Lint(args) => {
            run_lint(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Diff(args) => {
            run_diff(&args, cli.color, cli.quiet, &config)?;
        }
    }
    Ok(())
}

/// Setup logging based on verbosity and quiet flags.
fn setup_logging(verbose: u8, quiet: bool) {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = if quiet {
        EnvFilter::new("error")
    } else {
        match verbose {
            0 => EnvFilter::new("warn"),
            1 => EnvFilter::new("info"),
            2 => EnvFilter::new("debug"),
            _ => EnvFilter::new("trace"),
        }
    };

    let span_events = if verbose >= 3 {
        FmtSpan::NEW | FmtSpan::CLOSE
    } else {
        FmtSpan::NONE
    };

    let _ = fmt()
        .with_env_filter(filter)
        .with_span_events(span_events)
        .with_target(verbose >= 2)
        .with_writer(std::io::stderr)
        .without_time()
        .try_init();
}
