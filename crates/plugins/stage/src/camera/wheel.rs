//! `camera/wheel.ts`: the wheel, and whose it is. A scrolling element hands
//! the wheel to the page around it once it can't scroll that way. The Stage
//! is a camera, not a scroller, so it gives that answer itself: `wheel_by`
//! pans and returns true, or returns false for a wheel it leaves to the
//! page, which the surface binding then lets the host have.
//!
//! The answer holds for a whole stream (the wheels between two pauses, as
//! in `navigation/turn.rs`) and comes from its first wheel, the way a
//! browser picks what a scroll gesture scrolls. A stream is the Stage's
//! when it begins with somewhere to go. So scrolling down to the end stops
//! there, momentum included, and the next push scrolls the page.
//!
//! A trackpad leaves no pause before that next push: a swipe coasts for a
//! second or more, and the next one begins before it stops. So a new push
//! (`coasting.rs`) begins the next stream too. It is the Stage's without
//! asking when the one before it was still moving the pages.
//!
//! Page turns go by the same streams: a new push is the next one to turn a
//! page with, whether or not the wheel chains.

use super::coasting::Coasting;
use crate::navigation::turn::{Push, QUIET_MS};
use crate::prelude::*;

/// `createWheel`'s closure.
#[derive(Default)]
pub(crate) struct Wheel {
    /// Whether the stream in progress is the Stage's; `None` between
    /// streams.
    taken: Cell<Option<bool>>,
    /// The stream is moving the pages: it is the Stage's, and its last
    /// wheel had somewhere to go.
    moving: Cell<bool>,
    coasting: Cell<Coasting>,
    cancel_quiet: RefCell<Option<epdf_kernel::clock::Cancel>>,
}

/// `Math.max` over two numbers: NaN when either is.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

impl StageCapability {
    fn wheel(&self) -> &Wheel {
        &self.inner.wheel
    }

    /// End the stream after a pause; every wheel restarts the countdown.
    fn close_wheel_when_quiet(&self) {
        let cancel = self.wheel().cancel_quiet.borrow_mut().take();
        if let Some(cancel) = cancel {
            cancel.call();
        }
        let this = self.clone();
        let cancel = self.inner.clock.after(QUIET_MS, move || {
            let wheel = this.wheel();
            wheel.cancel_quiet.borrow_mut().take();
            wheel.taken.set(None);
            wheel.moving.set(false);
            let mut coasting = wheel.coasting.get();
            coasting.forget();
            wheel.coasting.set(coasting);
        });
        *self.wheel().cancel_quiet.borrow_mut() = Some(cancel);
    }

    /// Pan by a wheel's delta, in viewport px and signed like
    /// [`StageCapability::pan_by`]. `time_ms` is when the wheel happened, on
    /// a clock that only goes forward: it tells a new push from a scroll
    /// that coasts. False: the view leaves this wheel to the page around the
    /// viewer (the `scroll_chaining` setting), so the binding lets the host
    /// have the event.
    pub fn wheel_by(&self, dx: f64, dy: f64, time_ms: f64) -> Result<bool, Error> {
        let chains = self.st().settings.scroll_chaining;
        let wheel = self.wheel();
        // A wheel that moves nothing says nothing about where its stream is
        // headed. It's kept, so the host starts no scroll with a stream that
        // has no direction yet.
        if dx == 0.0 && dy == 0.0 {
            return Ok(!chains || wheel.taken.get().unwrap_or(true));
        }
        let room = self.leads_somewhere(Push { x: dx, y: dy });
        let mut coasting = wheel.coasting.get();
        let broken = coasting.broken_by(js_max(dx.abs(), dy.abs()), time_ms);
        wheel.coasting.set(coasting);
        if broken {
            self.turn_push_began();
            // Stopped at an end, the Stage holds what coasts in and no
            // more: the new push is decided afresh, as it is after a stream
            // the page took. One that follows a stream still moving the
            // pages stays the Stage's, wherever it heads.
            if !wheel.moving.get() {
                wheel.taken.set(None);
            }
        }
        let taken = match wheel.taken.get() {
            Some(taken) => taken,
            None => {
                wheel.taken.set(Some(room));
                room
            }
        };
        wheel.moving.set(taken && room);
        self.close_wheel_when_quiet();
        // With chaining off every wheel is the Stage's.
        if chains && !taken {
            return Ok(false);
        }
        self.pan_by(dx, dy)?;
        Ok(true)
    }
}
