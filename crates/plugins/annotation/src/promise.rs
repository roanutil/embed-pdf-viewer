//! The JavaScript promise shapes the capability chains, each costing the
//! turns its TypeScript does. Engine calls return an `AbortablePromise` (a
//! `Promise` subclass): `.then` on one reacts one turn after it settles,
//! while `await` adopts it as a thenable and resumes three turns after.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use serde_json::Value;

/// `await doc.x(...)` on an engine call: the resolve-thenable job, the
/// reaction, then the `await`'s own resumption.
pub async fn await_engine(call: Promise<Value>) -> Result<Value, Error> {
    let adopted = Promise::spawn(async move {
        rt::tick().await;
        call.await
    });
    adopted.await
}

/// `promise.then(onFulfilled, onRejected)` whose handlers return nothing:
/// they run one turn after it settles.
pub fn then<T: Clone + 'static>(promise: Promise<T>, handle: impl FnOnce(Result<T, Error>) + 'static) {
    rt::spawn(async move {
        let result = promise.await;
        handle(result);
    });
}

/// `new Promise(...)`-free `async` function: run `body` now to its first
/// pending await and settle with what it returns.
pub fn spawn<T: Clone + 'static>(body: impl Future<Output = Result<T, Error>> + 'static) -> Promise<T> {
    Promise::spawn(body)
}

/// `Promise.all(promises)`: settles one turn after the last element's
/// reaction, or with the first rejection.
pub fn all<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<Vec<T>> {
    if promises.is_empty() {
        return Promise::resolved(Vec::new());
    }
    let (out, resolver) = Promise::pending();
    let slots: Rc<RefCell<Vec<Option<T>>>> = Rc::new(RefCell::new(vec![None; promises.len()]));
    let remaining = Rc::new(Cell::new(promises.len()));
    for (i, promise) in promises.into_iter().enumerate() {
        let (slots, remaining, resolver) = (slots.clone(), remaining.clone(), resolver.clone());
        rt::spawn(async move {
            match promise.await {
                Ok(value) => {
                    slots.borrow_mut()[i] = Some(value);
                    remaining.set(remaining.get() - 1);
                    if remaining.get() == 0 {
                        resolver.resolve(slots.borrow_mut().drain(..).flatten().collect());
                    }
                }
                Err(error) => resolver.reject(error),
            }
        });
    }
    out
}
