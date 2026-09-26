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
/// A state-change listener. An `Err` is a throwing listener: reported, and
/// the pass continues.
pub type ChangeListener = dyn Fn() -> Result<(), Error>;
pub type ActionListener = dyn Fn(&dyn Action) -> Result<(), Error>;

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
    store: Weak<Store>,
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
            store.emit_change();
        }
    }

    fn json(&self) -> Value {
        (self.0.to_json)(self.0.state.borrow().as_ref())
    }
}

pub struct Store {
    this: Weak<Store>,
    core: RefCell<Rc<CoreState>>,
    leases: RefCell<BTreeMap<String, SliceLease>>,
    lease_counter: Cell<u64>,
    change: Listeners<ChangeListener>,
    action: Listeners<ActionListener>,
    emitting: Cell<bool>,
    pending: Cell<bool>,
    report: Report,
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
        })
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

    fn emit_action(&self, action: &dyn Action) {
        self.action.walk(|listener| self.guarded(|| listener(action)));
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
            store: self.this.clone(),
        }));
        self.leases.borrow_mut().insert(key.to_string(), lease.clone());
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
        let id = self.action.add(listener);
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
        let leases: Vec<SliceLease> = self.leases.borrow().values().cloned().collect();
        for lease in leases {
            lease.revoke();
        }
        self.leases.borrow_mut().clear();
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
