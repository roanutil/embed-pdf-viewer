//! `services/scheduler.ts`: the host timing seam.
//!
//! The TypeScript binds `globalThis.requestAnimationFrame` when the host has
//! one. Rust has no ambient frame clock: the host passes a [`Scheduler`] in
//! [`StageConfig::scheduler`], and without one the lens is frameless
//! (navigation jumps, no rest detector, `fling` only snaps into bounds).

use crate::prelude::*;

impl StageCapability {
    pub(crate) fn can_animate(&self) -> bool {
        self.inner.scheduler.is_some()
    }

    pub(crate) fn raf_call(&self, callback: Box<dyn FnOnce(f64)>) -> u64 {
        match &self.inner.scheduler {
            Some(s) => s.raf(callback),
            None => 0,
        }
    }

    pub(crate) fn caf_call(&self, handle: u64) {
        if let Some(s) = &self.inner.scheduler {
            s.caf(handle);
        }
    }
}
