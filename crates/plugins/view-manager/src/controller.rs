//! `controller.ts`: reads, verbs that apply the pure transitions in
//! [`model`](crate::model), the pane events derived from each state change,
//! and the reconciliation against the document registry wired in
//! `connect`. It only arranges document ids into panes; it touches no
//! engine.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, DOCUMENTS, Error};
use serde_json::Value;

use crate::contract::{DocumentMovedEvent, PaneEvent, PaneFocusChangedEvent, PaneId, PaneInfo};
use crate::model::{self, Pane, Panes, ViewManagerState};

type PaneView = Option<Rc<PaneInfo>>;
type PaneList = Rc<Vec<Rc<PaneInfo>>>;

/// The identity memos (`memoByKey`, `memo`). Each holds the `Rc` inputs it
/// compares, so an input can't be freed and its address reused while the
/// cache still names it.
#[derive(Default)]
struct Memos {
    /// `paneInfoOf`, per id; unbounded, as the TypeScript's has no
    /// `maxEntries`.
    panes: HashMap<String, (Option<Rc<Pane>>, PaneView)>,
    /// `listPanes`, over `[panes, order]`.
    list: Option<(Rc<Panes>, Rc<Vec<PaneId>>, PaneList)>,
}

struct Inner {
    cx: Controller<ViewManagerState, Value>,
    pane_created: EventHookSource<PaneEvent>,
    pane_removed: EventHookSource<PaneEvent>,
    focus_changed: EventHookSource<PaneFocusChangedEvent>,
    document_moved: EventHookSource<DocumentMovedEvent>,
    memos: RefCell<Memos>,
}

/// `ViewManagerCapability`: panes, tabs, and which document each pane
/// shows.
#[derive(Clone)]
pub struct ViewManagerCapability(Rc<Inner>);

/// `Object.is` over an optional `Rc` input.
fn same_pane(a: &Option<Rc<Pane>>, b: &Option<Rc<Pane>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl ViewManagerCapability {
    fn state(&self) -> Rc<ViewManagerState> {
        self.0.cx.state()
    }

    /// `ctx.state.update(transition, ...args)`.
    fn update(&self, transition: impl FnOnce(&Rc<ViewManagerState>) -> Rc<ViewManagerState>) {
        self.0.cx.update_state(|state| transition(&state));
    }

    /// Aborted when the kernel is destroyed: a workspace plugin outlives
    /// every document. The foreign handle checks it before every call.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    /// `paneInfoOf`: a pane's public shape, the same `Rc` until its record
    /// changes.
    fn pane_info_of(&self, id: &str) -> PaneView {
        let pane = self.state().panes.get(id).cloned();
        if let Some((inputs, result)) = self.0.memos.borrow().panes.get(id)
            && same_pane(inputs, &pane)
        {
            return result.clone();
        }
        let result = pane.as_ref().map(|pane| {
            Rc::new(PaneInfo {
                id: pane.id.clone(),
                document_ids: pane.document_ids.clone(),
                active_document_id: pane.active_document_id.clone(),
            })
        });
        self.0.memos.borrow_mut().panes.insert(id.to_string(), (pane, result.clone()));
        result
    }

    // ── reading ──

    /// Every pane, in order. The same `Rc` until a pane changes.
    pub fn list_panes(&self) -> PaneList {
        let state = self.state();
        if let Some((panes, order, result)) = &self.0.memos.borrow().list
            && Rc::ptr_eq(panes, &state.panes)
            && Rc::ptr_eq(order, &state.order)
        {
            return result.clone();
        }
        // `order.map((id) => paneInfoOf(id)!)`: every order entry has a pane.
        let result: PaneList =
            Rc::new(state.order.iter().filter_map(|id| self.pane_info_of(id)).collect());
        self.0.memos.borrow_mut().list =
            Some((state.panes.clone(), state.order.clone(), result.clone()));
        result
    }

    /// One pane, or `None` when there is none with that id.
    pub fn get_pane(&self, id: &str) -> PaneView {
        self.pane_info_of(id)
    }

    /// The panes' ids, in order: the state's own array.
    pub fn get_pane_order(&self) -> Rc<Vec<PaneId>> {
        self.state().order.clone()
    }

    /// The pane new documents open in, if any.
    pub fn get_focused_pane_id(&self) -> Option<PaneId> {
        self.state().focused_pane_id.clone()
    }

    /// Which pane holds this document, if any.
    pub fn get_pane_of_document(&self, document_id: &str) -> Option<PaneId> {
        model::pane_of_document(&self.state(), document_id)
    }

    // ── panes ──

    /// `createPaneWith(documentIds, index)`: a new pane at `index` (the end
    /// when left out), focused, with these documents moved into it.
    fn create_pane_with(&self, document_ids: &[String], index: Option<f64>) -> PaneId {
        let id = model::next_pane_id(&self.state());
        self.update(|state| model::create_pane(state, index));
        for document_id in document_ids {
            self.update(|state| model::add_document(state, &id, document_id, None));
        }
        id
    }

    /// Add a pane at the end, empty or with these documents moved into it,
    /// and focus it. Answers its id.
    pub fn create_pane(&self, document_ids: &[String]) -> PaneId {
        self.create_pane_with(document_ids, None)
    }

    /// Close a pane. Its documents move to `move_documents_to`, or else to
    /// the focused or the last pane; a target that is the pane itself or
    /// no pane moves nothing.
    pub fn remove_pane(&self, id: &str, move_documents_to: Option<&str>) {
        let state = self.state();
        let Some(pane) = state.panes.get(id).cloned() else { return };
        let remaining: Vec<&PaneId> = state.order.iter().filter(|other| *other != id).collect();
        let focused = state.focused_pane_id.clone();
        let target_id = move_documents_to
            .map(str::to_string)
            .or_else(|| focused.filter(|focused| focused != id))
            .or_else(|| remaining.last().map(|other| other.to_string()));
        if let Some(target_id) = target_id
            && !target_id.is_empty()
            && target_id != id
            && self.state().panes.contains_key(&target_id)
        {
            for document_id in &pane.document_ids {
                self.update(|state| {
                    model::move_document_between(state, id, &target_id, document_id, None)
                });
            }
        }
        self.update(|state| model::remove_pane(state, id));
    }

    /// Move a pane to another place in the order. Unknown ids are ignored.
    pub fn move_pane(&self, id: &str, to_index: f64) {
        self.update(|state| model::move_pane(state, id, to_index));
    }

    /// Make a pane the focused one; `None` focuses none.
    pub fn set_focused_pane(&self, id: Option<&str>) {
        self.update(|state| model::set_focused_pane(state, id));
    }

    /// Move an open document into a new pane beside `from` (or the focused
    /// pane), and focus it; at the end when neither is a pane. Answers the
    /// new pane's id.
    pub fn split_pane(&self, document_id: &str, from: Option<&str>) -> PaneId {
        let state = self.state();
        // `order.indexOf(options?.from ?? focusedPaneId ?? '')`
        let beside = from.or(state.focused_pane_id.as_deref()).unwrap_or("");
        let index = state.order.iter().position(|id| id == beside).map(|at| (at + 1) as f64);
        self.create_pane_with(&[document_id.to_string()], index)
    }

    // ── tabs ──

    /// Show another of the pane's documents.
    pub fn set_active_document(&self, pane_id: &str, document_id: Option<&str>) {
        self.update(|state| model::set_active_document(state, pane_id, document_id));
    }

    /// Add a document to a pane's tabs; it leaves the pane it was in.
    pub fn add_document(&self, pane_id: &str, document_id: &str, index: Option<f64>) {
        self.update(|state| model::add_document(state, pane_id, document_id, index));
    }

    /// Take a document out of a pane, without closing it.
    pub fn remove_document(&self, pane_id: &str, document_id: &str) {
        self.update(|state| model::remove_document(state, pane_id, document_id));
    }

    /// Move a tab to another place inside its pane.
    pub fn move_document_within(&self, pane_id: &str, document_id: &str, to_index: f64) {
        self.update(|state| model::move_document_within(state, pane_id, document_id, to_index));
    }

    /// Move a tab from one pane to another.
    pub fn move_document_between(
        &self,
        from_pane_id: &str,
        to_pane_id: &str,
        document_id: &str,
        to_index: Option<f64>,
    ) {
        self.update(|state| {
            model::move_document_between(state, from_pane_id, to_pane_id, document_id, to_index)
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

    /// Keep panes matching the document registry: newly opened documents
    /// land in the focused pane (the first open creates a default pane),
    /// and closed documents leave whatever pane held them.
    fn connect(&self) -> Result<(), Error> {
        let documents = self.0.cx.get(&DOCUMENTS)?;
        let this = self.clone();
        let sync = move |open: &[String], previous: &[String]| {
            // A document that got its real id while opening is in the same
            // place in the order.
            if open.len() == previous.len() {
                for (id, previous_id) in open.iter().zip(previous) {
                    if id != previous_id && !open.contains(previous_id) && !previous.contains(id) {
                        this.update(|state| model::rename_document(state, previous_id, id));
                    }
                }
            }
            let prefer = this.state().focused_pane_id.clone();
            this.update(|state| model::reconcile(state, open, prefer.as_deref()));
        };
        let initial = documents.get_order();
        sync(&initial, &[]);
        // `ctx.watch(() => documents.getOrder(), sync, sameIds)`.
        let previous = RefCell::new(initial);
        let unsubscribe = self.0.cx.subscribe(Rc::new(move || {
            let next = documents.get_order();
            if *previous.borrow() == next {
                return Ok(());
            }
            let prior = previous.replace(next.clone());
            sync(&next, &prior);
            Ok(())
        }));
        self.0.cx.listen(unsubscribe);
        Ok(())
    }
}

/// The `ctx.state.onChange` listener: panes created (in the new order),
/// panes removed (in the old order), the focus, then each document whose
/// pane changed, documents enumerated from the old panes then the new ones.
fn announce(
    previous: &ViewManagerState,
    next: &ViewManagerState,
    pane_created: &EventHookSource<PaneEvent>,
    pane_removed: &EventHookSource<PaneEvent>,
    focus_changed: &EventHookSource<PaneFocusChangedEvent>,
    document_moved: &EventHookSource<DocumentMovedEvent>,
) {
    for id in next.order.iter() {
        if !previous.panes.contains_key(id) {
            pane_created.emit(&PaneEvent { pane_id: id.clone() });
        }
    }
    for id in previous.order.iter() {
        if !next.panes.contains_key(id) {
            pane_removed.emit(&PaneEvent { pane_id: id.clone() });
        }
    }
    if previous.focused_pane_id != next.focused_pane_id {
        focus_changed.emit(&PaneFocusChangedEvent { pane_id: next.focused_pane_id.clone() });
    }
    if Rc::ptr_eq(&previous.panes, &next.panes) {
        return;
    }
    let mut documents: Vec<&str> = Vec::new();
    for pane in previous.panes.values().chain(next.panes.values()) {
        for document_id in &pane.document_ids {
            if !documents.contains(&document_id.as_str()) {
                documents.push(document_id);
            }
        }
    }
    for document_id in documents {
        let from_pane_id = model::pane_of_document(previous, document_id);
        let to_pane_id = model::pane_of_document(next, document_id);
        if from_pane_id != to_pane_id {
            document_moved.emit(&DocumentMovedEvent {
                document_id: document_id.to_string(),
                from_pane_id,
                to_pane_id,
            });
        }
    }
}

/// `createViewManagerController(ctx)`: the api and its `connect`.
pub fn create_view_manager_controller(
    cx: Controller<ViewManagerState, Value>,
) -> (ViewManagerCapability, ConnectFn) {
    let pane_created = cx.event_source::<PaneEvent>();
    let pane_removed = cx.event_source::<PaneEvent>();
    let focus_changed = cx.event_source::<PaneFocusChangedEvent>();
    let document_moved = cx.event_source::<DocumentMovedEvent>();
    {
        let (pane_created, pane_removed, focus_changed, document_moved) = (
            pane_created.clone(),
            pane_removed.clone(),
            focus_changed.clone(),
            document_moved.clone(),
        );
        cx.on_state_change(move |next, previous| {
            announce(
                &previous,
                &next,
                &pane_created,
                &pane_removed,
                &focus_changed,
                &document_moved,
            );
            Ok(())
        });
    }
    let api = ViewManagerCapability(Rc::new(Inner {
        cx,
        pane_created,
        pane_removed,
        focus_changed,
        document_moved,
        memos: RefCell::default(),
    }));
    let connecting = api.clone();
    (api, Rc::new(move || connecting.connect()))
}
