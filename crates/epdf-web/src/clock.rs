//! The kernel's host clock on a page: `createKernel({ clock })`.
//!
//! Timers stay on the runtime's virtual clock (`TimerClock`), which follows
//! wall time at every entry (`runtime`). Frames come from the host clock's
//! `nextFrame(run)` when it has one (`browserClock()` over
//! `requestAnimationFrame`), as the TypeScript kernel's `ctx.clock` takes
//! them; without one the clock has no frames, and plugins skip animation.
//!
//! One frame request is outstanding at a time, through one JavaScript
//! function this clock owns: every `next_frame` asked before it fires runs in
//! that frame, with the host's frame time, as each would with its own
//! request. A frame nothing waits for any more is cancelled.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use epdf_kernel::Unsubscribe;
use epdf_kernel::clock::{Cancel, HostClock, TimerClock};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::runtime::entry;

type Run = Box<dyn FnOnce(f64)>;

/// What the host calls with a frame's time.
type OnFrame = Closure<dyn FnMut(JsValue)>;

struct Frames {
    /// The host clock's `nextFrame(run)`, answering its canceller.
    next_frame: js_sys::Function,
    /// What runs in the next frame, by request id, in request order.
    waiting: RefCell<Vec<(u64, Run)>>,
    next_id: Cell<u64>,
    /// The outstanding request's canceller, while one is.
    requested: RefCell<Option<JsValue>>,
    /// The function every request hands the host. Set once, right after
    /// this is built: it holds this weakly.
    on_frame: RefCell<Option<OnFrame>>,
}

impl Frames {
    fn request(&self) {
        if self.requested.borrow().is_some() {
            return;
        }
        let on_frame = self.on_frame.borrow();
        let Some(on_frame) = on_frame.as_ref() else { return };
        let cancel = self.next_frame.call1(&JsValue::UNDEFINED, on_frame.as_ref().unchecked_ref());
        *self.requested.borrow_mut() = Some(cancel.unwrap_or(JsValue::UNDEFINED));
    }

    fn cancel_request(&self) {
        let Some(cancel) = self.requested.borrow_mut().take() else { return };
        if let Some(cancel) = cancel.dyn_ref::<js_sys::Function>() {
            let _ = cancel.call0(&JsValue::UNDEFINED);
        }
    }

    fn fire(&self, time: f64) {
        self.requested.borrow_mut().take();
        let due = std::mem::take(&mut *self.waiting.borrow_mut());
        for (_, run) in due {
            run(time);
        }
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        self.cancel_request();
    }
}

/// The page's [`HostClock`]: [`TimerClock`] plus, given a `nextFrame`, its
/// frames.
pub struct PageClock {
    frames: Option<Rc<Frames>>,
}

impl PageClock {
    /// `next_frame` is the host clock's `nextFrame`, or `None` for a clock
    /// without frames.
    pub fn new(next_frame: Option<js_sys::Function>) -> Self {
        let frames = next_frame.map(|next_frame| {
            let frames = Rc::new(Frames {
                next_frame,
                waiting: RefCell::new(Vec::new()),
                next_id: Cell::new(0),
                requested: RefCell::new(None),
                on_frame: RefCell::new(None),
            });
            let weak: Weak<Frames> = Rc::downgrade(&frames);
            let on_frame = OnFrame::new(move |time: JsValue| {
                let Some(frames) = weak.upgrade() else { return };
                let time = time.as_f64().unwrap_or_else(js_sys::Date::now);
                entry(|| frames.fire(time));
            });
            *frames.on_frame.borrow_mut() = Some(on_frame);
            frames
        });
        PageClock { frames }
    }
}

impl HostClock for PageClock {
    fn now(&self) -> f64 {
        TimerClock.now()
    }

    fn after(&self, ms: f64, run: Box<dyn FnOnce()>) -> Cancel {
        TimerClock.after(ms, run)
    }

    fn has_frames(&self) -> bool {
        self.frames.is_some()
    }

    fn next_frame(&self, run: Run) -> Cancel {
        let Some(frames) = &self.frames else { return TimerClock.next_frame(run) };
        let id = frames.next_id.get();
        frames.next_id.set(id + 1);
        frames.waiting.borrow_mut().push((id, run));
        frames.request();
        let weak = Rc::downgrade(frames);
        Unsubscribe::new(move || {
            let Some(frames) = weak.upgrade() else { return };
            let empty = {
                let mut waiting = frames.waiting.borrow_mut();
                waiting.retain(|(each, _)| *each != id);
                waiting.is_empty()
            };
            if empty {
                frames.cancel_request();
            }
        })
    }
}
