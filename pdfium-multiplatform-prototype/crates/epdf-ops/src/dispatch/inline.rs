use std::any::Any;
use std::cell::UnsafeCell;
use std::panic::{self, AssertUnwindSafe};

use crate::{OpsError, State};

use super::BorrowFlag;

/// Extracts a human-readable message from a `catch_unwind` payload the same
/// way `threaded::panic_message` does.
fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "worker job panicked with a non-string payload".to_string()
    }
}

// If this scaffold ever grows a pthreads wasm build (embedpdf_thread_local_globals
// on for wasm32, per dispatch/mod.rs), this file's `unsafe impl Sync` is no
// longer sound: a second thread could then call `run` while the first is
// still inside it, aliasing `&mut State` for real. Fail the build rather than
// rely on nobody enabling atomics by accident.
#[cfg(target_feature = "atomics")]
compile_error!("inline dispatch is unsound with pthreads; use the threaded dispatch");

pub struct Worker {
    state: UnsafeCell<State>,
    /// Set while a `&mut State` borrowed by `run` is live. `run` claims it
    /// with `try_borrow` before touching `state`, and the resulting
    /// `BorrowGuard`'s `Drop` clears it on every exit path, including a
    /// panic inside `f` unwinding past the point `run` catches it. It is
    /// what actually enforces the aliasing invariant the SAFETY comment
    /// below describes; without it, `run` would just be handing out
    /// `&mut *state.get()` from a `&self` with nothing stopping a second
    /// live borrow.
    borrowed: BorrowFlag,
}

// SAFETY: this file compiles only for wasm32-unknown-emscripten, which is
// single-threaded, and each wasm instance holds its own PDFium globals in its
// own linear memory, which is why build-target.sh leaves
// embedpdf_thread_local_globals off for wasm32. There is no second thread to
// race against, which is what justifies `Send` and the cross-thread half of
// `Sync` (a `&Worker` can be shared with, or a `Worker` moved to, a context
// that never actually runs concurrently with this one).
//
// That says nothing about aliasing, which is the other half of what `Sync`
// promises and the real hazard here: `run` hands out `&mut *state.get()`
// from a `&self`, so two calls to `run` overlapping on this one thread (a
// job that, directly or by dropping a captured `epdf_ops::Document`,
// re-enters `run` before the outer call returns) would produce two live
// `&mut State` at once. That is undefined behaviour even with a single
// thread involved; it is not merely the deadlock the same reentrancy causes
// on `threaded`. The `borrowed` guard is what rules it out: `run` refuses to
// hand out a second `&mut State` while one is already live, and its `Drop`
// releases it even if `f` panics, so a panic cannot leave it stuck set. (`run`
// also wraps `f` in `catch_unwind`, matching `threaded`, so in the ordinary
// case the panic never gets as far as unwinding through the guard at all.)
// That `Drop`-releases-on-unwind property is load-bearing on this target, and
// it did not used to be. build/link-wasm.sh once built epdf-wasm with
// `-C panic=abort` to satisfy libembedpdf.a's prebuilt linkage, and under that
// strategy nothing unwound, `catch_unwind` could not catch anything, and the
// guard's `Drop` was unexercised: a panic took the whole instance with it, so
// there was no surviving `Worker` to observe a stuck flag. f1a48f51 dropped
// that `RUSTFLAGS` when the fork gained wasm32-eh, and the em++ link now
// passes `-fwasm-exceptions`, so a Rust panic really does unwind here and
// `catch_unwind` really does turn it into `OpsError::Internal` for one call
// while the instance stays alive. Which means the flag can be left set by an
// unwind, and `BorrowGuard`'s `Drop` is the only thing that clears it. The
// `#[cfg(target_feature = "atomics")] compile_error!` above, which is what
// actually guards the `Sync` claim, is untouched by any of this.
unsafe impl Send for Worker {}
unsafe impl Sync for Worker {}

impl Worker {
    pub fn spawn() -> Result<Self, OpsError> {
        Ok(Self { state: UnsafeCell::new(State::new()?), borrowed: BorrowFlag::new() })
    }

    pub fn run<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T, OpsError> + Send + 'static,
    {
        let guard = self.borrowed.try_borrow().ok_or_else(|| OpsError::Internal {
            message: "epdf_ops::Worker::run was called reentrantly on the inline dispatch \
                      (a job captured or dropped a Document, or otherwise called back into \
                      the worker while already running on it)"
                .to_string(),
        })?;
        let state = self.state.get();
        // Caught here, matching threaded.rs:83-84, so a panic in `f` becomes
        // an `OpsError::Internal` for this one call instead of propagating
        // out of `run` (and out of a uniffi/wasm boundary that does not want
        // it). `guard`'s `Drop` clears `borrowed` regardless, whether we get
        // here or an unwind skips straight past this function.
        let result = panic::catch_unwind(AssertUnwindSafe(|| f(unsafe { &mut *state })))
            .unwrap_or_else(|payload| Err(OpsError::Internal { message: panic_message(payload) }));
        drop(guard);
        result
    }

    /// Enqueues `f` and returns immediately without waiting for a reply.
    /// There is no queue to defer to on this single-threaded dispatch, so
    /// `f` still runs before this call returns; the point is the signature,
    /// which matches `threaded::Worker::send` so `Drop for Document` does
    /// not need to know which dispatch it is running on.
    pub fn send<F>(&self, f: F)
    where
        F: FnOnce(&mut State) + Send + 'static,
    {
        let _ = self.run(move |state| {
            f(state);
            Ok(())
        });
    }
}
