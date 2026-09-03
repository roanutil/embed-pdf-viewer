use std::cell::UnsafeCell;

use crate::{OpsError, State};

pub struct Worker {
    state: UnsafeCell<State>,
}

// SAFETY: this file compiles only for wasm32-unknown-emscripten, which is
// single-threaded. There is no second thread to race against, and each wasm
// instance holds its own PDFium globals in its own linear memory, which is why
// build-target.sh leaves embedpdf_thread_local_globals off for wasm32. If this
// scaffold ever grows a pthreads wasm build, delete this impl and use `threaded`.
unsafe impl Send for Worker {}
unsafe impl Sync for Worker {}

impl Worker {
    pub fn spawn() -> Result<Self, OpsError> {
        Ok(Self { state: UnsafeCell::new(State::new()?) })
    }

    pub fn run<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T, OpsError> + Send + 'static,
    {
        f(unsafe { &mut *self.state.get() })
    }
}
