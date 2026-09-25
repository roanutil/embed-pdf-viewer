//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the pure suite calls.
//!
//! A command's functions arrive as `$fn` specs. Each becomes a function
//! that logs `{fn, args: [ctx]}` and answers as the spec says, and one
//! written with `uses` first does those things with its `CommandCtx` and
//! logs what came back as `<name>:uses`. The generator
//! (`tooling/plugin-conformance/src/suites/commands.ts`) documents the
//! format; this is its Rust half.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined, undefined};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Map, Value, json};

use crate::capability::CommandRegistry;
use crate::types::{
    AccentDerivation, CommandCtx, CommandDef, CommandsAction, CommandsConfig, CommandsState,
    Derivation, IconAccent, Panel, ResolvedCommand, RunFn, Shortcut,
};
use crate::{
    COMMANDS, CommandsCapability, commands_plugin, commands_reducer, initial_commands_state,
    register_command, resolved_commands_equal,
};

pub struct Commands;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "register",
    "unregister",
    "has",
    "ids",
    "resolve",
    "search",
    "menuTarget",
    "execute",
    "matchStroke",
    "disabledCategories",
    "isCategoryDisabled",
    "disableCategory",
    "enableCategory",
    "setDisabledCategories",
];

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn str_arg(args: &[Value], i: usize) -> Option<&str> {
    arg(args, i).and_then(Value::as_str)
}

fn bad(what: &str, e: impl std::fmt::Display) -> Error {
    Error::new(format!("[commands harness] {what}: {e}"))
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn opt_string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_string)
}

// ── scripted command functions ────────────────────────────────────────────────

/// The canonical form of a `CommandCtx` in a callback's arguments.
fn ctx_json(c: &CommandCtx) -> Value {
    json!({
        "documentId": c.document_id(),
        "core": { "$function": true },
        "get": { "$function": true },
        "tryGet": { "$function": true },
    })
}

fn outcome_value(outcome: Outcome) -> Value {
    match outcome {
        Outcome::Returned(value) => json!({ "returned": value }),
        Outcome::Threw(error) => json!({ "threw": error.to_canonical() }),
        Outcome::Promise(_) => json!({ "returned": undefined() }),
    }
}

/// One `uses` entry, done with the context a command function was handed.
fn run_use(spec: &Value, c: &CommandCtx, h: &Harness) -> Value {
    if let Some(member) = spec.get("core").and_then(Value::as_str) {
        let core = serde_json::to_value(&*c.core()).unwrap_or(Value::Null);
        return json!({ "returned": core.get(member).cloned().unwrap_or_else(undefined) });
    }
    let capability = if let Some(name) = spec.get("get").and_then(Value::as_str) {
        match c.get_any(&h.token(name, None)) {
            Ok(capability) => capability,
            Err(error) => return json!({ "threw": error.to_canonical() }),
        }
    } else {
        let name = spec.get("tryGet").and_then(Value::as_str).unwrap_or_default();
        match c.try_get_any(&h.token(name, None)) {
            Some(capability) => capability,
            None => return json!({ "returned": null }),
        }
    };
    let Some(method) = spec.get("call").and_then(Value::as_str) else {
        return json!({ "returned": "resolved" });
    };
    let args = spec.get("args").and_then(Value::as_array).cloned().unwrap_or_default();
    outcome_value(h.call(&capability, method, args))
}

/// A `$fn` spec as a command function: logs the call, does its `uses`,
/// logs those, then answers.
fn scripted(spec: &Value, h: &Harness) -> Rc<dyn Fn(&CommandCtx) -> Answer> {
    let callback = h.callback(spec);
    let uses = spec.get("uses").and_then(Value::as_array).cloned();
    let h = h.clone();
    Rc::new(move |c: &CommandCtx| {
        let answer = callback.call(vec![ctx_json(c)]);
        if let Some(uses) = &uses {
            let results = uses.iter().map(|u| run_use(u, c, &h)).collect();
            h.log(&format!("{}:uses", callback.name()), results);
        }
        answer
    })
}

/// JavaScript truthiness of a JSON answer.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) => true,
        Value::Object(_) => !is_undefined(value),
    }
}

fn derivation(spec: &Value, h: &Harness) -> Derivation {
    let f = scripted(spec, h);
    Rc::new(move |c| match f(c) {
        Answer::Returns(value) => Ok(truthy(&value)),
        Answer::Throws(error) => Err(error),
        Answer::Promise(_) => Ok(true),
    })
}

fn accent(spec: &Value, h: &Harness) -> AccentDerivation {
    let f = scripted(spec, h);
    Rc::new(move |c| match f(c) {
        Answer::Returns(value) if value.is_null() || is_undefined(&value) => Ok(None),
        Answer::Returns(value) => Ok(Some(IconAccent {
            primary: value.get("primary").and_then(Value::as_str).map(str::to_string),
            secondary: value.get("secondary").and_then(Value::as_str).map(str::to_string),
        })),
        Answer::Throws(error) => Err(error),
        Answer::Promise(_) => Ok(None),
    })
}

fn run(spec: &Value, h: &Harness) -> RunFn {
    let f = scripted(spec, h);
    Rc::new(move |c| match f(c) {
        Answer::Throws(error) => Err(error),
        // The return value, a promise included, is never read.
        Answer::Returns(_) | Answer::Promise(_) => Ok(()),
    })
}

// ── decoding definitions ──────────────────────────────────────────────────────

/// A `CommandDef` from a vector. Function members need a harness; a
/// function case has none and never writes them.
pub fn decode_def(value: &Value, h: Option<&Harness>) -> Result<CommandDef, Error> {
    let object = value.as_object().ok_or_else(|| bad("command", value))?;
    let func = |key: &str| {
        h.and_then(|h| object.get(key).filter(|v| v.get("$fn").is_some()).map(|spec| (spec, h)))
    };
    Ok(CommandDef {
        id: opt_string(object, "id").ok_or_else(|| bad("command id", value))?,
        label_key: opt_string(object, "labelKey").ok_or_else(|| bad("command labelKey", value))?,
        icon: opt_string(object, "icon"),
        icon_accent: func("iconAccent").map(|(spec, h)| accent(spec, h)),
        shortcut: match object.get("shortcut") {
            Some(Value::String(s)) => Some(Shortcut::One(s.clone())),
            Some(list @ Value::Array(_)) => Some(Shortcut::Many(strings(Some(list)))),
            _ => None,
        },
        categories: object.get("categories").map(|c| strings(Some(c))),
        menu: opt_string(object, "menu"),
        panel: match object.get("panel") {
            Some(Value::String(id)) => Some(Panel::Id(id.clone())),
            Some(Value::Object(target)) => Some(Panel::Target {
                id: opt_string(target, "id").unwrap_or_default(),
                exclusive: opt_string(target, "exclusive"),
            }),
            _ => None,
        },
        modal: opt_string(object, "modal"),
        enabled: func("enabled").map(|(spec, h)| derivation(spec, h)),
        active: func("active").map(|(spec, h)| derivation(spec, h)),
        visible: func("visible").map(|(spec, h)| derivation(spec, h)),
        run: func("run").map(|(spec, h)| run(spec, h)),
    })
}

fn decode_config(raw: &Value, h: Option<&Harness>) -> Result<CommandsConfig, Error> {
    if is_undefined(raw) {
        return Ok(CommandsConfig::default());
    }
    let commands = match raw.get("commands").and_then(Value::as_array) {
        Some(list) => list.iter().map(|d| decode_def(d, h)).collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    Ok(CommandsConfig { commands, disabled_categories: strings(raw.get("disabledCategories")) })
}

fn resolved(result: Result<Option<ResolvedCommand>, Error>) -> Outcome {
    match result {
        Ok(Some(r)) => Outcome::Returned(serde_json::to_value(r).unwrap_or(Value::Null)),
        Ok(None) => Outcome::Returned(Value::Null),
        Err(error) => Outcome::Threw(error),
    }
}

fn unit(result: Result<(), Error>) -> Outcome {
    match result {
        Ok(()) => Outcome::undefined(),
        Err(error) => Outcome::Threw(error),
    }
}

impl HarnessPlugin for Commands {
    fn factories(&self) -> Vec<&'static str> {
        vec!["commands"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        commands_plugin(decode_config(config, Some(h))?)
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![COMMANDS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<CommandsCapability>()?;
        let id = str_arg(&args, 0).unwrap_or_default();
        let doc = str_arg(&args, 1);
        Some(match method {
            "register" => match decode_def(arg(&args, 0).unwrap_or(&Value::Null), Some(h)) {
                Ok(def) => unit(cap.register(def)),
                Err(error) => Outcome::Threw(error),
            },
            "unregister" => {
                cap.unregister(id);
                Outcome::undefined()
            }
            "has" => Outcome::Returned(json!(cap.has(id))),
            "ids" => Outcome::Returned(json!(cap.ids())),
            "resolve" => resolved(cap.resolve(id, doc)),
            "search" => match cap.search(id, doc) {
                Ok(hits) => Outcome::Returned(serde_json::to_value(hits).unwrap_or(Value::Null)),
                Err(error) => Outcome::Threw(error),
            },
            "menuTarget" => Outcome::Returned(
                cap.menu_target(id).map_or(Value::Null, |t| serde_json::to_value(t).unwrap()),
            ),
            "execute" => unit(cap.execute(id, doc)),
            "matchStroke" => {
                let stroke = arg(&args, 0).map(|s| serde_json::from_value(scrub(s)));
                let opts = arg(&args, 1).map(|o| serde_json::from_value(scrub(o)));
                match (stroke, opts) {
                    (Some(Ok(stroke)), Some(Ok(opts))) => Outcome::Returned(
                        cap.match_stroke(&stroke, opts).map_or(Value::Null, Value::String),
                    ),
                    _ => Outcome::Threw(bad("matchStroke", "a stroke and options")),
                }
            }
            "disabledCategories" => match cap.disabled_categories() {
                Ok(list) => Outcome::Returned(json!(list)),
                Err(error) => Outcome::Threw(error),
            },
            "isCategoryDisabled" => match cap.is_category_disabled(id) {
                Ok(disabled) => Outcome::Returned(json!(disabled)),
                Err(error) => Outcome::Threw(error),
            },
            "disableCategory" => {
                cap.disable_category(id);
                Outcome::undefined()
            }
            "enableCategory" => {
                cap.enable_category(id);
                Outcome::undefined()
            }
            "setDisabledCategories" => {
                cap.set_disabled_categories(&strings(arg(&args, 0)));
                Outcome::undefined()
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<CommandsCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── function cases ────────────────────────────────────────────────────────────

/// The reducer with its reference identity made visible. An action no
/// variant decodes is the TypeScript's `default: return state`.
fn f_reducer(args: &[Value]) -> Result<Value, Error> {
    let state: CommandsState = serde_json::from_value(scrub(arg(args, 0).unwrap_or(&Value::Null)))
        .map_err(|e| bad("state", e))?;
    let action: Option<CommandsAction> =
        arg(args, 1).and_then(|a| serde_json::from_value(scrub(a)).ok());
    let next = action.and_then(|action| commands_reducer(&state, &action));
    let shown =
        serde_json::to_value(next.as_ref().unwrap_or(&state)).map_err(|e| bad("state", e))?;
    Ok(json!({ "same": next.is_none(), "next": shown }))
}

fn f_initial(_: &[Value]) -> Result<Value, Error> {
    serde_json::to_value(initial_commands_state()).map_err(|e| bad("state", e))
}

fn resolved_arg(args: &[Value], i: usize) -> Result<Option<ResolvedCommand>, Error> {
    match arg(args, i) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            serde_json::from_value(scrub(value)).map(Some).map_err(|e| bad("resolved", e))
        }
    }
}

fn f_equal(args: &[Value]) -> Result<Value, Error> {
    let (a, b) = (resolved_arg(args, 0)?, resolved_arg(args, 1)?);
    Ok(json!(resolved_commands_equal(a.as_ref(), b.as_ref())))
}

fn f_equal_self(args: &[Value]) -> Result<Value, Error> {
    let a = resolved_arg(args, 0)?;
    Ok(json!(resolved_commands_equal(a.as_ref(), a.as_ref())))
}

/// Register each definition into a fresh registry, in order: the entries,
/// and the error that stopped it, if one did.
fn f_register(args: &[Value]) -> Result<Value, Error> {
    let mut registry = CommandRegistry::new();
    let mut error = None;
    for def in arg(args, 0).and_then(Value::as_array).into_iter().flatten() {
        if let Err(e) = register_command(&mut registry, decode_def(def, None)?) {
            error = Some(e);
            break;
        }
    }
    let entries: Vec<Value> = registry
        .entries()
        .into_iter()
        .map(
            |(id, entry)| json!({ "id": id, "shortcuts": entry.shortcuts, "parsed": entry.parsed }),
        )
        .collect();
    Ok(match error {
        None => json!({ "entries": entries }),
        Some(error) => json!({ "entries": entries, "error": error.to_canonical() }),
    })
}

/// What `commandsPlugin(config)` builds: its identity and initial state.
fn f_plugin(args: &[Value]) -> Result<Value, Error> {
    let config = decode_config(arg(args, 0).unwrap_or(&undefined()), None)?;
    let plugin = commands_plugin(config)?;
    let state = (plugin.initial_state)();
    Ok(json!({
        "id": plugin.id,
        "scope": match plugin.scope {
            epdf_kernel::types::PluginScope::Workspace => "workspace",
            epdf_kernel::types::PluginScope::Document => "document",
        },
        "initialState": (plugin.to_json)(&*state),
    }))
}

pub fn functions() -> FunctionTable {
    vec![
        ("commandsReducer", f_reducer),
        ("initialCommandsState", f_initial),
        ("resolvedCommandsEqual", f_equal),
        ("resolvedCommandsEqualSelf", f_equal_self),
        ("registerCommand", f_register),
        ("commandsPlugin", f_plugin),
    ]
}
