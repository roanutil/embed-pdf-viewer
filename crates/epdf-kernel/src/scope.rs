//! `scope.ts`: the ownership half of the lifecycle model.
//!
//! Every acquisition registers its release into the owning scope at the
//! acquisition site. `dispose()` unwinds LIFO, awaits asynchronous
//! teardowns, and is idempotent. A teardown registered on a scope that is
//! already disposing runs once that disposal finishes, instead of being
//! dropped.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::Error;
use crate::rt::{self, Promise};
use crate::store::Report;

/// What a teardown did: finished (the TypeScript returned `undefined`),
/// started something to await, or threw.
pub type Teardown = Box<dyn FnOnce() -> Result<Option<Promise<()>>, Error>>;

/// `CancelledError`: the close won, not a failure.
pub fn cancelled(what: &str) -> Error {
    Error::named("CancelledError", format!("[kernel] cancelled: {what}"))
}

pub fn is_cancelled(error: &Error) -> bool {
    error.name == "CancelledError"
}

struct Inner {
    teardowns: Vec<Teardown>,
    disposal: Option<Promise<()>>,
}

#[derive(Clone)]
pub struct Scope {
    inner: Rc<RefCell<Inner>>,
    report: Report,
}

impl Scope {
    pub fn new(report: Report) -> Self {
        Scope {
            inner: Rc::new(RefCell::new(Inner { teardowns: Vec::new(), disposal: None })),
            report,
        }
    }

    pub fn disposed(&self) -> bool {
        self.inner.borrow().disposal.is_some()
    }

    pub fn defer(&self, teardown: Teardown) {
        let disposal = self.inner.borrow().disposal.clone();
        match disposal {
            None => self.inner.borrow_mut().teardowns.push(teardown),
            Some(disposal) => {
                // `disposal.then(() => Promise.resolve().then(teardown).catch(report))`
                let report = self.report.clone();
                rt::spawn(async move {
                    let _ = disposal.await;
                    rt::tick().await;
                    match teardown() {
                        Ok(None) => {}
                        Ok(Some(promise)) => {
                            if let Err(error) = promise.await {
                                report(&error);
                            }
                        }
                        Err(error) => report(&error),
                    }
                });
            }
        }
    }

    /// A synchronous teardown that cannot fail.
    pub fn defer_fn(&self, teardown: impl FnOnce() + 'static) {
        self.defer(Box::new(move || {
            teardown();
            Ok(None)
        }));
    }

    pub fn dispose(&self) -> Promise<()> {
        if let Some(disposal) = self.inner.borrow().disposal.clone() {
            return disposal;
        }
        let inner = self.inner.clone();
        let report = self.report.clone();
        let (disposal, resolver) = Promise::pending();
        // `disposal ??= (async () => ...)()`: the first teardown runs inside
        // the call, BEFORE the assignment, so it still sees an undisposed scope.
        let assign = disposal.clone();
        rt::spawn(async move {
            loop {
                let next = inner.borrow_mut().teardowns.pop();
                let Some(teardown) = next else { break };
                match teardown() {
                    // `await undefined`
                    Ok(None) => rt::tick().await,
                    Ok(Some(promise)) => {
                        if let Err(error) = promise.await {
                            report(&error);
                        }
                    }
                    Err(error) => report(&error),
                }
            }
            resolver.resolve(());
        });
        self.inner.borrow_mut().disposal = Some(assign);
        disposal
    }
}
