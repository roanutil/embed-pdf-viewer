//! Cancelling a call from the foreign side.
//!
//! UniFFI 0.32's Swift does not cancel the Rust future when the awaiting
//! Swift task is cancelled, so dropping the future (which aborts a
//! `call_promise` promise) never happens on cancellation. A `CancelToken` is
//! the explicit path instead: the foreign wrapper makes one per call, passes
//! it in, and cancels it from its cancellation handler.

use std::sync::{Arc, Mutex};

type Hook = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct State {
    cancelled: bool,
    hook: Option<Hook>,
}

/// Governs one call. Cancelling is idempotent, and cancelling after the call
/// finished does nothing.
#[derive(uniffi::Object)]
pub struct CancelToken {
    state: Mutex<State>,
}

#[uniffi::export]
impl CancelToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(CancelToken { state: Mutex::new(State::default()) })
    }

    pub fn cancel(&self) {
        let hook = {
            let mut state = self.state.lock().unwrap();
            if state.cancelled {
                return;
            }
            state.cancelled = true;
            state.hook.take()
        };
        if let Some(hook) = hook {
            hook();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.lock().unwrap().cancelled
    }
}

impl CancelToken {
    /// Run `hook` when the token is cancelled, or now if it already was. A
    /// token governs one call, so a second hook replaces the first.
    pub fn on_cancel(&self, hook: impl FnOnce() + Send + 'static) {
        let mut state = self.state.lock().unwrap();
        if state.cancelled {
            drop(state);
            hook();
        } else {
            state.hook = Some(Box::new(hook));
        }
    }
}
