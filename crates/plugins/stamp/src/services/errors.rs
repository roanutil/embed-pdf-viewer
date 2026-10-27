//! `services/errors.ts`.

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;

/// The capability name every stamp `PluginError` carries.
pub const CAPABILITY: &str = "stamp";

/// `stampError(code, message)`: a `PluginError` raised by the stamp plugin.
pub fn stamp_error(code: &str, message: impl Into<String>) -> Error {
    plugin_error(code, CAPABILITY, message, None, None)
}

/// `notFound(what, id)`.
pub fn not_found(what: &str, id: &str) -> Error {
    stamp_error(code::NOT_FOUND, format!("unknown {what} '{id}'"))
}

/// `permissionDenied(permission, operation)`: a refusal that names the
/// missing permission. The stamp plugin acts on a document it isn't bound
/// to, so it asks that document's checks instead of `ctx.assertAllowed`.
pub fn permission_denied(permission: &str, operation: &str) -> Error {
    stamp_error(code::PERMISSION_DENIED, format!("{operation} requires {permission}"))
        .with("permission", permission)
}

/// `throwIfCancelled(signal)`: stop before the next step, or before
/// anything is kept, once the caller cancelled.
pub fn throw_if_cancelled(signal: Option<&AbortSignal>) -> Result<(), Error> {
    match signal {
        Some(signal) if signal.aborted() => Err(plugin_error(
            code::OPERATION_CANCELLED,
            CAPABILITY,
            "operation cancelled",
            signal.reason_value(),
            None,
        )),
        _ => Ok(()),
    }
}

/// `verb(run)(...args)`: whatever the asset engine, a sibling plugin or a
/// binary source rejects with reaches the caller as a `PluginError`. The
/// `async` wrapper awaits `run`'s promise, so it settles a turn after it.
pub fn verb<T: Clone + 'static>(run: Promise<T>) -> Promise<T> {
    Promise::spawn(async move { run.await.map_err(|error| to_plugin_error(CAPABILITY, &error)) })
}
