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
use commands::{run_diff, run_doc, run_export, run_inspect, run_lint, run_render, run_review};
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
            // DiffChangesDetected is a signaling exit, not an error
            if !matches!(e, CliError::DiffChangesDetected) {
                // The alternate Display walks the anyhow source chain so
                // operators see the underlying sqlx / IO root cause rather
                // than only the top-level context message.
                eprintln!("Error: {e:#}");
            }
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
        Command::Review(args) => {
            run_review(&args, cli.color, cli.quiet, &config)?;
        }
    }
    Ok(())
}

/// Setup logging based on verbosity and quiet flags.
fn setup_logging(verbose: u8, quiet: bool) {
    use tracing_subscriber::fmt;
    use tracing_subscriber::fmt::format::FmtSpan;

    let filter = build_env_filter(verbose, quiet, std::env::var("RUST_LOG").ok().as_deref());

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

/// Build the log filter from the verbosity flags and `RUST_LOG`.
///
/// `RUST_LOG`, when set to a non-empty value, takes precedence and fully
/// controls per-target filtering (standard `EnvFilter` semantics), so an
/// operator can run e.g. `RUST_LOG=relune_introspect=debug` to trace a single
/// module. When `RUST_LOG` is unset or empty, `-v`/`-q` select the level.
fn build_env_filter(
    verbose: u8,
    quiet: bool,
    rust_log: Option<&str>,
) -> tracing_subscriber::EnvFilter {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::filter::LevelFilter;

    let default_level = if quiet {
        LevelFilter::ERROR
    } else {
        match verbose {
            0 => LevelFilter::WARN,
            1 => LevelFilter::INFO,
            2 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        }
    };

    let builder = EnvFilter::builder().with_default_directive(default_level.into());
    match rust_log {
        Some(value) if !value.trim().is_empty() => builder.parse_lossy(value),
        _ => builder.parse_lossy(""),
    }
}

#[cfg(test)]
mod tests {
    use super::build_env_filter;
    use tracing_subscriber::filter::LevelFilter;

    #[test]
    fn verbosity_sets_default_level_without_rust_log() {
        assert_eq!(
            build_env_filter(0, false, None).max_level_hint(),
            Some(LevelFilter::WARN)
        );
        assert_eq!(
            build_env_filter(1, false, None).max_level_hint(),
            Some(LevelFilter::INFO)
        );
        assert_eq!(
            build_env_filter(2, false, None).max_level_hint(),
            Some(LevelFilter::DEBUG)
        );
        assert_eq!(
            build_env_filter(9, false, None).max_level_hint(),
            Some(LevelFilter::TRACE)
        );
    }

    #[test]
    fn quiet_lowers_default_level_to_error() {
        assert_eq!(
            build_env_filter(0, true, None).max_level_hint(),
            Some(LevelFilter::ERROR)
        );
    }

    #[test]
    fn empty_rust_log_falls_back_to_verbosity() {
        assert_eq!(
            build_env_filter(1, false, Some("   ")).max_level_hint(),
            Some(LevelFilter::INFO)
        );
    }

    #[test]
    fn rust_log_target_directive_takes_precedence() {
        // A per-target directive enables that module even though the verbosity
        // default would otherwise cap output at WARN.
        assert_eq!(
            build_env_filter(0, false, Some("relune_introspect=debug")).max_level_hint(),
            Some(LevelFilter::DEBUG)
        );
    }

    #[test]
    fn rust_log_global_level_overrides_verbosity() {
        assert_eq!(
            build_env_filter(0, false, Some("trace")).max_level_hint(),
            Some(LevelFilter::TRACE)
        );
    }
}
