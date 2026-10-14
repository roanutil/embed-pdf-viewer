//! Answers a foreign implementation sends back later, from any thread, and
//! the error every foreign trait method throws.
//!
//! A foreign trait method runs on the kernel thread and can't block it, so a
//! port whose Rust side wants a `Promise` hands Swift a completion object.
//! That object holds a `ForeignSettle`: the promise's resolver, parked on
//! the kernel thread, and a way to post the answer there.

use std::sync::Mutex;

use epdf_kernel::Error;
use epdf_kernel::error::code;
use epdf_kernel::rt::Resolver;

use crate::{KernelBound, KernelPost, ViewerError};

pub struct ForeignSettle<T: Clone + 'static> {
    resolver: Mutex<Option<KernelBound<Resolver<T>>>>,
    what: &'static str,
}

impl<T: Clone + 'static> ForeignSettle<T> {
    /// On the kernel thread. `what` names the port in the rejection a
    /// dropped completion produces.
    pub fn new(resolver: Resolver<T>, post: KernelPost, what: &'static str) -> Self {
        ForeignSettle { resolver: Mutex::new(Some(KernelBound::new(resolver, post))), what }
    }

    /// From any thread. The first call wins; later ones do nothing. `make`
    /// runs on the kernel thread, so it may build `!Send` values.
    pub fn settle(&self, make: impl FnOnce() -> Result<T, Error> + Send + 'static) {
        let bound = self.resolver.lock().unwrap().take();
        if let Some(bound) = bound {
            bound.fire(move |resolver| resolver.settle(make()));
        }
    }
}

impl<T: Clone + 'static> Drop for ForeignSettle<T> {
    fn drop(&mut self) {
        let what = self.what;
        self.settle(move || {
            Err(Error::new(format!("{what} was dropped without an answer"))
                .with("code", code::OPERATION_FAILED))
        });
    }
}

/// What a foreign trait method throws. UniFFI needs the conversion from
/// `UnexpectedUniFFICallbackError` for any callback that returns `Result`:
/// a Swift error it has no case for arrives as that, and keeps its text.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum HostError {
    #[error("{detail}")]
    Failed { detail: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for HostError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        HostError::Failed { detail: error.reason }
    }
}

impl From<HostError> for Error {
    fn from(error: HostError) -> Self {
        match error {
            HostError::Failed { detail } => {
                Error::named("HostError", detail).with("code", code::OPERATION_FAILED)
            }
        }
    }
}

/// For a foreign trait method that returns `Result<_, ViewerError>`.
impl From<uniffi::UnexpectedUniFFICallbackError> for ViewerError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        ViewerError::Kernel {
            name: "HostError".into(),
            code: Some(code::OPERATION_FAILED.into()),
            detail: error.reason,
        }
    }
}
