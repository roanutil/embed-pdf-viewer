//! `write/reset.ts`: resets. The shared reset core (one effects batch, a
//! re-read, then the V/C/F recalculation when scripting is on) is ridden by
//! both doors, the `/ResetForm` executor and the public `reset` and
//! `resetAll`, so the two can never diverge.

use epdf_acrojs::{ScriptDiagnostic, ScriptDiagnosticCode};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::{ActionOrigin, PdfActionTargetRef};
use serde_json::{Value, json};

use crate::contract::{
    BatchFailure, BatchResult, FormCommitResult, FormCommitStatus, ResetAllOptions,
};
use crate::controller::{FormCapability, key_of};
use crate::core::model::{Msg, field_by_key};
use crate::field_selection::resolve_field_selection;
use crate::promise::{adopt, await_engine};
use crate::value::{get, str_of, string_of};

fn not_scripted() -> FormCommitResult {
    FormCommitResult::bare(FormCommitStatus::Unchanged, false)
}

fn is_refused(entry: &Value) -> bool {
    matches!(str_of(entry, "status"), Some("failed" | "rejected"))
}

fn effect_entries(result: Option<&Value>) -> Vec<Value> {
    result.and_then(|r| get(r, "results")).and_then(Value::as_array).cloned().unwrap_or_default()
}

/// Buttons and signatures have nothing to restore: the engine's batch
/// applier refuses them outright.
fn resettable_refs(selected: &[Value]) -> Vec<Value> {
    selected
        .iter()
        .filter(|f| !matches!(str_of(f, "family"), Some("pushbutton" | "signature")))
        .map(|f| get(f, "ref").cloned().unwrap_or_else(epdf_kernel::undefined))
        .collect()
}

fn fields_of(snapshot: &Value) -> Vec<Value> {
    get(snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default()
}

impl FormCapability {
    /// `applyResetBatch(refs, origin)`: one effects batch, a re-read, then
    /// the `/CO` recalculation when scripting is on. `origin` is kept through
    /// the recalculation's surfacing.
    async fn apply_reset_batch(
        &self,
        refs: Vec<Value>,
        origin: ActionOrigin,
    ) -> Result<FormCommitResult, Error> {
        let doc = self.doc().ok_or_else(|| Error::new("no document"))?;
        if !doc.has("forms.applyEffects") {
            let result = FormCommitResult {
                diagnostics: vec![ScriptDiagnostic {
                    code: ScriptDiagnosticCode::UnsupportedApi,
                    message: "this engine has no form-effects batch door".into(),
                }],
                ..not_scripted()
            };
            self.surface_via_actions(&result, origin)?;
            return Ok(result);
        }
        // Zero refs never reach the engine: `[]` with include resets nothing.
        if refs.is_empty() {
            return Ok(not_scripted());
        }
        let effects = json!([{ "kind": "reset", "refs": refs }]);
        let effects_result =
            await_engine(doc.call("forms.applyEffects", None, vec![effects])).await?;
        let reset_failed = effect_entries(Some(&effects_result)).iter().any(is_refused);
        self.refresh().await?;
        if reset_failed {
            return Ok(FormCommitResult {
                effects_result: Some(effects_result),
                ..FormCommitResult::bare(FormCommitStatus::Failed, false)
            });
        }
        // Acrobat recalculates after a reset; boot rides along lazily.
        let mut recalc = None;
        if let Some(scripting) = &self.0.scripting {
            let result = scripting.recalculate().await?;
            self.surface_via_actions(&result, origin)?;
            if result.effects_result.is_some() {
                self.refresh().await?;
            }
            recalc = Some(result);
        }
        Ok(FormCommitResult {
            status: FormCommitStatus::Applied,
            scripted: recalc.is_some(),
            effects_result: Some(effects_result),
            ui_effects: recalc.as_ref().map(|r| r.ui_effects.clone()).unwrap_or_default(),
            diagnostics: recalc.as_ref().map(|r| r.diagnostics.clone()).unwrap_or_default(),
            error: recalc.and_then(|r| r.error),
        })
    }

    /// Host lens: `resetFormAction(fields, exclude, origin = 'user')`. One
    /// ResetForm action: the ISO selection over a fresh read, buttons and
    /// signatures skipped, one batch.
    pub fn reset_form_action(
        &self,
        targets: Option<Vec<PdfActionTargetRef>>,
        exclude: bool,
        origin: Option<ActionOrigin>,
    ) -> Promise<FormCommitResult> {
        let origin = origin.unwrap_or(ActionOrigin::User);
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.doc().ok_or_else(|| Error::new("no document"))?;
                let snapshot = await_engine(doc.call("forms.list", None, Vec::new())).await?;
                let fields = fields_of(&snapshot);
                let selection = resolve_field_selection(&fields, targets.as_deref(), exclude);
                let refs = resettable_refs(&selection.selected);
                let batch = {
                    let this = this.clone();
                    Promise::spawn(async move { this.apply_reset_batch(refs, origin).await })
                };
                adopt(batch).await
            }))
        })
    }

    /// `resetAll({fields, exclude})`: reset the form, a set of fields, or
    /// everything but a set of fields.
    pub fn reset_all(&self, options: ResetAllOptions) -> Promise<BatchResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_fill("form.resetAll")?;
            let targets: Option<Vec<Value>> = options.fields.as_ref().map(|fields| {
                fields
                    .iter()
                    .map(|r| {
                        if str_of(r, "kind") == Some("objectNumber") {
                            json!({ "kind": "objectNumber", "objectNumber": crate::value::member(r, "fieldObjectNumber") })
                        } else {
                            json!({ "kind": "name", "name": crate::value::member(r, "name") })
                        }
                    })
                    .collect()
            });
            let exclude = options.exclude.unwrap_or(false);
            let snapshot = match this.model().snapshot.clone() {
                Some(snapshot) => snapshot,
                None => {
                    let doc = this.doc().ok_or_else(|| Error::new("no document"))?;
                    std::rc::Rc::new(await_engine(doc.call("forms.list", None, Vec::new())).await?)
                }
            };
            let fields = fields_of(&snapshot);
            let selection = resolve_field_selection(&fields, targets.as_deref(), exclude);
            let refs = resettable_refs(&selection.selected);
            let result = this.reset_form_action(targets, exclude, Some(ActionOrigin::User)).await?;
            if result.status == FormCommitStatus::Failed {
                let failed: Vec<Value> = effect_entries(result.effects_result.as_ref())
                    .iter()
                    .filter(|e| is_refused(e))
                    .flat_map(|e| {
                        get(e, "fields")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|f| get(&f, "ref").cloned().unwrap_or(Value::Null))
                    })
                    .collect();
                let failed_keys: Vec<String> = failed.iter().map(key_of).collect();
                return Ok(BatchResult {
                    applied: refs.into_iter().filter(|r| !failed_keys.contains(&key_of(r))).collect(),
                    skipped: Vec::new(),
                    failed: failed
                        .into_iter()
                        .map(|entry| BatchFailure {
                            entry,
                            error: json!({ "code": "operation-failed", "message": "reset failed", "capability": "form" }),
                        })
                        .collect(),
                });
            }
            Ok(BatchResult { applied: refs, skipped: Vec::new(), failed: Vec::new() })
        })
    }

    /// `reset(ref)`: restore one field's default value, through the same
    /// batch and recalculation a ResetForm action takes.
    pub fn reset(&self, field_ref: Value) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_fill("form.reset")?;
            let key = key_of(&field_ref);
            let queued = {
                let this = this.clone();
                this.0.queue.clone().run(move || {
                    Ok(Promise::spawn(async move {
                        let Some(doc) = this.doc() else { return Ok(()) };
                        this.apply(Msg::WriteStart { key: key.clone() });
                        let outcome: Result<(), Error> = async {
                            if doc.has("forms.applyEffects") {
                                let batch = {
                                    let (this, field_ref) = (this.clone(), field_ref.clone());
                                    Promise::spawn(async move {
                                        this.apply_reset_batch(vec![field_ref], ActionOrigin::User)
                                            .await
                                    })
                                };
                                let result = batch.await?;
                                if result.status == FormCommitStatus::Failed {
                                    let message = effect_entries(result.effects_result.as_ref())
                                        .iter()
                                        .find(|e| is_refused(e))
                                        .and_then(|e| get(e, "error"))
                                        .and_then(|e| get(e, "message"))
                                        .filter(|m| !m.is_null())
                                        .map(|m| string_of(Some(m)))
                                        .unwrap_or_else(|| "reset failed".into());
                                    return Err(plugin_error(
                                        code::OPERATION_FAILED,
                                        "form",
                                        message,
                                        None,
                                        None,
                                    ));
                                }
                                match field_by_key(&this.model(), &key) {
                                    Some(field) => {
                                        this.apply(Msg::WriteDone { key: key.clone(), field })
                                    }
                                    None => this.apply(Msg::WriteFailed { key: key.clone() }),
                                }
                            } else {
                                let result = await_engine(doc.call(
                                    "forms.reset",
                                    None,
                                    vec![field_ref.clone()],
                                ))
                                .await?;
                                let field = get(&result, "field").cloned().unwrap_or(Value::Null);
                                this.apply(Msg::WriteDone { key: key.clone(), field });
                            }
                            Ok(())
                        }
                        .await;
                        if let Err(error) = outcome {
                            this.apply(Msg::WriteFailed { key });
                            return Err(to_plugin_error("form", &error));
                        }
                        Ok(())
                    }))
                })
            };
            adopt(queued).await
        })
    }
}
