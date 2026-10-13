//! `controller.ts`: selectors (pure reads), intents (the only writers), the
//! pane events diffed on every write, and the reconciliation against the
//! document registry wired at `connect`. It only arranges document ids into
//! panes.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, DOCUMENTS, Error};

use crate::contract::{DocumentMovedEvent, PaneEvent, PaneFocusChangedEvent, PaneId, PaneInfo};
use crate::model::{Pane, ViewManagerAction, ViewManagerState, js_max, js_min, slice_end};

struct Inner {
    cx: Controller<ViewManagerState, ViewManagerAction>,
    pane_created: EventHookSource<PaneEvent>,
    pane_removed: EventHookSource<PaneEvent>,
    focus_changed: EventHookSource<PaneFocusChangedEvent>,
    document_moved: EventHookSource<DocumentMovedEvent>,
}

/// `ViewManagerCapability` (and `ViewManagerHostCapability`, which equals it).
#[derive(Clone)]
pub struct ViewManagerCapability(Rc<Inner>);

fn info_of(pane: &Pane) -> PaneInfo {
    PaneInfo {
        id: pane.id.clone(),
        document_ids: pane.document_ids.clone(),
        active_document_id: pane.active_document_id.clone(),
    }
}

/// `paneOfDocument(state, documentId)`: the first pane in display order
/// that holds the document.
fn pane_of_document(state: &ViewManagerState, document_id: &str) -> Option<PaneId> {
    state
        .order
        .iter()
        .find(|id| {
            state.panes.get(id).is_some_and(|p| p.document_ids.iter().any(|d| d == document_id))
        })
        .cloned()
}

impl ViewManagerCapability {
    fn state(&self) -> Rc<ViewManagerState> {
        self.0.cx.state()
    }

    /// Aborted when the kernel is destroyed: a workspace plugin outlives
    /// every document. The foreign handle checks it before every call.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    /// `emitDiffs(before, after)`: panes created (in the new order), panes
    /// removed (in the old order), the focus, then each document whose pane
    /// changed, documents enumerated from the old panes then the new ones.
    fn emit_diffs(&self, before: &ViewManagerState, after: &ViewManagerState) {
        for id in &after.order {
            if !before.panes.contains_key(id) {
                self.0.pane_created.emit(&PaneEvent { pane_id: id.clone() });
            }
        }
        for id in &before.order {
            if !after.panes.contains_key(id) {
                self.0.pane_removed.emit(&PaneEvent { pane_id: id.clone() });
            }
        }
        if before.focused_pane_id != after.focused_pane_id {
            self.0
                .focus_changed
                .emit(&PaneFocusChangedEvent { pane_id: after.focused_pane_id.clone() });
        }
        if before.panes != after.panes {
            let mut documents: Vec<&str> = Vec::new();
            for pane in before.panes.values().chain(after.panes.values()) {
                for d in &pane.document_ids {
                    if !documents.contains(&d.as_str()) {
                        documents.push(d);
                    }
                }
            }
            for document_id in documents {
                let from = pane_of_document(before, document_id);
                let to = pane_of_document(after, document_id);
                if from != to {
                    self.0.document_moved.emit(&DocumentMovedEvent {
                        document_id: document_id.to_string(),
                        from_pane_id: from,
                        to_pane_id: to,
                    });
                }
            }
        }
    }

    fn dispatch(&self, action: ViewManagerAction) {
        let before = self.state();
        self.0.cx.dispatch(action);
        let after = self.state();
        if !Rc::ptr_eq(&before, &after) {
            self.emit_diffs(&before, &after);
        }
    }

    // ── reading ──

    /// Panes in display order.
    pub fn list_panes(&self) -> Vec<PaneInfo> {
        let state = self.state();
        state.order.iter().filter_map(|id| state.panes.get(id).map(info_of)).collect()
    }

    pub fn get_pane(&self, id: &str) -> Option<PaneInfo> {
        self.state().panes.get(id).map(info_of)
    }

    pub fn get_pane_order(&self) -> Vec<PaneId> {
        self.state().order.clone()
    }

    pub fn get_focused_pane_id(&self) -> Option<PaneId> {
        self.state().focused_pane_id.clone()
    }

    /// Which pane holds this document, if any.
    pub fn get_pane_of_document(&self, document_id: &str) -> Option<PaneId> {
        pane_of_document(&self.state(), document_id)
    }

    // ── panes ──

    /// A new pane, focused, optionally populated: each document moves in
    /// from wherever it was.
    pub fn create_pane(&self, document_ids: &[String]) -> PaneId {
        let id = format!("pane-{}", self.state().seq + 1);
        self.dispatch(ViewManagerAction::CreatePane {
            pane: Pane { id: id.clone(), document_ids: Vec::new(), active_document_id: None },
        });
        for document_id in document_ids {
            self.dispatch(ViewManagerAction::AddDoc {
                pane_id: id.clone(),
                document_id: document_id.clone(),
                index: None,
            });
        }
        id
    }

    /// Remove a pane. Its documents go to `move_documents_to`, else the
    /// focused pane when that is another one, else the last remaining pane;
    /// with no such pane they leave with it.
    pub fn remove_pane(&self, id: &str, move_documents_to: Option<&str>) {
        let state = self.state();
        let Some(pane) = state.panes.get(id).cloned() else { return };
        let remaining: Vec<&PaneId> = state.order.iter().filter(|other| *other != id).collect();
        let focused = state.focused_pane_id.clone().filter(|f| f != id);
        let target = move_documents_to
            .map(str::to_string)
            .or(focused)
            .or_else(|| remaining.last().map(|p| p.to_string()));
        if let Some(target) = target
            && !target.is_empty()
            && target != id
            && state.panes.contains_key(&target)
        {
            for document_id in pane.document_ids {
                self.dispatch(ViewManagerAction::MoveDocBetween {
                    from_pane_id: id.to_string(),
                    to_pane_id: target.clone(),
                    document_id,
                    to_index: None,
                });
            }
        }
        self.dispatch(ViewManagerAction::RemovePane { id: id.to_string() });
    }

    /// Move a pane to a new position in the order. Unknown ids are ignored.
    pub fn move_pane(&self, id: &str, to_index: f64) {
        let current = self.state().order.clone();
        let without: Vec<String> = current.iter().filter(|x| *x != id).cloned().collect();
        if without.len() == current.len() {
            return;
        }
        let clamped = js_max(0.0, js_min(to_index, without.len() as f64));
        let at = slice_end(clamped, without.len());
        let mut order = without[..at].to_vec();
        order.push(id.to_string());
        order.extend_from_slice(&without[at..]);
        self.dispatch(ViewManagerAction::ReorderPanes { order });
    }

    pub fn set_focused_pane(&self, id: Option<&str>) {
        self.dispatch(ViewManagerAction::SetFocused { id: id.map(str::to_string) });
    }

    /// A new pane showing the document, in one call. The TypeScript's
    /// `options.from` is accepted there and unused, so it is not ported.
    pub fn split_pane(&self, document_id: &str) -> PaneId {
        self.create_pane(&[document_id.to_string()])
    }

    // ── tabs ──

    /// The tab the pane shows.
    pub fn set_active_document(&self, pane_id: &str, document_id: Option<&str>) {
        self.dispatch(ViewManagerAction::SetActiveDoc {
            pane_id: pane_id.to_string(),
            document_id: document_id.map(str::to_string),
        });
    }

    /// Add a document to a pane; a document another pane holds moves.
    pub fn add_document(&self, pane_id: &str, document_id: &str, index: Option<f64>) {
        self.dispatch(ViewManagerAction::AddDoc {
            pane_id: pane_id.to_string(),
            document_id: document_id.to_string(),
            index,
        });
    }

    pub fn remove_document(&self, pane_id: &str, document_id: &str) {
        self.dispatch(ViewManagerAction::RemoveDoc {
            pane_id: pane_id.to_string(),
            document_id: document_id.to_string(),
        });
    }

    /// Reorder a tab inside its pane.
    pub fn move_document_within(&self, pane_id: &str, document_id: &str, to_index: f64) {
        self.dispatch(ViewManagerAction::MoveDocWithin {
            pane_id: pane_id.to_string(),
            document_id: document_id.to_string(),
            to_index,
        });
    }

    /// Drag a tab from one pane into another.
    pub fn move_document_between(
        &self,
        from_pane_id: &str,
        to_pane_id: &str,
        document_id: &str,
        to_index: Option<f64>,
    ) {
        self.dispatch(ViewManagerAction::MoveDocBetween {
            from_pane_id: from_pane_id.to_string(),
            to_pane_id: to_pane_id.to_string(),
            document_id: document_id.to_string(),
            to_index,
        });
    }

    // ── events ──

    pub fn on_pane_created(
        &self,
        listener: impl Fn(&PaneEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.pane_created.on(listener)
    }

    pub fn on_pane_removed(
        &self,
        listener: impl Fn(&PaneEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.pane_removed.on(listener)
    }

    pub fn on_focus_changed(
        &self,
        listener: impl Fn(&PaneFocusChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.focus_changed.on(listener)
    }

    pub fn on_document_moved(
        &self,
        listener: impl Fn(&DocumentMovedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.document_moved.on(listener)
    }

    /// Keep panes consistent with the document registry: newly opened
    /// documents land in the focused pane (a default pane is created on
    /// first open), closed ones are dropped from whatever pane held them.
    /// The open order is compared joined with `|`, as the TypeScript does.
    fn connect(&self) -> Result<(), Error> {
        let documents = self.0.cx.get(&DOCUMENTS)?;
        let last: RefCell<Option<String>> = RefCell::new(None);
        let this = self.clone();
        let reconcile = Rc::new(move || {
            let order = documents.get_order();
            let key = order.join("|");
            if last.borrow().as_deref() == Some(key.as_str()) {
                return Ok(());
            }
            *last.borrow_mut() = Some(key);
            let prefer_pane_id = this.state().focused_pane_id.clone();
            this.dispatch(ViewManagerAction::Reconcile { open: order, prefer_pane_id });
            Ok(())
        });
        reconcile()?;
        let unsubscribe = self.0.cx.subscribe(reconcile);
        self.0.cx.cleanup_fn(move || unsubscribe.call());
        Ok(())
    }
}

/// `createViewManagerController(ctx)`: the api and its `connect`.
pub fn create_view_manager_controller(
    cx: Controller<ViewManagerState, ViewManagerAction>,
) -> (ViewManagerCapability, ConnectFn) {
    // The TypeScript reports a failing listener to `console.error`.
    let report: Rc<dyn Fn(&Error)> =
        Rc::new(|error| eprintln!("[view-manager] listener failed: {error}"));
    let inner = Inner {
        pane_created: EventHookSource::new(Some(report.clone())),
        pane_removed: EventHookSource::new(Some(report.clone())),
        focus_changed: EventHookSource::new(Some(report.clone())),
        document_moved: EventHookSource::new(Some(report)),
        cx,
    };
    let hooks = (
        inner.pane_created.clone(),
        inner.pane_removed.clone(),
        inner.focus_changed.clone(),
        inner.document_moved.clone(),
    );
    inner.cx.cleanup_fn(move || {
        hooks.0.dispose();
        hooks.1.dispose();
        hooks.2.dispose();
        hooks.3.dispose();
    });
    let api = ViewManagerCapability(Rc::new(inner));
    let connecting = api.clone();
    (api, Rc::new(move || connecting.connect()))
}
