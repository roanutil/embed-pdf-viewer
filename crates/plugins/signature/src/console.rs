//! `globalThis.console?.error`, which the plugin reports background failures
//! to (`[signature] ...`), as a host input. A host (the conformance harness,
//! a native shell) installs its own sink; the default writes to stderr.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::Error;

/// A console sink: the message and the error, as `console.error(message,
/// error)` receives them.
pub type ConsoleSink = Rc<dyn Fn(&str, &Error)>;

thread_local! {
    static SINK: RefCell<Option<ConsoleSink>> = const { RefCell::new(None) };
}

/// Install a console sink for this thread; returns the one it replaces.
pub fn set_sink(sink: Option<ConsoleSink>) -> Option<ConsoleSink> {
    SINK.with(|s| std::mem::replace(&mut *s.borrow_mut(), sink))
}

/// `console.error(message, error)`.
pub fn error(message: &str, error: &Error) {
    let sink = SINK.with(|s| s.borrow().clone());
    match sink {
        Some(sink) => sink(message, error),
        None => eprintln!("{message} {}", error.to_canonical()),
    }
}
