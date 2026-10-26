//! `ctx.serialQueue('writes')`: the kernel's serial queue, with the two
//! members of the TypeScript `SerialQueue` the Rust one doesn't carry
//! (`packages/core/main/src/serial-queue.ts`): an operation whose signal
//! fired while it waited never starts (`operation-cancelled`), and
//! `idle()`, which resolves once everything queued so far has finished.
//! Both are kept here, over the kernel queue's own order and timing.

use std::cell::RefCell;

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::signal::AbortSignal;

pub struct FormQueue {
    queue: SerialQueue,
    /// `tail`: settles a turn after the last operation queued.
    tail: RefCell<Promise<()>>,
}

impl FormQueue {
    pub fn new(queue: SerialQueue) -> Self {
        FormQueue { queue, tail: RefCell::new(Promise::resolved(())) }
    }

    /// `queue(operation, { signal })`.
    pub fn run<T: Clone + 'static>(
        &self,
        operation: impl FnOnce() -> Result<Promise<T>, Error> + 'static,
        signal: Option<AbortSignal>,
    ) -> Promise<T> {
        let start = move || match signal.filter(AbortSignal::aborted) {
            // A rejected promise, not a throw: `tail.then(start, start)`
            // adopts what `start` returns.
            Some(signal) => Ok(Promise::rejected(plugin_error(
                code::OPERATION_CANCELLED,
                crate::controller::CAPABILITY,
                "cancelled before it started",
                signal.reason().map(|e| e.to_canonical()),
                None,
            ))),
            None => operation(),
        };
        let result = self.queue.run(start);
        let done = result.clone();
        *self.tail.borrow_mut() = Promise::spawn(async move {
            let _ = done.await;
            Ok(())
        });
        result
    }

    /// `queue.idle()`.
    pub fn idle(&self) -> Promise<()> {
        self.tail.borrow().clone()
    }
}
