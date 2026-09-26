//! `contract.ts`, `token.ts` and `host-contract.ts`: the public form
//! vocabulary, the token, and the configuration. Fields are addressed by a
//! `FormFieldRef` ([`field_ref`] builds one), widgets by their
//! `AnnotationRef`, both as the engine writes them.
//!
//! The TypeScript's public capability and its host lens are one runtime
//! token typed two ways. Here both are [`FORM`] and
//! [`crate::FormCapability`]; the host members say so in their docs.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_acrojs::{ScriptDiagnostic, ScriptExecutionError, ScriptUiEffect};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::{Error, Token};
use epdf_plugin_actions::{ActionOrigin, ActionTriggerResult, ScriptPhase, page_ref_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::controller::FormCapability;
use crate::core::model::Box;

/// `FormToken`. The public, `/contract/host` and `/internal` entries are
/// type lenses over this one token.
pub static FORM: LazyLock<Token<FormCapability>> = LazyLock::new(|| {
    Token::with_hint("form", "add formPlugin() from '@embedpdf/plugin-form' to your plugins list")
});

/// `ResourceStatus`: `loading` until the field tree is in, then `ready`;
/// `forbidden` without `doc.forms.read`, `error` after a failed read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Forbidden,
    Error,
}

/// `FormConfig.validation`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Validation {
    /// Run the document's keystroke, validate, calculate and format scripts
    /// (needs the actions plugin with scripting on).
    #[default]
    Scripts,
    /// Write values straight to the engine.
    None,
}

/// Host inputs. The TypeScript reports three swallowed failures to the
/// console; a host that wants them passes the sinks.
#[derive(Clone, Default)]
pub struct FormHost {
    /// `console.warn('[form] form snapshot failed to load', err)`.
    pub console_warn: Option<Rc<dyn Fn(&str, &Error)>>,
    /// `console.error('[form] createField failed:', err)` and
    /// `console.error('[form] event listener failed:', err)`.
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

/// `formPlugin(config)`: `FormConfig` plus the host inputs.
#[derive(Clone, Default)]
pub struct FormConfig {
    /// Default [`Validation::Scripts`].
    pub validation: Option<Validation>,
    pub host: FormHost,
}

/// `FormCommitStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormCommitStatus {
    Applied,
    Unchanged,
    Rejected,
    Failed,
}

impl FormCommitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FormCommitStatus::Applied => "applied",
            FormCommitStatus::Unchanged => "unchanged",
            FormCommitStatus::Rejected => "rejected",
            FormCommitStatus::Failed => "failed",
        }
    }
}

/// `FormUiEffect`: a script UI request tagged with who asked. `phase` is the
/// script-model axis (`boot` for a name-tree script); `origin` the dispatch
/// axis, when the script ran inside a dispatch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormUiEffect {
    #[serde(flatten)]
    pub effect: ScriptUiEffect,
    pub phase: ScriptPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ActionOrigin>,
}

/// `FormCommitResult`, which is also `SetValueResult`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormCommitResult {
    pub status: FormCommitStatus,
    pub scripted: bool,
    /// The engine's `FormEffectsResult`, as it answered, or `None`.
    pub effects_result: Option<Value>,
    pub ui_effects: Vec<FormUiEffect>,
    pub diagnostics: Vec<ScriptDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptExecutionError>,
}

/// `SetValueResult`.
pub type SetValueResult = FormCommitResult;

impl FormCommitResult {
    /// `{status, scripted, effectsResult: null, uiEffects: [], diagnostics: []}`.
    pub fn bare(status: FormCommitStatus, scripted: bool) -> Self {
        FormCommitResult {
            status,
            scripted,
            effects_result: None,
            ui_effects: Vec::new(),
            diagnostics: Vec::new(),
            error: None,
        }
    }
}

/// `WidgetActivationResult`: which world handled an activation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "result", rename_all = "camelCase")]
pub enum WidgetActivationResult {
    Form(FormCommitResult),
    Dispatched(ActionTriggerResult),
}

/// `CreateFieldInput`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFieldInput {
    /// An authorable family: `text`, `checkbox`, `radio`, `combobox`,
    /// `listbox`, `signature`.
    pub family: String,
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
    /// The widget's page-space box.
    pub bounds: Box,
    /// A fully qualified name; auto-numbered from the family when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Widget styling in the engine vocabulary (`WidgetAppearance`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Value>,
    /// Choice fields: the options to start with, `{label, value}` each.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Value>>,
}

/// `CreatedField`: the created field and its widget on the requested page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatedField {
    pub field: Value,
    pub widget: Option<Value>,
}

/// `FormFilter`: every member optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FormFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Fields with a widget on this page.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "page_ref_json::option")]
    pub page: Option<PageRef>,
    /// Exact fully qualified name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `resetAll`'s options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResetAllOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<bool>,
}

/// One failed entry of a `BatchResult`: the entry and its `PluginErrorInfo`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BatchFailure {
    #[serde(rename = "ref")]
    pub entry: Value,
    pub error: Value,
}

/// `BatchResult<FormFieldRef, R>`: the refs that landed, none skipped, and
/// the entries that failed.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BatchResult {
    pub applied: Vec<Value>,
    pub skipped: Vec<Value>,
    pub failed: Vec<BatchFailure>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// `FormValueChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FormValueChangedEvent {
    #[serde(rename = "ref")]
    pub field_ref: Value,
    pub field: Value,
    /// `ChangeOrigin`.
    pub origin: Value,
}

/// `FormFieldChangedEvent`: `field` is null once deleted.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FormFieldChangedEvent {
    #[serde(rename = "ref")]
    pub field_ref: Value,
    pub field: Option<Value>,
    pub origin: Value,
}

/// `FormValidationRejectedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FormValidationRejectedEvent {
    #[serde(rename = "ref")]
    pub field_ref: Value,
    pub issues: Vec<ScriptDiagnostic>,
}

/// `FormResyncedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FormResyncedEvent {
    pub snapshot: Value,
}

/// `fieldRef`: build a `FormFieldRef` without spelling the discriminator.
pub mod field_ref {
    use super::*;

    /// `fieldRef.byName(name)`.
    pub fn by_name(name: &str) -> Value {
        json!({ "kind": "fqn", "name": name })
    }

    /// `fieldRef.byObjectNumber(fieldObjectNumber)`.
    pub fn by_object_number(field_object_number: f64) -> Value {
        json!({ "kind": "objectNumber", "fieldObjectNumber": crate::value::number(field_object_number) })
    }
}
