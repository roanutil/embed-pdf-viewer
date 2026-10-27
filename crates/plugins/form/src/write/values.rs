//! `write/values.ts`: value writes, each one fill (`write/fill.rs`): one
//! field through the form's scripts (when the `validation` setting runs
//! them), and the batch verbs, whose plain values go as one change with one
//! recalculation, as a script's `field.value` assignments do in Acrobat.

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_actions::ActionOrigin;
use serde_json::{Value, json};

use crate::contract::FormCommitStatus;
use crate::controller::CAPABILITY;
use crate::engine::field_value_write_of;
use crate::model::field_by_ref;
use crate::read::fields::write_of_plain_value;
use crate::value::get;
use crate::write::fill::{FillRequest, FillScripts, failed_fill};

/// `throwIfCancelled(options)`: a batch that stops when its caller cancels it.
fn throw_if_cancelled(signal: Option<&AbortSignal>) -> Result<(), Error> {
    match signal.filter(|s| s.aborted()) {
        Some(signal) => Err(plugin_error(
            code::OPERATION_CANCELLED,
            CAPABILITY,
            "operation cancelled",
            signal.reason().map(|e| e.to_canonical()),
            None,
        )),
        None => Ok(()),
    }
}

fn set_value_op(field: &Value, value: &Value) -> Value {
    json!({ "type": "forms.setValue", "field": field, "value": value })
}

fn info(error: &Error) -> Value {
    to_plugin_error(CAPABILITY, error).plugin_error_info()
}

/// What `fillPlain` did.
#[derive(Clone, Default)]
pub(crate) struct Plain {
    pub applied: Vec<Value>,
    pub failed: Vec<(Value, Value)>,
}

impl crate::controller::Core {
    /// `commit(ref, value, operation, options)`: one field's value through
    /// its scripts, shown at once, written as one change. Resolves
    /// `{ field, status }`.
    pub(crate) fn commit_value(
        &self,
        reference: Value,
        value: Value,
        operation: &'static str,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            // A field the form doesn't show yet is the engine's to answer.
            if let Some(known) = field_by_ref(&me.view(), &reference) {
                me.assert_may_fill(&known.clone(), operation)?;
            }
            let request = FillRequest {
                label: None,
                ops: vec![set_value_op(&reference, &value)],
                scripts: Some(FillScripts::Commit {
                    field: reference.clone(),
                    value: value.clone(),
                }),
                origin: ActionOrigin::User,
            };
            let outcome = me.fill(request, signal).await?;
            let result = outcome.result;
            if result.status == FormCommitStatus::Failed {
                return Err(failed_fill(&result));
            }
            let Some(field) = field_by_ref(&me.truth(), &reference).cloned() else {
                return Err(me.error(code::NOT_FOUND, "no form field has this ref"));
            };
            if result.status == FormCommitStatus::Rejected {
                let issues = epdf_kernel::js::to_value(&result.diagnostics);
                me.events.validation_rejected.emit(&json!({ "field": field, "issues": issues }));
                return Ok(json!({ "field": field, "status": "rejected" }));
            }
            Ok(json!({ "field": field, "status": epdf_kernel::js::to_value(&result.status) }))
        })
    }

    /// `setValue(ref, value, options)`.
    pub fn set_value(
        &self,
        reference: Value,
        value: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        self.commit_value(reference, value, "form.setValue", signal)
    }

    /// `fillPlain(entries, options)`: plain values for several fields, as
    /// one change with one recalculation. A value its field can't take, or
    /// a field the session may not fill in, fails on its own; the rest go
    /// together, and fail together when the engine refuses them.
    pub(crate) fn fill_plain(
        &self,
        entries: Vec<(Value, Value)>,
        signal: Option<AbortSignal>,
    ) -> Promise<Plain> {
        let me = self.rc();
        Promise::spawn(async move {
            if me.view().snapshot.is_none() {
                me.fields.settled().await?;
            }
            let index = me.view();
            let mut accepted: Vec<(Value, Value)> = Vec::new();
            let mut failed: Vec<(Value, Value)> = Vec::new();
            for (reference, value) in entries {
                let checked = (|| -> Result<Value, Error> {
                    let Some(field) = field_by_ref(&index, &reference) else {
                        return Err(me.error(code::NOT_FOUND, "no form field has this ref"));
                    };
                    me.assert_may_fill(field, "form.setValues")?;
                    field_value_write_of(field, &value)?;
                    Ok(get(field, "ref").cloned().unwrap_or(Value::Null))
                })();
                match checked {
                    Ok(own) => accepted.push((own, value)),
                    Err(error) => failed.push((reference, info(&error))),
                }
            }
            if accepted.is_empty() {
                return Ok(Plain { applied: Vec::new(), failed });
            }
            throw_if_cancelled(signal.as_ref())?;
            let request = FillRequest {
                label: None,
                ops: accepted.iter().map(|(r, v)| set_value_op(r, v)).collect(),
                scripts: Some(FillScripts::Recalculate),
                origin: ActionOrigin::User,
            };
            let outcome = match me.fill(request, signal).await {
                Ok(outcome) if outcome.result.status == FormCommitStatus::Failed => {
                    Err(failed_fill(&outcome.result))
                }
                other => other,
            };
            match outcome {
                Ok(_) => {
                    Ok(Plain { applied: accepted.into_iter().map(|(r, _)| r).collect(), failed })
                }
                Err(error) => {
                    let info = info(&error);
                    failed.extend(accepted.into_iter().map(|(r, _)| (r, info.clone())));
                    Ok(Plain { applied: Vec::new(), failed })
                }
            }
        })
    }

    /// `setValues(entries, options)`: several fields, in order; one that
    /// fails doesn't stop the rest. Resolves a `BatchResult`.
    pub fn set_values(
        &self,
        entries: Vec<(Value, Value)>,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            throw_if_cancelled(signal.as_ref())?;
            let plain = me.fill_plain(entries, signal).await?;
            let failed: Vec<Value> =
                plain.failed.into_iter().map(|(r, e)| json!({ "ref": r, "error": e })).collect();
            Ok(json!({ "applied": plain.applied, "skipped": [], "failed": failed }))
        })
    }

    /// `importValues(values, options)`: fields by full name from plain
    /// values. A name the form doesn't have, a field you may not fill in, or
    /// a value its field can't take, is skipped; the rest still land. The
    /// names are read in the order a plain object holds them (A5).
    pub fn import_values(&self, values: Value, signal: Option<AbortSignal>) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            throw_if_cancelled(signal.as_ref())?;
            if me.view().snapshot.is_none() {
                me.fields.settled().await?;
            }
            let index = me.view();
            let mut skipped: Vec<Value> = Vec::new();
            let mut entries: Vec<(Value, Value)> = Vec::new();
            let mut names: Vec<(String, String)> = Vec::new();
            let object = values.as_object().cloned().unwrap_or_default();
            for (name, plain) in crate::value::js_key_order(object) {
                let Some(field) = field_by_ref(&index, &json!({ "kind": "fqn", "name": name }))
                else {
                    skipped.push(
                        json!({ "ref": name, "reason": "the form has no field with this name" }),
                    );
                    continue;
                };
                if !me.may_fill(field) {
                    skipped
                        .push(json!({ "ref": name, "reason": "you may not fill in this field" }));
                    continue;
                }
                let Some(value) = write_of_plain_value(field, &plain) else {
                    let family = crate::value::string_of(get(field, "family"));
                    skipped.push(json!({ "ref": name, "reason": format!("a {family} field can't take this value") }));
                    continue;
                };
                let own = get(field, "ref").cloned().unwrap_or(Value::Null);
                names.push((crate::value::json_stringify(&own), name.clone()));
                entries.push((own, value));
            }
            let plain = me.fill_plain(entries, signal).await?;
            let failed: Vec<Value> = plain
                .failed
                .into_iter()
                .map(|(r, e)| {
                    let key = crate::value::json_stringify(&r);
                    // `names.get(...)`: a later name under the same ref wins.
                    let name = names.iter().rev().find(|(k, _)| *k == key).map(|(_, n)| n.clone());
                    json!({ "ref": name.unwrap_or_default(), "error": e })
                })
                .collect();
            Ok(json!({ "applied": plain.applied, "skipped": skipped, "failed": failed }))
        })
    }
}
