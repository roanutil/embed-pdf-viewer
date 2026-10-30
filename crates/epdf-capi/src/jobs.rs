use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use epdf_engine::cancel::CancelToken;

use crate::abi::Status;
use crate::buffers::OwnedResult;

pub(crate) struct Completion {
    pub status: Status,
    pub result: Option<Arc<OwnedResult>>,
    pub document: u64,
}

enum State {
    Pending,
    Ready(Completion),
    Taken,
    Abandoned,
}

pub(crate) struct Job {
    token: CancelToken,
    state: Mutex<State>,
    ready: Condvar,
}

impl Job {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            token: CancelToken::never(),
            state: Mutex::new(State::Pending),
            ready: Condvar::new(),
        })
    }

    pub(crate) fn token(&self) -> CancelToken {
        self.token.clone()
    }

    pub(crate) fn cancel(&self) {
        self.token.cancel();
    }

    pub(crate) fn abandon(&self) {
        self.cancel();
        let old = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::replace(&mut *state, State::Abandoned)
        };
        self.ready.notify_all();
        drop(old);
    }

    pub(crate) fn complete(&self, value: Completion) {
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if matches!(*state, State::Pending) {
                *state = State::Ready(value);
            } else {
                return;
            }
        }
        self.ready.notify_all();
    }

    pub(crate) fn wait(&self, timeout_ms: u32) -> Result<Completion, Status> {
        let deadline = (timeout_ms != u32::MAX)
            .then(|| Instant::now() + Duration::from_millis(u64::from(timeout_ms)));
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            match &*state {
                State::Ready(_) => {
                    if let State::Ready(result) = std::mem::replace(&mut *state, State::Taken) {
                        return Ok(result);
                    }
                    return Err(Status::AlreadyTaken);
                }
                State::Taken => return Err(Status::AlreadyTaken),
                State::Abandoned => return Err(Status::InvalidHandle),
                State::Pending => {}
            }
            state = match deadline {
                None => self.ready.wait(state).unwrap_or_else(|p| p.into_inner()),
                Some(deadline) => {
                    let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                        return Err(Status::Timeout);
                    };
                    let (next, _) =
                        self.ready.wait_timeout(state, left).unwrap_or_else(|p| p.into_inner());
                    next
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_timeout_does_not_cancel_and_collection_happens_once() {
        let job = Job::new();
        assert!(matches!(job.wait(0), Err(Status::Timeout)));
        assert!(!job.token().is_cancelled());
        job.complete(Completion { status: Status::Ok, result: None, document: 17 });
        assert_eq!(job.wait(u32::MAX).unwrap().document, 17);
        assert!(matches!(job.wait(0), Err(Status::AlreadyTaken)));
    }

    #[test]
    fn cancellation_is_cooperative_and_abandonment_wakes_waiters() {
        let job = Job::new();
        job.cancel();
        assert!(job.token().is_cancelled());
        assert!(matches!(job.wait(0), Err(Status::Timeout)));
        let held = job.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            tx.send(held.wait(u32::MAX).err()).unwrap();
        });
        job.abandon();
        job.abandon();
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap(),
            Some(Status::InvalidHandle)
        );
        waiter.join().unwrap();
    }
}
