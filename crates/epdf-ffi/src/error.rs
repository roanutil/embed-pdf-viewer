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
