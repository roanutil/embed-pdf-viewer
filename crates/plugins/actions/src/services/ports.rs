//! `services/ports.ts`: the registration slots, every port a sibling or the
//! embedder installs. Areas read them at call time, never captured, and
//! every disposer is identity-safe: it clears a slot only while the
//! registrant is still current. Executors are last-wins.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::ActionsCapability;
use crate::contract::{
    ActionDiagnostic, ActionDiagnosticCode, ActionExecutor, ActionSubmitHandler, ActionUiAdapter,
    Disposer,
};
use crate::host_contract::{AnnotCommitSink, FormCommitSink, SubmitResolver};

pub(crate) enum ExecutorKind {
    Registered(ActionExecutor),
    /// The built-in `javascript` executor over this document's realm.
    Script,
}

pub(crate) struct Registered {
    pub(crate) id: u64,
    pub(crate) kind: Rc<ExecutorKind>,
}

#[derive(Default)]
pub(crate) struct Ports {
    pub(crate) executors: RefCell<HashMap<String, Registered>>,
    pub(crate) next_id: Cell<u64>,
    pub(crate) annot_commit_sink: RefCell<Option<AnnotCommitSink>>,
    pub(crate) form_commit_sink: RefCell<Option<FormCommitSink>>,
    pub(crate) ui_adapter: RefCell<Option<Rc<ActionUiAdapter>>>,
    /// Sink 1 of the submit chain (consent is installation).
    pub(crate) submit_handler: RefCell<Option<ActionSubmitHandler>>,
    /// The form plugin's dataset resolver.
    pub(crate) submit_resolver: RefCell<Option<SubmitResolver>>,
}

/// An identity-safe slot disposer: clears the slot only while it still
/// holds `value`.
fn slot_disposer<T: ?Sized + 'static>(
    this: &ActionsCapability,
    value: Rc<T>,
    slot: fn(&Ports) -> &RefCell<Option<Rc<T>>>,
) -> Disposer {
    let weak = this.weak();
    Rc::new(move || {
        let Some(this) = ActionsCapability::upgrade(&weak) else { return };
        let cell = slot(&this.0.ports);
        let current = cell.borrow().as_ref().is_some_and(|s| Rc::ptr_eq(s, &value));
        if current {
            *cell.borrow_mut() = None;
        }
    })
}

impl ActionsCapability {
    pub(crate) fn executor(&self, kind: &str) -> Option<Rc<ExecutorKind>> {
        self.0.ports.executors.borrow().get(kind).map(|r| r.kind.clone())
    }

    pub(crate) fn install_executor(&self, action_type: &str, kind: ExecutorKind) -> u64 {
        let id = self.0.ports.next_id.get();
        self.0.ports.next_id.set(id + 1);
        self.0
            .ports
            .executors
            .borrow_mut()
            .insert(action_type.to_string(), Registered { id, kind: Rc::new(kind) });
        id
    }

    /// Interpret (or override) one action type: the door the stage and form
    /// plugins use for GoTo, Named, Hide and ResetForm. Last-wins on
    /// duplicates, with a `duplicate-executor` diagnostic; the disposer
    /// removes the entry only while it is still the current one.
    pub fn register_executor(&self, action_type: &str, executor: ActionExecutor) -> Disposer {
        if self.0.ports.executors.borrow().contains_key(action_type) {
            self.emit_diagnostic(ActionDiagnostic::new(
                ActionDiagnosticCode::DuplicateExecutor,
                format!("executor for '{action_type}' replaced (last-wins)"),
            ));
        }
        let id = self.install_executor(action_type, ExecutorKind::Registered(executor));
        let (weak, action_type) = (self.weak(), action_type.to_string());
        Rc::new(move || {
            let Some(this) = ActionsCapability::upgrade(&weak) else { return };
            let executors = &this.0.ports.executors;
            let current = executors.borrow().get(&action_type).is_some_and(|r| r.id == id);
            if current {
                executors.borrow_mut().remove(&action_type);
            }
        })
    }

    /// Host lens: the annotation plugin's commit sink.
    pub fn register_annot_commit_sink(&self, sink: AnnotCommitSink) -> Disposer {
        *self.0.ports.annot_commit_sink.borrow_mut() = Some(sink.clone());
        slot_disposer(self, sink, |p| &p.annot_commit_sink)
    }

    /// Host lens: the form plugin's commit sink.
    pub fn register_form_commit_sink(&self, sink: FormCommitSink) -> Disposer {
        *self.0.ports.form_commit_sink.borrow_mut() = Some(sink.clone());
        slot_disposer(self, sink, |p| &p.form_commit_sink)
    }

    /// Sink 1 of the submit chain. Installing it does not release the open
    /// latch; that stays the UI adapter's role. `None` force-clears.
    pub fn set_submit_handler(&self, handler: Option<ActionSubmitHandler>) -> Disposer {
        *self.0.ports.submit_handler.borrow_mut() = handler.clone();
        match handler {
            Some(handler) => slot_disposer(self, handler, |p| &p.submit_handler),
            // `slot === null` holds only while the slot is empty, and
            // clearing an empty slot changes nothing.
            None => Rc::new(|| {}),
        }
    }

    /// Host lens: the form plugin's dataset resolver.
    pub fn register_submit_resolver(&self, resolver: SubmitResolver) -> Disposer {
        *self.0.ports.submit_resolver.borrow_mut() = Some(resolver.clone());
        slot_disposer(self, resolver, |p| &p.submit_resolver)
    }
}
