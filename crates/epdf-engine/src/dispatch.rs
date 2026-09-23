//! Owner-thread dispatch, queues and budgets.
//!
//! One owner OS thread per engine, inside Rust. The exported engine object is
//! thread-safe and submits jobs; handle types are `!Send + !Sync`. A Swift
//! actor, a coroutine dispatcher or a mutex does not guarantee affinity, which
//! is the whole reason this exists rather than a lock around a `Document`.
//!
//! The written contract, clause by clause with what implements each one and
//! what is still owed, is
//! `.agents/skills/embedpdf-conventions/references/engine-lifecycle.md`.
//! What is here is the minimum Phase 1's smoke apps need: a thread that owns
//! PDFium state and runs closures on it, and a close that drains. Bounded
//! queues, admission budgets and the reserved control route are G3 and are not
//! here; the contract says so rather than implying otherwise.

use std::sync::mpsc::{Sender, SyncSender, channel, sync_channel};
use std::thread::JoinHandle;

use crate::error::{EngineError, EngineErrorCode};

/// A job the owner thread runs, handed the state that lives there. Boxed
/// because each one returns a different type; the result travels back through
/// a channel the caller owns.
type Job<S> = Box<dyn FnOnce(&mut S) + Send>;

/// A thread that owns PDFium state, and the only thread that touches it.
///
/// `S` is that state, and it is created on the thread and dropped on the
/// thread. It never appears in this type, which is the point: `OwnedDocument`
/// and everything else holding an `FPDF_*` handle is `!Send`, so a design that
/// parked them in a field would make the engine unshareable and a design that
/// forced them `Send` would be a lie. Callers send a closure in and get a
/// `Send` value back.
///
/// That is what lets the exported engine object be `Send + Sync` while every
/// handle it owns stays confined, which is the arrangement
/// `multiplatform-plan.md` Section 5.2 describes.
pub struct OwnerThread<S: 'static> {
    /// `None` once closed, which is what makes `close` idempotent and makes a
    /// later submit fail rather than block forever on a dead receiver.
    jobs: Option<Sender<Job<S>>>,
    handle: Option<JoinHandle<()>>,
}

impl<S: 'static> std::fmt::Debug for OwnerThread<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerThread").field("closed", &self.is_closed()).finish()
    }
}

impl<S: 'static> OwnerThread<S> {
    /// Starts the thread. `setup` runs on it before any job and builds the
    /// state, which is where per-thread PDFium initialization belongs.
    ///
    /// `setup` returns `S` rather than taking one, because `S` is typically
    /// `!Send` and could not be moved onto the thread even if a caller had
    /// built it.
    pub fn start(setup: impl FnOnce() -> S + Send + 'static) -> Self {
        let (jobs, receiver) = channel::<Job<S>>();
        let handle = std::thread::Builder::new()
            .name("epdf-owner".to_string())
            .spawn(move || {
                let mut state = setup();
                // Ends when every Sender is dropped, which is what `close`
                // does. No sentinel message, so a caller cannot invent one.
                while let Ok(job) = receiver.recv() {
                    job(&mut state);
                }
                // `state` drops here, on the owner thread, which is the only
                // place PDFium handles may be destroyed.
            })
            .expect("the owner thread starts");
        Self { jobs: Some(jobs), handle: Some(handle) }
    }

    /// Runs `job` on the owner thread and waits for its result.
    ///
    /// Blocking by design: the host's async boundary is above this, in the
    /// facade, and pretending to be async here would just move the block. The
    /// plan is explicit that a facade must not wrap this in an `async` method
    /// and call it on the UI thread.
    pub fn run<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut S) -> T + Send + 'static,
    ) -> Result<T, EngineError> {
        let jobs = self.jobs.as_ref().ok_or_else(Self::closed)?;
        let (tx, rx): (SyncSender<T>, _) = sync_channel(1);
        jobs.send(Box::new(move |state| {
            // A closed receiver means the caller gave up; the work is done
            // either way and dropping the value is correct.
            let _ = tx.send(job(state));
        }))
        .map_err(|_| Self::closed())?;
        rx.recv().map_err(|_| {
            EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                "the owner thread stopped before returning a result",
            )
        })
    }

    /// Stops accepting work and waits for the thread to finish what it has.
    ///
    /// Idempotent. Dropping the sender is what ends the loop, so jobs already
    /// queued still run: this drains rather than cancels, which is the
    /// lifecycle contract's close for work that has already been admitted.
    pub fn close(&mut self) {
        self.jobs = None;
        if let Some(handle) = self.handle.take() {
            // A panicked owner thread has already reported through its job's
            // channel; joining it here is about releasing the thread, not
            // about learning what happened.
            let _ = handle.join();
        }
    }

    pub fn is_closed(&self) -> bool {
        self.jobs.is_none()
    }

    fn closed() -> EngineError {
        EngineError::with_message(EngineErrorCode::DocNotOpen, "the engine is closed")
    }
}

impl<S: 'static> Drop for OwnerThread<S> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn jobs_run_on_one_thread_that_is_not_the_caller() {
        let owner = OwnerThread::<()>::start(|| ());
        let caller = std::thread::current().id();
        let first = owner.run(|_| std::thread::current().id()).expect("first");
        let second = owner.run(|_| std::thread::current().id()).expect("second");

        assert_ne!(first, caller, "PDFium work must not run on the caller's thread");
        assert_eq!(first, second, "and every job must land on the same one");
    }

    #[test]
    fn setup_runs_before_any_job() {
        // Per-thread PDFium initialization goes here, so a job that ran first
        // would be using an uninitialized library.
        let order = Arc::new(AtomicU32::new(0));
        let setup_at = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&order);
        let stamp = Arc::clone(&setup_at);
        let owner = OwnerThread::<()>::start(move || {
            stamp.store(seen.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
        });
        let job_at = owner.run(move |_| order.fetch_add(1, Ordering::SeqCst) + 1).expect("job");
        assert_eq!(setup_at.load(Ordering::SeqCst), 1);
        assert_eq!(job_at, 2);
    }

    #[test]
    fn a_closed_engine_refuses_work_rather_than_blocking() {
        let mut owner = OwnerThread::<()>::start(|| ());
        owner.run(|_| ()).expect("works while open");
        owner.close();
        assert_eq!(
            owner.run(|_| ()).expect_err("refused once closed").code,
            EngineErrorCode::DocNotOpen,
        );
        // Idempotent, per the lifecycle contract.
        owner.close();
        assert!(owner.is_closed());
    }

    #[test]
    fn close_drains_work_that_was_already_accepted() {
        let counter = Arc::new(AtomicU32::new(0));
        let mut owner = OwnerThread::<()>::start(|| ());
        for _ in 0..50 {
            let c = Arc::clone(&counter);
            owner.run(move |_| c.fetch_add(1, Ordering::SeqCst)).expect("queued");
        }
        owner.close();
        assert_eq!(counter.load(Ordering::SeqCst), 50);
    }

    #[test]
    fn a_panicking_job_does_not_take_the_engine_with_it() {
        // `catch_unwind` belongs to the job, not to the loop, so a job that
        // panics kills the owner thread. What matters is that the caller
        // learns rather than hanging on a channel nobody will answer.
        let owner = OwnerThread::<()>::start(|| ());
        let refused = owner
            .run(|_: &mut ()| panic!("boom"))
            .expect_err("a panicked job reports rather than deadlocking");
        assert_eq!(refused.code, EngineErrorCode::RuntimeUnavailable);
    }

    #[test]
    fn state_is_created_used_and_dropped_on_the_owner_thread() {
        // The whole reason `S` is a parameter rather than a field: it may be
        // `!Send`, and a `Rc` is the cheapest stand-in for a PDFium handle
        // that the compiler will refuse to move between threads.
        let mut owner = OwnerThread::start(|| std::rc::Rc::new(std::cell::Cell::new(0u32)));
        owner.run(|state| state.set(state.get() + 1)).expect("first");
        owner.run(|state| state.set(state.get() + 1)).expect("second");
        let seen = owner.run(|state| state.get()).expect("read");
        assert_eq!(seen, 2, "the same state is handed to every job");
        owner.close();
    }
}
