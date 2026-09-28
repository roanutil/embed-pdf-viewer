//! The conformance adapter: how `crates/plugin-conformance` builds these
//! plugins from a vector and calls their capabilities by name, plus the
//! function table for `plugins/interaction-functions.json`.
//!
//! Downstream crates whose vectors drive the hub (selection, annotation,
//! form, stage) register [`Interaction`] next to their own adapter, and may
//! use the JSON bridges here ([`sample_from_json`], [`sample_json`],
//! [`tool_json`], [`ScriptedHandler`]).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use epdf_geometry::Point;
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined, undefined};
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde_json::{Map, Value, json};

use crate::contract::*;
use crate::feedback::{
    FEEDBACK, Feedback, FeedbackPluginOptions, ImpactWeight, NotifyKind, PlatformFeedback,
    feedback_plugin,
};
use crate::model::{InteractionAction, InteractionState};
use crate::{
    InteractionCapability, builtin_tools, initial_interaction_state, interaction_plugin,
    reduce_interaction,
};
use epdf_kernel::geometry::PageRef;

// ── JavaScript values ─────────────────────────────────────────────────────────

/// JavaScript truthiness of a canonical value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) => true,
        Value::Object(o) => {
            if is_undefined(value) {
                false
            } else if let Some(n) = o.get("$number") {
                n != "NaN"
            } else {
                true
            }
        }
    }
}

/// A canonical number, `$number` spellings included.
pub fn number(value: &Value) -> Option<f64> {
    plugin_conformance::js::from_value::<f64>(value).ok()
}

/// What a scripted callback's answer means where the TypeScript tests it
/// for truth. A promise is an object, so it is truthy.
pub fn answer_truthy(answer: Answer) -> bool {
    match answer {
        Answer::Returns(value) => truthy(&value),
        Answer::Throws(_) => false,
        Answer::Promise(_) => true,
    }
}

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn str_arg(args: &[Value], i: usize) -> String {
    arg(args, i).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn opt_str(value: Option<&Value>) -> Option<String> {
    value.filter(|v| !is_undefined(v)).and_then(Value::as_str).map(str::to_string)
}

// ── tools ─────────────────────────────────────────────────────────────────────

/// A tool as the TypeScript prints it: `enables` is a `Set`.
pub fn tool_json(tool: &Tool) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(tool.id));
    out.insert("cursor".into(), json!(tool.cursor));
    if let Some(gap) = &tool.gap_cursor {
        out.insert("gapCursor".into(), json!(gap));
    }
    out.insert("enables".into(), json!({ "$set": tool.enables }));
    if let Some(touch) = tool.touch_direct {
        out.insert("touchDirect".into(), json!(touch));
    }
    Value::Object(out)
}

/// A tool from a vector: `enables` is `{"$set": [...]}` (or a plain list).
pub fn tool_from_json(value: &Value) -> Tool {
    let tags = value.get("enables").map(|e| e.get("$set").unwrap_or(e));
    let tags: Vec<&str> =
        tags.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let mut tool = Tool::new(
        value.get("id").and_then(Value::as_str).unwrap_or_default(),
        value.get("cursor").and_then(Value::as_str).unwrap_or_default(),
        tags,
    );
    tool.gap_cursor = opt_str(value.get("gapCursor"));
    tool.touch_direct = value.get("touchDirect").and_then(Value::as_bool);
    tool
}

/// An `InteractionConfig` from a vector; `undefined` or `null` is `{}`.
pub fn config_from_json(value: &Value) -> InteractionConfig {
    InteractionConfig {
        default_tool: opt_str(value.get("defaultTool")),
        tools: value
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| tools.iter().map(tool_from_json).collect())
            .unwrap_or_default(),
    }
}

// ── samples ───────────────────────────────────────────────────────────────────

fn decode<T: serde::de::DeserializeOwned>(value: &Value, what: &str) -> Result<T, Error> {
    plugin_conformance::js::from_value(value)
        .map_err(|e| Error::new(format!("[harness] {what}: {}", e.message)))
}

/// A sample from a vector. A `$fn` `project` member becomes a scripted
/// callback when a harness is given; `project_from` builds one otherwise.
pub fn sample_from_json(
    value: &Value,
    h: Option<&Harness>,
    project_from: Option<Project>,
) -> Result<PointerSample, Error> {
    let phase: Phase = decode(value.get("phase").unwrap_or(&Value::Null), "phase")?;
    let viewport: Point = decode(value.get("viewport").unwrap_or(&Value::Null), "viewport")?;
    let page = match value.get("page") {
        None | Some(Value::Null) => None,
        Some(page) if is_undefined(page) => None,
        Some(page) => Some(decode::<PageHit>(page, "page")?),
    };
    let modifiers = match value.get("modifiers") {
        Some(m) => decode(m, "modifiers")?,
        None => Modifiers::default(),
    };
    let project = match (value.get("project"), h) {
        (Some(spec), Some(h)) if spec.get("$fn").is_some() => {
            let callback = h.callback(spec);
            let f: Project = Rc::new(move |page| match callback.call(vec![page.to_json()]) {
                Answer::Returns(v) => decode::<Point>(&v, "project").ok(),
                _ => None,
            });
            Some(f)
        }
        _ => project_from,
    };
    Ok(PointerSample {
        phase,
        viewport,
        page,
        modifiers,
        click_count: value.get("clickCount").and_then(number),
        pointer_type: value.get("pointerType").map(|v| decode(v, "pointerType")).transpose()?,
        gesture: value.get("gesture").map(|v| decode(v, "gesture")).transpose()?,
        source: opt_str(value.get("source")),
        project,
    })
}

/// A sample as the TypeScript prints it in a callback's arguments.
pub fn sample_json(sample: &PointerSample) -> Value {
    sample_members(sample, true)
}

/// A sample for a callback, `project` shown only when `with_project`. A live
/// host gets no `project` at all: a placeholder would be non-nullish, and
/// `samplePointOn` does `s.project?.(page) ?? ...`.
fn sample_members(sample: &PointerSample, with_project: bool) -> Value {
    let mut out = Map::new();
    out.insert("phase".into(), serde_json::to_value(sample.phase).unwrap());
    out.insert("viewport".into(), serde_json::to_value(sample.viewport).unwrap());
    out.insert("modifiers".into(), serde_json::to_value(sample.modifiers).unwrap());
    if let Some(page) = &sample.page {
        out.insert("page".into(), serde_json::to_value(page).unwrap());
    }
    if let Some(clicks) = sample.click_count {
        out.insert("clickCount".into(), json!(clicks));
    }
    if let Some(kind) = sample.pointer_type {
        out.insert("pointerType".into(), serde_json::to_value(kind).unwrap());
    }
    if let Some(gesture) = sample.gesture {
        out.insert("gesture".into(), serde_json::to_value(gesture).unwrap());
    }
    if let Some(source) = &sample.source {
        out.insert("source".into(), json!(source));
    }
    if with_project && sample.project.is_some() {
        out.insert("project".into(), json!({ "$function": true }));
    }
    Value::Object(out)
}

// ── scripted handlers ─────────────────────────────────────────────────────────

/// An interaction handler whose members are a vector's `$fn` specs. An
/// absent optional member behaves as the hub treats absence.
pub struct ScriptedHandler {
    id: String,
    priority: f64,
    enabled_for: Option<Callback>,
    on_down: Option<Callback>,
    on_move: Option<Callback>,
    on_up: Option<Callback>,
    on_cancel: Option<Callback>,
    on_hover: Option<Callback>,
    claims_touch: Option<Callback>,
    /// Built on a live harness: samples go out without `project`.
    live: bool,
}

impl ScriptedHandler {
    pub fn from_json(spec: &Value, h: &Harness) -> Self {
        let member =
            |name: &str| spec.get(name).filter(|v| v.get("$fn").is_some()).map(|v| h.callback(v));
        ScriptedHandler {
            id: spec.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
            priority: spec.get("priority").and_then(number).unwrap_or(f64::NAN),
            enabled_for: member("enabledFor"),
            on_down: member("onDown"),
            on_move: member("onMove"),
            on_up: member("onUp"),
            on_cancel: member("onCancel"),
            on_hover: member("onHover"),
            claims_touch: member("claimsTouch"),
            live: h.is_live(),
        }
    }

    fn sample(&self, sample: &PointerSample) -> Value {
        sample_members(sample, !self.live)
    }

    fn tell(&self, callback: &Option<Callback>, sample: &PointerSample) {
        if let Some(c) = callback {
            c.call(vec![self.sample(sample)]);
        }
    }
}

fn ask(callback: &Option<Callback>, arg: Value) -> bool {
    callback.as_ref().is_some_and(|c| answer_truthy(c.call(vec![arg])))
}

impl InteractionHandler for ScriptedHandler {
    fn id(&self) -> &str {
        &self.id
    }
    fn priority(&self) -> f64 {
        self.priority
    }
    fn enabled_for(&self, tool: &Tool) -> bool {
        ask(&self.enabled_for, tool_json(tool))
    }
    fn on_down(&self, sample: &PointerSample) -> bool {
        ask(&self.on_down, self.sample(sample))
    }
    fn on_move(&self, sample: &PointerSample) {
        self.tell(&self.on_move, sample);
    }
    fn on_up(&self, sample: &PointerSample) {
        self.tell(&self.on_up, sample);
    }
    fn on_cancel(&self, sample: &PointerSample) {
        match &self.on_cancel {
            Some(_) => self.tell(&self.on_cancel, sample),
            None => self.tell(&self.on_up, sample),
        }
    }
    fn on_hover(&self, sample: &PointerSample) {
        self.tell(&self.on_hover, sample);
    }
    fn claims_touch(&self, sample: &PointerSample) -> bool {
        ask(&self.claims_touch, self.sample(sample))
    }
}

// ── the adapter ───────────────────────────────────────────────────────────────

pub struct Interaction;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "getActiveTool",
    "getActiveToolId",
    "getDefaultToolId",
    "listTools",
    "getTool",
    "hasTool",
    "activeToolEnables",
    "activateTool",
    "activateDefaultTool",
    "pushTool",
    "popTool",
    "setToolCursor",
    "registerTool",
    "onToolChanged",
    "onGestureStarted",
    "onGestureEnded",
    "onGestureCancelled",
    "registerHandler",
    "claimCursor",
    "getCursor",
    "onCursorChanged",
    "wouldClaimTouch",
    "dispatchPointer",
];

const FEEDBACK_MEMBERS: &[&str] = &["selection", "impact", "notify"];

/// The generator's logging provider: each call is `feedback.<verb>`.
struct LoggingFeedback(Harness);

impl PlatformFeedback for LoggingFeedback {
    fn selection(&self) {
        self.0.log("feedback.selection", Vec::new());
    }
    fn impact(&self, weight: Option<ImpactWeight>) {
        let arg = weight.map_or_else(undefined, |w| serde_json::to_value(w).unwrap());
        self.0.log("feedback.impact", vec![arg]);
    }
    fn notify(&self, kind: NotifyKind) {
        self.0.log("feedback.notify", vec![serde_json::to_value(kind).unwrap()]);
    }
}

/// The unregister as a ref. On a live harness, its first call runs the
/// `Unregister` and drops it: the `Unregister` holds the handler it
/// removes, so keeping it would keep the handler's callbacks (and a live
/// host's functions) alive until the ref itself is released. A scripted
/// harness never drops it, and calls it on every invocation, exactly like
/// the TypeScript closure: `dispatch_pointer` sets `owner` to a handler
/// only after `on_down` returns, so a handler that unregisters itself
/// inside `on_down` still needs a later call to clear `owner`. Taking the
/// `Unregister` early would make that later call a no-op; only the live
/// harness accepts that trade for the memory it frees.
fn unregister_ref(h: &Harness, unregister: crate::Unregister) -> Outcome {
    if h.is_live() {
        let unregister = RefCell::new(Some(unregister));
        Outcome::Returned(h.register_fn(Rc::new(move |_| {
            let taken = unregister.borrow_mut().take();
            if let Some(unregister) = taken {
                unregister.call();
            }
            Outcome::undefined()
        })))
    } else {
        Outcome::Returned(h.register_fn(Rc::new(move |_| {
            unregister.call();
            Outcome::undefined()
        })))
    }
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

/// A `ToolChangedEvent` as the TypeScript prints it; an absent payload is
/// an `undefined` member, which the canonical form drops.
pub fn tool_changed_json(event: &ToolChangedEvent) -> Value {
    let mut out = Map::new();
    out.insert("toolId".into(), json!(event.tool_id));
    out.insert("previousToolId".into(), json!(event.previous_tool_id));
    out.insert("tool".into(), tool_json(&event.tool));
    if let Some(payload) = &event.payload {
        out.insert("payload".into(), payload.clone());
    }
    Value::Object(out)
}

fn activate_options(value: Option<&Value>) -> Option<ActivateToolOptions> {
    value.map(|o| ActivateToolOptions {
        payload: o.get("payload").filter(|p| !is_undefined(p)).cloned(),
    })
}

/// Subscribe a scripted callback to an event hook; the unsubscribe is a ref.
fn listen<T: 'static>(
    h: &Harness,
    spec: Option<&Value>,
    to_json: fn(&T) -> Value,
    on: impl FnOnce(Box<dyn Fn(&T) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = on(Box::new(move |event| match callback.call(vec![to_json(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn unit(result: Result<(), Error>) -> Outcome {
    match result {
        Ok(()) => Outcome::undefined(),
        Err(error) => Outcome::Threw(error),
    }
}

fn call_interaction(
    cap: &InteractionCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Outcome {
    let sample = |i: usize| sample_from_json(arg(args, i).unwrap_or(&Value::Null), Some(h), None);
    let gesture_json: fn(&GestureEvent) -> Value = |e| serde_json::to_value(e).unwrap();
    match method {
        "getActiveTool" => Outcome::Returned(tool_json(&cap.get_active_tool())),
        "getActiveToolId" => Outcome::Returned(json!(cap.get_active_tool_id())),
        "getDefaultToolId" => Outcome::Returned(json!(cap.get_default_tool_id())),
        "listTools" => {
            Outcome::Returned(Value::Array(cap.list_tools().iter().map(tool_json).collect()))
        }
        "getTool" => Outcome::Returned(
            cap.get_tool(&str_arg(args, 0)).map_or(Value::Null, |t| tool_json(&t)),
        ),
        "hasTool" => Outcome::Returned(json!(cap.has_tool(&str_arg(args, 0)))),
        "activeToolEnables" => Outcome::Returned(json!(cap.active_tool_enables(&str_arg(args, 0)))),
        "activateTool" => {
            unit(cap.activate_tool(&str_arg(args, 0), activate_options(arg(args, 1))))
        }
        "activateDefaultTool" => unit(cap.activate_default_tool()),
        "pushTool" => unit(cap.push_tool(&str_arg(args, 0), activate_options(arg(args, 1)))),
        "popTool" => unit(cap.pop_tool()),
        "setToolCursor" => {
            let skin = arg(args, 1).and_then(Value::as_object).map(|skin| {
                skin.iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                    .collect()
            });
            cap.set_tool_cursor(&str_arg(args, 0), skin);
            Outcome::undefined()
        }
        "registerTool" => {
            let replace = arg(args, 1).and_then(|o| o.get("replace")).is_some_and(truthy);
            match cap.register_tool(
                tool_from_json(arg(args, 0).unwrap_or(&Value::Null)),
                RegisterToolOptions { replace },
            ) {
                Ok(unregister) => unregister_ref(h, unregister),
                Err(error) => Outcome::Threw(error),
            }
        }
        "onToolChanged" => listen(h, arg(args, 0), tool_changed_json, |f| cap.on_tool_changed(f)),
        "onGestureStarted" => listen(h, arg(args, 0), gesture_json, |f| cap.on_gesture_started(f)),
        "onGestureEnded" => listen(h, arg(args, 0), gesture_json, |f| cap.on_gesture_ended(f)),
        "onGestureCancelled" => {
            listen(h, arg(args, 0), gesture_json, |f| cap.on_gesture_cancelled(f))
        }
        "registerHandler" => {
            let handler = ScriptedHandler::from_json(arg(args, 0).unwrap_or(&Value::Null), h);
            let source = opt_str(arg(args, 1).and_then(|o| o.get("source")));
            unregister_ref(
                h,
                cap.register_handler(Rc::new(handler), RegisterHandlerOptions { source }),
            )
        }
        "claimCursor" => {
            let cursor = arg(args, 1).and_then(Value::as_str);
            let priority = arg(args, 2).and_then(number).unwrap_or(0.0);
            cap.claim_cursor(&str_arg(args, 0), cursor, priority);
            Outcome::undefined()
        }
        "getCursor" => Outcome::Returned(json!(cap.get_cursor())),
        "onCursorChanged" => listen(
            h,
            arg(args, 0),
            |e: &CursorChangedEvent| serde_json::to_value(e).unwrap(),
            |f| cap.on_cursor_changed(f),
        ),
        "wouldClaimTouch" => match sample(0) {
            Ok(sample) => Outcome::Returned(json!(cap.would_claim_touch(&sample))),
            Err(error) => Outcome::Threw(error),
        },
        "dispatchPointer" => match sample(0) {
            Ok(sample) => {
                cap.dispatch_pointer(&sample);
                Outcome::undefined()
            }
            Err(error) => Outcome::Threw(error),
        },
        other => not_a_function(other),
    }
}

fn call_feedback(cap: &Feedback, method: &str, args: &[Value]) -> Outcome {
    match method {
        "selection" => cap.selection(),
        "impact" => cap.impact(arg(args, 0).and_then(|v| serde_json::from_value(v.clone()).ok())),
        "notify" => match arg(args, 0).and_then(|v| serde_json::from_value(v.clone()).ok()) {
            Some(kind) => cap.notify(kind),
            None => return Outcome::Threw(Error::new("[harness] notify: unknown kind")),
        },
        other => return not_a_function(other),
    }
    Outcome::undefined()
}

impl HarnessPlugin for Interaction {
    fn factories(&self) -> Vec<&'static str> {
        vec!["interaction", "feedback"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "feedback" => {
                let wants = config.get("provider").and_then(Value::as_bool) == Some(true);
                let provider: Option<Rc<dyn PlatformFeedback>> =
                    wants.then(|| Rc::new(LoggingFeedback(h.clone())) as Rc<dyn PlatformFeedback>);
                Ok(feedback_plugin(FeedbackPluginOptions { provider }))
            }
            _ => Ok(interaction_plugin(config_from_json(config))),
        }
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![INTERACTION.any().clone(), FEEDBACK.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<InteractionCapability>() {
            return Some(call_interaction(cap, method, &args, h));
        }
        let cap = capability.downcast_ref::<Feedback>()?;
        Some(call_feedback(cap, method, &args))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<InteractionCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        capability.downcast_ref::<Feedback>()?;
        Some(FEEDBACK_MEMBERS.to_vec())
    }
}

// ── function suite ────────────────────────────────────────────────────────────

fn value_arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

/// The generator's `withProject`: `projects` maps a pon to what `project`
/// answers; a missing key is `undefined`.
fn sample_point_on_fn(args: &[Value]) -> Result<Value, Error> {
    let sample = value_arg(args, 0);
    let project: Option<Project> = sample.get("projects").cloned().map(|table| {
        let f: Project = Rc::new(move |page| {
            table
                .get(page.page_object_number.to_string())
                .and_then(|p| serde_json::from_value::<Point>(p.clone()).ok())
        });
        f
    });
    let sample = sample_from_json(&sample, None, project)?;
    let page = PageRef::new(number(&value_arg(args, 1)).unwrap_or(f64::NAN) as u32);
    Ok(sample_point_on(&sample, page).map_or(Value::Null, |p| serde_json::to_value(p).unwrap()))
}

fn builtin_tools_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(Value::Array(builtin_tools().iter().map(tool_json).collect()))
}

fn initial_state_fn(args: &[Value]) -> Result<Value, Error> {
    let config = config_from_json(&value_arg(args, 0));
    Ok(serde_json::to_value(initial_interaction_state(&config)).unwrap())
}

fn reducer_fn(args: &[Value]) -> Result<Value, Error> {
    let state: InteractionState = decode(&value_arg(args, 0), "state")?;
    let action: InteractionAction = decode(&value_arg(args, 1), "action")?;
    let next = reduce_interaction(&state, &action).unwrap_or(state);
    Ok(serde_json::to_value(next).unwrap())
}

/// `plugins/interaction-functions.json`, by the generator's names.
pub fn functions() -> FunctionTable {
    vec![
        ("samplePointOn", sample_point_on_fn),
        ("builtinTools", builtin_tools_fn),
        ("initialInteractionState", initial_state_fn),
        ("reduceInteraction", reducer_fn),
    ]
}

#[cfg(test)]
mod unregister_ref_tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    use plugin_conformance::{Answer, LiveFn};

    use super::*;

    /// The ref `unregister_ref` handed back, called as JavaScript would call
    /// it: with no arguments.
    fn call_ref(h: &Harness, value: &Value) -> Outcome {
        let id = value["$ref"].as_u64().unwrap() as usize;
        (h.ref_fn(id).expect("the ref is still registered"))(Vec::new())
    }

    /// A scripted harness never takes the `Unregister`, so calling the ref
    /// twice runs it twice, exactly like the TypeScript closure. This is
    /// the case in the finding: a handler that unregisters itself inside
    /// `on_down`, before `dispatch_pointer` sets `owner`, needs a second
    /// call to reach the owner-clearing branch.
    #[test]
    fn a_scripted_ref_calls_unregister_every_time() {
        let h = Harness::new(Vec::new(), HashMap::new());
        let calls = Rc::new(Cell::new(0));
        let counted = calls.clone();
        let unregister = crate::Unregister::new(move || counted.set(counted.get() + 1));

        let Outcome::Returned(value) = unregister_ref(&h, unregister) else {
            panic!("unregister_ref always returns a ref");
        };
        call_ref(&h, &value);
        call_ref(&h, &value);

        assert_eq!(calls.get(), 2, "a second off() must still run the unregister, clearing owner");
    }

    /// A live harness takes the `Unregister` on the first call, so a
    /// second call is a no-op: the documented live-only divergence from
    /// the TypeScript (see docs/research/rust-plugins-web-eval.md).
    #[test]
    fn a_live_ref_calls_unregister_once() {
        let live: LiveFn = Rc::new(|_spec, _args| Answer::Returns(json!(null)));
        let h = Harness::live(Vec::new(), HashMap::new(), live);
        let calls = Rc::new(Cell::new(0));
        let counted = calls.clone();
        let unregister = crate::Unregister::new(move || counted.set(counted.get() + 1));

        let Outcome::Returned(value) = unregister_ref(&h, unregister) else {
            panic!("unregister_ref always returns a ref");
        };
        call_ref(&h, &value);
        call_ref(&h, &value);

        assert_eq!(calls.get(), 1, "the live path drops the Unregister after its first call");
    }
}
