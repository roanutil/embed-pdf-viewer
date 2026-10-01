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
