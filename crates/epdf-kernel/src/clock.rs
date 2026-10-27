//! Time supplied by the host, owned by each plugin instance.
use crate::{rt, signal::AbortSignal, store::Unsubscribe};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
pub type Cancel = Unsubscribe;
pub trait HostClock {
    fn now(&self) -> f64;
    fn after(&self, ms: f64, run: Box<dyn FnOnce()>) -> Cancel;
    fn has_frames(&self) -> bool {
        false
    }
    fn next_frame(&self, run: Box<dyn FnOnce(f64)>) -> Cancel {
        let now = self.now();
        self.after(0.0, Box::new(move || run(now)))
    }
}
pub struct TimerClock;
impl HostClock for TimerClock {
    fn now(&self) -> f64 {
        rt::now()
    }
    fn after(&self, ms: f64, run: Box<dyn FnOnce()>) -> Cancel {
        let id = rt::set_timeout(ms, run);
        Unsubscribe::new(move || rt::clear_timer(id))
    }
    fn next_frame(&self, run: Box<dyn FnOnce(f64)>) -> Cancel {
        self.after(0.0, Box::new(move || run(rt::now())))
    }
}
#[derive(Clone)]
pub struct PluginClock {
    host: Rc<dyn HostClock>,
    lifetime: AbortSignal,
    pending: Rc<RefCell<Vec<(u64, Cancel)>>>,
    next: Rc<Cell<u64>>,
}
impl PluginClock {
    pub fn new(host: Rc<dyn HostClock>, lifetime: AbortSignal) -> Self {
        let pending: Rc<RefCell<Vec<(u64, Cancel)>>> = Rc::default();
        let weak = Rc::downgrade(&pending);
        lifetime.on_abort(Rc::new(move |_| {
            if let Some(p) = weak.upgrade() {
                let cancels = std::mem::take(&mut *p.borrow_mut());
                for (_, cancel) in cancels {
                    cancel.call();
                }
            }
        }));
        Self { host, lifetime, pending, next: Rc::new(Cell::new(0)) }
    }
    pub fn has_frames(&self) -> bool {
        self.host.has_frames()
    }
    pub fn now(&self) -> f64 {
        self.host.now()
    }
    fn own(
        &self,
        schedule: impl FnOnce(Box<dyn FnOnce()>) -> Cancel,
        run: Box<dyn FnOnce()>,
    ) -> Cancel {
        if self.lifetime.aborted() {
            return Cancel::noop();
        }
        let id = self.next.get();
        self.next.set(id + 1);
        let done = Rc::new(Cell::new(false));
        let cancel_host: Rc<RefCell<Option<Cancel>>> = Rc::default();
        let d = done.clone();
        let p = Rc::downgrade(&self.pending);
        let c = cancel_host.clone();
        let cancel = Cancel::new(move || {
            if d.replace(true) {
                return;
            }
            if let Some(p) = p.upgrade() {
                p.borrow_mut().retain(|(i, _)| *i != id);
            }
            if let Some(c) = c.borrow_mut().take() {
                c.call();
            }
        });
        self.pending.borrow_mut().push((id, cancel.clone()));
        let p = Rc::downgrade(&self.pending);
        let d = done.clone();
        let host = schedule(Box::new(move || {
            if d.replace(true) {
                return;
            }
            if let Some(p) = p.upgrade() {
                p.borrow_mut().retain(|(i, _)| *i != id);
            }
            run();
        }));
        if done.get() {
            host.call();
        } else {
            *cancel_host.borrow_mut() = Some(host);
        }
        cancel
    }
    pub fn after(&self, ms: f64, run: impl FnOnce() + 'static) -> Cancel {
        let host = self.host.clone();
        self.own(move |fire| host.after(ms, fire), Box::new(run))
    }
    pub fn next_frame(&self, run: impl FnOnce(f64) + 'static) -> Cancel {
        let host = self.host.clone();
        if !host.has_frames() {
            return self.after(0.0, move || run(host.now()));
        }
        let time = Rc::new(Cell::new(0.0));
        let t = time.clone();
        self.own(
            move |fire| {
                host.next_frame(Box::new(move |now| {
                    t.set(now);
                    fire();
                }))
            },
            Box::new(move || run(time.get())),
        )
    }
}
