//! `services/errors.ts`: a capability verb. Whatever the engine, a signer
//! port or the validator rejects with reaches the caller as a `PluginError`.

use epdf_kernel::error::to_plugin_error;
use epdf_kernel::rt::Promise;

/// `verb(run)(...args)`: `run`'s promise, its rejection as a `PluginError`
/// of this capability. The verb is an `async` function awaiting `run`.
pub fn verb<T: Clone + 'static>(run: Promise<T>) -> Promise<T> {
    Promise::spawn(async move { run.await.map_err(|error| to_plugin_error("signature", &error)) })
}
