//! `write/reset.ts`: resets, one shared core (one fill: the reset, then the
//! recalculation when the document's scripts run) used by both the
//! ResetForm action executor and the public `reset`, so the two cannot
//! diverge. A reset resets the fields it selects that the user may fill in:
//! another signer's fields stay as they are.

use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_actions::ActionOrigin;
use serde_json::{Value, json};

use crate::contract::{FormCommitResult, FormCommitStatus};
use crate::field_selection::resolve_field_selection;
use crate::model::{field_by_ref, fields_with_changed_values};
use crate::value::{get, str_of};
use crate::write::fill::{FillOutcome, FillRequest, FillScripts};

/// `targetOf(ref)`: a field ref as an action's target.
fn target_of(reference: &Value) -> Value {
    if str_of(reference, "kind") == Some("objectNumber") {
        json!({ "kind": "objectNumber", "objectNumber": get(reference, "objectNumber").cloned().unwrap_or(Value::Null) })
    } else {
        json!({ "kind": "name", "name": get(reference, "name").cloned().unwrap_or(Value::Null) })
    }
}

impl crate::controller::Core {
    /// `resettable(field)`: the engine refuses push button and signature
    /// refs, and another signer's fields are theirs.
    fn resettable(&self, field: &Value) -> bool {
        !matches!(str_of(field, "family"), Some("pushbutton" | "signature")) && self.may_fill(field)
    }

    /// `resetSelection(targets, exclude, origin, options)`: `None` when it
    /// selects nothing; a refused reset rejects.
    pub(crate) fn reset_selection(
        &self,
        targets: Option<Vec<Value>>,
        exclude: bool,
        origin: ActionOrigin,
        signal: Option<AbortSignal>,
    ) -> Promise<Option<FillOutcome>> {
        let me = self.rc();
        Promise::spawn(async move {
            if me.view().snapshot.is_none() {
                me.fields.settled().await?;
            }
            let index = me.view();
            // ISO 32000-2 Tables 241/242: a parent name resets its descendants too.
            let selection = resolve_field_selection(index.fields(), targets.as_deref(), exclude);
            let refs: Vec<Value> = selection
                .selected
                .iter()
                .filter(|f| me.resettable(f))
                .map(|f| get(f, "ref").cloned().unwrap_or(Value::Null))
                .collect();
            if refs.is_empty() {
                return Ok(None);
            }
            let request = FillRequest {
                label: Some(json!({ "key": "form.reset", "count": refs.len() })),
                ops: vec![json!({ "type": "forms.reset", "fields": refs })],
                scripts: Some(FillScripts::Recalculate),
                origin,
            };
            // `return fill(...)` from an async function: adopted.
            Ok(Some(crate::promise::adopt(me.fill(request, signal)).await?))
        })
    }

    /// `reset(refs, options)`: each field's default, for the fields given,
    /// or every field you may fill in, then the calculations. Resolves
    /// `{ fields }`, the fields it changed.
    pub fn reset(&self, refs: Option<Vec<Value>>, signal: Option<AbortSignal>) -> Promise<Value> {
        let me = self.rc();
        Promise::spawn(async move {
            // A field you name is one you mean: one you may not fill in
            // refuses the reset.
            for reference in refs.iter().flatten() {
                if let Some(field) = field_by_ref(&me.view(), reference).cloned() {
                    me.assert_may_fill(&field, "form.reset")?;
                }
            }
            if refs.as_ref().is_some_and(Vec::is_empty) {
                return Ok(json!({ "fields": [] }));
            }
            let targets = refs.map(|refs| refs.iter().map(target_of).collect());
            let outcome = me.reset_selection(targets, false, ActionOrigin::User, signal).await?;
            let fields = match outcome {
                Some(outcome) => fields_with_changed_values(&outcome.before, &outcome.after),
                None => Vec::new(),
            };
            Ok(json!({ "fields": fields }))
        })
    }

    /// `resetFormAction(fields, exclude, origin)`: a ResetForm action.
    pub fn reset_form_action(
        &self,
        targets: Option<Vec<Value>>,
        exclude: bool,
        origin: Option<ActionOrigin>,
    ) -> Promise<FormCommitResult> {
        let me = self.rc();
        Promise::spawn(async move {
            let outcome = me
                .reset_selection(targets, exclude, origin.unwrap_or(ActionOrigin::User), None)
                .await?;
            let Some(outcome) = outcome else {
                return Ok(FormCommitResult::bare(FormCommitStatus::Unchanged, false));
            };
            // The action ran, even on fields already at their defaults.
            let mut result = outcome.result;
            if result.status == FormCommitStatus::Unchanged {
                result.status = FormCommitStatus::Applied;
            }
            Ok(result)
        })
    }
}
