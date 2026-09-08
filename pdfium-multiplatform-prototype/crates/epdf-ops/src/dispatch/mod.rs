//! One trait-shaped API, two implementations chosen by target.
//!
//! On every native target the fork builds with embedpdf_thread_local_globals=true
//! (see runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77), so PDFium's
//! globals are per-thread and a handle cannot cross threads. `threaded` owns one
//! OS thread and every op is a channel round-trip to it.
//!
//! On wasm32-unknown-emscripten there are no threads and each wasm instance has
//! its own linear memory, which is why that target leaves the flag off. `inline`
//! calls straight through.

#[cfg(not(target_os = "emscripten"))]
mod threaded;
#[cfg(not(target_os = "emscripten"))]
pub use threaded::Worker;

#[cfg(target_os = "emscripten")]
mod inline;
#[cfg(target_os = "emscripten")]
pub use inline::Worker;

use std::cell::Cell;

/// A reentrancy guard for a single live borrow. `try_borrow` sets the flag
/// and hands back a `BorrowGuard` that clears it in `Drop`; because `Drop`
/// runs during unwinding as well as on a normal return, a panic while the
/// guard is held still releases the flag, with no `catch_unwind` needed for
/// the flag itself. Compiled on every target (unlike `inline`, which is
/// `emscripten`-only) so it can be unit-tested on the host; `inline` is its
/// only production caller, which is why non-`emscripten` builds only reach
/// it from the tests below and need the `dead_code` allow.
#[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
pub(crate) struct BorrowFlag(Cell<bool>);

#[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
impl BorrowFlag {
    pub(crate) fn new() -> Self {
        Self(Cell::new(false))
    }

    /// `Some(guard)` if the flag was clear, which this also sets; `None` if
    /// a guard is already outstanding.
    pub(crate) fn try_borrow(&self) -> Option<BorrowGuard<'_>> {
        if self.0.replace(true) {
            None
        } else {
            Some(BorrowGuard(&self.0))
        }
    }
}

#[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
pub(crate) struct BorrowGuard<'a>(&'a Cell<bool>);

impl Drop for BorrowGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{self, AssertUnwindSafe};

    #[test]
    fn try_borrow_fails_while_a_guard_is_outstanding() {
        let flag = BorrowFlag::new();
        let _guard = flag.try_borrow().expect("flag starts clear");
        assert!(flag.try_borrow().is_none(), "a second borrow must be refused while the first is live");
    }

    #[test]
    fn dropping_the_guard_releases_the_flag() {
        let flag = BorrowFlag::new();
        {
            let _guard = flag.try_borrow().expect("flag starts clear");
        }
        assert!(flag.try_borrow().is_some(), "the flag must be clear once the guard has dropped");
    }

    /// The regression this type exists to fix: a panic while the guard is
    /// held must not leave the flag permanently set. `Drop for BorrowGuard`
    /// runs during unwinding, so `try_borrow` succeeding again after the
    /// `catch_unwind` below is the proof.
    #[test]
    fn a_panic_while_the_guard_is_held_still_releases_the_flag() {
        let flag = BorrowFlag::new();

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = flag.try_borrow().expect("flag starts clear");
            panic!("deliberate panic for the guard-release test");
        }));
        assert!(result.is_err(), "the closure must actually have panicked");

        assert!(
            flag.try_borrow().is_some(),
            "a later try_borrow must succeed after a panic dropped the guard during unwinding"
        );
    }
}
