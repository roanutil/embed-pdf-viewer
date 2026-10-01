//! A listener registered on the kernel thread, cancellable from any thread.
//!
//! The export returns the `Subscription` before the kernel thread has run
//! the job that registers the listener, so a cancel can arrive first. The
//! cancelled flag and the unsubscribe handle share one lock: whichever of
//! `cancel` and `fill` runs second sees the other's effect and unsubscribes.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;

use crate::{KernelBound, KernelPost};

#[derive(Default)]
struct State {
    cancelled: bool,
    unsubscribe: Option<KernelBound<Unsubscribe>>,
}

#[derive(uniffi::Object)]
pub struct Subscription {
    state: Arc<Mutex<State>>,
}

/// The kernel-thread half: `fill` it once the listener is registered.
pub struct SubscriptionSlot {
    state: Arc<Mutex<State>>,
    post: KernelPost,
}

impl Subscription {
    pub fn pending(post: KernelPost) -> (Arc<Subscription>, SubscriptionSlot) {
        let state = Arc::new(Mutex::new(State::default()));
        let subscription = Arc::new(Subscription { state: state.clone() });
        (subscription, SubscriptionSlot { state, post })
    }
}

impl SubscriptionSlot {
    /// On the kernel thread, once the listener is registered. If the
    /// subscription was cancelled first, the listener goes straight away.
    pub fn fill(self, unsubscribe: Unsubscribe) {
        let mut state = self.state.lock().unwrap();
        if state.cancelled {
            drop(state);
            unsubscribe.call();
        } else {
            state.unsubscribe = Some(KernelBound::new(unsubscribe, self.post));
        }
    }

    /// On the kernel thread: `fill` with a listener that lives no longer than
    /// `lifetime` (a per-document controller's lifetime, aborted at close).
    /// When the lifetime aborts, the listener is removed and `on_end` runs,
    /// once; if it already aborted, `register` is never called and `on_end`
    /// runs now. A subscription cancelled first never ends, and cancelling
    /// after the end does nothing more.
    pub fn fill_until_end(
        self,
        lifetime: &AbortSignal,
        register: impl FnOnce() -> Unsubscribe,
        on_end: impl FnOnce() + 'static,
    ) {
        if lifetime.aborted() {
            on_end();
            self.fill(Unsubscribe::noop());
            return;
        }
        let off = Rc::new(register());
        let end = Cell::new(Some(on_end));
        let listener = {
            let off = off.clone();
            lifetime.on_abort(Rc::new(move |_| {
                off.call();
                if let Some(on_end) = end.take() {
                    on_end();
                }
            }))
        };
        let lifetime = lifetime.clone();
        self.fill(Unsubscribe::new(move || {
            lifetime.remove(listener);
            off.call();
        }));
    }
}

#[uniffi::export]
impl Subscription {
    /// Idempotent. Dropping the object cancels too.
    pub fn cancel(&self) {
        let bound = {
            let mut state = self.state.lock().unwrap();
            state.cancelled = true;
            state.unsubscribe.take()
        };
        if let Some(bound) = bound {
            bound.fire(|unsubscribe| unsubscribe.call());
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel();
    }
}
