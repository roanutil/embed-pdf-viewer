//! `dispatch/run.ts`: the tree walker. Policy per node, document-lifetime
//! work in walk order (Hide through the owner sinks, executors for the
//! rest), navigation and external effects deferred until every document node
//! succeeded.
//!
//! Every TypeScript `async` function is an `async fn` here, and an `await` of
//! one is `rt::returned(...)`, which spends the turn a JavaScript caller
//! spends resuming.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use epdf_acrojs::js::number_to_string as num;
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use serde_json::{Value, json};

use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionDispatchResult,
    ActionExecutorResult, ActionNodeResult, ActionNodeStatus, ActionPolicyDecision,
    ActionResultStatus, UriOptions,
};
use crate::host_contract::{AnnotCommitEntry, AnnotCommitResultEntry, AnnotCommitStatus};
use crate::services::policy::{DOCUMENT_TYPES, Decision, is_print_verb};
use crate::services::ports::ExecutorKind;
use crate::services::{Diagnose, get, node_type, same_number, str_of, truthy};
use crate::submit::intent::intent_of_payload;

pub(crate) type LocalFuture<T> = Pin<Box<dyn Future<Output = T>>>;

impl ActionsCapability {
    /// A Hide action sets or clears the document Hidden state through the
    /// owning plane's commit sink: widget targets by their field's display,
    /// plain annotations by a flag patch.
    async fn interpret_hide(
        self,
        node: Value,
        diagnose: Diagnose,
    ) -> Result<(ActionNodeStatus, Option<String>), Error> {
        let doc = self.doc();
        let hide = get(&node, "hide").and_then(Value::as_bool).unwrap_or(false);
        let display = if truthy(get(&node, "hide")) { "hidden" } else { "visible" };
        let mut field_refs: Vec<Value> = Vec::new();
        let mut annot_entries: Vec<AnnotCommitEntry> = Vec::new();
        let mut names: Vec<Value> = Vec::new();
        let mut bare_object_numbers: Vec<Value> = Vec::new();
        for target in get(&node, "targets").and_then(Value::as_array).cloned().unwrap_or_default() {
            if str_of(&target, "kind") == "objectNumber" {
                bare_object_numbers
                    .push(get(&target, "objectNumber").cloned().unwrap_or(Value::Null));
            } else {
                names.push(get(&target, "name").cloned().unwrap_or(Value::Null));
            }
        }
        if (!names.is_empty() || !bare_object_numbers.is_empty())
            && let Ok(doc) = doc
        {
            let snapshot = doc.call("forms.list", None, Vec::new()).await?;
            let fields =
                get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
            let field_ref = |field: &Value| {
                json!({
                    "kind": "objectNumber",
                    "fieldObjectNumber": get(field, "fieldObjectNumber").cloned().unwrap_or(Value::Null),
                })
            };
            for name in &names {
                match fields.iter().find(|candidate| get(candidate, "name") == Some(name)) {
                    None => diagnose(ActionDiagnostic::new(
                        ActionDiagnosticCode::UnresolvedTarget,
                        format!("hide: no field named '{}'", epdf_acrojs::js::string_of(name)),
                    )),
                    Some(field) => field_refs.push(field_ref(field)),
                }
            }
            for object_number in &bare_object_numbers {
                // A bare object number may be a widget (its field's display)
                // or a plain annotation (its own flags).
                let owner = fields.iter().find(|field| {
                    get(field, "widgets").and_then(Value::as_array).is_some_and(|widgets| {
                        widgets
                            .iter()
                            .any(|w| same_number(get(w, "annotObjectNumber"), Some(object_number)))
                    })
                });
                match owner {
                    Some(owner) => field_refs.push(field_ref(owner)),
                    None => annot_entries.push(AnnotCommitEntry {
                        annot_object_number: object_number.as_f64().unwrap_or(f64::NAN),
                        page: None,
                        patch: json!({ "flags": { "hidden": hide } }),
                    }),
                }
            }
        }
        let mut failed_detail: Option<String> = None;
        if !field_refs.is_empty() {
            let sink = self.0.ports.form_commit_sink.borrow().clone();
            let Some(sink) = sink else {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    "hide: no form commit sink registered (form plugin absent)",
                ));
                return Ok((ActionNodeStatus::NoExecutor, None));
            };
            let effects = field_refs
                .into_iter()
                .map(|r| json!({ "kind": "setDisplay", "ref": r, "display": display }))
                .collect();
            let result = sink(effects)?.await?;
            if let Some(bad) =
                result.results.iter().find(|e| e.status == "failed" || e.status == "rejected")
            {
                let detail = bad
                    .error
                    .as_ref()
                    .and_then(|e| e.message.clone())
                    .unwrap_or_else(|| bad.status.clone());
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!("hide: {detail}"),
                ));
                failed_detail = Some(detail);
            }
        }
        let failed_so_far = failed_detail.as_deref().is_some_and(|d| !d.is_empty());
        if !annot_entries.is_empty() && !failed_so_far {
            let sink = self.0.ports.annot_commit_sink.borrow().clone();
            let Some(sink) = sink else {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoCommitSink,
                    "hide: no annotation commit sink registered (annotation plugin absent)",
                ));
                return Ok((ActionNodeStatus::NoExecutor, None));
            };
            let committed = sink(annot_entries)?.await?;
            let failed: Vec<&AnnotCommitResultEntry> = committed
                .results
                .iter()
                .filter(|e| e.status == AnnotCommitStatus::Failed)
                .collect();
            for failure in &failed {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!(
                        "hide: annotation {}: {}",
                        num(failure.annot_object_number),
                        failure.error.clone().unwrap_or_else(|| "failed".into())
                    ),
                ));
            }
            if failed.len() == committed.results.len() && !failed.is_empty() {
                failed_detail = failed[0].error.clone();
            }
        }
        match failed_detail.filter(|d| !d.is_empty()) {
            Some(detail) => Ok((ActionNodeStatus::Failed, Some(detail))),
            None => Ok((ActionNodeStatus::Executed, None)),
        }
    }

    /// `executor(node, ctx)` as the promise `await` sees; `Err` is a
    /// synchronous throw.
    fn invoke_executor(
        &self,
        executor: &ExecutorKind,
        node: &Value,
        ctx: &ActionContext,
    ) -> Result<Promise<ActionExecutorResult>, Error> {
        match executor {
            ExecutorKind::Registered(run) => run(node, ctx),
            ExecutorKind::Script => {
                Ok(Promise::spawn(self.clone().script_executor(node.clone(), ctx.clone())))
            }
        }
    }

    /// `run(tree, ctx)`.
    pub(crate) async fn run(
        self,
        tree: Value,
        action_ctx: ActionContext,
    ) -> Result<ActionDispatchResult, Error> {
        let diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>> = Rc::default();
        let diagnose = self.collector(&diagnostics);

        // The law: never execute an incomplete tree, not even its root.
        if truthy(get(&tree, "incomplete")) {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::IncompleteTree,
                "refused: the action tree is incomplete",
            ));
            let diagnostics = diagnostics.borrow().clone();
            return Ok(ActionDispatchResult {
                status: ActionResultStatus::Refused,
                nodes: Vec::new(),
                diagnostics,
            });
        }
        let Some(root) = get(&tree, "root").filter(|r| truthy(Some(r))).cloned() else {
            let diagnostics = diagnostics.borrow().clone();
            return Ok(ActionDispatchResult {
                status: ActionResultStatus::Inert,
                nodes: Vec::new(),
                diagnostics,
            });
        };

        let walk = Rc::new(Walk {
            this: self.clone(),
            ctx: action_ctx,
            diagnose,
            nodes: RefCell::new(Vec::new()),
            deferred: RefCell::new(Vec::new()),
            document_failed: Cell::new(false),
        });
        rt::returned(walk.clone().walk(root, Vec::new())).await?;
        if !walk.document_failed.get() {
            let deferred = std::mem::take(&mut *walk.deferred.borrow_mut());
            for entry in deferred {
                rt::returned(walk.clone().fire(entry)).await?;
            }
        }
        let nodes = walk.nodes.borrow().clone();
        let any_executed = nodes.iter().any(|n| n.status == ActionNodeStatus::Executed);
        let any_failed_or_skipped = nodes
            .iter()
            .any(|n| matches!(n.status, ActionNodeStatus::Failed | ActionNodeStatus::Skipped));
        let status = if any_executed {
            if any_failed_or_skipped {
                ActionResultStatus::Partial
            } else {
                ActionResultStatus::Executed
            }
        } else if any_failed_or_skipped {
            ActionResultStatus::Partial
        } else {
            ActionResultStatus::Inert
        };
        let diagnostics = diagnostics.borrow().clone();
        Ok(ActionDispatchResult { status, nodes, diagnostics })
    }
}

enum Deferred {
    Submit(Value),
    External(Value),
    Navigation(Value),
}

/// One `run`'s walk state: the node results, the deferred thunks, and the
/// document-failure latch.
struct Walk {
    this: ActionsCapability,
    ctx: ActionContext,
    diagnose: Diagnose,
    nodes: RefCell<Vec<ActionNodeResult>>,
    deferred: RefCell<Vec<(usize, Deferred)>>,
    document_failed: Cell<bool>,
}

impl Walk {
    fn set(&self, index: usize, status: ActionNodeStatus, detail: Option<String>) {
        let mut nodes = self.nodes.borrow_mut();
        nodes[index].status = status;
        if let Some(detail) = detail.filter(|d| !d.is_empty()) {
            nodes[index].detail = Some(detail);
        }
    }

    fn settle(&self, index: usize, outcome: ActionExecutorResult) {
        let node_type = self.nodes.borrow()[index].node_type.clone();
        match outcome {
            ActionExecutorResult::Executed => {
                self.nodes.borrow_mut()[index].status = ActionNodeStatus::Executed
            }
            ActionExecutorResult::Inert { reason } => {
                {
                    let mut nodes = self.nodes.borrow_mut();
                    nodes[index].status = ActionNodeStatus::Inert;
                    nodes[index].detail = Some(reason.clone());
                }
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorInert,
                    format!("{node_type}: {reason}"),
                ));
            }
            ActionExecutorResult::Failed { error } => {
                {
                    let mut nodes = self.nodes.borrow_mut();
                    nodes[index].status = ActionNodeStatus::Failed;
                    nodes[index].detail = Some(error.clone());
                }
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!("{node_type}: {error}"),
                ));
            }
        }
    }

    /// `try { settle(result, await executor(node, ctx)) } catch ...`.
    async fn execute_with(&self, index: usize, executor: Rc<ExecutorKind>, node: &Value) {
        let outcome = match self.this.invoke_executor(&executor, node, &self.ctx) {
            Ok(promise) => promise.await,
            Err(error) => Err(error),
        };
        self.settle(
            index,
            outcome.unwrap_or_else(|error| ActionExecutorResult::Failed { error: error.message }),
        );
    }

    fn status(&self, index: usize) -> ActionNodeStatus {
        self.nodes.borrow()[index].status
    }

    async fn interpret(self: Rc<Self>, node: Value, path: Vec<usize>) -> Result<(), Error> {
        let kind = node_type(&node);
        let index = {
            let mut nodes = self.nodes.borrow_mut();
            nodes.push(ActionNodeResult {
                path,
                node_type: kind.clone(),
                status: ActionNodeStatus::Blocked,
                detail: None,
            });
            nodes.len() - 1
        };
        let origin = self.ctx.origin;
        match self.this.decision_for(&node, origin) {
            Decision::Never => {
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::Blocked,
                    format!("{kind}: never executable"),
                ));
                return Ok(());
            }
            Decision::Policy(
                decision @ (ActionPolicyDecision::Block | ActionPolicyDecision::Report),
            ) => {
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::Blocked,
                    format!(
                        "{kind}: policy '{}' for origin '{}'",
                        decision.as_str(),
                        origin.as_str()
                    ),
                ));
                return Ok(());
            }
            Decision::Unsupported => {
                self.set(index, ActionNodeStatus::NoExecutor, None);
                return Ok(());
            }
            Decision::Policy(_) => {}
        }

        if kind == "hide" {
            // A document mutation now: an earlier failure skips it, and its
            // failure stops later document work.
            if self.document_failed.get() {
                self.set(index, ActionNodeStatus::Skipped, None);
                return Ok(());
            }
            let (status, detail) =
                rt::returned(self.this.clone().interpret_hide(node, self.diagnose.clone())).await?;
            self.set(index, status, detail);
            if status == ActionNodeStatus::Failed {
                self.document_failed.set(true);
            }
            return Ok(());
        }

        if DOCUMENT_TYPES.contains(&kind.as_str()) {
            if self.document_failed.get() {
                self.set(index, ActionNodeStatus::Skipped, None);
                return Ok(());
            }
            let Some(executor) = self.this.executor(&kind) else {
                let javascript = kind == "javascript";
                self.set(
                    index,
                    if javascript { ActionNodeStatus::Inert } else { ActionNodeStatus::NoExecutor },
                    None,
                );
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::NoExecutor,
                    format!(
                        "{kind}: no executor registered{}",
                        if javascript { " (scripting unavailable)" } else { "" }
                    ),
                ));
                return Ok(());
            };
            self.execute_with(index, executor, &node).await;
            if self.status(index) == ActionNodeStatus::Failed {
                self.document_failed.set(true);
            }
            return Ok(());
        }

        if kind == "submit-form" {
            let Some(payload) = get(&node, "payload").filter(|p| truthy(Some(p))).cloned() else {
                // Older-runtime extraction: recognized-inert, honestly diagnosed.
                self.set(
                    index,
                    ActionNodeStatus::Inert,
                    Some("submit payload unavailable (older runtime extraction)".into()),
                );
                (self.diagnose)(ActionDiagnostic::new(
                    ActionDiagnosticCode::SubmitPayloadUnavailable,
                    "submit-form: payload unavailable (older runtime extraction) — node inert",
                ));
                return Ok(());
            };
            // External: deferred until every document node succeeded.
            self.set(index, ActionNodeStatus::Skipped, None);
            self.deferred.borrow_mut().push((index, Deferred::Submit(payload)));
            return Ok(());
        }

        if is_print_verb(&node) || kind == "uri" {
            self.set(index, ActionNodeStatus::Skipped, None);
            self.deferred.borrow_mut().push((index, Deferred::External(node)));
            return Ok(());
        }

        // goto and the named page verbs: navigation, deferred.
        self.set(index, ActionNodeStatus::Skipped, None);
        self.deferred.borrow_mut().push((index, Deferred::Navigation(node)));
        Ok(())
    }

    fn walk(self: Rc<Self>, node: Value, path: Vec<usize>) -> LocalFuture<Result<(), Error>> {
        Box::pin(async move {
            rt::returned(self.clone().interpret(node.clone(), path.clone())).await?;
            let children =
                get(&node, "next").and_then(Value::as_array).cloned().unwrap_or_default();
            for (index, child) in children.into_iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(index);
                rt::returned(self.clone().walk(child, child_path)).await?;
            }
            Ok(())
        })
    }

    async fn fire(self: Rc<Self>, (index, entry): (usize, Deferred)) -> Result<(), Error> {
        match entry {
            Deferred::Submit(payload) => {
                let intent = intent_of_payload(&payload);
                let (status, detail) = rt::returned(self.this.clone().perform_submit(
                    intent,
                    self.ctx.clone(),
                    self.diagnose.clone(),
                ))
                .await;
                self.set(index, status, detail);
            }
            Deferred::External(node) if node_type(&node) == "uri" => {
                let adapter = self.this.0.ports.ui_adapter.borrow().clone();
                let Some(adapter) = adapter else {
                    self.set(index, ActionNodeStatus::NoExecutor, None);
                    (self.diagnose)(ActionDiagnostic::new(
                        ActionDiagnosticCode::NoAdapter,
                        "uri: no UI adapter installed",
                    ));
                    return Ok(());
                };
                (adapter.open_uri)(
                    str_of(&node, "uri"),
                    &UriOptions {
                        is_map: get(&node, "isMap").and_then(Value::as_bool).unwrap_or(false),
                        origin: self.ctx.origin,
                    },
                )?;
                self.set(index, ActionNodeStatus::Executed, None);
            }
            Deferred::External(_) => {
                // The Print verb: will-print, the adapter once, did-print.
                let (status, detail) = rt::returned(
                    self.this.clone().fire_print_through_adapter(None, self.diagnose.clone()),
                )
                .await?;
                self.set(index, status, detail);
            }
            Deferred::Navigation(node) => {
                let kind = node_type(&node);
                let Some(executor) = self.this.executor(&kind) else {
                    self.set(index, ActionNodeStatus::NoExecutor, None);
                    (self.diagnose)(ActionDiagnostic::new(
                        ActionDiagnosticCode::NoExecutor,
                        format!("{kind}: no executor registered"),
                    ));
                    return Ok(());
                };
                self.execute_with(index, executor, &node).await;
            }
        }
        Ok(())
    }
}
