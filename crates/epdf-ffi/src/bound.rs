//! A `Send + Sync` handle to a `!Send` value that lives on the kernel thread.
//!
//! The value sits in a kernel-thread-local slab; the handle is its id plus a
//! way to post jobs. Every uniffi object that wraps a capability holds one.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use epdf_kernel::{Error, Promise};
use futures::channel::oneshot;

use crate::{CancelToken, KernelPost, ViewerError};

thread_local! {
    static SLAB: RefCell<HashMap<u64, Box<dyn Any>>> = RefCell::new(HashMap::new());
    static AT_TEARDOWN: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}
/// Run the teardown hooks, then empty this thread's slab and drop what it
/// held: the kernel thread's last step, while its thread locals (the
/// runtime, the host table) are alive. Left to thread-local teardown, a
/// capability a foreign handle still holds would drop after the locals its
/// drop reaches, and abort the process.
pub(crate) fn drop_all() {
    loop {
        let hooks = AT_TEARDOWN.with(|h| std::mem::take(&mut *h.borrow_mut()));
        if hooks.is_empty() {
            break;
        }
        hooks.into_iter().for_each(|hook| hook());
    }
    let held = SLAB.with(|s| std::mem::take(&mut *s.borrow_mut()));
    drop(held);
}

/// On the kernel thread: run `hook` when the thread tears down, just before
/// the slab empties. For a crate that keeps its own kernel-thread locals
/// (the manual bridge's abort table): what they hold must drop here, not in
/// thread-local teardown, for the reason `drop_all` gives.
pub fn at_teardown(hook: impl FnOnce() + 'static) {
    AT_TEARDOWN.with(|h| h.borrow_mut().push(Box::new(hook)));
}

static NEXT: AtomicU64 = AtomicU64::new(1);
/// Stored in a `call_promise` slot by a dropped future that had no promise yet.
const CANCELLED: u64 = u64::MAX;

pub struct KernelBound<C: 'static> {
    id: u64,
    post: KernelPost,
    _value: PhantomData<fn() -> C>,
}

impl<C: 'static> KernelBound<C> {
    /// Call on the kernel thread: the value goes into that thread's slab.
    pub fn new(value: C, post: KernelPost) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        SLAB.with(|s| s.borrow_mut().insert(id, Box::new(value)));
        KernelBound { id, post, _value: PhantomData }
    }

    pub fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&C) -> Result<T, Error> + Send + 'static,
    ) -> impl Future<Output = Result<T, ViewerError>> + Send {
        let (tx, rx) = oneshot::channel();
        let id = self.id;
        (self.post)(Box::new(move || {
            let _ = tx.send(with(id, |c: &C| f(c).map_err(ViewerError::from)));
        }));
        async move { rx.await.unwrap_or(Err(ViewerError::Closed)) }
    }

    /// Dropping the returned future aborts the promise.
    pub fn call_promise<U: Clone + 'static, T: Send + 'static>(
        &self,
        f: impl FnOnce(&C) -> Result<Promise<U>, Error> + Send + 'static,
        map: impl FnOnce(U) -> T + Send + 'static,
    ) -> impl Future<Output = Result<T, ViewerError>> + Send {
        let (slot, reply) = PromiseSlot::pending(self.post.clone(), map);
        let id = self.id;
        (self.post)(Box::new(move || {
            slot.start(with(id, |c: &C| f(c).map_err(ViewerError::from)));
        }));
        reply
    }

    /// `call_promise`, also aborted by `token`. A token cancelled before the
    /// job runs skips `f` and answers `operation-cancelled`; one cancelled
    /// while the promise is pending aborts it, and the future answers with
    /// whatever the promise settles with (a promise that ignores the abort
    /// still answers normally). Cancelling after it settled does nothing.
    pub fn call_promise_cancellable<U: Clone + 'static, T: Send + 'static>(
        &self,
        token: Arc<CancelToken>,
        f: impl FnOnce(&C) -> Result<Promise<U>, Error> + Send + 'static,
        map: impl FnOnce(U) -> T + Send + 'static,
    ) -> impl Future<Output = Result<T, ViewerError>> + Send {
        let (slot, reply) = PromiseSlot::pending(self.post.clone(), map);
        slot.abort_on(&token);
        let id = self.id;
        (self.post)(Box::new(move || {
            if token.is_cancelled() {
                slot.start(Err(ViewerError::cancelled()));
            } else {
                slot.start(with(id, |c: &C| f(c).map_err(ViewerError::from)));
            }
        }));
        reply
    }

    /// No reply: for hot paths where the caller never waits.
    pub fn fire(&self, f: impl FnOnce(&C) + Send + 'static) {
        let id = self.id;
        (self.post)(Box::new(move || {
            let _ = with(id, |c: &C| {
                f(c);
                Ok(())
            });
        }));
    }
}

impl<C: 'static> Drop for KernelBound<C> {
    fn drop(&mut self) {
        let id = self.id;
        (self.post)(Box::new(move || {
            SLAB.with(|s| s.borrow_mut().remove(&id));
        }));
    }
}

fn with<C: 'static, T>(
    id: u64,
    f: impl FnOnce(&C) -> Result<T, ViewerError>,
) -> Result<T, ViewerError> {
    let value = SLAB
        .with(|s| s.borrow().get(&id).and_then(|v| v.downcast_ref::<C>().map(|c| c as *const C)));
    match value {
        // The slab entry outlives this call: only a later job on this same
        // thread can remove it. The `RefCell` borrow is not held across `f`,
        // so `f` may re-enter the slab (a call that returns a new capability
        // wraps it in a `KernelBound`).
        Some(c) => f(unsafe { &*c }),
        None => Err(ViewerError::Closed),
    }
}

/// The kernel-thread half of a call that answers with a promise. The job
/// hands it what it started; it parks the promise until it settles, or until
/// the caller's future is dropped, which aborts it. `KernelBound` and any
/// other owner of kernel-thread state share it.
pub struct PromiseSlot<U: Clone + 'static, T: Send + 'static> {
    tx: oneshot::Sender<Result<T, ViewerError>>,
    pending: Arc<AtomicU64>,
    post: KernelPost,
    map: Box<dyn FnOnce(U) -> T + Send>,
}

impl<U: Clone + 'static, T: Send + 'static> PromiseSlot<U, T> {
    /// The slot to move into the job, and the future that answers. Aborts
    /// go through `post`. A slot dropped unstarted (its job never ran)
    /// answers `Closed`.
    pub fn pending(
        post: KernelPost,
        map: impl FnOnce(U) -> T + Send + 'static,
    ) -> (Self, impl Future<Output = Result<T, ViewerError>> + Send) {
        let (tx, rx) = oneshot::channel::<Result<T, ViewerError>>();
        let pending = Arc::new(AtomicU64::new(0));
        let guard =
            AbortOnDrop::<U> { pending: pending.clone(), post: post.clone(), _value: PhantomData };
        let reply = async move {
            let result = rx.await.unwrap_or(Err(ViewerError::Closed));
            guard.disarm();
            result
        };
        (PromiseSlot { tx, pending, post, map: Box::new(map) }, reply)
    }

    /// Abort the promise when `token` is cancelled, as dropping the future
    /// would, but keep answering: the future gets the promise's rejection.
    pub fn abort_on(&self, token: &CancelToken) {
        let pending = self.pending.clone();
        let post = self.post.clone();
        token.on_cancel(move || abort_parked::<U>(&pending, &post));
    }

    /// On the kernel thread, with what the job started.
    pub fn start(self, started: Result<Promise<U>, ViewerError>) {
        let PromiseSlot { tx, pending, map, .. } = self;
        match started {
            Err(error) => {
                let _ = tx.send(Err(error));
            }
            Ok(promise) => {
                let pid = NEXT.fetch_add(1, Ordering::Relaxed);
                // The future may have been dropped, or its token cancelled,
                // before this job ran: then `CANCELLED` is already in the
                // slot, so abort now and don't park the promise. A cancelled
                // caller is still waiting for the answer.
                let parked =
                    pending.compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst).is_ok();
                if parked {
                    SLAB.with(|s| s.borrow_mut().insert(pid, Box::new(promise.clone())));
                } else {
                    promise.abort(None);
                }
                promise.on_settle(move |result| {
                    if parked {
                        SLAB.with(|s| s.borrow_mut().remove(&pid));
                    }
                    let _ = tx.send(match result {
                        Ok(value) => Ok(map(value.clone())),
                        Err(error) => Err(ViewerError::from(error.clone())),
                    });
                });
            }
        }
    }
}

struct AbortOnDrop<U: Clone + 'static> {
    pending: Arc<AtomicU64>,
    post: KernelPost,
    _value: PhantomData<fn() -> U>,
}

impl<U: Clone + 'static> AbortOnDrop<U> {
    fn disarm(self) {
        self.pending.store(0, Ordering::SeqCst);
    }
}

impl<U: Clone + 'static> Drop for AbortOnDrop<U> {
    fn drop(&mut self) {
        abort_parked::<U>(&self.pending, &self.post);
    }
}

/// Mark the slot cancelled, and abort the promise parked in it, if any.
fn abort_parked<U: Clone + 'static>(pending: &AtomicU64, post: &KernelPost) {
    let pid = pending.swap(CANCELLED, Ordering::SeqCst);
    // 0: the job hasn't published a promise yet and will see `CANCELLED`
    // (or the reply already disarmed the slot, and there is nothing to do).
    if pid == 0 || pid == CANCELLED {
        return;
    }
    post(Box::new(move || {
        let promise = SLAB.with(|s| s.borrow_mut().remove(&pid));
        if let Some(promise) = promise.and_then(|p| p.downcast::<Promise<U>>().ok()) {
            promise.abort(None);
        }
    }));
}
