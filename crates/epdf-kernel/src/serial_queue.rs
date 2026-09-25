//! `serial-queue.ts`: operations run one at a time, in submission order, and
//! a failure never poisons the ones after it.
//!
//! Direction law for nested queues: an operation on queue A may enqueue into
//! queue B and await it, never the reverse on the same pair.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use crate::error::Error;
use crate::rt::Promise;

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
    pub fn run<T, F, Fut>(&self, operation: F) -> Promise<T>
    where
        T: Clone + 'static,
        F: FnOnce() -> Fut + 'static,
        Fut: Future<Output = Result<T, Error>> + 'static,
    {
        let previous = self.tail.borrow().clone();
        let result = Promise::spawn(async move {
            let _ = previous.await;
            operation().await
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
    use crate::rt;

    #[test]
    fn runs_in_order_and_survives_failures() {
        rt::reset();
        let queue = SerialQueue::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
        let (gate, open) = Promise::<()>::pending();
        let first = queue.run(move || async move {
            gate.await?;
            l1.borrow_mut().push(1);
            Err::<(), _>(Error::new("first failed"))
        });
        let second = queue.run(move || async move {
            l2.borrow_mut().push(2);
            Ok(2)
        });
        queue.run(move || async move {
            l3.borrow_mut().push(3);
            Ok(())
        });
        rt::run_until_stalled();
        assert!(log.borrow().is_empty());
        open.resolve(());
        rt::run_until_stalled();
        assert_eq!(*log.borrow(), [1, 2, 3]);
        assert_eq!(first.peek().unwrap().unwrap_err().message, "first failed");
        assert_eq!(second.peek().unwrap().unwrap(), 2);
    }
}
