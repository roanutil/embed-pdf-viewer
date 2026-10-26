//! `write/script-effects.ts`: the actions plugin's seams into the form
//! plugin: the form-effects commit sink (script `setValue`, `setDisplay` and
//! reset effects are written through the plugin that owns the fields, as one
//! fill with the calculations they cause) and the submit dataset resolver.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::{
    ActionContext, ActionDiagnostic, ActionOrigin, ActionSubmitRequest, FormCommitSinkResult,
    FormCommitStatus as SinkStatus, SubmitIntent,
};
use serde_json::Value;

use crate::engine::form_effect_ops;
use crate::field_selection::build_submit_entries;
use crate::model::field_by_ref;
use crate::value::{get, items, str_of};
use crate::write::fill::{FillRequest, FillScripts};

/// `refsOf(effects)`: the fields a script's effects write to.
fn refs_of(effects: &[Value]) -> Vec<Value> {
    effects
        .iter()
        .flat_map(|effect| {
            if str_of(effect, "kind") == Some("reset") {
                items(effect, "refs").to_vec()
            } else {
                vec![get(effect, "ref").cloned().unwrap_or(Value::Null)]
            }
        })
        .collect()
}

impl crate::controller::Core {
    /// `resolveSubmitDataset(intent, actionContext, diagnose)`: the fields as
    /// the user sees them once every fill on its way is answered, then the
    /// pure ISO builder.
    pub fn resolve_submit_dataset(
        &self,
        intent: SubmitIntent,
        action_context: ActionContext,
        diagnose: Rc<dyn Fn(ActionDiagnostic)>,
    ) -> Promise<ActionSubmitRequest> {
        let me = self.rc();
        Promise::spawn(async move {
            me.enqueue.idle().await?;
            me.cx.changes()?.when_settled().await?;
            if me.view().snapshot.is_none() {
                me.fields.settled().await?;
            }
            let entries = build_submit_entries(me.view().fields(), &intent, &|d| diagnose(d));
            Ok(ActionSubmitRequest {
                url: intent.url.clone(),
                method: intent.method.clone(),
                format: intent.format.clone(),
                flags_raw: intent.flags_raw,
                char_set: intent.char_set.clone(),
                entries,
                origin: action_context.origin,
                event: action_context.event.clone(),
            })
        })
    }

    /// `commitScriptFormEffects(effects)`: write what a document script
    /// changed, with the calculations a value causes, as one change. Never
    /// rejects; a change it can't stage reports why.
    pub fn commit_script_form_effects(&self, effects: Vec<Value>) -> Promise<FormCommitSinkResult> {
        let me = self.rc();
        Promise::spawn(async move {
            let attempt = (|| -> Result<(), Error> {
                // A field the session may not fill in is refused here, at once.
                for reference in refs_of(&effects) {
                    if let Some(field) = field_by_ref(&me.view(), &reference).cloned() {
                        me.assert_may_fill(&field, "form.fill")?;
                    }
                }
                // The script's realm transaction is still running: the
                // effects show at once, and the calculations a value causes
                // run once it ends, in the same change.
                let recalculates =
                    effects.iter().any(|e| matches!(str_of(e, "kind"), Some("setValue" | "reset")));
                me.fill_soon(FillRequest {
                    label: None,
                    ops: form_effect_ops(&effects),
                    scripts: recalculates.then_some(FillScripts::Recalculate),
                    origin: ActionOrigin::User,
                })
            })();
            Ok(match attempt {
                Ok(()) => FormCommitSinkResult { status: SinkStatus::Applied, error: None },
                Err(error) => FormCommitSinkResult {
                    status: SinkStatus::Failed,
                    error: Some(crate::engine::serialized_message(&error)),
                },
            })
        })
    }
}
