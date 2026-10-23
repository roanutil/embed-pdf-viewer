//! The conformance adapter: the scripted sandbox both this crate's and the
//! actions plugin's vectors drive, the script-host driver plugin, and the
//! function table. `tooling/plugin-conformance/src/suites/acrojs.ts` is the
//! TypeScript twin of every piece here.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Token, define, undefined};
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::host::{
    IdentityOverrides, ScriptHost, ScriptHostOptions, ScriptSandbox, ScriptSandboxFactory,
    ScriptWorldInput, resolve_script_identity_from, seed_from,
};
use crate::types::*;
use crate::{color, input, js};

// ── scripted callbacks ────────────────────────────────────────────────────────

/// `scriptedCallback(name, spec, env)`: a `$fn`-shaped answer script under a
/// fixed name.
pub fn scripted_callback(name: &str, spec: &Value, h: &Harness) -> Callback {
    let mut spec = spec.as_object().cloned().unwrap_or_default();
    spec.insert("$fn".into(), Value::String(name.to_string()));
    h.callback(&Value::Object(spec))
}

/// A callback's answer as the promise a caller awaits: a value is a resolved
/// promise (awaiting either costs one turn), a throw is `Err`.
pub fn answer_promise(answer: Answer) -> Result<Promise<Value>, Error> {
    match answer {
        Answer::Returns(value) => Ok(Promise::resolved(value)),
        Answer::Throws(error) => Err(error),
        Answer::Promise(promise) => Ok(promise),
    }
}

/// Read a JSON promise as a typed one without spending a turn: the typed
/// promise settles in the same moment, as a JavaScript value needs no
/// conversion at all.
pub fn typed<T: DeserializeOwned + Clone + 'static>(promise: Promise<Value>) -> Promise<T> {
    let (typed, resolver) = Promise::pending();
    promise.on_settle(move |result| {
        resolver.settle(result.clone().and_then(epdf_kernel::engine::decode));
    });
    typed
}

/// The other direction, also without a turn: a typed promise as the JSON a
/// trace records.
pub fn untyped<T: Serialize + Clone + 'static>(promise: Promise<T>) -> Promise<Value> {
    let (json, resolver) = Promise::pending();
    promise.on_settle(move |result| {
        resolver
            .settle(result.clone().map(|value| serde_json::to_value(value).unwrap_or(Value::Null)));
    });
    json
}

pub fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

// ── the scripted sandbox ──────────────────────────────────────────────────────

fn pick(list: Option<&Vec<Value>>, index: usize) -> Option<&Value> {
    let list = list.filter(|l| !l.is_empty())?;
    list.get(index.min(list.len() - 1))
}

/// The empty output with the answer's `output` members merged over it.
fn merged_output(partial: Option<&Value>) -> Result<ScriptOutput, Error> {
    let mut base = to_json(&ScriptOutput::empty());
    if let (Some(base), Some(Value::Object(over))) = (base.as_object_mut(), partial) {
        for (key, value) in over {
            base.insert(key.clone(), value.clone());
        }
    }
    epdf_kernel::engine::decode(base)
}

struct ScriptedSandbox {
    id: usize,
    build: Value,
    h: Harness,
    disposed: Cell<bool>,
    boots: Cell<usize>,
    runs: Cell<usize>,
}

impl ScriptedSandbox {
    fn respond(&self, list: &str, index: usize) -> Result<ScriptOutput, Error> {
        let empty = json!({});
        let answer = pick(self.build.get(list).and_then(Value::as_array), index).unwrap_or(&empty);
        if answer.get("dispose").and_then(Value::as_bool) == Some(true) {
            self.disposed.set(true);
        }
        if let Some(error) = answer.get("throws") {
            return Err(Error::from_spec(error));
        }
        merged_output(answer.get("output"))
    }
}

fn budget_json(budget: Option<&ScriptBudget>) -> Value {
    budget.map_or_else(undefined, to_json)
}

impl ScriptSandbox for ScriptedSandbox {
    fn disposed(&self) -> bool {
        self.disposed.get()
    }

    fn boot(
        &self,
        sources: &[String],
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.h.log(
            "sandbox.boot",
            vec![json!(self.id), json!(sources), to_json(input), budget_json(budget)],
        );
        self.boots.set(self.boots.get() + 1);
        self.respond("boot", self.boots.get() - 1)
    }

    fn run(
        &self,
        source: &str,
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.h.log(
            "sandbox.run",
            vec![json!(self.id), json!(source), to_json(input), budget_json(budget)],
        );
        self.runs.set(self.runs.get() + 1);
        self.respond("run", self.runs.get() - 1)
    }

    fn dispose(&self) {
        self.h.log("sandbox.dispose", vec![json!(self.id)]);
        self.disposed.set(true);
    }
}

/// `scriptedSandboxFactory(spec, env)`: one build per call (the last
/// repeating); each logs `sandbox.factory`, and each realm logs every `boot`,
/// `run` and `dispose` with its canonical input.
pub fn scripted_sandbox_factory(spec: &Value, h: &Harness) -> ScriptSandboxFactory {
    let builds: Vec<Value> = spec
        .get("builds")
        .and_then(Value::as_array)
        .filter(|b| !b.is_empty())
        .cloned()
        .unwrap_or_else(|| vec![json!({})]);
    let built = Rc::new(Cell::new(0usize));
    let h = h.clone();
    Rc::new(move || {
        let id = built.get();
        built.set(id + 1);
        h.log("sandbox.factory", vec![json!(id)]);
        let build = builds[id.min(builds.len() - 1)].clone();
        let sandbox: Rc<dyn ScriptSandbox> = Rc::new(ScriptedSandbox {
            id,
            build: build.clone(),
            h: h.clone(),
            disposed: Cell::new(false),
            boots: Cell::new(0),
            runs: Cell::new(0),
        });
        if let Some(error) = build.get("rejects") {
            return Promise::rejected(Error::from_spec(error));
        }
        if let Some(label) = build.get("defer").and_then(Value::as_str) {
            let (promise, resolver) = Promise::pending();
            h.defer(label).on_settle(move |result| match result {
                Ok(_) => resolver.resolve(sandbox),
                Err(error) => resolver.reject(error.clone()),
            });
            return promise;
        }
        Promise::resolved(sandbox)
    })
}

// ── the script-host driver ────────────────────────────────────────────────────

pub static SCRIPT_HOST: LazyLock<Token<ScriptHostDriver>> =
    LazyLock::new(|| Token::new("script-host"));

#[derive(Clone)]
pub struct ScriptHostDriver {
    host: ScriptHost,
    h: Harness,
}

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !js::is_undefined(v))
}

fn world_of(op: &Value) -> Result<ScriptWorldInput, Error> {
    epdf_kernel::engine::decode(op.get("world").cloned().unwrap_or(Value::Null))
}

fn budget_of(op: &Value) -> Result<Option<ScriptBudget>, Error> {
    match op.get("budget") {
        None => Ok(None),
        Some(budget) => epdf_kernel::engine::decode(budget.clone()).map(Some),
    }
}

impl ScriptHostDriver {
    fn transaction(&self, ops: Vec<Value>) -> Promise<Value> {
        let h = self.h.clone();
        self.host.transaction(move |txn| {
            Ok(Promise::spawn(async move {
                let mut out = Vec::new();
                for op in ops {
                    match op.get("op").and_then(Value::as_str).unwrap_or_default() {
                        "boot" => {
                            let output = txn.boot(world_of(&op)?, budget_of(&op)?).await?;
                            out.push(to_json(&output));
                        }
                        "run" => {
                            let program =
                                op.get("program").and_then(Value::as_str).unwrap_or_default();
                            let output = txn.run(program, world_of(&op)?, budget_of(&op)?).await?;
                            out.push(to_json(&output));
                        }
                        "wait" => {
                            let label = op.get("label").and_then(Value::as_str).unwrap_or_default();
                            h.defer(label).await?;
                            out.push(json!(label));
                        }
                        _ => {
                            let message =
                                op.get("message").and_then(Value::as_str).unwrap_or_default();
                            return Err(Error::new(message));
                        }
                    }
                }
                Ok(Value::Array(out))
            }))
        })
    }
}

pub struct ScriptHostHarness;

impl HarnessPlugin for ScriptHostHarness {
    fn factories(&self) -> Vec<&'static str> {
        vec!["script-host"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let config = if js::is_undefined(config) { json!({}) } else { config.clone() };
        let sandbox_factory =
            scripted_sandbox_factory(config.get("sandbox").unwrap_or(&json!({})), h);
        let boot_sources = scripted_callback(
            "bootSources",
            config.get("bootSources").unwrap_or(&json!({ "returns": ["boot()"] })),
            h,
        );
        let document: ScriptDocumentInput = match config.get("document") {
            Some(document) => epdf_kernel::engine::decode(document.clone())?,
            None => ScriptDocumentInput {
                id: "doc-1".into(),
                file_name: "x.pdf".into(),
                page_count: 1.0,
                page_number: 0.0,
            },
        };
        let identity: ScriptIdentity = match config.get("identity") {
            Some(identity) => epdf_kernel::engine::decode(identity.clone())?,
            None => ScriptIdentity::default(),
        };
        let budget = budget_of(&config)?;
        let h = h.clone();
        // `create: (ctx) => ...`: the probe owns its host, and the kernel's
        // cleanup disposes it (`ctx.cleanup(() => host.dispose())`).
        Ok(define::<Value, Value>("script-host")
            .create(&SCRIPT_HOST, move |cx| {
                let boot_sources = boot_sources.clone();
                let document = document.clone();
                let identity = identity.clone();
                let host = ScriptHost::new(ScriptHostOptions {
                    sandbox_factory: sandbox_factory.clone(),
                    document: Rc::new(move || document.clone()),
                    identity: Rc::new(move || identity.clone()),
                    environment: Rc::new(|sequence| ScriptEnvironment {
                        now_ms: 1000.0,
                        utc_offset_minutes: 0.0,
                        random_seed: seed_from("doc-1", sequence),
                    }),
                    boot_sources: Rc::new(move || {
                        answer_promise(boot_sources.call(Vec::new())).map(typed::<Vec<String>>)
                    }),
                    budget: budget.clone(),
                });
                let owned = host.clone();
                cx.cleanup_fn(move || owned.dispose());
                Ok((ScriptHostDriver { host, h: h.clone() }, None))
            })
            .build())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![SCRIPT_HOST.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        _h: &Harness,
    ) -> Option<Outcome> {
        let driver = capability.downcast_ref::<ScriptHostDriver>()?;
        Some(match method {
            "transaction" => {
                let ops = arg(&args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
                Outcome::Promise(driver.transaction(ops))
            }
            "dispose" => {
                driver.host.dispose();
                Outcome::undefined()
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<ScriptHostDriver>()?;
        Some(vec!["transaction", "dispose"])
    }
}

// ── the function table ────────────────────────────────────────────────────────

fn color_arg(args: &[Value]) -> Result<ScriptColorArray, Error> {
    let items = args.first().and_then(Value::as_array).ok_or_else(|| Error::new("not a color"))?;
    let space = items.first().and_then(Value::as_str).unwrap_or_default();
    let parts: Vec<f64> = items[1..].iter().filter_map(js::number).collect();
    ScriptColorArray::from_parts(space, &parts).ok_or_else(|| Error::new("not a color"))
}

fn script_color_to_rgb(args: &[Value]) -> Result<Value, Error> {
    Ok(color::script_color_to_rgb(&color_arg(args)?).map_or(Value::Null, |rgb| to_json(&rgb)))
}

fn script_color_to_css(args: &[Value]) -> Result<Value, Error> {
    Ok(color::script_color_to_css(&color_arg(args)?).map_or(Value::Null, Value::String))
}

fn css_to_script_color(args: &[Value]) -> Result<Value, Error> {
    Ok(to_json(&color::css_to_script_color(args.first().and_then(Value::as_str))))
}

fn is_script_color_array(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(color::is_script_color_array(args.first().unwrap_or(&Value::Null))))
}

fn java_script_sources_from_action_tree(args: &[Value]) -> Result<Value, Error> {
    crate::actions::java_script_sources_from_action_tree(args.first().unwrap_or(&Value::Null))
        .map(|s| json!(s))
}

fn java_script_program_from_action_tree(args: &[Value]) -> Result<Value, Error> {
    crate::actions::java_script_program_from_action_tree(args.first().unwrap_or(&Value::Null))
        .map(Value::String)
}

fn script_fields_from_snapshot(args: &[Value]) -> Result<Value, Error> {
    input::script_fields_from_snapshot(args.first().unwrap_or(&Value::Null)).map(|f| to_json(&f))
}

fn seed(args: &[Value]) -> Result<Value, Error> {
    let id = args.first().and_then(Value::as_str).unwrap_or_default();
    let sequence = args.get(1).and_then(js::number).unwrap_or(f64::NAN);
    Ok(json!(seed_from(id, sequence)))
}

fn resolve_identity(args: &[Value]) -> Result<Value, Error> {
    let identity = args.first().cloned().unwrap_or(Value::Null);
    let overrides = match args.get(1) {
        None | Some(Value::Null) => None,
        Some(value) => {
            let partial: PartialScriptIdentity = epdf_kernel::engine::decode(value.clone())?;
            Some(if args.get(2).and_then(Value::as_bool) == Some(true) {
                IdentityOverrides::Function(Rc::new(move || partial.clone()))
            } else {
                IdentityOverrides::Value(partial)
            })
        }
    };
    Ok(to_json(&resolve_script_identity_from(&identity, overrides.as_ref())))
}

fn annot_writable_keys(_: &[Value]) -> Result<Value, Error> {
    let mut out = Map::new();
    for (subtype, keys) in ANNOT_WRITABLE_KEYS {
        out.insert(subtype.to_string(), json!(keys));
    }
    Ok(Value::Object(out))
}

fn default_script_budget(_: &[Value]) -> Result<Value, Error> {
    Ok(to_json(&DEFAULT_SCRIPT_BUDGET))
}

fn default_script_security_policy(_: &[Value]) -> Result<Value, Error> {
    Ok(to_json(&DEFAULT_SCRIPT_SECURITY_POLICY))
}

fn script_event_matrix_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(script_event_matrix())
}

fn prelude_source(_: &[Value]) -> Result<Value, Error> {
    Ok(Value::String(crate::PRELUDE_SOURCE.to_string()))
}

/// The functions `vectors/core/acrojs.json` names.
pub fn functions() -> FunctionTable {
    vec![
        ("scriptColorToRgb", script_color_to_rgb),
        ("scriptColorToCss", script_color_to_css),
        ("cssToScriptColor", css_to_script_color),
        ("isScriptColorArray", is_script_color_array),
        ("javaScriptSourcesFromActionTree", java_script_sources_from_action_tree),
        ("javaScriptProgramFromActionTree", java_script_program_from_action_tree),
        ("scriptFieldsFromSnapshot", script_fields_from_snapshot),
        ("seedFrom", seed),
        ("resolveScriptIdentity", resolve_identity),
        ("annotWritableKeys", annot_writable_keys),
        ("defaultScriptBudget", default_script_budget),
        ("defaultScriptSecurityPolicy", default_script_security_policy),
        ("scriptEventMatrix", script_event_matrix_fn),
        ("preludeSource", prelude_source),
    ]
}
