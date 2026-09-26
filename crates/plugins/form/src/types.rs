//! `types.ts` and the `/contract` entry: state, the action, the commit
//! result, the placement DTOs, the token and the configuration.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_acrojs::{
    IdentityOverrides, ScriptBudget, ScriptDiagnostic, ScriptExecutionError, ScriptSandboxFactory,
    ScriptUiEffect,
};
use epdf_kernel::{Error, Token};
use epdf_plugin_actions::{ActionOrigin, ActionTriggerResult, ScriptPhase};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability::FormCapability;
use crate::core::model::{Box, Model};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FormState {
    pub model: Rc<Model>,
}

/// `FormAction`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum FormAction {
    #[serde(rename = "SET_MODEL")]
    SetModel { model: Rc<Model> },
}

/// `FormToken`. The TypeScript's public, `/contract/host` and `/internal`
/// entries are type lenses over one runtime token; this is that token.
pub static FORM: LazyLock<Token<FormCapability>> = LazyLock::new(|| Token::new("form"));

/// `FormCommitStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormCommitStatus {
    Applied,
    Unchanged,
    Rejected,
    Failed,
}

/// `FormUiEffect`: a script UI request tagged with who asked. `phase` is
/// the script-model axis (`boot` for a name-tree script); `origin` the
/// dispatch axis, when the script ran inside a dispatch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormUiEffect {
    #[serde(flatten)]
    pub effect: ScriptUiEffect,
    pub phase: ScriptPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ActionOrigin>,
}

/// `FormCommitResult`.
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

/// `PlaceFieldInput`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceFieldInput {
    /// An authorable family: `text`, `checkbox`, `radio`, `combobox`,
    /// `listbox`.
    pub family: String,
    pub page_object_number: f64,
    /// Content-space logical field box.
    #[serde(rename = "box")]
    pub bx: Box,
    /// Widget styling in the engine vocabulary (`WidgetAppearance`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Value>,
}

/// `PlacedField`: the created field and its widget on the requested page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedField {
    pub field: Value,
    pub widget: Option<Value>,
}

/// Host inputs. The TypeScript reports two swallowed failures to the
/// console; a host that wants them passes the sinks.
#[derive(Clone, Default)]
pub struct FormHost {
    /// `console.warn('[form] form snapshot failed to load', err)`.
    pub console_warn: Option<Rc<dyn Fn(&str, &Error)>>,
    /// `console.error('[form] placeField failed:', err)`.
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

/// `FormPluginOptions` is empty in the TypeScript (scripting moved to the
/// actions plugin); the Rust config carries only the host inputs.
#[derive(Clone, Default)]
pub struct FormPluginConfig {
    pub host: FormHost,
}

/// `FormScriptingOptions`: the standalone realm's configuration (stamp's
/// detached documents). The TypeScript lazily imports a QuickJS sandbox when
/// `sandboxFactory` is absent; Rust has no default, so the factory is
/// required, and a host without a sandbox builds no controller.
#[derive(Clone)]
pub struct FormScriptingOptions {
    pub sandbox_factory: ScriptSandboxFactory,
    /// Embedder identity fields layered over the engine identity.
    pub identity: Option<IdentityOverrides>,
    /// `this.documentFileName`.
    pub file_name: Option<Rc<dyn Fn() -> String>>,
    pub now: Option<Rc<dyn Fn() -> f64>>,
    pub utc_offset_minutes: Option<Rc<dyn Fn() -> f64>>,
    pub random_seed: Option<Rc<dyn Fn() -> f64>>,
    pub budget: Option<ScriptBudget>,
    /// Host input: `-new Date(nowMs).getTimezoneOffset()`, used when
    /// `utc_offset_minutes` is not configured. Default zero (UTC).
    pub host_utc_offset_minutes: Option<Rc<dyn Fn(f64) -> f64>>,
}

impl FormScriptingOptions {
    /// Options with only a sandbox.
    pub fn new(sandbox_factory: ScriptSandboxFactory) -> Self {
        FormScriptingOptions {
            sandbox_factory,
            identity: None,
            file_name: None,
            now: None,
            utc_offset_minutes: None,
            random_seed: None,
            budget: None,
            host_utc_offset_minutes: None,
        }
    }
}
