//! `hover-pump.ts`: the one hover state machine every event plane shares.
//!
//! Per pointer feed, `{delivered, desired, in_flight}`. On each settle the
//! pump delivers `delivered -> desired` as `Exit(delivered)` then
//! `Enter(desired)`, submitted back to back, so the ordered pair can never be
//! split by a slow resolution. Under pressure intermediate targets are
//! skipped by design.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::rt::{self, Promise};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{
    ActionSource, ActionTrigger, ActionTriggerResult, AnnotationRef, PageObjectNumber,
    PdfAnnotationEventKind,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HoverEvents {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enter: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<bool>,
}

/// One hoverable target. `events` lets a feed that knows which trees exist
/// skip the inert half of a pair; absent flags are true.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoverTarget {
    #[serde(rename = "ref")]
    pub annotation_ref: AnnotationRef,
    pub pon: PageObjectNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ActionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<HoverEvents>,
}

fn member<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

/// `sameTarget`. The TypeScript compares an index ref's `revision` with
/// `===`, which for a `RevisionToken` object is reference identity; Rust
/// compares it by value, which is what a feed holding one token object means.
fn same_target(a: Option<&HoverTarget>, b: Option<&HoverTarget>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return a.is_none() && b.is_none() };
    let (x, y) = (&a.annotation_ref, &b.annotation_ref);
    let kind = |v: &Value| member(v, "kind").as_str().map(str::to_string);
    if kind(x) != kind(y) || a.pon != b.pon {
        return false;
    }
    let number = |v: &Value, key: &str| member(v, key).as_f64();
    match kind(x).as_deref() {
        Some("objectNumber") => number(x, "annotObjectNumber") == number(y, "annotObjectNumber"),
        Some("nm") => member(x, "nm") == member(y, "nm"),
        Some("index") => {
            number(x, "index") == number(y, "index")
                && member(x, "revision") == member(y, "revision")
        }
        _ => false,
    }
}

type Dispatch = Rc<dyn Fn(ActionTrigger) -> Promise<ActionTriggerResult>>;

struct Inner {
    dispatch: Dispatch,
    delivered: RefCell<Option<HoverTarget>>,
    desired: RefCell<Option<HoverTarget>>,
    in_flight: Cell<bool>,
}

#[derive(Clone)]
pub struct HoverPump(Rc<Inner>);

/// `Promise.allSettled(promises)`, settling one turn after the last one does.
fn all_settled<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<()> {
    let (all, resolver) = Promise::pending();
    if promises.is_empty() {
        resolver.resolve(());
        return all;
    }
    let remaining = Rc::new(Cell::new(promises.len()));
    for promise in promises {
        let (remaining, resolver) = (remaining.clone(), resolver.clone());
        rt::spawn(async move {
            let _ = promise.await;
            remaining.set(remaining.get() - 1);
            if remaining.get() == 0 {
                resolver.resolve(());
            }
        });
    }
    all
}

fn hover_trigger(event: PdfAnnotationEventKind, target: &HoverTarget) -> ActionTrigger {
    ActionTrigger::Annotation {
        event,
        annotation_ref: target.annotation_ref.clone(),
        pon: target.pon,
        source: target.source.clone(),
    }
}

fn pump(inner: &Rc<Inner>) {
    if inner.in_flight.get()
        || same_target(inner.delivered.borrow().as_ref(), inner.desired.borrow().as_ref())
    {
        return;
    }
    inner.in_flight.set(true);
    let from = inner.delivered.borrow().clone();
    let to = inner.desired.borrow().clone();
    *inner.delivered.borrow_mut() = to.clone();
    let mut submissions = Vec::new();
    if let Some(from) = &from
        && from.events.as_ref().and_then(|e| e.exit) != Some(false)
    {
        submissions.push((inner.dispatch)(hover_trigger(PdfAnnotationEventKind::CursorExit, from)));
    }
    if let Some(to) = &to
        && to.events.as_ref().and_then(|e| e.enter) != Some(false)
    {
        submissions.push((inner.dispatch)(hover_trigger(PdfAnnotationEventKind::CursorEnter, to)));
    }
    let settled = all_settled(submissions);
    let inner = inner.clone();
    rt::spawn(async move {
        let _ = settled.await;
        inner.in_flight.set(false);
        // The pointer may have moved on: deliver the latest transition.
        pump(&inner);
    });
}

impl HoverPump {
    /// Report where the pointer is now (`None`: nowhere interesting).
    pub fn hover(&self, target: Option<HoverTarget>) {
        *self.0.desired.borrow_mut() = target;
        pump(&self.0);
    }

    /// Forget everything without dispatching (teardown, document switch).
    pub fn reset(&self) {
        *self.0.delivered.borrow_mut() = None;
        *self.0.desired.borrow_mut() = None;
    }
}

/// `createHoverPump(dispatch)`.
pub fn create_hover_pump(
    dispatch: impl Fn(ActionTrigger) -> Promise<ActionTriggerResult> + 'static,
) -> HoverPump {
    HoverPump(Rc::new(Inner {
        dispatch: Rc::new(dispatch),
        delivered: RefCell::new(None),
        desired: RefCell::new(None),
        in_flight: Cell::new(false),
    }))
}
