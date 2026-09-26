//! `scripting/results.ts`: result shaping for the transaction: errors,
//! budgets, page numbers, statuses.

use epdf_acrojs::{
    ScriptDiagnostic, ScriptDiagnosticCode, ScriptExecutionError, ScriptExecutionErrorKind,
};
use epdf_kernel::types::DocumentMeta;
use serde_json::Value;

use crate::contract::{FormCommitResult, FormCommitStatus, FormUiEffect};
use crate::value::{get, num, str_of};

pub(crate) fn script_error(message: impl Into<String>) -> ScriptExecutionError {
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

/// `pageNumberFor(meta, field)`: the index of the page of the field's first
/// widget whose `page` is not null, 0 when there is none, the member is
/// absent, or the page is unknown.
pub(crate) fn page_number_for(meta: Option<&DocumentMeta>, field: &Value) -> f64 {
    let placed = get(field, "widgets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|w| !w.get("page").is_some_and(Value::is_null))
        .and_then(|w| get(w, "page"));
    let (Some(meta), Some(placed)) = (meta, placed) else { return 0.0 };
    let pon = num(placed, "pageObjectNumber");
    let index = meta.pages.as_array().and_then(|pages| {
        pages.iter().position(|p| get(p, "ref").and_then(|r| num(r, "pageObjectNumber")) == pon)
    });
    index.map_or(0.0, |i| i as f64)
}

/// `statusFromEffects(result)`.
pub(crate) fn status_from_effects(result: &Value) -> FormCommitStatus {
    let statuses: Vec<&str> = get(result, "results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| str_of(entry, "status").unwrap_or_default())
        .collect();
    if statuses.iter().any(|s| *s == "failed" || *s == "skipped") {
        return FormCommitStatus::Failed;
    }
    if statuses.contains(&"rejected") {
        return FormCommitStatus::Failed;
    }
    if statuses.contains(&"applied") {
        FormCommitStatus::Applied
    } else {
        FormCommitStatus::Unchanged
    }
}

pub(crate) fn failed(
    ui_effects: Vec<FormUiEffect>,
    diagnostics: Vec<ScriptDiagnostic>,
    error: ScriptExecutionError,
) -> FormCommitResult {
    FormCommitResult {
        status: FormCommitStatus::Failed,
        scripted: true,
        effects_result: None,
        ui_effects,
        diagnostics,
        error: Some(error),
    }
}
