//! `services/errors.ts`.

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};

/// `notFound(what, id)`: a `not-found` PluginError.
pub fn not_found(what: &str, id: &str) -> Error {
    plugin_error(code::NOT_FOUND, "stamp", format!("unknown {what} '{id}'"), None, None)
}
