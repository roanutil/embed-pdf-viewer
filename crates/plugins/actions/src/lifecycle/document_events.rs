//! `lifecycle/document-events.ts`: the verb-shaped document events
//! (WC/WS/DS/WP/DP) and the print door. One catalog lifecycle tree per
//! event, run inside the current queue operation and never failing (a
//! broken script must not cancel the user's save or print); the adapter
//! print wrapped will-print, print, did-print under the one latch.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};

use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionNodeStatus, ActionOrigin,
    ActionSource, ActionStepResult, ActionTriggerEvent, ActionTriggerResult, ActionUiContext,
    DocumentTriggerEvent, DocumentVerb,
};
use crate::dispatch::fold::fold_steps;
use crate::services::catalog::doc_event_tree_key;
use crate::services::{Diagnose, get, truthy};

/// An `async` function's `return promise`: resolved through the promise's
/// adoption, two turns after an already-settled promise.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.await
}

impl ActionsCapability {
    /// One catalog lifecycle tree inside the current queue operation. Never
    /// fails: a read failure degrades to a diagnostic.
    async fn run_doc_event_tree_safe(
        self,
        event: DocumentTriggerEvent,
        diagnose: Diagnose,
    ) -> Option<ActionStepResult> {
        if self.0.config.triggers.as_ref().and_then(|g| g.document) == Some(false) {
            return None;
        }
        let attempt: Result<Option<ActionStepResult>, Error> = async {
            rt::returned(self.clone().ensure_open_sequence_before_doc_event()).await;
            let snapshot = self.read_document_actions().await?;
            let tree = snapshot.as_ref().and_then(|s| get(s, doc_event_tree_key(event))).cloned();
            let runnable = tree
                .as_ref()
                .is_some_and(|t| truthy(get(t, "root")) || truthy(get(t, "incomplete")));
            let Some(tree) = tree.filter(|_| runnable) else { return Ok(None) };
            let ctx = ActionContext {
                origin: ActionOrigin::Lifecycle,
                source: ActionSource::Document,
                event: ActionTriggerEvent::Document { name: event },
            };
            let result = rt::returned(self.clone().run_and_emit(tree.clone(), ctx)).await?;
            Ok(Some(ActionStepResult { source: ActionSource::Document, tree, result }))
        }
        .await;
        match attempt {
            Ok(step) => step,
            Err(error) => {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::TriggerFailed,
                    format!("{}: {}", event.as_str(), error.message),
                ));
                None
            }
        }
    }

    /// The dispatch-path body for a verb-shaped document event.
    pub(crate) async fn run_document_event_op(
        self,
        event: DocumentTriggerEvent,
        diagnostics: Rc<RefCell<Vec<ActionDiagnostic>>>,
        diagnose: Diagnose,
    ) -> ActionTriggerResult {
        let step = rt::returned(self.clone().run_doc_event_tree_safe(event, diagnose)).await;
        fold_steps(step.into_iter().collect(), diagnostics.borrow().clone())
    }

    /// Both adapter print invocations go through here: will-print, the
    /// adapter exactly once, did-print, with the latch held throughout. An
    /// adapter throw skips did-print; the latch still resets.
    pub(crate) async fn fire_print_through_adapter(
        self,
        ui_context: Option<ActionUiContext>,
        diagnose: Diagnose,
    ) -> Result<(ActionNodeStatus, Option<String>), Error> {
        if self.0.print_latch.get() {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::ReentrantPrint,
                "print request during a document print event — suppressed (one dialog per request)",
            ));
            return Ok((ActionNodeStatus::Blocked, Some("reentrant print suppressed".into())));
        }
        if !self.allows_print() {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::Blocked,
                "print: doc.print is not allowed",
            ));
            return Ok((ActionNodeStatus::Blocked, Some("doc.print is not allowed".into())));
        }
        if self.0.ports.ui_adapter.borrow().is_none() {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::NoAdapter,
                "print: no UI adapter installed",
            ));
            return Ok((ActionNodeStatus::NoExecutor, Some("no UI adapter installed".into())));
        }
        self.0.print_latch.set(true);
        let outcome: Result<(ActionNodeStatus, Option<String>), Error> = async {
            rt::returned(
                self.clone()
                    .run_doc_event_tree_safe(DocumentTriggerEvent::WillPrint, diagnose.clone()),
            )
            .await;
            let adapter = self.0.ports.ui_adapter.borrow().clone();
            let Some(adapter) = adapter else {
                return Ok((
                    ActionNodeStatus::NoExecutor,
                    Some("UI adapter uninstalled mid-print".into()),
                ));
            };
            (adapter.print)(ui_context.as_ref())?;
            rt::returned(
                self.clone()
                    .run_doc_event_tree_safe(DocumentTriggerEvent::DidPrint, diagnose.clone()),
            )
            .await;
            Ok((ActionNodeStatus::Executed, None))
        }
        .await;
        self.0.print_latch.set(false);
        outcome
    }

    /// Run one embedder-owned document verb as one serialized operation:
    /// the open-ordering guard, the before-event tree, `operation()`, the
    /// after-event tree. A before-event failure never cancels the operation;
    /// `operation` failing (`Err` is a synchronous throw) skips the
    /// after-event and rejects; the print verb holds the latch throughout.
    pub fn run_document_verb<T: Clone + 'static>(
        &self,
        verb: DocumentVerb,
        operation: impl FnOnce() -> Result<Promise<T>, Error> + 'static,
    ) -> Promise<T> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                this.0.script_nodes.set(0.0); // one aggregate for the whole verb op
                let diagnose = this.emitter();
                let (before, after) = match verb {
                    DocumentVerb::Save => {
                        (DocumentTriggerEvent::WillSave, DocumentTriggerEvent::DidSave)
                    }
                    DocumentVerb::Print => {
                        (DocumentTriggerEvent::WillPrint, DocumentTriggerEvent::DidPrint)
                    }
                };
                let body = {
                    let this = this.clone();
                    async move {
                        rt::returned(
                            this.clone().run_doc_event_tree_safe(before, diagnose.clone()),
                        )
                        .await;
                        let value = operation()?.await?;
                        rt::returned(this.clone().run_doc_event_tree_safe(after, diagnose)).await;
                        Ok(value)
                    }
                };
                if verb != DocumentVerb::Print {
                    // `return body()`: the async function adopts it.
                    return adopt(Promise::spawn(body)).await;
                }
                this.0.print_latch.set(true);
                let value = rt::returned(body).await;
                this.0.print_latch.set(false);
                value
            }))
        })
    }
}
