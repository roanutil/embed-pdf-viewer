//! The JavaScript promise shapes the plugin chains, each costing the turns
//! its TypeScript does (`crates/plugins/annotation/src/promise.rs` found the
//! rules). An engine call returns an `AbortablePromise`, a `Promise`
//! subclass: `.then` on one reacts one turn after it settles, `await` adopts
//! it as a thenable and resumes three turns after. An `async` function that
//! returns a promise settles two turns after that promise does.

use std::future::Future;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use serde_json::Value;

/// `await doc.x(...)` on an engine call.
pub async fn await_engine(call: Promise<Value>) -> Result<Value, Error> {
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
    promise.await
}

/// `return asyncFn()` from an `async` function whose body runs inline: the
/// callee settles, then its promise is adopted.
pub async fn adopted<F: Future>(body: F) -> F::Output {
    let out = body.await;
    rt::tick().await;
    rt::tick().await;
    out
}
