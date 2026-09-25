//! `AbortController` / `AbortSignal`, for the places the TypeScript hands a
//! signal to someone else's code: an open thunk, a fetch.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::Error;

struct Inner {
    reason: Option<Error>,
    listeners: Vec<(u64, Rc<dyn Fn(&Error)>)>,
    next: u64,
}

#[derive(Clone)]
pub struct AbortSignal(Rc<RefCell<Inner>>);

impl AbortSignal {
    pub fn new() -> Self {
        AbortSignal(Rc::new(RefCell::new(Inner { reason: None, listeners: Vec::new(), next: 0 })))
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
        self.0.borrow_mut().listeners.retain(|(i, _)| *i != id);
    }

    /// `controller.abort(reason)`. A second abort does nothing.
    pub fn abort(&self, reason: Error) {
        let listeners = {
            let mut inner = self.0.borrow_mut();
            if inner.reason.is_some() {
                return;
            }
            inner.reason = Some(reason.clone());
            std::mem::take(&mut inner.listeners)
        };
        for (_, listener) in listeners {
            listener(&reason);
        }
    }
}

impl Default for AbortSignal {
    fn default() -> Self {
        Self::new()
    }
}
