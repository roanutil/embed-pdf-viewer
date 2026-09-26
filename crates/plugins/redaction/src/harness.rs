//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table.
//!
//! It mirrors the generator's two shims in
//! `tooling/plugin-conformance/src/suites/redaction.ts`: a `console.error`
//! from this plugin logs as the callback `console.error`, and the
//! `redaction-tool` factory builds the suite's lens (`redaction-test/tool`),
//! which keeps v2's `enableRedact`, `toggleRedact` and `isRedactActive` on
//! top of the interaction hub.

use std::any::Any;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_annotation_core::{AnnotationRef, PageRef};
use epdf_kernel::{AnyPlugin, AnyToken, Ctx, Error, Token, Unsubscribe, define, is_undefined};
use epdf_plugin_annotation::repository::seam::annotation_key_of;
use epdf_plugin_interaction::INTERACTION;
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::*;

/// The composed capability's member order: the pending reads, marking,
/// applying, then the controller's own members.
pub const MEMBERS: &[&str] = &[
    "listPending",
    "getPending",
    "getPendingCount",
    "estimateCollateral",
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

/// The suite's `redaction-test/tool` lens: v2's redact-tool verbs over the
/// interaction hub.
#[derive(Clone)]
pub struct ToolLens {
    ctx: Ctx<Value, Value>,
}

pub static TOOL_LENS: LazyLock<Token<ToolLens>> =
    LazyLock::new(|| Token::new("redaction-test/tool"));

const TOOL_MEMBERS: &[&str] = &["enableRedact", "toggleRedact", "isRedactActive"];

impl ToolLens {
    fn call(&self, method: &str) -> Result<Value, Error> {
        let hub = self.ctx.get(&INTERACTION)?;
        let active = hub.get_active_tool_id() == "redact";
        Ok(match method {
            "enableRedact" => {
                hub.activate_tool("redact", None)?;
                Value::Null
            }
            "toggleRedact" => {
                if active {
                    hub.activate_default_tool()?;
                } else {
                    hub.activate_tool("redact", None)?;
                }
                Value::Null
            }
            _ => json!(active),
        })
    }
}

/// The adapter.
pub struct Redaction;

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn present(args: &[Value], index: usize) -> Option<&Value> {
    args.get(index).filter(|v| !is_undefined(v) && !v.is_null())
}

fn arg<T: serde::de::DeserializeOwned>(args: &[Value], index: usize) -> Result<T, Error> {
    from_value(args.get(index).unwrap_or(&epdf_kernel::undefined()))
}

fn refs_arg(args: &[Value], index: usize) -> Result<Vec<AnnotationRef>, Error> {
    match present(args, index) {
        Some(v) => from_value(v),
        None => Ok(Vec::new()),
    }
}

fn sync_promise<T: serde::Serialize + Clone + 'static>(
    result: Result<epdf_kernel::rt::Promise<T>, Error>,
) -> Outcome {
    match result {
        Ok(promise) => Outcome::promise(promise),
        Err(error) => Outcome::Threw(error),
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
    Ok(match method {
        "listPending" => {
            let filter: Option<RedactionMarkFilter> =
                present(args, 0).map(from_value).transpose()?;
            Outcome::Returned(to_value(&cap.list_pending(filter.as_ref())))
        }
        "getPending" => {
            let r: AnnotationRef = arg(args, 0)?;
            Outcome::Returned(cap.get_pending(&r).map_or(Value::Null, |m| to_value(&m)))
        }
        "getPendingCount" => {
            let page: Option<PageRef> = present(args, 0).map(from_value).transpose()?;
            Outcome::Returned(json!(cap.get_pending_count(page.as_ref())))
        }
        "estimateCollateral" => {
            let refs: Option<Vec<AnnotationRef>> = present(args, 0).map(from_value).transpose()?;
            Outcome::Returned(to_value(&cap.estimate_collateral(refs.as_deref())))
        }
        "markSelection" => Outcome::promise(cap.mark_selection()),
        "markArea" => {
            let page: PageRef = arg(args, 0)?;
            let bounds = args.get(1).cloned().unwrap_or(Value::Null);
            sync_promise(cap.mark_area(&page, bounds))
        }
        "markPage" => sync_promise(cap.mark_page(&arg(args, 0)?)),
        "markMatches" => {
            let pages: Option<Vec<PageRef>> = present(args, 1)
                .and_then(|o| o.get("pages"))
                .filter(|v| !is_undefined(v) && !v.is_null())
                .map(from_value)
                .transpose()?;
            Outcome::promise(cap.mark_matches(arg(args, 0)?, pages))
        }
        "unmark" => Outcome::promise(cap.unmark(refs_arg(args, 0)?)),
        "clearPending" => Outcome::promise(cap.clear_pending()),
        "updateLabel" => {
            let r: AnnotationRef = arg(args, 0)?;
            Outcome::promise_unit(cap.update_label(r, arg(args, 1)?))
        }
        // `refs.map(annotationKey)` runs before any promise exists.
        "apply" => match args.first() {
            Some(v) if !is_undefined(v) && !v.is_null() => {
                Outcome::promise(cap.apply(&from_value::<Vec<AnnotationRef>>(v)?))
            }
            other => {
                let what = if other.is_some_and(Value::is_null) { "null" } else { "undefined" };
                Outcome::Threw(Error::named(
                    "TypeError",
                    format!("Cannot read properties of {what} (reading 'map')"),
                ))
            }
        },
        "applyPages" => {
            let pages: Vec<PageRef> =
                present(args, 0).map(from_value).transpose()?.unwrap_or_default();
            Outcome::promise(cap.apply_pages(&pages))
        }
        "applyAll" => Outcome::promise(cap.apply_all()),
        "canMark" => Outcome::Returned(json!(cap.can_mark())),
        "canApply" => Outcome::Returned(json!(cap.can_apply())),
        "isApplying" => Outcome::Returned(json!(cap.is_applying())),
        "getLastResult" => Outcome::Returned(cap.get_last_result().unwrap_or(Value::Null)),
        "onApplied" => hook(h, args.first(), |listener| cap.on_applied(listener)),
        "onPendingChanged" => hook(h, args.first(), |listener| cap.on_pending_changed(listener)),
        other => not_a_function(other),
    })
}

impl HarnessPlugin for Redaction {
    fn factories(&self) -> Vec<&'static str> {
        vec!["redaction", "redaction-tool"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "redaction-tool" {
            return Ok(define::<Value, Value>("redaction-test/tool")
                .document_scoped()
                .requires(INTERACTION.any())
                .capability(&TOOL_LENS, |ctx| Ok(ToolLens { ctx }))
                .build());
        }
        let mut config: RedactionConfig = if is_undefined(config) || config.is_null() {
            RedactionConfig::default()
        } else {
            from_value(config)?
        };
        let log = h.clone();
        config.host = RedactionHost {
            console_error: Some(Rc::new(move |message: &str, error: &Error| {
                log.log("console.error", vec![json!(message), error.to_canonical()]);
            })),
        };
        Ok(redaction_plugin(config))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![REDACTION.any().clone(), TOOL_LENS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<RedactionCapability>() {
            return Some(call_capability(cap, method, &args, h).unwrap_or_else(Outcome::Threw));
        }
        let lens = capability.downcast_ref::<ToolLens>()?;
        Some(if TOOL_MEMBERS.contains(&method) {
            Outcome::from_result(
                lens.call(method).map(|v| if v.is_null() { epdf_kernel::undefined() } else { v }),
            )
        } else {
            not_a_function(method)
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<RedactionCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        capability.downcast_ref::<ToolLens>()?;
        Some(TOOL_MEMBERS.to_vec())
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

/// The generator's `idRef(id)` then `annotationKey`: a v2 mark id as the
/// key of the ref it stands for (`obj:N` on page 1, `nm:X` on page 2,
/// `idx:P:I`, and any other string as an `nm` on page 1).
fn id_key(id: &Value) -> Option<String> {
    let Some(id) = id.as_str() else { return annotation_key_of(id) };
    if let Some(n) = id.strip_prefix("obj:") {
        let n: f64 = n.parse().unwrap_or(f64::NAN);
        return annotation_key_of(&json!({
            "kind": "objectNumber",
            "page": PageRef::new(1),
            "annotObjectNumber": n,
        }));
    }
    if let Some(nm) = id.strip_prefix("nm:") {
        return Some(format!("nm:2:{nm}"));
    }
    if let Some(rest) = id.strip_prefix("idx:") {
        let mut parts = rest.split(':');
        let pon = parts.next().unwrap_or_default();
        let index = parts.next().unwrap_or_default();
        return Some(format!("idx:{pon}:{index}"));
    }
    Some(format!("nm:1:{id}"))
}

/// The generator's stub: `pages` is the document's page list (`null` for no
/// document), each `{index, pageObjectNumber}`; `byPage` is what
/// `listRaw({page})` answers per object number; the filter is a ref list.
fn estimate_collateral_fn(a: &[Value]) -> Result<Value, Error> {
    let pages: Vec<PageRef> = a
        .first()
        .and_then(Value::as_array)
        .map(|pages| {
            pages
                .iter()
                .filter_map(|p| p.get("pageObjectNumber").and_then(Value::as_u64))
                .map(|pon| PageRef::new(pon as u32))
                .collect()
        })
        .unwrap_or_default();
    let by_page = a.get(1).cloned().unwrap_or(Value::Null);
    let list_raw = |page: &PageRef| {
        by_page
            .get(page.page_object_number.to_string())
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let wanted: Option<HashSet<String>> = present(a, 2).map(|ids| {
        ids.as_array().map(|ids| ids.iter().filter_map(id_key).collect()).unwrap_or_default()
    });
    Ok(to_value(&estimate_collateral(&pages, &list_raw, wanted.as_ref())))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialRedactionState", initial_redaction_state_fn),
        ("redactionReducer", redaction_reducer_fn),
        ("estimateCollateral", estimate_collateral_fn),
    ]
}
