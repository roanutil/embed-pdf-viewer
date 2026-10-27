//! The globals the TypeScript reads, as host inputs: `console.error` and
//! `console.warn`, which the plugin writes `[stamp]` lines to, and
//! `Math.random`, which a minted stamp identifier draws from
//! (`customStampName`). A host (the conformance harness, a native shell)
//! installs its own; the defaults write to stderr and draw from a
//! clock-seeded xorshift.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::rt;
use serde_json::Value;

/// A console sink: the level (`error`, `warn`) and the arguments.
pub type ConsoleSink = Rc<dyn Fn(&str, Vec<Value>)>;

/// `Math.random`.
pub type RandomSource = Rc<dyn Fn() -> f64>;

thread_local! {
    static SINK: RefCell<Option<ConsoleSink>> = const { RefCell::new(None) };
    static RANDOM: RefCell<Option<RandomSource>> = const { RefCell::new(None) };
    static XORSHIFT: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
}

/// Install a console sink for this thread; returns the one it replaces.
pub fn set_sink(sink: Option<ConsoleSink>) -> Option<ConsoleSink> {
    SINK.with(|s| std::mem::replace(&mut *s.borrow_mut(), sink))
}

/// Install a `Math.random` for this thread; returns the one it replaces.
pub fn set_random(random: Option<RandomSource>) -> Option<RandomSource> {
    RANDOM.with(|r| std::mem::replace(&mut *r.borrow_mut(), random))
}

fn write(level: &str, args: Vec<Value>) {
    let sink = SINK.with(|s| s.borrow().clone());
    match sink {
        Some(sink) => sink(level, args),
        None => {
            let text: Vec<String> = args
                .iter()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
                .collect();
            eprintln!("{}", text.join(" "));
        }
    }
}

/// `console.warn(...args)`.
pub fn warn(args: Vec<Value>) {
    write("warn", args);
}

/// `console.error(...args)`.
pub fn error(args: Vec<Value>) {
    write("error", args);
}

/// `Math.random()`.
pub fn random() -> f64 {
    let installed = RANDOM.with(|r| r.borrow().clone());
    match installed {
        Some(random) => random(),
        None => clock_random(),
    }
}

/// The default: a small xorshift seeded from the clock. Without a
/// dependency the default host has no better entropy to offer, and an
/// identifier needs only to be unlikely to repeat.
fn clock_random() -> f64 {
    XORSHIFT.with(|s| {
        let mut x = s.get() ^ (rt::now() as u64);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

#[cfg(test)]
mod tests {
    use super::clock_random;

    /// `Math.random`'s contract: draws in [0, 1), spread over the interval.
    #[test]
    fn the_default_random_spreads_over_the_unit_interval() {
        let draws: Vec<f64> = (0..10_000).map(|_| clock_random()).collect();
        assert!(draws.iter().all(|x| (0.0..1.0).contains(x)), "a draw left [0, 1)");
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        assert!((0.45..0.55).contains(&mean), "mean {mean}");
        let mut tenths = [0usize; 10];
        for x in &draws {
            tenths[(x * 10.0) as usize] += 1;
        }
        assert!(tenths.iter().all(|&n| n > 800), "{tenths:?}");
    }
}
