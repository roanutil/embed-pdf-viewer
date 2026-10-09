//! The JavaScript promise shapes the plugin chains, each costing the turns
//! its TypeScript does (`crates/plugins/annotation/src/promise.rs` found the
//! rules).

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};

/// `await doc.x(...)` on an engine call: the guarded handle answers an
/// `AbortablePromise`, a `Promise` subclass that `await` adopts as a
/// thenable, one turn more than a plain promise.
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

/// `void promise.catch(onError)`: `on_error` runs one turn after `promise`
/// rejects.
pub fn catch(promise: Promise<impl Clone + 'static>, on_error: impl FnOnce(Error) + 'static) {
    rt::spawn(async move {
        if let Err(error) = promise.await {
            on_error(error);
        }
    });
}
