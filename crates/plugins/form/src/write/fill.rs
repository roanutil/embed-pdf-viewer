//! `write/fill.ts`: filling the form in. Every fill is one change, whatever
//! started it: a value, a batch of values, a reset, a widget's activation, a
//! script's or an action's effects.
//!
//!   1. What the user did shows at once: the change is held with the user's
//!      own ops.
//!   2. When its turn comes, the form's scripts run against the view, in one
//!      realm transaction.
//!   3. Inside that transaction the change gets every op the scripts asked
//!      for, and is sent.
//!
//! Fills take turns up to their send, not their answer: the next fill's
//! scripts read this one in the view while the engine answers it. A
//! validation refusal cancels the change. A change the engine refuses leaves
//! the view, and the calculations run again against what is left, outside
//! the history.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::changes::{HeldChange, PendingChange, SettledChange, StagedChange};
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_actions::ActionOrigin;
use serde_json::{Value, json};

use crate::contract::{FormCommitResult, FormCommitStatus};
use crate::engine::{field_value_of, form_effect_ops};
use crate::model::{FieldIndex, field_by_key, field_key_of};
use crate::scripting::controller::{FormScriptResult, FormScripts, ScriptStatus};
use crate::scripting::results::script_error;
use crate::services::scripting::Surfaced;
use crate::sync::predict::{fields_named_by, predict_form_op, undo_look_of, with_fields_from};
use crate::value::{get, json_stringify};

/// The scripts a fill runs before it is sent, when the document's scripts run.
#[derive(Clone, Debug)]
pub enum FillScripts {
    /// A field's new value through its own scripts, then the calculations.
    Commit { field: Value, value: Value },
    /// A widget's activation action: what it changes is the change.
    Activate { field: Value, action: Value },
    /// The calculations the user's ops cause.
    Recalculate,
}

/// `FillRequest`.
#[derive(Clone, Debug)]
pub struct FillRequest {
    /// What the change is called; a fill of the fields its ops name when absent.
    pub label: Option<Value>,
    /// What the user did: shown at once.
    pub ops: Vec<Value>,
    pub scripts: Option<FillScripts>,
    /// Who started it, for the alerts its scripts raise.
    pub origin: ActionOrigin,
}

/// `FillOutcome`: how a fill ended, once the engine answered.
#[derive(Clone)]
pub struct FillOutcome {
    pub result: FormCommitResult,
    /// The form before the fill, as the view showed it.
    pub before: Rc<FieldIndex>,
    /// The form once the fill had all its ops, as the view showed it.
    pub after: Rc<FieldIndex>,
}

/// A fill shown, waiting for its turn.
#[derive(Clone)]
struct Begun {
    request: FillRequest,
    /// The form before the user's ops showed.
    before: Rc<FieldIndex>,
    /// The form as the user's ops showed it.
    shown: Rc<FieldIndex>,
    hold: Option<HeldChange>,
}

/// A fill sent, waiting for the engine.
#[derive(Clone)]
struct Sent {
    script: Option<FormScriptResult>,
    /// The changes it was sent as: one, or two when the hold went first.
    changes: Vec<PendingChange>,
    after: Rc<FieldIndex>,
}

const SCRIPTS_LABEL: &str = "form.scripts";
const RECALCULATE_LABEL: &str = "form.recalculate";

fn same_op(left: &Value, right: &Value) -> bool {
    json_stringify(left) == json_stringify(right)
}

fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    let entry =
        |f: Option<&Value>| f.and_then(|f| f.get("valueEntry")).cloned().unwrap_or(Value::Null);
    json_stringify(&entry(left)) == json_stringify(&entry(right))
}

fn staged(label: Value, ops: Vec<Value>, undo: Vec<Value>, history: bool) -> StagedChange {
    let mut change = StagedChange::ops(label, ops);
    change.undo = undo;
    change.history = history;
    change
}

/// `failedFill(result)`: a fill that failed in the form's own scripts.
pub fn failed_fill(result: &FormCommitResult) -> Error {
    let message = result
        .error
        .as_ref()
        .map(|e| e.message.clone())
        .or_else(|| result.diagnostics.first().map(|d| d.message.clone()))
        .unwrap_or_else(|| "the write failed".into());
    plugin_error(code::OPERATION_FAILED, crate::controller::CAPABILITY, message, None, None)
}

fn to_form_error(error: &Error) -> Error {
    to_plugin_error(crate::controller::CAPABILITY, error)
}

impl crate::controller::Core {
    fn label_of(&self, request: &FillRequest, index: &FieldIndex, ops: &[Value]) -> Value {
        request.label.clone().unwrap_or_else(
            || json!({ "key": "form.fill", "count": fields_named_by(index, ops).len() }),
        )
    }

    /// Step 1: what the user did shows at once.
    fn begin(&self, request: FillRequest) -> Result<Begun, Error> {
        let before = self.view();
        let shown =
            request.ops.iter().fold(before.clone(), |index, op| predict_form_op(&index, op));
        if request.ops.is_empty() {
            return Ok(Begun { request, before, shown, hold: None });
        }
        let hold = self.cx.changes()?.hold(self.label_of(&request, &before, &request.ops), None)?;
        hold.set(request.ops.clone(), undo_look_of(&before, &request.ops));
        Ok(Begun { request, before, shown, hold: Some(hold) })
    }

    /// `baseOf(begun)`: the form the change is undone to: as it is now, with
    /// the user's own fields as they were before the user's ops showed.
    fn base_of(&self, begun: &Begun) -> Rc<FieldIndex> {
        with_fields_from(
            &self.view(),
            &begun.before,
            &fields_named_by(&begun.before, &begun.request.ops),
        )
    }

    /// `sentEarly(begun)`: the hold went with the user's ops before the
    /// scripts finished.
    fn sent_early(begun: &Begun) -> bool {
        begun.hold.as_ref().is_some_and(|hold| !hold.open() && hold.change().is_some())
    }

    /// `stillShown(begun)`: the form still shows the values the user's ops
    /// wrote.
    fn still_shown(&self, begun: &Begun) -> bool {
        let now = self.view();
        fields_named_by(&begun.shown, &begun.request.ops)
            .iter()
            .all(|field| same_value(field_by_key(&now, &field_key_of(field)), Some(field)))
    }

    /// Step 3: the change with all its ops, sent.
    fn send(
        &self,
        begun: &Begun,
        ops: Vec<Value>,
        script: Option<FormScriptResult>,
    ) -> Result<Sent, Error> {
        let changes = self.cx.changes()?;
        if ops.is_empty() {
            if let Some(hold) = &begun.hold {
                hold.cancel();
            }
            return Ok(Sent { script, changes: Vec::new(), after: self.view() });
        }
        let Some(hold) = &begun.hold else {
            let label = self.label_of(&begun.request, &self.view(), &ops);
            let undo = undo_look_of(&self.base_of(begun), &ops);
            let change = changes.stage(staged(label, ops, undo, true))?;
            return Ok(Sent { script, changes: vec![change], after: self.view() });
        };
        if hold.open() {
            hold.set(ops.clone(), undo_look_of(&self.base_of(begun), &ops));
            let change = hold.send();
            return Ok(Sent { script, changes: change.into_iter().collect(), after: self.view() });
        }
        // Something sent the hold with the user's ops while the scripts ran
        // (another plugin's change, a download): the rest is a change of
        // its own.
        let sent = hold.change();
        let shown = sent.as_ref().map(|c| c.shows()).unwrap_or_default();
        let rest: Vec<Value> =
            ops.into_iter().filter(|op| !shown.iter().any(|s| same_op(s, op))).collect();
        let mut out: Vec<PendingChange> = sent.into_iter().collect();
        if !rest.is_empty() {
            let label = self.label_of(&begun.request, &self.view(), &rest);
            let undo = undo_look_of(&self.view(), &rest);
            out.push(changes.stage(staged(label, rest, undo, true))?);
        }
        Ok(Sent { script, changes: out, after: self.view() })
    }

    /// `takeBack(begun)`: the user's ops went on their own and the scripts
    /// refused them: their fields are written back.
    fn take_back(&self, begun: &Begun) -> Result<(), Error> {
        let ops: Vec<Value> = fields_named_by(&begun.before, &begun.request.ops)
            .iter()
            .filter_map(|field| {
                field_value_of(field).map(|value| {
                    json!({ "type": "forms.setValue", "field": get(field, "ref").cloned().unwrap_or(Value::Null), "value": value })
                })
            })
            .collect();
        if ops.is_empty() {
            return Ok(());
        }
        let label = self.label_of(&begun.request, &begun.before, &ops);
        self.cx.changes()?.stage(staged(label, ops, Vec::new(), false))?;
        Ok(())
    }

    /// `recalculateNow(scripts, origin)`: the calculations again, against
    /// what the form shows now: one change, outside the history.
    fn recalculate_now(&self, scripts: FormScripts, origin: ActionOrigin) -> Promise<()> {
        let me = self.rc();
        Promise::spawn(async move {
            let Some(shown) = me.view().snapshot.clone() else { return Ok(()) };
            let script = scripts.recalculate(shown).await?;
            me.surface(
                Surfaced {
                    ui_effects: &script.ui_effects,
                    diagnostics: &script.diagnostics,
                    error: script.error.as_ref(),
                },
                origin,
            )?;
            if script.status != ScriptStatus::Accepted || script.effects.is_empty() {
                return Ok(());
            }
            let ops = form_effect_ops(&script.effects);
            me.cx.changes()?.stage(staged(
                json!({ "key": RECALCULATE_LABEL }),
                ops,
                Vec::new(),
                false,
            ))?;
            Ok(())
        })
    }

    /// Step 2: the scripts, against the view.
    fn run_scripts(
        &self,
        scripts: &FormScripts,
        begun: &Begun,
        plan: &FillScripts,
    ) -> Option<Promise<FormScriptResult>> {
        let shown = self.view().snapshot.clone()?;
        Some(match plan {
            FillScripts::Commit { field, value } => {
                // The value's own scripts read the form without it.
                let form = self.base_of(begun).snapshot.clone().unwrap_or(shown);
                scripts.commit(form, field.clone(), value.clone())
            }
            FillScripts::Activate { field, action } => {
                scripts.activate(shown, field.clone(), action.clone())
            }
            FillScripts::Recalculate => scripts.recalculate(shown),
        })
    }

    /// Steps 2 and 3, when the fill's turn comes.
    fn finish(&self, begun: Begun) -> Promise<Sent> {
        let me = self.rc();
        Promise::spawn(async move {
            let plan = begun.request.scripts.clone();
            let pipeline = match &plan {
                None => None,
                Some(FillScripts::Activate { .. }) => me.script_controller(),
                Some(_) => me.scripts_for(&me.view()),
            };
            let (Some(plan), Some(pipeline)) = (plan, pipeline) else {
                return me.send(&begun, begun.request.ops.clone(), None);
            };
            if me.view().snapshot.is_none() {
                me.fields.settled().await?;
            }
            let body_me = me.clone();
            let body_begun = begun.clone();
            let transaction = pipeline.transaction(move |scripts: FormScripts| {
                let me = body_me;
                let begun = body_begun;
                Ok(Promise::spawn(async move {
                    let Some(running) = me.run_scripts(&scripts, &begun, &plan) else {
                        return me.send(&begun, begun.request.ops.clone(), None);
                    };
                    let script = running.await?;
                    me.surface(
                        Surfaced {
                            ui_effects: &script.ui_effects,
                            diagnostics: &script.diagnostics,
                            error: script.error.as_ref(),
                        },
                        begun.request.origin,
                    )?;
                    let early = Self::sent_early(&begun);
                    if script.status != ScriptStatus::Accepted {
                        if let Some(hold) = &begun.hold {
                            hold.cancel();
                        }
                        if early {
                            me.take_back(&begun)?;
                        }
                        if script.status == ScriptStatus::Rejected
                            && !script.boot_effects.is_empty()
                        {
                            let ops = form_effect_ops(&script.boot_effects);
                            me.cx.changes()?.stage(staged(
                                json!({ "key": SCRIPTS_LABEL }),
                                ops,
                                Vec::new(),
                                false,
                            ))?;
                        }
                        return Ok(Sent {
                            script: Some(script),
                            changes: Vec::new(),
                            after: me.view(),
                        });
                    }
                    if early && !me.still_shown(&begun) {
                        // The form shows other values for the user's fields now
                        // (an undo, a later fill): the calculations run again.
                        me.recalculate_now(scripts.clone(), begun.request.origin).await?;
                        let change = begun.hold.as_ref().and_then(HeldChange::change);
                        return Ok(Sent {
                            script: Some(script),
                            changes: change.into_iter().collect(),
                            after: me.view(),
                        });
                    }
                    let effects = form_effect_ops(&script.effects);
                    let ops = if matches!(plan, FillScripts::Recalculate) {
                        begun.request.ops.iter().cloned().chain(effects).collect()
                    } else {
                        effects
                    };
                    me.send(&begun, ops, Some(script))
                }))
            });
            match transaction.await {
                Ok(sent) => Ok(sent),
                Err(error) => {
                    if let Some(hold) = &begun.hold {
                        hold.cancel();
                    }
                    Err(error)
                }
            }
        })
    }

    /// What a sent fill did, once the engine answered.
    fn answered(&self, begun: Begun, sent: Sent) -> Promise<FillOutcome> {
        Promise::spawn(async move {
            let script = sent.script.clone();
            let outcome = |status: FormCommitStatus| FillOutcome {
                result: FormCommitResult {
                    status,
                    scripted: script.is_some(),
                    ui_effects: script.as_ref().map(|s| s.ui_effects.clone()).unwrap_or_default(),
                    diagnostics: script.as_ref().map(|s| s.diagnostics.clone()).unwrap_or_default(),
                    error: script.as_ref().and_then(|s| s.error.clone()),
                },
                before: begun.before.clone(),
                after: sent.after.clone(),
            };
            match script.as_ref().map(|s| s.status) {
                Some(ScriptStatus::Failed) => return Ok(outcome(FormCommitStatus::Failed)),
                Some(ScriptStatus::Rejected) => return Ok(outcome(FormCommitStatus::Rejected)),
                _ => {}
            }
            let mut undoable = false;
            for change in &sent.changes {
                match change.result().await {
                    Ok(result) => {
                        let flag = result.get("meta").and_then(|m| m.get("undoable"));
                        undoable = crate::value::truthy(flag) || undoable;
                    }
                    Err(error) => return Err(to_form_error(&error)),
                }
            }
            Ok(outcome(if undoable {
                FormCommitStatus::Applied
            } else {
                FormCommitStatus::Unchanged
            }))
        })
    }

    /// `fill(request, options)`: shown at once, sent when its turn comes;
    /// resolves once the engine answered. A change the engine refuses rejects.
    pub(crate) fn fill(
        &self,
        request: FillRequest,
        signal: Option<AbortSignal>,
    ) -> Promise<FillOutcome> {
        let me = self.rc();
        Promise::spawn(async move {
            let begun = me.begin(request)?;
            let finishing = {
                let (me, begun) = (me.clone(), begun.clone());
                me.clone().enqueue.run(move || Ok(me.finish(begun)), signal.clone())
            };
            let sent = match finishing.await {
                Ok(sent) => sent,
                Err(error) => {
                    if let Some(hold) = &begun.hold {
                        hold.cancel();
                    }
                    return Err(to_form_error(&error));
                }
            };
            // `return ctx.cancellable(...)` from an async function: adopted.
            let answer = me.cancellable(signal, me.answered(begun, sent));
            crate::promise::adopt(answer).await
        })
    }

    /// `fillSoon(request)`: fill the form in without waiting, as a script's
    /// effects need, written while its own transaction still runs. A failure
    /// is reported as the scripts' error.
    pub(crate) fn fill_soon(&self, request: FillRequest) -> Result<(), Error> {
        let origin = request.origin;
        let begun = self.begin(request)?;
        let me = self.rc();
        let run = {
            let (me, begun) = (me.clone(), begun.clone());
            me.clone().enqueue.run(move || Ok(me.finish(begun)), None)
        };
        rt::spawn(async move {
            if let Err(error) = run.reaction().await {
                if let Some(hold) = &begun.hold {
                    hold.cancel();
                }
                let message = to_form_error(&error).message;
                let error = script_error(message);
                if let Err(thrown) = me.surface(
                    Surfaced { ui_effects: &[], diagnostics: &[], error: Some(&error) },
                    origin,
                ) {
                    // Thrown inside the `.catch` handler: an unhandled rejection.
                    crate::console::error(
                        "[form] surfacing a fill failure threw:",
                        thrown.to_canonical(),
                    );
                }
            }
        });
        Ok(())
    }

    /// The fills' wiring: a fill the engine refused leaves the view, and the
    /// fills after it were calculated with its values, so the calculations
    /// run again against what is left; and a download waits for the fills
    /// on their way, and for the engine's answers to them.
    pub(crate) fn wire_fills(&self) -> Result<(), Error> {
        let me = self.weak();
        let changes = self.cx.changes()?;
        let off = changes.on_settled().on(move |settled| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            let SettledChange::Refused { change, .. } = settled else { return Ok(()) };
            if change.undo_of().is_some() {
                return Ok(());
            }
            let key = change.label().get("key").and_then(Value::as_str).unwrap_or("");
            if !matches!(key, "form.fill" | "form.reset") || core.recalculating.get() {
                return Ok(());
            }
            let index = core.view();
            let calculates = index
                .snapshot
                .as_deref()
                .map(|s| crate::value::items(s, "calculationOrder").iter().any(|r| !r.is_null()))
                .unwrap_or(false);
            if core.script_controller().is_none() || !calculates {
                return Ok(());
            }
            core.recalculating.set(true);
            // The scripts' failure is surfaced; a refused recalculation is
            // reported by the queue.
            core.recalculate_after_refusal().ignore();
            Ok(())
        });
        self.cx.listen(off);
        let me = self.weak();
        self.cx.on_settle(Rc::new(move |_signal| {
            let Some(core) = me.upgrade() else { return Ok(None) };
            let changes = core.cx.changes()?;
            let idle = core.enqueue.idle();
            Ok(Some(Promise::spawn(async move {
                idle.await?;
                changes.when_settled().await?;
                Ok(())
            })))
        }))?;
        Ok(())
    }

    /// `recalculate()`: the calculations, on the queue, outside the history.
    fn recalculate_after_refusal(&self) -> Promise<()> {
        let me = self.rc();
        self.enqueue.run(
            move || {
                Ok(Promise::spawn(async move {
                    me.recalculating.set(false);
                    if let Some(pipeline) = me.script_controller() {
                        let inner = me.clone();
                        pipeline
                            .transaction(move |scripts| {
                                Ok(inner.recalculate_now(scripts, ActionOrigin::User))
                            })
                            .await?;
                    }
                    Ok(())
                }))
            },
            None,
        )
    }
}
