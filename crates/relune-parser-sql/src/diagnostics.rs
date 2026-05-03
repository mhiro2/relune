//! Diagnostic helpers shared across submodules.

use crate::ParseOutput;
use relune_core::Severity;

pub(crate) const MAX_UNSUPPORTED_DEBUG_LEN: usize = 80;
const MAX_UNSUPPORTED_DEBUG_PREFIX_LEN: usize = MAX_UNSUPPORTED_DEBUG_LEN - 3;

pub(crate) fn truncate_unsupported_debug(debug_str: &str) -> String {
    if debug_str.len() <= MAX_UNSUPPORTED_DEBUG_LEN {
        return debug_str.to_owned();
    }

    let boundary = debug_str.floor_char_boundary(MAX_UNSUPPORTED_DEBUG_PREFIX_LEN);
    format!("{}...", &debug_str[..boundary])
}

pub(crate) fn error_summary(output: &ParseOutput) -> String {
    let messages = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| {
            format!(
                "{} {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            )
        })
        .collect::<Vec<_>>();

    if messages.is_empty() {
        "Failed to parse any valid schema elements".to_string()
    } else {
        format!(
            "SQL parsing reported error diagnostics: {}",
            messages.join("; ")
        )
    }
}
