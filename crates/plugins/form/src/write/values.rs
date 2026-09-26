//! `write/values.ts`: value writes. The validated path (scripts when
//! enabled), the raw passthrough and the batch doors, every one through the
//! serial queue.

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::ActionOrigin;
use serde_json::{Map, Value, json};

use crate::contract::{
    BatchFailure, BatchResult, FormCommitResult, FormCommitStatus, FormValidationRejectedEvent,
    SetValueResult,
};
use crate::controller::{FormCapability, key_of};
use crate::core::model::Msg;
use crate::promise::{adopt, await_engine};
use crate::value::get;

fn not_ready() -> Error {
    plugin_error(code::NOT_READY, "form", "no document", None, None)
}

fn fqn(name: &str) -> Value {
    json!({ "kind": "fqn", "name": name })
}

impl FormCapability {
    /// `commitValue(ref, value)`: one value transaction, K/V/C/F included
    /// when scripting is on.
    async fn commit_value(
        &self,
        field_ref: Value,
        value: Value,
    ) -> Result<FormCommitResult, Error> {
        let doc = self.doc().ok_or_else(|| Error::new("no document"))?;
        if let Some(scripting) = &self.0.scripting {
            let result = scripting.commit(field_ref, value).await?;
            self.surface_via_actions(&result, ActionOrigin::User)?;
            // A native partial or failed result can still have mutated state.
            if result.effects_result.is_some() {
                self.refresh().await?;
            }
            return Ok(result);
        }
        let result = await_engine(doc.call("forms.setValue", None, vec![field_ref, value])).await?;
        self.refresh().await?;
        let changed = get(&result, "changedWidgets").and_then(Value::as_array).map_or(0, Vec::len);
        Ok(FormCommitResult::bare(
            if changed > 0 { FormCommitStatus::Applied } else { FormCommitStatus::Unchanged },
            false,
        ))
    }

    /// `write(ref, value)`: the validated path. Refuses without
    /// `doc.forms.fill` before the spinner; marks the field writing, commits,
    /// and clears the mark itself when no read-back will.
    pub(crate) fn write(&self, field_ref: Value, value: Value) -> Promise<SetValueResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_fill("form.setValue")?;
            let key = key_of(&field_ref);
            let queued = {
                let this = this.clone();
                this.0.queue.clone().run(move || {
                    Ok(Promise::spawn(async move {
                        if this.doc().is_none() {
                            return Err(not_ready());
                        }
                        this.apply(Msg::WriteStart { key: key.clone() });
                        let committing = {
                            let (this, field_ref) = (this.clone(), field_ref.clone());
                            Promise::spawn(async move { this.commit_value(field_ref, value).await })
                        };
                        match committing.await {
                            Ok(result) => {
                                let refused = matches!(
                                    result.status,
                                    FormCommitStatus::Rejected | FormCommitStatus::Failed
                                );
                                // A scripted no-op has no read-back to clear the spinner.
                                if refused || (result.effects_result.is_none() && result.scripted) {
                                    this.apply(Msg::WriteFailed { key });
                                }
                                if result.status == FormCommitStatus::Rejected {
                                    this.0.events.validation_rejected.emit(
                                        &FormValidationRejectedEvent {
                                            field_ref,
                                            issues: result.diagnostics.clone(),
                                        },
                                    );
                                }
                                Ok(result)
                            }
                            Err(error) => {
                                this.apply(Msg::WriteFailed { key });
                                Err(to_plugin_error("form", &error))
                            }
                        }
                    }))
                })
            };
            adopt(queued).await
        })
    }

    /// `setValue(ref, value, options?)`: commit a value through the
    /// validated path. Rejects `permission-denied` without `doc.forms.fill`.
    pub fn set_value(&self, field_ref: Value, value: Value) -> Promise<SetValueResult> {
        self.write(field_ref, value)
    }

    /// `setText(ref, text)`.
    pub fn set_text(&self, field_ref: Value, text: Value) -> Promise<SetValueResult> {
        self.write(field_ref, json!({ "type": "text", "value": text }))
    }

    /// `setChecked(ref, onState)`: a checkbox or radio by its on-state; null
    /// clears the group.
    pub fn set_checked(&self, field_ref: Value, on_state: Value) -> Promise<SetValueResult> {
        self.write(field_ref, json!({ "type": "toggle", "state": on_state }))
    }

    /// `setChoice(ref, values)`: choice options by export value.
    pub fn set_choice(&self, field_ref: Value, values: Vec<Value>) -> Promise<SetValueResult> {
        self.write(field_ref, json!({ "type": "choice", "values": values }))
    }

    /// `setValueRaw(ref, value)`: the engine passthrough, no scripts, then a
    /// re-read.
    pub fn set_value_raw(&self, field_ref: Value, value: Value) -> Promise<Value> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.doc().ok_or_else(not_ready)?;
                let result =
                    await_engine(doc.call("forms.setValue", None, vec![field_ref, value])).await?;
                this.refresh().await?;
                Ok(result)
            }))
        })
    }

    /// `writeBatch(entries, run, refOf)`: each entry in order, best effort.
    fn write_batch(&self, entries: Vec<(Value, Value, Value)>) -> Promise<BatchResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let mut out = BatchResult::default();
            for (entry, field_ref, value) in entries {
                match this.write(field_ref.clone(), value).await {
                    Ok(result) => {
                        if matches!(
                            result.status,
                            FormCommitStatus::Rejected | FormCommitStatus::Failed
                        ) {
                            let code = if result.status == FormCommitStatus::Rejected {
                                "invalid-input"
                            } else {
                                "operation-failed"
                            };
                            let message = result
                                .error
                                .as_ref()
                                .map(|e| e.message.clone())
                                .or_else(|| result.diagnostics.first().map(|d| d.message.clone()))
                                .unwrap_or_else(|| result.status.as_str().to_string());
                            out.failed.push(BatchFailure {
                                entry,
                                error: json!({ "code": code, "message": message, "capability": "form" }),
                            });
                        } else {
                            out.applied.push(field_ref);
                        }
                    }
                    Err(error) => out.failed.push(BatchFailure {
                        entry,
                        error: to_plugin_error("form", &error).plugin_error_info(),
                    }),
                }
            }
            Ok(out)
        })
    }

    /// `setValues(entries)`: several values, queued in order, best effort
    /// per field. `entries` are `{ref, value}` pairs.
    pub fn set_values(&self, entries: Vec<(Value, Value)>) -> Promise<BatchResult> {
        let batch = self.write_batch(
            entries
                .into_iter()
                .map(|(field_ref, value)| (field_ref.clone(), field_ref, value))
                .collect(),
        );
        Promise::spawn(async move {
            // The failed entries are reported by their refs.
            batch.await
        })
    }

    /// `importValues(values)`: set values from an object keyed by fully
    /// qualified name; a failure is reported by its name.
    pub fn import_values(&self, values: Map<String, Value>) -> Promise<BatchResult> {
        self.write_batch(
            values.into_iter().map(|(name, value)| (json!(name), fqn(&name), value)).collect(),
        )
    }
}
