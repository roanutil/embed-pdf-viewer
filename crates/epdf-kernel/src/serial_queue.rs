//! `serial-queue.ts`: operations run one at a time, in submission order, and
//! a failure never poisons the ones after it.
//!
//! Direction law for nested queues: an operation on queue A may enqueue into
//! queue B and await it, never the reverse on the same pair.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::Error;
use crate::rt::{self, Promise};

#[derive(Clone)]
pub struct SerialQueue {
    tail: Rc<RefCell<Promise<()>>>,
}

impl Default for SerialQueue {
    fn default() -> Self {
        SerialQueue { tail: Rc::new(RefCell::new(Promise::resolved(()))) }
    }
}

impl SerialQueue {
    pub fn new() -> Self {
        Self::default()
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
}
