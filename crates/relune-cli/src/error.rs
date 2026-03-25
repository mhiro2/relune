//! CLI error handling.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use crate::config::ConfigError;

/// Result type used by CLI entry points.
pub(crate) type CliResult<T> = Result<T, CliError>;

/// Structured CLI error with an exit code.
#[derive(Debug)]
pub(crate) enum CliError {
    /// Usage or configuration error.
    Usage(anyhow::Error),
    /// Warning-level failure.
    Warning(anyhow::Error),
    /// Unexpected runtime failure.
    General(anyhow::Error),
}

impl CliError {
    /// Create a usage error.
    #[must_use]
    pub fn usage(error: impl Into<anyhow::Error>) -> Self {
        Self::Usage(error.into())
    }

    /// Create a warning failure.
    #[must_use]
    pub fn warning(error: impl Into<anyhow::Error>) -> Self {
        Self::Warning(error.into())
    }

    /// Create a general failure.
    #[must_use]
    pub fn general(error: impl Into<anyhow::Error>) -> Self {
        Self::General(error.into())
    }

    /// Returns the process exit code for this error.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Warning(_) => 3,
            Self::General(_) => 1,
        }
    }
}

impl Display for CliError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) | Self::Warning(error) | Self::General(error) => {
                Display::fmt(error, f)
            }
        }
    }
}

impl StdError for CliError {}

impl From<anyhow::Error> for CliError {
    fn from(error: anyhow::Error) -> Self {
        Self::general(error)
    }
}

impl From<ConfigError> for CliError {
    fn from(error: ConfigError) -> Self {
        Self::usage(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_errors_use_exit_code_2() {
        let error = CliError::usage(anyhow::anyhow!("usage error"));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn warning_errors_use_exit_code_3() {
        let error = CliError::warning(anyhow::anyhow!("warning error"));
        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn general_errors_use_exit_code_1() {
        let error = CliError::general(anyhow::anyhow!("general error"));
        assert_eq!(error.exit_code(), 1);
    }
}
