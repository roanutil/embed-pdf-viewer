//! Experimental: a hand-written C ABI for the search plugin and the render
//! plugin's `render_page`, built only to time against the uniffi bridge
//! (Task 11, `docs/research/swift-bridge-benchmarks.md`). Nothing ships it.
//!
//! `include/epdf_manual.h` is the contract, written by hand; `tests/layout.rs`
//! holds every `#[repr(C)]` struct here to the sizes and offsets the header
//! asserts. The ownership rules are the header's: a `result` pointer lives
//! for the callback only, except an `EpdfHits` or `EpdfPixels`, whose
//! `owner` the caller frees.
//!
//! Callbacks run on the kernel thread. When the viewer is gone, a call
//! answers `EPDF_ERROR_CLOSED` instead, possibly on the calling thread
//! before the call returns.

pub mod bench;
pub mod registry;
pub mod render;
pub mod search;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CString, c_char, c_void};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use epdf_ffi::{KernelPost, ViewerError};
use epdf_kernel::{Error, Promise};

pub use bench::{epdf_bench_noop, epdf_bench_noop_hop};
pub use render::{
    EPDF_PIXEL_FORMAT_RGBA8, EpdfPixels, EpdfRender, epdf_pixels_free, epdf_render_free,
    epdf_render_open, epdf_render_page,
};
pub use search::*;

pub const EPDF_ERROR_OK: i32 = 0;
pub const EPDF_ERROR_KERNEL: i32 = 1;
pub const EPDF_ERROR_CLOSED: i32 = 2;

/// `kind` 0 is success, and then both strings are null. Free a non-ok
/// error with `epdf_error_free`.
#[repr(C)]
pub struct EpdfError {
    pub kind: i32,
    pub code: *mut c_char,
    pub detail: *mut c_char,
}

fn c_string(s: &str) -> *mut c_char {
    // An interior NUL cannot cross; cut the string there.
    let s = s.split('\0').next().unwrap_or_default();
    CString::new(s).unwrap_or_default().into_raw()
}

impl EpdfError {
    pub fn ok() -> Self {
        EpdfError { kind: EPDF_ERROR_OK, code: std::ptr::null_mut(), detail: std::ptr::null_mut() }
    }

    pub fn closed() -> Self {
        EpdfError {
            kind: EPDF_ERROR_CLOSED,
            code: c_string("instance-closed"),
            detail: c_string("closed"),
        }
    }
}

impl From<ViewerError> for EpdfError {
    fn from(error: ViewerError) -> Self {
        match error {
            ViewerError::Kernel { name, code, detail } => EpdfError {
                kind: EPDF_ERROR_KERNEL,
                code: code.as_deref().map_or(std::ptr::null_mut(), c_string),
                detail: c_string(&format!("{name}: {detail}")),
            },
            ViewerError::Closed => EpdfError::closed(),
        }
    }
}

impl From<Error> for EpdfError {
    fn from(error: Error) -> Self {
        ViewerError::from(error).into()
    }
}

/// # Safety
/// `error` came from a callback and is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_error_free(error: EpdfError) {
    for s in [error.code, error.detail] {
        if !s.is_null() {
            drop(unsafe { CString::from_raw(s) });
        }
    }
}

/// `result` is valid for the duration of the call only.
pub type EpdfComplete = extern "C" fn(ctx: *mut c_void, error: EpdfError, result: *const c_void);

/// A pointer moved to the kernel thread. The caller vouches for it.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct SendPtr<T>(pub *mut T);
unsafe impl<T> Send for SendPtr<T> {}

/// One call's callback, run exactly once: with the answer, or, if the call
/// is dropped unanswered (no kernel thread, a released handle), with
/// `EPDF_ERROR_CLOSED`.
pub(crate) struct Completion {
    ctx: SendPtr<c_void>,
    done: Option<EpdfComplete>,
}

impl Completion {
    pub(crate) fn new(ctx: *mut c_void, done: EpdfComplete) -> Self {
        Completion { ctx: SendPtr(ctx), done: Some(done) }
    }

    pub(crate) fn ok<T>(mut self, result: &T) {
        if let Some(done) = self.done.take() {
            done(self.ctx.0, EpdfError::ok(), result as *const T as *const c_void);
        }
    }

    pub(crate) fn ok_ptr(mut self, result: *const c_void) {
        if let Some(done) = self.done.take() {
            done(self.ctx.0, EpdfError::ok(), result);
        }
    }

    pub(crate) fn fail(mut self, error: impl Into<EpdfError>) {
        if let Some(done) = self.done.take() {
            done(self.ctx.0, error.into(), std::ptr::null());
        }
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            done(self.ctx.0, EpdfError::closed(), std::ptr::null());
        }
    }
}

/// A NUL-terminated UTF-8 argument, or `invalid-input`.
pub(crate) fn arg(s: *const c_char, what: &str) -> Result<String, EpdfError> {
    let invalid = || {
        EpdfError::from(
            Error::named("TypeError", format!("[manual-bridge] {what} is not UTF-8 text"))
                .with("code", "invalid-input"),
        )
    };
    if s.is_null() {
        return Err(invalid());
    }
    unsafe { std::ffi::CStr::from_ptr(s) }.to_str().map(String::from).map_err(|_| invalid())
}

// ── cancellable calls ─────────────────────────────────────────────────────────
//
// A promise-returning export answers a call id. The id maps to the kernel
// thread's post (any thread) and to the promise's abort (kernel thread
// only). `epdf_cancel` posts a job that runs the abort, which is what
// dropping a `call_promise` future does on the uniffi side. Jobs run in
// post order, so a cancel always runs after the job that started the call.

fn posts() -> &'static Mutex<HashMap<u64, KernelPost>> {
    static POSTS: OnceLock<Mutex<HashMap<u64, KernelPost>>> = OnceLock::new();
    POSTS.get_or_init(Default::default)
}

thread_local! {
    static ABORTS: RefCell<HashMap<u64, Rc<dyn Fn()>>> = RefCell::new(HashMap::new());
}

static NEXT_CALL: AtomicU64 = AtomicU64::new(1);

/// A call id, live until the call settles or its job is dropped.
pub(crate) struct Call(u64);

impl Call {
    pub(crate) fn begin(post: &KernelPost) -> Call {
        let id = NEXT_CALL.fetch_add(1, Ordering::Relaxed);
        posts().lock().unwrap().insert(id, post.clone());
        Call(id)
    }

    pub(crate) fn id(&self) -> u64 {
        self.0
    }

    /// On the kernel thread: park `promise` so `epdf_cancel` can abort it,
    /// and answer `done` when it settles.
    pub(crate) fn settle<U: Clone + 'static>(
        self,
        promise: Promise<U>,
        done: impl FnOnce(&Result<U, Error>) + 'static,
    ) {
        let id = self.0;
        let parked = promise.clone();
        ABORTS.with(|a| a.borrow_mut().insert(id, Rc::new(move || parked.abort(None))));
        promise.on_settle(move |result| {
            let _ = ABORTS.try_with(|a| a.borrow_mut().remove(&id));
            drop(self);
            done(result);
        });
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        posts().lock().unwrap().remove(&self.0);
    }
}

/// Abort a call. A call that already settled, or an unknown id, is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_cancel(call_id: u64) {
    let post = posts().lock().unwrap().get(&call_id).cloned();
    if let Some(post) = post {
        post(Box::new(move || {
            let abort = ABORTS.with(|a| a.borrow_mut().remove(&call_id));
            if let Some(abort) = abort {
                abort();
            }
        }));
    }
}
