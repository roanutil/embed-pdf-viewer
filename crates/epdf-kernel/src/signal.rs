//! `AbortController` / `AbortSignal`, for the places the TypeScript hands a
//! signal to someone else's code: an open thunk, a fetch.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::error::Error;

struct Inner {
    reason: Option<Error>,
    listeners: Vec<(u64, Rc<dyn Fn(&Error)>)>,
    next: u64,
    /// While `abort` dispatches: the listeners removed since it started,
    /// which it then skips, as the DOM does.
    removed: Option<HashSet<u64>>,
}

#[derive(Clone)]
pub struct AbortSignal(Rc<RefCell<Inner>>);

impl AbortSignal {
    pub fn new() -> Self {
        AbortSignal(Rc::new(RefCell::new(Inner {
            reason: None,
            listeners: Vec::new(),
            next: 0,
            removed: None,
        })))
    }

    /// The same controller.
    pub fn same(&self, other: &AbortSignal) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn aborted(&self) -> bool {
        self.0.borrow().reason.is_some()
    }

    pub fn reason(&self) -> Option<Error> {
        self.0.borrow().reason.clone()
    }

    /// `addEventListener('abort', listener, {once: true})`; the id removes it.
    pub fn on_abort(&self, listener: Rc<dyn Fn(&Error)>) -> u64 {
        let mut inner = self.0.borrow_mut();
        let id = inner.next;
        inner.next += 1;
        inner.listeners.push((id, listener));
        id
    }

    pub fn remove(&self, id: u64) {
        let mut inner = self.0.borrow_mut();
        inner.listeners.retain(|(i, _)| *i != id);
        if let Some(removed) = &mut inner.removed {
            removed.insert(id);
        }
    }

    /// How many abort listeners are registered, for tests that check a
    /// caller removes its listener.
    pub fn listener_count(&self) -> usize {
        self.0.borrow().listeners.len()
    }

    /// `controller.abort(reason)`. A second abort does nothing. As the DOM
    /// dispatches: the listeners registered when the abort starts, less
    /// any an earlier one removed.
    pub fn abort(&self, reason: Error) {
        let listeners = {
            let mut inner = self.0.borrow_mut();
            if inner.reason.is_some() {
                return;
            }
            inner.reason = Some(reason.clone());
            inner.removed = Some(HashSet::new());
            std::mem::take(&mut inner.listeners)
        };
        for (id, listener) in listeners {
            let removed = self.0.borrow().removed.as_ref().is_some_and(|r| r.contains(&id));
            if !removed {
                listener(&reason);
            }
        }
        self.0.borrow_mut().removed = None;
    }
}

impl Default for AbortSignal {
    fn default() -> Self {
        Self::new()
    }
}
