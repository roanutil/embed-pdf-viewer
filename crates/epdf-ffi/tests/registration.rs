use std::cell::Cell;

use epdf_ffi::{KernelThread, RegistrationHandle};
use futures::executor::block_on;

thread_local! {
    static CALLS: Cell<u32> = const { Cell::new(0) };
}

fn registration() -> (KernelThread<()>, std::sync::Arc<RegistrationHandle>) {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |_| RegistrationHandle::new(post, || CALLS.with(|c| c.set(c.get() + 1)))),
    )
    .unwrap();
    (thread, handle)
}

fn calls(thread: &KernelThread<()>) -> u32 {
    block_on(thread.call(|_| CALLS.with(Cell::get))).unwrap()
}

#[test]
fn unregister_runs_the_closure_on_the_kernel_thread_once() {
    let (thread, registration) = registration();
    registration.unregister();
    registration.unregister();
    assert_eq!(calls(&thread), 1);
}

#[test]
fn dropping_the_handle_does_not_unregister() {
    let (thread, registration) = registration();
    drop(registration);
    assert_eq!(calls(&thread), 0);
}
