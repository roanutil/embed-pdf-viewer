//! `scripting/results.ts`: result shaping for the scripting transaction:
//! errors, budgets, page numbers.

use epdf_acrojs::{
    ScriptDiagnostic, ScriptDiagnosticCode, ScriptExecutionError, ScriptExecutionErrorKind,
};
use epdf_kernel::types::DocumentMeta;
use serde_json::Value;

use crate::value::{get, items};

/// `scriptError(message)`.
pub fn script_error(message: impl Into<String>) -> ScriptExecutionError {
    ScriptExecutionError {
        kind: ScriptExecutionErrorKind::InvalidOutput,
        message: message.into(),
        stack: None,
    }
}

pub(crate) fn budget_error(message: impl Into<String>) -> ScriptExecutionError {
    ScriptExecutionError {
        kind: ScriptExecutionErrorKind::Budget,
        message: message.into(),
        stack: None,
    }
}

pub(crate) fn script_diagnostic(message: String) -> ScriptDiagnostic {
    ScriptDiagnostic { code: ScriptDiagnosticCode::ScriptError, message }
}

/// `utf8Length(value)`: the bytes `value`'s UTF-16 code units take in
/// UTF-8, a lone surrogate counted as three.
pub fn utf8_length(value: &str) -> usize {
    // A Rust `str` holds no lone surrogate, so its UTF-8 length is the answer.
    value.len()
}

/// `pageNumberFor(meta, field)`: the index of the page of the field's first
/// placed widget; 0 without one, or for a page the document doesn't have.
pub fn page_number_for(meta: Option<&DocumentMeta>, field: &Value) -> f64 {
    let placed = items(field, "widgets")
        .iter()
        .find(|w| get(w, "page").is_some_and(|p| !p.is_null()))
        .and_then(|w| get(w, "page"));
    let (Some(meta), Some(placed)) = (meta, placed) else { return 0.0 };
    let wanted = get(placed, "objectNumber");
    let index = meta.pages.as_array().and_then(|pages| {
        pages.iter().position(|p| {
            crate::value::strict_eq(get(p, "ref").and_then(|r| get(r, "objectNumber")), wanted)
        })
    });
    index.map_or(0.0, |i| i as f64)
}
