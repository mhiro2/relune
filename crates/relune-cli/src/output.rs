//! Output handling for relune CLI.
//!
//! This module provides utilities for handling output to files or stdout,
//! colored output, and diagnostic formatting.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::cli::ColorWhen;
use crate::error::{CliError, CliResult};
use relune_core::{Diagnostic, Severity};

/// Output writer that handles both file and stdout output.
///
/// File output uses a temporary file in the same directory as the target,
/// then atomically renames on completion. This prevents partial writes from
/// corrupting existing output files on failure or interruption.
pub struct OutputWriter {
    /// The output destination.
    destination: OutputDestination,
}

enum OutputDestination {
    Stdout,
    TempFile {
        file: tempfile::NamedTempFile,
        final_path: PathBuf,
    },
}

impl OutputWriter {
    /// Create a new output writer.
    ///
    /// If `path` is `None`, writes to stdout.
    /// If `path` is `Some`, writes to a temporary file that will be atomically
    /// renamed to the target path when [`finish`] is called.
    pub fn new(path: Option<&Path>, _color: ColorWhen) -> io::Result<Self> {
        let destination = match path {
            Some(p) => {
                let dir = p.parent().unwrap_or_else(|| Path::new("."));
                let file = tempfile::NamedTempFile::new_in(dir)?;
                OutputDestination::TempFile {
                    file,
                    final_path: p.to_path_buf(),
                }
            }
            None => OutputDestination::Stdout,
        };

        Ok(Self { destination })
    }

    /// Write content to the output destination.
    pub fn write(&mut self, content: &str) -> io::Result<()> {
        self.write_bytes(content.as_bytes())
    }

    /// Write raw bytes to the output destination.
    pub fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
        match &mut self.destination {
            OutputDestination::Stdout => {
                io::stdout().write_all(data)?;
                io::stdout().flush()
            }
            OutputDestination::TempFile { file, .. } => {
                file.write_all(data)?;
                file.flush()
            }
        }
    }

    /// Finalize file output by atomically renaming the temp file to the target path.
    ///
    /// For stdout output, this is a no-op.
    /// Must be called after all writes are complete to persist the output file.
    pub fn finish(self) -> io::Result<()> {
        match self.destination {
            OutputDestination::Stdout => Ok(()),
            OutputDestination::TempFile { file, final_path } => {
                file.persist(&final_path).map_err(|e| e.error)?;
                Ok(())
            }
        }
    }
}

/// Diagnostic printer for stderr output.
pub struct DiagnosticPrinter {
    /// Whether to use colors.
    use_colors: bool,
}

impl DiagnosticPrinter {
    /// Create a new diagnostic printer.
    pub fn new(color: ColorWhen) -> Self {
        let use_colors = match color {
            ColorWhen::Always => true,
            ColorWhen::Never => false,
            ColorWhen::Auto => io::stderr().is_terminal(),
        };

        Self { use_colors }
    }

    /// Print a diagnostic to stderr.
    pub fn print(&self, diagnostic: &Diagnostic) {
        let message = self.format_diagnostic(diagnostic);
        eprintln!("{message}");
    }

    /// Print multiple diagnostics to stderr.
    pub fn print_all(&self, diagnostics: &[Diagnostic]) {
        for diagnostic in diagnostics {
            self.print(diagnostic);
        }
    }

    /// Format a diagnostic message.
    fn format_diagnostic(&self, diagnostic: &Diagnostic) -> String {
        if self.use_colors {
            self.format_colored(diagnostic)
        } else {
            self.format_plain(diagnostic)
        }
    }

    #[allow(clippy::unused_self)]
    fn format_colored(&self, diagnostic: &Diagnostic) -> String {
        let severity_str = match diagnostic.severity {
            Severity::Error => "\x1b[31merror\x1b[0m",
            Severity::Warning => "\x1b[33mwarning\x1b[0m",
            Severity::Info => "\x1b[34minfo\x1b[0m",
            Severity::Hint => "\x1b[36mhint\x1b[0m",
        };

        let code = &diagnostic.code;
        let message = &diagnostic.message;

        if let Some(ref source) = diagnostic.source {
            format!("{severity_str}[{code}]: {message} (in {source})")
        } else {
            format!("{severity_str}[{code}]: {message}")
        }
    }

    #[allow(clippy::unused_self)]
    fn format_plain(&self, diagnostic: &Diagnostic) -> String {
        let severity_str = match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Hint => "hint",
        };

        let code = &diagnostic.code;
        let message = &diagnostic.message;

        if let Some(ref source) = diagnostic.source {
            format!("{severity_str}[{code}]: {message} (in {source})")
        } else {
            format!("{severity_str}[{code}]: {message}")
        }
    }

    /// Check if there are any warnings in the diagnostics.
    pub fn has_warnings(diagnostics: &[Diagnostic]) -> bool {
        diagnostics.iter().any(|d| d.severity == Severity::Warning)
    }

    /// Check if there are any errors in the diagnostics.
    pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
        diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

/// Print stats to stderr.
pub fn print_stats(stats: &relune_app::RenderStats) {
    eprintln!(
        "Stats: {} tables, {} columns, {} edges, {} views",
        stats.table_count, stats.column_count, stats.edge_count, stats.view_count
    );
    eprintln!(
        "Timing: parse {:.2}ms, graph {:.2}ms, layout {:.2}ms, render {:.2}ms, total {:.2}ms",
        stats.parse_time.as_secs_f64() * 1000.0,
        stats.graph_time.as_secs_f64() * 1000.0,
        stats.layout_time.as_secs_f64() * 1000.0,
        stats.render_time.as_secs_f64() * 1000.0,
        stats.total_time.as_secs_f64() * 1000.0
    );
}

/// Print diagnostics, fail on errors, and optionally fail on warnings.
///
/// This is the shared post-execution diagnostics pipeline used by every CLI
/// command.  It prints all diagnostics, then checks for errors (always) and
/// warnings (when `fail_on_warning` is `true`).
pub fn check_diagnostics(
    diagnostics: &[Diagnostic],
    color: ColorWhen,
    fail_on_warning: bool,
) -> crate::error::CliResult<()> {
    let printer = DiagnosticPrinter::new(color);
    printer.print_all(diagnostics);

    if fail_on_warning && DiagnosticPrinter::has_warnings(diagnostics) {
        return Err(crate::error::CliError::warning(anyhow::anyhow!(
            "Warnings were emitted and --fail-on-warning is set"
        )));
    }
    if DiagnosticPrinter::has_errors(diagnostics) {
        return Err(crate::error::CliError::general(anyhow::anyhow!(
            "Errors were encountered during processing"
        )));
    }
    Ok(())
}

/// Write string content to an output destination and finalise the writer.
pub fn write_output(
    content: &str,
    out_path: Option<&Path>,
    color: ColorWhen,
) -> crate::error::CliResult<()> {
    use anyhow::Context;

    let mut writer =
        OutputWriter::new(out_path, color).context("Failed to create output writer")?;
    writer.write(content).context("Failed to write output")?;
    writer.finish().context("Failed to finalize output")?;
    Ok(())
}

/// Reject raw markup output to an interactive terminal unless explicitly allowed.
pub fn validate_markup_stdout_usage(
    markup_label: &str,
    has_output_path: bool,
    explicit_stdout: bool,
    stdout_is_terminal: bool,
) -> CliResult<()> {
    if !has_output_path && !explicit_stdout && stdout_is_terminal {
        return Err(CliError::usage(anyhow::anyhow!(
            "Refusing to write raw {markup_label} to an interactive terminal. Use --out <FILE> or --stdout."
        )));
    }

    Ok(())
}

/// Reject binary output to an interactive terminal.
pub fn validate_binary_stdout_usage(
    format_label: &str,
    has_output_path: bool,
    stdout_is_terminal: bool,
) -> CliResult<()> {
    if !has_output_path && stdout_is_terminal {
        return Err(CliError::usage(anyhow::anyhow!(
            "Refusing to write binary {format_label} data to an interactive terminal. Use --out <FILE>."
        )));
    }

    Ok(())
}

/// Print a success message to stderr.
pub fn print_success(message: &str, color: ColorWhen) {
    let use_colors = match color {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        ColorWhen::Auto => io::stderr().is_terminal(),
    };

    if use_colors {
        eprintln!("\x1b[32m{message}\x1b[0m");
    } else {
        eprintln!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::{Diagnostic, Severity, diagnostic::codes};

    #[test]
    fn diagnostic_printer_formats_plain_messages() {
        let printer = DiagnosticPrinter::new(ColorWhen::Never);
        let diagnostic = Diagnostic::warning(codes::lint_orphan_table(), "table has no parents")
            .with_source("schema.sql");

        assert_eq!(
            printer.format_plain(&diagnostic),
            "warning[LINT002]: table has no parents (in schema.sql)"
        );
    }

    #[test]
    fn diagnostic_printer_formats_colored_messages() {
        let printer = DiagnosticPrinter::new(ColorWhen::Always);
        let diagnostic = Diagnostic::error(codes::parse_error(), "syntax error");

        assert_eq!(
            printer.format_colored(&diagnostic),
            "\x1b[31merror\x1b[0m[PARSE001]: syntax error"
        );
    }

    #[test]
    fn output_writer_persists_temp_file_contents() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let output_path = temp.path().join("diagram.svg");

        let mut writer =
            OutputWriter::new(Some(&output_path), ColorWhen::Never).expect("create writer");
        writer.write("<svg>diagram</svg>").expect("write output");
        writer.finish().expect("persist output");

        let content = std::fs::read_to_string(&output_path).expect("read output");
        assert_eq!(content, "<svg>diagram</svg>");
    }

    #[test]
    fn diagnostic_helpers_detect_severity() {
        let diagnostics = vec![
            Diagnostic::info(codes::parse_skipped(), "ignored"),
            Diagnostic::warning(codes::lint_orphan_table(), "warn"),
            Diagnostic::error(codes::parse_error(), "err"),
        ];

        assert!(DiagnosticPrinter::has_warnings(&diagnostics));
        assert!(DiagnosticPrinter::has_errors(&diagnostics));
        assert_eq!(diagnostics[0].severity, Severity::Info);
    }
}
