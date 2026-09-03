use std::cell::{Cell, UnsafeCell};

use crate::{OpsError, State};

// If this scaffold ever grows a pthreads wasm build (embedpdf_thread_local_globals
// on for wasm32, per dispatch/mod.rs), this file's `unsafe impl Sync` is no
// longer sound: a second thread could then call `run` while the first is
// still inside it, aliasing `&mut State` for real. Fail the build rather than
// rely on nobody enabling atomics by accident.
#[cfg(target_feature = "atomics")]
compile_error!("inline dispatch is unsound with pthreads; use the threaded dispatch");

pub struct Worker {
    state: UnsafeCell<State>,
    /// True while a `&mut State` borrowed by `run` is live. `run` checks and
    /// sets this before touching `state`, and clears it on every exit path,
    /// including an `Err` return. It is what actually enforces the aliasing
    /// invariant the SAFETY comment below describes; without it, `run` would
    /// just be handing out `&mut *state.get()` from a `&self` with nothing
    /// stopping a second live borrow.
    borrowed: Cell<bool>,
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
// on `threaded`. The `borrowed` flag is what rules it out: `run` refuses to
// hand out a second `&mut State` while one is already live.
unsafe impl Send for Worker {}
unsafe impl Sync for Worker {}

impl Worker {
    pub fn spawn() -> Result<Self, OpsError> {
        Ok(Self { state: UnsafeCell::new(State::new()?), borrowed: Cell::new(false) })
    }

    pub fn run<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T, OpsError> + Send + 'static,
    {
        if self.borrowed.replace(true) {
            return Err(OpsError::Internal {
                message: "epdf_ops::Worker::run was called reentrantly on the inline dispatch \
                          (a job captured or dropped a Document, or otherwise called back into \
                          the worker while already running on it)"
                    .to_string(),
            });
        }
        let result = f(unsafe { &mut *self.state.get() });
        self.borrowed.set(false);
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
