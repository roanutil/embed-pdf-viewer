//! G1. Cancellation tokens.
//!
//! Replaces `services/src/shared/abort.ts` and its 86 `throwIfAborted(signal)`
//! call sites. A token, not an `AbortSignal`: `AbortSignal` is a host object,
//! and every service method taking one is what makes the service layer
//! unportable (`multiplatform-port-hazards.md` 3.2).
//!
//! What cancellation means, per the four cases in
//! `multiplatform-plan-claude-remix.md` Section 7:
//!
//! 1. Queued, not started: the job is dropped, inputs released, nothing
//!    published. That is the scheduler's decision, not this token's.
//! 2. Cooperative, before any write: [`CancelToken::check`] between units of
//!    work. Mutators are cancellable up to the apply boundary and not past it.
//! 3. Non-preemptible: one synchronous native call, `FPDF_RenderPageBitmap`
//!    for instance. The wait ends; the work completes. A token cannot
//!    interrupt it and no operation may claim otherwise.
//! 4. After the first write: bookkeeping completes. Mutations are not
//!    rollback-atomic, so an `Aborted` error is never evidence that no write
//!    happened.
//!
//! On the web under Stage A the flag can only be set between jobs, because an
//! abort message is not observed while a synchronous handler runs
//! (`WorkerHost.ts:199`, `:225`). The type does not change; what changes is
//! who can set it and when.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{EngineError, EngineErrorCode};

/// A cancellation flag shared with whoever can cancel the job.
///
/// `Arc<AtomicBool>` rather than a channel: every check is on the operation's
/// hot path, between units of work, and must cost a relaxed load.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token nobody holds the other end of. For call sites that have no
    /// cancellation story yet, and for tests.
    pub fn never() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        // Release pairs with the Acquire in `is_cancelled`, so whatever the
        // canceller wrote before cancelling is visible to the worker that
        // observes the flag.
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// The port of `throwIfAborted`. Every call site that had one gets one of
    /// these, in the same place.
    pub fn check(&self) -> Result<(), EngineError> {
        if self.is_cancelled() {
            return Err(EngineError::with_message(
                EngineErrorCode::Aborted,
                "operation was cancelled",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_pass_until_cancelled_and_fail_from_then_on() {
        let token = CancelToken::never();
        assert!(token.check().is_ok());

        token.cancel();
        let err = token.check().unwrap_err();
        assert!(err.is(EngineErrorCode::Aborted));
        // Cancellation is sticky: a second check does not clear it.
        assert!(token.check().is_err());
    }

    #[test]
    fn a_clone_shares_the_flag_with_the_canceller() {
        let held_by_worker = CancelToken::never();
        let held_by_caller = held_by_worker.clone();
        held_by_caller.cancel();
        assert!(held_by_worker.is_cancelled());
    }

    #[test]
    fn crosses_threads_so_a_scheduler_can_cancel_running_work() {
        let token = CancelToken::never();
        let canceller = token.clone();
        std::thread::spawn(move || canceller.cancel()).join().unwrap();
        assert!(token.is_cancelled());
    }
}
