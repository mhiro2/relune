//! Output handling for relune CLI.
//!
//! This module provides utilities for handling output to files or stdout,
//! colored output, and diagnostic formatting.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::cli::ColorWhen;
use crate::error::{CliError, CliResult};
use relune_core::{Diagnostic, Severity};
use tracing::warn;

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
                persist_output_file(file, &final_path)?;
                Ok(())
            }
        }
    }
}

fn persist_output_file(file: tempfile::NamedTempFile, final_path: &Path) -> io::Result<()> {
    match file.persist(final_path) {
        Ok(_persisted_file) => Ok(()),
        Err(error) if is_cross_device_link_error(&error.error) => {
            warn!(
                path = %final_path.display(),
                "atomic rename is unavailable across devices; falling back to a non-atomic copy"
            );
            copy_temp_file_into_place(error.file, final_path)
        }
        Err(error) => Err(error.error),
    }
}

fn copy_temp_file_into_place(
    temp_file: tempfile::NamedTempFile,
    final_path: &Path,
) -> io::Result<()> {
    let mut source = temp_file.reopen()?;
    let temp_path = temp_file.into_temp_path();
    let mut destination = std::fs::File::create(final_path)?;
    io::copy(&mut source, &mut destination)?;
    destination.flush()?;
    destination.sync_all()?;
    drop(temp_path);
    Ok(())
}

#[cfg(unix)]
const CROSS_DEVICE_LINK_ERROR_CODE: i32 = 18;
#[cfg(windows)]
const CROSS_DEVICE_LINK_ERROR_CODE: i32 = 17;

fn is_cross_device_link_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(CROSS_DEVICE_LINK_ERROR_CODE)
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

    /// Print diagnostics to stderr.
    ///
    /// When `quiet` is set, only error-severity diagnostics are printed so that
    /// `--quiet` lives up to its "less non-error output" promise. Suppression
    /// is purely cosmetic: callers still compute exit codes from the full
    /// diagnostic set.
    pub fn print_all(&self, diagnostics: &[Diagnostic], quiet: bool) {
        for diagnostic in diagnostics {
            if !quiet || diagnostic.severity == Severity::Error {
                self.print(diagnostic);
            }
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
    quiet: bool,
) -> crate::error::CliResult<()> {
    let threshold = if fail_on_warning {
        Severity::Warning
    } else {
        Severity::Error
    };
    check_diagnostics_at_or_above(diagnostics, color, threshold, quiet)
}

/// Print diagnostics, then fail when any diagnostic meets the threshold.
///
/// `quiet` suppresses printing of non-error diagnostics; the failure decision
/// always considers the full diagnostic set.
pub fn check_diagnostics_at_or_above(
    diagnostics: &[Diagnostic],
    color: ColorWhen,
    minimum_severity: Severity,
    quiet: bool,
) -> crate::error::CliResult<()> {
    let printer = DiagnosticPrinter::new(color);
    printer.print_all(diagnostics, quiet);

    let highest = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.severity)
        .filter(|severity| *severity >= minimum_severity)
        .max();

    match highest {
        Some(Severity::Error) => Err(crate::error::CliError::general(anyhow::anyhow!(
            "Errors were encountered during processing"
        ))),
        Some(_severity) => Err(crate::error::CliError::warning(anyhow::anyhow!(
            "Diagnostics at or above {minimum_severity} were emitted"
        ))),
        None => Ok(()),
    }
}

/// Validate that the parent directory of an `--out` path exists before we try
/// to create a temp file there.
///
/// A missing directory otherwise surfaces as an opaque general failure
/// (exit 1) without naming the offending path. Reporting it as a usage error
/// (exit 2) with the path matches how input-file problems are handled.
pub fn validate_output_path(path: &Path) -> CliResult<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    match std::fs::metadata(parent) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(CliError::usage(anyhow::anyhow!(
            "Output directory '{}' is not a directory (for --out '{}')",
            parent.display(),
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(CliError::usage(anyhow::anyhow!(
                "Output directory '{}' does not exist (for --out '{}')",
                parent.display(),
                path.display()
            )))
        }
        // Other I/O errors (permission denied, symlink loops, ...) are not
        // usage mistakes; let temp-file creation surface them as a general
        // failure with the underlying error rather than mislabeling them here.
        Err(_) => Ok(()),
    }
}

/// Write string content to an output destination and finalise the writer.
pub fn write_output(
    content: &str,
    out_path: Option<&Path>,
    color: ColorWhen,
) -> crate::error::CliResult<()> {
    use anyhow::Context;

    if let Some(path) = out_path {
        validate_output_path(path)?;
    }

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
    use relune_core::{Diagnostic, LintRuleId, Severity, diagnostic::codes};

    #[test]
    fn diagnostic_printer_formats_plain_messages() {
        let printer = DiagnosticPrinter::new(ColorWhen::Never);
        let diagnostic = Diagnostic::warning(
            LintRuleId::OrphanTable.diagnostic_code(),
            "table has no parents",
        )
        .with_source("schema.sql");

        assert_eq!(
            printer.format_plain(&diagnostic),
            "warning[LINT004]: table has no parents (in schema.sql)"
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
    fn copy_fallback_replaces_target_contents() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let output_path = temp.path().join("diagram.svg");
        std::fs::write(&output_path, "stale").expect("seed output");

        let mut temp_file = tempfile::NamedTempFile::new_in(temp.path()).expect("create temp");
        temp_file
            .write_all(b"<svg>diagram</svg>")
            .expect("write temp");
        temp_file.flush().expect("flush temp");

        copy_temp_file_into_place(temp_file, &output_path).expect("copy fallback");

        let content = std::fs::read_to_string(&output_path).expect("read output");
        assert_eq!(content, "<svg>diagram</svg>");
    }

    #[test]
    fn cross_device_error_detection_matches_platform_code() {
        let error = io::Error::from_raw_os_error(CROSS_DEVICE_LINK_ERROR_CODE);

        assert!(is_cross_device_link_error(&error));
        assert!(!is_cross_device_link_error(&io::Error::other(
            "different error"
        )));
    }

    #[test]
    fn diagnostic_helpers_detect_severity() {
        let diagnostics = [
            Diagnostic::info(codes::parse_skipped(), "ignored"),
            Diagnostic::warning(LintRuleId::OrphanTable.diagnostic_code(), "warn"),
            Diagnostic::error(codes::parse_error(), "err"),
        ];

        assert!(diagnostics.iter().any(|d| d.severity == Severity::Warning));
        assert!(diagnostics.iter().any(|d| d.severity == Severity::Error));
        assert_eq!(diagnostics[0].severity, Severity::Info);
    }

    #[test]
    fn diagnostics_threshold_respects_info_level() {
        let diagnostics = vec![Diagnostic::info(codes::parse_skipped(), "ignored")];

        let error =
            check_diagnostics_at_or_above(&diagnostics, ColorWhen::Never, Severity::Info, false)
                .expect_err("info diagnostics should trip the configured threshold");
        assert_eq!(error.exit_code(), 3);
        assert!(error.to_string().contains("at or above info"));
    }

    #[test]
    fn quiet_still_fails_on_threshold_even_when_diagnostics_are_hidden() {
        // `--quiet` suppresses printing of non-error diagnostics, but the exit
        // code must still reflect the full diagnostic set.
        let diagnostics = vec![Diagnostic::info(codes::parse_skipped(), "ignored")];

        let error =
            check_diagnostics_at_or_above(&diagnostics, ColorWhen::Never, Severity::Info, true)
                .expect_err("threshold must still trip under --quiet");
        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn validate_output_path_accepts_existing_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("out.svg");
        validate_output_path(&path).expect("existing parent directory should be accepted");
    }

    #[test]
    fn validate_output_path_rejects_missing_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("missing").join("out.svg");

        let error = validate_output_path(&path)
            .expect_err("missing parent directory should be a usage error");
        assert_eq!(error.exit_code(), 2);
        let message = error.to_string();
        assert!(message.contains("does not exist"), "message: {message}");
        assert!(message.contains("missing"), "message: {message}");
    }

    #[test]
    fn validate_output_path_rejects_file_as_parent() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("not-a-dir");
        std::fs::write(&file, "x").expect("seed file");
        let path = file.join("out.svg");

        let error = validate_output_path(&path).expect_err("file parent should be a usage error");
        assert_eq!(error.exit_code(), 2);
        assert!(
            error.to_string().contains("is not a directory"),
            "message: {error}"
        );
    }

    #[test]
    fn diagnostics_threshold_keeps_errors_fatal() {
        let diagnostics = vec![Diagnostic::error(codes::parse_error(), "syntax error")];

        let error =
            check_diagnostics_at_or_above(&diagnostics, ColorWhen::Never, Severity::Warning, false)
                .expect_err("errors should remain fatal");
        assert_eq!(error.exit_code(), 1);
        assert!(error.to_string().contains("Errors were encountered"));
    }
}
