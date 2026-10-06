//! The thread the kernel lives on.
//!
//! The kernel is `Rc` all the way down and its runtime is thread-local, so
//! it is built here and never leaves. Everything else reaches it by posting
//! a job. Between jobs the loop sleeps until the runtime's next virtual
//! deadline, then catches the virtual clock up to wall time, as
//! `crates/epdf-web/src/runtime.rs` `after_entry` does with `setTimeout`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use epdf_kernel::{Error, rt};

use crate::bound;
use futures::channel::oneshot;

pub type Job<S> = Box<dyn FnOnce(&mut S) + Send>;
pub type KernelPost = Arc<dyn Fn(Box<dyn FnOnce() + Send>) -> bool + Send + Sync>;

enum Msg<S> {
    Job(Job<S>),
    Stop,
}

type SharedSender<S> = Arc<Mutex<Option<Sender<Msg<S>>>>>;

pub struct KernelThread<S: 'static> {
    tx: SharedSender<S>,
    handle: Option<JoinHandle<()>>,
}

impl<S: 'static> KernelThread<S> {
    pub fn spawn<B>(build: B) -> Result<Self, Error>
    where
        B: FnOnce(KernelPost) -> Result<S, Error> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Msg<S>>();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let post = post_for(&tx);
        let (built_tx, built_rx) = mpsc::channel::<Result<(), Error>>();
        let handle = std::thread::Builder::new()
            .name("epdf-kernel".into())
            .spawn(move || {
                let origin = Instant::now();
                rt::set_monotonic_clock(Some(std::rc::Rc::new(move || {
                    origin.elapsed().as_secs_f64() * 1000.0
                })));
                let mut state = match build(post) {
                    Ok(state) => {
                        let _ = built_tx.send(Ok(()));
                        state
                    }
                    Err(error) => {
                        let _ = built_tx.send(Err(error));
                        return;
                    }
                };
                // A panic, in a job or in a timer or task a drain runs, is
                // reported by the panic hook (stderr by default) and ends the
                // loop through the normal teardown, so what is still bound
                // drops while the thread locals it reaches are alive.
                let mut alive = drain(origin);
                while alive {
                    let msg = match rt::next_deadline() {
                        Some(deadline) => {
                            let wait = (deadline - rt::now()).max(0.0);
                            rx.recv_timeout(Duration::from_secs_f64(wait / 1000.0))
                        }
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    match msg {
                        Ok(Msg::Job(job)) => {
                            // After an idle stretch the virtual clock is
                            // behind: catch it up first, or a timer the job
                            // sets is already due when the job returns.
                            if !drain(origin) {
                                break;
                            }
                            if catch_unwind(AssertUnwindSafe(|| job(&mut state))).is_err() {
                                break;
                            }
                        }
                        Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    alive = drain(origin);
                }
                // Refuse later posts at once rather than queueing them
                // behind the teardown.
                drop(rx);
                drop(state);
                drain(origin);
                // Whatever foreign handles still hold goes now, not in
                // thread-local teardown. Their later removal posts fail:
                // the sender is gone. This covers a thread that shut itself
                // down (detached) as well as one that was joined.
                bound::drop_all();
                drain(origin);
            })
            .map_err(|e| Error::new(format!("[epdf-ffi] no kernel thread: {e}")))?;
        built_rx
            .recv()
            .map_err(|_| Error::new("[epdf-ffi] the kernel thread died while building"))??;
        Ok(KernelThread { tx, handle: Some(handle) })
    }

    pub fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut S) -> T + Send + 'static,
    ) -> oneshot::Receiver<T> {
        let (reply, rx) = oneshot::channel();
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(Msg::Job(Box::new(move |s| {
                let _ = reply.send(f(s));
            })));
        }
        rx
    }

    pub fn post(&self) -> KernelPost {
        post_for(&self.tx)
    }

    /// Stop the loop and join it. Once the sender is taken, every outstanding
    /// `KernelPost` returns `false`, which is how a handle that outlives the
    /// viewer learns the viewer is gone.
    ///
    /// Called from the kernel thread itself (a callback that releases the
    /// last owner), it cannot join: the handle is dropped instead, and the
    /// loop stops after the current job and drops its state as usual.
    pub fn shutdown(&mut self) {
        let Some(handle) = self.handle.take() else { return };
        if let Some(tx) = self.tx.lock().unwrap().take() {
            let _ = tx.send(Msg::Stop);
        }
        if std::thread::current().id() != handle.thread().id() {
            let _ = handle.join();
        }
    }
}

impl<S: 'static> Drop for KernelThread<S> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn post_for<S: 'static>(tx: &SharedSender<S>) -> KernelPost {
    let tx = tx.clone();
    Arc::new(move |job: Box<dyn FnOnce() + Send>| match tx.lock().unwrap().as_ref() {
        Some(tx) => tx.send(Msg::Job(Box::new(move |_| job()))).is_ok(),
        None => false,
    })
}

/// Catch the virtual clock up to wall time, then run everything runnable.
/// `rt::advance` fires each due timer and drains after it. False when a
/// timer or task panicked; the work queued behind it is left unrun.
fn drain(origin: Instant) -> bool {
    catch_unwind(|| {
        let wall = origin.elapsed().as_secs_f64() * 1000.0;
        let behind = wall - rt::now();
        if behind > 0.0 {
            rt::advance(behind);
        }
        rt::run_until_stalled();
    })
    .is_ok()
}
