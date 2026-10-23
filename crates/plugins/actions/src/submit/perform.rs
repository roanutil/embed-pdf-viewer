//! `submit/perform.ts`: the one submit door. Sink order: the embedder's
//! handler (explicit beats ambient; detached-promise contract), then the
//! document's home (`doc.forms.submit`, awaited), then blocked. Never a
//! network call from this plugin.

use epdf_kernel::rt;

use super::intent::{iso_string, to_form_submission_request};
use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionNodeStatus, ActionPolicyDecision,
    SubmitHandlerContext, SubmitIntent, SubmitToDocumentHome,
};
use crate::services::{Diagnose, DiagnosticPlace};

impl ActionsCapability {
    /// `environment.now?.() ?? Date.now()`.
    pub(crate) fn now_ms(&self) -> f64 {
        match &self.0.environment.now {
            Some(now) => now(),
            None => rt::now(),
        }
    }

    /// `performSubmit(intent, ctx, diagnose)`: both sources land here after
    /// policy.
    pub(crate) async fn perform_submit(
        self,
        intent: SubmitIntent,
        action_ctx: ActionContext,
        diagnose: Diagnose,
    ) -> (ActionNodeStatus, Option<String>) {
        let decision = self.current_policy().submit_form.get(action_ctx.origin);
        if !matches!(decision, ActionPolicyDecision::Adapter | ActionPolicyDecision::Allow) {
            // `block` refuses quietly; `report` says so.
            if decision == ActionPolicyDecision::Report {
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::Blocked,
                    format!(
                        "submit-form: policy 'report' for origin '{}'",
                        action_ctx.origin.as_str()
                    ),
                ));
            }
            return (ActionNodeStatus::Blocked, None);
        }
        let resolver = self.0.ports.submit_resolver.borrow().clone();
        let Some(resolver) = resolver else {
            diagnose(ActionDiagnostic::new(
                ActionDiagnosticCode::NoSubmitResolver,
                "submit-form: no dataset resolver registered (form plugin missing?)",
            ));
            return (ActionNodeStatus::Blocked, Some("no dataset resolver".into()));
        };
        let resolved = match resolver(&intent, &action_ctx, diagnose.clone()) {
            Ok(promise) => promise.await,
            Err(error) => Err(error),
        };
        let request = match resolved {
            Ok(request) => request,
            Err(error) => {
                let detail = error.message;
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::ExecutorFailed,
                    format!("submit-form: dataset resolution: {detail}"),
                ));
                return (ActionNodeStatus::Failed, Some(detail));
            }
        };
        let home: Option<SubmitToDocumentHome> = match self.doc() {
            Ok(doc) if doc.has("forms.submit") => {
                let (this, request) = (self.clone(), request.clone());
                Some(std::rc::Rc::new(move || {
                    let body = to_form_submission_request(&request, &iso_string(this.now_ms())?);
                    // `Promise.resolve(guarded)` is the guarded promise itself.
                    Ok(doc.call("forms.submit", None, vec![body]))
                }))
            }
            _ => None,
        };
        let handler = self.0.ports.submit_handler.borrow().clone();
        if let Some(handler) = handler {
            return match handler(&request, SubmitHandlerContext { submit_to_document_home: home }) {
                Ok(outcome) => {
                    if let Some(outcome) = outcome {
                        // Detached by contract: `executed` means handed to the
                        // embedder; a later rejection is observability only.
                        let (this, source) = (self.clone(), action_ctx.source.clone());
                        rt::spawn(async move {
                            if let Err(error) = outcome.await {
                                this.report_diagnostic(
                                    &ActionDiagnostic::new(
                                        ActionDiagnosticCode::ExecutorFailed,
                                        format!(
                                            "submit handler rejected (detached): {}",
                                            error.message
                                        ),
                                    ),
                                    &DiagnosticPlace::of(
                                        Some("submit-form".into()),
                                        source.clone(),
                                    ),
                                );
                            }
                        });
                    }
                    (ActionNodeStatus::Executed, None)
                }
                Err(error) => {
                    let detail = error.message;
                    diagnose(ActionDiagnostic::new(
                        ActionDiagnosticCode::ExecutorFailed,
                        format!("submit handler threw: {detail}"),
                    ));
                    (ActionNodeStatus::Failed, Some(detail))
                }
            };
        }
        if let Some(home) = home {
            let submitted: Result<_, _> = match home() {
                Ok(promise) => promise.await,
                Err(error) => Err(error),
            };
            return match submitted {
                Ok(_) => (ActionNodeStatus::Executed, None),
                Err(error) => {
                    // The engine's refusal, surfaced on the node.
                    let detail = error.message;
                    diagnose(ActionDiagnostic::new(
                        ActionDiagnosticCode::ExecutorFailed,
                        format!("document home refused submit: {detail}"),
                    ));
                    (ActionNodeStatus::Failed, Some(detail))
                }
            };
        }
        diagnose(ActionDiagnostic::new(
            ActionDiagnosticCode::NoSubmitSink,
            "submit-form: no handler installed and the document has no submit-capable home",
        ));
        (ActionNodeStatus::Blocked, Some("no submit sink".into()))
    }
}
