//! What a plugin hands back for a registration (a tool, a handler, a
//! command, a locale): a way to undo it, callable from any thread.
//!
//! Dropping the handle does not unregister. A registration nobody keeps a
//! handle to is the common case, and it should stay registered.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{KernelBound, KernelPost};

#[derive(uniffi::Object)]
pub struct RegistrationHandle {
    unregister: KernelBound<Rc<dyn Fn()>>,
    done: AtomicBool,
}

impl RegistrationHandle {
    /// On the kernel thread.
    pub fn new(post: KernelPost, unregister: impl Fn() + 'static) -> Arc<Self> {
        let unregister: Rc<dyn Fn()> = Rc::new(unregister);
        Arc::new(RegistrationHandle {
            unregister: KernelBound::new(unregister, post),
            done: AtomicBool::new(false),
        })
    }
}

#[uniffi::export]
impl RegistrationHandle {
    /// Idempotent. Runs on the kernel thread after this returns.
    pub fn unregister(&self) {
        if !self.done.swap(true, Ordering::SeqCst) {
            self.unregister.fire(|unregister| unregister());
        }
    }
}
