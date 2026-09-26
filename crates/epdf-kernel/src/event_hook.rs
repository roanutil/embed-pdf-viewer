//! `event-hook.ts`: the one event shape a capability exposes.
//!
//! Synchronous fan-out over a snapshot of the listeners (one added or
//! removed during an emit does not affect that emit); a failing listener is
//! reported and never breaks its siblings; after `dispose`, `on` returns an
//! inert unsubscribe. No value cache, no replay.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::Error;
use crate::signal::AbortSignal;
use crate::store::Unsubscribe;

type Listener<T> = Rc<dyn Fn(&T) -> Result<(), Error>>;

struct Inner<T> {
    listeners: Option<Vec<(u64, Listener<T>)>>,
    next: u64,
}

pub struct EventHookSource<T> {
    inner: Rc<RefCell<Inner<T>>>,
    on_error: Option<Rc<dyn Fn(&Error)>>,
}

impl<T> Clone for EventHookSource<T> {
    fn clone(&self) -> Self {
        EventHookSource { inner: self.inner.clone(), on_error: self.on_error.clone() }
    }
}

impl<T: 'static> EventHookSource<T> {
    pub fn new(on_error: Option<Rc<dyn Fn(&Error)>>) -> Self {
        EventHookSource {
            inner: Rc::new(RefCell::new(Inner { listeners: Some(Vec::new()), next: 0 })),
            on_error,
        }
    }

    /// The subscribe-only half, as a capability exposes it.
    pub fn on(&self, listener: impl Fn(&T) -> Result<(), Error> + 'static) -> Unsubscribe {
        self.on_signal(listener, None)
    }

    /// `on(listener, {signal})`: the signal's abort unsubscribes, and a
    /// pre-aborted signal never registers.
    pub fn on_signal(
        &self,
        listener: impl Fn(&T) -> Result<(), Error> + 'static,
        signal: Option<&AbortSignal>,
    ) -> Unsubscribe {
        let id = {
            let mut inner = self.inner.borrow_mut();
            if inner.listeners.is_none() || signal.is_some_and(AbortSignal::aborted) {
                return Unsubscribe::noop();
            }
            let id = inner.next;
            inner.next += 1;
            inner.listeners.as_mut().expect("live").push((id, Rc::new(listener)));
            id
        };
        let weak = Rc::downgrade(&self.inner);
        let off = Unsubscribe::new(move || {
            if let Some(inner) = weak.upgrade()
                && let Some(listeners) = inner.borrow_mut().listeners.as_mut()
            {
                listeners.retain(|(i, _)| *i != id);
            }
        });
        if let Some(signal) = signal {
            let release = off.clone();
            signal.on_abort(Rc::new(move |_| release.call()));
        }
        off
    }

    pub fn emit(&self, event: &T) {
        let snapshot: Vec<Listener<T>> = match &self.inner.borrow().listeners {
            Some(listeners) => listeners.iter().map(|(_, l)| l.clone()).collect(),
            None => return,
        };
        for listener in snapshot {
            if let Err(error) = listener(event)
                && let Some(on_error) = &self.on_error
            {
                on_error(&error);
            }
        }
    }

    pub fn dispose(&self) {
        self.inner.borrow_mut().listeners = None;
    }

    pub fn listener_count(&self) -> usize {
        self.inner.borrow().listeners.as_ref().map_or(0, Vec::len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // Mirrors packages/core/main/test/event-hook.test.ts.

    #[test]
    fn fans_out_in_subscription_order_over_a_snapshot() {
        let hook = EventHookSource::<i32>::new(None);
        let log = Rc::new(RefCell::new(Vec::new()));
        let late: Rc<RefCell<Option<Unsubscribe>>> = Rc::default();
        let (l1, l2, h2, late2) = (log.clone(), log.clone(), hook.clone(), late.clone());
        hook.on(move |e| {
            l1.borrow_mut().push(format!("a{e}"));
            let l3 = l2.clone();
            *late2.borrow_mut() = Some(h2.on(move |e| {
                l3.borrow_mut().push(format!("late{e}"));
                Ok(())
            }));
            Ok(())
        });
        hook.emit(&1);
        assert_eq!(*log.borrow(), ["a1"]);
    }

    #[test]
    fn a_failing_listener_is_reported_and_isolated() {
        let reported = Rc::new(RefCell::new(Vec::new()));
        let r = reported.clone();
        let hook = EventHookSource::<()>::new(Some(Rc::new(move |e: &Error| {
            r.borrow_mut().push(e.message.clone())
        })));
        let ran = Rc::new(RefCell::new(false));
        let ran2 = ran.clone();
        hook.on(|_| Err(Error::new("boom")));
        hook.on(move |_| {
            *ran2.borrow_mut() = true;
            Ok(())
        });
        hook.emit(&());
        assert_eq!(*reported.borrow(), ["boom"]);
        assert!(*ran.borrow());
    }

    #[test]
    fn dispose_makes_on_inert_and_unsubscribe_harmless() {
        let hook = EventHookSource::<()>::new(None);
        let count = Rc::new(RefCell::new(0));
        let c = count.clone();
        let off = hook.on(move |_| {
            *c.borrow_mut() += 1;
            Ok(())
        });
        hook.emit(&());
        off.call();
        off.call();
        hook.emit(&());
        assert_eq!(*count.borrow(), 1);
        hook.dispose();
        let inert = hook.on(|_| panic!("disposed"));
        hook.emit(&());
        inert.call();
    }
}
