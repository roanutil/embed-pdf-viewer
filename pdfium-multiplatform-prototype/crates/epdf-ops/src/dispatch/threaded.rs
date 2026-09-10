use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{channel, Sender};
use std::thread;

use crate::{OpsError, State};

/// Extracts a human-readable message from a `catch_unwind` payload the same
/// way `Engine::new` already does for a panic during `State::new`.
fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "worker job panicked with a non-string payload".to_string()
    }
}

type Job = Box<dyn FnOnce(&mut State) + Send>;

pub struct Worker {
    tx: Sender<Job>,
}

impl Worker {
    pub fn spawn() -> Result<Self, OpsError> {
        let (tx, rx) = channel::<Job>();
        let (ready_tx, ready_rx) = channel::<Result<(), OpsError>>();

        thread::Builder::new()
            .name("epdf-pdfium".to_string())
            .spawn(move || {
                let mut state = match State::new() {
                    Ok(state) => {
                        let _ = ready_tx.send(Ok(()));
                        state
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                // Ends when the process outlives the last handle: every Sender
                // drops, which drops State and therefore FPDF_DestroyLibrary on
                // this thread. That is the only thread allowed to call it.
                while let Ok(job) = rx.recv() {
                    // Jobs built by `run` already catch their own panics and
                    // reply with `OpsError::Internal` before this point, so
                    // `state` should never be observed unwinding through here.
                    // This is defense in depth: it keeps one panicking job
                    // from taking every future document on this engine down
                    // with it.
                    let _ = panic::catch_unwind(AssertUnwindSafe(|| job(&mut state)));
                }
            })
            .map_err(|err| OpsError::Internal { message: err.to_string() })?;

        ready_rx
            .recv()
            .map_err(|err| OpsError::Internal { message: err.to_string() })??;

        Ok(Self { tx })
    }

    /// Blocks the calling thread until the PDFium thread answers. Deliberately
    /// synchronous: this scaffold does not answer the wave-0 cancellation
    /// question in docs/research/rust-core-port.md, and an async signature would
    /// imply that it does.
    pub fn run<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T, OpsError> + Send + 'static,
    {
        let (tx, rx) = channel();
        self.tx
            .send(Box::new(move |state: &mut State| {
                // Caught here, not in the worker loop: `tx` is captured by
                // this closure, so it is only still alive to carry the
                // answer back while we are still inside it. By the time an
                // unwind reaches the loop that calls this closure, `tx` has
                // already been dropped and there is no one left to tell.
                let result = panic::catch_unwind(AssertUnwindSafe(|| f(state)))
                    .unwrap_or_else(|payload| Err(OpsError::Internal { message: panic_message(payload) }));
                let _ = tx.send(result);
            }))
            .map_err(|_| OpsError::EngineFailed { message: "the worker thread's job queue is gone".to_string() })?;
        // In practice this only fires if `tx` above was dropped without a
        // send, which the job closure always does on every path (including
        // a caught panic), so this is nearly unreachable. If it ever does
        // fire, note the asymmetry: the worker thread is still alive and
        // serving other jobs, yet `OpsError::EngineFailed`'s own doc comment
        // tells the caller to discard the whole engine anyway.
        rx.recv().map_err(|_| OpsError::EngineFailed { message: "the worker thread's reply channel is gone".to_string() })?
    }

    /// Enqueues `f` and returns immediately, without waiting for it to run
    /// or observing its result. Unlike `run`, this cannot block the caller
    /// on whatever is already ahead of it in the queue. `f` still runs, in
    /// queue order, once the worker gets to it; there is simply no reply to
    /// wait for, and a panic inside it is swallowed by the loop's own
    /// `catch_unwind` rather than reported anywhere. Used by
    /// `Drop for Document`, where blocking is not acceptable.
    pub fn send<F>(&self, f: F)
    where
        F: FnOnce(&mut State) + Send + 'static,
    {
        let _ = self.tx.send(Box::new(f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_really_run_on_the_named_pdfium_thread() {
        let worker = Worker::spawn().unwrap();
        let name = worker.run(|_state| Ok(thread::current().name().map(str::to_string))).unwrap();
        assert_eq!(name.as_deref(), Some("epdf-pdfium"));
    }

    #[test]
    fn a_panicking_job_reports_internal_and_the_slab_stays_usable() {
        let worker = Worker::spawn().unwrap();

        // A job that ignores `State` entirely would only prove the thread
        // survives, not that its slab does. Open a real document first, so
        // the panic below has something in `State` to potentially corrupt.
        let bytes = test_support::fixture_bytes();
        let id = worker
            .run({
                let bytes = bytes.clone();
                move |state| {
                    let doc = pdfium::Document::from_bytes(state.library(), &bytes, None)?;
                    Ok(state.insert(doc))
                }
            })
            .unwrap();

        // Expected: the default panic hook still prints this panic's
        // message to stderr, even though catch_unwind stops it from
        // unwinding any further. That is a property of
        // std::panic::catch_unwind itself, not a bug here. The job touches
        // the document opened above before panicking, so the panic happens
        // with a live borrow of the slab entry in scope.
        let err = worker
            .run(move |state| -> Result<(), OpsError> {
                let _doc = state.get(id)?;
                panic!("deliberate panic for the recovery test")
            })
            .expect_err("a panicking job must not be reported as success");
        assert!(
            matches!(err, OpsError::Internal { .. }),
            "expected OpsError::Internal from a caught panic, got {err:?}"
        );
        assert_ne!(err, OpsError::Closed, "a panic must not be conflated with a closed document");

        // The document that was open before the panic must still be usable:
        // proof the slab entry itself, not just the worker thread, survived.
        let page_count = worker.run(move |state| Ok(state.get(id)?.page_count())).unwrap();
        assert!(page_count > 0);

        // Opening a brand-new document on the same worker must also still
        // work, proving the slab as a whole (not merely the one entry
        // touched above) is unharmed.
        let other_id = worker
            .run(move |state| {
                let doc = pdfium::Document::from_bytes(state.library(), &bytes, None)?;
                Ok(state.insert(doc))
            })
            .unwrap();
        let other_page_count = worker.run(move |state| Ok(state.get(other_id)?.page_count())).unwrap();
        assert_eq!(other_page_count, page_count);
    }
}
