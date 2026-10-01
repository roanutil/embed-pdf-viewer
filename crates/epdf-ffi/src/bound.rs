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

use crate::{KernelPost, ViewerError};

thread_local! {
    static SLAB: RefCell<HashMap<u64, Box<dyn Any>>> = RefCell::new(HashMap::new());
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
        let guard = AbortOnDrop::<U> { pending: pending.clone(), post, _value: PhantomData };
        let reply = async move {
            let result = rx.await.unwrap_or(Err(ViewerError::Closed));
            guard.disarm();
            result
        };
        (PromiseSlot { tx, pending, map: Box::new(map) }, reply)
    }

    /// On the kernel thread, with what the job started.
    pub fn start(self, started: Result<Promise<U>, ViewerError>) {
        let PromiseSlot { tx, pending, map } = self;
        match started {
            Err(error) => {
                let _ = tx.send(Err(error));
            }
            Ok(promise) => {
                let pid = NEXT.fetch_add(1, Ordering::Relaxed);
                // The future may have been dropped before this job ran:
                // then `CANCELLED` is already in the slot, so abort now
                // and don't park the promise.
                if pending.compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst).is_err() {
                    promise.abort(None);
                    return;
                }
                SLAB.with(|s| s.borrow_mut().insert(pid, Box::new(promise.clone())));
                promise.on_settle(move |result| {
                    SLAB.with(|s| s.borrow_mut().remove(&pid));
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
        let pid = self.pending.swap(CANCELLED, Ordering::SeqCst);
        // 0: the job hasn't published a promise yet and will see `CANCELLED`.
        if pid == 0 || pid == CANCELLED {
            return;
        }
        (self.post)(Box::new(move || {
            let promise = SLAB.with(|s| s.borrow_mut().remove(&pid));
            if let Some(promise) = promise.and_then(|p| p.downcast::<Promise<U>>().ok()) {
                promise.abort(None);
            }
        }));
    }
}
