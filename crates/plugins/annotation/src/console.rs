//! The console the TypeScript writes to (`console.error`), as a host input.
//! A host (the conformance harness, a native shell) installs a sink; the
//! default writes to stderr.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

/// A console sink: the level (`error`, `warn`) and the arguments.
pub type ConsoleSink = Rc<dyn Fn(&str, Vec<Value>)>;

thread_local! {
    static SINK: RefCell<Option<ConsoleSink>> = const { RefCell::new(None) };
}

/// Install a sink for this thread; returns the one it replaces.
pub fn set_sink(sink: Option<ConsoleSink>) -> Option<ConsoleSink> {
    SINK.with(|s| std::mem::replace(&mut *s.borrow_mut(), sink))
}

/// `console.error(message, value)`.
pub fn error(message: &str, value: Value) {
    let sink = SINK.with(|s| s.borrow().clone());
    match sink {
        Some(sink) => sink("error", vec![Value::String(message.to_string()), value]),
        None => eprintln!("{message} {value}"),
    }
}
