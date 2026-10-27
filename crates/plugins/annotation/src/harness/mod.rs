//! The conformance adapter: how `crates/plugin-conformance` builds these
//! plugins from a vector and calls their capabilities by name, plus the
//! function tables of the pure suites.
//!
//! It mirrors `tooling/plugin-conformance/src/suites/annotation-live-*.ts`:
//! `annotation-live` builds the plugin and logs its `console.error` lines
//! (the ones starting `[annotation]`) as the callback `console.error`;
//! `annotation-live-actions`, `annotation-live-render` and
//! `annotation-live-selection` build the sibling plugins the suites pair it
//! with. A function a result carries (a picture's `blob()` and
//! `objectUrl()`, an anchor's `boundsIn`) prints as a ref, as the
//! TypeScript's canonical form registers one.

pub mod functions;

use std::any::Any;
use std::rc::Rc;

use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::{Authority, Effect, Model, Painter, Point, ViewEnv};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe};
use plugin_conformance::{Answer, Callback, Harness, HarnessPlugin, Outcome};
use serde_json::{Value, json};

pub use functions::{pure_functions, repository_functions};

use crate::host_contract::{AnnotationLender, Behavior, StampPreviewProvider};
use crate::js::Object;
use crate::read::annotations::AnnotationFilter;
use crate::read::chrome::HitView;
use crate::services::lending::LentRecords;
use crate::write::crud::CreateOptions;
use crate::write::selection::SelectionPatch;
use crate::write::stamps::{StampInput, StampPreview};
use crate::{ANNOTATION, AnnotationCapability, annotation_plugin};

/// The adapter.
pub struct Annotation;

/// A noun of the capability (`selection`, `draft`, `text`, `tools`,
/// `stamps`, `links`, `comments`), resolved for a dotted call.
pub struct Noun {
    pub capability: AnnotationCapability,
    pub name: &'static str,
}

const NOUNS: [&str; 7] = ["links", "selection", "draft", "text", "tools", "stamps", "comments"];

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !epdf_kernel::is_undefined(v))
}

fn value(args: &[Value], i: usize) -> Value {
    arg(args, i).cloned().unwrap_or(Value::Null)
}

fn num(args: &[Value], i: usize) -> Option<f64> {
    arg(args, i).and_then(epdf_annotation_core::json::number)
}

fn text(args: &[Value], i: usize) -> String {
    arg(args, i).and_then(Value::as_str).unwrap_or("undefined").to_string()
}

fn point(args: &[Value], i: usize) -> Point {
    arg(args, i)
        .map(epdf_annotation_core::json::point_loose)
        .unwrap_or(Point { x: f64::NAN, y: f64::NAN })
}

fn object(args: &[Value], i: usize) -> Object {
    arg(args, i).and_then(Value::as_object).cloned().unwrap_or_default()
}

fn refs(args: &[Value], i: usize) -> Vec<Value> {
    arg(args, i).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn opt_value(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

fn undefined_outcome() -> Outcome {
    Outcome::undefined()
}

fn unit<T>(result: Result<T, Error>) -> Outcome {
    match result {
        Ok(_) => Outcome::undefined(),
        Err(e) => Outcome::Threw(e),
    }
}

fn promise(p: Promise<Value>) -> Outcome {
    Outcome::Promise(p)
}

fn promise_unit(p: Promise<()>) -> Outcome {
    Outcome::promise_unit(p)
}

/// An unsubscribe, handed back as a ref.
fn remover(h: &Harness, off: Unsubscribe) -> Value {
    h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    }))
}

/// Subscribe a scripted listener to an event; the outcome is the unsubscribe.
fn subscribe(
    h: &Harness,
    hook: &epdf_kernel::event_hook::EventHookSource<Value>,
    spec: &Value,
) -> Outcome {
    let callback = h.callback(spec);
    let off = hook.on(move |event| match callback.call(vec![event.clone()]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    });
    Outcome::Returned(remover(h, off))
}

/// A view env (`{ zoom, rotation }`), or none.
fn view_env(args: &[Value], i: usize) -> Option<ViewEnv> {
    let v = arg(args, i)?;
    if !v.is_object() {
        return None;
    }
    ViewEnv::read(v).ok().or_else(|| {
        Some(ViewEnv::new(
            v.get("zoom").and_then(Value::as_f64).unwrap_or(1.0),
            v.get("rotation").and_then(Value::as_f64).unwrap_or(0.0),
        ))
    })
}

fn hit_view(v: &Value) -> HitView {
    let n = |k: &str| {
        v.get(k)
            .filter(|x| !epdf_kernel::is_undefined(x))
            .and_then(epdf_annotation_core::json::number)
    };
    HitView { scale: n("scale"), rotation: n("rotation"), zoom: n("zoom") }
}

/// A callback's answer as a promise of a JSON value.
fn answer_promise(answer: Answer) -> Promise<Value> {
    match answer {
        Answer::Returns(v) => Promise::resolved(v),
        Answer::Throws(e) => Promise::rejected(e),
        Answer::Promise(p) => p,
    }
}

/// A callback answering synchronously.
fn answer_value(answer: Answer) -> Value {
    match answer {
        Answer::Returns(v) => v,
        _ => Value::Null,
    }
}

/// A function, printed as a ref that answers `undefined`.
fn function_ref(h: &Harness) -> Value {
    h.register_fn(Rc::new(|_| Outcome::undefined()))
}

/// An `ImageSource`: its `blob()` and `objectUrl()` as refs.
fn image_source(h: &Harness, image: &Value) -> Value {
    match image {
        Value::Object(members) if members.contains_key("$imageBytes") => {
            json!({ "blob": function_ref(h), "objectUrl": function_ref(h) })
        }
        Value::Object(members) => {
            let mut out = members.clone();
            out.insert("blob".into(), function_ref(h));
            out.insert("objectUrl".into(), function_ref(h));
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Pictures with their images as `ImageSource`s.
fn pictures(h: &Harness, list: Vec<Value>) -> Value {
    Value::Array(
        list.into_iter()
            .map(|mut picture| {
                if let Some(image) = picture.get("image").cloned() {
                    picture["image"] = image_source(h, &image);
                }
                picture
            })
            .collect(),
    )
}

/// A scripted lender (`AnnotationLender` whose members are `$fn` specs).
struct ScriptedLender {
    owner: String,
    records: Callback,
    ops_of: Callback,
    undo_of: Callback,
    reorder_ops: Callback,
    authority: Callback,
    picture_epoch: Callback,
    picture_version: Callback,
    pending: Callback,
    painter: Callback,
    render_appearances: Callback,
}

fn list_of(value: Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

impl AnnotationLender for ScriptedLender {
    fn owner(&self) -> String {
        self.owner.clone()
    }
    fn records(&self) -> Option<LentRecords> {
        let records = answer_value(self.records.call(Vec::new()));
        if records.is_null() {
            return None;
        }
        // Every answer is fresh JSON: new rows, as the TypeScript fixture's are.
        let rows =
            |v: &Value| Rc::new(list_of(v.clone()).into_iter().map(Rc::new).collect::<Vec<_>>());
        Some(LentRecords { confirmed: rows(&records["confirmed"]), shown: rows(&records["shown"]) })
    }
    fn ops_of(&self, effects: &[Effect], model: &Model, before: &Model) -> Vec<Value> {
        let effects: Vec<Value> = effects.iter().map(Json::json).collect();
        let _ = (model, before);
        list_of(answer_value(self.ops_of.call(vec![json!(effects)])))
    }
    fn undo_of(&self, ops: &[Value]) -> Vec<Value> {
        list_of(answer_value(self.undo_of.call(vec![json!(ops)])))
    }
    fn reorder_ops(&self, page: &Value, refs: &[Value], position: &Value) -> Vec<Value> {
        list_of(answer_value(self.reorder_ops.call(vec![
            page.clone(),
            json!(refs),
            position.clone(),
        ])))
    }
    fn authority(&self, record: &Object) -> Authority {
        let answer = answer_value(self.authority.call(vec![Value::Object(record.clone())]));
        Authority {
            update: answer.get("update").and_then(Value::as_bool).unwrap_or(false),
            delete: answer.get("delete").and_then(Value::as_bool).unwrap_or(false),
            order: Default::default(),
        }
    }
    fn picture_epoch(&self, page: &Value) -> String {
        answer_value(self.picture_epoch.call(vec![page.clone()]))
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
    fn picture_version(&self, reference: &Value) -> f64 {
        answer_value(self.picture_version.call(vec![reference.clone()])).as_f64().unwrap_or(0.0)
    }
    fn pending(&self, reference: &Value) -> bool {
        answer_value(self.pending.call(vec![reference.clone()])).as_bool().unwrap_or(false)
    }
    fn painter(&self, record: &Rc<Object>) -> Painter {
        let _ = answer_value(self.painter.call(vec![Value::Object((**record).clone())]));
        Painter(None)
    }
    fn render_appearances(
        &self,
        page: &Value,
        scale: f64,
        _signal: Option<AbortSignal>,
    ) -> Promise<Vec<Value>> {
        let answer = answer_promise(self.render_appearances.call(vec![page.clone(), json!(scale)]));
        answer.map(|v| Ok(list_of(v)))
    }
}

/// A scripted behavior.
struct ScriptedBehavior {
    id: String,
    matches: Callback,
    engaged: Callback,
}

impl Behavior for ScriptedBehavior {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn matches(&self, annotation: &Object) -> bool {
        epdf_annotation_core::json::truthy(Some(&answer_value(
            self.matches.call(vec![Value::Object(annotation.clone())]),
        )))
    }
    fn engaged(&self, annotation: &Object) -> bool {
        epdf_annotation_core::json::truthy(Some(&answer_value(
            self.engaged.call(vec![Value::Object(annotation.clone())]),
        )))
    }
}

fn member_callback(h: &Harness, spec: &Value, key: &str) -> Callback {
    h.callback(spec.get(key).unwrap_or(&json!({ "$fn": key })))
}

fn stamp_input(h: &Harness, value: &Value) -> StampInput {
    let mut input = StampInput::read(value);
    if let Some(preview) = value.get("preview").filter(|p| p.get("$fn").is_some()) {
        let callback = h.callback(preview);
        let provider: StampPreviewProvider = Rc::new(move |width| {
            let answer = answer_promise(callback.call(vec![json!(width)]));
            answer.map(|v| Ok(if v.is_null() { None } else { Some(v) }))
        });
        input.preview = Some(StampPreview::Provider(provider));
    }
    input
}

/// The composed api's own members, in `composeApi`'s order (the slices,
/// then the nouns and the services).
const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "get",
    "list",
    "isPending",
    "getHovered",
    "isEditable",
    "listChromeNodes",
    "getSelectionAnchorIn",
    "getAnnotationAnchor",
    "getHitKind",
    "claimsTouchAt",
    "getCursorAt",
    "listPageItems",
    "listTextItems",
    "listLinkItems",
    "getAppearanceEpoch",
    "renderAppearances",
    "listResolvedTools",
    "getResolvedTool",
    "getToolSubtype",
    "registerBehavior",
    "getBehaviorFor",
    "pruneEngagedSelection",
    "beginTextEditAt",
    "getEditingId",
    "draftContents",
    "draftRichText",
    "setTextSelection",
    "getCssFontFamily",
    "links",
    "create",
    "update",
    "delete",
    "reorder",
    "export",
    "import",
    "downloadResource",
    "placeArmedStamp",
    "requestStampAt",
    "getArmedStamp",
    "renderArmedStampPreview",
    "hoverGhostAt",
    "clearGhost",
    "previewPlacement",
    "clearPlacementPreview",
    "getImageGhost",
    "placeAt",
    "setFilePickerProvider",
    "setPageViewports",
    "remeasurePage",
    "editPointer",
    "marqueePointer",
    "createPointer",
    "hoverAt",
    "finishInkDraft",
    "cancel",
    "distanceCreationPage",
    "onDraftCaptured",
    "createMarkup",
    "createCaret",
    "createReplaceText",
    "applyToolToSelection",
    "previewMarkup",
    "clearMarkupPreview",
    "createFromSelection",
    "commitScriptEffects",
    "comments",
    "getAt",
    "selection",
    "draft",
    "text",
    "tools",
    "stamps",
    "lend",
    "getStatus",
    "refresh",
    "whenSynced",
    "onResynced",
    "canRead",
    "canCreate",
    "canUpdate",
    "canDelete",
    "onCreated",
    "onUpdated",
    "onDeleted",
    "onReordered",
    "onSelectionChanged",
    "onDraftChanged",
    "onEditingChanged",
    "onHoverChanged",
    "onWriteFailed",
];

impl HarnessPlugin for Annotation {
    fn factories(&self) -> Vec<&'static str> {
        vec![
            "annotation-live",
            "annotation-live-actions",
            "annotation-live-render",
            "annotation-live-selection",
        ]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        match factory {
            "annotation-live" => {
                let log = h.clone();
                crate::console::set_sink(Some(Rc::new(move |level, args| {
                    let first = args.first().and_then(Value::as_str).unwrap_or("");
                    if level == "error" && first.starts_with("[annotation]") {
                        log.log("console.error", args);
                    } else if level == "warn" {
                        log.log("console.warn", args);
                    }
                })));
                Ok(annotation_plugin(
                    Some(config.clone()).filter(|c| !epdf_kernel::is_undefined(c)),
                ))
            }
            "annotation-live-actions" => {
                Ok(epdf_plugin_actions::actions_plugin(epdf_plugin_actions::ActionsConfig {
                    open_sequence: Some(epdf_plugin_actions::OpenSequence::Off),
                    javascript: Some(epdf_plugin_actions::JavaScriptConfig {
                        enabled: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                }))
            }
            "annotation-live-render" => Ok(epdf_plugin_render::render_plugin(Default::default())),
            "annotation-live-selection" => {
                Ok(epdf_plugin_selection::selection_plugin(Default::default()))
            }
            other => Err(Error::new(format!("[annotation] no factory {other}"))),
        }
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![ANNOTATION.any().clone()]
    }

    fn resolve_member(
        &self,
        capability: &Rc<dyn Any>,
        member: &str,
        _h: &Harness,
    ) -> Option<Result<Rc<dyn Any>, Error>> {
        let cap = capability.downcast_ref::<AnnotationCapability>()?;
        let name = NOUNS.iter().find(|n| **n == member)?;
        Some(Ok(Rc::new(Noun { capability: cap.clone(), name })))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<AnnotationCapability>().is_some() {
            return Some(MEMBERS.to_vec());
        }
        None
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(noun) = capability.downcast_ref::<Noun>() {
            return Some(call_noun(&noun.capability, noun.name, method, &args, h));
        }
        let a = capability.downcast_ref::<AnnotationCapability>()?;
        Some(call_capability(a, method, &args, h))
    }
}

fn call_noun(
    a: &AnnotationCapability,
    noun: &str,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Outcome {
    match (noun, method) {
        ("selection", "set") => {
            a.selection_set(&refs(args, 0));
            undefined_outcome()
        }
        ("selection", "add") => {
            a.selection_add(&refs(args, 0));
            undefined_outcome()
        }
        ("selection", "selectAll") => {
            a.selection_select_all(arg(args, 0));
            undefined_outcome()
        }
        ("selection", "selectInRect") => {
            let rect = arg(args, 1).map(epdf_annotation_core::json::rect_loose).unwrap_or_default();
            unit(a.selection_select_in_rect(&value(args, 0), rect))
        }
        ("selection", "clear") => {
            a.selection_clear();
            undefined_outcome()
        }
        ("selection", "list") => Outcome::Returned(json!(*a.list_selected())),
        ("selection", "update") => {
            let patch = match arg(args, 0) {
                Some(spec) if spec.get("$fn").is_some() => {
                    let callback = h.callback(spec);
                    SelectionPatch::PerMember(Rc::new(move |annotation: &Value| {
                        answer_value(callback.call(vec![annotation.clone()]))
                            .as_object()
                            .cloned()
                            .unwrap_or_default()
                    }))
                }
                _ => SelectionPatch::Fields(object(args, 0)),
            };
            promise(a.selection_update(patch, None))
        }
        ("selection", "updateLink") => promise(a.selection_update_link(value(args, 0), None)),
        ("selection", "getProperties") => Outcome::Returned(a.selection_properties()),
        ("selection", "delete") => promise(a.selection_delete(None)),
        ("selection", "rotateBy") => {
            promise_unit(a.selection_rotate_by(num(args, 0).unwrap_or(f64::NAN), None))
        }
        ("selection", "resetRotation") => promise_unit(a.selection_reset_rotation(None)),
        ("selection", "group") => promise_unit(a.selection_group(None)),
        ("selection", "ungroup") => promise_unit(a.selection_ungroup(None)),
        ("selection", "canGroup") => Outcome::Returned(json!(a.selection_can_group())),
        ("selection", "canUngroup") => Outcome::Returned(json!(a.selection_can_ungroup())),
        ("selection", "getAnchor") => Outcome::Returned(a.selection_get_anchor()),
        ("selection", "getRotationAnchor") => Outcome::Returned(a.rotation_anchor()),
        ("draft", "get") => Outcome::Returned(a.draft_get()),
        ("draft", "finish") => promise(a.draft_finish(None)),
        ("draft", "cancel") => {
            a.cancel();
            undefined_outcome()
        }
        ("text", "begin") => {
            a.text_begin(&value(args, 0));
            undefined_outcome()
        }
        ("text", "end") => promise_unit(a.text_end(None)),
        ("text", "getEditing") => Outcome::Returned(opt_value(a.get_editing())),
        ("text", "toggleFormat") => promise_unit(a.toggle_format(&text(args, 0), None)),
        ("tools", "list") => Outcome::Returned(json!(a.tools_list())),
        ("tools", "get") => Outcome::Returned(a.tools_get(&text(args, 0))),
        ("tools", "register") => match a.tools_register(value(args, 0)) {
            Ok(off) => Outcome::Returned(remover(h, off)),
            Err(e) => Outcome::Threw(e),
        },
        ("tools", "getDefaults") => Outcome::Returned(a.tools_get_defaults(&text(args, 0))),
        ("tools", "updateDefaults") => {
            unit(a.tools_update_defaults(&text(args, 0), &object(args, 1)))
        }
        ("tools", "getProperties") => Outcome::Returned(a.tools_get_properties(&text(args, 0))),
        ("tools", "onDefaultsChanged") => {
            subscribe(h, &a.events.tool_defaults_changed, &value(args, 0))
        }
        ("stamps", "arm") => promise_unit(a.stamps_arm(stamp_input(h, &value(args, 0)), None)),
        ("stamps", "disarm") => {
            a.stamps_disarm();
            undefined_outcome()
        }
        ("stamps", "isArmed") => Outcome::Returned(json!(a.stamps_is_armed())),
        ("stamps", "place") => {
            promise(a.stamps_place(stamp_input(h, &value(args, 0)), &value(args, 1), None))
        }
        ("links", "get") => Outcome::Returned(a.links_get(&value(args, 0))),
        ("links", "set") => promise_unit(a.links_set(&value(args, 0), value(args, 1), None)),
        ("links", "clear") => promise_unit(a.links_clear(&value(args, 0), None)),
        ("comments", "listThreads") => Outcome::Returned(a.comments_list_threads()),
        ("comments", "getThread") => Outcome::Returned(a.comments_get_thread(&value(args, 0))),
        ("comments", "onThreadChanged") => subscribe(h, &a.events.thread_changed, &value(args, 0)),
        ("comments", "reply") => promise(a.comments_reply(&value(args, 0), &text(args, 1), None)),
        ("comments", "setText") => {
            promise_unit(a.comments_set_text(&value(args, 0), &text(args, 1), None))
        }
        ("comments", "setStatus") => {
            promise_unit(a.comments_set_status(&value(args, 0), &text(args, 1), None))
        }
        ("comments", "setMarked") => {
            let marked = arg(args, 1).is_some_and(|v| epdf_annotation_core::json::truthy(Some(v)));
            promise_unit(a.comments_set_marked(&value(args, 0), marked, None))
        }
        ("comments", "delete") => promise_unit(a.comments_delete(&value(args, 0), None)),
        ("comments", "deleteThread") => promise(a.comments_delete_thread(&value(args, 0), None)),
        ("comments", "canReply") | ("comments", "canSetStatus") | ("comments", "canSetMarked") => {
            Outcome::Returned(json!(a.can_create()))
        }
        ("comments", "canSetText") => {
            Outcome::Returned(json!(a.comments_can_set_text(&value(args, 0))))
        }
        ("comments", "canDelete") => Outcome::Returned(json!(a.can_delete(&value(args, 0)))),
        ("comments", "canDeleteThread") => {
            Outcome::Returned(json!(a.comments_can_delete_thread(&value(args, 0))))
        }
        (noun, other) => {
            Outcome::Threw(Error::new(format!("[scenario] \"{noun}.{other}\" is not a function")))
        }
    }
}

fn call_capability(a: &AnnotationCapability, method: &str, args: &[Value], h: &Harness) -> Outcome {
    match method {
        // ── settings ──
        "getSettings" => Outcome::Returned((*a.settings.get()).clone()),
        "updateSettings" => {
            a.settings.store.update_settings(&value(args, 0));
            undefined_outcome()
        }
        "resetSettings" => {
            a.settings.store.reset_settings();
            undefined_outcome()
        }
        // ── reads ──
        "get" => Outcome::Returned(opt_value(a.get(&value(args, 0)))),
        "list" => {
            let filter = arg(args, 0).map(AnnotationFilter::read);
            Outcome::Returned(json!(*a.list(filter.as_ref())))
        }
        "isPending" => Outcome::Returned(json!(a.is_pending(&value(args, 0)))),
        "getHovered" => Outcome::Returned(opt_value(a.get_hovered())),
        "isEditable" => Outcome::Returned(json!(a.is_editable(&value(args, 0)))),
        "getAt" => Outcome::Returned(opt_value(a.annotation_at(&value(args, 0), point(args, 1)))),
        "getStatus" => Outcome::Returned(json!(a.records.get_status().as_str())),
        "refresh" => promise_unit(a.cancellable(None, a.records.refresh())),
        "whenSynced" => promise_unit(a.records.settled()),
        "canRead" => Outcome::Returned(json!(a.can_read())),
        "canCreate" => Outcome::Returned(json!(a.can_create())),
        "canUpdate" => Outcome::Returned(json!(a.can_update(&value(args, 0)))),
        "canDelete" => Outcome::Returned(json!(a.can_delete(&value(args, 0)))),
        // ── chrome ──
        "listChromeNodes" => Outcome::Returned(a.list_chrome_nodes(
            &value(args, 0),
            num(args, 1),
            num(args, 2),
            num(args, 3),
        )),
        "getSelectionAnchorIn" => {
            let view = hit_view(&value(args, 0));
            Outcome::Returned(a.selection_anchor_of(view.scale, view.rotation, view.zoom))
        }
        "getAnnotationAnchor" => {
            let anchor = a.get_annotation_anchor(&value(args, 0), view_env(args, 1));
            Outcome::Returned(match anchor {
                Value::Object(mut members) if members.contains_key("boundsIn") => {
                    members.insert("boundsIn".into(), function_ref(h));
                    Value::Object(members)
                }
                other => other,
            })
        }
        "getHitKind" => {
            let view = HitView { scale: num(args, 2), rotation: num(args, 3), zoom: num(args, 4) };
            let touch = arg(args, 5).map(|v| epdf_annotation_core::json::truthy(Some(v)));
            Outcome::Returned(json!(a.get_hit_kind(&value(args, 0), point(args, 1), view, touch)))
        }
        "claimsTouchAt" => {
            let view = HitView { scale: num(args, 2), rotation: num(args, 3), zoom: num(args, 4) };
            Outcome::Returned(json!(a.claims_touch_at(&value(args, 0), point(args, 1), view)))
        }
        "getCursorAt" => {
            let view = HitView { scale: num(args, 2), rotation: num(args, 3), zoom: num(args, 4) };
            Outcome::Returned(a.get_cursor_at(&value(args, 0), point(args, 1), view))
        }
        // ── render ──
        "listPageItems" => {
            Outcome::Returned(lent_scenes(h, a.list_page_items(&value(args, 0), view_env(args, 1))))
        }
        "listTextItems" => Outcome::Returned(a.list_text_items(&value(args, 0), view_env(args, 1))),
        "listLinkItems" => Outcome::Returned(a.list_link_items(&value(args, 0))),
        "getAppearanceEpoch" => Outcome::Returned(json!(a.get_appearance_epoch(&value(args, 0)))),
        "renderAppearances" => {
            let h = h.clone();
            let p = a.render_appearances(&value(args, 0), num(args, 1).unwrap_or(f64::NAN), None);
            promise(p.map(move |list| Ok(pictures(&h, list))))
        }
        // ── tools and behaviors ──
        "listResolvedTools" => Outcome::Returned(json!(
            a.list_resolved_tools().iter().map(|t| t.to_json()).collect::<Vec<_>>()
        )),
        "getResolvedTool" => Outcome::Returned(
            a.get_resolved_tool(&text(args, 0)).map_or(Value::Null, |t| t.to_json()),
        ),
        "getToolSubtype" => Outcome::Returned(json!(a.get_tool_subtype(&text(args, 0)))),
        "registerBehavior" => {
            let spec = value(args, 0);
            let behavior = Rc::new(ScriptedBehavior {
                id: spec.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                matches: member_callback(h, &spec, "matches"),
                engaged: member_callback(h, &spec, "engaged"),
            });
            Outcome::Returned(remover(h, a.register_behavior(behavior)))
        }
        "getBehaviorFor" => {
            // The behavior object as its registrant built it: `id`, then
            // `matches` and `engaged` as callable refs, registered in that
            // order (link.json's `link-nav-behavior` calls both).
            let behavior = a.get_behavior_for(&object(args, 0));
            Outcome::Returned(behavior.map_or(Value::Null, |b| {
                let id = b.id();
                let held = b.clone();
                let matches = h.register_fn(Rc::new(move |args: Vec<Value>| {
                    Outcome::Returned(json!(held.matches(&object(&args, 0))))
                }));
                let engaged = h.register_fn(Rc::new(move |args: Vec<Value>| {
                    Outcome::Returned(json!(b.engaged(&object(&args, 0))))
                }));
                json!({ "id": id, "matches": matches, "engaged": engaged })
            }))
        }
        "pruneEngagedSelection" => {
            a.prune_engaged_selection();
            undefined_outcome()
        }
        "lend" => {
            let spec = value(args, 0);
            let lender = Rc::new(ScriptedLender {
                owner: spec.get("owner").and_then(Value::as_str).unwrap_or_default().to_string(),
                records: member_callback(h, &spec, "records"),
                ops_of: member_callback(h, &spec, "opsOf"),
                undo_of: member_callback(h, &spec, "undoOf"),
                reorder_ops: member_callback(h, &spec, "reorderOps"),
                authority: member_callback(h, &spec, "authority"),
                picture_epoch: member_callback(h, &spec, "pictureEpoch"),
                picture_version: member_callback(h, &spec, "pictureVersion"),
                pending: member_callback(h, &spec, "pending"),
                painter: member_callback(h, &spec, "painter"),
                render_appearances: member_callback(h, &spec, "renderAppearances"),
            });
            let cx = a.cx.clone();
            let off = a.lending.lend(lender, Rc::new(move || cx.notify()));
            Outcome::Returned(remover(h, off))
        }
        // ── text editing ──
        "beginTextEditAt" => {
            let view = HitView { scale: num(args, 2), rotation: num(args, 3), zoom: num(args, 4) };
            Outcome::Returned(json!(a.begin_text_edit_at(&value(args, 0), point(args, 1), view)))
        }
        "getEditingId" => Outcome::Returned(a.get_editing_id().map_or(Value::Null, Value::String)),
        "draftContents" => unit(a.draft_contents(&value(args, 0), &text(args, 1))),
        "draftRichText" => unit(a.draft_rich_text(
            &value(args, 0),
            value(args, 1).get("paragraphs").cloned().unwrap_or(json!([])),
        )),
        "setTextSelection" => {
            let range = arg(args, 1).filter(|r| !r.is_null()).map(|r| {
                (
                    r.get("start").and_then(Value::as_f64).unwrap_or(f64::NAN),
                    r.get("end").and_then(Value::as_f64).unwrap_or(f64::NAN),
                )
            });
            a.set_text_selection(&value(args, 0), range);
            undefined_outcome()
        }
        "getCssFontFamily" => Outcome::Returned(json!(a.get_css_font_family(&text(args, 0)))),
        // ── crud ──
        "create" => {
            let options = value(args, 3);
            let create = CreateOptions {
                signal: None,
                tool: options.get("tool").and_then(Value::as_str).map(str::to_string),
                select: options
                    .get("select")
                    .is_some_and(|v| epdf_annotation_core::json::truthy(Some(v))),
            };
            let resources = arg(args, 2).filter(|r| !r.is_null()).cloned();
            promise(a.create(&value(args, 0), &object(args, 1), resources, create))
        }
        "update" => {
            let resources = arg(args, 2).filter(|r| !r.is_null()).cloned();
            promise(a.update(&value(args, 0), &object(args, 1), resources, None))
        }
        "delete" => promise_unit(a.delete(&value(args, 0), None)),
        "reorder" => promise_unit(a.reorder(&refs(args, 0), &value(args, 1), None)),
        "export" => promise(a.export(&value(args, 0), None)),
        "import" => {
            promise(a.import(value(args, 0), arg(args, 1).cloned().unwrap_or(json!({})), None))
        }
        "downloadResource" => promise(a.download_resource(&value(args, 0), &text(args, 1), None)),
        // ── stamps and placement ──
        "placeArmedStamp" => Outcome::Returned(json!(a.place_armed_stamp(
            crate::js::page_object_number(&value(args, 0)),
            point(args, 1),
            num(args, 2)
        ))),
        "requestStampAt" => Outcome::Returned(json!(a.request_stamp_at(
            crate::js::page_object_number(&value(args, 0)),
            point(args, 1),
            num(args, 2)
        ))),
        "getArmedStamp" => Outcome::Returned(a.get_armed_stamp()),
        "renderArmedStampPreview" => promise(
            a.render_armed_stamp_preview(num(args, 0)).map(|v| Ok(v.unwrap_or(Value::Null))),
        ),
        "hoverGhostAt" => {
            a.hover_ghost_at(
                &text(args, 0),
                &value(args, 1),
                point(args, 2),
                num(args, 3),
                num(args, 4),
            );
            undefined_outcome()
        }
        "clearGhost" => {
            a.clear_ghost();
            undefined_outcome()
        }
        "previewPlacement" => {
            a.preview_placement(&text(args, 0), &value(args, 1), point(args, 2), point(args, 3));
            undefined_outcome()
        }
        "clearPlacementPreview" => {
            a.clear_placement_preview();
            undefined_outcome()
        }
        "getImageGhost" => Outcome::Returned(a.get_image_ghost(&value(args, 0))),
        "placeAt" => Outcome::Returned(json!(a.place_at(
            crate::js::page_object_number(&value(args, 0)),
            point(args, 1),
            num(args, 2),
            num(args, 3)
        ))),
        "setFilePickerProvider" => {
            let provider = arg(args, 0).filter(|p| !p.is_null()).map(|spec| {
                let callback = h.callback(spec);
                let provider: crate::host_contract::FilePickerProvider =
                    Rc::new(move |request: Value| {
                        answer_promise(callback.call(vec![request]))
                            .map(|v| Ok(if v.is_null() { None } else { Some(v) }))
                    });
                provider
            });
            Outcome::Returned(remover(h, a.set_file_picker_provider(provider)))
        }
        // ── measurement ──
        "setPageViewports" => {
            let viewports = arg(args, 1).and_then(Value::as_array).cloned();
            a.set_page_viewports(&value(args, 0), viewports, value(args, 2));
            undefined_outcome()
        }
        "remeasurePage" => promise(a.remeasure_page(&value(args, 0), value(args, 1))),
        "onDraftCaptured" => subscribe(h, &a.events.draft_captured, &value(args, 0)),
        "distanceCreationPage" => Outcome::Returned(opt_value(a.distance_creation_page())),
        // ── pointer ──
        "editPointer" => {
            let touch = arg(args, 7).map(|v| epdf_annotation_core::json::truthy(Some(v)));
            let shift = arg(args, 3).is_some_and(|v| epdf_annotation_core::json::truthy(Some(v)));
            unit(a.edit_pointer(
                &text(args, 0),
                &value(args, 1),
                point(args, 2),
                shift,
                num(args, 4),
                num(args, 5),
                num(args, 6),
                touch,
            ))
        }
        "marqueePointer" => {
            let shift = arg(args, 3).is_some_and(|v| epdf_annotation_core::json::truthy(Some(v)));
            unit(a.marquee_pointer(
                &text(args, 0),
                &value(args, 1),
                point(args, 2),
                shift,
                num(args, 5),
                num(args, 6),
            ))
        }
        "createPointer" => {
            let finish = arg(args, 4).is_some_and(|v| epdf_annotation_core::json::truthy(Some(v)));
            unit(a.create_pointer(
                &text(args, 0),
                &text(args, 1),
                &value(args, 2),
                point(args, 3),
                finish,
                num(args, 5),
            ))
        }
        "hoverAt" => {
            let at = arg(args, 0).filter(|v| !v.is_null()).map(|v| {
                (
                    v.get("page").cloned().unwrap_or(Value::Null),
                    v.get("point").map(epdf_annotation_core::json::point_loose).unwrap_or_default(),
                    hit_view(v),
                )
            });
            unit(a.hover_at(at.as_ref().map(|(p, point, view)| (p, *point, *view))))
        }
        "finishInkDraft" => {
            a.finish_ink_draft();
            undefined_outcome()
        }
        "cancel" => {
            a.cancel();
            undefined_outcome()
        }
        // ── markup ──
        "createMarkup" => {
            let preset = arg(args, 3).and_then(Value::as_str).map(str::to_string);
            unit(a.create_markup(
                &text(args, 0),
                &value(args, 1),
                &value(args, 2),
                preset.as_deref(),
            ))
        }
        "createCaret" => unit(a.create_caret(&value(args, 0), &value(args, 1))),
        "createReplaceText" => {
            let preset = arg(args, 3).and_then(Value::as_str).map(str::to_string);
            unit(a.create_replace_text(
                &value(args, 0),
                &value(args, 1),
                &value(args, 2),
                preset.as_deref(),
            ))
        }
        "applyToolToSelection" => match a.apply_tool_to_selection(&text(args, 0)) {
            Ok(b) => Outcome::Returned(json!(b)),
            Err(e) => Outcome::Threw(e),
        },
        "previewMarkup" => {
            let preset = arg(args, 2).and_then(Value::as_str).map(str::to_string);
            unit(a.preview_markup(&text(args, 0), &value(args, 1), preset.as_deref()))
        }
        "clearMarkupPreview" => {
            a.clear_markup_preview();
            undefined_outcome()
        }
        "createFromSelection" => {
            let clear =
                arg(args, 1).and_then(|o| o.get("clear")).and_then(Value::as_bool) != Some(false);
            promise(a.create_from_selection(&text(args, 0), clear, None))
        }
        "commitScriptEffects" => {
            let entries: Vec<epdf_plugin_actions::AnnotCommitEntry> =
                serde_json::from_value(value(args, 0)).unwrap_or_default();
            Outcome::promise(a.commit_script_effects(entries))
        }
        // ── events ──
        "onResynced" => subscribe(h, &a.events.resynced, &value(args, 0)),
        "onCreated" => subscribe(h, &a.events.created, &value(args, 0)),
        "onUpdated" => subscribe(h, &a.events.updated, &value(args, 0)),
        "onDeleted" => subscribe(h, &a.events.deleted, &value(args, 0)),
        "onReordered" => subscribe(h, &a.events.reordered, &value(args, 0)),
        "onSelectionChanged" => subscribe(h, &a.events.selection_changed, &value(args, 0)),
        "onDraftChanged" => subscribe(h, &a.events.draft_changed, &value(args, 0)),
        "onEditingChanged" => subscribe(h, &a.events.editing_changed, &value(args, 0)),
        "onHoverChanged" => subscribe(h, &a.events.hover_changed, &value(args, 0)),
        "onWriteFailed" => subscribe(h, &a.events.write_failed, &value(args, 0)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    }
}

/// A page item's `lentScene` prints as a ref, a function the item carries.
fn lent_scenes(h: &Harness, items: Value) -> Value {
    match items {
        Value::Array(list) => Value::Array(
            list.into_iter()
                .map(|mut item| {
                    if item.get("lentScene").is_some_and(|s| s.get("$function").is_some()) {
                        item["lentScene"] = function_ref(h);
                    }
                    item
                })
                .collect(),
        ),
        other => other,
    }
}
