//! Relune CLI entry point.
//!
//! This crate provides the command-line interface for relune.

use std::process::ExitCode;

use clap::Parser;

mod cli;
mod commands;
mod config;
mod output;

use cli::{Cli, Command};
use commands::{run_diff, run_export, run_inspect, run_lint, run_render};
use config::ReluneConfig;

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
            ExitCode::from(get_exit_code(&e))
        }
    }
}

/// Load configuration file if specified, otherwise return default config.
fn load_config(config_path: Option<&std::path::Path>) -> anyhow::Result<ReluneConfig> {
    match config_path {
        Some(path) => ReluneConfig::from_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to load config file '{}': {}", path.display(), e)),
        None => Ok(ReluneConfig::default()),
    }
}

/// Run the specified command.
fn run_command(cli: Cli) -> anyhow::Result<()> {
    // Load config file if specified
    let config = load_config(cli.config.as_deref())?;

    match cli.command {
        Command::Render(args) => {
            run_render(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Inspect(args) => {
            run_inspect(&args, cli.color, &config)?;
        }
        Command::Export(args) => {
            run_export(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Lint(args) => {
            run_lint(&args, cli.color, &config)?;
        }
        Command::Diff(args) => {
            run_diff(&args, cli.color, cli.quiet, &config)?;
        }
        Command::Doctor => {
            run_doctor();
        }
    }
    Ok(())
}

/// Run the doctor command.
fn run_doctor() {
    println!("relune doctor: ok");
    println!("- cli: loaded");
    println!("- parser: wired");
    println!("- renderer: wired");
    println!("- app: wired");
    println!(
        "- wasm target: expected via cargo build -p relune-wasm --target wasm32-unknown-unknown"
    );
}

/// Setup logging based on verbosity and quiet flags.
fn setup_logging(verbose: u8, quiet: bool) {
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

    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .try_init();
}

/// Determine the exit code based on the error.
fn get_exit_code(error: &anyhow::Error) -> u8 {
    // Check for specific error types
    let error_string = error.to_string();

    // Check for invalid input/arguments or config errors
    if error_string.contains("At least one input option is required")
        || error_string.contains("Only one input option can be specified")
        || error_string.contains("Failed to read")
        || error_string.contains("not found")
        || error_string.contains("Failed to load config")
        || error_string.contains("Failed to parse config")
        || error_string.contains("Invalid config value")
        || error_string.contains("Export format must be provided")
    {
        return 2;
    }

    // Check for warnings with --fail-on-warning
    if error_string.contains("Warnings were emitted and --fail-on-warning is set") {
        return 3;
    }

    // Default to general error
    1
}
