//! `scripting/executor.ts`: the real `javascript` executor. One node is one
//! host transaction, with prefetch, boot, run, commit and surface inside the
//! boundary. Print and submit effects are external: they are replayed after
//! the transaction releases, still inside the queued dispatch.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_acrojs::js::number_to_string as num;
use epdf_acrojs::{
    ScriptEventInput, ScriptEventKind, ScriptOutput, ScriptTransaction, ScriptUiEffect,
    ScriptWorldInput,
};
use epdf_kernel::Error;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Promise};
use serde_json::Value;

use super::world::script_event_for;
use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionExecutorResult, ActionSource,
    ActionUiContext, ScriptPhase,
};
use crate::host_contract::{
    AnnotCommitEntry, AnnotCommitStatus, ScriptRealmKind, ScriptSurfaceResult,
};
use crate::services::ports::ExecutorKind;
use crate::services::{Diagnose, get, str_of};
use crate::submit::intent::intent_of_submit_effect;

impl ActionsCapability {
    /// `registerScriptExecutor(...)`: with a realm, `javascript` is this
    /// executor.
    pub(crate) fn register_script_executor(&self) {
        if self.0.script_host.is_some() {
            self.install_executor("javascript", ExecutorKind::Script);
        }
    }

    /// Commit one run's document effects through the owner sinks in the
    /// declared order (form first, then annotations); the first failure
    /// skips the rest across both streams. The failure summary, if any.
    async fn commit_script_output(
        self,
        output: ScriptOutput,
        diagnose: Diagnose,
    ) -> Result<Option<String>, Error> {
        let mut failure: Option<String> = None;
        if !output.form_effects.is_empty() {
            let sink = self.0.ports.form_commit_sink.borrow().clone();
            match sink {
                None => diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    format!(
                        "script form effects dropped: no form commit sink registered ({})",
                        output.form_effects.len()
                    ),
                )),
                Some(sink) => {
                    let result = sink(output.form_effects.clone())?.await?;
                    if let Some(bad) = result
                        .results
                        .iter()
                        .find(|e| e.status == "failed" || e.status == "rejected")
                    {
                        let why = bad
                            .error
                            .as_ref()
                            .and_then(|e| e.message.clone())
                            .unwrap_or_else(|| bad.status.clone());
                        failure = Some(format!("form effect {}: {why}", num(bad.index)));
                    }
                }
            }
        }
        if !output.annot_effects.is_empty() {
            let sink = self.0.ports.annot_commit_sink.borrow().clone();
            if failure.is_some() {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!(
                        "script annot effects skipped after form failure ({})",
                        output.annot_effects.len()
                    ),
                ));
            } else if let Some(sink) = sink {
                let entries = output
                    .annot_effects
                    .iter()
                    .map(|effect| {
                        let by_number = str_of(&effect.annot_ref, "kind") == "objectNumber";
                        AnnotCommitEntry {
                            annot_object_number: if by_number {
                                get(&effect.annot_ref, "annotObjectNumber")
                                    .and_then(Value::as_f64)
                                    .unwrap_or(f64::NAN)
                            } else {
                                -1.0
                            },
                            page: if by_number {
                                get(&effect.annot_ref, "page").and_then(PageRef::from_json)
                            } else {
                                None
                            },
                            patch: effect.patch.clone(),
                        }
                    })
                    .collect();
                let result = sink(entries)?.await?;
                if let Some(bad) =
                    result.results.iter().find(|e| e.status == AnnotCommitStatus::Failed)
                {
                    failure = Some(format!(
                        "annotation {}: {}",
                        num(bad.annot_object_number),
                        bad.error.clone().unwrap_or_else(|| "failed".into())
                    ));
                }
            } else {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    format!(
                        "script annot effects dropped: no annotation commit sink registered ({})",
                        output.annot_effects.len()
                    ),
                ));
            }
        }
        if let Some(failure) = &failure {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::ExecutorFailed,
                format!("script commit: {failure}"),
            ));
        }
        Ok(failure)
    }

    /// The executor body for one `javascript` node.
    pub(crate) async fn script_executor(
        self,
        node: Value,
        action_ctx: ActionContext,
    ) -> Result<ActionExecutorResult, Error> {
        if str_of(&node, "type") != "javascript" {
            return Ok(ActionExecutorResult::Inert { reason: "not a JS node".into() });
        }
        let Some(host) = self.0.script_host.clone() else {
            return Ok(ActionExecutorResult::Inert { reason: "no document".into() });
        };
        let node_cap = self
            .0
            .config
            .javascript
            .as_ref()
            .and_then(|js| js.max_script_nodes_per_dispatch)
            .unwrap_or(16.0);
        if self.0.script_nodes.get() >= node_cap {
            return Ok(ActionExecutorResult::Inert {
                reason: format!("dispatch script budget exhausted ({} JS nodes)", num(node_cap)),
            });
        }
        self.0.script_nodes.set(self.0.script_nodes.get() + 1.0);
        let pon = match &action_ctx.source {
            ActionSource::Widget { page, .. }
            | ActionSource::Page { page }
            | ActionSource::Annotation { page, .. } => Some(page.page_object_number),
            ActionSource::Link { page, .. } => {
                page.map(|p| p.page_object_number).or_else(|| self.first_pon())
            }
            _ => self.first_pon(),
        };
        let Some(pon) = pon else {
            return Ok(ActionExecutorResult::Inert {
                reason: "no page to anchor the world on".into(),
            });
        };
        let diagnose = self.emitter();
        // Print and submit effects are external: they run after the
        // transaction releases, still inside this queued dispatch.
        let pending_external: Rc<RefCell<Vec<(ScriptUiEffect, ScriptPhase)>>> = Rc::default();
        let split_external = {
            let pending = pending_external.clone();
            move |output: &ScriptOutput, phase: ScriptPhase| -> Vec<ScriptUiEffect> {
                output
                    .ui_effects
                    .iter()
                    .filter(|effect| {
                        if matches!(
                            effect,
                            ScriptUiEffect::Print | ScriptUiEffect::SubmitForm { .. }
                        ) {
                            pending.borrow_mut().push(((*effect).clone(), phase));
                            false
                        } else {
                            true
                        }
                    })
                    .cloned()
                    .collect()
            }
        };
        let script = str_of(&node, "script").to_string();
        let (this, ctx2, diagnose2) = (self.clone(), action_ctx.clone(), diagnose.clone());
        let outcome = host
            .transaction(move |txn: ScriptTransaction| {
                Ok(Promise::spawn(async move {
                    let this = this;
                    let mut built = rt::returned(this.clone().script_world_for(pon)).await?;
                    let mut boot_event = ScriptEventInput::of(ScriptEventKind::NameTreeBoot);
                    boot_event.event_type = Some("Doc".into());
                    boot_event.name = Some("Open".into());
                    let boot = txn
                        .boot(ScriptWorldInput { event: boot_event, ..built.1.clone() }, None)
                        .await?;
                    if let Some(boot) = boot {
                        // Boot effects belong to this first transaction; a boot
                        // fault only degrades. Refetch so the run sees them.
                        rt::returned(
                            this.clone().commit_script_output(boot.clone(), diagnose2.clone()),
                        )
                        .await?;
                        this.surface_script_result(&ScriptSurfaceResult {
                            ui_effects: split_external(&boot, ScriptPhase::Boot),
                            diagnostics: boot.diagnostics.clone(),
                            error: boot.error.clone(),
                            origin: ctx2.origin,
                            phase: ScriptPhase::Boot,
                            realm: ScriptRealmKind::Document,
                        })?;
                        if !boot.form_effects.is_empty() || !boot.annot_effects.is_empty() {
                            built = rt::returned(this.clone().script_world_for(pon)).await?;
                        }
                    }
                    let event = script_event_for(&ctx2, &built.0);
                    let output = txn
                        .run(&script, ScriptWorldInput { event, ..built.1.clone() }, None)
                        .await?;
                    this.surface_script_result(&ScriptSurfaceResult {
                        ui_effects: split_external(&output, ScriptPhase::User),
                        diagnostics: output.diagnostics.clone(),
                        error: output.error.clone(),
                        origin: ctx2.origin,
                        phase: ScriptPhase::User,
                        realm: ScriptRealmKind::Document,
                    })?;
                    if let Some(error) = &output.error {
                        return Ok(ActionExecutorResult::Failed { error: error.message.clone() });
                    }
                    let failure =
                        rt::returned(this.clone().commit_script_output(output, diagnose2)).await?;
                    Ok(match failure {
                        Some(error) => ActionExecutorResult::Failed { error },
                        None => ActionExecutorResult::Executed,
                    })
                }))
            })
            .await?;
        let external = std::mem::take(&mut *pending_external.borrow_mut());
        for (effect, phase) in external {
            match intent_of_submit_effect(&effect) {
                None => {
                    let ui = ActionUiContext { origin: action_ctx.origin, phase };
                    rt::returned(
                        self.clone().fire_print_through_adapter(Some(ui), diagnose.clone()),
                    )
                    .await?;
                }
                Some(intent) => {
                    rt::returned(self.clone().perform_submit(
                        intent,
                        action_ctx.clone(),
                        diagnose.clone(),
                    ))
                    .await;
                }
            }
        }
        Ok(outcome)
    }
}
