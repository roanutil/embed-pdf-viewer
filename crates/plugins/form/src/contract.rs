//! `contract.ts` and `host-contract.ts`: the form vocabulary. The engine's
//! DTOs (fields, widget rows, refs, values, bundles, drafts, patches) stay
//! JSON; the plugin's own results are typed where the plugin builds them.

use epdf_acrojs::{ScriptDiagnostic, ScriptExecutionError, ScriptUiEffect};
use epdf_plugin_actions::{ActionOrigin, ScriptPhase};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::{Value, json};

/// `FORM_DEFAULTS`: the settings when the app registers none.
pub fn form_defaults() -> Value {
    json!({
        "validation": "scripts",
        "focus": { "color": null },
        "fields": { "border": null, "background": "rgb(255 255 255 / 0.92)", "color": "#1f2a44" },
        "fillable": true,
    })
}

/// `FormCommitStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FormCommitStatus {
    Applied,
    Unchanged,
    Rejected,
    Failed,
}

/// `FormUiEffect`: a script's UI request, tagged with who asked.
#[derive(Clone, Debug, PartialEq)]
pub struct FormUiEffect {
    pub effect: ScriptUiEffect,
    pub phase: ScriptPhase,
    pub origin: Option<ActionOrigin>,
}

impl Serialize for FormUiEffect {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let effect = serde_json::to_value(&self.effect).map_err(serde::ser::Error::custom)?;
        let mut map = s.serialize_map(None)?;
        for (key, value) in effect.as_object().into_iter().flatten() {
            map.serialize_entry(key, value)?;
        }
        map.serialize_entry("phase", &self.phase)?;
        if let Some(origin) = &self.origin {
            map.serialize_entry("origin", origin)?;
        }
        map.end()
    }
}

/// `FormCommitResult`: a write through the document's scripts.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormCommitResult {
    pub status: FormCommitStatus,
    pub scripted: bool,
    pub ui_effects: Vec<FormUiEffect>,
    pub diagnostics: Vec<ScriptDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptExecutionError>,
}

impl FormCommitResult {
    /// A result with no script output.
    pub fn bare(status: FormCommitStatus, scripted: bool) -> Self {
        FormCommitResult {
            status,
            scripted,
            ui_effects: Vec::new(),
            diagnostics: Vec::new(),
            error: None,
        }
    }
}

/// `WidgetActivationResult`.
#[derive(Clone, Debug)]
pub enum WidgetActivationResult {
    Form(FormCommitResult),
    Dispatched(epdf_plugin_actions::ActionTriggerResult),
}

impl WidgetActivationResult {
    pub fn to_json(&self) -> Value {
        match self {
            WidgetActivationResult::Form(result) => {
                json!({ "kind": "form", "result": epdf_kernel::js::to_value(result) })
            }
            WidgetActivationResult::Dispatched(result) => {
                json!({ "kind": "dispatched", "result": epdf_kernel::js::to_value(result) })
            }
        }
    }
}
