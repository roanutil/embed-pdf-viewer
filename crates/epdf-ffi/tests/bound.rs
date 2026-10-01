use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use epdf_ffi::{KernelBound, KernelThread, Subscription, ViewerError};
use epdf_kernel::Promise;
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
    let error = block_on(bound.call(|_| -> Result<(), _> {
        Err(epdf_kernel::Error::engine("not-found", "gone"))
    }))
    .unwrap_err();
    assert!(matches!(error, ViewerError::Kernel { code: Some(ref c), .. } if c == "not-found"));
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
            let (promise, _resolver) = Promise::<u32>::pending();
            Ok(promise.with_abort(move |_| flag.store(true, Ordering::SeqCst)))
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
