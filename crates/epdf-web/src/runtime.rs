//! The kernel's runtime on a JavaScript event loop.
//!
//! `rt` only moves when a host drains it and advances its clock. Here the
//! host is the page. [`start_clock`] moves the virtual clock to `Date.now()`
//! once, so `rt::now()` reads as a real date. Every export then ends with
//! [`after_entry`], which catches the virtual clock up to `Date.now()`
//! (firing whatever timers came due on the way), drains the ready queue, and
//! arms one real `setTimeout` for the earliest virtual timer left. When that
//! fires it runs [`after_entry`] again. Between entries the virtual clock
//! stands still; at each entry it is wall time, to the millisecond.
//!
//! Promises cross in both directions as JSON values, with a rejection as the
//! kernel's canonical `{"$error": {name, message, ...}}`. A JavaScript `await`
//! on a converted promise runs on the JavaScript microtask queue, not the
//! Rust ready queue, so ordering across the boundary is not the order the
//! vectors pin.

use std::cell::{Cell, RefCell};

use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Error, is_undefined, undefined};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(handler: &Closure<dyn FnMut()>, ms: f64) -> JsValue;
    #[wasm_bindgen(js_name = clearTimeout)]
    fn clear_timeout(handle: &JsValue);
    /// `String(value)`.
    #[wasm_bindgen(js_name = String)]
    fn js_string(value: &JsValue) -> String;
}

/// The one armed `setTimeout`.
struct Armed {
    handle: JsValue,
    /// The virtual deadline it wakes for.
    deadline: f64,
    /// Kept alive until it fires or is cleared.
    _callback: Closure<dyn FnMut()>,
}

thread_local! {
    static ARMED: RefCell<Option<Armed>> = const { RefCell::new(None) };
    /// Whether [`start_clock`] has run, so entries follow `Date.now()`.
    static STARTED: Cell<bool> = const { Cell::new(false) };
    /// How many entries are on the stack. A timer's callback can reach
    /// JavaScript, which can call an export, whose `after_entry` then runs
    /// inside the outer one's `rt::advance`.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// One entry on the stack, counted while it lives, so a panic cannot leave
/// the count stuck.
struct Entry {
    outermost: bool,
}

impl Entry {
    fn enter() -> Entry {
        let depth = DEPTH.with(|d| {
            d.set(d.get() + 1);
            d.get()
        });
        Entry { outermost: depth == 1 }
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Put this thread's runtime back to nothing: disarm the timeout, stop
/// following wall time, and [`rt::reset`]. For tests running one case after
/// another in one instance.
pub fn reset() {
    if let Some(armed) = ARMED.with(|armed| armed.borrow_mut().take()) {
        clear_timeout(&armed.handle);
    }
    STARTED.with(|started| started.set(false));
    DEPTH.with(|d| d.set(0));
    rt::reset();
}

/// Move the virtual clock forward to `Date.now()`, and keep it following
/// wall time at every [`after_entry`] from here on. Call once, when the
/// kernel is created, so what plugins timestamp with `rt::now()` is a date.
///
/// Call it before anything schedules a timer. The jump is one
/// `rt::advance` across decades, and an interval set before it would fire
/// once for every period in the gap.
pub fn start_clock() {
    STARTED.with(|started| started.set(true));
    let entry = Entry::enter();
    if entry.outermost {
        catch_up();
    }
}

fn catch_up() {
    if STARTED.with(Cell::get) {
        let behind = js_sys::Date::now() - rt::now();
        if behind > 0.0 {
            rt::advance(behind);
        }
    }
}

/// Call at the end of every export: catch the clock up to wall time once it
/// is started, drain the ready queue, then make sure a real timeout is armed
/// for the earliest virtual timer, if there is one.
///
/// Called from inside another entry (a timer's callback reached an export),
/// it only drains the queue. Catching up there would advance the clock
/// inside the outer `rt::advance`, which then lands on its own, earlier
/// target and turns the clock back; the outer entry catches up and arms.
pub fn after_entry() {
    let entry = Entry::enter();
    if !entry.outermost {
        rt::run_until_stalled();
        return;
    }
    catch_up();
    rt::run_until_stalled();
    let next = rt::next_deadline();
    let keep = ARMED.with(|armed| {
        let armed = armed.borrow();
        match (armed.as_ref(), next) {
            (Some(armed), Some(next)) => armed.deadline <= next,
            _ => false,
        }
    });
    if keep {
        return;
    }
    if let Some(stale) = ARMED.with(|armed| armed.borrow_mut().take()) {
        clear_timeout(&stale.handle);
    }
    let Some(deadline) = next else { return };
    let callback = Closure::<dyn FnMut()>::new(move || {
        // Dropped at the end, not while this closure is still running.
        let spent = ARMED.with(|armed| armed.borrow_mut().take());
        // The timer this was armed for is due, even if the timeout came back
        // a little early or the clock was never started; `after_entry` then
        // catches up the rest of the way to wall time.
        {
            let _entry = Entry::enter();
            rt::advance((deadline - rt::now()).max(0.0));
        }
        after_entry();
        drop(spent);
    });
    let handle = set_timeout(&callback, (deadline - rt::now()).max(0.0));
    ARMED.with(|armed| {
        *armed.borrow_mut() = Some(Armed { handle, deadline, _callback: callback });
    });
}

/// A Rust promise as a JavaScript one. A rejection carries the canonical
/// `{"$error": ...}` object.
pub fn to_js(p: Promise<Value>) -> js_sys::Promise {
    js_sys::Promise::new(&mut |resolve, reject| {
        p.on_settle(move |result| {
            let _ = match result {
                Ok(value) => resolve.call1(&JsValue::UNDEFINED, &value_to_js(value)),
                Err(error) => {
                    reject.call1(&JsValue::UNDEFINED, &value_to_js(&error.to_canonical()))
                }
            };
        });
    })
}

/// A JavaScript promise as a Rust one. Its settlement arrives on the
/// JavaScript microtask queue, which then runs [`after_entry`]. A value JSON
/// cannot carry (a `BigInt`, a cycle) rejects it with a `TypeError`.
pub fn from_js(p: js_sys::Promise) -> Promise<Value> {
    let (promise, resolver) = Promise::pending();
    wasm_bindgen_futures::spawn_local(async move {
        let result = match JsFuture::from(p).await {
            Ok(value) => js_to_value(&value),
            Err(thrown) => Err(js_to_error(&thrown)),
        };
        resolver.settle(result);
        after_entry();
    });
    promise
}

fn value_to_js(value: &Value) -> JsValue {
    if is_undefined(value) {
        return JsValue::UNDEFINED;
    }
    let json = serde_json::to_string(value).expect("a JSON value serializes");
    js_sys::JSON::parse(&json).expect("JSON.parse reads what serde_json wrote")
}

/// A JavaScript value as JSON. `undefined` and a function (which
/// `JSON.stringify` drops) are the kernel's `undefined`; anything else JSON
/// cannot carry is a `TypeError`.
fn js_to_value(value: &JsValue) -> Result<Value, Error> {
    if value.is_undefined() || value.is_function() {
        return Ok(undefined());
    }
    let json = js_sys::JSON::stringify(value).map_err(|thrown| {
        let message = match thrown.dyn_ref::<js_sys::Error>() {
            Some(error) => String::from(error.message()),
            None => js_string(&thrown),
        };
        Error::named("TypeError", message)
    })?;
    let json = json.as_string().ok_or_else(|| {
        Error::named("TypeError", format!("{} is not JSON-serializable", js_string(value)))
    })?;
    serde_json::from_str(&json).map_err(|e| Error::named("TypeError", e.to_string()))
}

/// What a JavaScript promise rejected with: a canonical `{"$error": ...}`,
/// an `Error` or any object with a string `message` (its `name`, `message`
/// and any `code`), or anything else as an `Error` whose message is
/// `String(value)`.
fn js_to_error(value: &JsValue) -> Error {
    let spec = js_to_value(value).ok();
    if let Some(spec) = spec.as_ref().filter(|spec| spec.get("$error").is_some()) {
        return Error::from_spec(spec);
    }
    let member = |key: &str| {
        js_sys::Reflect::get(value, &JsValue::from_str(key)).ok().filter(|v| !v.is_undefined())
    };
    let message = value.is_object().then(|| member("message")).flatten();
    let Some(message) = message.and_then(|m| m.as_string()) else {
        return Error::new(js_string(value));
    };
    let name = member("name").and_then(|n| n.as_string()).unwrap_or_else(|| "Error".into());
    let mut out = Error::named(name, message);
    if let Some(code) = member("code") {
        out = out.with("code", js_to_value(&code).unwrap_or_else(|_| js_string(&code).into()));
    }
    out
}
