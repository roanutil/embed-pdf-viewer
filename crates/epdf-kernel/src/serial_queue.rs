//! `serial-queue.ts`: operations run one at a time, in submission order, and
//! a failure never poisons the ones after it.
//!
//! Direction law for nested queues: an operation on queue A may enqueue into
//! queue B and await it, never the reverse on the same pair.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::{Error, code, plugin_error};
use crate::rt::{self, Promise};
use crate::signal::AbortSignal;

#[derive(Clone)]
pub struct SerialQueue {
    tail: Rc<RefCell<Promise<()>>>,
    /// `createSerialQueue(capability)`: names the plugin in the error a
    /// skipped operation rejects with.
    capability: Rc<str>,
}

impl Default for SerialQueue {
    fn default() -> Self {
        SerialQueue::for_capability("")
    }
}

impl SerialQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// `createSerialQueue(capability)`.
    pub fn for_capability(capability: &str) -> Self {
        SerialQueue {
            tail: Rc::new(RefCell::new(Promise::resolved(()))),
            capability: Rc::from(capability),
        }
    }

    /// `queue(operation, { signal })`: an operation whose signal fired while
    /// it waited never starts. It rejects `operation-cancelled` naming the
    /// queue's capability, with the signal's raw reason as the cause. Once it
    /// has started, cancelling is up to the operation (`cancellable`).
    pub fn run_with<T, F>(&self, operation: F, signal: Option<AbortSignal>) -> Promise<T>
    where
        T: Clone + 'static,
        F: FnOnce() -> Result<Promise<T>, Error> + 'static,
    {
        let capability = self.capability.clone();
        self.run(move || match signal.filter(AbortSignal::aborted) {
            // `Promise.reject(...)`, not a throw: `tail.then(start, start)`
            // adopts what `start` returns.
            Some(signal) => Ok(Promise::rejected(plugin_error(
                code::OPERATION_CANCELLED,
                &capability,
                "cancelled before it started",
                signal.reason_value(),
                None,
            ))),
            None => operation(),
        })
    }

    /// `queue.idle()`: resolves once every operation queued so far has
    /// finished, however it ended.
    pub fn idle(&self) -> Promise<()> {
        self.tail.borrow().clone()
    }

    /// `tail.then(operation, operation)`: starts one turn after the previous
    /// operation settles, whichever way it settled.
    ///
    /// The operation is the TypeScript's `() => Promise<T>`: it returns the
    /// promise of the work it started (usually an `async` function's), or
    /// throws. A throw rejects the result in the same turn. A returned promise
    /// resolves the result the way `then` adopts a thenable: one turn for the
    /// resolve-thenable job, then one for the reaction, so the result settles
    /// two turns after an already-settled promise and one turn after a later
    /// one. Awaiting the work directly would put every queued operation's
    /// caller two turns ahead of the TypeScript.
    pub fn run<T, F>(&self, operation: F) -> Promise<T>
    where
        T: Clone + 'static,
        F: FnOnce() -> Result<Promise<T>, Error> + 'static,
    {
        let previous = self.tail.borrow().clone();
        let result = Promise::spawn(async move {
            let _ = previous.await;
            let promise = operation()?;
            rt::tick().await;
            promise.reaction().await
        });
        let done = result.clone();
        *self.tail.borrow_mut() = Promise::spawn(async move {
            let _ = done.await;
            Ok(())
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn runs_in_order_and_survives_failures() {
        rt::reset();
        let queue = SerialQueue::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
        let (gate, open) = Promise::<()>::pending();
        let first = queue.run(move || {
            Ok(Promise::spawn(async move {
                gate.await?;
                l1.borrow_mut().push(1);
                Err::<(), _>(Error::new("first failed"))
            }))
        });
        let second = queue.run(move || {
            Ok(Promise::spawn(async move {
                l2.borrow_mut().push(2);
                Ok(2)
            }))
        });
        let third = queue.run(move || -> Result<Promise<()>, Error> {
            l3.borrow_mut().push(3);
            Err(Error::new("third threw"))
        });
        rt::run_until_stalled();
        assert!(log.borrow().is_empty());
        open.resolve(());
        rt::run_until_stalled();
        assert_eq!(*log.borrow(), [1, 2, 3]);
        assert_eq!(first.peek().unwrap().unwrap_err().message, "first failed");
        assert_eq!(second.peek().unwrap().unwrap(), 2);
        assert_eq!(third.peek().unwrap().unwrap_err().message, "third threw");
    }

    #[test]
    fn a_returned_promise_is_adopted_in_two_turns() {
        // `q(async () => 1).then(log)` beside a chain of bare ticks: the
        // operation runs in turn 1, the result settles in turn 3, and its
        // reaction runs in turn 4.
        rt::reset();
        let queue = SerialQueue::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let result = queue.run(|| Ok(Promise::resolved(1)));
        let l = log.clone();
        rt::spawn(async move {
            let _ = result.await;
            l.borrow_mut().push("result");
        });
        let l = log.clone();
        rt::spawn(async move {
            for turn in 1..=4 {
                rt::tick().await;
                l.borrow_mut().push(["t1", "t2", "t3", "t4"][turn - 1]);
            }
        });
        rt::run_until_stalled();
        assert_eq!(*log.borrow(), ["t1", "t2", "t3", "result", "t4"]);
    }

    /// Logs `name` when `promise` settles, then runs a chain of bare ticks
    /// beside it, as the TypeScript scenarios below measured them.
    fn log_turns(log: &Rc<RefCell<Vec<String>>>, watched: Vec<(&'static str, Promise<()>)>) {
        for (name, promise) in watched {
            let l = log.clone();
            rt::spawn(async move {
                let outcome = promise.await;
                l.borrow_mut().push(match outcome {
                    Ok(()) => name.to_string(),
                    Err(e) => format!(
                        "{name}:{}",
                        e.member("cause").map(|c| c.to_string()).unwrap_or_default()
                    ),
                });
            });
        }
        let l = log.clone();
        rt::spawn(async move {
            for turn in 1..=6 {
                rt::tick().await;
                l.borrow_mut().push(format!("t{turn}"));
            }
        });
        rt::run_until_stalled();
    }

    #[test]
    fn a_signal_that_fired_skips_the_operation_on_the_typescript_turns() {
        // serial-queue.ts, measured in node: a pre-aborted signal rejects
        // after t3 and `idle()` resolves after t4:
        // ["t1","t2","t3","rejected:why","t4","idle","t5","t6"].
        rt::reset();
        let queue = SerialQueue::for_capability("form");
        let signal = AbortSignal::new();
        signal.abort_value(Error::new("why"), Value::from("why"));
        let ran = Rc::new(RefCell::new(false));
        let r = ran.clone();
        let skipped = queue.run_with(
            move || {
                *r.borrow_mut() = true;
                Ok(Promise::resolved(()))
            },
            Some(signal),
        );
        let log = Rc::new(RefCell::new(Vec::new()));
        log_turns(&log, vec![("rejected", skipped.clone()), ("idle", queue.idle())]);
        assert!(!*ran.borrow());
        assert_eq!(*log.borrow(), ["t1", "t2", "t3", "rejected:\"why\"", "t4", "idle", "t5", "t6"]);
        let error = skipped.peek().unwrap().unwrap_err();
        assert_eq!(error.code(), Some(code::OPERATION_CANCELLED));
        assert_eq!(error.member("capability"), Some(&Value::from("form")));
        assert_eq!(error.message, "cancelled before it started");
    }

    #[test]
    fn a_signal_that_fires_while_waiting_skips_the_operation() {
        // ["t1","t2","t3","first","t4","t5","t6","rejected:late","idle"]:
        // the first operation's gate opens in the same turn the second's
        // signal fires.
        rt::reset();
        let queue = SerialQueue::for_capability("form");
        let signal = AbortSignal::new();
        let (gate, open) = Promise::<()>::pending();
        let first = queue.run(move || Ok(gate));
        let ran = Rc::new(RefCell::new(false));
        let r = ran.clone();
        let second = queue.run_with(
            move || {
                *r.borrow_mut() = true;
                Ok(Promise::resolved(()))
            },
            Some(signal.clone()),
        );
        let idle = queue.idle();
        signal.abort_value(Error::new("late"), Value::from("late"));
        open.resolve(());
        let log = Rc::new(RefCell::new(Vec::new()));
        log_turns(&log, vec![("first", first), ("rejected", second), ("idle", idle)]);
        assert!(!*ran.borrow());
        assert_eq!(
            *log.borrow(),
            ["t1", "t2", "t3", "first", "t4", "t5", "t6", "rejected:\"late\"", "idle"]
        );
    }

    #[test]
    fn idle_resolves_on_the_typescript_turns() {
        // Empty: ["idle","t1",...]. After one operation:
        // ["t1","t2","t3","result","t4","idle","t5","t6"].
        rt::reset();
        let queue = SerialQueue::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        log_turns(&log, vec![("idle", queue.idle())]);
        assert_eq!(*log.borrow(), ["idle", "t1", "t2", "t3", "t4", "t5", "t6"]);

        rt::reset();
        let queue = SerialQueue::new();
        let result = queue.run(|| Ok(Promise::resolved(())));
        let log = Rc::new(RefCell::new(Vec::new()));
        log_turns(&log, vec![("result", result), ("idle", queue.idle())]);
        assert_eq!(*log.borrow(), ["t1", "t2", "t3", "result", "t4", "idle", "t5", "t6"]);
    }
}
