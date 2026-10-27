use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use epdf_ffi::{
    CancelToken, KernelBound, KernelThread, Subscription, SubscriptionSlot, ViewerError,
};
use epdf_kernel::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;
use futures::executor::block_on;

/// Something `!Send`, as every capability is.
#[derive(Clone)]
struct Counter(Rc<Cell<u32>>);

fn bound() -> (KernelThread<()>, KernelBound<Counter>) {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let bound =
        block_on(thread.call(move |_| KernelBound::new(Counter(Rc::new(Cell::new(0))), post)))
            .unwrap();
    (thread, bound)
}

#[test]
fn a_call_reaches_a_non_send_value() {
    let (_thread, bound) = bound();
    let n = block_on(bound.call(|c| {
        c.0.set(c.0.get() + 1);
        Ok(c.0.get())
    }))
    .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn a_kernel_error_keeps_its_code() {
    let (_thread, bound) = bound();
    let error = block_on(
        bound.call(|_| -> Result<(), _> { Err(epdf_kernel::Error::engine("not-found", "gone")) }),
    )
    .unwrap_err();
    assert!(matches!(error, ViewerError::Kernel { code: Some(ref c), .. } if c == "not-found"));
}

#[test]
fn an_engine_code_crosses_as_its_plugin_code() {
    let (_thread, bound) = bound();
    let code = |error: epdf_kernel::Error| match block_on(
        bound.call(move |_| -> Result<(), _> { Err(error) }),
    ) {
        Err(ViewerError::Kernel { code, .. }) => code,
        other => panic!("{other:?}"),
    };
    let engine = |c: &str| epdf_kernel::Error::engine(c, format!("{c}: gone"));
    assert_eq!(code(engine("NotFound")).as_deref(), Some("not-found"));
    assert_eq!(code(engine("DocPasswordIncorrect")).as_deref(), Some("permission-denied"));
    assert_eq!(code(engine("InvalidArg")).as_deref(), Some("invalid-input"));
    assert_eq!(code(engine("Unknown")).as_deref(), Some("operation-failed"));
    assert_eq!(code(epdf_kernel::Error::new("no code")).as_deref(), Some("operation-failed"));
    let undecodable = epdf_kernel::Error::named("InvalidState", "x").with("code", "invalid-state");
    assert_eq!(code(undecodable).as_deref(), Some("invalid-state"));
    assert_eq!(
        code(epdf_kernel::Error::permission_denied("doc.download", None)).as_deref(),
        Some("permission-denied")
    );
}

#[test]
fn a_promise_resolves_through_the_future() {
    let (_thread, bound) = bound();
    let n = block_on(bound.call_promise(|_| Ok(Promise::resolved(7u32)), |n| n * 6)).unwrap();
    assert_eq!(n, 42);
}

#[test]
fn dropping_a_promise_future_aborts_the_promise() {
    let (thread, bound) = bound();
    let aborted = Arc::new(AtomicBool::new(false));
    let flag = aborted.clone();
    let mut future = Box::pin(bound.call_promise(
        move |_| {
            // The abort holds the resolver. Dropped here instead, it would
            // abandon the promise, which drops the slot's sender: a future
            // first polled after the job ran would answer `Closed`, disarm,
            // and leave nothing for the drop to abort.
            let (promise, resolver) = Promise::<u32>::pending();
            Ok(promise.with_abort(move |_| {
                let _ = &resolver;
                flag.store(true, Ordering::SeqCst);
            }))
        },
        |n| n,
    ));
    // Poll once, then drain the kernel thread so the job that created the
    // promise has certainly run before the waiting future is dropped.
    let _ = block_on(futures::future::select(future.as_mut(), futures::future::ready(())));
    block_on(thread.call(|_| ())).unwrap();
    drop(future);
    block_on(thread.call(|_| ())).unwrap(); // drain the abort job
    assert!(aborted.load(Ordering::SeqCst));
}

#[test]
fn a_call_after_the_thread_stops_is_closed() {
    let (mut thread, bound) = bound();
    thread.shutdown();
    assert!(matches!(block_on(bound.call(|_| Ok(()))), Err(ViewerError::Closed)));
}

#[test]
fn cancelling_a_subscription_unsubscribes_on_the_kernel_thread() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let unsubscribed = Arc::new(AtomicBool::new(false));
    let flag = unsubscribed.clone();
    let (subscription, slot) = Subscription::pending(thread.post());
    block_on(thread.call(move |_| {
        slot.fill(Unsubscribe::new(move || flag.store(true, Ordering::SeqCst)));
    }))
    .unwrap();
    subscription.cancel();
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert!(unsubscribed.load(Ordering::SeqCst));
}

#[test]
fn cancelling_before_the_listener_is_registered_still_unsubscribes() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let unsubscribed = Arc::new(AtomicBool::new(false));
    let flag = unsubscribed.clone();
    let (subscription, slot) = Subscription::pending(thread.post());
    subscription.cancel();
    block_on(thread.call(move |_| {
        slot.fill(Unsubscribe::new(move || flag.store(true, Ordering::SeqCst)));
    }))
    .unwrap();
    assert!(unsubscribed.load(Ordering::SeqCst));
}

#[test]
fn dropping_a_promise_future_before_the_job_runs_aborts_and_releases_the_promise() {
    let (thread, bound) = bound();
    // Hold the kernel thread so the call's job queues behind this one.
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();

    let aborted = Arc::new(AtomicBool::new(false));
    let flag = aborted.clone();
    let held = Arc::new(());
    let witness = held.clone();
    let future = bound.call_promise(
        move |_| {
            let (promise, _resolver) = Promise::<u32>::pending();
            Ok(promise.with_abort(move |_| {
                let _keep = &witness;
                flag.store(true, Ordering::SeqCst)
            }))
        },
        |n| n,
    );
    drop(future);
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    block_on(thread.call(|_| ())).unwrap();
    assert!(aborted.load(Ordering::SeqCst));
    // The slab would keep the promise, and its abort closure, alive.
    assert_eq!(Arc::strong_count(&held), 1);
}

/// A pending promise whose abort rejects it, as an engine call's does.
fn abortable(aborted: Arc<AtomicBool>) -> Promise<u32> {
    let (promise, resolver) = Promise::<u32>::pending();
    promise.with_abort(move |reason| {
        aborted.store(true, Ordering::SeqCst);
        resolver.reject(epdf_kernel::Error::abort(reason));
    })
}

fn is_cancelled(error: &ViewerError) -> bool {
    matches!(error, ViewerError::Kernel { code: Some(c), .. } if c == "operation-cancelled")
}

#[test]
fn cancelling_before_the_job_runs_skips_the_call() {
    let (thread, bound) = bound();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();

    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let token = CancelToken::new();
    let future = bound.call_promise_cancellable(
        token.clone(),
        move |_| {
            flag.store(true, Ordering::SeqCst);
            Ok(Promise::resolved(1u32))
        },
        |n| n,
    );
    token.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(future).unwrap_err();
    assert!(is_cancelled(&error), "{error:?}");
    assert!(!ran.load(Ordering::SeqCst));
}

#[test]
fn cancelling_a_pending_promise_aborts_it_and_answers_its_rejection() {
    let (thread, bound) = bound();
    let aborted = Arc::new(AtomicBool::new(false));
    let flag = aborted.clone();
    let token = CancelToken::new();
    let mut future = Box::pin(bound.call_promise_cancellable(
        token.clone(),
        move |_| Ok(abortable(flag)),
        |n| n,
    ));
    let _ = block_on(futures::future::select(future.as_mut(), futures::future::ready(())));
    block_on(thread.call(|_| ())).unwrap(); // the job has parked the promise
    token.cancel();
    token.cancel();
    // The promise's own rejection comes back: here the bare `AbortError` an
    // engine call rejects with (a plugin maps it to `operation-cancelled`).
    let error = block_on(future).unwrap_err();
    assert!(
        matches!(error, ViewerError::Kernel { ref name, .. } if name == "AbortError"),
        "{error:?}"
    );
    assert!(aborted.load(Ordering::SeqCst));
}

#[test]
fn cancelling_after_the_promise_settled_changes_nothing() {
    let (thread, bound) = bound();
    let aborted = Arc::new(AtomicBool::new(false));
    let flag = aborted.clone();
    let token = CancelToken::new();
    let n = block_on(bound.call_promise_cancellable(
        token.clone(),
        move |_| {
            Ok(Promise::resolved(7u32).with_abort(move |_| flag.store(true, Ordering::SeqCst)))
        },
        |n| n * 6,
    ))
    .unwrap();
    token.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(n, 42);
    assert!(!aborted.load(Ordering::SeqCst));
}

#[test]
fn a_promise_that_ignores_the_abort_still_answers() {
    let (thread, bound) = bound();
    let token = CancelToken::new();
    let mut future = Box::pin(bound.call_promise_cancellable(
        token.clone(),
        |_| {
            let (promise, resolve) = Promise::<u32>::pending();
            // `Resolver` is `!Send`: it stays on the kernel thread.
            RESOLVER.with(|r| *r.borrow_mut() = Some(resolve));
            Ok(promise)
        },
        |n| n,
    ));
    let _ = block_on(futures::future::select(future.as_mut(), futures::future::ready(())));
    block_on(thread.call(|_| ())).unwrap();
    token.cancel();
    block_on(thread.call(|_| RESOLVER.with(|r| r.borrow_mut().take().unwrap().resolve(5))))
        .unwrap();
    assert_eq!(block_on(future).unwrap(), 5);
}

thread_local! {
    static RESOLVER: std::cell::RefCell<Option<epdf_kernel::rt::Resolver<u32>>> =
        const { std::cell::RefCell::new(None) };
    static LIFETIME: AbortSignal = AbortSignal::new();
}

/// Counts `on_end` calls and records whether the listener was removed.
#[derive(Clone, Default)]
struct Ends {
    ended: Arc<AtomicUsize>,
    unsubscribed: Arc<AtomicBool>,
    registered: Arc<AtomicBool>,
}

impl Ends {
    /// On the kernel thread: fill `slot` against this thread's `LIFETIME`.
    fn fill(&self, slot: SubscriptionSlot) {
        let (ended, unsubscribed, registered) =
            (self.ended.clone(), self.unsubscribed.clone(), self.registered.clone());
        LIFETIME.with(|lifetime| {
            slot.fill_until_end(
                lifetime,
                move || {
                    registered.store(true, Ordering::SeqCst);
                    Unsubscribe::new(move || unsubscribed.store(true, Ordering::SeqCst))
                },
                move || {
                    ended.fetch_add(1, Ordering::SeqCst);
                },
            )
        });
    }
}

fn end_lifetime() {
    LIFETIME.with(|l| l.abort(epdf_kernel::Error::new("closed")));
}

#[test]
fn a_subscription_ends_once_when_its_lifetime_aborts() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let ends = Ends::default();
    let (subscription, slot) = Subscription::pending(thread.post());
    let filler = ends.clone();
    block_on(thread.call(move |_| {
        filler.fill(slot);
        end_lifetime();
        end_lifetime();
    }))
    .unwrap();
    assert_eq!(ends.ended.load(Ordering::SeqCst), 1);
    assert!(ends.unsubscribed.load(Ordering::SeqCst));
    // Cancelling after the end is harmless and does not end again.
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(ends.ended.load(Ordering::SeqCst), 1);
}

#[test]
fn subscribing_after_the_lifetime_ended_ends_at_once() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let ends = Ends::default();
    let (_subscription, slot) = Subscription::pending(thread.post());
    let filler = ends.clone();
    block_on(thread.call(move |_| {
        end_lifetime();
        filler.fill(slot);
    }))
    .unwrap();
    assert_eq!(ends.ended.load(Ordering::SeqCst), 1);
    assert!(!ends.registered.load(Ordering::SeqCst));
}

#[test]
fn a_cancelled_subscription_does_not_end() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let ends = Ends::default();
    let (subscription, slot) = Subscription::pending(thread.post());
    let filler = ends.clone();
    block_on(thread.call(move |_| filler.fill(slot))).unwrap();
    subscription.cancel();
    block_on(thread.call(|_| end_lifetime())).unwrap();
    assert_eq!(ends.ended.load(Ordering::SeqCst), 0);
    assert!(ends.unsubscribed.load(Ordering::SeqCst));
}

thread_local! {
    static WATCH: std::cell::RefCell<Option<std::rc::Weak<Cell<u32>>>> =
        const { std::cell::RefCell::new(None) };
}

#[test]
fn a_clone_shares_the_value_and_the_last_drop_frees_it() {
    let (thread, bound) = bound();
    block_on(bound.call(|c| {
        WATCH.with(|w| *w.borrow_mut() = Some(Rc::downgrade(&c.0)));
        Ok(())
    }))
    .unwrap();
    let clone = bound.clone();
    drop(bound);
    let n = block_on(clone.call(|c| {
        c.0.set(5);
        Ok(c.0.get())
    }))
    .unwrap();
    assert_eq!(n, 5);
    drop(clone);
    let alive = block_on(thread.call(|_| {
        WATCH.with(|w| w.borrow().as_ref().is_some_and(|weak| weak.upgrade().is_some()))
    }))
    .unwrap();
    assert!(!alive, "the slab entry outlived its last handle");
}
