//! The JavaScript promise shapes the capability chains, each costing the
//! turns its TypeScript does (`crates/plugins/annotation/src/promise.rs`
//! found the rules). An engine call returns an `AbortablePromise`, a
//! `Promise` subclass that `await` adopts as a thenable: it resumes three
//! turns after an already-settled call. An `async` function that returns a
//! promise settles two turns after that promise does.

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};

/// `await doc.x(...)` or `await engine.open(...)` on an engine call.
pub async fn await_engine<T: Clone + 'static>(call: Promise<T>) -> Result<T, Error> {
    if call.is_abortable() {
        // `await` on the subclass takes the adoption turns itself.
        return call.await;
    }
    let adopted = Promise::spawn(async move {
        rt::tick().await;
        call.await
    });
    adopted.await
}

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the reaction.
pub async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
}

/// `promise.finally(onFinally)` with a synchronous `onFinally`: the callback
/// runs one turn after `promise` settles, and the derived promise settles,
/// with `promise`'s outcome, three turns after it (the callback's result is
/// adopted through `Promise.resolve(result).then(() => value)`).
pub fn finally<T: Clone + 'static>(
    promise: Promise<T>,
    on_finally: impl FnOnce() + 'static,
) -> Promise<T> {
    Promise::spawn(async move {
        let outcome = promise.await;
        on_finally();
        rt::tick().await;
        rt::tick().await;
        outcome
    })
}

/// `Promise.all(promises)`: settles one turn after the last element's
/// reaction, or with the first rejection.
pub fn all<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<Vec<T>> {
    Promise::spawn(async move {
        let mut out = Vec::new();
        for promise in promises {
            out.push(promise.await?);
        }
        Ok(out)
    })
}
