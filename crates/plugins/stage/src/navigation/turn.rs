//! `navigation/turn.ts`: page turns in paged flow. A push past the edge of
//! the page turns it, read off what the host port reports (pans, the drag
//! bracket, the release fling). A push is the part of a pan the clamp
//! refused. A stream is one drag, or one push of a wheel or trackpad with
//! its momentum: the pans up to the next pause, or up to the next push the
//! wheel tells while the last still coasts (`camera/wheel.rs`). A stream
//! turns only at an edge it began at, so reading down to the bottom stops
//! there, and it turns once: the rest of it is spent, so momentum never
//! scrolls the page that just arrived.
//!
//! In every flow it also says whether a push has anywhere to go, which is
//! how the wheel tells a stream of its own from one for the page around the
//! viewer (`camera/wheel.rs`).

use crate::prelude::*;

/// The push past an edge that turns the page, in screen px: about one
/// wheel notch.
const TURN_PUSH: f64 = 60.0;
/// The release speed past an edge that turns the page, in screen px/s: a
/// flick.
const TURN_FLICK: f64 = 600.0;
/// The pause that ends a wheel or trackpad stream.
pub(crate) const QUIET_MS: f64 = 200.0;

/// A screen-space amount per axis (`Point`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Push {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl Push {
    fn plus(self, other: Push) -> Push {
        Push { x: self.x + other.x, y: self.y + other.y }
    }

    fn axis(self, y: bool) -> f64 {
        if y { self.y } else { self.x }
    }
}

/// Per axis, whether the camera can't move further toward the next page,
/// and toward the previous.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Edges {
    next_x: bool,
    next_y: bool,
    previous_x: bool,
    previous_y: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Stream {
    /// The edges the camera already sat at when the stream began: the only
    /// ones it turns at.
    started_at: Edges,
    /// The push so far, per axis, positive toward the next page.
    push: Push,
    /// It turned: the rest of it moves nothing.
    spent: bool,
}

/// The turn detector's state (`createTurns`' closure).
#[derive(Default)]
pub(crate) struct Turns {
    pub(crate) stream: Cell<Option<Stream>>,
    pub(crate) dragging: Cell<bool>,
    pub(crate) cancel_quiet: RefCell<Option<epdf_kernel::clock::Cancel>>,
}

/// The part of a pan the clamp refused, in screen px, signed like the pan.
pub(crate) fn refused_by(wanted: Camera, allowed: Camera) -> Push {
    Push { x: (allowed.x - wanted.x) * wanted.zoom, y: (allowed.y - wanted.y) * wanted.zoom }
}

impl StageCapability {
    fn turns(&self) -> &Turns {
        &self.inner.turns
    }

    fn turns_enabled(&self) -> bool {
        self.paged() && self.st().settings.turn_gestures
    }

    fn rtl(&self) -> bool {
        self.st().settings.direction == Direction::Rtl
    }

    /// A screen delta in reading terms: positive toward the next page on
    /// each axis.
    fn ahead(&self, delta: Push) -> Push {
        Push { x: if self.rtl() { delta.x } else { -delta.x }, y: -delta.y }
    }

    /// An axis that doesn't scroll sits at both of its edges.
    fn edges_now(&self) -> Edges {
        let m = self.scroll_metrics_now();
        let top = !m.scrollable_y || m.scroll_top <= 0.5;
        let bottom = !m.scrollable_y || m.scroll_top + m.client_height >= m.scroll_height - 0.5;
        let left = !m.scrollable_x || m.scroll_left <= 0.5;
        let right = !m.scrollable_x || m.scroll_left + m.client_width >= m.scroll_width - 0.5;
        let rtl = self.rtl();
        Edges {
            next_x: if rtl { left } else { right },
            next_y: bottom,
            previous_x: if rtl { right } else { left },
            previous_y: top,
        }
    }

    fn open_stream(&self) -> Stream {
        let turns = self.turns();
        match turns.stream.get() {
            Some(stream) => stream,
            None => {
                let stream =
                    Stream { started_at: self.edges_now(), push: Push::default(), spent: false };
                turns.stream.set(Some(stream));
                stream
            }
        }
    }

    fn cancel_quiet(&self) {
        let cancel = self.turns().cancel_quiet.borrow_mut().take();
        if let Some(cancel) = cancel {
            cancel.call();
        }
    }

    /// End the stream after a pause; every pan restarts the countdown.
    fn close_when_quiet(&self) {
        self.cancel_quiet();
        let this = self.clone();
        let cancel = self.inner.clock.after(QUIET_MS, move || {
            this.turns().cancel_quiet.borrow_mut().take();
            this.turns().stream.set(None);
        });
        *self.turns().cancel_quiet.borrow_mut() = Some(cancel);
    }

    /// Turn when, on either axis, `amount` reaches `threshold` at an edge
    /// the stream began at.
    fn turn_past(&self, amount: Push, threshold: f64) -> bool {
        let Some(current) = self.turns().stream.get() else { return false };
        for y in [true, false] {
            let at_next = if y { current.started_at.next_y } else { current.started_at.next_x };
            if amount.axis(y) >= threshold && at_next && self.can_go_next() {
                self.turns().stream.set(Some(Stream { spent: true, ..current }));
                self.next_page(Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)));
                return true;
            }
            let at_previous =
                if y { current.started_at.previous_y } else { current.started_at.previous_x };
            if amount.axis(y) <= -threshold && at_previous && self.can_go_previous() {
                self.turns().stream.set(Some(Stream { spent: true, ..current }));
                // Back lands where you left it: the bottom of the previous
                // page, or its reading end.
                let end = Some(ArrivalAlignValue::named(Align::End));
                let arrival_align = if y {
                    PartialArrivalAlignment { x: None, y: end }
                } else {
                    PartialArrivalAlignment { x: end, y: None }
                };
                self.previous_page(Some(GoToOptions {
                    behavior: Some(ScrollBehaviorKind::Instant),
                    viewpoint: None,
                    arrival_align: Some(arrival_align),
                }));
                return true;
            }
        }
        false
    }

    /// Before a pan moves the camera. True: this stream already turned, so
    /// the pan moves nothing.
    pub(crate) fn turn_before_pan(&self) -> bool {
        if !self.turns_enabled() {
            return false;
        }
        let current = self.open_stream();
        if !self.turns().dragging.get() {
            self.close_when_quiet();
        }
        current.spent
    }

    /// After a rigid pan: the part of it the clamp refused, in screen px.
    pub(crate) fn turn_refused(&self, delta: Push) {
        if !self.turns_enabled() {
            return;
        }
        let Some(mut stream) = self.turns().stream.get() else { return };
        stream.push = stream.push.plus(self.ahead(delta));
        self.turns().stream.set(Some(stream));
        // A wheel turns as soon as it has pushed far enough; a drag waits
        // for its release.
        if !self.turns().dragging.get() {
            self.turn_past(stream.push, TURN_PUSH);
        }
    }

    /// A wheel or trackpad pushes anew while its last push still coasts:
    /// the next stream begins here, with no pause before it.
    pub(crate) fn turn_push_began(&self) {
        if !self.turns().dragging.get() {
            self.turns().stream.set(None);
        }
    }

    /// A drag begins: one stream for all of it, however long it holds
    /// still.
    pub(crate) fn turn_drag_began(&self) {
        if !self.turns_enabled() {
            return;
        }
        self.cancel_quiet();
        self.turns().stream.set(None);
        self.turns().dragging.set(true);
        self.open_stream();
    }

    /// The drag ended, its finger pulled `overshoot` past the edge (screen
    /// px). True: it turned the page, so nothing springs home.
    pub(crate) fn turn_drag_ended(&self, overshoot: Push, zoomed: bool) -> bool {
        self.turns().dragging.set(false);
        if !self.turns_enabled() {
            return false;
        }
        let Some(stream) = self.turns().stream.get() else { return false };
        // A pinch zooms; it never turns the page, nor does the fling after it.
        if zoomed {
            self.turns().stream.set(None);
            return false;
        }
        // The release fling comes next and belongs to this stream.
        self.close_when_quiet();
        self.turn_past(stream.push.plus(self.ahead(overshoot)), TURN_PUSH)
    }

    /// The release speed, right after a drag. True: no momentum, as the
    /// page turned.
    pub(crate) fn turn_flung(&self, velocity: Push) -> bool {
        if !self.turns_enabled() {
            return false;
        }
        let Some(stream) = self.turns().stream.get() else { return false };
        stream.spent || self.turn_past(self.ahead(velocity), TURN_FLICK)
    }

    /// Whether a push by `delta` (screen px) has anywhere to go along its
    /// longer axis: the camera can move that way, or a turn has a page to
    /// turn to.
    pub(crate) fn leads_somewhere(&self, delta: Push) -> bool {
        // `Math.abs(x) > Math.abs(y) ? 'x' : 'y'`: a tie, or NaN, is `y`.
        let along_x = delta.x.abs() > delta.y.abs();
        let next = self.ahead(delta).axis(!along_x) > 0.0;
        let edges = self.edges_now();
        let at_edge = match (next, !along_x) {
            (true, true) => edges.next_y,
            (true, false) => edges.next_x,
            (false, true) => edges.previous_y,
            (false, false) => edges.previous_x,
        };
        // An unbounded camera has no edges.
        if !self.st().settings.bounded || !at_edge {
            return true;
        }
        if !self.turns_enabled() {
            return false;
        }
        if next { self.can_go_next() } else { self.can_go_previous() }
    }
}
