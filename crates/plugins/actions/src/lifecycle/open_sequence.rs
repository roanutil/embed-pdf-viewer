//! `lifecycle/open-sequence.ts`: the §3.9 document-open sequence and its
//! latch. It fires once at the earliest of a UI adapter installing or the
//! first user-origin dispatch (`auto`), at bring-up (`headless`), or never
//! (`off`, which still releases the page barrier). D1's ordering law: the
//! sequence runs before the first verb-shaped document event.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use serde_json::{Value, json};

use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionOrigin, ActionSource,
    ActionStepResult, ActionTriggerEvent, ActionTriggerResult, ActionUiAdapter, Disposer,
    DocumentTriggerEvent, OpenSequence, OpenSequenceCompletedEvent,
};
use crate::dispatch::fold::fold_steps;
use crate::services::{DiagnosticPlace, get, truthy};

impl ActionsCapability {
    /// The `openSequence` setting, read when it decides: it applies until
    /// the sequence has run.
    fn open_sequence_mode(&self) -> OpenSequence {
        self.settings_now().open_sequence
    }

    /// The sequence body, inside the queue; callers set `open_fired`.
    pub(crate) async fn run_open_sequence_op(self) -> ActionTriggerResult {
        let mut diagnostics = Vec::new();
        let mut steps: Vec<ActionStepResult> = Vec::new();
        let lifecycle_ctx = ActionContext {
            origin: ActionOrigin::Lifecycle,
            source: ActionSource::Document,
            event: ActionTriggerEvent::Document { name: DocumentTriggerEvent::Open },
        };
        let attempt: Result<(), Error> = async {
            let snapshot = self.read_document_actions().await?.unwrap_or(Value::Null);
            if let Some(destination) = get(&snapshot, "openDestination").filter(|d| truthy(Some(d)))
            {
                // The initial view reuses the whole spine as a synthesized
                // lifecycle goto.
                let tree = json!({
                    "root": { "type": "goto", "subtype": "GoTo", "destination": destination, "next": [] },
                    "incomplete": false,
                    "warningFlags": 0,
                    "warnings": [],
                });
                let result =
                    rt::returned(self.clone().run_and_emit(tree.clone(), lifecycle_ctx.clone(), None))
                        .await?;
                steps.push(ActionStepResult { source: ActionSource::Document, tree, result });
            }
            if let Some(open_action) = get(&snapshot, "openAction")
                && (truthy(get(open_action, "root")) || truthy(get(open_action, "incomplete")))
            {
                let tree = open_action.clone();
                let result =
                    rt::returned(self.clone().run_and_emit(tree.clone(), lifecycle_ctx.clone(), None))
                        .await?;
                steps.push(ActionStepResult { source: ActionSource::Document, tree, result });
            }
            Ok(())
        }
        .await;
        if let Err(error) = attempt {
            let diagnostic = ActionDiagnostic::new(
                ActionDiagnosticCode::TriggerFailed,
                format!("open sequence failed: {}", error.message),
            );
            diagnostics.push(diagnostic.clone());
            self.report_diagnostic(&diagnostic, &DiagnosticPlace::source(ActionSource::Document));
        }
        // Release after the document steps, on every path. Page-open emission
        // enqueues behind this op.
        self.release_barrier(true);
        let result = fold_steps(steps, diagnostics);
        self.0
            .events
            .open_sequence_completed
            .emit(&OpenSequenceCompletedEvent { result: result.clone() });
        result
    }

    /// OpenAction never runs after a WillSave, WillPrint or WillClose script:
    /// an armed-but-unfired sequence runs inline first.
    pub(crate) async fn ensure_open_sequence_before_doc_event(self) {
        if self.0.open_fired.get() || self.open_sequence_mode() == OpenSequence::Off {
            return;
        }
        self.0.open_fired.set(true);
        rt::returned(self.clone().run_open_sequence_op()).await;
    }

    fn maybe_fire_open_sequence(&self) {
        if self.0.open_fired.get() {
            return;
        }
        if self.open_sequence_mode() == OpenSequence::Off {
            // The sequence never runs, but feeds must not buffer forever.
            self.0.open_fired.set(true);
            self.release_barrier(false);
            return;
        }
        if self.0.ports.ui_adapter.borrow().is_none()
            && self.open_sequence_mode() != OpenSequence::Headless
            && !self.0.saw_user_activity.get()
        {
            return;
        }
        self.0.open_fired.set(true);
        let this = self.clone();
        let _ = self
            .0
            .queue
            .run(move || Ok(Promise::spawn(async move { Ok(this.run_open_sequence_op().await) })));
    }

    pub(crate) fn note_user_activity(&self) {
        self.reset_cascade();
        if !self.0.saw_user_activity.get() {
            self.0.saw_user_activity.set(true);
            self.maybe_fire_open_sequence();
        }
    }

    /// Claim the one-shot open for a dispatched document-open trigger: false
    /// when it already fired.
    pub(crate) fn claim_open_sequence(&self) -> bool {
        if self.0.open_fired.get() {
            return false;
        }
        self.0.open_fired.set(true);
        true
    }

    /// Arm the latch at bring-up: fires now for `headless`, releases the
    /// barrier for `off`, waits for an adapter or user activity for `auto`.
    pub(crate) fn arm_open_sequence(&self) {
        self.maybe_fire_open_sequence();
    }

    /// Identity-safe port install: the disposer clears the slot only while
    /// this adapter is still current; `None` force-clears.
    pub fn set_ui_adapter(&self, adapter: Option<Rc<ActionUiAdapter>>) -> Disposer {
        *self.0.ports.ui_adapter.borrow_mut() = adapter.clone();
        // An adapter arriving is the open latch's usual release.
        if adapter.is_some() {
            self.maybe_fire_open_sequence();
        }
        let weak = self.weak();
        Rc::new(move || {
            let Some(this) = ActionsCapability::upgrade(&weak) else { return };
            let current = match (&*this.0.ports.ui_adapter.borrow(), &adapter) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if current {
                *this.0.ports.ui_adapter.borrow_mut() = None;
            }
        })
    }
}
