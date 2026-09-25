//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table.
//!
//! It mirrors the generator's shim in
//! `tooling/plugin-conformance/src/suites/redaction.ts`: a `console.error`
//! from this plugin logs as the callback `console.error`.

use std::any::Any;
use std::rc::Rc;

use epdf_annotation_core::AnnotationRef;
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::{
    REDACTION, RedactionAction, RedactionCapability, RedactionHost, RedactionLabelPatch,
    RedactionState, collect_pending, estimate_collateral, initial_redaction_state,
    redaction_plugin, redaction_reducer,
};

/// The adapter.
pub struct Redaction;

/// The TypeScript object literal's member order.
pub const MEMBERS: &[&str] = &[
    "canMark",
    "canApply",
    "isApplying",
    "lastResult",
    "enableRedact",
    "toggleRedact",
    "isRedactActive",
    "queueCurrentSelection",
    "preparePending",
    "getPending",
    "pendingCount",
    "estimateCollateral",
    "setLabel",
    "apply",
    "applyAll",
    "onApplied",
];

fn present(args: &[Value], index: usize) -> Option<&Value> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null())
}

/// The string members of an id list: `Set.has` never matches anything else
/// against a string id.
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn unit(result: Result<(), Error>) -> Outcome {
    match result {
        Ok(()) => Outcome::undefined(),
        Err(error) => Outcome::Threw(error),
    }
}

impl HarnessPlugin for Redaction {
    fn factories(&self) -> Vec<&'static str> {
        vec!["redaction"]
    }

    fn build(&self, _factory: &str, _config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let log = h.clone();
        Ok(redaction_plugin(RedactionHost {
            console_error: Some(Rc::new(move |message: &str, error: &Error| {
                log.log("console.error", vec![json!(message), error.to_canonical()]);
            })),
        }))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![REDACTION.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<RedactionCapability>()?;
        Some(match method {
            "canMark" => Outcome::Returned(json!(cap.can_mark())),
            "canApply" => Outcome::Returned(json!(cap.can_apply())),
            "isApplying" => Outcome::Returned(json!(cap.is_applying())),
            "lastResult" => Outcome::Returned(cap.last_result().unwrap_or(Value::Null)),
            "enableRedact" => unit(cap.enable_redact()),
            "toggleRedact" => unit(cap.toggle_redact()),
            "isRedactActive" => Outcome::from_result(cap.is_redact_active().map(|b| json!(b))),
            "queueCurrentSelection" => Outcome::promise(cap.queue_current_selection()),
            "preparePending" => Outcome::promise_unit(cap.prepare_pending()),
            "getPending" => Outcome::Returned(to_value(&cap.get_pending())),
            "pendingCount" => Outcome::Returned(json!(cap.pending_count())),
            "estimateCollateral" => {
                let ids = present(&args, 0).map(strings);
                Outcome::Returned(json!(cap.estimate_collateral(ids.as_deref())))
            }
            "setLabel" => {
                let decoded = (|| -> Result<(AnnotationRef, RedactionLabelPatch), Error> {
                    let undefined = epdf_kernel::undefined();
                    let r = from_value(args.first().unwrap_or(&undefined))?;
                    let patch = from_value(args.get(1).unwrap_or(&undefined))?;
                    Ok((r, patch))
                })();
                match decoded {
                    Ok((r, patch)) => Outcome::promise_unit(cap.set_label(r, patch)),
                    Err(error) => Outcome::Threw(error),
                }
            }
            "apply" => {
                let ids = present(&args, 0).map(strings).unwrap_or_default();
                Outcome::promise(cap.apply(&ids))
            }
            "applyAll" => Outcome::promise(cap.apply_all()),
            "onApplied" => {
                let callback = h.callback(args.first().unwrap_or(&Value::Null));
                let unsubscribe = cap.on_applied(Rc::new(move |result: &Value| {
                    match callback.call(vec![result.clone()]) {
                        Answer::Throws(error) => Err(error),
                        _ => Ok(()),
                    }
                }));
                Outcome::Returned(
                    h.register_fn(Rc::new(move |_| Outcome::Returned(json!(unsubscribe())))),
                )
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<RedactionCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn initial_redaction_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_redaction_state()))
}

/// `{changed: next !== state, state: next}`. An action outside the union is
/// the reducer's `return state`.
fn redaction_reducer_fn(a: &[Value]) -> Result<Value, Error> {
    let state: RedactionState = from_value(&a[0])?;
    let next = match from_value::<RedactionAction>(&a[1]) {
        Ok(action) => redaction_reducer(&state, &action),
        Err(_) => None,
    };
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

/// The stub context's inputs: the page list (`null` for no document) and
/// the annotations `list(pon)` answers, keyed by object number.
fn stub(a: &[Value]) -> (Vec<Value>, impl Fn(u32) -> Vec<Value> + '_) {
    let pages = a.first().and_then(Value::as_array).cloned().unwrap_or_default();
    let by_page = a.get(1);
    let list = move |pon: u32| {
        by_page
            .and_then(|b| b.get(pon.to_string()))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    (pages, list)
}

fn estimate_collateral_fn(a: &[Value]) -> Result<Value, Error> {
    let (pages, list) = stub(a);
    let ids = present(a, 2).map(strings);
    Ok(json!(estimate_collateral(&pages, &list, ids.as_deref())))
}

fn get_pending_fn(a: &[Value]) -> Result<Value, Error> {
    let (pages, list) = stub(a);
    Ok(to_value(&collect_pending(&pages, &list)))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialRedactionState", initial_redaction_state_fn),
        ("redactionReducer", redaction_reducer_fn),
        ("estimateCollateral", estimate_collateral_fn),
        ("getPending", get_pending_fn),
    ]
}
