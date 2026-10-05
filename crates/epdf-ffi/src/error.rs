//! The error every export returns.

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ViewerError {
    #[error("{name}: {detail}")]
    Kernel { name: String, code: Option<String>, detail: String },
    /// The kernel thread is gone, or the handle's value was released.
    #[error("closed")]
    Closed,
}

impl From<epdf_kernel::Error> for ViewerError {
    fn from(error: epdf_kernel::Error) -> Self {
        ViewerError::Kernel {
            code: error.code().map(String::from),
            name: error.name,
            detail: error.message,
        }
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
