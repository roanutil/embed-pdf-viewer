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
use std::rc::Rc;

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

struct Slice {
    state: SliceState,
    reducer: SliceReducer,
    to_json: SliceJson,
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

pub struct Store {
    core: RefCell<Rc<CoreState>>,
    slices: RefCell<BTreeMap<String, Slice>>,
    change: Listeners<ChangeListener>,
    action: Listeners<ActionListener>,
    emitting: Cell<bool>,
    pending: Cell<bool>,
    report: Report,
}

impl Store {
    pub fn new(report: Report) -> Rc<Store> {
        Rc::new(Store {
            core: RefCell::new(Rc::new(CoreState::default())),
            slices: RefCell::new(BTreeMap::new()),
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

    pub fn register_slice(
        &self,
        key: &str,
        reducer: SliceReducer,
        initial: SliceState,
        to_json: SliceJson,
    ) {
        self.slices
            .borrow_mut()
            .insert(key.to_string(), Slice { state: initial, reducer, to_json });
    }

    pub fn remove_slice(&self, key: &str) {
        self.slices.borrow_mut().remove(key);
        self.emit_change();
    }

    pub fn get_slice(&self, key: &str) -> Option<SliceState> {
        self.slices.borrow().get(key).map(|s| s.state.clone())
    }

    pub fn has_slice(&self, key: &str) -> bool {
        self.slices.borrow().contains_key(key)
    }

    pub fn core(&self) -> Rc<CoreState> {
        self.core.borrow().clone()
    }

    /// `getState()`: the whole tree as canonical JSON.
    pub fn state_json(&self) -> Value {
        let slices = self.slices.borrow();
        let mut plugins = serde_json::Map::new();
        for (key, slice) in slices.iter() {
            plugins.insert(key.clone(), (slice.to_json)(slice.state.as_ref()));
        }
        serde_json::json!({ "core": crate::js::to_value(&*self.core()), "plugins": plugins })
    }

    /// One slice as canonical JSON.
    pub fn slice_json(&self, key: &str) -> Option<Value> {
        let slices = self.slices.borrow();
        slices.get(key).map(|slice| (slice.to_json)(slice.state.as_ref()))
    }

    pub fn dispatch_to(&self, key: &str, action: &dyn Action) {
        let (state, reducer) = match self.slices.borrow().get(key) {
            Some(slice) => (slice.state.clone(), slice.reducer.clone()),
            None => return,
        };
        if let Some(next) = reducer(&state, action) {
            if let Some(slice) = self.slices.borrow_mut().get_mut(key) {
                slice.state = next;
            }
            self.emit_change();
        }
        self.emit_action(action);
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

    /// Kernel-destroy teardown: reset core, drop every slice and listener.
    pub fn destroy(&self) {
        *self.core.borrow_mut() = Rc::new(CoreState::default());
        let slices = std::mem::take(&mut *self.slices.borrow_mut());
        drop(slices);
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
