//! `camera/coasting.ts`: coasting, told from a new push. After the fingers
//! leave a trackpad the scroll coasts: wheels keep coming for a second or
//! more, slower and slower. The next swipe often begins before they stop,
//! so no pause sets the two apart, and a wheel doesn't say which of the two
//! it is. The speed does: coasting only slows down, so a scroll that speeds
//! up after it has slowed is a new push.
//!
//! Speed, not the size of a wheel. A browser hands over one wheel for
//! however many the trackpad sent since the last, so sizes double and halve
//! at a steady speed. Each wheel carries the time it happened, and the
//! distance over that time holds steady.

/// How many times its slowest speed a scroll has to reach, to have sped up.
const PUSH_GROWTH: f64 = 2.5;

/// The slowest speed a push is told against, in px per ms. A scroll about
/// to stop moves a pixel or two a frame, where one pixel more doubles its
/// speed.
const SLOWEST_TOLD: f64 = 0.1;

/// The shortest time a speed is told over: a frame. Wheels closer together
/// count as one, which evens out a screen that sends two or more a frame.
const MEASURE_MS: f64 = 16.0;

/// The longest. A scroll that coasts sends a wheel every frame; wheels
/// further apart than this are the notches of a mouse wheel, or a scroll
/// that rested. No speed is told across the gap, so notches never read as
/// a new push.
const STEADY_MS: f64 = 40.0;

/// `createCoasting()`'s closure.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Coasting {
    /// When the last wheel that began or ended a speed's span came.
    since: f64,
    /// How far the scroll has gone since.
    gone: f64,
    /// The last speed, in px per ms.
    last: f64,
    /// The slowest speed since the scroll began to slow. While it has only
    /// sped up there is none (infinity), and nothing is a new push.
    slowest: f64,
}

impl Default for Coasting {
    fn default() -> Self {
        Coasting { since: f64::NEG_INFINITY, gone: 0.0, last: 0.0, slowest: f64::INFINITY }
    }
}

impl Coasting {
    /// Take the next wheel: `size` px along its longer axis, at `time_ms`.
    /// True: with it the scroll sped up, a new push.
    pub(crate) fn broken_by(&mut self, size: f64, time_ms: f64) -> bool {
        let span = time_ms - self.since;
        if span > STEADY_MS {
            self.since = time_ms;
            self.gone = 0.0;
            return false;
        }
        self.gone += size;
        if span < MEASURE_MS {
            return false;
        }
        let speed = self.gone / span;
        self.since = time_ms;
        self.gone = 0.0;

        // `Math.max(slowest, SLOWEST_TOLD)`: `slowest` is never NaN.
        let pushed = speed > PUSH_GROWTH * self.slowest.max(SLOWEST_TOLD);
        if pushed {
            self.slowest = f64::INFINITY;
        } else if speed < self.last {
            // Only reached with a speed that compares, so never NaN.
            self.slowest = self.slowest.min(speed);
        }
        self.last = speed;
        pushed
    }

    /// The scroll paused: the next wheel has nothing before it.
    pub(crate) fn forget(&mut self) {
        *self = Coasting::default();
    }
}
