//! `dispatch/dispatcher.ts`: the one serialized queue every tree rides.
//! `execute` runs a resolved tree; `dispatch` resolves a trigger inside the
//! queued operation (submission order is execution order) and fans out
//! through the lifecycle areas. `run_and_emit` is the shared per-tree unit.
//!
//! A caller's signal skips an operation that hasn't started, stops a
//! trigger's reads, and stops a tree before its next node. `execute` then
//! rejects `operation-cancelled`; `dispatch` never rejects, so it resolves
//! `cancelled`.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, is_undefined};
use serde_json::{Value, json};

use super::fold::fold_steps;
use super::run::LocalFuture;
use super::triggers::plan_page_steps;
use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionDispatchResult,
    ActionExecutedEvent, ActionOrigin, ActionPolicyDecision, ActionResultStatus, ActionSource,
    ActionStepResult, ActionTreeSource, ActionTrigger, ActionTriggerEvent, ActionTriggerResult,
    DocumentTriggerEvent, PageEventKind, PartialActionContext, PdfActionTree, event_of,
    trigger_origin_of,
};
use crate::services::catalog::doc_event_tree_key;
use crate::services::policy::Decision;
use crate::services::{DiagnosticPlace, get, str_of, truthy};

/// `sourceOfTrigger(trigger)`: the source a trigger reports for, what its
/// diagnostics carry before a tree is found.
fn source_of_trigger(trigger: &ActionTrigger) -> ActionSource {
    match trigger {
        ActionTrigger::Activate { annotation_ref, page, source }
        | ActionTrigger::Annotation { annotation_ref, page, source, .. } => {
            source.clone().unwrap_or(ActionSource::Annotation {
                annotation: annotation_ref.clone(),
                page: *page,
            })
        }
        ActionTrigger::Page { page, .. } => ActionSource::Page { page: *page },
        ActionTrigger::Document { .. } => ActionSource::Document,
    }
}

/// `new PluginError('operation-cancelled', 'actions', ...)`.
pub(crate) fn cancelled(operation: &str, signal: Option<&AbortSignal>) -> Error {
    plugin_error(
        code::OPERATION_CANCELLED,
        "actions",
        format!("{operation} was cancelled"),
        signal.and_then(AbortSignal::reason).map(|reason| reason.to_canonical()),
        None,
    )
}

fn is_cancellation(error: &Error) -> bool {
    error.is_plugin_error(Some(code::OPERATION_CANCELLED))
}

/// `namedTree(name)`: the tree `executeNamed` runs.
fn named_tree(name: &str) -> PdfActionTree {
    json!({
        "root": { "type": "named", "subtype": "Named", "name": name, "next": [] },
        "incomplete": false,
        "warningFlags": 0,
        "warnings": [],
    })
}

/// `namedContext(context)`: a click from your code unless `context` says
/// otherwise.
fn named_context(context: Option<PartialActionContext>) -> ActionContext {
    let context = context.unwrap_or_default();
    ActionContext {
        origin: context.origin.unwrap_or(ActionOrigin::User),
        source: context.source.unwrap_or(ActionSource::Api),
        event: context.event.unwrap_or(ActionTriggerEvent::Activate),
    }
}

impl ActionsCapability {
    /// The serial queue with `{signal}`: an operation whose signal fired
    /// while it waited never starts.
    pub(crate) fn enqueue<T: Clone + 'static>(
        &self,
        operation: impl FnOnce() -> Result<Promise<T>, Error> + 'static,
        signal: Option<AbortSignal>,
    ) -> Promise<T> {
        self.0.queue.run(move || match &signal {
            Some(signal) if signal.aborted() => Ok(Promise::rejected(plugin_error(
                code::OPERATION_CANCELLED,
                "actions",
                "cancelled before it started",
                signal.reason().map(|reason| reason.to_canonical()),
                None,
            ))),
            _ => operation(),
        })
    }

    /// Run one tree and announce it: the shared per-tree unit.
    ///
    /// Boxed: a tree can print, and printing runs the will-print tree.
    pub(crate) fn run_and_emit(
        self,
        tree: Value,
        action_ctx: ActionContext,
        signal: Option<AbortSignal>,
    ) -> LocalFuture<Result<ActionDispatchResult, Error>> {
        Box::pin(async move {
            let result =
                rt::returned(self.clone().run(tree.clone(), action_ctx.clone(), signal)).await?;
            self.0.events.executed.emit(&ActionExecutedEvent {
                tree,
                result: result.clone(),
                source: action_ctx.source,
            });
            Ok(result)
        })
    }

    /// Run a resolved tree on the dispatch queue and fire `on_executed`; a
    /// failing node is reported in the result, not thrown. Rejects
    /// `operation-cancelled` when `signal` fires: before the tree starts,
    /// nothing runs; while it runs, it stops before its next node.
    pub fn execute(
        &self,
        tree: PdfActionTree,
        action_ctx: ActionContext,
        signal: Option<AbortSignal>,
    ) -> Promise<ActionDispatchResult> {
        // Before enqueueing: a real user gesture arms the open sequence (its
        // operation then lands ahead of this action) and resets the cascade
        // budget.
        if action_ctx.origin == ActionOrigin::User {
            self.note_user_activity();
        }
        let this = self.clone();
        let queued = signal.clone();
        self.enqueue(
            move || {
                this.0.script_nodes.set(0.0); // one script budget per dispatch
                Ok(Promise::spawn(async move {
                    let result =
                        rt::returned(this.clone().run_and_emit(tree, action_ctx, signal.clone()))
                            .await?;
                    if result.status == ActionResultStatus::Cancelled {
                        return Err(cancelled("actions.execute", signal.as_ref()));
                    }
                    Ok(result)
                }))
            },
            queued,
        )
    }

    /// The tree is complete and its root node's policy decision is `allow`
    /// or `adapter`.
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
        signal: Option<AbortSignal>,
    ) -> Result<ActionTriggerResult, Error> {
        let aborted = || signal.as_ref().is_some_and(AbortSignal::aborted);
        let mut results = Vec::new();
        for (source, tree) in steps {
            if aborted() {
                break;
            }
            let ctx = ActionContext { origin, source: source.clone(), event: event.clone() };
            let result =
                rt::returned(self.clone().run_and_emit(tree.clone(), ctx, signal.clone())).await?;
            results.push(ActionStepResult { source, tree, result });
        }
        if aborted() {
            return Ok(ActionTriggerResult {
                status: ActionResultStatus::Cancelled,
                steps: results,
                diagnostics: diagnostics.borrow().clone(),
            });
        }
        Ok(fold_steps(results, diagnostics.borrow().clone()))
    }

    /// Everything a trigger needs, reads included, inside the queued
    /// operation: submission order is execution order. Never fails.
    async fn resolve_and_run(
        self,
        trigger: ActionTrigger,
        signal: Option<AbortSignal>,
    ) -> ActionTriggerResult {
        let diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>> = Rc::default();
        let diagnose =
            self.collector(&diagnostics, DiagnosticPlace::source(source_of_trigger(&trigger)));
        let inert = |diagnostics: &Rc<RefCell<Vec<ActionDiagnostic>>>| ActionTriggerResult {
            status: ActionResultStatus::Inert,
            steps: Vec::new(),
            diagnostics: diagnostics.borrow().clone(),
        };
        let attempt: Result<ActionTriggerResult, Error> = async {
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
                ActionTrigger::Activate { annotation_ref, .. }
                | ActionTrigger::Annotation { annotation_ref, .. } => {
                    let event = match &trigger {
                        ActionTrigger::Annotation { event, .. } => event.as_str(),
                        _ => "activate",
                    };
                    let annotation = rt::returned(
                        self.clone().read_annotation(annotation_ref.clone(), signal.clone()),
                    )
                    .await?;
                    let actions = annotation.as_ref().and_then(|a| get(a, "actions"));
                    // ISO 32000-2 Table 197: "the A entry, if present, takes
                    // precedence over [the /AA U entry]"; a shadowed U tree is
                    // silently inert, exactly like an absent one.
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
                    let source = source_of_trigger(&trigger);
                    rt::returned(self.clone().run_steps(
                        vec![(source, tree)],
                        origin,
                        event_of(&trigger),
                        diagnostics.clone(),
                        signal.clone(),
                    ))
                    .await
                }
                ActionTrigger::Page { event, page } => {
                    // A shared read other triggers may wait on too: never
                    // aborted, only not awaited further.
                    let lifecycle = self.lifecycle_annotations_of(*page).await?;
                    let layout = self.layout_of(page.page_object_number);
                    let steps = plan_page_steps(
                        *event,
                        *page,
                        layout.as_ref().and_then(|l| get(l, "actions")),
                        &lifecycle,
                    );
                    rt::returned(self.clone().run_steps(
                        steps,
                        origin,
                        event_of(&trigger),
                        diagnostics.clone(),
                        signal.clone(),
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
            Err(error)
                if is_cancellation(&error) && signal.as_ref().is_some_and(AbortSignal::aborted) =>
            {
                ActionTriggerResult {
                    status: ActionResultStatus::Cancelled,
                    steps: Vec::new(),
                    diagnostics: diagnostics.borrow().clone(),
                }
            }
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

    /// Report a trigger. The queue slot is taken before this returns, so
    /// two dispatches execute in call order; all reads happen inside the
    /// queued operation. Never rejects: resolution failures come back as
    /// `refused` with a `trigger-failed` diagnostic, and a fired `signal` as
    /// `cancelled`.
    pub fn dispatch(
        &self,
        trigger: ActionTrigger,
        signal: Option<AbortSignal>,
    ) -> Promise<ActionTriggerResult> {
        // A user-origin trigger is user activity, noted before taking the
        // slot so an armed open sequence runs first.
        if trigger_origin_of(&trigger) == ActionOrigin::User {
            self.note_user_activity();
        }
        let this = self.clone();
        let queued = signal.clone();
        let result = self.enqueue(
            move || {
                this.0.script_nodes.set(0.0); // one script budget per dispatch
                Ok(Promise::spawn(async move { Ok(this.resolve_and_run(trigger, signal).await) }))
            },
            queued,
        );
        // `.catch(...)`: only a signal that fired while the trigger waited
        // for its turn gets here; `resolve_and_run` never fails.
        Promise::spawn(async move {
            Ok(result.reaction().await.unwrap_or_else(|_| ActionTriggerResult {
                status: ActionResultStatus::Cancelled,
                steps: Vec::new(),
                diagnostics: Vec::new(),
            }))
        })
    }

    /// The trigger's family is enabled (the `triggers` setting). Per-tree
    /// truth stays in the results: resolution is async and never previewed.
    pub fn can_dispatch(&self, trigger: &ActionTrigger) -> bool {
        self.trigger_enabled(trigger)
    }

    /// The cooperative /WC door: runs the catalog will-close tree (after the
    /// document-open sequence) and resolves when its effects are committed.
    /// Call `documents.close()` after it resolves. Never rejects.
    pub fn prepare_close(&self, signal: Option<AbortSignal>) -> Promise<ActionTriggerResult> {
        self.dispatch(ActionTrigger::Document { event: DocumentTriggerEvent::WillClose }, signal)
    }

    /// Run one Named verb (`NextPage`, `Print`, ...) as a user-origin action
    /// without building a tree. `context` overrides the default
    /// `{origin: 'user', source: {kind: 'api'}, event: {scope: 'activate'}}`.
    pub fn execute_named(
        &self,
        name: &str,
        context: Option<PartialActionContext>,
        signal: Option<AbortSignal>,
    ) -> Promise<ActionDispatchResult> {
        self.execute(named_tree(name), named_context(context), signal)
    }

    /// Whether the rules would run this Named verb from `context`; `Print`
    /// also needs `doc.print`.
    pub fn can_execute_named(&self, name: &str, context: Option<PartialActionContext>) -> bool {
        self.can_execute(&named_tree(name), &named_context(context))
            && (name != "Print" || self.allows_print())
    }

    /// The raw tree behind a source, read from the document (`null` when
    /// absent), with no dispatch rules applied.
    async fn read_action_tree(self, source: ActionTreeSource) -> Result<Value, Error> {
        Ok(match source {
            ActionTreeSource::Annotation { annotation, event, .. } => {
                let found = rt::returned(self.clone().read_annotation(annotation, None)).await?;
                let key = event.map_or("activate", |e| e.as_str());
                found
                    .as_ref()
                    .and_then(|a| get(a, "actions"))
                    .and_then(|actions| get(actions, key))
                    .cloned()
            }
            ActionTreeSource::Field { field, event } => {
                let snapshot = self.read_form(None).await?;
                let fields =
                    get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
                let by_number = str_of(&field, "kind") == "objectNumber";
                let found = fields.iter().find(|candidate| {
                    if by_number {
                        let r = get(candidate, "ref").unwrap_or(&Value::Null);
                        str_of(r, "kind") == "objectNumber"
                            && crate::services::same_number(
                                get(r, "objectNumber"),
                                get(&field, "objectNumber"),
                            )
                    } else {
                        get(candidate, "name") == get(&field, "name")
                    }
                });
                found
                    .and_then(|f| get(f, "actions"))
                    .and_then(|actions| get(actions, event.as_str()))
                    .cloned()
            }
            ActionTreeSource::Page { page, event } => {
                let key = match event {
                    Some(PageEventKind::Close) => "close",
                    Some(PageEventKind::Visible) => "visible",
                    Some(PageEventKind::Invisible) => "invisible",
                    _ => "open",
                };
                self.layout_of(page.page_object_number)
                    .as_ref()
                    .and_then(|layout| get(layout, "actions"))
                    .and_then(|actions| get(actions, key))
                    .cloned()
            }
            ActionTreeSource::Outline { item } => {
                let outline = self.doc()?.call("outline.get", None, Vec::new()).await?;
                get(&outline, "items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|candidate| {
                        crate::services::same_number(
                            get(candidate, "ref").and_then(|r| get(r, "objectNumber")),
                            get(&item, "objectNumber"),
                        )
                    })
                    .and_then(|candidate| get(candidate, "action"))
                    .cloned()
            }
            ActionTreeSource::Document { event } => {
                let snapshot = self.read_document_actions().await?;
                snapshot.as_ref().and_then(|s| {
                    get(s, doc_event_tree_key(event.unwrap_or(DocumentTriggerEvent::Open))).cloned()
                })
            }
        }
        // `?? null`.
        .filter(|tree| !tree.is_null() && !is_undefined(tree))
        .unwrap_or(Value::Null))
    }

    /// Read the /A or /AA tree behind a source straight from the document
    /// (`null` when absent). This is the raw tree: dispatch-time rules such
    /// as ISO 32000-2 Table 197's "/A shadows /AA U" are not applied.
    /// Rejects `operation-cancelled` when `signal` fires.
    pub fn get_action_tree(
        &self,
        source: ActionTreeSource,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let read = Promise::spawn(self.clone().read_action_tree(source));
        self.read(signal.as_ref(), read)
    }
}
