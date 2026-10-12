//! `store.ts`: one state tree (`core` plus keyed plugin slices) and two
//! channels, state changed and action dispatched.
//!
//! Change notification is non-re-entrant: a listener that dispatches finishes
//! the current pass and triggers another, so listener order stays
//! deterministic. Listener lists iterate the way a JavaScript `Set` does
//! during `forEach`: a listener added mid-pass is visited in that pass, one
//! removed mid-pass is skipped.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use serde_json::Value;

use crate::action::Action;
use crate::error::Error;
use crate::types::CoreState;

pub type Report = Rc<dyn Fn(&Error)>;
pub type SliceState = Rc<dyn Any>;
/// A reducer: `None` is the TypeScript `return state` (no change, no
/// notification); `Some` is a new state object.
pub type SliceReducer = Rc<dyn Fn(&SliceState, &dyn Action) -> Option<SliceState>>;
pub type SliceJson = Rc<dyn Fn(&dyn Any) -> Value>;
/// A slice's JSON text written directly, byte for byte what its
/// [`SliceJson`] gives through `to_string()`, for a slice that can build it
/// cheaper than whole (a large map of shared entries can reuse each entry's
/// text). `None` falls back to the [`SliceJson`].
pub type SliceText = Rc<dyn Fn(&dyn Any) -> Option<String>>;
/// A state-change listener. An `Err` is a throwing listener: reported, and
/// the pass continues.
pub type ChangeListener = dyn Fn() -> Result<(), Error>;
pub type ActionListener = dyn Fn(&dyn Action) -> Result<(), Error>;

/// An action listener, and the one `type` it hears when it has one
/// (`ctx.onAction(type, handler)`).
pub(crate) struct ActionEntry {
    kind: Option<String>,
    listener: Rc<ActionListener>,
}

/// A `Set`-like listener list. Entries are tombstoned rather than removed
/// while a pass is walking it.
pub(crate) struct Listeners<F: ?Sized> {
    entries: RefCell<Vec<Option<(u64, Rc<F>)>>>,
    next: Cell<u64>,
    walking: Cell<u32>,
}

impl<F: ?Sized> Default for Listeners<F> {
    fn default() -> Self {
        Listeners { entries: RefCell::new(Vec::new()), next: Cell::new(0), walking: Cell::new(0) }
    }
}

impl<F: ?Sized> Listeners<F> {
    pub(crate) fn add(&self, listener: Rc<F>) -> u64 {
        let id = self.next.get();
        self.next.set(id + 1);
        self.entries.borrow_mut().push(Some((id, listener)));
        id
    }

    pub(crate) fn remove(&self, id: u64) {
        let mut entries = self.entries.borrow_mut();
        if let Some(slot) = entries.iter_mut().find(|e| matches!(e, Some((i, _)) if *i == id)) {
            *slot = None;
        }
        if self.walking.get() == 0 {
            entries.retain(Option::is_some);
        }
    }

    pub(crate) fn clear(&self) {
        let mut entries = self.entries.borrow_mut();
        if self.walking.get() == 0 {
            entries.clear();
        } else {
            entries.iter_mut().for_each(|e| *e = None);
        }
    }

    /// Visit every live listener, including ones added during the walk.
    pub(crate) fn walk(&self, mut visit: impl FnMut(&Rc<F>)) {
        self.walking.set(self.walking.get() + 1);
        let mut index = 0;
        loop {
            let listener = {
                let entries = self.entries.borrow();
                if index >= entries.len() {
                    break;
                }
                entries[index].as_ref().map(|(_, l)| l.clone())
            };
            index += 1;
            if let Some(listener) = listener {
                visit(&listener);
            }
        }
        self.walking.set(self.walking.get() - 1);
        if self.walking.get() == 0 {
            self.entries.borrow_mut().retain(Option::is_some);
        }
    }
}

/// A plugin instance's authority over its state slice. Unique per
/// instance: closing and reopening a document id yields a new lease, and the
/// old one is revoked at the start of close, so a callback the old instance
/// retained can never reach the new instance's state. `commit` drops the
/// action and answers false once revoked; `read` keeps answering the last
/// state.
#[derive(Clone)]
pub struct SliceLease(Rc<LeaseInner>);

struct LeaseInner {
    key: String,
    instance_id: String,
    live: Cell<bool>,
    state: RefCell<SliceState>,
    reducer: SliceReducer,
    to_json: SliceJson,
    to_text: Option<SliceText>,
    store: Weak<Store>,
    /// This slice's JSON text, for the state it was built from. The state is
    /// an immutable `Rc` a reducer replaces, so pointer identity is enough;
    /// holding the `Rc` keeps its address from being reused.
    text: RefCell<Option<(SliceState, Rc<str>)>>,
    /// Moves when this slice's state is replaced, from a counter the store
    /// shares across slices, so a lease that replaces another under the same
    /// key never repeats its revision.
    revision: Cell<u64>,
}

impl SliceLease {
    pub fn key(&self) -> &str {
        &self.0.key
    }

    pub fn instance_id(&self) -> &str {
        &self.0.instance_id
    }

    pub fn live(&self) -> bool {
        self.0.live.get()
    }

    pub fn read(&self) -> SliceState {
        self.0.state.borrow().clone()
    }

    /// Run the reducer; notify on a new state; tell action listeners either
    /// way. False, and nothing happens, once the lease is revoked.
    pub fn commit(&self, action: &dyn Action) -> bool {
        if !self.0.live.get() {
            return false;
        }
        let state = self.read();
        let next = (self.0.reducer)(&state, action);
        let store = self.0.store.upgrade();
        if let Some(next) = next {
            *self.0.state.borrow_mut() = next;
            if let Some(store) = &store {
                self.0.revision.set(store.next_slice_revision());
                store.bump();
                store.emit_change();
            }
        }
        if let Some(store) = &store {
            store.emit_action(action);
        }
        true
    }

    pub fn revoke(&self) {
        if !self.0.live.get() {
            return;
        }
        self.0.live.set(false);
        let Some(store) = self.0.store.upgrade() else { return };
        let current =
            store.leases.borrow().get(&self.0.key).is_some_and(|l| Rc::ptr_eq(&l.0, &self.0));
        if current {
            store.leases.borrow_mut().remove(&self.0.key);
            store.bump();
            store.emit_change();
        }
    }

    fn json(&self) -> Value {
        (self.0.to_json)(self.0.state.borrow().as_ref())
    }

    /// [`json`](Self::json) as text, rebuilt only when the state changed.
    fn text(&self) -> Rc<str> {
        let state = self.read();
        if let Some((cached, text)) = &*self.0.text.borrow() {
            if Rc::ptr_eq(cached, &state) {
                return text.clone();
            }
        }
        let written = self.0.to_text.as_ref().and_then(|to_text| to_text(state.as_ref()));
        let text: Rc<str> = match written {
            Some(text) => text.into(),
            None => (self.0.to_json)(state.as_ref()).to_string().into(),
        };
        *self.0.text.borrow_mut() = Some((state, text.clone()));
        text
    }
}

pub struct Store {
    this: Weak<Store>,
    core: RefCell<Rc<CoreState>>,
    leases: RefCell<BTreeMap<String, SliceLease>>,
    lease_counter: Cell<u64>,
    change: Listeners<ChangeListener>,
    action: Listeners<ActionEntry>,
    emitting: Cell<bool>,
    pending: Cell<bool>,
    report: Report,
    /// Moves on every change to core or to the set of slices or their
    /// states, before listeners hear of it.
    revision: Cell<u64>,
    /// Moves on every change to core only: documents opening, closing,
    /// becoming ready or active, which is what decides the capabilities a
    /// caller can reach.
    core_revision: Cell<u64>,
    /// `state_json` as text for `revision`, and core's text for the core
    /// `Rc` it was built from.
    state_text: RefCell<Option<(u64, Rc<str>)>>,
    core_text: RefCell<Option<(Rc<CoreState>, Rc<str>)>>,
    slice_revisions: Cell<u64>,
}

impl Store {
    pub fn new(report: Report) -> Rc<Store> {
        Rc::new_cyclic(|this| Store {
            this: this.clone(),
            core: RefCell::new(Rc::new(CoreState::default())),
            leases: RefCell::new(BTreeMap::new()),
            lease_counter: Cell::new(0),
            change: Listeners::default(),
            action: Listeners::default(),
            emitting: Cell::new(false),
            pending: Cell::new(false),
            report,
            revision: Cell::new(0),
            core_revision: Cell::new(0),
            state_text: RefCell::new(None),
            core_text: RefCell::new(None),
            slice_revisions: Cell::new(0),
        })
    }

    fn next_slice_revision(&self) -> u64 {
        self.slice_revisions.set(self.slice_revisions.get() + 1);
        self.slice_revisions.get()
    }

    fn bump(&self) {
        self.revision.set(self.revision.get() + 1);
    }

    /// The store's revision: equal revisions mean an equal `state_json`.
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// Core's revision: equal revisions mean an equal core.
    pub fn core_revision(&self) -> u64 {
        self.core_revision.get()
    }

    fn guarded(&self, run: impl FnOnce() -> Result<(), Error>) {
        if let Err(error) = run() {
            (self.report)(&error);
        }
    }

    fn emit_change(&self) {
        if self.emitting.get() {
            self.pending.set(true);
            return;
        }
        self.emitting.set(true);
        loop {
            self.pending.set(false);
            self.change.walk(|listener| self.guarded(|| listener()));
            if !self.pending.get() {
                break;
            }
        }
        self.emitting.set(false);
    }

    /// The action's `type` is read once per pass, and only if a typed
    /// listener asks: reading it serializes the whole action.
    fn emit_action(&self, action: &dyn Action) {
        let mut kind: Option<String> = None;
        self.action.walk(|entry| {
            if let Some(wanted) = &entry.kind
                && kind.get_or_insert_with(|| action.action_type()) != wanted
            {
                return;
            }
            self.guarded(|| (entry.listener)(action))
        });
    }

    /// Report an error a listener raised, keeping the pass going. Listeners
    /// in this crate return `()`; a fallible one calls this.
    pub fn report(&self, error: &Error) {
        (self.report)(error);
    }

    /// Acquire the lease for a slice, revoking any earlier lease under the
    /// same key. `instance_id` defaults to `<key>#<n>`, counted per store.
    pub fn lease(
        &self,
        key: &str,
        reducer: SliceReducer,
        initial: SliceState,
        to_json: SliceJson,
        instance_id: Option<String>,
    ) -> SliceLease {
        self.lease_with_text(key, reducer, initial, to_json, None, instance_id)
    }

    /// [`lease`](Self::lease), with a [`SliceText`] for the slice's text.
    pub fn lease_with_text(
        &self,
        key: &str,
        reducer: SliceReducer,
        initial: SliceState,
        to_json: SliceJson,
        to_text: Option<SliceText>,
        instance_id: Option<String>,
    ) -> SliceLease {
        let instance_id = instance_id.unwrap_or_else(|| {
            self.lease_counter.set(self.lease_counter.get() + 1);
            format!("{key}#{}", self.lease_counter.get())
        });
        let prior = self.leases.borrow().get(key).cloned();
        if let Some(prior) = prior {
            prior.revoke();
        }
        let lease = SliceLease(Rc::new(LeaseInner {
            key: key.to_string(),
            instance_id,
            live: Cell::new(true),
            state: RefCell::new(initial),
            reducer,
            to_json,
            to_text,
            store: self.this.clone(),
            text: RefCell::new(None),
            revision: Cell::new(self.next_slice_revision()),
        }));
        self.leases.borrow_mut().insert(key.to_string(), lease.clone());
        self.bump();
        lease
    }

    /// `registerSlice`, kept for the transition: a lease nobody holds.
    pub fn register_slice(
        &self,
        key: &str,
        reducer: SliceReducer,
        initial: SliceState,
        to_json: SliceJson,
    ) {
        self.lease(key, reducer, initial, to_json, None);
    }

    /// `removeSlice`, kept for the transition: revoke the current lease.
    pub fn remove_slice(&self, key: &str) {
        let lease = self.leases.borrow().get(key).cloned();
        if let Some(lease) = lease {
            lease.revoke();
        }
    }

    pub fn get_slice(&self, key: &str) -> Option<SliceState> {
        self.leases.borrow().get(key).map(SliceLease::read)
    }

    pub fn has_slice(&self, key: &str) -> bool {
        self.leases.borrow().contains_key(key)
    }

    pub fn core(&self) -> Rc<CoreState> {
        self.core.borrow().clone()
    }

    /// `getState()`: the whole tree as canonical JSON.
    pub fn state_json(&self) -> Value {
        let leases = self.leases.borrow();
        let mut plugins = serde_json::Map::new();
        for (key, lease) in leases.iter() {
            plugins.insert(key.clone(), lease.json());
        }
        serde_json::json!({ "core": crate::js::to_value(&*self.core()), "plugins": plugins })
    }

    /// [`state_json`](Self::state_json) as text, byte for byte what
    /// `state_json().to_string()` gives, built from cached per-slice text and
    /// cached whole while the revision holds.
    pub fn state_text(&self) -> Rc<str> {
        let revision = self.revision.get();
        if let Some((cached, text)) = &*self.state_text.borrow() {
            if *cached == revision {
                return text.clone();
            }
        }
        let core_text = self.core_text();
        let leases: Vec<SliceLease> = self.leases.borrow().values().cloned().collect();
        let mut out = String::with_capacity(core_text.len() + 64);
        out.push_str("{\"core\":");
        out.push_str(&core_text);
        out.push_str(",\"plugins\":{");
        for (i, lease) in leases.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&serde_json::to_string(lease.key()).expect("a string key serializes"));
            out.push(':');
            out.push_str(&lease.text());
        }
        out.push_str("}}");
        let text: Rc<str> = out.into();
        *self.state_text.borrow_mut() = Some((revision, text.clone()));
        text
    }

    /// Core as canonical JSON text, rebuilt only when core changed.
    pub fn core_text(&self) -> Rc<str> {
        let core = self.core();
        if let Some((cached, text)) = &*self.core_text.borrow() {
            if Rc::ptr_eq(cached, &core) {
                return text.clone();
            }
        }
        let text: Rc<str> = crate::js::to_value(&*core).to_string().into();
        *self.core_text.borrow_mut() = Some((core, text.clone()));
        text
    }

    /// One slice's canonical JSON text, rebuilt only when it changed.
    pub fn slice_text(&self, key: &str) -> Option<Rc<str>> {
        let lease = self.leases.borrow().get(key).cloned();
        lease.map(|lease| lease.text())
    }

    /// `{"core": n, "plugins": {"<key>": n, ...}}`: a revision per part of
    /// [`state_json`](Self::state_json), so a reader can refetch only the
    /// parts that moved.
    pub fn revisions_json(&self) -> String {
        let leases = self.leases.borrow();
        let mut plugins = serde_json::Map::new();
        for (key, lease) in leases.iter() {
            plugins.insert(key.clone(), Value::from(lease.0.revision.get()));
        }
        serde_json::json!({ "core": self.core_revision(), "plugins": plugins }).to_string()
    }

    /// One slice as canonical JSON.
    pub fn slice_json(&self, key: &str) -> Option<Value> {
        self.leases.borrow().get(key).map(SliceLease::json)
    }

    /// `dispatchTo`, kept for the transition: commit through the current lease.
    pub fn dispatch_to(&self, key: &str, action: &dyn Action) {
        let lease = self.leases.borrow().get(key).cloned();
        if let Some(lease) = lease {
            lease.commit(action);
        }
    }

    pub fn set_core(&self, patch: impl FnOnce(&mut CoreState), action: &dyn Action) {
        let mut next = (*self.core()).clone();
        patch(&mut next);
        *self.core.borrow_mut() = Rc::new(next);
        self.core_revision.set(self.core_revision.get() + 1);
        self.bump();
        self.emit_change();
        self.emit_action(action);
    }

    pub fn subscribe(self: &Rc<Self>, listener: Rc<ChangeListener>) -> Unsubscribe {
        let id = self.change.add(listener);
        let store = Rc::downgrade(self);
        Unsubscribe::new(move || {
            if let Some(store) = store.upgrade() {
                store.change.remove(id);
            }
        })
    }

    pub fn subscribe_action(self: &Rc<Self>, listener: Rc<ActionListener>) -> Unsubscribe {
        self.add_action_listener(None, listener)
    }

    /// [`subscribe_action`](Self::subscribe_action) for one action `type`.
    pub fn subscribe_action_of(
        self: &Rc<Self>,
        kind: &str,
        listener: Rc<ActionListener>,
    ) -> Unsubscribe {
        self.add_action_listener(Some(kind.to_string()), listener)
    }

    fn add_action_listener(
        self: &Rc<Self>,
        kind: Option<String>,
        listener: Rc<ActionListener>,
    ) -> Unsubscribe {
        let id = self.action.add(Rc::new(ActionEntry { kind, listener }));
        let store = Rc::downgrade(self);
        Unsubscribe::new(move || {
            if let Some(store) = store.upgrade() {
                store.action.remove(id);
            }
        })
    }

    /// Kernel-destroy teardown: reset core, revoke every lease, drop every
    /// listener.
    pub fn destroy(&self) {
        *self.core.borrow_mut() = Rc::new(CoreState::default());
        self.core_revision.set(self.core_revision.get() + 1);
        self.bump();
        let leases: Vec<SliceLease> = self.leases.borrow().values().cloned().collect();
        for lease in leases {
            lease.revoke();
        }
        self.leases.borrow_mut().clear();
        self.bump();
        self.change.clear();
        self.action.clear();
    }

    /// Run a listener body that may fail, reporting instead of unwinding.
    pub fn isolate(&self, run: impl FnOnce() -> Result<(), Error>) {
        self.guarded(run);
    }
}

/// `() => void` returned by a subscription. Calling it twice is harmless.
#[derive(Clone)]
pub struct Unsubscribe(Rc<RefCell<Option<Box<dyn FnOnce()>>>>);

impl Unsubscribe {
    pub fn new(run: impl FnOnce() + 'static) -> Self {
        Unsubscribe(Rc::new(RefCell::new(Some(Box::new(run)))))
    }

    pub fn noop() -> Self {
        Unsubscribe(Rc::new(RefCell::new(None)))
    }

    pub fn call(&self) {
        let run = self.0.borrow_mut().take();
        if let Some(run) = run {
            run();
        }
    }
}

/// A plugin's remover (`register_command`, `register_locale`) as an
/// `Unsubscribe`, so both kinds of teardown sit in one list. Like any
/// `Unsubscribe` it runs the remover once; later calls do nothing.
impl From<Rc<dyn Fn()>> for Unsubscribe {
    fn from(remove: Rc<dyn Fn()>) -> Self {
        Unsubscribe::new(move || remove())
    }
}
