//! Output handling for relune CLI.
//!
//! This module provides utilities for handling output to files or stdout,
//! colored output, and diagnostic formatting.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::cli::ColorWhen;
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
        match &mut self.destination {
            OutputDestination::Stdout => {
                print!("{content}");
                io::stdout().flush()
            }
            OutputDestination::TempFile { file, .. } => {
                write!(file, "{content}")?;
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
