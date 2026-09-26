//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the reducer suite calls by its TypeScript names.
//!
//! It mirrors the shim in `tooling/plugin-conformance/src/suites/search.ts`:
//! a `findAll` option `signal: {"$abortSignal": name}` is the signal of a
//! controller named `name`, which the `search-signals` test plugin's
//! `abort(name, reason?)` aborts, with the platform's `AbortError`
//! DOMException when no reason is given.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use epdf_plugin_stage::{RevealAnchor, ScrollBehaviorKind};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::*;

// ── the scenario adapter ──────────────────────────────────────────────────────

/// The capability object literal's member order.
pub const MEMBERS: &[&str] = &[
    "canSearch",
    "search",
    "refresh",
    "cancel",
    "clear",
    "nextHit",
    "previousHit",
    "goToHit",
    "revealActiveHit",
    "getQuery",
    "getStatus",
    "listHits",
    "getHitCount",
    "listPagesWithHits",
    "getActiveHitIndex",
    "getActiveHit",
    "getProgress",
    "getError",
    "findAll",
    "onStarted",
    "onProgress",
    "onCompleted",
    "onCancelled",
    "onFailed",
    "onActiveHitChanged",
    "onCleared",
];

thread_local! {
    /// The case's abort controllers by name; a search build starts a case.
    static CONTROLLERS: RefCell<HashMap<String, AbortSignal>> = RefCell::new(HashMap::new());
}

fn controller_named(name: &str) -> AbortSignal {
    CONTROLLERS.with(|c| c.borrow_mut().entry(name.to_string()).or_default().clone())
}

/// `controller.abort()` with no reason: the platform's DOMException.
pub fn dom_abort_error() -> Error {
    Error::named("AbortError", "This operation was aborted")
}

/// The `search-signals` test plugin's capability.
#[derive(Clone)]
pub struct Signals;

pub static SIGNALS: LazyLock<Token<Signals>> = LazyLock::new(|| Token::new("search-test/signals"));

pub struct Search;

fn present(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn hit_json(hit: Option<SearchHit>) -> Value {
    hit.map_or(Value::Null, |h| to_value(&h))
}

fn hit_result(result: Result<Option<SearchHit>, Error>) -> Outcome {
    Outcome::from_result(result.map(hit_json))
}

fn query_arg(args: &[Value]) -> Result<SearchQuery, Error> {
    from_value(args.first().unwrap_or(&undefined()))
}

/// A `PageRef` member of an options object.
fn page_member(options: Option<&Value>, key: &str) -> Option<PageRef> {
    options.and_then(|o| o.get(key)).and_then(PageRef::from_json)
}

/// A named abort controller's signal, from `{"$abortSignal": name}`.
fn signal_member(options: Option<&Value>) -> Option<AbortSignal> {
    options
        .and_then(|o| o.get("signal"))
        .and_then(|s| s.get("$abortSignal"))
        .and_then(Value::as_str)
        .map(controller_named)
}

fn reveal_arg(args: &[Value], i: usize) -> Result<Option<SearchRevealOptions>, Error> {
    present(args, i).map(reveal_from_json).transpose()
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

/// A `SearchRevealOptions` object as the spread reads it: an absent key
/// leaves the layer below, `undefined` overrides it away, and a `null`
/// anchor is present with no axes.
pub fn reveal_from_json(value: &Value) -> Result<SearchRevealOptions, Error> {
    let anchor = match value.get("anchor") {
        None => None,
        Some(v) if is_undefined(v) => Some(None),
        Some(Value::Null) => Some(Some(RevealAnchor::default())),
        Some(v) => Some(Some(from_value::<RevealAnchor>(v)?)),
    };
    let behavior = match value.get("behavior") {
        None => None,
        Some(v) if is_undefined(v) || v.is_null() => Some(None),
        Some(v) => Some(Some(from_value::<ScrollBehaviorKind>(v)?)),
    };
    Ok(SearchRevealOptions { anchor, behavior })
}

/// The generator's `SearchPluginConfig` from a vector's config.
pub fn config_from_json(config: &Value) -> Result<SearchConfig, Error> {
    if is_undefined(config) || config.is_null() {
        return Ok(SearchConfig::default());
    }
    let reveal = match config.get("reveal") {
        None | Some(Value::Null) => None,
        Some(v) if is_undefined(v) => None,
        Some(v) => Some(reveal_from_json(v)?),
    };
    Ok(SearchConfig { reveal })
}

fn mode_arg(value: Option<&Value>) -> Option<SearchMode> {
    value
        .and_then(Value::as_str)
        .map(|m| if m == "full" { SearchMode::Full } else { SearchMode::Rects })
}

impl Search {
    fn call_capability(
        &self,
        cap: &SearchCapability,
        method: &str,
        args: &[Value],
        h: &Harness,
    ) -> Result<Outcome, Error> {
        Ok(match method {
            "canSearch" => Outcome::Returned(json!(cap.can_search(mode_arg(present(args, 0))))),
            "search" => {
                let options = present(args, 1).filter(|v| !v.is_null());
                // The generator's shim converts `findAll`'s signal only.
                let options =
                    SearchOptions { start_page: page_member(options, "startPage"), signal: None };
                Outcome::promise(cap.search(query_arg(args)?, options))
            }
            "refresh" => Outcome::promise(cap.refresh(OperationOptions::default())),
            "cancel" => {
                cap.cancel();
                Outcome::undefined()
            }
            "clear" => {
                cap.clear();
                Outcome::undefined()
            }
            "nextHit" => hit_result(cap.next_hit(reveal_arg(args, 0)?)),
            "previousHit" => hit_result(cap.previous_hit(reveal_arg(args, 0)?)),
            "goToHit" => {
                let index = present(args, 0).and_then(Value::as_i64).unwrap_or(0) as isize;
                hit_result(cap.go_to_hit(index, reveal_arg(args, 1)?))
            }
            "revealActiveHit" => Outcome::from_result(
                cap.reveal_active_hit(reveal_arg(args, 0)?).map(|()| undefined()),
            ),
            "getQuery" => Outcome::Returned(cap.get_query().map_or(Value::Null, |q| to_value(&q))),
            "getStatus" => Outcome::Returned(to_value(&cap.get_status())),
            "listHits" => {
                Outcome::Returned(to_value(&*cap.list_hits(page_member(present(args, 0), "page"))))
            }
            "getHitCount" => Outcome::Returned(json!(
                cap.get_hit_count(present(args, 0).and_then(PageRef::from_json))
            )),
            "listPagesWithHits" => Outcome::Returned(Value::Array(
                cap.list_pages_with_hits().into_iter().map(PageRef::to_json).collect(),
            )),
            "getActiveHitIndex" => Outcome::Returned(json!(cap.get_active_hit_index())),
            "getActiveHit" => Outcome::Returned(hit_json(cap.get_active_hit())),
            "getProgress" => Outcome::Returned(to_value(&cap.get_progress())),
            "getError" => Outcome::Returned(cap.get_error().map_or(Value::Null, |e| to_value(&e))),
            "findAll" => {
                let options = present(args, 1);
                let options = SearchFindAllOptions {
                    signal: signal_member(options),
                    mode: mode_arg(options.and_then(|o| o.get("mode"))),
                };
                Outcome::promise(cap.find_all(query_arg(args)?, options))
            }
            "onStarted" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_started(f))
            }
            "onProgress" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_progress(f))
            }
            "onCompleted" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_completed(f))
            }
            "onCancelled" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_cancelled(f))
            }
            "onFailed" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_failed(f))
            }
            "onActiveHitChanged" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_active_hit_changed(f))
            }
            "onCleared" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_cleared(f))
            }
            other => not_a_function(other),
        })
    }
}

impl HarnessPlugin for Search {
    fn factories(&self) -> Vec<&'static str> {
        vec!["search", "search-signals"]
    }

    fn build(&self, factory: &str, config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "search-signals" {
            return Ok(define::<Value, Value>("search-test/signals")
                .capability(&SIGNALS, |_| Ok(Signals))
                .build());
        }
        CONTROLLERS.with(|c| c.borrow_mut().clear());
        Ok(search_plugin(config_from_json(config)?))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![SEARCH.any().clone(), SIGNALS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<SearchCapability>() {
            return Some(
                self.call_capability(cap, method, &args, h).unwrap_or_else(Outcome::Threw),
            );
        }
        capability.downcast_ref::<Signals>()?;
        Some(match method {
            "abort" => {
                let name = present(&args, 0).and_then(Value::as_str).unwrap_or_default();
                let reason = present(&args, 1).map_or_else(dom_abort_error, Error::from_spec);
                controller_named(name).abort(reason);
                Outcome::undefined()
            }
            other => not_a_function(other),
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<SearchCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        capability.downcast_ref::<Signals>()?;
        Some(vec!["abort"])
    }
}

// ── the function table ────────────────────────────────────────────────────────

fn initial_search_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_search_state()))
}

/// `{changed: next !== state, state: next}`. An action outside the union is
/// the reducer's `return state`.
fn reduce_search_fn(a: &[Value]) -> Result<Value, Error> {
    let state: SearchState = serde_json::from_value(a[0].clone())
        .map_err(|e| Error::new(format!("[search] bad state: {e}")))?;
    let next = match serde_json::from_value::<SearchAction>(a[1].clone()) {
        Ok(action) => reduce_search(&state, &action),
        Err(_) => None,
    };
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![("initialSearchState", initial_search_state_fn), ("reduceSearch", reduce_search_fn)]
}
