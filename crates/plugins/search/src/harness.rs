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

use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Token, define, is_undefined, undefined};
use epdf_plugin_stage::{RevealAnchor, ScrollBehaviorKind};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

use crate::*;

// ── the scenario adapter ──────────────────────────────────────────────────────

/// The capability object literal's member order.
pub const MEMBERS: &[&str] = &[
    "canSearch",
    "search",
    "rerun",
    "clear",
    "next",
    "prev",
    "goTo",
    "query",
    "findAll",
    "status",
    "hits",
    "hitCount",
    "activeIndex",
    "activeHit",
    "hitsForPage",
    "progress",
    "errorMessage",
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
pub fn config_from_json(config: &Value) -> Result<SearchPluginConfig, Error> {
    if is_undefined(config) || config.is_null() {
        return Ok(SearchPluginConfig::default());
    }
    let reveal = match config.get("reveal") {
        None | Some(Value::Null) => None,
        Some(v) if is_undefined(v) => None,
        Some(v) => Some(reveal_from_json(v)?),
    };
    Ok(SearchPluginConfig { reveal })
}

fn mode_arg(value: Option<&Value>) -> Option<SearchMode> {
    value
        .and_then(Value::as_str)
        .map(|m| if m == "full" { SearchMode::Full } else { SearchMode::Rects })
}

impl Search {
    fn call_capability(&self, cap: &SearchCapability, method: &str, args: &[Value]) -> Outcome {
        match method {
            "canSearch" => Outcome::Returned(json!(cap.can_search(mode_arg(present(args, 0))))),
            "search" => {
                let query = match query_arg(args) {
                    Ok(query) => query,
                    Err(error) => return Outcome::Threw(error),
                };
                let exec = present(args, 1).filter(|v| !v.is_null()).map(|e| SearchExecOptions {
                    start_page: e
                        .get("startPage")
                        .and_then(Value::as_u64)
                        .map(|p| p as PageObjectNumber),
                });
                cap.search(query, exec);
                Outcome::undefined()
            }
            "rerun" => {
                cap.rerun();
                Outcome::undefined()
            }
            "clear" => {
                cap.clear();
                Outcome::undefined()
            }
            "next" => hit_result(cap.next()),
            "prev" => hit_result(cap.prev()),
            "goTo" => {
                let index = present(args, 0).and_then(Value::as_i64).unwrap_or(0) as isize;
                let reveal = match present(args, 1) {
                    None => None,
                    Some(v) => match reveal_from_json(v) {
                        Ok(reveal) => Some(reveal),
                        Err(error) => return Outcome::Threw(error),
                    },
                };
                hit_result(cap.go_to(index, reveal))
            }
            "query" => Outcome::Returned(cap.query().map_or(Value::Null, |q| to_value(&q))),
            "findAll" => {
                let query = match query_arg(args) {
                    Ok(query) => query,
                    Err(error) => return Outcome::Threw(error),
                };
                let opts = present(args, 1);
                let options = SearchFindAllOptions {
                    signal: opts
                        .and_then(|o| o.get("signal"))
                        .and_then(|s| s.get("$abortSignal"))
                        .and_then(Value::as_str)
                        .map(controller_named),
                    mode: mode_arg(opts.and_then(|o| o.get("mode"))),
                };
                Outcome::promise(cap.find_all(query, options))
            }
            "status" => Outcome::Returned(to_value(&cap.status())),
            "hits" => Outcome::Returned(to_value(&*cap.hits())),
            "hitCount" => Outcome::Returned(json!(cap.hit_count())),
            "activeIndex" => Outcome::Returned(json!(cap.active_index())),
            "activeHit" => Outcome::Returned(hit_json(cap.active_hit())),
            "hitsForPage" => {
                let pon = present(args, 0).and_then(Value::as_u64).unwrap_or(0) as PageObjectNumber;
                Outcome::Returned(to_value(&cap.hits_for_page(pon)))
            }
            "progress" => Outcome::Returned(to_value(&cap.progress())),
            "errorMessage" => {
                Outcome::Returned(cap.error_message().map_or(Value::Null, Value::String))
            }
            other => not_a_function(other),
        }
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
        _h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<SearchCapability>() {
            return Some(self.call_capability(cap, method, &args));
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
fn search_reducer_fn(a: &[Value]) -> Result<Value, Error> {
    let state: SearchState = serde_json::from_value(a[0].clone())
        .map_err(|e| Error::new(format!("[search] bad state: {e}")))?;
    let next = match serde_json::from_value::<SearchAction>(a[1].clone()) {
        Ok(action) => search_reducer(&state, &action),
        Err(_) => None,
    };
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![("initialSearchState", initial_search_state_fn), ("searchReducer", search_reducer_fn)]
}
