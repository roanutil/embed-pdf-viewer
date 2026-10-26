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
//!
//! A definition prints as the vector wrote it, functions as
//! `{"$function": true}`: the TypeScript hands back the object the app
//! passed, members in its order. The adapter keeps each definition it
//! decoded with that print, by `Rc`, for the case.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::types::PluginScope;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Kernel, KernelOptions, is_undefined, undefined};
use epdf_ui_core::shortcuts::KeyStroke;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Map, Value, json};

use crate::contract::{
    AccentDerivation, CommandCtx, CommandDef, CommandItem, CommandsConfig, CommandsSettings,
    Derivation, ExecuteOptions, IconAccent, MatchStrokeOptions, NotAllowedMode, Panel,
    RegisterCommandOptions, ResolvedCommand, RunFn, Shortcut,
};
use crate::registry::registered_command;
use crate::{COMMANDS, CommandsCapability, plugin_parts, resolved_commands_equal};

pub struct Commands;

/// The TypeScript object literal's member order (`controller.ts`, the
/// settings calls spread first).
const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
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
    "getRefusal",
    "getDisabledCategories",
    "isCategoryDisabled",
    "disableCategory",
    "enableCategory",
    "setDisabledCategories",
    "getDisabledCommands",
    "isCommandDisabled",
    "disableCommand",
    "enableCommand",
    "setDisabledCommands",
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

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

thread_local! {
    /// Removers kept by `remember` uses, per case (`build` resets it), as
    /// the generator's factory keeps them.
    static REMEMBERED: RefCell<HashMap<String, Rc<dyn Fn()>>> = RefCell::new(HashMap::new());
    /// Each definition decoded in this case, with how it prints. Holding
    /// the `Rc` keeps its address from being reused while it is a key.
    static PRINTED: RefCell<Vec<(Rc<CommandDef>, Value)>> = const { RefCell::new(Vec::new()) };
}

fn reset_case() {
    REMEMBERED.with(|r| r.borrow_mut().clear());
    PRINTED.with(|p| p.borrow_mut().clear());
}

// ── printing ──────────────────────────────────────────────────────────────────

/// A vector's definition as the TypeScript prints it: `$fn` specs become
/// `{"$function": true}`.
fn printed_from_vector(value: &Value) -> Value {
    match value {
        Value::Object(object) if object.contains_key("$fn") => json!({ "$function": true }),
        Value::Object(object) => {
            Value::Object(object.iter().map(|(k, v)| (k.clone(), printed_from_vector(v))).collect())
        }
        Value::Array(list) => Value::Array(list.iter().map(printed_from_vector).collect()),
        other => other.clone(),
    }
}

/// A definition this adapter did not decode, in the interface's member
/// order.
fn printed_from_def(def: &CommandDef) -> Value {
    let mut out = Map::new();
    let function = || json!({ "$function": true });
    out.insert("id".into(), json!(def.id));
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            out.insert(key.into(), value);
        }
    };
    put("label", def.label.as_ref().map(|v| json!(v)));
    put("labelKey", def.label_key.as_ref().map(|v| json!(v)));
    put("icon", def.icon.as_ref().map(|v| json!(v)));
    put("iconAccent", def.icon_accent.as_ref().map(|_| function()));
    put(
        "shortcut",
        def.shortcut.as_ref().map(|s| match s {
            Shortcut::One(s) => json!(s),
            Shortcut::Many(list) => json!(list),
        }),
    );
    put("shortcutInTextFields", def.shortcut_in_text_fields.map(|v| json!(v)));
    put("categories", def.categories.as_ref().map(|v| json!(v)));
    put("requires", def.requires.as_ref().map(|v| json!(v)));
    put("allowed", def.allowed.as_ref().map(|_| function()));
    put("notAllowedKey", def.not_allowed_key.as_ref().map(|v| json!(v)));
    put("menu", def.menu.as_ref().map(|v| json!(v)));
    put(
        "panel",
        def.panel.as_ref().map(|p| match p {
            Panel::Id(id) => json!(id),
            Panel::Target { id, exclusive, focus } => {
                let mut target = Map::new();
                target.insert("id".into(), json!(id));
                if let Some(exclusive) = exclusive {
                    target.insert("exclusive".into(), json!(exclusive));
                }
                if let Some(focus) = focus {
                    target.insert("focus".into(), json!(focus));
                }
                Value::Object(target)
            }
        }),
    );
    put("modal", def.modal.as_ref().map(|v| json!(v)));
    put("enabled", def.enabled.as_ref().map(|_| function()));
    put("active", def.active.as_ref().map(|_| function()));
    put("visible", def.visible.as_ref().map(|_| function()));
    put("run", def.run.as_ref().map(|_| function()));
    Value::Object(out)
}

fn print_def(def: &Rc<CommandDef>) -> Value {
    PRINTED
        .with(|p| p.borrow().iter().find(|(d, _)| Rc::ptr_eq(d, def)).map(|(_, v)| v.clone()))
        .unwrap_or_else(|| printed_from_def(def))
}

fn print_item(item: &CommandItem) -> Value {
    match item {
        CommandItem::Command(def) => print_def(def),
        CommandItem::Family(family) => json!({
            "prefix": family.prefix,
            "names": { "$function": true },
            "command": { "$function": true },
        }),
    }
}

fn not_allowed_json(mode: NotAllowedMode) -> Value {
    json_of(&mode)
}

fn print_settings(settings: &CommandsSettings) -> Value {
    json!({
        "commands": settings.commands.iter().map(print_item).collect::<Vec<_>>(),
        "disabledCategories": *settings.disabled_categories,
        "disabledCommands": *settings.disabled_commands,
        "notAllowed": not_allowed_json(settings.not_allowed),
    })
}

// ── scripted command functions ────────────────────────────────────────────────

/// The canonical form of a `CommandCtx` in a callback's arguments. On a
/// live harness its `get` and `tryGet` are markers the host resolves
/// against the kernel, `{"$kernel": member, "doc": documentId}`; a trace
/// prints each as `{"$function": true}`.
fn ctx_json(c: &CommandCtx, live: bool) -> Value {
    let mut out = Map::new();
    out.insert("documentId".into(), json!(c.document_id()));
    if let Some(args) = c.args() {
        out.insert("args".into(), args.clone());
    }
    for member in ["get", "tryGet"] {
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
    if let Some(member) = spec.get("context").and_then(Value::as_str) {
        let returned = match member {
            "documentId" => json!(c.document_id()),
            "args" => c.args().cloned().unwrap_or_else(undefined),
            _ => undefined(),
        };
        return json!({ "returned": returned });
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

/// A `CommandDef` from a vector, kept with its print. Function members need
/// a harness; a function case has none and never writes them.
pub fn decode_def(value: &Value, h: Option<&Harness>) -> Result<Rc<CommandDef>, Error> {
    let object = value.as_object().ok_or_else(|| bad("command", value))?;
    let func = |key: &str| {
        h.and_then(|h| object.get(key).filter(|v| v.get("$fn").is_some()).map(|spec| (spec, h)))
    };
    let def = Rc::new(CommandDef {
        id: opt_string(object, "id").ok_or_else(|| bad("command id", value))?,
        label: opt_string(object, "label"),
        label_key: opt_string(object, "labelKey"),
        icon: opt_string(object, "icon"),
        icon_accent: func("iconAccent").map(|(spec, h)| accent(spec, h)),
        shortcut: match object.get("shortcut") {
            Some(Value::String(s)) => Some(Shortcut::One(s.clone())),
            Some(list @ Value::Array(_)) => Some(Shortcut::Many(strings(Some(list)))),
            _ => None,
        },
        shortcut_in_text_fields: object.get("shortcutInTextFields").and_then(Value::as_bool),
        categories: object.get("categories").map(|c| strings(Some(c))),
        requires: object.get("requires").map(|c| strings(Some(c))),
        allowed: func("allowed").map(|(spec, h)| derivation(spec, h)),
        not_allowed_key: opt_string(object, "notAllowedKey"),
        menu: opt_string(object, "menu"),
        panel: match object.get("panel") {
            Some(Value::String(id)) => Some(Panel::Id(id.clone())),
            Some(Value::Object(target)) => Some(Panel::Target {
                id: opt_string(target, "id").unwrap_or_default(),
                exclusive: opt_string(target, "exclusive"),
                focus: target.get("focus").and_then(Value::as_bool),
            }),
            _ => None,
        },
        modal: opt_string(object, "modal"),
        enabled: func("enabled").map(|(spec, h)| derivation(spec, h)),
        active: func("active").map(|(spec, h)| derivation(spec, h)),
        visible: func("visible").map(|(spec, h)| derivation(spec, h)),
        run: func("run").map(|(spec, h)| run(spec, h)),
    });
    PRINTED.with(|p| p.borrow_mut().push((def.clone(), printed_from_vector(value))));
    Ok(def)
}

fn not_allowed_of(value: &Value) -> Result<NotAllowedMode, Error> {
    serde_json::from_value(value.clone()).map_err(|e| bad("notAllowed", e))
}

/// A `DeepPartial<CommandsSettings>` from a vector; `undefined` is no
/// config at all.
fn decode_config(raw: &Value, h: Option<&Harness>) -> Result<Option<CommandsConfig>, Error> {
    if is_undefined(raw) {
        return Ok(None);
    }
    let member = |key: &str| raw.get(key).filter(|v| !is_undefined(v));
    let commands = match member("commands").and_then(Value::as_array) {
        Some(list) => Some(
            list.iter()
                .map(|d| decode_def(d, h).map(CommandItem::Command))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        None => None,
    };
    Ok(Some(CommandsConfig {
        commands,
        disabled_categories: member("disabledCategories").map(|v| strings(Some(v))),
        disabled_commands: member("disabledCommands").map(|v| strings(Some(v))),
        not_allowed: member("notAllowed").map(not_allowed_of).transpose()?,
    }))
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
fn listen<E>(
    h: &Harness,
    spec: Option<&Value>,
    print: impl Fn(&E) -> Value + 'static,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![print(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn returned<T: serde::Serialize>(result: Result<T, Error>) -> Outcome {
    match result {
        Ok(value) => Outcome::Returned(json_of(&value)),
        Err(error) => Outcome::Threw(error),
    }
}

fn done() -> Outcome {
    Outcome::undefined()
}

impl HarnessPlugin for Commands {
    /// The suite's own names for the shell and i18n plugins it runs next
    /// to this one (`commands-current-shell`, `commands-current-i18n`):
    /// those plugins' adapters build them.
    fn factories(&self) -> Vec<&'static str> {
        vec!["commands", "commands-current-shell", "commands-current-i18n"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "commands-current-shell" => epdf_plugin_shell::harness::Shell.build("shell", config, h),
            "commands-current-i18n" => epdf_plugin_i18n::harness::I18n.build("i18n", config, h),
            _ => {
                reset_case();
                Ok(plugin_parts(decode_config(config, Some(h))?).0)
            }
        }
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
            "getSettings" => Outcome::Returned(print_settings(&cap.get_settings())),
            "updateSettings" => match decode_config(arg(&args, 0).unwrap_or(&json!({})), Some(h)) {
                Ok(changes) => {
                    cap.update_settings(&changes.unwrap_or_default());
                    done()
                }
                Err(error) => Outcome::Threw(error),
            },
            "resetSettings" => {
                cap.reset_settings();
                done()
            }
            "onSettingsChanged" => listen(
                h,
                arg(&args, 0),
                |event: &crate::CommandsSettingsChanged| json!({ "settings": print_settings(&event.settings), "changed": event.changed }),
                |f| cap.on_settings_changed(f),
            ),
            "registerCommand" => match decode_def(arg(&args, 0).unwrap_or(&Value::Null), Some(h)) {
                Ok(def) => remover(cap.register_command(def, register_options(arg(&args, 1))), h),
                Err(error) => Outcome::Threw(error),
            },
            "registerCommands" => {
                let defs: Result<Vec<Rc<CommandDef>>, Error> = arg(&args, 0)
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
            "hasCommand" => returned(cap.has_command(id)),
            "getCommand" => match cap.get_command(id) {
                Ok(def) => Outcome::Returned(def.map_or(Value::Null, |d| print_def(&d))),
                Err(error) => Outcome::Threw(error),
            },
            "listCommandIds" => returned(cap.list_command_ids()),
            "resolveCommand" => returned(cap.resolve_command(id, doc)),
            "listCommands" => returned(cap.list_commands(str_arg(&args, 0))),
            "searchCommands" => returned(cap.search_commands(id, doc)),
            "listShortcuts" => returned(cap.list_shortcuts()),
            "execute" => {
                let options = arg(&args, 1);
                let member =
                    |key: &str| options.and_then(|o| o.get(key)).filter(|v| !is_undefined(v));
                Outcome::promise(cap.execute(
                    id,
                    ExecuteOptions {
                        document_id:
                            member("documentId").and_then(Value::as_str).map(str::to_string),
                        args: member("args").cloned(),
                        signal: None,
                    },
                ))
            }
            "canExecute" => returned(cap.can_execute(id, doc)),
            "getRefusal" => returned(cap.get_refusal(id, doc)),
            "getDisabledCategories" => Outcome::Returned(json!(*cap.get_disabled_categories())),
            "isCategoryDisabled" => Outcome::Returned(json!(cap.is_category_disabled(id))),
            "disableCategory" => {
                cap.disable_category(id);
                done()
            }
            "enableCategory" => {
                cap.enable_category(id);
                done()
            }
            "setDisabledCategories" => {
                cap.set_disabled_categories(&strings(arg(&args, 0)));
                done()
            }
            "getDisabledCommands" => Outcome::Returned(json!(*cap.get_disabled_commands())),
            "isCommandDisabled" => returned(cap.is_command_disabled(id)),
            "disableCommand" => {
                cap.disable_command(id);
                done()
            }
            "enableCommand" => {
                cap.enable_command(id);
                done()
            }
            "setDisabledCommands" => {
                cap.set_disabled_commands(&strings(arg(&args, 0)));
                done()
            }
            "onExecuted" => listen(h, arg(&args, 0), json_of, |f| cap.on_executed(f)),
            "onExecutionFailed" => {
                listen(h, arg(&args, 0), json_of, |f| cap.on_execution_failed(f))
            }
            "matchStroke" => {
                let stroke = arg(&args, 0).map(|s| serde_json::from_value::<KeyStroke>(scrub(s)));
                let opts =
                    arg(&args, 1).map(|o| serde_json::from_value::<MatchStrokeOptions>(scrub(o)));
                match (stroke, opts) {
                    (Some(Ok(stroke)), Some(Ok(opts))) => returned(cap.match_stroke(&stroke, opts)),
                    _ => Outcome::Threw(bad("matchStroke", "a stroke and options")),
                }
            }
            "getMenuTarget" => returned(cap.get_menu_target(id)),
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

/// A kernel with no documents and the plugin registered with `config`: the
/// workspace controller exists from construction, as `createTestContext`
/// builds it.
fn controller(config: Option<CommandsConfig>) -> Result<(Kernel, CommandsCapability), Error> {
    let engine = Rc::new(AsEngine(FakeEngine::new(json!({ "documents": {} }))));
    let kernel = Kernel::new(engine, vec![plugin_parts(config).0], KernelOptions::default())?;
    let capability = kernel.capability(&COMMANDS, None)?;
    Ok((kernel, capability))
}

/// A settings mutation, with `same` the identity of the disabled-category
/// list before and after.
fn f_settings(args: &[Value]) -> Result<Value, Error> {
    let config = decode_config(arg(args, 0).unwrap_or(&json!({})), None)?.unwrap_or_default();
    let (_kernel, api) = controller(Some(config))?;
    let before = api.get_disabled_categories();
    match str_arg(args, 1) {
        Some("disableCategory") => api.disable_category(str_arg(args, 2).unwrap_or_default()),
        Some("enableCategory") => api.enable_category(str_arg(args, 2).unwrap_or_default()),
        Some("setDisabledCategories") => api.set_disabled_categories(&strings(arg(args, 2))),
        _ => {}
    }
    Ok(json!({
        "same": Rc::ptr_eq(&before, &api.get_disabled_categories()),
        "settings": print_settings(&api.get_settings()),
    }))
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

/// Register each definition through a fresh controller, in order: the
/// entries, and the error that stopped it, if one did.
fn f_register(args: &[Value]) -> Result<Value, Error> {
    let (_kernel, api) = controller(None)?;
    let mut error = None;
    for def in arg(args, 0).and_then(Value::as_array).into_iter().flatten() {
        if let Err(e) = api.register_command(decode_def(def, None)?, Default::default()) {
            error = Some(e);
            break;
        }
    }
    let mut entries = Vec::new();
    for id in api.list_command_ids()?.iter() {
        let def = api.get_command(id)?.ok_or_else(|| bad("entry", id))?;
        let entry = registered_command(def)?;
        entries.push(json!({ "id": id, "shortcuts": entry.shortcuts, "parsed": entry.parsed }));
    }
    Ok(match error {
        None => json!({ "entries": entries }),
        Some(error) => json!({ "entries": entries, "error": error.to_canonical() }),
    })
}

/// What `commandsPlugin(config)` builds: its identity, scope and settings
/// declaration, and that it has no state.
fn f_plugin(args: &[Value]) -> Result<Value, Error> {
    let config = decode_config(arg(args, 0).unwrap_or(&undefined()), None)?;
    let (plugin, table) = plugin_parts(config);
    let declaration = plugin.settings.clone().ok_or_else(|| bad("settings", "none"))?;
    let mut settings = Map::new();
    settings.insert("defaults".into(), declaration.defaults.clone());
    if let Some(registered) = &declaration.registered {
        let mut shown = registered.as_object().cloned().unwrap_or_default();
        if let Some(commands) = shown.get_mut("commands") {
            let items = table.items_of(commands).ok_or_else(|| bad("commands", &*commands))?;
            *commands = Value::Array(items.iter().map(print_item).collect());
        }
        settings.insert("registered".into(), Value::Object(shown));
    }
    let state = (plugin.initial_state)();
    Ok(json!({
        "id": plugin.id,
        "scope": match plugin.scope {
            PluginScope::Workspace => "workspace",
            PluginScope::Document => "document",
        },
        "settings": settings,
        "hasState": !is_undefined(&(plugin.to_json)(&*state)),
        // The kernel has no `resolvesAnyCapability` yet: its guard applies.
        "resolvesAnyCapability": false,
    }))
}

pub fn functions() -> FunctionTable {
    vec![
        ("commandSettings", f_settings),
        ("resolvedCommandsEqual", f_equal),
        ("resolvedCommandsEqualSelf", f_equal_self),
        ("registerCurrentCommands", f_register),
        ("commandsPlugin", f_plugin),
    ]
}
