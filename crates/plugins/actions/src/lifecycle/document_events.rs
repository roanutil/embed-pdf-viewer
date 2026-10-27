//! `lifecycle/document-events.ts`: the verb-shaped document events
//! (WC/WS/DS/WP/DP) and the print door. One catalog lifecycle tree per
//! event, run inside the current queue operation and never failing (a
//! broken script must not cancel the user's save or print), and a print
//! request sent to the kernel's print, or, without a printer, to the
//! adapter, wrapped will-print, print, did-print under the one latch.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;

use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionNodeStatus, ActionOrigin,
    ActionPolicyDecision, ActionSource, ActionStepResult, ActionTriggerEvent, ActionTriggerResult,
    ActionUiContext, DocumentTriggerEvent, DocumentVerb,
};
use crate::dispatch::dispatcher::cancelled;
use crate::dispatch::fold::fold_steps;
use crate::services::catalog::doc_event_tree_key;
use crate::services::{Diagnose, DiagnosticPlace, get, truthy};

/// An `async` function's `return promise`: resolved through the promise's
/// adoption, two turns after an already-settled promise.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
}

impl ActionsCapability {
    /// One catalog lifecycle tree inside the current queue operation. Never
    /// fails: a read failure degrades to a diagnostic.
    async fn run_doc_event_tree_safe(
        self,
        event: DocumentTriggerEvent,
        diagnose: Diagnose,
    ) -> Option<ActionStepResult> {
        if !self.settings_now().triggers.document {
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
            let result = rt::returned(self.clone().run_and_emit(tree.clone(), ctx, None)).await?;
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

    /// Both print paths (the Named Print verb, and script `print()` effects
    /// from the actions plane) go through here. While the latch is held a
    /// nested request is suppressed with `reentrant-print`.
    ///
    /// With the kernel's print, a user's request starts it and returns; a
    /// hover or lifecycle request never opens the dialog. Without a printer:
    /// will-print, the adapter exactly once, did-print, with the latch reset
    /// on every path. An adapter throw skips did-print.
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
        let printer = self.0.document_print.clone();
        if (printer.available)() {
            if let Some(ui) = &ui_context
                && ui.origin != ActionOrigin::User
            {
                let origin = ui.origin.as_str();
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::Blocked,
                    format!("print: a {origin} request never prints"),
                ));
                return Ok((ActionNodeStatus::Blocked, Some(format!("{origin} print request"))));
            }
            let started = (printer.start)();
            let diagnose = diagnose.clone();
            rt::spawn(async move {
                if let Err(error) = started.reaction().await {
                    diagnose(ActionDiagnostic::new(
                        ActionDiagnosticCode::TriggerFailed,
                        format!("print: {}", error.message),
                    ));
                }
            });
            return Ok((ActionNodeStatus::Executed, None));
        }
        // M14: a script's print() is not a dispatched node, so the print row
        // has not been consulted yet; the dispatch path (no context) already
        // was. The printer path above refuses every non-user request itself.
        if let Some(ui) = &ui_context {
            let decision = self.current_policy().print.get(ui.origin);
            if matches!(decision, ActionPolicyDecision::Block | ActionPolicyDecision::Report) {
                let decision = decision.as_str();
                diagnose(ActionDiagnostic::new(
                    ActionDiagnosticCode::Blocked,
                    format!("print: policy '{decision}' for origin '{}'", ui.origin.as_str()),
                ));
                return Ok((ActionNodeStatus::Blocked, Some(format!("policy '{decision}'"))));
            }
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
        signal: Option<AbortSignal>,
    ) -> Promise<T> {
        let this = self.clone();
        let queued = signal.clone();
        self.enqueue(
            move || {
                Ok(Promise::spawn(async move {
                    this.0.script_nodes.set(0.0); // one script budget for the whole verb operation
                    let diagnose = this.reporter(DiagnosticPlace::source(ActionSource::Document));
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
                            // A before-event failure never cancels the user's verb.
                            rt::returned(
                                this.clone().run_doc_event_tree_safe(before, diagnose.clone()),
                            )
                            .await;
                            // The caller can still cancel here; once the
                            // operation started, it finishes, and so does its
                            // after-event.
                            if signal.as_ref().is_some_and(AbortSignal::aborted) {
                                let verb = match verb {
                                    DocumentVerb::Save => "save",
                                    DocumentVerb::Print => "print",
                                };
                                return Err(cancelled(verb, signal.as_ref()));
                            }
                            // `operation()` throwing skips the after-event.
                            let value = operation()?.await?;
                            rt::returned(this.clone().run_doc_event_tree_safe(after, diagnose))
                                .await;
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
            },
            queued,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use epdf_kernel::kernel::{OpenOptions, OpenSource};
    use epdf_kernel::rt::{self, Promise};
    use epdf_kernel::{Kernel, KernelOptions};
    use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
    use serde_json::json;

    use crate::contract::{
        ACTIONS, ActionDiagnostic, ActionNodeStatus, ActionOrigin, ActionPolicyDecision,
        ActionPolicyPatch, ActionUiAdapter, ActionUiContext, ActionsConfig, ActionsSettingsPatch,
        OpenSequence, PartialActionPolicyRow, ScriptPhase,
    };
    use crate::{ActionsCapability, actions_plugin};

    fn opened() -> (Kernel, ActionsCapability) {
        opened_with(None)
    }

    fn opened_with(
        printer: Option<Rc<dyn epdf_kernel::print::HostPrinter>>,
    ) -> (Kernel, ActionsCapability) {
        rt::reset();
        let fake = FakeEngine::new(json!({
            "documents": { "a": {
                "security": { "allows": "*" },
                "pages": [],
                "responses": [{
                    "path": "actions.get",
                    "value": { "nameTreeScripts": [], "openAction": null, "openDestination": null },
                }],
            } }
        }));
        let config = ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() };
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake)),
            vec![actions_plugin(config)],
            KernelOptions { printer, ..KernelOptions::default() },
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
        let actions = kernel.capability(&ACTIONS, Some("a")).expect("actions");
        (kernel, actions)
    }

    /// What the print door answers a script print from `origin`, and the
    /// diagnostics it gave.
    fn script_print(
        actions: &ActionsCapability,
        origin: ActionOrigin,
    ) -> ((ActionNodeStatus, Option<String>), Vec<ActionDiagnostic>) {
        let seen: Rc<RefCell<Vec<ActionDiagnostic>>> = Rc::default();
        let log = seen.clone();
        let ui = ActionUiContext { origin, phase: ScriptPhase::User };
        let outcome = Promise::spawn(
            actions
                .clone()
                .fire_print_through_adapter(Some(ui), Rc::new(move |d| log.borrow_mut().push(d))),
        );
        rt::run_until_stalled();
        let outcome = outcome.peek().expect("settled").expect("an outcome");
        (outcome, seen.borrow().clone())
    }

    fn messages(diagnostics: &[ActionDiagnostic]) -> Vec<&str> {
        diagnostics.iter().map(|d| d.message.as_str()).collect()
    }

    #[test]
    fn m14_a_script_print_reads_the_print_row_with_these_words() {
        let (_kernel, actions) = opened();
        let _adapter = actions.set_ui_adapter(Some(Rc::new(ActionUiAdapter {
            open_uri: Rc::new(|_, _| Ok(())),
            print: Rc::new(|_| Ok(())),
            alert: None,
            goto_page: None,
        })));

        let (outcome, said) = script_print(&actions, ActionOrigin::Lifecycle);
        assert_eq!(outcome, (ActionNodeStatus::Blocked, Some("policy 'block'".into())));
        assert_eq!(messages(&said), ["print: policy 'block' for origin 'lifecycle'"]);

        let row = |user| ActionsSettingsPatch {
            policy: Some(ActionPolicyPatch {
                print: Some(PartialActionPolicyRow { user: Some(user), ..Default::default() }),
                ..Default::default()
            }),
            ..Default::default()
        };
        actions.update_settings(&row(ActionPolicyDecision::Block));
        let (outcome, said) = script_print(&actions, ActionOrigin::User);
        assert_eq!(outcome, (ActionNodeStatus::Blocked, Some("policy 'block'".into())));
        assert_eq!(messages(&said), ["print: policy 'block' for origin 'user'"]);

        actions.update_settings(&row(ActionPolicyDecision::Report));
        let (outcome, said) = script_print(&actions, ActionOrigin::User);
        assert_eq!(outcome, (ActionNodeStatus::Blocked, Some("policy 'report'".into())));
        assert_eq!(messages(&said), ["print: policy 'report' for origin 'user'"]);

        actions.update_settings(&row(ActionPolicyDecision::Adapter));
        let (outcome, said) = script_print(&actions, ActionOrigin::User);
        assert_eq!(outcome, (ActionNodeStatus::Executed, None));
        assert!(said.is_empty());
    }

    /// A printer that logs what it stages and prints, and whether the
    /// document's print verb (the actions plugin's `aroundPrint` wrap) held
    /// the print latch while the dialog opened.
    struct Printer {
        log: Rc<RefCell<Vec<String>>>,
        latch: Rc<RefCell<Option<ActionsCapability>>>,
    }
    struct Staged(Rc<RefCell<Vec<String>>>, Rc<RefCell<Option<ActionsCapability>>>);
    impl epdf_kernel::print::StagedPrint for Staged {
        fn print(&self) -> Promise<()> {
            let held = self.1.borrow().as_ref().map(|a| a.0.print_latch.get());
            self.0.borrow_mut().push(format!("print latch={held:?}"));
            Promise::resolved(())
        }
        fn dispose(&self) {
            self.0.borrow_mut().push("dispose".into());
        }
    }
    impl epdf_kernel::print::HostPrinter for Printer {
        fn stage(
            &self,
            sheets: Vec<epdf_kernel::print::PrintSheet>,
            _: epdf_kernel::print::StageOptions,
        ) -> Promise<Rc<dyn epdf_kernel::print::StagedPrint>> {
            self.log.borrow_mut().push(format!("stage {}", sheets.len()));
            Promise::resolved(Rc::new(Staged(self.log.clone(), self.latch.clone()))
                as Rc<dyn epdf_kernel::print::StagedPrint>)
        }
    }

    fn with_printer() -> (Kernel, ActionsCapability, Rc<RefCell<Vec<String>>>) {
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let latch: Rc<RefCell<Option<ActionsCapability>>> = Rc::default();
        let printer = Printer { log: log.clone(), latch: latch.clone() };
        let (kernel, actions) = opened_with(Some(Rc::new(printer)));
        *latch.borrow_mut() = Some(actions.clone());
        (kernel, actions, log)
    }

    #[test]
    fn with_a_printer_a_user_print_prints_through_the_kernel_inside_the_print_verb() {
        // document-events.ts firePrintThroughAdapter: documentPrint.available()
        // → documentPrint.start(), status executed, the adapter never called;
        // controller.ts:94 aroundPrint runs the print inside
        // runDocumentVerb('print'), which holds the print latch.
        let (_kernel, actions, log) = with_printer();
        let adapter_prints = Rc::new(RefCell::new(0));
        let count = adapter_prints.clone();
        let _adapter = actions.set_ui_adapter(Some(Rc::new(ActionUiAdapter {
            open_uri: Rc::new(|_, _| Ok(())),
            print: Rc::new(move |_| {
                *count.borrow_mut() += 1;
                Ok(())
            }),
            alert: None,
            goto_page: None,
        })));
        let (outcome, said) = script_print(&actions, ActionOrigin::User);
        assert_eq!(outcome, (ActionNodeStatus::Executed, None));
        assert!(said.is_empty());
        assert_eq!(*log.borrow(), ["stage 0", "print latch=Some(true)", "dispose"]);
        assert_eq!(*adapter_prints.borrow(), 0);
        assert!(!actions.0.print_latch.get());

        // The dispatch path (no UI context) prints too.
        log.borrow_mut().clear();
        let outcome =
            Promise::spawn(actions.clone().fire_print_through_adapter(None, Rc::new(|_| {})));
        rt::run_until_stalled();
        assert_eq!(outcome.peek().unwrap().unwrap(), (ActionNodeStatus::Executed, None));
        assert_eq!(*log.borrow(), ["stage 0", "print latch=Some(true)", "dispose"]);
    }

    #[test]
    fn m14_with_a_printer_a_non_user_print_request_never_prints() {
        // document-events.ts: `if (uiContext && uiContext.origin !== 'user')`
        // diagnoses `print: a <origin> request never prints`, blocked.
        let (_kernel, actions, log) = with_printer();
        for origin in [ActionOrigin::Lifecycle, ActionOrigin::Hover] {
            let (outcome, said) = script_print(&actions, origin);
            let name = origin.as_str();
            assert_eq!(outcome, (ActionNodeStatus::Blocked, Some(format!("{name} print request"))));
            assert_eq!(messages(&said), [format!("print: a {name} request never prints").as_str()]);
        }
        assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    }

    #[test]
    fn with_a_printer_a_form_pipeline_print_effect_prints_for_the_user_only() {
        // scripting/surface.ts:85-97.
        use crate::host_contract::{ScriptRealmKind, ScriptSurfaceResult};
        let (_kernel, actions, log) = with_printer();
        let suppressed: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = suppressed.clone();
        let _off = actions.on_script_diagnostic_reported(move |d| {
            sink.borrow_mut().push(d.message.clone());
            Ok(())
        });
        let result = |origin| ScriptSurfaceResult {
            ui_effects: vec![epdf_acrojs::ScriptUiEffect::Print],
            diagnostics: vec![],
            error: None,
            origin,
            phase: ScriptPhase::User,
            realm: ScriptRealmKind::Document,
            source: None,
        };
        actions.surface_script_result(&result(ActionOrigin::Lifecycle)).unwrap();
        rt::run_until_stalled();
        assert_eq!(
            *suppressed.borrow(),
            ["script print request from a lifecycle event suppressed"]
        );
        assert!(log.borrow().is_empty());
        actions.surface_script_result(&result(ActionOrigin::User)).unwrap();
        rt::run_until_stalled();
        assert_eq!(*log.borrow(), ["stage 0", "print latch=Some(true)", "dispose"]);
    }

    #[test]
    fn the_kernels_print_runs_inside_the_documents_print_actions() {
        // controller.ts:94: every kernel print, whoever starts it, runs inside
        // runDocumentVerb('print').
        let (kernel, actions, log) = with_printer();
        let printed = kernel.documents().print(Some("a"), Default::default());
        rt::run_until_stalled();
        printed.peek().expect("settled").expect("printed");
        assert_eq!(*log.borrow(), ["stage 0", "print latch=Some(true)", "dispose"]);
        assert!(!actions.0.print_latch.get());
    }
}
