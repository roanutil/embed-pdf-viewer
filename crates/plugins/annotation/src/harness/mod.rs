//! The conformance adapter: how `crates/plugin-conformance` builds these
//! plugins from a vector and calls their capabilities by name, plus the
//! function tables.
//!
//! It mirrors the generator's shims in
//! `tooling/plugin-conformance/src/suites/annotation-fixtures.ts`:
//! `crypto.randomUUID` is a counter reset per `annotation` build,
//! `console.error` lines log as the callback `console.error`, a
//! `renderAppearances` signal `{"$abortSignal": name}` is the signal of a
//! controller the `annotation-signals` plugin aborts, and the
//! `annotation-lens` plugin exposes the `comments` and `links` verbs flat.

pub mod functions;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_annotation_core::{
    AnnotationRef, PageRef, PdfLinkTarget, Phase, Point, Rect, TextEndAnchor, TextQuad, ViewEnv,
};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use plugin_conformance::harness::RefFn;
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, Callback, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Map, Value, json};

pub use functions::{pure_functions, repository_functions};

use crate::rich_text::TextFormat;
use crate::write::measurement::PageMeasurementViewport;
use crate::{
    ANNOTATION, AnnotationCapability, AnnotationConfig, AnnotationHost, Behavior, HitView, HoverAt,
    QuarterTurn, StampPlacement, StampPreview, StampToolInput, annotation_plugin,
};

/// The generator's `crypto.randomUUID` stand-in: the `n`th id of a run.
pub fn uuid_at(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

thread_local! {
    static CONTROLLERS: RefCell<HashMap<String, AbortSignal>> = RefCell::new(HashMap::new());
}

fn controller_named(name: &str) -> AbortSignal {
    CONTROLLERS.with(|c| c.borrow_mut().entry(name.to_string()).or_default().clone())
}

/// The `annotation-signals` test plugin's capability.
#[derive(Clone)]
pub struct Signals;

pub static SIGNALS: LazyLock<Token<Signals>> =
    LazyLock::new(|| Token::new("annotation-test/signals"));

/// The `annotation-lens` test plugin: the `comments` and `links` verbs.
#[derive(Clone)]
pub struct Lens {
    ctx: epdf_kernel::Ctx<Value, Value>,
}

pub static LENS: LazyLock<Token<Lens>> = LazyLock::new(|| Token::new("annotation-test/lens"));

/// The adapter.
pub struct Annotation;

/// The composed api's member order (`composeApi` over the controller's
/// slices). `links` and `comments` are objects of functions, which
/// [`describe_member`](HarnessPlugin::describe_member) prints.
const MEMBERS: &[&str] = &[
    "get",
    "list",
    "listRaw",
    "getRaw",
    "listSelected",
    "getSelection",
    "listChromeNodes",
    "getSelectionAnchor",
    "hitTestAt",
    "getHitKind",
    "claimsTouchAt",
    "getCursorAt",
    "listPageItems",
    "listTextItems",
    "listLinkItems",
    "getAppearanceEpoch",
    "getBakeScale",
    "renderAppearances",
    "pdfToPageRect",
    "getSelectionProps",
    "getSelectionFlags",
    "listTools",
    "getTool",
    "registerTool",
    "getToolDefaults",
    "setToolDefaults",
    "listPropSpecs",
    "listResolvedTools",
    "getResolvedTool",
    "getToolSubtype",
    "registerBehavior",
    "getBehaviorFor",
    "pruneEngagedSelection",
    "getStatus",
    "refresh",
    "onResynced",
    "getHydration",
    "ensureHydrated",
    "reloadPage",
    "deliverRemoteEvent",
    "setContents",
    "setRichText",
    "beginTextEdit",
    "beginTextEditAt",
    "endTextEdit",
    "getEditingRef",
    "getEditingId",
    "draftContents",
    "draftRichText",
    "setTextSelection",
    "getCssFontFamily",
    "create",
    "createMany",
    "createRaw",
    "update",
    "updateMany",
    "updateRaw",
    "setRotation",
    "rotateBy",
    "delete",
    "deleteMany",
    "links",
    "armStamp",
    "disarmStamp",
    "placeStamp",
    "hasArmedStamp",
    "placeArmedStamp",
    "requestStampAt",
    "getArmedStampPreview",
    "getStampArmEpoch",
    "hoverGhostAt",
    "clearGhost",
    "setPlacementPreview",
    "clearPlacementPreview",
    "getToolGhost",
    "placeAt",
    "createAttachment",
    "readAttachment",
    "setFilePickerProvider",
    "select",
    "selectAll",
    "selectInRect",
    "clearSelection",
    "updateSelection",
    "updateSelectionFlags",
    "deleteSelection",
    "rotateSelectionBy",
    "resetSelectionRotation",
    "toggleTextFormat",
    "group",
    "ungroup",
    "canGroup",
    "canUngroup",
    "setPageViewports",
    "remeasurePage",
    "editPointer",
    "marqueePointer",
    "createPointer",
    "hoverAt",
    "getCreationDraft",
    "hasCreationDraft",
    "finishCreationDraft",
    "finishInkDraft",
    "cancelCreationDraft",
    "cancel",
    "comments",
    "distanceCreationPage",
    "onDraftCaptured",
    "createMarkup",
    "createCaret",
    "createReplaceText",
    "previewMarkup",
    "clearMarkupPreview",
    "createFromSelection",
    "commitScriptEffects",
    "getSnapSettings",
    "updateSnapSettings",
    "getChromeSettings",
    "updateChromeSettings",
    "canRead",
    "canCreate",
    "canEdit",
    "canDelete",
    "onCreated",
    "onUpdated",
    "onDeleted",
    "onSelectionChanged",
    "onDraftChanged",
    "onEditingChanged",
];

const LENS_MEMBERS: &[&str] = &[
    "listThreads",
    "getThread",
    "getHydration",
    "refresh",
    "reply",
    "setText",
    "setStatus",
    "setMarked",
    "delete",
    "deleteThread",
    "getPermissions",
    "linksGet",
    "linksSet",
    "linksClear",
    "onThreadChanged",
];

/// `CommentsApi`, in the contract's order.
const COMMENTS_MEMBERS: &[&str] = &[
    "listThreads",
    "getThread",
    "reply",
    "setText",
    "setStatus",
    "setMarked",
    "delete",
    "deleteThread",
    "getPermissions",
    "onThreadChanged",
];

/// The api's `links` object.
const LINKS_MEMBERS: &[&str] = &["get", "set", "clear"];

// ── argument decoding ─────────────────────────────────────────────────────────

fn present(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn opt_num(args: &[Value], i: usize) -> Option<f64> {
    present(args, i).filter(|v| !v.is_null()).and_then(|v| from_value::<f64>(v).ok())
}

fn num(args: &[Value], i: usize) -> f64 {
    opt_num(args, i).unwrap_or(f64::NAN)
}

fn text(args: &[Value], i: usize) -> String {
    present(args, i).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn opt_text(args: &[Value], i: usize) -> Option<String> {
    present(args, i).and_then(Value::as_str).map(str::to_string)
}

fn truthy_arg(args: &[Value], i: usize) -> bool {
    crate::value::truthy(present(args, i))
}

fn opt_bool(args: &[Value], i: usize) -> Option<bool> {
    present(args, i).filter(|v| !v.is_null()).map(|v| crate::value::truthy(Some(v)))
}

fn decode<T: serde::de::DeserializeOwned>(args: &[Value], i: usize) -> Result<T, Error> {
    from_value(present(args, i).unwrap_or(&Value::Null))
}

fn opt_decode<T: serde::de::DeserializeOwned>(
    args: &[Value],
    i: usize,
) -> Result<Option<T>, Error> {
    match present(args, i) {
        Some(v) if !v.is_null() => Ok(Some(from_value(v)?)),
        _ => Ok(None),
    }
}

fn page(args: &[Value], i: usize) -> Result<PageRef, Error> {
    decode(args, i)
}

fn view_arg(args: &[Value], i: usize) -> Result<Option<ViewEnv>, Error> {
    opt_decode(args, i)
}

fn phase(args: &[Value], i: usize) -> Result<Phase, Error> {
    decode(args, i)
}

/// The JSON a trace records for a serializable value.
fn out<T: Serialize>(value: &T) -> Outcome {
    Outcome::Returned(to_value(value))
}

fn out_opt<T: Serialize>(value: Option<T>) -> Outcome {
    Outcome::Returned(value.map_or(Value::Null, |v| to_value(&v)))
}

/// A typed promise as the JSON promise a trace records, settling in the
/// same moment.
fn untyped<T: Clone + 'static>(
    promise: Promise<T>,
    json: impl Fn(&T) -> Value + 'static,
) -> Outcome {
    let (out, resolver) = Promise::pending();
    promise
        .on_settle(move |result| resolver.settle(result.as_ref().map(&json).map_err(Clone::clone)));
    Outcome::Promise(out)
}

fn unit(promise: Promise<()>) -> Outcome {
    untyped(promise, |_| undefined())
}

fn undef() -> Outcome {
    Outcome::undefined()
}

fn from_result(result: Result<Outcome, Error>) -> Outcome {
    result.unwrap_or_else(Outcome::Threw)
}

fn not_a_function(method: &str) -> Outcome {
    Outcome::Threw(Error::new(format!("[scenario] \"{method}\" is not a function")))
}

fn unregister_ref(h: &Harness, un: epdf_plugin_interaction::Unregister) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        un.call();
        Outcome::undefined()
    })))
}

fn unsubscribe_ref(h: &Harness, un: Unsubscribe) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        un.call();
        Outcome::undefined()
    })))
}

/// An event hook: subscribe a scripted callback, return its unsubscribe.
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
    unsubscribe_ref(h, off)
}

// ── scripted callbacks ────────────────────────────────────────────────────────

fn answer_promise(answer: Answer) -> Promise<Value> {
    match answer {
        Answer::Returns(value) => Promise::resolved(value),
        Answer::Throws(error) => Promise::rejected(error),
        Answer::Promise(promise) => promise,
    }
}

/// A `Behavior` object of scripted `$fn` members.
struct ScriptedBehavior {
    id: String,
    spec: Value,
    matches: Option<Callback>,
    engaged: Option<Callback>,
    /// A callback for every `$fn` member, made at registration, by member:
    /// [`behavior_json`] restarts these rather than making new ones from
    /// `spec`, whose functions a live host may have forgotten by then.
    members: HashMap<String, Callback>,
}

impl Behavior for ScriptedBehavior {
    fn id(&self) -> &str {
        &self.id
    }
    fn matches(&self, subtype: &str, r: Option<&AnnotationRef>) -> bool {
        let arg = json!({ "subtype": subtype, "ref": r.map_or(Value::Null, to_value) });
        self.matches
            .as_ref()
            .is_some_and(|c| epdf_plugin_interaction::harness::answer_truthy(c.call(vec![arg])))
    }
    fn engaged(&self) -> bool {
        self.engaged
            .as_ref()
            .is_some_and(|c| epdf_plugin_interaction::harness::answer_truthy(c.call(Vec::new())))
    }
}

fn scripted_behavior(spec: &Value, h: &Harness) -> ScriptedBehavior {
    let member =
        |name: &str| spec.get(name).filter(|v| v.get("$fn").is_some()).map(|v| h.callback(v));
    let members = spec
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.get("$fn").is_some())
        .map(|(key, v)| (key.clone(), h.callback(v)))
        .collect();
    ScriptedBehavior {
        id: spec.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
        spec: spec.clone(),
        matches: member("matches"),
        engaged: member("engaged"),
        members,
    }
}

/// The behavior object as the TypeScript prints it: its function members as
/// fresh refs, in the spec's member order.
fn behavior_json(
    b: &Rc<dyn Behavior>,
    registry: &Rc<RefCell<Vec<(Rc<dyn Behavior>, Rc<ScriptedBehavior>)>>>,
    h: &Harness,
) -> Value {
    let Some((_, scripted)) = registry.borrow().iter().find(|(x, _)| Rc::ptr_eq(x, b)).cloned()
    else {
        return sibling_behavior_json(b, h);
    };
    let mut out = Map::new();
    for (key, value) in scripted.spec.as_object().into_iter().flatten() {
        if let Some(callback) = scripted.members.get(key) {
            let callback = callback.restarted();
            out.insert(
                key.clone(),
                h.register_fn(Rc::new(move |args| match callback.call(args) {
                    Answer::Returns(v) => Outcome::Returned(v),
                    Answer::Throws(e) => Outcome::Threw(e),
                    Answer::Promise(p) => Outcome::Promise(p),
                })),
            );
        } else {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

/// A behavior a sibling plugin registered (link, form): the object literal
/// `{id, matches, engaged}` those plugins write, its two functions as fresh
/// refs that call the behavior.
fn sibling_behavior_json(b: &Rc<dyn Behavior>, h: &Harness) -> Value {
    let (on_matches, on_engaged) = (b.clone(), b.clone());
    let matches = h.register_fn(Rc::new(move |args: Vec<Value>| {
        let a = args.first().cloned().unwrap_or(Value::Null);
        let subtype = a.get("subtype").and_then(Value::as_str).unwrap_or_default();
        let r: Option<AnnotationRef> = a.get("ref").and_then(|v| from_value(v).ok());
        Outcome::Returned(json!(on_matches.matches(subtype, r.as_ref())))
    }));
    let engaged = h.register_fn(Rc::new(move |_| Outcome::Returned(json!(on_engaged.engaged()))));
    json!({ "id": b.id(), "matches": matches, "engaged": engaged })
}

thread_local! {
    static BEHAVIORS: Rc<RefCell<Vec<(Rc<dyn Behavior>, Rc<ScriptedBehavior>)>>> = Rc::default();
}

// ── the capability ────────────────────────────────────────────────────────────

fn hover_arg(v: Option<&Value>) -> Result<Option<HoverAt>, Error> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(None) };
    let n = |k: &str| v.get(k).filter(|x| !x.is_null() && !is_undefined(x)).and_then(Value::as_f64);
    Ok(Some(HoverAt {
        page: from_value(v.get("page").unwrap_or(&Value::Null))?,
        point: from_value(v.get("point").unwrap_or(&Value::Null))?,
        scale: n("scale"),
        rotation: n("rotation"),
        zoom: n("zoom"),
    }))
}

fn preview_json(p: &Option<crate::ArmedStampPreview>) -> Value {
    p.as_ref().map_or(Value::Null, |p| {
        let mut o = Map::new();
        o.insert("bytes".into(), epdf_kernel::bytes::Bytes(p.bytes.clone()).to_json());
        if let Some(m) = &p.mime_type {
            o.insert("mimeType".into(), json!(m));
        }
        Value::Object(o)
    })
}

/// A `StampToolInput` from its JSON; a `$fn` preview is a scripted
/// provider answering `{bytes, mimeType?}` or `null`.
fn stamp_input(v: Option<&Value>, h: &Harness) -> Result<StampToolInput, Error> {
    let v = v.cloned().unwrap_or(Value::Null);
    let member = |k: &str| v.get(k).filter(|x| !is_undefined(x)).cloned();
    let preview = match member("preview") {
        Some(p) if p.get("$fn").is_some() => {
            let callback = h.callback(&p);
            let provider: crate::StampPreviewProvider = Rc::new(move |px| {
                answer_promise(callback.call(vec![json!(px)])).map(|v| {
                    if v.is_null() {
                        return Ok(None);
                    }
                    let bytes = v
                        .get("bytes")
                        .and_then(epdf_kernel::bytes::Bytes::from_json)
                        .map(|b| b.0)
                        .unwrap_or_default();
                    let mime_type = v.get("mimeType").and_then(Value::as_str).map(str::to_string);
                    Ok(Some(crate::ArmedStampPreview { bytes, mime_type }))
                })
            });
            Some(StampPreview::Provider(provider))
        }
        Some(p) => Some(StampPreview::Source(p)),
        None => None,
    };
    Ok(StampToolInput {
        source: member("source").unwrap_or_else(undefined),
        target_width: member("targetWidth").and_then(|x| x.as_f64()),
        name: member("name").and_then(|x| x.as_str().map(str::to_string)),
        subject: member("subject").and_then(|x| x.as_str().map(str::to_string)),
        preview,
        intrinsic_size: match member("intrinsicSize") {
            Some(s) if !s.is_null() => Some(from_value(&s)?),
            _ => None,
        },
    })
}

fn refs_arg(v: Option<&Value>) -> Result<Vec<AnnotationRef>, Error> {
    match v {
        Some(Value::Array(list)) => list.iter().map(from_value).collect(),
        Some(v) => Ok(vec![from_value(v)?]),
        None => Ok(Vec::new()),
    }
}

fn add_option(v: Option<&Value>) -> Option<bool> {
    v.and_then(|o| o.get("add")).filter(|v| !is_undefined(v)).map(|v| crate::value::truthy(Some(v)))
}

fn call_capability(
    cap: &AnnotationCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let point = |i: usize| -> Result<Point, Error> { decode(args, i) };
    let r = |i: usize| -> Result<AnnotationRef, Error> { decode(args, i) };
    Ok(match method {
        "get" => out_opt(cap.get(&r(0)?)),
        "list" => out(&cap.list(opt_decode(args, 0)?.as_ref())),
        "listRaw" => out(&cap.list_raw(opt_decode(args, 0)?.as_ref())),
        "getRaw" => Outcome::Returned(cap.get_raw(&r(0)?).unwrap_or(Value::Null)),
        "listSelected" => out(&cap.list_selected()),
        "getSelection" => out(&cap.get_selection()),
        "listChromeNodes" => out(&cap.list_chrome_nodes(
            page(args, 0)?,
            opt_num(args, 1),
            opt_num(args, 2),
            opt_num(args, 3),
        )),
        "getSelectionAnchor" => {
            let v = present(args, 0).cloned().unwrap_or(Value::Null);
            let n = |k: &str| {
                v.get(k).filter(|x| !x.is_null() && !is_undefined(x)).and_then(Value::as_f64)
            };
            out_opt(cap.get_selection_anchor(HitView {
                scale: n("scale"),
                rotation: n("rotation"),
                zoom: n("zoom"),
            }))
        }
        "hitTestAt" => out_opt(cap.hit_test_at(page(args, 0)?, point(1)?)),
        "getHitKind" => out(&cap.get_hit_kind(
            page(args, 0)?,
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
            opt_bool(args, 5),
        )),
        "claimsTouchAt" => out(&cap.claims_touch_at(
            page(args, 0)?,
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
        )),
        "getCursorAt" => out_opt(cap.get_cursor_at(
            page(args, 0)?,
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
        )),
        "listPageItems" => out(&cap.list_page_items(page(args, 0)?, view_arg(args, 1)?)),
        "listTextItems" => out(&cap.list_text_items(page(args, 0)?, view_arg(args, 1)?)),
        "listLinkItems" => out(&cap.list_link_items(page(args, 0)?)),
        "getAppearanceEpoch" => out(&cap.get_appearance_epoch(page(args, 0)?)),
        "getBakeScale" => out(&cap.get_bake_scale(num(args, 0))?),
        "renderAppearances" => {
            let signal = present(args, 2)
                .and_then(|s| s.get("$abortSignal"))
                .and_then(Value::as_str)
                .map(controller_named);
            Outcome::Promise(cap.render_appearances(page(args, 0)?, num(args, 1), signal)?)
        }
        "pdfToPageRect" => {
            let rect = crate::value::pdf_rect_of(present(args, 1));
            out_opt(cap.pdf_to_page_rect(page(args, 0)?, rect))
        }
        "getSelectionProps" => out(&cap.get_selection_props()?),
        "getSelectionFlags" => {
            Outcome::Returned(cap.get_selection_flags().map_or(Value::Null, Value::Object))
        }
        "listTools" => out(&cap.list_tools()),
        "getTool" => out_opt(cap.get_tool(&text(args, 0))),
        "registerTool" => unregister_ref(h, cap.register_tool(decode(args, 0)?)?),
        "getToolDefaults" => out(&cap.get_tool_defaults(&text(args, 0))),
        "setToolDefaults" => {
            cap.set_tool_defaults(&text(args, 0), decode(args, 1)?);
            undef()
        }
        "listPropSpecs" => out(&cap.list_prop_specs(&text(args, 0))),
        "listResolvedTools" => Outcome::Returned(Value::Array(
            cap.list_resolved_tools().iter().map(|t| t.to_json()).collect(),
        )),
        "getResolvedTool" => Outcome::Returned(
            cap.get_resolved_tool(&text(args, 0)).map_or(Value::Null, |t| t.to_json()),
        ),
        "getToolSubtype" => out(&cap.get_tool_subtype(&text(args, 0))),
        "registerBehavior" => {
            let spec = present(args, 0).cloned().unwrap_or(Value::Null);
            let scripted = Rc::new(scripted_behavior(&spec, h));
            let behavior: Rc<dyn Behavior> = scripted.clone();
            BEHAVIORS.with(|reg| reg.borrow_mut().push((behavior.clone(), scripted)));
            unregister_ref(h, cap.register_behavior(behavior))
        }
        "getBehaviorFor" => {
            let a = present(args, 0).cloned().unwrap_or(Value::Null);
            let subtype = a.get("subtype").and_then(Value::as_str).unwrap_or_default().to_string();
            let r: Option<AnnotationRef> = a.get("ref").and_then(|v| from_value(v).ok());
            match cap.get_behavior_for(&subtype, r.as_ref()) {
                Some(b) => Outcome::Returned(BEHAVIORS.with(|reg| behavior_json(&b, reg, h))),
                None => Outcome::Returned(Value::Null),
            }
        }
        "pruneEngagedSelection" => {
            cap.prune_engaged_selection();
            undef()
        }
        "getStatus" => out(&cap.get_status()),
        "refresh" => unit(cap.refresh()),
        "onResynced" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_resynced(f))
        }
        "getHydration" => out(&cap.get_hydration()),
        "ensureHydrated" => {
            cap.ensure_hydrated();
            undef()
        }
        "reloadPage" => unit(cap.reload_page(page(args, 0)?)),
        "deliverRemoteEvent" => {
            cap.deliver_remote_event(present(args, 0).cloned().unwrap_or(Value::Null));
            undef()
        }
        "setContents" => unit(cap.set_contents(r(0)?, text(args, 1))),
        "setRichText" => unit(cap.set_rich_text(r(0)?, decode(args, 1)?)),
        "beginTextEdit" => {
            cap.begin_text_edit(&r(0)?);
            undef()
        }
        "beginTextEditAt" => out(&cap.begin_text_edit_at(
            page(args, 0)?,
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
        )),
        "endTextEdit" => unit(cap.end_text_edit()),
        "getEditingRef" => out_opt(cap.get_editing_ref()),
        "getEditingId" => out(&cap.get_editing_id()),
        "draftContents" => {
            cap.draft_contents(r(0)?, text(args, 1));
            undef()
        }
        "draftRichText" => {
            cap.draft_rich_text(r(0)?, decode(args, 1)?);
            undef()
        }
        "setTextSelection" => {
            let range = present(args, 1).filter(|v| !v.is_null()).map(|v| {
                (
                    v.get("start").and_then(Value::as_f64).unwrap_or(f64::NAN),
                    v.get("end").and_then(Value::as_f64).unwrap_or(f64::NAN),
                )
            });
            cap.set_text_selection(&r(0)?, range);
            undef()
        }
        "getCssFontFamily" => out(&cap.get_css_font_family(&text(args, 0))),
        "create" => untyped(cap.create(decode(args, 0)?), to_value),
        "createMany" => {
            let inputs: Vec<Value> = decode(args, 0)?;
            untyped(cap.create_many(inputs), |b| b.to_json())
        }
        "createRaw" => untyped(
            cap.create_raw(page(args, 0)?, present(args, 1).cloned().unwrap_or(Value::Null)),
            to_value,
        ),
        "update" => unit(cap.update(r(0)?, decode(args, 1)?)),
        "updateMany" => {
            untyped(cap.update_many(refs_arg(present(args, 0))?, decode(args, 1)?), |b| b.to_json())
        }
        "updateRaw" => {
            unit(cap.update_raw(r(0)?, present(args, 1).cloned().unwrap_or(Value::Null)))
        }
        "setRotation" => unit(cap.set_rotation(r(0)?, num(args, 1))),
        "rotateBy" => unit(cap.rotate_by(r(0)?, num(args, 1))),
        "delete" => unit(cap.delete(r(0)?)),
        "deleteMany" => untyped(cap.delete_many(refs_arg(present(args, 0))?), |b| b.to_json()),
        "armStamp" => unit(cap.arm_stamp(stamp_input(present(args, 0), h)?)),
        "disarmStamp" => {
            cap.disarm_stamp();
            undef()
        }
        "placeStamp" => {
            let placement: StampPlacement = decode(args, 1)?;
            untyped(cap.place_stamp(stamp_input(present(args, 0), h)?, placement), to_value)
        }
        "hasArmedStamp" => out(&cap.has_armed_stamp()),
        "placeArmedStamp" => {
            out(&cap.place_armed_stamp(page(args, 0)?, point(1)?, opt_num(args, 2)))
        }
        "requestStampAt" => out(&cap.request_stamp_at(page(args, 0)?, point(1)?, opt_num(args, 2))),
        "getArmedStampPreview" => {
            untyped(cap.get_armed_stamp_preview(opt_num(args, 0)), preview_json)
        }
        "getStampArmEpoch" => out(&cap.get_stamp_arm_epoch()),
        "hoverGhostAt" => {
            cap.hover_ghost_at(&text(args, 0), page(args, 1)?, point(2)?, opt_num(args, 3));
            undef()
        }
        "clearGhost" => {
            cap.clear_ghost();
            undef()
        }
        "setPlacementPreview" => {
            let bx: Rect = decode(args, 2)?;
            cap.set_placement_preview(&text(args, 0), page(args, 1)?, bx);
            undef()
        }
        "clearPlacementPreview" => {
            cap.clear_placement_preview();
            undef()
        }
        "getToolGhost" => out_opt(cap.get_tool_ghost(page(args, 0)?)),
        "placeAt" => out(&cap.place_at(page(args, 0)?, point(1)?, opt_num(args, 2))),
        "createAttachment" => untyped(
            cap.create_attachment(
                page(args, 0)?,
                point(1)?,
                present(args, 2).cloned().unwrap_or(Value::Null),
            ),
            to_value,
        ),
        "readAttachment" => untyped(cap.read_attachment(r(0)?), Clone::clone),
        "setFilePickerProvider" => {
            let provider = present(args, 0).filter(|v| v.get("$fn").is_some()).map(|spec| {
                let callback = h.callback(spec);
                let provider: crate::FilePickerProvider =
                    Rc::new(move |req| answer_promise(callback.call(vec![to_value(&req)])));
                provider
            });
            unsubscribe_ref(h, cap.set_file_picker_provider(provider))
        }
        "select" => {
            cap.select(&refs_arg(present(args, 0))?, add_option(present(args, 1)));
            undef()
        }
        "selectAll" => {
            cap.select_all(opt_decode(args, 0)?);
            undef()
        }
        "selectInRect" => {
            cap.select_in_rect(page(args, 0)?, decode(args, 1)?, add_option(present(args, 2)));
            undef()
        }
        "clearSelection" => {
            cap.clear_selection();
            undef()
        }
        "updateSelection" => untyped(cap.update_selection(decode(args, 0)?), |b| b.to_json()),
        "updateSelectionFlags" => {
            untyped(cap.update_selection_flags(decode(args, 0)?), |b| b.to_json())
        }
        "deleteSelection" => untyped(cap.delete_selection(), |b| b.to_json()),
        "rotateSelectionBy" => unit(cap.rotate_selection_by(QuarterTurn::from_delta(num(args, 0)))),
        "resetSelectionRotation" => unit(cap.reset_selection_rotation()),
        "toggleTextFormat" => {
            let format: TextFormat = decode(args, 0)?;
            unit(cap.toggle_text_format(format))
        }
        "group" => unit(cap.group()),
        "ungroup" => unit(cap.ungroup()),
        "canGroup" => out(&cap.can_group()),
        "canUngroup" => out(&cap.can_ungroup()),
        "setPageViewports" => {
            let viewports: Option<Vec<PageMeasurementViewport>> = opt_decode(args, 1)?;
            cap.set_page_viewports(page(args, 0)?, viewports, opt_decode(args, 2)?);
            undef()
        }
        "remeasurePage" => untyped(
            cap.remeasure_page(page(args, 0)?, present(args, 1).cloned().unwrap_or(Value::Null)),
            |r| r.to_json(),
        ),
        "editPointer" => {
            cap.edit_pointer(
                phase(args, 0)?,
                page(args, 1)?,
                point(2)?,
                truthy_arg(args, 3),
                opt_num(args, 4),
                opt_num(args, 5),
                opt_num(args, 6),
                opt_bool(args, 7),
            );
            undef()
        }
        "marqueePointer" => {
            cap.marquee_pointer(
                phase(args, 0)?,
                page(args, 1)?,
                point(2)?,
                truthy_arg(args, 3),
                opt_num(args, 4),
                opt_num(args, 5),
                opt_num(args, 6),
            );
            undef()
        }
        "createPointer" => {
            cap.create_pointer(
                &text(args, 0),
                phase(args, 1)?,
                page(args, 2)?,
                point(3)?,
                truthy_arg(args, 4),
                opt_num(args, 5),
            );
            undef()
        }
        "hoverAt" => {
            cap.hover_at(hover_arg(present(args, 0))?);
            undef()
        }
        "getCreationDraft" => out_opt(cap.get_creation_draft()),
        "hasCreationDraft" => out(&cap.has_creation_draft()),
        "finishCreationDraft" => untyped(cap.finish_creation_draft(), to_value),
        "finishInkDraft" => {
            cap.finish_ink_draft();
            undef()
        }
        "cancelCreationDraft" => {
            cap.cancel_creation_draft();
            undef()
        }
        "cancel" => {
            cap.cancel();
            undef()
        }
        "distanceCreationPage" => out_opt(cap.distance_creation_page()),
        "onDraftCaptured" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_draft_captured(f))
        }
        "createMarkup" => {
            let quads: Vec<TextQuad> = decode(args, 2)?;
            cap.create_markup(&text(args, 0), page(args, 1)?, quads, opt_text(args, 3).as_deref());
            undef()
        }
        "createCaret" => {
            let anchor: TextEndAnchor = decode(args, 1)?;
            cap.create_caret(page(args, 0)?, anchor);
            undef()
        }
        "createReplaceText" => {
            let quads: Vec<TextQuad> = decode(args, 1)?;
            let anchor: TextEndAnchor = decode(args, 2)?;
            cap.create_replace_text(page(args, 0)?, quads, anchor, opt_text(args, 3).as_deref());
            undef()
        }
        "previewMarkup" => {
            cap.preview_markup(&text(args, 0), decode(args, 1)?, opt_text(args, 2).as_deref());
            undef()
        }
        "clearMarkupPreview" => {
            cap.clear_markup_preview();
            undef()
        }
        "createFromSelection" => {
            let options = present(args, 1).cloned().unwrap_or(Value::Null);
            let preset = options.get("preset").and_then(Value::as_str).map(str::to_string);
            let clear = options
                .get("clear")
                .filter(|v| !is_undefined(v))
                .map(|v| crate::value::truthy(Some(v)));
            untyped(cap.create_from_selection(&text(args, 0), preset, clear), to_value)
        }
        "commitScriptEffects" => untyped(cap.commit_script_effects(decode(args, 0)?), to_value),
        "getSnapSettings" => out(&cap.get_snap_settings()),
        "updateSnapSettings" => {
            cap.update_snap_settings(decode(args, 0)?);
            undef()
        }
        "getChromeSettings" => out(&cap.get_chrome_settings()),
        "updateChromeSettings" => {
            cap.update_chrome_settings(decode(args, 0)?);
            undef()
        }
        "canRead" => out(&cap.can_read()),
        "canCreate" => out(&cap.can_create()),
        "canEdit" => out(&cap.can_edit(&r(0)?)),
        "canDelete" => out(&cap.can_delete(&r(0)?)),
        "onCreated" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_created(f))
        }
        "onUpdated" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_updated(f))
        }
        "onDeleted" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_deleted(f))
        }
        "onSelectionChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_selection_changed(f))
        }
        "onDraftChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_draft_changed(f))
        }
        "onEditingChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_editing_changed(f))
        }
        other => not_a_function(other),
    })
}

fn call_lens(lens: &Lens, method: &str, args: &[Value], h: &Harness) -> Result<Outcome, Error> {
    let cap = lens.ctx.get(&ANNOTATION)?;
    match method {
        "getHydration" => Ok(out(&cap.get_hydration())),
        "refresh" => Ok(unit(cap.refresh())),
        "linksGet" => call_links(&cap, "get", args),
        "linksSet" => call_links(&cap, "set", args),
        "linksClear" => call_links(&cap, "clear", args),
        verb => call_comments(&cap, verb, args, h),
    }
}

/// One `comments` verb. The capability's `comments` object and the test
/// lens's flat verbs both call it.
fn call_comments(
    cap: &AnnotationCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let comments = cap.comments();
    let r = |i: usize| -> Result<AnnotationRef, Error> { decode(args, i) };
    Ok(match method {
        "listThreads" => Outcome::Returned(Value::Array(
            comments.list_threads().iter().map(|t| t.to_json()).collect(),
        )),
        "getThread" => {
            Outcome::Returned(comments.get_thread(&r(0)?).map_or(Value::Null, |t| t.to_json()))
        }
        "reply" => untyped(comments.reply(r(0)?, text(args, 1)), to_value),
        "setText" => unit(comments.set_text(r(0)?, text(args, 1))),
        "setStatus" => unit(comments.set_status(r(0)?, text(args, 1))),
        "setMarked" => unit(comments.set_marked(r(0)?, truthy_arg(args, 1))),
        "delete" => unit(comments.delete(r(0)?)),
        "deleteThread" => untyped(comments.delete_thread(r(0)?), |t| t.to_json()),
        "getPermissions" => out(&comments.get_permissions(&r(0)?)),
        "onThreadChanged" => {
            let comments = comments.clone();
            hook(h, present(args, 0), move |f| comments.on_thread_changed(f))
        }
        other => not_a_function(other),
    })
}

/// One `links` verb.
fn call_links(cap: &AnnotationCapability, method: &str, args: &[Value]) -> Result<Outcome, Error> {
    let links = cap.links();
    let r = |i: usize| -> Result<AnnotationRef, Error> { decode(args, i) };
    Ok(match method {
        "get" => out_opt(links.get(&r(0)?)),
        "set" => {
            let target: PdfLinkTarget = decode(args, 1)?;
            unit(links.set(&r(0)?, target))
        }
        "clear" => unit(links.clear(&r(0)?)),
        other => not_a_function(other),
    })
}

/// A nested object of functions: each verb a ref into `call`.
fn verbs_json(
    h: &Harness,
    cap: &AnnotationCapability,
    verbs: &[&'static str],
    call: fn(&AnnotationCapability, &str, &[Value], &Harness) -> Result<Outcome, Error>,
) -> Value {
    let mut out = serde_json::Map::new();
    for &verb in verbs {
        let cap = cap.clone();
        let harness = h.clone();
        let f: RefFn =
            Rc::new(move |args: Vec<Value>| from_result(call(&cap, verb, &args, &harness)));
        out.insert(verb.to_string(), h.register_fn(f));
    }
    Value::Object(out)
}

/// A live host's globals: `host: {randomUuid, consoleError}` in the config,
/// each a `$fn` the host answers (`crypto.randomUUID` and `console.error` in
/// a browser), so ids differ across sessions and swallowed failures reach
/// the console. Only a live harness reads it; a vector's config never has
/// one.
fn live_host(host: &mut AnnotationHost, config: &Value, h: &Harness) {
    if !h.is_live() {
        return;
    }
    let Some(live) = config.get("host") else { return };
    let spec = |name: &str| live.get(name).filter(|f| f.get("$fn").is_some());
    if let Some(random_uuid) = spec("randomUuid").map(|f| h.callback(f)) {
        let fallback = host.random_uuid.take();
        host.random_uuid = Some(Rc::new(move || match random_uuid.call(Vec::new()) {
            Answer::Returns(Value::String(uuid)) => uuid,
            _ => fallback.as_ref().map(|f| f()).unwrap_or_default(),
        }));
    }
    if let Some(console_error) = spec("consoleError").map(|f| h.callback(f)) {
        host.console_error = Some(Rc::new(move |message: &str, error: Option<&Error>| {
            let mut args = vec![json!(message)];
            if let Some(error) = error {
                args.push(error.to_canonical());
            }
            let _ = console_error.call(args);
        }));
    }
}

impl HarnessPlugin for Annotation {
    fn factories(&self) -> Vec<&'static str> {
        vec!["annotation", "annotation-lens", "annotation-signals"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "annotation-signals" => {
                return Ok(define::<Value, Value>("annotation-test/signals")
                    .capability(&SIGNALS, |_| Ok(Signals))
                    .build());
            }
            "annotation-lens" => {
                return Ok(define::<Value, Value>("annotation-test/lens")
                    .document_scoped()
                    .requires(ANNOTATION.any())
                    .capability(&LENS, |ctx| Ok(Lens { ctx }))
                    .build());
            }
            _ => {}
        }
        CONTROLLERS.with(|c| c.borrow_mut().clear());
        BEHAVIORS.with(|b| b.borrow_mut().clear());
        let raw = config;
        let mut config: AnnotationConfig = if is_undefined(config) || config.is_null() {
            AnnotationConfig::default()
        } else {
            from_value(config)?
        };
        let seq = Rc::new(Cell::new(0u64));
        let log = h.clone();
        config.host = AnnotationHost {
            console_error: Some(Rc::new(move |message: &str, error: Option<&Error>| {
                let mut args = vec![json!(message)];
                if let Some(error) = error {
                    args.push(error.to_canonical());
                }
                log.log("console.error", args);
            })),
            random_uuid: Some(Rc::new(move || {
                seq.set(seq.get() + 1);
                uuid_at(seq.get())
            })),
            fonts: None,
        };
        live_host(&mut config.host, raw, h);
        Ok(annotation_plugin(config))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![ANNOTATION.any().clone(), LENS.any().clone(), SIGNALS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(cap) = capability.downcast_ref::<AnnotationCapability>() {
            return Some(from_result(call_capability(cap, method, &args, h)));
        }
        if let Some(lens) = capability.downcast_ref::<Lens>() {
            return Some(from_result(call_lens(lens, method, &args, h)));
        }
        capability.downcast_ref::<Signals>()?;
        Some(match method {
            "abort" => {
                let name = present(&args, 0).and_then(Value::as_str).unwrap_or_default();
                controller_named(name)
                    .abort(Error::named("AbortError", "This operation was aborted"));
                Outcome::undefined()
            }
            other => not_a_function(other),
        })
    }

    fn describe_member(
        &self,
        capability: &Rc<dyn Any>,
        member: &'static str,
        h: &Harness,
    ) -> Option<Value> {
        let cap = capability.downcast_ref::<AnnotationCapability>()?;
        match member {
            "comments" => Some(verbs_json(h, cap, COMMENTS_MEMBERS, call_comments)),
            "links" => Some(verbs_json(h, cap, LINKS_MEMBERS, |cap, verb, args, _| {
                call_links(cap, verb, args)
            })),
            _ => None,
        }
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<AnnotationCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        if capability.downcast_ref::<Lens>().is_some() {
            return Some(LENS_MEMBERS.to_vec());
        }
        capability.downcast_ref::<Signals>()?;
        Some(vec!["abort"])
    }
}
