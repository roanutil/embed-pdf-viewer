//! `write/script-effects.ts`: the actions plane's seams into the form owner.
//! The form-effects commit sink (script `setValue`, `setDisplay` and reset
//! effects land through the owner, so the engine write and the visible model
//! can never diverge) and the submit dataset resolver (a fresh engine read,
//! then the pure ISO builder).

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::{ActionContext, ActionDiagnostic, ActionSubmitRequest, SubmitIntent};
use serde_json::{Value, json};

use crate::controller::FormCapability;
use crate::field_selection::build_submit_entries;
use crate::promise::await_engine;
use crate::value::{get, num};

/// An all-failed batch: the sink never throws.
pub(crate) fn failed_batch(effects: &[Value], error: Option<(&str, &str)>) -> Value {
    let results: Vec<Value> = (0..effects.len())
        .map(|index| {
            let mut entry = json!({
                "index": index,
                "status": "failed",
                "fields": [],
                "changedWidgets": [],
            });
            if let Some((code, message)) = error {
                entry["error"] = json!({ "code": code, "message": message });
            }
            entry
        })
        .collect();
    json!({ "results": results, "changedWidgets": [], "meta": null })
}

impl FormCapability {
    /// Host lens: `resolveSubmitDataset(intent, ctx, diagnose)`. A fresh
    /// read, never the cached model, then the pure ISO builder.
    pub fn resolve_submit_dataset(
        &self,
        intent: SubmitIntent,
        action_ctx: ActionContext,
        diagnose: Rc<dyn Fn(ActionDiagnostic)>,
    ) -> Promise<ActionSubmitRequest> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("no document"))?;
            let snapshot = await_engine(doc.call("forms.list", None, Vec::new())).await?;
            let fields: Vec<Value> =
                get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
            let entries = build_submit_entries(&fields, &intent, &*diagnose);
            Ok(ActionSubmitRequest {
                url: intent.url.clone(),
                method: intent.method.clone(),
                format: intent.format.clone(),
                flags_raw: intent.flags_raw,
                char_set: intent.char_set.clone(),
                entries,
                origin: action_ctx.origin,
                event: action_ctx.event,
            })
        })
    }

    /// Host lens: `commitScriptFormEffects(effects)`, the document commit
    /// sink. Never throws: a refused batch or a missing batch door resolves
    /// as all-failed. It reconciles this plugin's model, then the annotation
    /// plane's view of every changed widget's page, one page at a time.
    pub fn commit_script_form_effects(&self, effects: Vec<Value>) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = match this.doc() {
                Some(doc) if doc.has("forms.applyEffects") => doc,
                _ => {
                    return Ok(failed_batch(
                        &effects,
                        Some(("NotSupported", "no form-effects batch door")),
                    ));
                }
            };
            let call = doc.call("forms.applyEffects", None, vec![Value::Array(effects.clone())]);
            let result = match await_engine(call).await {
                Ok(result) => result,
                Err(error) => return Ok(failed_batch(&effects, Some(("Refused", &error.message)))),
            };
            this.refresh().await?;
            if let Some(annotation) = &this.0.annotation {
                let mut seen: Vec<f64> = Vec::new();
                for widget in get(&result, "changedWidgets")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                {
                    let Some(page) = get(&widget, "page").filter(|p| !p.is_null()) else {
                        continue;
                    };
                    let pon = num(page, "pageObjectNumber").unwrap_or(f64::NAN);
                    if seen.contains(&pon) {
                        continue;
                    }
                    seen.push(pon);
                    annotation.reload_page(epdf_annotation_core::PageRef::new(pon as u32)).await?;
                }
            }
            Ok(result)
        })
    }
}
