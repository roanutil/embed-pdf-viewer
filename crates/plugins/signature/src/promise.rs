//! The JavaScript promise shapes the plugin chains, each costing the turns
//! its TypeScript does.

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};

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
        if let Err(error) = promise.reaction().await {
            on_error(error);
        }
    });
}
