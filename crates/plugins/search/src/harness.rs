//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the transitions suite calls by its TypeScript names.
//!
//! It mirrors the shim in `tooling/plugin-conformance/src/suites/search.ts`:
//! a `findAll` or `search` option `signal: {"$abortSignal": name}` is the
//! signal of a controller named `name`, which the `search-signals` test
//! plugin's `abort(name, reason?)` aborts, with the platform's `AbortError`
//! DOMException when no reason is given.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use epdf_plugin_stage::{RevealAnchor, ScrollBehaviorKind};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::*;

// ── the scenario adapter ──────────────────────────────────────────────────────

/// The capability object literal's member order: the settings API spread
/// first, then the members `controller.ts` lists.
pub const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
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
    "onProgressChanged",
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

fn member<'a>(options: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    options?.get(key).filter(|v| !is_undefined(v))
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

/// A promise whose value prints as JSON, converted the moment it settles so
/// the conversion costs no turn: the TypeScript hands the caller the
/// capability's own promise.
fn as_json<T: Serialize + Clone + 'static>(promise: Promise<T>) -> Outcome {
    let (out, resolver) = Promise::pending();
    promise.on_settle(move |result| resolver.settle(result.clone().map(|v| to_value(&v))));
    Outcome::Promise(out)
}

fn hit_result(result: Result<Option<Rc<SearchHit>>, Error>) -> Outcome {
    Outcome::from_result(result.map(|hit| hit.map_or(Value::Null, |h| to_value(&*h))))
}

fn query_arg(args: &[Value]) -> Result<SearchQuery, Error> {
    from_value(args.first().unwrap_or(&undefined()))
}

/// A `PageRef | number` argument.
fn page_target(value: &Value) -> Option<PageTarget> {
    match value {
        Value::Number(n) => n.as_f64().map(PageTarget::Index),
        other => PageRef::from_json(other).map(PageTarget::Ref),
    }
}

/// A named abort controller's signal, from `{"$abortSignal": name}`.
fn signal_member(options: Option<&Value>) -> Option<AbortSignal> {
    member(options, "signal")
        .and_then(|s| s.get("$abortSignal"))
        .and_then(Value::as_str)
        .map(controller_named)
}

fn reveal_arg(args: &[Value], i: usize) -> Result<Option<SearchRevealOptions>, Error> {
    present(args, i).filter(|v| !v.is_null()).map(reveal_from_json).transpose()
}

/// Subscribe a scripted listener; the outcome is the unsubscribe, as a ref.
fn hook<E: Serialize + 'static>(
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
/// leaves the setting, `undefined` overrides it away, and a `null` anchor is
/// present with no axes.
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

/// The generator's `SearchConfig` from a vector's config.
pub fn config_from_json(config: &Value) -> SearchConfig {
    if is_undefined(config) || config.is_null() {
        return SearchConfig::default();
    }
    SearchConfig::from_json(config)
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
            "getSettings" => Outcome::Returned(to_value(&cap.get_settings())),
            "updateSettings" => {
                cap.update_settings(&SearchConfig::from_json(args.first().unwrap_or(&Value::Null)));
                Outcome::undefined()
            }
            "resetSettings" => {
                cap.reset_settings();
                Outcome::undefined()
            }
            "onSettingsChanged" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_settings_changed(f))
            }
            "canSearch" => {
                let snippets = member(present(args, 0), "snippets").and_then(Value::as_bool);
                Outcome::Returned(json!(cap.can_search(snippets)))
            }
            "search" => {
                let options = present(args, 1);
                let options = SearchOptions {
                    from: member(options, "from").and_then(page_target),
                    signal: signal_member(options),
                };
                as_json(cap.search(query_arg(args)?, options))
            }
            "refresh" => {
                as_json(cap.refresh(OperationOptions { signal: signal_member(present(args, 0)) }))
            }
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
                let target = match present(args, 0) {
                    Some(Value::Number(n)) => HitTarget::Index(n.as_f64().unwrap_or(0.0) as isize),
                    Some(hit) => HitTarget::Hit(Rc::new(from_value::<SearchHit>(hit)?)),
                    None => HitTarget::Index(0),
                };
                hit_result(cap.go_to_hit(target, reveal_arg(args, 1)?))
            }
            "revealActiveHit" => Outcome::from_result(
                cap.reveal_active_hit(reveal_arg(args, 0)?).map(|()| undefined()),
            ),
            "getQuery" => Outcome::Returned(cap.get_query().map_or(Value::Null, |q| to_value(&q))),
            "getStatus" => Outcome::Returned(to_value(&cap.get_status())),
            "listHits" => {
                let page = member(present(args, 0), "page").and_then(page_target);
                Outcome::Returned(to_value(&*cap.list_hits(page)))
            }
            "getHitCount" => {
                Outcome::Returned(json!(cap.get_hit_count(present(args, 0).and_then(page_target))))
            }
            "listPagesWithHits" => Outcome::Returned(to_value(&cap.list_pages_with_hits())),
            "getActiveHitIndex" => Outcome::Returned(json!(cap.get_active_hit_index())),
            "getActiveHit" => {
                Outcome::Returned(cap.get_active_hit().map_or(Value::Null, |h| to_value(&*h)))
            }
            "getProgress" => Outcome::Returned(to_value(&*cap.get_progress())),
            "getError" => Outcome::Returned(cap.get_error().map_or(Value::Null, |e| to_value(&e))),
            "findAll" => {
                let options = present(args, 1);
                let options = SearchFindAllOptions {
                    snippets: member(options, "snippets").and_then(Value::as_bool),
                    signal: signal_member(options),
                };
                as_json(cap.find_all(query_arg(args)?, options))
            }
            "onStarted" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_started(f))
            }
            "onProgressChanged" => {
                let cap = cap.clone();
                hook(h, present(args, 0), move |f| cap.on_progress_changed(f))
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
        Ok(search_plugin(config_from_json(config)))
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

fn state_arg(args: &[Value]) -> Result<Rc<SearchState>, Error> {
    serde_json::from_value(args.first().cloned().unwrap_or(Value::Null))
        .map(Rc::new)
        .map_err(|e| Error::new(format!("[search] bad state: {e}")))
}

fn arg<T: serde::de::DeserializeOwned>(args: &[Value], i: usize) -> Result<T, Error> {
    from_value(args.get(i).unwrap_or(&undefined()))
}

/// `{changed: next !== state, state: next}`.
fn observed(state: &Rc<SearchState>, next: Rc<SearchState>) -> Value {
    json!({ "changed": !Rc::ptr_eq(state, &next), "state": to_value(&*next) })
}

fn initial_search_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_search_state()))
}

fn start_session_fn(a: &[Value]) -> Result<Value, Error> {
    let state = state_arg(a)?;
    let next = start_session(&state, &arg(a, 1)?, &arg::<String>(a, 2)?);
    Ok(observed(&state, next))
}

fn append_hits_fn(a: &[Value]) -> Result<Value, Error> {
    let state = state_arg(a)?;
    let hits: Vec<Rc<SearchHit>> = arg(a, 1)?;
    let next = append_hits(&state, &hits, arg(a, 2)?);
    Ok(observed(&state, next))
}

fn complete_session_fn(a: &[Value]) -> Result<Value, Error> {
    let state = state_arg(a)?;
    Ok(observed(&state, complete_session(&state)))
}

fn cancel_session_fn(a: &[Value]) -> Result<Value, Error> {
    let state = state_arg(a)?;
    Ok(observed(&state, cancel_session(&state)))
}

fn fail_session_fn(a: &[Value]) -> Result<Value, Error> {
    let state = state_arg(a)?;
    let next = fail_session(&state, &arg(a, 1)?);
    Ok(observed(&state, next))
}

fn set_active_hit_fn(a: &[Value]) -> Result<Value, Error> {
    let state = state_arg(a)?;
    let next = set_active_hit(&state, arg(a, 1)?);
    Ok(observed(&state, next))
}

fn clear_session_fn(a: &[Value]) -> Result<Value, Error> {
    let state = state_arg(a)?;
    Ok(observed(&state, clear_session(&state)))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialSearchState", initial_search_state_fn),
        ("startSession", start_session_fn),
        ("appendHits", append_hits_fn),
        ("completeSession", complete_session_fn),
        ("cancelSession", cancel_session_fn),
        ("failSession", fail_session_fn),
        ("setActiveHit", set_active_hit_fn),
        ("clearSession", clear_session_fn),
    ]
}
