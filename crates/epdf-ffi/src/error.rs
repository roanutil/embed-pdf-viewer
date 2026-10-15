//! The error every export returns.

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ViewerError {
    #[error("{name}: {detail}")]
    Kernel { name: String, code: Option<String>, detail: String },
    /// The kernel thread is gone, or the handle's value was released.
    #[error("closed")]
    Closed,
}

/// The one code beyond `PluginErrorCode`: a value this boundary could not
/// decode or convert (a kernel record missing a field, an index past
/// `u32`). It marks a bug in the bindings, never a caller's mistake.
pub const INVALID_STATE: &str = "invalid-state";

/// Every code that crosses is a `PluginErrorCode` (`errors.ts`), the one
/// vocabulary a plugin-level caller sees, or [`INVALID_STATE`]: an engine
/// code (`NotFound`, `DocPasswordIncorrect`) becomes what `toPluginError`
/// makes it, and an error without a code `operation-failed`. `name` and
/// `detail` are kept, so an engine error from the wire still leads its
/// detail with its engine code.
impl From<epdf_kernel::Error> for ViewerError {
    fn from(error: epdf_kernel::Error) -> Self {
        let code = match error.code() {
            Some(INVALID_STATE) => INVALID_STATE,
            _ => epdf_kernel::error::plugin_code_of(&error),
        };
        ViewerError::Kernel { code: Some(code.into()), name: error.name, detail: error.message }
    }
}

impl ViewerError {
    /// What a call answers when its `CancelToken` was cancelled before it
    /// started: the code a cancelled plugin call carries.
    pub fn cancelled() -> Self {
        ViewerError::Kernel {
            name: "AbortError".into(),
            code: Some(epdf_kernel::error::code::OPERATION_CANCELLED.into()),
            detail: "operation cancelled".into(),
        }
    }
}

/// `PluginErrorInfo`: the serializable face of a plugin error, as a plugin
/// keeps it in state or hands it to an event (a failed search, say).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PluginErrorInfo {
    pub code: String,
    pub message: String,
    pub capability: String,
}
