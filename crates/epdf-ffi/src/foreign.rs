//! Answers a foreign implementation sends back later, from any thread, and
//! the error every foreign trait method throws.
//!
//! A foreign trait method runs on the kernel thread and can't block it, so a
//! port whose Rust side wants a `Promise` hands Swift a completion object.
//! That object holds a `ForeignSettle`: the promise's resolver, parked on
//! the kernel thread, and a way to post the answer there.

use std::sync::Mutex;

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
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

impl<T: Clone + 'static> ForeignSettle<T> {
    /// From any thread: reject with `failure`, as `capability`'s error.
    pub fn fail(&self, failure: HostFailure, capability: &'static str) {
        self.settle(move || Err(failure.into_error(capability)));
    }
}

/// Why a foreign implementation failed, as a completion's `fail_with`
/// carries it: the thrown error's name, its kernel code when it has one
/// (`operation-cancelled` for a cancelled task, `permission-denied` from a
/// capability it called), and its message. `None` is `operation-failed`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct HostFailure {
    pub name: String,
    #[uniffi(default = None)]
    pub code: Option<String>,
    pub detail: String,
}

impl HostFailure {
    /// The kernel's error for it: a plugin error of `capability` with the
    /// failure's code, so `toPluginError` passes it through unchanged, under
    /// the failure's own name.
    pub fn into_error(self, capability: &str) -> Error {
        let code = self.code.unwrap_or_else(|| code::OPERATION_FAILED.into());
        let mut error = plugin_error(&code, capability, self.detail, None, None);
        error.name = self.name;
        error
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
