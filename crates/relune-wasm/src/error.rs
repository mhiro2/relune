//! WASM error handling.
//!
//! Provides error types suitable for crossing the WASM boundary.

use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Error type for WASM boundary.
#[derive(Debug, Serialize)]
pub struct WasmError {
    /// Error message.
    pub message: String,
    /// Optional error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl WasmError {
    /// Create a new error with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    /// Create a new input error with a stable error code.
    pub fn input(message: impl Into<String>) -> Self {
        Self::with_code(message, "INPUT_ERROR")
    }

    /// Create a new error with a message and code.
    pub fn with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code.into()),
        }
    }
}

impl From<relune_app::AppError> for WasmError {
    fn from(err: relune_app::AppError) -> Self {
        let code = err.category_code().map(str::to_string);
        Self {
            message: err.to_string(),
            code,
        }
    }
}

impl From<serde_wasm_bindgen::Error> for WasmError {
    fn from(err: serde_wasm_bindgen::Error) -> Self {
        Self::with_code(err.to_string(), "SERIALIZATION_ERROR")
    }
}

/// Converts a `WasmError` into a `JsValue` for the WASM boundary.
///
/// JS consumers may receive errors in one of three shapes:
/// 1. **Structured object** — `{ message: string, code?: string }` (normal path via `serde_wasm_bindgen`).
/// 2. **Plain string** — the error message only (fallback when serialization itself fails).
/// 3. **Panic string** — from `console_error_panic_hook` if Rust panics (unrecoverable).
impl From<WasmError> for JsValue {
    fn from(err: WasmError) -> Self {
        serde_wasm_bindgen::to_value(&err).unwrap_or_else(|_| Self::from_str(&err.message))
    }
}

impl std::fmt::Display for WasmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_errors_use_stable_code() {
        let err = WasmError::input("bad request");
        assert_eq!(err.code.as_deref(), Some("INPUT_ERROR"));
    }

    #[test]
    fn with_code_sets_both_fields() {
        let err = WasmError::with_code("failed", "CUSTOM_CODE");
        assert_eq!(err.message, "failed");
        assert_eq!(err.code.as_deref(), Some("CUSTOM_CODE"));
    }

    #[test]
    fn new_error_has_no_code() {
        let err = WasmError::new("something went wrong");
        assert_eq!(err.message, "something went wrong");
        assert!(err.code.is_none());
    }

    #[test]
    fn display_shows_message() {
        let err = WasmError::with_code("bad input", "INPUT_ERROR");
        assert_eq!(err.to_string(), "bad input");
    }
}
