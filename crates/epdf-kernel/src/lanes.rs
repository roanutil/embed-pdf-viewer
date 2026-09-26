//! `lanes.ts`: newest wins. Starting a run aborts the previous one; a
//! superseded run cannot publish (`commit` returns false) and its promise
//! rejects `operation-cancelled`, whatever its work later does.
//!
//! The TypeScript's abort reasons are strings (`'superseded'`,
//! `'cancelled'`, `'instance-closed'`, or what the caller passed to
//! `cancel`); here a run's signal carries each as an error whose message is
//! the reason's `String()`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde_json::{Value, json};

use crate::error::{Error, code, plugin_error, to_plugin_error};
use crate::rt::Promise;
use crate::signal::AbortSignal;

/// JavaScript's `String(value)` for the reasons a lane is aborted with.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        // Integers and the shortest round-trip form, which is what JavaScript
        // prints for every reason a lane is given in practice.
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.fract() == 0.0 && x.abs() < 1e21 {
                format!("{}", x as i64)
            } else {
                format!("{x}")
            }
        }
        Value::Array(items) => items
            .iter()
            .map(|v| if v.is_null() { String::new() } else { js_string(v) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(o) if o.contains_key("$undefined") => "undefined".into(),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// One execution inside a lane.
#[derive(Clone)]
pub struct LatestRun {
    /// `<lane>#<n>`, unique per run.
    pub id: String,
    /// Aborts when a newer run starts, the caller's signal aborts, or the
    /// instance closes.
    pub signal: AbortSignal,
    lane: Rc<LaneInner>,
    mine: u64,
}

impl LatestRun {
    /// True once a newer run has started.
    pub fn superseded(&self) -> bool {
        self.mine != self.lane.generation.get()
    }

    fn current(&self) -> bool {
        self.mine == self.lane.generation.get() && !self.signal.aborted()
    }

    /// Run `commit` only while this run is still the newest and alive.
    pub fn commit(&self, commit: impl FnOnce()) -> bool {
        if !self.current() {
            return false;
        }
        commit();
        true
    }
}

struct LaneInner {
    lifetime: AbortSignal,
    capability: String,
    name: String,
    generation: Cell<u64>,
    active: RefCell<Option<AbortSignal>>,
}

/// `createLatestLane(lifetime, capability, name)`.
#[derive(Clone)]
pub struct LatestLane(Rc<LaneInner>);

fn reason(error: &Error) -> String {
    error.message.clone()
}

impl LatestLane {
    pub fn new(lifetime: AbortSignal, capability: &str, name: &str) -> Self {
        LatestLane(Rc::new(LaneInner {
            lifetime,
            capability: capability.to_string(),
            name: name.to_string(),
            generation: Cell::new(0),
            active: RefCell::new(None),
        }))
    }

    pub fn running(&self) -> bool {
        self.0.active.borrow().is_some()
    }

    /// Cancel the current run, if any, with `String(reason ?? 'cancelled')`.
    pub fn cancel(&self, reason: Option<&Value>) {
        let active = self.0.active.borrow_mut().take();
        if let Some(active) = active {
            let why = match reason {
                Some(r) if !r.is_null() && !crate::is_undefined(r) => js_string(r),
                _ => "cancelled".into(),
            };
            active.abort(Error::new(why));
        }
    }

    fn closed(&self) -> Error {
        plugin_error(
            code::INSTANCE_CLOSED,
            &self.0.capability,
            format!("{}: instance closed", self.0.name),
            None,
            None,
        )
    }

    fn cancelled(&self, id: &str, why: &str) -> Error {
        plugin_error(
            code::OPERATION_CANCELLED,
            &self.0.capability,
            format!("{}: {why}", self.0.name),
            None,
            Some(json!({ "runId": id, "reason": why })),
        )
    }

    /// `run(fn, {signal})`, an `async` method: its refusals are rejections.
    /// `work` is the TypeScript's `fn`: it returns the promise of the work it
    /// started (usually an `async` function's), which the lane awaits.
    pub fn run<T, F>(&self, work: F, signal: Option<AbortSignal>) -> Promise<T>
    where
        T: Clone + 'static,
        F: FnOnce(LatestRun) -> Promise<T> + 'static,
    {
        let lane = self.clone();
        Promise::spawn(async move {
            let inner = lane.0.clone();
            if inner.lifetime.aborted() {
                return Err(lane.closed());
            }
            if signal.as_ref().is_some_and(AbortSignal::aborted) {
                let id = format!("{}#{}", inner.name, inner.generation.get() + 1);
                return Err(lane.cancelled(&id, "cancelled before start"));
            }
            let mine = inner.generation.get() + 1;
            inner.generation.set(mine);
            let id = format!("{}#{mine}", inner.name);
            let controller = AbortSignal::new();
            // Install before aborting: abort listeners run synchronously.
            let previous = inner.active.borrow_mut().replace(controller.clone());
            let on_lifetime = {
                let c = controller.clone();
                inner.lifetime.on_abort(Rc::new(move |_| c.abort(Error::new("instance-closed"))))
            };
            let on_caller = signal.as_ref().map(|s| {
                let c = controller.clone();
                s.on_abort(Rc::new(move |_| c.abort(Error::new("cancelled"))))
            });
            let run =
                LatestRun { id: id.clone(), signal: controller.clone(), lane: inner.clone(), mine };
            let why = |run: &LatestRun| {
                run.signal.reason().map(|e| reason(&e)).unwrap_or_else(|| "superseded".into())
            };

            // The try block: every refusal it raises goes through the catch below.
            let outcome: Result<T, Error> = async {
                if let Some(previous) = previous {
                    previous.abort(Error::new("superseded"));
                }
                if !run.current() {
                    return Err(lane.cancelled(&id, "superseded"));
                }
                let value = work(run.clone()).await?;
                if !run.current() {
                    return Err(lane.cancelled(&id, &why(&run)));
                }
                Ok(value)
            }
            .await;
            let result = match outcome {
                Ok(value) => Ok(value),
                Err(error) => {
                    if inner.lifetime.aborted() {
                        Err(lane.closed())
                    } else if !run.current() {
                        Err(lane.cancelled(&id, &why(&run)))
                    } else {
                        Err(to_plugin_error(&inner.capability, &error))
                    }
                }
            };
            inner.lifetime.remove(on_lifetime);
            if let (Some(signal), Some(id)) = (signal.as_ref(), on_caller) {
                signal.remove(id);
            }
            let is_active = inner.active.borrow().as_ref().is_some_and(|a| a.same(&controller));
            if is_active {
                *inner.active.borrow_mut() = None;
            }
            result
        })
    }
}
