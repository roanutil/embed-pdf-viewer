use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use epdf_engine::cancel::CancelToken;
use epdf_engine::dispatch::OwnerThread;
use epdf_engine::error::EngineError;
use epdf_engine::host::HostResponse;

use crate::abi::{Failure, Status, response_error};
use crate::buffers::{ByteBudget, OwnedResult, Reservation};
use crate::config::Limits;
use crate::jobs::{Completion, Job};

const OPEN: u8 = 1;
const CLOSING: u8 = 2;
const POISONED: u8 = 3;
const CLOSED: u8 = 4;

struct Control {
    phase: AtomicU8,
    finished: Mutex<bool>,
    done: Condvar,
}

struct OwnedState<S> {
    state: Option<S>,
    control: Arc<Control>,
}

impl<S> Drop for OwnedState<S> {
    fn drop(&mut self) {
        struct Finished<'a>(&'a Control);
        impl Drop for Finished<'_> {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    self.0.phase.store(POISONED, Ordering::Release);
                } else if self.0.phase.load(Ordering::Acquire) != POISONED {
                    self.0.phase.store(CLOSED, Ordering::Release);
                }
                *self.0.finished.lock().unwrap_or_else(|p| p.into_inner()) = true;
                self.0.done.notify_all();
            }
        }
        let _finished = Finished(&self.control);
        drop(self.state.take());
    }
}

pub(crate) struct Admission {
    control: Arc<Control>,
    _slot: Reservation,
    _input: Reservation,
}

/// Admission and lifetime control around confined state; it never contains S.
pub(crate) struct Engine<S: 'static> {
    owner: Mutex<OwnerThread<Option<OwnedState<S>>>>,
    control: Arc<Control>,
    slots: Arc<ByteBudget>,
    input: Arc<ByteBudget>,
    output: Arc<ByteBudget>,
    pub limits: Limits,
}

impl<S: 'static> Engine<S> {
    pub(crate) fn start(
        limits: Limits,
        setup: impl FnOnce() -> Result<S, Failure> + Send + 'static,
    ) -> Result<Arc<Self>, Failure> {
        limits.validate().map_err(|s| Failure::new(s, "invalid engine limits"))?;
        let control = Arc::new(Control {
            phase: AtomicU8::new(OPEN),
            finished: Mutex::new(false),
            done: Condvar::new(),
        });
        let owned_control = control.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let mut owner = catch_unwind(AssertUnwindSafe(|| {
            OwnerThread::start(move || match catch_unwind(AssertUnwindSafe(setup)) {
                Ok(Ok(state)) => {
                    let state = OwnedState { state: Some(state), control: owned_control };
                    let _ = tx.send(Ok(()));
                    Some(state)
                }
                Ok(Err(error)) => {
                    let _ = tx.send(Err(error));
                    None
                }
                Err(_) => {
                    let _ = tx.send(Err(Failure::new(Status::Panicked, "engine setup panicked")));
                    None
                }
            })
        }))
        .map_err(|_| Failure::new(Status::RuntimeUnavailable, "owner thread could not start"))?;
        let started = rx.recv().unwrap_or_else(|_| {
            Err(Failure::new(Status::RuntimeUnavailable, "owner setup stopped"))
        });
        if let Err(error) = started {
            owner.close();
            return Err(error);
        }
        Ok(Arc::new(Self {
            owner: Mutex::new(owner),
            control,
            slots: ByteBudget::new(u64::from(limits.max_pending_jobs)),
            input: ByteBudget::new(limits.max_queued_input_bytes),
            output: ByteBudget::new(limits.max_retained_result_bytes),
            limits,
        }))
    }

    fn admission_status(&self) -> Result<(), Status> {
        match self.control.phase.load(Ordering::Acquire) {
            OPEN => Ok(()),
            POISONED => Err(Status::Panicked),
            _ => Err(Status::Closed),
        }
    }

    /// Reserves capacity before copies; submission is the acceptance point.
    pub(crate) fn reserve(&self, bytes: u64) -> Result<Admission, Status> {
        self.admission_status()?;
        let slot = self.slots.reserve(1).map_err(|_| Status::Busy)?;
        let input = self.input.reserve(bytes)?;
        self.admission_status()?;
        Ok(Admission { control: self.control.clone(), _slot: slot, _input: input })
    }

    pub(crate) fn submit(
        self: &Arc<Self>,
        admission: Admission,
        action: impl FnOnce(&mut S, &CancelToken) -> Result<HostResponse, EngineError> + Send + 'static,
    ) -> Result<Arc<Job>, Status> {
        if !Arc::ptr_eq(&admission.control, &self.control) {
            return Err(Status::InvalidHandle);
        }
        let owner = self.owner.lock().unwrap_or_else(|p| p.into_inner());
        self.admission_status()?;
        let job = Job::new();
        let held = job.clone();
        let weak = Arc::downgrade(self);
        let control = self.control.clone();
        let output = self.output.clone();
        let limit = self.limits.max_result_bytes;
        owner
            .submit(move |state| {
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let (status, response) = if control.phase.load(Ordering::Acquire) == POISONED {
                        let error = Failure::new(Status::Panicked, "engine is poisoned");
                        (error.status, error.response)
                    } else {
                        let token = held.token();
                        let response = (|| {
                            token.check()?;
                            let state =
                                state.as_mut().and_then(|s| s.state.as_mut()).ok_or_else(|| {
                                    epdf_engine::error::EngineError::with_message(
                                        epdf_engine::error::EngineErrorCode::RuntimeUnavailable,
                                        "owner state is unavailable",
                                    )
                                })?;
                            action(state, &token)
                        })();
                        (Status::Ok, response.unwrap_or_else(response_error))
                    };
                    let completion =
                        match OwnedResult::from_response(&response, limit, Some(&output)) {
                            Ok(result) => Completion { status, result: Some(result), document: 0 },
                            Err(failure) => Completion {
                                status: if status == Status::Ok { failure } else { status },
                                result: None,
                                document: 0,
                            },
                        };
                    drop(admission);
                    held.complete(completion);
                }));
                if outcome.is_err() {
                    control.phase.store(POISONED, Ordering::Release);
                    if let Some(engine) = weak.upgrade() {
                        engine.request_shutdown();
                    }
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        let error = Failure::new(Status::Panicked, "owner job panicked");
                        OwnedResult::from_response(&error.response, limit, Some(&output))
                    }))
                    .ok()
                    .and_then(Result::ok);
                    held.complete(Completion { status: Status::Panicked, result, document: 0 });
                }
            })
            .map_err(|_| Status::RuntimeUnavailable)?;
        Ok(job)
    }

    pub(crate) fn request_shutdown(&self) {
        let mut owner = self.owner.lock().unwrap_or_else(|p| p.into_inner());
        let _ =
            self.control.phase.compare_exchange(OPEN, CLOSING, Ordering::AcqRel, Ordering::Acquire);
        owner.shutdown_detached();
    }

    pub(crate) fn close(&self, timeout_ms: u32) -> Status {
        self.request_shutdown();
        let deadline = (timeout_ms != u32::MAX)
            .then(|| Instant::now() + Duration::from_millis(u64::from(timeout_ms)));
        let mut finished = self.control.finished.lock().unwrap_or_else(|p| p.into_inner());
        while !*finished {
            finished = match deadline {
                None => self.control.done.wait(finished).unwrap_or_else(|p| p.into_inner()),
                Some(deadline) => {
                    let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                        return Status::Timeout;
                    };
                    self.control
                        .done
                        .wait_timeout(finished, left)
                        .unwrap_or_else(|p| p.into_inner())
                        .0
                }
            };
        }
        Status::Ok
    }
}

impl<S: 'static> Drop for Engine<S> {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::mpsc::{channel, sync_channel};
    use std::time::Duration;

    fn response(value: u32) -> HostResponse {
        HostResponse { json: format!("{{\"result\":{value}}}"), payloads: Vec::new() }
    }

    fn json(job: &Arc<Job>) -> serde_json::Value {
        let output = job.wait(u32::MAX).unwrap();
        assert_eq!(output.status, Status::Ok);
        let bytes = &output.result.unwrap().bytes;
        let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[4..4 + len]).unwrap()
    }

    #[test]
    fn saturated_admission_does_not_block_cancel_or_close() {
        let engine = Engine::start(Limits::default(), || Ok(Rc::new(Cell::new(0u32)))).unwrap();
        let (started_tx, started_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let first = engine
            .submit(engine.reserve(0).unwrap(), move |_, _| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(response(7))
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut queued = Vec::new();
        for _ in 0..15 {
            queued.push(
                engine
                    .submit(engine.reserve(0).unwrap(), |state, _| {
                        state.set(state.get() + 1);
                        Ok(response(state.get()))
                    })
                    .unwrap(),
            );
        }
        assert!(matches!(engine.reserve(0), Err(Status::Busy)));
        queued[0].cancel();
        queued[1].abandon();
        assert_eq!(engine.close(0), Status::Timeout);
        assert!(matches!(engine.reserve(0), Err(Status::Closed)));
        release_tx.send(()).unwrap();
        assert_eq!(json(&first)["result"], 7);
        assert_eq!(json(&queued[0])["error"]["code"], "Aborted");
        assert!(matches!(queued[1].wait(0), Err(Status::InvalidHandle)));
        for (index, job) in queued.iter().skip(2).enumerate() {
            assert_eq!(json(job)["result"], index + 1);
        }
        assert_eq!(engine.close(u32::MAX), Status::Ok);
    }

    #[test]
    fn input_bytes_are_bounded_and_released_when_a_reservation_is_dropped() {
        let limits = Limits { max_queued_input_bytes: 4, ..Limits::default() };
        let engine = Engine::start(limits, || Ok(())).unwrap();
        let held = engine.reserve(4).unwrap();
        assert!(matches!(engine.reserve(1), Err(Status::LimitExceeded)));
        drop(held);
        assert!(engine.reserve(4).is_ok());
        assert_eq!(engine.close(u32::MAX), Status::Ok);
    }

    #[test]
    fn a_panic_is_still_reported_when_the_error_result_budget_is_exhausted() {
        let limits = Limits { max_retained_result_bytes: 1, ..Limits::default() };
        let engine = Engine::start(limits, || Ok(())).unwrap();
        let job = engine.submit(engine.reserve(0).unwrap(), |_, _| panic!("job failed")).unwrap();
        let completion = job.wait(2_000).unwrap();
        assert_eq!(completion.status, Status::Panicked);
        assert!(completion.result.is_none());
        assert_eq!(engine.close(u32::MAX), Status::Ok);
    }

    #[test]
    fn retained_results_fail_fast_at_the_budget_and_release_capacity_on_drop() {
        let limits = Limits { max_retained_result_bytes: 24, ..Limits::default() };
        let engine = Engine::start(limits, || Ok(())).unwrap();
        let first = engine.submit(engine.reserve(0).unwrap(), |_, _| Ok(response(7))).unwrap();
        let held = first.wait(2_000).unwrap().result.unwrap();
        let second = engine.submit(engine.reserve(0).unwrap(), |_, _| Ok(response(8))).unwrap();
        let refused = second.wait(2_000).unwrap();
        assert_eq!(refused.status, Status::LimitExceeded);
        assert!(refused.result.is_none());
        drop(held);
        let third = engine.submit(engine.reserve(0).unwrap(), |_, _| Ok(response(9))).unwrap();
        assert_eq!(json(&third)["result"], 9);
        assert_eq!(engine.close(u32::MAX), Status::Ok);
    }

    #[test]
    fn a_panicking_job_completes_waiters_and_prevents_queued_state_access() {
        let engine = Engine::start(Limits::default(), || Ok(())).unwrap();
        let (started_tx, started_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let first = engine
            .submit(engine.reserve(0).unwrap(), move |_, _| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                panic!("owner job panic")
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let visits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let observed = visits.clone();
        let second = engine
            .submit(engine.reserve(0).unwrap(), move |_, _| {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(response(1))
            })
            .unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(first.wait(u32::MAX).unwrap().status, Status::Panicked);
        assert_eq!(second.wait(u32::MAX).unwrap().status, Status::Panicked);
        assert_eq!(visits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(matches!(engine.reserve(0), Err(Status::Panicked)));
        assert_eq!(engine.close(u32::MAX), Status::Ok);
    }

    #[test]
    fn dropping_an_engine_detaches_and_disposes_state_only_on_its_owner_thread() {
        struct State(std::sync::mpsc::Sender<std::thread::ThreadId>, Rc<()>);
        impl Drop for State {
            fn drop(&mut self) {
                let _ = self.0.send(std::thread::current().id());
            }
        }
        let (drop_tx, drop_rx) = channel();
        let engine =
            Engine::start(Limits::default(), move || Ok(State(drop_tx, Rc::new(())))).unwrap();
        let (started_tx, started_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let job = engine
            .submit(engine.reserve(0).unwrap(), move |state, _| {
                assert_eq!(Rc::strong_count(&state.1), 1);
                started_tx.send(std::thread::current().id()).unwrap();
                release_rx.recv().unwrap();
                Ok(response(23))
            })
            .unwrap();
        let owner = started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (released_tx, released_rx) = channel();
        let release = std::thread::spawn(move || {
            drop(engine);
            released_tx.send(()).unwrap();
        });
        let detached = released_rx.recv_timeout(Duration::from_secs(2));
        release_tx.send(()).unwrap();
        release.join().unwrap();
        assert!(detached.is_ok());
        assert_eq!(json(&job)["result"], 23);
        assert_eq!(drop_rx.recv_timeout(Duration::from_secs(2)).unwrap(), owner);
        assert_ne!(owner, std::thread::current().id());
    }
}
