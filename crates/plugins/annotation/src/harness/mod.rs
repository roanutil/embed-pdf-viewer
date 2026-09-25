//! The conformance adapter: how `crates/plugin-conformance` builds these
//! plugins from a vector and calls their capabilities by name, plus the
//! function tables.
//!
//! It mirrors the generator's shims in
//! `tooling/plugin-conformance/src/suites/annotation-fixtures.ts`:
//! `crypto.randomUUID` is a counter reset per `annotation` build,
//! `console.error` lines log as the callback `console.error`, an
//! `appearances` signal `{"$abortSignal": name}` is the signal of a
//! controller the `annotation-signals` plugin aborts, and the
//! `annotation-lens` plugin exposes the `comments` and `links` verbs flat.

pub mod functions;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_annotation_core::{
    AnnotationRef, PdfLinkTarget, Phase, Point, Rect, TextEndAnchor, TextQuad, ViewEnv,
};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, AnyToken, Ctx, Error, Token, define, is_undefined, undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, Callback, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Map, Value, json};

pub use functions::{pure_functions, repository_functions};

use crate::{
    ANNOTATION, AnnotationCapability, AnnotationConfig, AnnotationHost, Behavior, HoverAt,
    annotation_plugin,
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
    ctx: Ctx<Value, Value>,
}

pub static LENS: LazyLock<Token<Lens>> = LazyLock::new(|| Token::new("annotation-test/lens"));

/// The adapter.
pub struct Annotation;

/// The TypeScript object literal's member order (`comments` and `links` are
/// objects, which this adapter does not print).
const MEMBERS: &[&str] = &[
    "create",
    "armStamp",
    "disarmStamp",
    "placeArmedStamp",
    "requestStampAt",
    "placeAt",
    "hasArmedStamp",
    "ghostHoverAt",
    "clearGhost",
    "setPlacementPreview",
    "clearPlacementPreview",
    "toolGhost",
    "armedStampPreview",
    "stampArmEpoch",
    "setFilePickerProvider",
    "downloadAttachment",
    "tools",
    "tool",
    "toolSubtype",
    "registerTool",
    "update",
    "updateSelection",
    "updateSelectionFlags",
    "getSelectionFlags",
    "delete",
    "canRead",
    "canCreate",
    "canEdit",
    "canDelete",
    "getSelection",
    "get",
    "list",
    "getSelected",
    "getSelectionProps",
    "propsForTool",
    "pageItems",
    "chrome",
    "selectionAnchor",
    "creationDraftAnchor",
    "selection",
    "hitKind",
    "claimsTouchAt",
    "cursorAt",
    "hoverAt",
    "behaviorFor",
    "linkItemsOn",
    "appearanceEpoch",
    "bakeScale",
    "appearances",
    "toContentBox",
    "editPointer",
    "marqueePointer",
    "createPointer",
    "finishCreationDraft",
    "finishInkDraft",
    "cancelCreationDraft",
    "markupFromSelection",
    "createMarkup",
    "createCaret",
    "createReplaceText",
    "previewMarkup",
    "clearMarkupPreview",
    "setDefaults",
    "currentDefaults",
    "setSnap",
    "snapSettings",
    "setChrome",
    "chromeSettings",
    "deleteSelection",
    "deselect",
    "select",
    "pruneEngagedSelection",
    "cancel",
    "rotateSelection90",
    "resetSelectionRotation",
    "group",
    "ungroup",
    "canGroup",
    "canUngroup",
    "ensurePage",
    "reloadPage",
    "hydration",
    "ensureHydrated",
    "rehydrate",
    "deliverRemoteAnnotationEvent",
    "textItems",
    "currentEditing",
    "beginTextEdit",
    "beginTextEditAt",
    "setContents",
    "endTextEdit",
    "commitScriptEffects",
    "registerBehavior",
];

const LENS_MEMBERS: &[&str] = &[
    "threads",
    "thread",
    "hydration",
    "rehydrate",
    "reply",
    "edit",
    "setStatus",
    "setMarked",
    "remove",
    "removeThread",
    "permissionsFor",
    "linksOf",
    "linksSet",
    "linksClear",
];

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

fn pon(args: &[Value], i: usize) -> u32 {
    num(args, i) as u32
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

fn view_arg(args: &[Value], i: usize) -> Result<Option<ViewEnv>, Error> {
    match present(args, i) {
        Some(v) if !v.is_null() => Ok(Some(from_value(v)?)),
        _ => Ok(None),
    }
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
    ScriptedBehavior {
        id: spec.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
        spec: spec.clone(),
        matches: member("matches"),
        engaged: member("engaged"),
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
        return json!({ "id": b.id() });
    };
    let mut out = Map::new();
    for (key, value) in scripted.spec.as_object().into_iter().flatten() {
        if value.get("$fn").is_some() {
            let callback = h.callback(value);
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

thread_local! {
    static BEHAVIORS: Rc<RefCell<Vec<(Rc<dyn Behavior>, Rc<ScriptedBehavior>)>>> = Rc::default();
}

// ── the capability ────────────────────────────────────────────────────────────

fn hover_arg(v: Option<&Value>) -> Result<Option<HoverAt>, Error> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(None) };
    let n = |k: &str| v.get(k).filter(|x| !x.is_null() && !is_undefined(x)).and_then(Value::as_f64);
    Ok(Some(HoverAt {
        pon: n("pon").unwrap_or(f64::NAN) as u32,
        point: from_value(v.get("point").unwrap_or(&Value::Null))?,
        scale: n("scale"),
        rotation: n("rotation"),
        zoom: n("zoom"),
    }))
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
        "create" => untyped(
            cap.create(pon(args, 0), present(args, 1).cloned().unwrap_or(Value::Null)),
            Clone::clone,
        ),
        "armStamp" => unit(cap.arm_stamp(decode(args, 0)?)),
        "disarmStamp" => {
            cap.disarm_stamp();
            Outcome::undefined()
        }
        "placeArmedStamp" => out(&cap.place_armed_stamp(pon(args, 0), point(1)?, opt_num(args, 2))),
        "requestStampAt" => out(&cap.request_stamp_at(pon(args, 0), point(1)?, opt_num(args, 2))),
        "placeAt" => out(&cap.place_at(pon(args, 0), point(1)?, opt_num(args, 2))),
        "hasArmedStamp" => out(&cap.has_armed_stamp()),
        "ghostHoverAt" => {
            cap.ghost_hover_at(&text(args, 0), pon(args, 1), point(2)?, opt_num(args, 3));
            Outcome::undefined()
        }
        "clearGhost" => {
            cap.clear_ghost();
            Outcome::undefined()
        }
        "setPlacementPreview" => {
            let bx: Rect = decode(args, 2)?;
            cap.set_placement_preview(&text(args, 0), pon(args, 1), bx);
            Outcome::undefined()
        }
        "clearPlacementPreview" => {
            cap.clear_placement_preview();
            Outcome::undefined()
        }
        "toolGhost" => out_opt(cap.tool_ghost(pon(args, 0))),
        "armedStampPreview" => {
            Outcome::Returned(cap.armed_stamp_preview().map_or(Value::Null, |p| {
                let mut o = Map::new();
                o.insert("bytes".into(), epdf_kernel::bytes::Bytes(p.bytes).to_json());
                if let Some(m) = p.mime_type {
                    o.insert("mimeType".into(), json!(m));
                }
                Value::Object(o)
            }))
        }
        "stampArmEpoch" => out(&cap.stamp_arm_epoch()),
        "setFilePickerProvider" => {
            let provider = present(args, 0).filter(|v| v.get("$fn").is_some()).map(|spec| {
                let callback = h.callback(spec);
                let provider: crate::FilePickerProvider =
                    Rc::new(move |req| answer_promise(callback.call(vec![to_value(&req)])));
                provider
            });
            cap.set_file_picker_provider(provider);
            Outcome::undefined()
        }
        "downloadAttachment" => untyped(cap.download_attachment(r(0)?), Clone::clone),
        "tools" => {
            Outcome::Returned(Value::Array(cap.tools().iter().map(|t| t.to_json()).collect()))
        }
        "tool" => Outcome::Returned(cap.tool(&text(args, 0)).map_or(Value::Null, |t| t.to_json())),
        "toolSubtype" => out(&cap.tool_subtype(&text(args, 0))),
        "registerTool" => unregister_ref(h, cap.register_tool(decode(args, 0)?)?),
        "update" => unit(cap.update(r(0)?, present(args, 1).cloned().unwrap_or(Value::Null))),
        "updateSelection" => {
            cap.update_selection(decode(args, 0)?);
            Outcome::undefined()
        }
        "updateSelectionFlags" => {
            cap.update_selection_flags(decode(args, 0)?);
            Outcome::undefined()
        }
        "getSelectionFlags" => {
            Outcome::Returned(cap.get_selection_flags().map_or(Value::Null, Value::Object))
        }
        "delete" => unit(cap.delete(r(0)?)),
        "canRead" => out(&cap.can_read()),
        "canCreate" => out(&cap.can_create()),
        "canEdit" => out(&cap.can_edit(&r(0)?)),
        "canDelete" => out(&cap.can_delete(&r(0)?)),
        "getSelection" => out(&cap.get_selection()),
        "get" => Outcome::Returned(cap.get(&r(0)?).unwrap_or(Value::Null)),
        "list" => out(&cap.list(pon(args, 0))),
        "getSelected" => out(&cap.get_selected()),
        "getSelectionProps" => out(&cap.get_selection_props()),
        "propsForTool" => out(&cap.props_for_tool(&text(args, 0))),
        "pageItems" => out(&cap.page_items(pon(args, 0), view_arg(args, 1)?)),
        "chrome" => {
            out(&cap.chrome(pon(args, 0), opt_num(args, 1), opt_num(args, 2), opt_num(args, 3)))
        }
        "selectionAnchor" => {
            out_opt(cap.selection_anchor(opt_num(args, 0), opt_num(args, 1), opt_num(args, 2)))
        }
        "creationDraftAnchor" => out_opt(cap.creation_draft_anchor()),
        "selection" => out(&cap.selection()),
        "hitKind" => out(&cap.hit_kind(
            pon(args, 0),
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
            opt_bool(args, 5),
        )),
        "claimsTouchAt" => out(&cap.claims_touch_at(
            pon(args, 0),
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
        )),
        "cursorAt" => out_opt(cap.cursor_at(
            pon(args, 0),
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
        )),
        "hoverAt" => {
            cap.hover_at(hover_arg(present(args, 0))?);
            Outcome::undefined()
        }
        "behaviorFor" => {
            let a = present(args, 0).cloned().unwrap_or(Value::Null);
            let subtype = a.get("subtype").and_then(Value::as_str).unwrap_or_default().to_string();
            let r: Option<AnnotationRef> = a.get("ref").and_then(|v| from_value(v).ok());
            match cap.behavior_for(&subtype, r.as_ref()) {
                Some(b) => Outcome::Returned(BEHAVIORS.with(|reg| behavior_json(&b, reg, h))),
                None => Outcome::Returned(Value::Null),
            }
        }
        "linkItemsOn" => out(&cap.link_items_on(pon(args, 0))),
        "appearanceEpoch" => out(&cap.appearance_epoch(pon(args, 0))),
        "bakeScale" => out(&cap.bake_scale(num(args, 0))?),
        "appearances" => {
            let signal = present(args, 2)
                .and_then(|s| s.get("$abortSignal"))
                .and_then(Value::as_str)
                .map(controller_named);
            Outcome::Promise(cap.appearances(pon(args, 0), num(args, 1), signal)?)
        }
        "toContentBox" => {
            let rect = crate::value::pdf_rect_of(present(args, 1));
            out_opt(cap.to_content_box(pon(args, 0), rect))
        }
        "editPointer" => {
            cap.edit_pointer(
                phase(args, 0)?,
                pon(args, 1),
                point(2)?,
                truthy_arg(args, 3),
                opt_num(args, 4),
                opt_num(args, 5),
                opt_num(args, 6),
                opt_bool(args, 7),
            );
            Outcome::undefined()
        }
        "marqueePointer" => {
            cap.marquee_pointer(
                phase(args, 0)?,
                pon(args, 1),
                point(2)?,
                truthy_arg(args, 3),
                opt_num(args, 4),
                opt_num(args, 5),
                opt_num(args, 6),
            );
            Outcome::undefined()
        }
        "createPointer" => {
            cap.create_pointer(
                &text(args, 0),
                phase(args, 1)?,
                pon(args, 2),
                point(3)?,
                truthy_arg(args, 4),
                opt_num(args, 5),
            );
            Outcome::undefined()
        }
        "finishCreationDraft" => {
            cap.finish_creation_draft();
            Outcome::undefined()
        }
        "finishInkDraft" => {
            cap.finish_ink_draft();
            Outcome::undefined()
        }
        "cancelCreationDraft" => {
            cap.cancel_creation_draft();
            Outcome::undefined()
        }
        "markupFromSelection" => {
            out(&cap.markup_from_selection(&text(args, 0), opt_text(args, 1).as_deref())?)
        }
        "createMarkup" => {
            let quads: Vec<TextQuad> = decode(args, 2)?;
            cap.create_markup(&text(args, 0), pon(args, 1), quads, opt_text(args, 3).as_deref());
            Outcome::undefined()
        }
        "createCaret" => {
            let anchor: TextEndAnchor = decode(args, 1)?;
            cap.create_caret(pon(args, 0), anchor);
            Outcome::undefined()
        }
        "createReplaceText" => {
            let quads: Vec<TextQuad> = decode(args, 1)?;
            let anchor: TextEndAnchor = decode(args, 2)?;
            cap.create_replace_text(pon(args, 0), quads, anchor, opt_text(args, 3).as_deref());
            Outcome::undefined()
        }
        "previewMarkup" => {
            cap.preview_markup(&text(args, 0), decode(args, 1)?, opt_text(args, 2).as_deref());
            Outcome::undefined()
        }
        "clearMarkupPreview" => {
            cap.clear_markup_preview();
            Outcome::undefined()
        }
        "setDefaults" => {
            cap.set_defaults(&text(args, 0), decode(args, 1)?);
            Outcome::undefined()
        }
        "currentDefaults" => out(&cap.current_defaults(&text(args, 0))),
        "setSnap" => {
            cap.set_snap(decode(args, 0)?);
            Outcome::undefined()
        }
        "snapSettings" => out(&cap.snap_settings()),
        "setChrome" => {
            cap.set_chrome(decode(args, 0)?);
            Outcome::undefined()
        }
        "chromeSettings" => out(&cap.chrome_settings()),
        "deleteSelection" => {
            cap.delete_selection();
            Outcome::undefined()
        }
        "deselect" => {
            cap.deselect();
            Outcome::undefined()
        }
        "select" => {
            let add = present(args, 1)
                .and_then(|o| o.get("add"))
                .filter(|v| !is_undefined(v))
                .map(|v| crate::value::truthy(Some(v)));
            cap.select(&r(0)?, add);
            Outcome::undefined()
        }
        "pruneEngagedSelection" => {
            cap.prune_engaged_selection();
            Outcome::undefined()
        }
        "cancel" => {
            cap.cancel();
            Outcome::undefined()
        }
        "rotateSelection90" => {
            cap.rotate_selection_90();
            Outcome::undefined()
        }
        "resetSelectionRotation" => {
            cap.reset_selection_rotation();
            Outcome::undefined()
        }
        "group" => unit(cap.group()),
        "ungroup" => unit(cap.ungroup()),
        "canGroup" => out(&cap.can_group()),
        "canUngroup" => out(&cap.can_ungroup()),
        "ensurePage" => {
            cap.ensure_page(pon(args, 0));
            Outcome::undefined()
        }
        "reloadPage" => unit(cap.reload_page(pon(args, 0))),
        "hydration" => out(&cap.hydration()),
        "ensureHydrated" => {
            cap.ensure_hydrated();
            Outcome::undefined()
        }
        "rehydrate" => unit(cap.rehydrate()),
        "deliverRemoteAnnotationEvent" => {
            cap.deliver_remote_annotation_event(present(args, 0).cloned().unwrap_or(Value::Null));
            Outcome::undefined()
        }
        "textItems" => out(&cap.text_items(pon(args, 0), view_arg(args, 1)?)),
        "currentEditing" => out(&cap.current_editing()),
        "beginTextEdit" => {
            cap.begin_text_edit(&r(0)?);
            Outcome::undefined()
        }
        "beginTextEditAt" => out(&cap.begin_text_edit_at(
            pon(args, 0),
            point(1)?,
            opt_num(args, 2),
            opt_num(args, 3),
            opt_num(args, 4),
        )),
        "setContents" => {
            cap.set_contents(r(0)?, text(args, 1));
            Outcome::undefined()
        }
        "endTextEdit" => {
            cap.end_text_edit();
            Outcome::undefined()
        }
        "commitScriptEffects" => untyped(cap.commit_script_effects(decode(args, 0)?), to_value),
        "registerBehavior" => {
            let spec = present(args, 0).cloned().unwrap_or(Value::Null);
            let scripted = Rc::new(scripted_behavior(&spec, h));
            let behavior: Rc<dyn Behavior> = scripted.clone();
            BEHAVIORS.with(|reg| reg.borrow_mut().push((behavior.clone(), scripted)));
            unregister_ref(h, cap.register_behavior(behavior))
        }
        other => not_a_function(other),
    })
}

fn call_lens(lens: &Lens, method: &str, args: &[Value]) -> Result<Outcome, Error> {
    let cap = lens.ctx.get(&ANNOTATION)?;
    let r = |i: usize| -> Result<AnnotationRef, Error> { decode(args, i) };
    Ok(match method {
        "threads" => Outcome::Returned(Value::Array(
            cap.comment_threads().iter().map(|t| t.to_json()).collect(),
        )),
        "thread" => {
            Outcome::Returned(cap.comment_thread(&r(0)?).map_or(Value::Null, |t| t.to_json()))
        }
        "hydration" => out(&cap.hydration()),
        "rehydrate" => unit(cap.rehydrate()),
        "reply" => untyped(cap.comment_reply(r(0)?, text(args, 1)), Clone::clone),
        "edit" => unit(cap.comment_edit(r(0)?, text(args, 1))),
        "setStatus" => unit(cap.comment_set_status(r(0)?, text(args, 1))),
        "setMarked" => unit(cap.comment_set_marked(r(0)?, truthy_arg(args, 1))),
        "remove" => unit(cap.comment_remove(present(args, 0).cloned().unwrap_or(Value::Null))),
        "removeThread" => untyped(cap.comment_remove_thread(r(0)?), |t| t.to_json()),
        "permissionsFor" => out(&cap.comment_permissions_for(&r(0)?)),
        "linksOf" => out_opt(cap.links_of(&r(0)?)),
        "linksSet" => {
            let target: PdfLinkTarget = decode(args, 1)?;
            unit(cap.links_set(&r(0)?, target))
        }
        "linksClear" => unit(cap.links_clear(&r(0)?)),
        other => not_a_function(other),
    })
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
        };
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
            return Some(from_result(call_lens(lens, method, &args)));
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
