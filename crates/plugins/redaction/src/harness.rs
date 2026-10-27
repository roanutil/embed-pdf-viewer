//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table. The generator is `tooling/plugin-conformance/src/suites/redaction.ts`
//! with `redaction-fixtures.ts`: its `redaction-current` factory is
//! `redactionPlugin(config)` and `redaction-search` is `searchPlugin()`,
//! neither with a shim. The function table's `estimateCollateral` runs the
//! shipping estimate over a read-only `annotation.list({pages})` port
//! (`redaction-read-inputs.ts`), as [`estimate_collateral`] takes one.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe, is_undefined, undefined};
use epdf_plugin_search::{SearchConfig, SearchQuery, search_plugin};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::model::RedactionState;
use crate::*;

/// The adapter.
pub struct Redaction;

/// The capability's member order: the areas' slices as `composeApi`
/// spreads them (settings, the pending reads, marking, applying), then the
/// controller's own members.
pub const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "listPending",
    "getPending",
    "getPendingCount",
    "estimateCollateral",
    "canUnmark",
    "canUpdateLabel",
    "markSelection",
    "markArea",
    "markPage",
    "markMatches",
    "unmark",
    "clearPending",
    "updateLabel",
    "apply",
    "applyPages",
    "applyAll",
    "canMark",
    "canApply",
    "isApplying",
    "getLastResult",
    "onApplied",
    "onPendingChanged",
];

fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or_else(undefined)
}

fn present(args: &[Value], index: usize) -> Option<&Value> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null())
}

/// A `PageRef | number` argument.
fn page_target(value: &Value) -> PageTarget {
    match value.as_f64() {
        Some(index) => PageTarget::Index(index),
        None => match PageRef::from_json(value) {
            Some(page) => PageTarget::Ref(page),
            None => PageTarget::Index(f64::NAN),
        },
    }
}

fn page_targets(value: &Value) -> Vec<PageTarget> {
    value.as_array().into_iter().flatten().map(page_target).collect()
}

fn refs(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

/// `RedactionLabelPatch`: `overlayText: null` clears, an absent member
/// leaves it.
fn label_patch(value: &Value) -> RedactionLabelPatch {
    let member = |key: &str| value.get(key).filter(|v| !is_undefined(v));
    RedactionLabelPatch {
        overlay_text: member("overlayText").map(|v| v.as_str().map(str::to_string)),
        repeat: member("repeat").and_then(Value::as_bool),
    }
}

/// Subscribe a scripted listener; the outcome is the unsubscribe, as a ref.
fn hook<E: serde::Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![to_value(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn call_capability(
    cap: &RedactionCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let none = OperationOptions::default;
    Ok(match method {
        "getSettings" => Outcome::Returned((*cap.settings_json()).clone()),
        "updateSettings" => {
            cap.update_settings_json(&arg(args, 0));
            Outcome::undefined()
        }
        "resetSettings" => {
            cap.reset_settings();
            Outcome::undefined()
        }
        "onSettingsChanged" => {
            let callback = h.callback(args.first().unwrap_or(&Value::Null));
            let off = cap.on_settings_changed_json(move |settings, changed| {
                match callback.call(vec![json!({ "settings": settings, "changed": changed })]) {
                    Answer::Throws(error) => Err(error),
                    _ => Ok(()),
                }
            });
            Outcome::Returned(h.register_fn(Rc::new(move |_| {
                off.call();
                Outcome::undefined()
            })))
        }
        "listPending" => {
            let filter = present(args, 0).map(|f| RedactionMarkFilter {
                page: f.get("page").filter(|v| !is_undefined(v)).map(page_target),
            });
            Outcome::Returned(to_value(&*cap.list_pending(filter)))
        }
        "getPending" => {
            Outcome::Returned(cap.get_pending(&arg(args, 0)).map_or(Value::Null, |m| to_value(&m)))
        }
        "getPendingCount" => {
            let page = args.first().filter(|v| !is_undefined(v)).map(page_target);
            Outcome::Returned(json!(cap.get_pending_count(page)))
        }
        "estimateCollateral" => {
            let refs = args.first().filter(|v| !is_undefined(v)).map(refs);
            Outcome::Returned(to_value(&cap.estimate_collateral(refs.as_deref())))
        }
        "canUnmark" => Outcome::Returned(json!(cap.can_unmark(&arg(args, 0)))),
        "canUpdateLabel" => Outcome::Returned(json!(cap.can_update_label(&arg(args, 0)))),
        "markSelection" => Outcome::promise(cap.mark_selection(none())),
        "markArea" => {
            Outcome::promise(cap.mark_area_json(page_target(&arg(args, 0)), arg(args, 1), none()))
        }
        "markPage" => Outcome::promise(cap.mark_page(page_target(&arg(args, 0)), none())),
        "markMatches" => {
            let query: SearchQuery = from_value(&arg(args, 0))?;
            let pages = present(args, 1)
                .and_then(|o| o.get("pages"))
                .filter(|v| !is_undefined(v))
                .map(page_targets);
            Outcome::promise(cap.mark_matches(query, pages, none()))
        }
        "unmark" => Outcome::promise(cap.unmark(refs(&arg(args, 0)), none())),
        "clearPending" => Outcome::promise(cap.clear_pending(none())),
        "updateLabel" => {
            Outcome::promise(cap.update_label(arg(args, 0), label_patch(&arg(args, 1)), none()))
        }
        "apply" => match arg(args, 0) {
            Value::Array(list) => Outcome::promise(cap.apply(&list, none())),
            // `refs.map(annotationKey)` in the queued operation
            // (`write/apply.ts:60`): the promise rejects with the
            // TypeError. The Rust signature takes a slice, so the harness
            // rejects it here, before the queue turn.
            other => {
                let what = if other.is_null() { "null" } else { "undefined" };
                let error = Error::named(
                    "TypeError",
                    format!("Cannot read properties of {what} (reading 'map')"),
                );
                Outcome::promise(epdf_kernel::rt::Promise::<RedactionApplyResult>::rejected(error))
            }
        },
        "applyPages" => Outcome::promise(cap.apply_pages(&page_targets(&arg(args, 0)), none())),
        "applyAll" => Outcome::promise(cap.apply_all(none())),
        "canMark" => Outcome::Returned(json!(cap.can_mark())),
        "canApply" => Outcome::Returned(json!(cap.can_apply())),
        "isApplying" => Outcome::Returned(json!(cap.is_applying())),
        "getLastResult" => {
            Outcome::Returned(cap.get_last_result().map_or(Value::Null, |r| (*r).clone()))
        }
        "onApplied" => hook(h, args.first(), |listener| cap.on_applied(listener)),
        "onPendingChanged" => hook(h, args.first(), |listener| cap.on_pending_changed(listener)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for Redaction {
    fn factories(&self) -> Vec<&'static str> {
        vec!["redaction-current", "redaction-search"]
    }

    fn build(&self, factory: &str, config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "redaction-search" {
            return Ok(search_plugin(SearchConfig::default()));
        }
        let registered = Some(config.clone()).filter(|c| !c.is_null() && !is_undefined(c));
        Ok(redaction_plugin_json(registered))
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
        Some(call_capability(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<RedactionCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn state_arg(a: &[Value], index: usize) -> Result<Rc<RedactionState>, Error> {
    Ok(Rc::new(from_value(&arg(a, index))?))
}

fn initial_redaction_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_redaction_state()))
}

fn start_apply_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*start_apply(state_arg(a, 0)?)))
}

fn finish_apply_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*finish_apply(state_arg(a, 0)?)))
}

fn set_last_result_fn(a: &[Value]) -> Result<Value, Error> {
    let result = Rc::new(arg(a, 1));
    Ok(to_value(&*set_last_result(state_arg(a, 0)?, &result)))
}

/// `markOf(record, pageIndex)`. The typed mark holds a whole display index,
/// so a `pageIndex` that is not one (which `markOf` would pass through) is
/// refused rather than cast.
fn mark_of_fn(a: &[Value]) -> Result<Value, Error> {
    let index =
        arg(a, 1).as_f64().filter(|n| n.fract() == 0.0 && n.abs() <= i64::MAX as f64).ok_or_else(
            || Error::named("TypeError", "[redaction harness] markOf takes a whole pageIndex"),
        )?;
    Ok(to_value(&mark_of(&arg(a, 0), index as i64)?))
}

/// The generator's read port: `pages` is the document's page layouts
/// (`null` for no document), `records` what the annotation plane holds, and
/// `list({pages: [page]})` the records on that page.
fn estimate_collateral_fn(a: &[Value]) -> Result<Value, Error> {
    let pages: Vec<PageRef> = arg(a, 0)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|layout| layout.get("ref").and_then(PageRef::from_json))
        .collect();
    let records = arg(a, 1);
    let list = |page: PageRef| -> Rc<Vec<Value>> {
        Rc::new(
            records
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| {
                    r.get("page").and_then(|p| p.get("objectNumber")).and_then(Value::as_f64)
                        == Some(page.page_object_number as f64)
                })
                .cloned()
                .collect(),
        )
    };
    let refs = args_refs(a, 2);
    Ok(to_value(&estimate_collateral(&pages, &list, refs.as_deref())))
}

fn args_refs(a: &[Value], index: usize) -> Option<Vec<Value>> {
    a.get(index).filter(|v| !is_undefined(v)).map(refs)
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialRedactionState", initial_redaction_state_fn),
        ("startApply", start_apply_fn),
        ("finishApply", finish_apply_fn),
        ("setLastResult", set_last_result_fn),
        ("markOf", mark_of_fn),
        ("estimateCollateral", estimate_collateral_fn),
    ]
}
