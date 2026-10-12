use std::sync::mpsc;
use std::time::{Duration, Instant};

use epdf_ffi::KernelThread;
use futures::executor::block_on;

#[test]
fn a_job_runs_on_the_kernel_thread_and_answers() {
    let thread = KernelThread::spawn(|_| Ok(41u32)).unwrap();
    let answer = block_on(thread.call(|n: &mut u32| {
        *n += 1;
        *n
    }))
    .unwrap();
    assert_eq!(answer, 42);
}

#[test]
fn a_virtual_timer_fires_on_wall_time() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    (thread.post())(Box::new(move || {
        epdf_kernel::rt::set_timeout(50.0, move || tx.send(Instant::now()).unwrap());
    }));
    let fired = rx.recv_timeout(Duration::from_secs(2)).expect("timer never fired");
    assert!(fired.duration_since(started) >= Duration::from_millis(50));
}

#[test]
fn shutdown_is_idempotent_and_refuses_later_jobs() {
    let mut thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    thread.shutdown();
    thread.shutdown();
    assert!(!post(Box::new(|| {})));
}

#[test]
fn a_build_error_reaches_the_caller() {
    let result = KernelThread::<()>::spawn(|_| Err(epdf_kernel::Error::new("no")));
    assert_eq!(result.err().map(|e| e.message), Some("no".into()));
}

/// Signals when the kernel thread drops its state.
struct Dropped(mpsc::Sender<()>);
impl Drop for Dropped {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[test]
fn a_job_that_shuts_down_its_own_thread_detaches_instead_of_joining() {
    let (dropped_tx, dropped) = mpsc::channel();
    let thread = KernelThread::spawn(move |_| Ok(Dropped(dropped_tx))).unwrap();
    let post = thread.post();
    let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(thread)));
    let (done_tx, done) = mpsc::channel();
    let own = shared.clone();
    post(Box::new(move || {
        drop(own.lock().unwrap().take());
        done_tx.send(()).unwrap();
    }));
    done.recv_timeout(Duration::from_secs(2)).expect("the job panicked or hung");
    dropped.recv_timeout(Duration::from_secs(2)).expect("the loop never dropped its state");
    assert!(!post(Box::new(|| {})));
}

/// A thread local with a destructor, first touched just after the slab is:
/// thread locals are torn down in reverse order of first use, so while the
/// slab's teardown runs, this one is already gone.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {}
}

thread_local! {
    static GUARD: Guard = const { Guard };
}

/// A kernel-bound value whose drop reaches the runtime and another thread
/// local, as a plugin capability's does (a timer, a promise, a host table).
/// It reports whether the guard was still alive.
struct TouchesThreadLocals(mpsc::Sender<bool>);
impl Drop for TouchesThreadLocals {
    fn drop(&mut self) {
        let alive = GUARD.try_with(|_| ()).is_ok();
        if alive {
            epdf_kernel::rt::set_timeout(16.0, || {});
        }
        let _ = self.0.send(alive);
    }
}

fn bind(
    thread: &KernelThread<()>,
    dropped: mpsc::Sender<bool>,
) -> epdf_ffi::KernelBound<TouchesThreadLocals> {
    let post = thread.post();
    block_on(thread.call(move |_| {
        let bound = epdf_ffi::KernelBound::new(TouchesThreadLocals(dropped), post);
        GUARD.with(|_| ());
        bound
    }))
    .unwrap()
}

#[test]
fn a_value_still_bound_at_shutdown_is_dropped_before_the_thread_exits() {
    let (dropped_tx, dropped) = mpsc::channel();
    let mut thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let bound = bind(&thread, dropped_tx);
    thread.shutdown();
    let alive = dropped.try_recv().expect("the bound value was never dropped");
    assert!(alive, "the bound value was dropped during thread-local teardown");
    // The handle outlived the thread; its removal post fails harmlessly.
    drop(bound);
}

#[test]
fn a_thread_that_shuts_itself_down_drops_what_is_still_bound() {
    let (dropped_tx, dropped) = mpsc::channel();
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let bound = bind(&thread, dropped_tx);
    let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(thread)));
    let own = shared.clone();
    post(Box::new(move || drop(own.lock().unwrap().take())));
    let alive = dropped.recv_timeout(Duration::from_secs(2)).expect("never dropped");
    assert!(alive, "the bound value was dropped during thread-local teardown");
    drop(bound);
}

#[test]
fn a_timer_set_after_an_idle_gap_waits_its_full_delay() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    // Idle: nothing scheduled, so the loop sleeps and the virtual clock
    // stands still while wall time moves on.
    std::thread::sleep(Duration::from_millis(200));
    let (tx, rx) = mpsc::channel();
    (thread.post())(Box::new(move || {
        let ran = Instant::now();
        epdf_kernel::rt::set_timeout(50.0, move || tx.send((ran, Instant::now())).unwrap());
    }));
    let (ran, fired) = rx.recv_timeout(Duration::from_secs(2)).expect("timer never fired");
    let waited = fired.duration_since(ran);
    assert!(waited >= Duration::from_millis(50), "fired after {waited:?}");
}

#[test]
fn a_panicking_job_tears_the_thread_down_cleanly() {
    let (dropped_tx, dropped) = mpsc::channel();
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let bound = bind(&thread, dropped_tx);
    post(Box::new(|| panic!("a kernel job panicked")));
    let alive = dropped.recv_timeout(Duration::from_secs(2)).expect("never dropped");
    assert!(alive, "the bound value was dropped during thread-local teardown");
    assert!(!post(Box::new(|| {})), "a post after the panic was accepted");
    assert!(block_on(thread.call(|_| ())).is_err(), "a call after the panic was answered");
    assert!(matches!(block_on(bound.call(|_| Ok(()))), Err(epdf_ffi::ViewerError::Closed)));
}

/// Bind a value, run `panics` as a kernel job (it schedules work that
/// panics later, in a drain rather than in the job), and check the thread
/// tore down as it does for a panicking job.
fn assert_a_later_panic_tears_down_cleanly(panics: fn()) {
    let (dropped_tx, dropped) = mpsc::channel();
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let bound = bind(&thread, dropped_tx);
    post(Box::new(panics));
    let alive = dropped.recv_timeout(Duration::from_secs(2)).expect("never dropped");
    assert!(alive, "the bound value was dropped during thread-local teardown");
    assert!(!post(Box::new(|| {})), "a post after the panic was accepted");
    assert!(block_on(thread.call(|_| ())).is_err(), "a call after the panic was answered");
    assert!(matches!(block_on(bound.call(|_| Ok(()))), Err(epdf_ffi::ViewerError::Closed)));
}

#[test]
fn a_panicking_timer_tears_the_thread_down_cleanly() {
    assert_a_later_panic_tears_down_cleanly(|| {
        epdf_kernel::rt::set_timeout(0.0, || panic!("a kernel timer panicked"));
    });
}

#[test]
fn a_task_that_panics_in_a_later_poll_tears_the_thread_down_cleanly() {
    assert_a_later_panic_tears_down_cleanly(|| {
        epdf_kernel::rt::spawn(async {
            epdf_kernel::rt::tick().await;
            panic!("a kernel task panicked");
        });
    });
}

/// Posts again from its Drop, and reports what that post answered.
struct PostsOnDrop(epdf_ffi::KernelPost, mpsc::Sender<bool>);
impl Drop for PostsOnDrop {
    fn drop(&mut self) {
        let _ = self.1.send((self.0)(Box::new(|| {})));
    }
}

#[test]
fn a_refused_job_that_posts_from_its_drop_does_not_relock_the_sender() {
    // A panicking job ends the loop and drops the receiver, but leaves the
    // sender in place, so the next post's send fails and drops the job.
    let (dropped_tx, dropped) = mpsc::channel();
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let bound = bind(&thread, dropped_tx);
    post(Box::new(|| panic!("a kernel job panicked")));
    dropped.recv_timeout(Duration::from_secs(2)).expect("never torn down");
    // Both drops take the sender's lock: if a post below leaves it stuck,
    // they would hang the test instead of failing it.
    std::mem::forget(bound);
    std::mem::forget(thread);
    let (answer_tx, answer) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    std::thread::spawn(move || {
        let inner = PostsOnDrop(post.clone(), answer_tx);
        let accepted = post(Box::new(move || drop(inner)));
        let _ = done_tx.send(accepted);
    });
    let accepted = done.recv_timeout(Duration::from_secs(2)).expect("the post deadlocked");
    assert!(!accepted);
    assert_eq!(answer.recv_timeout(Duration::from_secs(2)), Ok(false));
}
