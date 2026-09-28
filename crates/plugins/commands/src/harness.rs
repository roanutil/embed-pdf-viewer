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
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined, undefined};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Map, Value, json};

use crate::contract::{
    AccentDerivation, CommandCtx, CommandDef, CommandsConfig, Derivation, IconAccent, Panel,
    RegisterCommandOptions, ResolvedCommand, RunFn, Shortcut,
};
use crate::model::{CommandsAction, CommandsState};
use crate::registry::CommandRegistry;
use crate::{
    COMMANDS, CommandsCapability, commands_plugin, commands_reducer, initial_commands_state,
    register_command, resolved_commands_equal,
};

pub struct Commands;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "registerCommand",
    "registerCommands",
    "hasCommand",
    "getCommand",
    "listCommandIds",
    "resolveCommand",
    "listCommands",
    "searchCommands",
    "listShortcuts",
    "execute",
    "canExecute",
    "getDisabledCategories",
    "isCategoryDisabled",
    "disableCategory",
    "enableCategory",
    "setDisabledCategories",
    "onExecuted",
    "onExecutionFailed",
    "matchStroke",
    "getMenuTarget",
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

/// The canonical form of a `CommandCtx` in a callback's arguments. On a
/// live harness its `core`, `get` and `tryGet` are markers the host resolves
/// against the kernel, `{"$kernel": member, "doc": documentId}`; a trace
/// prints each as `{"$function": true}`.
fn ctx_json(c: &CommandCtx, live: bool) -> Value {
    let mut out = Map::new();
    out.insert("documentId".into(), json!(c.document_id()));
    if let Some(args) = c.args() {
        out.insert("args".into(), args.clone());
    }
    for member in ["core", "get", "tryGet"] {
        let shown = if live {
            json!({ "$kernel": member, "doc": c.document_id() })
        } else {
            json!({ "$function": true })
        };
        out.insert(member.into(), shown);
    }
    Value::Object(out)
}

fn outcome_value(outcome: Outcome) -> Value {
    match outcome {
        Outcome::Returned(value) => json!({ "returned": value }),
        Outcome::Threw(error) => json!({ "threw": error.to_canonical() }),
        // A promise has no canonical form in callback arguments.
        Outcome::Promise(_) => json!({ "returnedPromise": true }),
    }
}

thread_local! {
    /// Removers kept by `remember` uses, per case (`build` resets it), as
    /// the generator's factory keeps them.
    static REMEMBERED: RefCell<HashMap<String, Rc<dyn Fn()>>> = RefCell::new(HashMap::new());
}

/// One `uses` entry, done with the context a command function was handed.
fn run_use(spec: &Value, c: &CommandCtx, h: &Harness) -> Value {
    if let Some(name) = spec.get("forget").and_then(Value::as_str) {
        let remove = REMEMBERED.with(|r| r.borrow().get(name).cloned());
        return match remove {
            Some(remove) => {
                remove();
                json!({ "returned": undefined() })
            }
            None => {
                json!({ "threw": Error::new(format!("nothing remembered as {name}")).to_canonical() })
            }
        };
    }
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
    // A remover a use gets back is a function in callback arguments, which
    // the trace prints as `{"$function": true}` and never numbers as a ref.
    if method == "registerCommand"
        && let Some(cap) = capability.downcast_ref::<CommandsCapability>()
    {
        let registered = decode_def(arg(&args, 0).unwrap_or(&Value::Null), Some(h))
            .and_then(|def| cap.register_command(def, register_options(arg(&args, 1))));
        return match registered {
            Ok(remove) => {
                if let Some(name) = spec.get("remember").and_then(Value::as_str) {
                    REMEMBERED.with(|r| r.borrow_mut().insert(name.to_string(), remove));
                }
                json!({ "returned": { "$function": true } })
            }
            Err(error) => json!({ "threw": error.to_canonical() }),
        };
    }
    outcome_value(h.call(&capability, method, args))
}

/// A `$fn` spec as a command function: logs the call, does its `uses`,
/// logs those, then answers.
fn scripted(spec: &Value, h: &Harness) -> Rc<dyn Fn(&CommandCtx) -> Answer> {
    let callback = h.callback(spec);
    let uses = spec.get("uses").and_then(Value::as_array).cloned();
    let h = h.clone();
    Rc::new(move |c: &CommandCtx| {
        let answer = callback.call(vec![ctx_json(c, h.is_live())]);
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
        Answer::Returns(_) => Ok(None),
        Answer::Promise(promise) => Ok(Some(promise)),
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

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A definition as `getCommand` hands it back: its data members. Function
/// members are not printed; no vector reads a definition that has them.
fn def_json(def: &CommandDef) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(def.id));
    out.insert("labelKey".into(), json!(def.label_key));
    if let Some(icon) = &def.icon {
        out.insert("icon".into(), json!(icon));
    }
    match &def.shortcut {
        Some(Shortcut::One(s)) => {
            out.insert("shortcut".into(), json!(s));
        }
        Some(Shortcut::Many(list)) => {
            out.insert("shortcut".into(), json!(list));
        }
        None => {}
    }
    if let Some(categories) = &def.categories {
        out.insert("categories".into(), json!(categories));
    }
    if let Some(menu) = &def.menu {
        out.insert("menu".into(), json!(menu));
    }
    match &def.panel {
        Some(Panel::Id(id)) => {
            out.insert("panel".into(), json!(id));
        }
        Some(Panel::Target { id, exclusive }) => {
            let mut target = Map::new();
            target.insert("id".into(), json!(id));
            if let Some(exclusive) = exclusive {
                target.insert("exclusive".into(), json!(exclusive));
            }
            out.insert("panel".into(), Value::Object(target));
        }
        None => {}
    }
    if let Some(modal) = &def.modal {
        out.insert("modal".into(), json!(modal));
    }
    Value::Object(out)
}

fn register_options(value: Option<&Value>) -> RegisterCommandOptions {
    RegisterCommandOptions {
        replace: value.and_then(|o| o.get("replace")).and_then(Value::as_bool).unwrap_or(false),
    }
}

/// A remover as a ref the vector can call.
fn remover(result: Result<Rc<dyn Fn()>, Error>, h: &Harness) -> Outcome {
    match result {
        Ok(remove) => Outcome::Returned(h.register_fn(Rc::new(move |_| {
            remove();
            Outcome::undefined()
        }))),
        Err(error) => Outcome::Threw(error),
    }
}

/// Subscribe a scripted `$fn` listener; the outcome is the unsubscribe.
fn listen<E: serde::Serialize>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![json_of(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

impl HarnessPlugin for Commands {
    fn factories(&self) -> Vec<&'static str> {
        vec!["commands"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        REMEMBERED.with(|r| r.borrow_mut().clear());
        Ok(commands_plugin(decode_config(config, Some(h))?))
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
            "registerCommand" => match decode_def(arg(&args, 0).unwrap_or(&Value::Null), Some(h)) {
                Ok(def) => remover(cap.register_command(def, register_options(arg(&args, 1))), h),
                Err(error) => Outcome::Threw(error),
            },
            "registerCommands" => {
                let defs: Result<Vec<CommandDef>, Error> = arg(&args, 0)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|d| decode_def(d, Some(h)))
                    .collect();
                match defs {
                    Ok(defs) => {
                        remover(cap.register_commands(defs, register_options(arg(&args, 1))), h)
                    }
                    Err(error) => Outcome::Threw(error),
                }
            }
            "hasCommand" => Outcome::Returned(json!(cap.has_command(id))),
            "getCommand" => {
                Outcome::Returned(cap.get_command(id).map_or(Value::Null, |d| def_json(&d)))
            }
            "listCommandIds" => Outcome::Returned(json!(cap.list_command_ids())),
            "resolveCommand" => Outcome::Returned(json_of(&cap.resolve_command(id, doc))),
            "listCommands" => Outcome::Returned(json_of(&cap.list_commands(str_arg(&args, 0)))),
            "searchCommands" => Outcome::Returned(json_of(&cap.search_commands(id, doc))),
            "listShortcuts" => Outcome::Returned(json_of(&cap.list_shortcuts())),
            "execute" => {
                let options = arg(&args, 1);
                let document_id = options.and_then(|o| o.get("documentId")).and_then(Value::as_str);
                let call_args =
                    options.and_then(|o| o.get("args")).filter(|a| !is_undefined(a)).cloned();
                Outcome::promise(cap.execute(id, document_id, call_args))
            }
            "canExecute" => Outcome::Returned(json!(cap.can_execute(id, doc))),
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
            "getMenuTarget" => Outcome::Returned(json_of(&cap.get_menu_target(id))),
            "getDisabledCategories" => Outcome::Returned(json!(cap.get_disabled_categories())),
            "isCategoryDisabled" => Outcome::Returned(json!(cap.is_category_disabled(id))),
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
            "onExecuted" => listen(h, arg(&args, 0), |f| cap.on_executed(f)),
            "onExecutionFailed" => listen(h, arg(&args, 0), |f| cap.on_execution_failed(f)),
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
    serde_json::to_value(initial_commands_state(&[])).map_err(|e| bad("state", e))
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
        let def = decode_def(def, None)?;
        if let Err(e) = register_command(&mut registry, def, RegisterCommandOptions::default()) {
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
/// The definitions are registered when a controller is created, not here.
fn f_plugin(args: &[Value]) -> Result<Value, Error> {
    let config = decode_config(arg(args, 0).unwrap_or(&undefined()), None)?;
    let plugin = commands_plugin(config);
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
