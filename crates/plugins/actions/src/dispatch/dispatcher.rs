//! `dispatch/dispatcher.ts`: the one serialized queue every tree rides.
//! `execute` runs a resolved tree; `dispatch` resolves a trigger inside the
//! queued operation (submission order is execution order) and fans out
//! through the lifecycle areas. `run_and_emit` is the shared per-tree unit.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use serde_json::{Value, json};

use super::fold::fold_steps;
use super::run::LocalFuture;
use super::triggers::{plan_page_steps, same_ref};
use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionDispatchEvent,
    ActionDispatchResult, ActionOrigin, ActionPolicyDecision, ActionResultStatus, ActionSource,
    ActionStepResult, ActionTreeSource, ActionTrigger, ActionTriggerEvent, ActionTriggerResult,
    DocumentTriggerEvent, PageEventKind, PartialActionContext, PdfActionTree, event_of,
    trigger_origin_of,
};
use crate::model::ActionsAction;
use crate::services::catalog::doc_event_tree_key;
use crate::services::policy::Decision;
use crate::services::{get, str_of, truthy};

impl ActionsCapability {
    /// run, then the counter, then the event: the shared per-tree unit.
    ///
    /// Boxed: a tree can print, and printing runs the will-print tree.
    pub(crate) fn run_and_emit(
        self,
        tree: Value,
        action_ctx: ActionContext,
    ) -> LocalFuture<Result<ActionDispatchResult, Error>> {
        Box::pin(async move {
            let result = rt::returned(self.clone().run(tree.clone(), action_ctx.clone())).await?;
            self.0.cx.dispatch(ActionsAction::ActionsDispatched);
            self.0.events.action_hook.emit(&ActionDispatchEvent {
                ctx: action_ctx,
                tree,
                result: result.clone(),
            });
            Ok(result)
        })
    }

    /// Execute one tree as one queued dispatch.
    pub fn execute(
        &self,
        tree: PdfActionTree,
        action_ctx: ActionContext,
    ) -> Promise<ActionDispatchResult> {
        // Before enqueueing: a real user gesture arms the open-sequence latch
        // (its op then lands ahead of this one) and resets the cascade count.
        if action_ctx.origin == ActionOrigin::User {
            self.note_user_activity();
        }
        let this = self.clone();
        self.0.queue.run(move || {
            this.0.script_nodes.set(0.0); // one aggregate per dispatch
            Ok(Promise::spawn(this.clone().run_and_emit(tree, action_ctx)))
        })
    }

    /// Would the dispatcher accept this tree's root and attempt it.
    pub fn can_execute(&self, tree: &PdfActionTree, action_ctx: &ActionContext) -> bool {
        if truthy(get(tree, "incomplete")) || !truthy(get(tree, "root")) {
            return false;
        }
        let root = get(tree, "root").cloned().unwrap_or(Value::Null);
        matches!(
            self.decision_for(&root, action_ctx.origin),
            Decision::Policy(ActionPolicyDecision::Allow | ActionPolicyDecision::Adapter)
        )
    }

    async fn run_steps(
        self,
        steps: Vec<(ActionSource, Value)>,
        origin: ActionOrigin,
        event: ActionTriggerEvent,
        diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>>,
    ) -> Result<ActionTriggerResult, Error> {
        let mut results = Vec::new();
        for (source, tree) in steps {
            let ctx = ActionContext { origin, source: source.clone(), event: event.clone() };
            let result = rt::returned(self.clone().run_and_emit(tree.clone(), ctx)).await?;
            results.push(ActionStepResult { source, tree, result });
        }
        Ok(fold_steps(results, diagnostics.borrow().clone()))
    }

    /// Everything a trigger needs, reads included, inside the queued op.
    /// Never fails.
    async fn resolve_and_run(self, trigger: ActionTrigger) -> ActionTriggerResult {
        let diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>> = Rc::default();
        let diagnose = self.collector(&diagnostics);
        let inert = |diagnostics: &Rc<RefCell<Vec<ActionDiagnostic>>>| ActionTriggerResult {
            status: ActionResultStatus::Inert,
            steps: Vec::new(),
            diagnostics: diagnostics.borrow().clone(),
        };
        let attempt: Result<ActionTriggerResult, Error> = async {
            let doc = self.doc()?;
            if !self.trigger_enabled(&trigger) {
                let scope = match &trigger {
                    ActionTrigger::Activate { .. } => "activate",
                    ActionTrigger::Annotation { .. } => "annotation",
                    ActionTrigger::Page { .. } => "page",
                    ActionTrigger::Document { .. } => "document",
                };
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::TriggerDisabled,
                    format!("{scope}: trigger family disabled by config"),
                ));
                return Ok(inert(&diagnostics));
            }
            let origin = trigger_origin_of(&trigger);
            match &trigger {
                ActionTrigger::Activate { annotation_ref, page, source }
                | ActionTrigger::Annotation { annotation_ref, page, source, .. } => {
                    let event = match &trigger {
                        ActionTrigger::Annotation { event, .. } => event.as_str(),
                        _ => "activate",
                    };
                    let pon = page.page_object_number;
                    doc.page_index(pon)?;
                    let listed = doc.call("page.annotations.list", Some(pon), Vec::new()).await?;
                    let annotations = get(&listed, "annotations")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let annotation = annotations.iter().find(|candidate| {
                        same_ref(get(candidate, "ref").unwrap_or(&Value::Null), annotation_ref)
                    });
                    let actions = annotation.and_then(|a| get(a, "actions"));
                    // ISO Table 197: /A takes precedence over /AA U, so a
                    // shadowed U tree is silently inert.
                    if event == "mouseUp" && truthy(actions.and_then(|a| get(a, "activate"))) {
                        return Ok(inert(&diagnostics));
                    }
                    let tree = actions.and_then(|a| get(a, event)).cloned();
                    let runnable = tree
                        .as_ref()
                        .is_some_and(|t| truthy(get(t, "root")) || truthy(get(t, "incomplete")));
                    let Some(tree) = tree.filter(|_| runnable) else {
                        return Ok(inert(&diagnostics));
                    };
                    let source = source.clone().unwrap_or(ActionSource::Annotation {
                        annotation: annotation_ref.clone(),
                        page: *page,
                    });
                    rt::returned(self.clone().run_steps(
                        vec![(source, tree)],
                        origin,
                        event_of(&trigger),
                        diagnostics.clone(),
                    ))
                    .await
                }
                ActionTrigger::Page { event, page } => {
                    let pon = page.page_object_number;
                    let layout = self.layout_of(pon);
                    let lifecycle = rt::returned(self.clone().lifecycle_trees_for(pon)).await?;
                    let steps = plan_page_steps(
                        *event,
                        pon,
                        layout.as_ref().and_then(|l| get(l, "actions")),
                        &lifecycle,
                    );
                    rt::returned(self.clone().run_steps(
                        steps,
                        origin,
                        event_of(&trigger),
                        diagnostics.clone(),
                    ))
                    .await
                }
                ActionTrigger::Document { event } => {
                    if *event != DocumentTriggerEvent::Open {
                        return Ok(rt::returned(self.clone().run_document_event_op(
                            *event,
                            diagnostics.clone(),
                            diagnose.clone(),
                        ))
                        .await);
                    }
                    if !self.claim_open_sequence() {
                        diagnose(ActionDiagnostic::new(
                            ActionDiagnosticCode::OpenSequenceReplayed,
                            "document open sequence already fired for this document",
                        ));
                        return Ok(inert(&diagnostics));
                    }
                    Ok(rt::returned(self.clone().run_open_sequence_op()).await)
                }
            }
        }
        .await;
        match attempt {
            Ok(result) => result,
            Err(error) => {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::TriggerFailed,
                    format!("trigger resolution failed: {}", error.message),
                ));
                ActionTriggerResult {
                    status: ActionResultStatus::Refused,
                    steps: Vec::new(),
                    diagnostics: diagnostics.borrow().clone(),
                }
            }
        }
    }

    /// Report a trigger. The queue slot is taken before this returns, so two
    /// dispatches execute in call order; the promise never rejects.
    pub fn dispatch(&self, trigger: ActionTrigger) -> Promise<ActionTriggerResult> {
        // A user-origin trigger is user activity, noted before taking the
        // slot so an armed open sequence runs first.
        if trigger_origin_of(&trigger) == ActionOrigin::User {
            self.note_user_activity();
        }
        let this = self.clone();
        self.0.queue.run(move || {
            this.0.script_nodes.set(0.0);
            Ok(Promise::spawn(async move { Ok(this.resolve_and_run(trigger).await) }))
        })
    }

    /// Family enabled and a document present. Per-tree truth stays in the
    /// results: resolution is async and never previewed here.
    pub fn can_dispatch(&self, trigger: &ActionTrigger) -> bool {
        self.trigger_enabled(trigger) && self.doc().is_ok()
    }

    /// The cooperative will-close door. Call `documents.close()` after it
    /// resolves.
    pub fn prepare_close(&self) -> Promise<ActionTriggerResult> {
        self.dispatch(ActionTrigger::Document { event: DocumentTriggerEvent::WillClose })
    }

    /// Run one Named verb (`NextPage`, `Print`, ...) as a user-origin api
    /// action without building a tree. `context` overrides the default
    /// `{origin: 'user', source: {kind: 'api'}, event: {scope: 'activate'}}`.
    pub fn execute_named(
        &self,
        name: &str,
        context: Option<PartialActionContext>,
    ) -> Promise<ActionDispatchResult> {
        let context = context.unwrap_or_default();
        self.execute(
            json!({
                "root": { "type": "named", "subtype": "Named", "name": name, "next": [] },
                "incomplete": false,
                "warningFlags": 0,
                "warnings": [],
            }),
            ActionContext {
                origin: context.origin.unwrap_or(ActionOrigin::User),
                source: context.source.unwrap_or(ActionSource::Api),
                event: context.event.unwrap_or(ActionTriggerEvent::Activate),
            },
        )
    }

    /// Read the /A or /AA tree behind a source straight from the document:
    /// `null` when absent. The raw tree: dispatch-time rules such as "/A
    /// shadows /AA U" are not applied.
    pub fn get_action_tree(&self, source: ActionTreeSource) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.doc()?;
            let pick = |holder: Option<&Value>, key: &str| {
                holder.and_then(|h| get(h, "actions")).and_then(|a| get(a, key)).cloned()
            };
            Ok(match source {
                ActionTreeSource::Annotation { annotation, page, event } => {
                    let pon = page.page_object_number;
                    doc.page_index(pon)?;
                    let listed = doc.call("page.annotations.list", Some(pon), Vec::new()).await?;
                    let annotations = get(&listed, "annotations")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let found = annotations.iter().find(|candidate| {
                        same_ref(get(candidate, "ref").unwrap_or(&Value::Null), &annotation)
                    });
                    pick(found, event.map_or("activate", |e| e.as_str()))
                }
                ActionTreeSource::Field { field, event } => {
                    let snapshot = doc.call("forms.list", None, Vec::new()).await?;
                    let fields = get(&snapshot, "fields")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let found = fields.iter().find(|candidate| {
                        if str_of(&field, "kind") == "objectNumber" {
                            let r = get(candidate, "ref").unwrap_or(&Value::Null);
                            str_of(r, "kind") == "objectNumber"
                                && crate::services::same_number(
                                    get(r, "fieldObjectNumber"),
                                    get(&field, "fieldObjectNumber"),
                                )
                        } else {
                            get(candidate, "name") == get(&field, "name")
                        }
                    });
                    pick(found, event.as_str())
                }
                ActionTreeSource::Page { page, event } => {
                    let layout = this.layout_of(page.page_object_number);
                    let key = match event {
                        Some(PageEventKind::Close) => "close",
                        Some(PageEventKind::Visible) => "visible",
                        Some(PageEventKind::Invisible) => "invisible",
                        _ => "open",
                    };
                    pick(layout.as_ref(), key)
                }
                ActionTreeSource::Document { event } => {
                    let snapshot = this.read_document_actions().await?;
                    snapshot.as_ref().and_then(|s| {
                        get(s, doc_event_tree_key(event.unwrap_or(DocumentTriggerEvent::Open)))
                            .cloned()
                    })
                }
            }
            .unwrap_or(Value::Null))
        })
    }
}
