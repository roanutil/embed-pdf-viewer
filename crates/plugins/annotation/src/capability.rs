//! `capability.ts`: the annotation shell. The pure `update` runs here so it
//! can emit effects; the next model is dispatched, and each effect is
//! performed against the engine: optimistic create reconciled to the
//! durable ref, patches that roll back on refusal, deletes fire and forget.
//!
//! Turn counts are behaviour: the trace records how an engine answer and
//! the model updates interleave. Where the TypeScript chains `.then` on an
//! engine call the port reacts one turn after it settles
//! ([`promise::then`]); where it `await`s one, it adopts it
//! ([`promise::await_engine`]); every TypeScript `async` function is a
//! spawned promise, so awaiting it costs the turn awaiting a native promise
//! does.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_acrojs::{ScriptColorArray, script_color_to_rgb};
use epdf_annotation_core::js::{IdSet, Nullable, Record};
use epdf_annotation_core::{
    Annot, AnnotationFlagsPatch, AnnotationProps, AnnotationPropsPatch, AnnotationRef,
    ChromeGeom, ChromeNode, CreationDraftAnchor, Effect, FLAG_KEYS, Geom, Id, Intent, Model, Msg,
    PageObjectNumber, PdfRect, Phase, PlacementOptions, PointerInput, PropKey, PropSpec, Rect,
    RenderItem, RenderSource, SelectionAnchor, SnapSettings, SnapSettingsPatch, Source, Target,
    TextEndAnchor, TextQuad, ViewEnv, annot_contents_editable, annot_deletable,
    annot_transformable, can_move, click_create_geom, content_to_pdf_rect,
    creation_draft_anchor, cursor_at, defaults_for, expand_groups, fit_stamp_box,
    geom_visual_bounds, group_key_of, hit_test, is_substrate_only, link_children_of, link_of,
    page_items, pdf_to_content_rect, props_for, read_prop, resolve_click_placement,
    selection_anchor, shared_props, style_from_props, update, upright_rotation, viewable,
};
use epdf_geometry::{Point, Size};
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::{self, Promise, TimerId};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Ctx, Error};
use epdf_plugin_actions::{
    ACTIONS, AnnotCommitEntry, AnnotCommitResult, AnnotCommitResultEntry, AnnotCommitStatus,
};
use epdf_plugin_interaction::{INTERACTION, Tool, Unregister};
use epdf_plugin_render::policy::{EngineRenderPolicy, snap_appearance_scale};
use epdf_plugin_selection::SELECTION;
use serde_json::{Map, Value, json};

use crate::binary::{resolve_binary_source, sniff_binary_metadata};
use crate::hover_feed::{AnnotationHoverFeed, create_annotation_hover_feed};
use crate::placement::{ICON_PLACE_SIZE, icon_placement_draft, is_icon_place_kind};
use crate::promise::{self, await_engine, then};
use crate::repository::seam::{num_json, pdf_json, ref_key_of};
use crate::repository::{
    Wire, box_geom_fields, from_dto, link_child_rects, ref_key, to_create_draft, to_scoped_patch,
    writable_target,
};
use crate::text_item::build_text_items;
use crate::threads::{CommentThread, build_comment_threads};
use crate::tools::{
    AnnotationToolDef, GhostPolicy, ResolvedTool, StampSourceSpec, ToolRegistry,
    build_tool_registry, is_touch_direct,
};
use crate::types::*;
use crate::value::{f64_of, get, str_of, truthy};

const TEXT_COMMIT_DEBOUNCE_MS: f64 = 250.0;
/// Finger-sized grab zones: 2x the mouse tolerances.
const TOUCH_GRAB_BOOST: f64 = 2.0;

/// `viewEnv(zoom, rotation)`: the core's view environment, or none.
pub fn view_env(zoom: Option<f64>, rotation: Option<f64>) -> Option<ViewEnv> {
    (zoom.is_some() || rotation.is_some())
        .then(|| ViewEnv { zoom: zoom.unwrap_or(1.0), rotation: rotation.unwrap_or(0.0) })
}

/// `===` over two optional numbers (`undefined === undefined`, and `NaN`
/// is never equal).
fn same_num(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn same_rc<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// JavaScript `a <= b` over two JSON values, as the hydration replay
/// compares a `serverId` with the snapshot's `auditHead`.
fn js_le(a: &Value, b: &Value) -> bool {
    if let (Value::String(x), Value::String(y)) = (a, b) {
        return x.encode_utf16().cmp(y.encode_utf16()) != std::cmp::Ordering::Greater;
    }
    let n = |v: &Value| match v {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    };
    n(a) <= n(b)
}

struct ItemsCache {
    model: Rc<Model>,
    ghost: Option<Rc<ToolGhost>>,
    zoom: Option<f64>,
    rotation: Option<f64>,
    v: Vec<RenderItem>,
}

struct ChromeCache {
    model: Rc<Model>,
    cs: Rc<ChromeSettings>,
    scale: Option<f64>,
    rotation: Option<f64>,
    zoom: Option<f64>,
    v: Vec<ChromeNode>,
}

#[derive(Default)]
struct Caches {
    items: HashMap<PageObjectNumber, ItemsCache>,
    chrome: HashMap<PageObjectNumber, ChromeCache>,
    anchor: Option<ChromeCache2>,
}

struct ChromeCache2 {
    model: Rc<Model>,
    cs: Rc<ChromeSettings>,
    scale: Option<f64>,
    rotation: Option<f64>,
    zoom: Option<f64>,
    v: Option<SelectionAnchor>,
}

/// The armed stamp payload: transient tool state, never in the store.
#[derive(Clone)]
struct ArmedStamp {
    source: Value,
    width: f64,
    height: f64,
    preview: Option<ArmedStampPreview>,
}

#[derive(Default)]
struct Hydration {
    started: Cell<bool>,
    window: Cell<bool>,
    running: RefCell<Option<Promise<()>>>,
    again: Cell<bool>,
    pending: RefCell<Vec<Value>>,
}

pub(crate) struct Inner {
    ctx: Ctx<AnnotationState, AnnotationAction>,
    host: AnnotationHost,
    config_tools: Vec<AnnotationToolDef>,
    registry: RefCell<ToolRegistry>,
    behaviors: RefCell<Vec<Rc<dyn Behavior>>>,
    /// Per-annotation debounce timer for the typed `contents` write, in
    /// `Map` insertion order.
    text_timers: RefCell<Vec<(String, TimerId)>>,
    file_picker: RefCell<Option<FilePickerProvider>>,
    hover_feed: Option<AnnotationHoverFeed>,
    caches: RefCell<Caches>,
    armed: RefCell<Option<ArmedStamp>>,
    hydration: Hydration,
    link_chains: RefCell<Vec<(Id, Promise<()>)>>,
    uuid_seq: Cell<u64>,
}

/// The annotation capability: `AnnotationHostCapability`, which includes the
/// public `AnnotationCapability`. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct AnnotationCapability(pub(crate) Rc<Inner>);

/// `createAnnotationCapability(ctx, config)`. Throws when a config tool
/// fails validation.
pub fn create_annotation_capability(
    ctx: Ctx<AnnotationState, AnnotationAction>,
    config: &AnnotationConfig,
) -> Result<AnnotationCapability, Error> {
    let config_tools = config.tools.clone().unwrap_or_default();
    let registry = build_tool_registry(&config_tools)?;
    let hover_feed = match ctx.try_get(&ACTIONS)? {
        Some(actions) => {
            let model_ctx = ctx.clone();
            Some(create_annotation_hover_feed(actions, move |id| {
                model_ctx.try_state().and_then(|s| s.model.by_id.get(id).cloned())
            }))
        }
        None => None,
    };
    Ok(AnnotationCapability(Rc::new(Inner {
        ctx,
        host: config.host.clone(),
        config_tools,
        registry: RefCell::new(registry),
        behaviors: RefCell::new(Vec::new()),
        text_timers: RefCell::new(Vec::new()),
        file_picker: RefCell::new(None),
        hover_feed,
        caches: RefCell::new(Caches::default()),
        armed: RefCell::new(None),
        hydration: Hydration::default(),
        link_chains: RefCell::new(Vec::new()),
        uuid_seq: Cell::new(0),
    })))
}

/// `{userId?, groupId?}` of a DTO: the stamped owner the collab resolver
/// checks.
fn owner_of(dto: Option<&Value>) -> Value {
    let mut out = Map::new();
    if let Some(d) = dto {
        for key in ["userId", "groupId"] {
            if let Some(v) = get(d, key) {
                out.insert(key.into(), v.clone());
            }
        }
    }
    Value::Object(out)
}

fn ref_json(r: &AnnotationRef) -> Value {
    serde_json::to_value(r).unwrap_or_default()
}

fn ref_from(v: &Value) -> Option<AnnotationRef> {
    serde_json::from_value(v.clone()).ok()
}

/// The page a wire ref names.
fn ref_pon(v: &Value) -> Option<PageObjectNumber> {
    get(v, "pageObjectNumber").and_then(Value::as_f64).map(|p| p as PageObjectNumber)
}

impl AnnotationCapability {
    // ── plumbing ──────────────────────────────────────────────────────────────

    fn ctx(&self) -> &Ctx<AnnotationState, AnnotationAction> {
        &self.0.ctx
    }

    fn state(&self) -> Rc<AnnotationState> {
        self.0.ctx.state()
    }

    /// `ctx.getState().model`.
    pub fn model(&self) -> Rc<Model> {
        self.state().model.clone()
    }

    fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.0.ctx.doc()
    }

    fn chrome_settings_rc(&self) -> Rc<ChromeSettings> {
        self.state().chrome.clone()
    }

    fn console_error(&self, message: &str, error: Option<&Error>) {
        if let Some(sink) = &self.0.host.console_error {
            sink(message, error);
        }
    }

    fn random_uuid(&self) -> String {
        match &self.0.host.random_uuid {
            Some(f) => f(),
            None => {
                let n = self.0.uuid_seq.get() + 1;
                self.0.uuid_seq.set(n);
                format!("00000000-0000-4000-8000-{n:012x}")
            }
        }
    }

    fn active_tool_id(&self) -> Option<String> {
        self.ctx().try_get(&INTERACTION).ok().flatten().map(|ix| ix.active_tool_id())
    }

    fn active_tool(&self) -> Option<ResolvedTool> {
        let id = self.active_tool_id()?;
        self.0.registry.borrow().get(&id).cloned()
    }

    fn registry_get(&self, id: &str) -> Option<ResolvedTool> {
        self.0.registry.borrow().get(id).cloned()
    }

    /// `doc.page(pon).annotations.<method>(...args)`: `page(pon)` refuses an
    /// unknown page synchronously.
    fn page_call(
        doc: &Rc<dyn DocumentHandle>,
        pon: PageObjectNumber,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Promise<Value>, Error> {
        doc.page_index(pon)?;
        Ok(doc.call(&format!("page.annotations.{method}"), Some(pon), args))
    }

    /// `cropOf(pon)`: the page's crop box from the document registry.
    fn crop_of(&self, pon: PageObjectNumber) -> Option<PdfRect> {
        let meta = self.ctx().document()?;
        let page = meta.pages.as_array()?.iter().find(|p| {
            get(p, "pageObjectNumber").and_then(Value::as_f64) == Some(f64::from(pon))
        })?;
        let crop = get(page, "boxes").and_then(|b| get(b, "crop")).filter(|c| c.is_object())?;
        Some(crate::value::pdf_rect_of(Some(crop)))
    }

    /// `pageBoxOf(pon)`: the page's box in content space.
    fn page_box_of(&self, pon: PageObjectNumber) -> Option<Rect> {
        let crop = self.crop_of(pon)?;
        Some(Rect { x: 0.0, y: 0.0, width: crop.right - crop.left, height: crop.top - crop.bottom })
    }

    fn page_size(crop: PdfRect) -> Size {
        Size { width: crop.right - crop.left, height: crop.top - crop.bottom }
    }

    /// `chromeGeomAt(scale, boost)`.
    fn chrome_geom_at(&self, scale: Option<f64>, boost: f64) -> ChromeGeom {
        let cs = self.chrome_settings_rc();
        let s = match scale {
            Some(s) if s != 0.0 && !s.is_nan() => s,
            _ => 1.0,
        };
        ChromeGeom {
            handle_tol: (cs.handles.hit_size / 2.0 / s) * boost,
            knob_tol: (cs.knob.hit_size / 2.0 / s) * boost,
            knob_offset: cs.knob.offset / s,
        }
    }

    fn grab_boost(touch: Option<bool>) -> f64 {
        if touch == Some(true) { TOUCH_GRAB_BOOST } else { 1.0 }
    }

    fn engaged_for(&self, a: &Annot) -> bool {
        let behaviors = self.0.behaviors.borrow().clone();
        behaviors.iter().any(|b| b.matches(&a.subtype, a.ref_.as_ref()) && b.engaged())
    }

    /// `inertIdsAt(pon)`: ids whose behavior is engaged, resolved per event.
    fn inert_ids_at(&self, pon: PageObjectNumber) -> Option<IdSet> {
        if self.0.behaviors.borrow().is_empty() {
            return None;
        }
        let m = self.model();
        let mut out: Option<IdSet> = None;
        for id in &m.order {
            let Some(a) = m.by_id.get(id) else { continue };
            if a.pon != pon {
                continue;
            }
            if self.engaged_for(a) {
                out.get_or_insert_with(IdSet::default).0.push(id.clone());
            }
        }
        out
    }

    /// `apply(msg)`: the pure update, the dispatch, then each effect.
    fn apply(&self, msg: Msg) {
        let m = self.model();
        let (next, effects) = update(&m, &msg);
        let next = next.map_or(m, Rc::new);
        self.ctx().dispatch(AnnotationAction::SetModel { model: next.clone() });
        for fx in effects {
            self.perform(fx, &next);
        }
    }

    /// `ingestDTO(dto, crop, source)`: the DTO with this session's
    /// per-record authority projected onto it.
    fn ingest_dto(&self, dto: &Value, crop: PdfRect, source: Source) -> Annot {
        let mut a = from_dto(dto, crop, source);
        let Some(doc) = self.doc() else { return a };
        let sec = doc.security();
        let target = owner_of(Some(dto));
        a.authority = Some(epdf_annotation_core::Authority {
            update: sec.allows_annotation_mutation("update", &target),
            delete: sec.allows_annotation_mutation("delete", &target),
        });
        a
    }

    /// `syncDTO(dto, source, bumpAp)`.
    fn sync_dto(&self, dto: &Value, source: Source, bump_ap: bool) {
        let Some(pon) = ref_pon(dto) else { return };
        if let Some(crop) = self.crop_of(pon) {
            let annot = self.ingest_dto(dto, crop, source);
            self.apply(Msg::Upsert { annots: vec![annot], bump_ap: Some(bump_ap) });
        }
    }

    /// `ponForRef(ref)`.
    fn pon_for_ref(&self, r: &AnnotationRef) -> Option<PageObjectNumber> {
        if let Some(a) = self.model().by_id.get(&ref_key(r)) {
            return Some(a.pon);
        }
        match r {
            AnnotationRef::Index { page_object_number, .. } => Some(*page_object_number),
            _ => None,
        }
    }

    fn allows_create(&self) -> bool {
        self.doc().is_some_and(|d| d.security().allows_annotation_create())
    }

    fn allows_mutation(&self, action: &str, r: &AnnotationRef) -> bool {
        let Some(doc) = self.doc() else { return false };
        let m = self.model();
        let target = owner_of(m.by_id.get(&ref_key(r)).and_then(|a| a.data.as_ref()));
        doc.security().allows_annotation_mutation(action, &target)
    }

    fn deletable_one(&self, r: &Value) -> bool {
        let m = self.model();
        ref_key_of(r).and_then(|k| m.by_id.get(&k).cloned()).is_some_and(|a| annot_deletable(&a))
    }

    fn current_user_id(&self) -> Option<String> {
        let identity = self.doc()?.security().identity();
        str_of(&identity, "user_id").map(str::to_string)
    }

    // ── engine writes ─────────────────────────────────────────────────────────

    /// `updateOne(ref, patch)`: the one engine-update path.
    fn update_one(&self, r: AnnotationRef, patch: Value) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let pon = this
                .pon_for_ref(&r)
                .ok_or_else(|| Error::new("[annotation] cannot resolve page for ref"))?;
            let call = Self::page_call(&doc, pon, "update", vec![ref_json(&r), patch])?;
            let res = await_engine(call).await?;
            this.sync_dto(get(&res, "updated").unwrap_or(&Value::Null), Source::Vector, false);
            Ok(())
        })
    }

    fn perform(&self, fx: Effect, m: &Rc<Model>) {
        let Some(doc) = self.doc() else { return };
        match fx {
            Effect::CreateGroup { primary, members } => self.perform_create_group(doc, primary, members, m),
            Effect::Create { id } => {
                let Some(a) = m.by_id.get(&id).cloned() else { return };
                let Some(crop) = self.crop_of(a.pon) else { return };
                let Some(draft) = to_create_draft(&a, crop, &|| self.random_uuid()) else { return };
                let Ok(call) = Self::page_call(&doc, a.pon, "create", vec![Value::Object(draft)]) else {
                    return;
                };
                let this = self.clone();
                then(call, move |result| match result {
                    Ok(res) => {
                        let created = get(&res, "created").cloned().unwrap_or(Value::Null);
                        let r = get(&created, "ref").and_then(ref_from);
                        if let Some(r) = r {
                            this.apply(Msg::Created { temp_id: id, id: ref_key(&r), ref_: r });
                        }
                        this.sync_dto(&created, Source::Vector, false);
                    }
                    Err(_) => this.apply(Msg::CreateFailed { temp_id: id }),
                });
            }
            Effect::Flags { id } => {
                let Some(a) = m.by_id.get(&id).cloned() else { return };
                let (Some(r), Some(data)) = (a.ref_.clone(), a.data.clone()) else { return };
                let patch = json!({
                    "subtype": get(&data, "subtype").cloned().unwrap_or(Value::Null),
                    "flags": serde_json::to_value(a.flags).unwrap_or_default(),
                });
                let Ok(call) = Self::page_call(&doc, a.pon, "update", vec![ref_json(&r), patch]) else {
                    return;
                };
                let this = self.clone();
                then(call, move |result| match result {
                    Ok(res) => this.sync_dto(get(&res, "updated").unwrap_or(&Value::Null), a.source, false),
                    Err(err) => this.console_error("[annotation] flags write failed:", Some(&err)),
                });
            }
            Effect::Patch { id, scope, .. } => {
                let Some(a) = m.by_id.get(&id).cloned() else { return };
                let crop = self.crop_of(a.pon);
                let patch = match (&a.ref_, crop) {
                    (Some(_), Some(crop)) => to_scoped_patch(&a, &scope, crop),
                    _ => None,
                };
                let (Some(r), Some(patch)) = (a.ref_.clone(), patch) else { return };
                let Ok(call) =
                    Self::page_call(&doc, a.pon, "update", vec![ref_json(&r), Value::Object(patch)])
                else {
                    return;
                };
                let this = self.clone();
                then(call, move |result| match result {
                    Ok(res) => {
                        let changed = get(&res, "appearance").and_then(|ap| get(ap, "changed"));
                        this.sync_dto(
                            get(&res, "updated").unwrap_or(&Value::Null),
                            a.source,
                            truthy(changed),
                        );
                        if !link_children_of(&this.model(), &id).is_empty() {
                            let _ = this.schedule_link_sync(&id, LinkIntent::Keep);
                        }
                    }
                    Err(_) => {
                        if let (Some(data), Some(crop)) = (&a.data, crop) {
                            let annot = this.ingest_dto(data, crop, a.source);
                            this.apply(Msg::Upsert { annots: vec![annot], bump_ap: None });
                        }
                    }
                });
            }
            Effect::SyncLink { id, target } => {
                let _ = self.schedule_link_sync(&id, LinkIntent::Target(target));
            }
            Effect::Delete { ref_ } => {
                let pon = match &ref_ {
                    AnnotationRef::ObjectNumber { page_object_number, .. }
                    | AnnotationRef::Nm { page_object_number, .. }
                    | AnnotationRef::Index { page_object_number, .. } => *page_object_number,
                };
                // `.then(() => {}, () => {})`: nothing observes the answer.
                let _ = Self::page_call(&doc, pon, "delete", vec![ref_json(&ref_)]);
            }
        }
    }

    fn perform_create_group(
        &self,
        doc: Rc<dyn DocumentHandle>,
        primary: Id,
        members: Vec<Id>,
        m: &Rc<Model>,
    ) {
        let mut ids = vec![primary.clone()];
        ids.extend(members.iter().cloned());
        let annots: Vec<Option<Annot>> = ids.iter().map(|id| m.by_id.get(id).cloned()).collect();
        let Some(first) = annots[0].clone() else {
            self.apply(Msg::Remove { ids });
            return;
        };
        if annots.iter().any(|a| a.as_ref().is_none_or(|a| a.pon != first.pon)) {
            self.apply(Msg::Remove { ids });
            return;
        }
        let crop = self.crop_of(first.pon);
        let drafts: Vec<Option<Wire>> = match crop {
            Some(crop) => annots
                .iter()
                .map(|a| a.as_ref().and_then(|a| to_create_draft(a, crop, &|| self.random_uuid())))
                .collect(),
            None => annots.iter().map(|_| None).collect(),
        };
        if drafts.iter().any(Option::is_none) {
            self.apply(Msg::Remove { ids });
            return;
        }
        let drafts: Vec<Wire> = drafts.into_iter().flatten().collect();
        let this = self.clone();
        let pon = first.pon;
        rt::spawn(async move {
            let mut committed: Vec<(Id, AnnotationRef)> = Vec::new();
            let run = async {
                let primary_call = Self::page_call(&doc, pon, "create", vec![Value::Object(drafts[0].clone())])?;
                let primary_result = await_engine(primary_call).await?;
                let created = get(&primary_result, "created").cloned().unwrap_or(Value::Null);
                let primary_ref = get(&created, "ref")
                    .and_then(ref_from)
                    .ok_or_else(|| Error::named("TypeError", "Cannot read properties of undefined (reading 'ref')"))?;
                committed.push((primary.clone(), primary_ref.clone()));
                this.apply(Msg::Created {
                    temp_id: primary.clone(),
                    id: ref_key(&primary_ref),
                    ref_: primary_ref.clone(),
                });
                this.sync_dto(&created, Source::Vector, false);
                for (i, temp_id) in members.iter().enumerate() {
                    let mut draft = drafts[i + 1].clone();
                    draft.insert("inReplyTo".into(), ref_json(&primary_ref));
                    draft.insert("replyType".into(), json!("group"));
                    let call = Self::page_call(&doc, pon, "create", vec![Value::Object(draft)])?;
                    let result = await_engine(call).await?;
                    let created = get(&result, "created").cloned().unwrap_or(Value::Null);
                    let r = get(&created, "ref")
                        .and_then(ref_from)
                        .ok_or_else(|| Error::named("TypeError", "Cannot read properties of undefined (reading 'ref')"))?;
                    committed.push((temp_id.clone(), r.clone()));
                    this.apply(Msg::Created { temp_id: temp_id.clone(), id: ref_key(&r), ref_: r });
                    this.sync_dto(&created, Source::Vector, false);
                }
                Ok::<(), Error>(())
            };
            let outcome = run.await;
            if let Err(error) = outcome {
                let mut remove_ids: Vec<Id> =
                    ids.iter().filter(|id| !committed.iter().any(|(t, _)| t == *id)).cloned().collect();
                for (_, r) in committed.iter().rev() {
                    let deleted = match Self::page_call(&doc, pon, "delete", vec![ref_json(r)]) {
                        Ok(call) => await_engine(call).await.is_ok(),
                        Err(_) => false,
                    };
                    if deleted {
                        remove_ids.push(ref_key(r));
                    }
                }
                if !remove_ids.is_empty() {
                    this.apply(Msg::Remove { ids: remove_ids });
                }
                this.console_error("[annotation] grouped annotation creation failed:", Some(&error));
            }
        });
    }

    // ── attached links ────────────────────────────────────────────────────────

    /// `scheduleLinkSync(id, intent)`: chain a reconcile after the parent's
    /// previous one.
    fn schedule_link_sync(&self, id: &str, intent: LinkIntent) -> Promise<()> {
        let prev = self
            .0
            .link_chains
            .borrow()
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| Promise::resolved(()));
        let this = self.clone();
        let owned = id.to_string();
        let next = Promise::spawn(async move {
            prev.await?;
            let desired = match intent {
                LinkIntent::Keep => link_of(&this.model(), &owned),
                LinkIntent::Target(t) => t,
            };
            let run = this.reconcile_link_children(owned, desired);
            rt::tick().await;
            run.await
        });
        {
            let mut chains = self.0.link_chains.borrow_mut();
            match chains.iter_mut().find(|(k, _)| k == id) {
                Some(slot) => slot.1 = next.clone(),
                None => chains.push((id.to_string(), next.clone())),
            }
        }
        let weak = Rc::downgrade(&self.0);
        let (key, mine) = (id.to_string(), next.clone());
        rt::spawn(async move {
            let _ = mine.clone().await;
            let Some(inner) = weak.upgrade() else { return };
            let mut chains = inner.link_chains.borrow_mut();
            if chains.iter().any(|(k, p)| *k == key && p.same(&mine)) {
                chains.retain(|(k, _)| *k != key);
            }
        });
        next
    }

    /// `reconcileLinkChildren(id, desired)`: the one place attached link
    /// children are created, retargeted, re-rected or deleted.
    fn reconcile_link_children(&self, id: Id, desired: Option<epdf_annotation_core::PdfLinkTarget>) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc();
            let a = this.model().by_id.get(&id).cloned();
            let (Some(doc), Some(a)) = (doc, a) else { return Ok(()) };
            let Some(parent_ref) = a.ref_.clone() else { return Ok(()) };
            if a.subtype == "link" {
                return Ok(());
            }
            let Some(crop) = this.crop_of(a.pon) else { return Ok(()) };
            doc.page_index(a.pon)?;
            let target = writable_target(desired.as_ref());
            let rects: Vec<PdfRect> = match desired {
                None => Vec::new(),
                Some(_) => link_child_rects(&a).into_iter().map(|r| content_to_pdf_rect(r, crop)).collect(),
            };
            let current: Vec<Annot> =
                link_children_of(&this.model(), &id).into_iter().cloned().collect();
            let target_json = target.as_ref().map(|t| serde_json::to_value(t).unwrap_or_default());
            let run = async {
                let paired = current.len().min(rects.len());
                for i in 0..paired {
                    let Some(r) = current[i].ref_.clone() else { continue };
                    let mut patch = Map::new();
                    patch.insert("subtype".into(), json!("link"));
                    patch.insert("rect".into(), pdf_json(rects[i]));
                    if let Some(t) = &target_json {
                        patch.insert("target".into(), t.clone());
                    }
                    let call = Self::page_call(&doc, a.pon, "update", vec![ref_json(&r), Value::Object(patch)])?;
                    let res = await_engine(call).await?;
                    let dto = get(&res, "updated").cloned().unwrap_or(Value::Null);
                    let annot = this.ingest_dto(&dto, crop, Source::Baked);
                    this.apply(Msg::Upsert { annots: vec![annot], bump_ap: None });
                }
                for rect in rects.iter().skip(current.len()) {
                    let draft = json!({
                        "subtype": "link",
                        "rect": pdf_json(*rect),
                        "target": target_json.clone().unwrap_or(Value::Null),
                        "inReplyTo": ref_json(&parent_ref),
                        "replyType": "group",
                    });
                    let call = Self::page_call(&doc, a.pon, "create", vec![draft])?;
                    let res = await_engine(call).await?;
                    let dto = get(&res, "created").cloned().unwrap_or(Value::Null);
                    let annot = this.ingest_dto(&dto, crop, Source::Baked);
                    this.apply(Msg::Upsert { annots: vec![annot], bump_ap: None });
                }
                for child in current.iter().skip(rects.len()) {
                    if let Some(r) = &child.ref_ {
                        let call = Self::page_call(&doc, a.pon, "delete", vec![ref_json(r)])?;
                        await_engine(call).await?;
                    }
                    this.apply(Msg::Remove { ids: vec![child.id.clone()] });
                }
                Ok::<(), Error>(())
            };
            if let Err(err) = run.await {
                this.console_error("[annotation] attached-link sync failed:", Some(&err));
            }
            Ok(())
        })
    }

    /// `writeRelationship(a, rel)`: a relationship-only patch, re-synced with
    /// the annotation's own render source.
    fn write_relationship(&self, a: Annot, in_reply_to: Option<AnnotationRef>, group: bool) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let Some(doc) = this.doc() else { return Ok(()) };
            let (Some(r), Some(data)) = (a.ref_.clone(), a.data.clone()) else { return Ok(()) };
            let mut patch = Map::new();
            patch.insert("subtype".into(), get(&data, "subtype").cloned().unwrap_or(Value::Null));
            patch.insert("inReplyTo".into(), in_reply_to.as_ref().map_or(Value::Null, ref_json));
            if group {
                patch.insert("replyType".into(), json!("group"));
            }
            let call = Self::page_call(&doc, a.pon, "update", vec![ref_json(&r), Value::Object(patch)])?;
            let res = await_engine(call).await?;
            this.sync_dto(get(&res, "updated").unwrap_or(&Value::Null), a.source, false);
            Ok(())
        })
    }

    fn selected_committed(&self) -> Vec<Annot> {
        let m = self.model();
        m.selected
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter(|a| a.ref_.is_some() && a.data.is_some())
            .cloned()
            .collect()
    }

    // ── whole-document hydration ──────────────────────────────────────────────

    fn set_hydration(&self, hydration: AnnotationHydration) {
        self.ctx().dispatch(AnnotationAction::SetHydration { hydration });
    }

    fn ingest_snapshot(&self, snap: &Value, bump_ap: bool) {
        let mut annots = Vec::new();
        for page in get(snap, "pages").and_then(Value::as_array).into_iter().flatten() {
            let pon = get(page, "pageState").and_then(ref_pon);
            let Some(crop) = pon.and_then(|p| self.crop_of(p)) else { continue };
            for d in get(page, "annotations").and_then(Value::as_array).into_iter().flatten() {
                annots.push(self.ingest_dto(d, crop, Source::Baked));
            }
        }
        self.apply(Msg::Hydrated { annots, bump_ap: Some(bump_ap) });
    }

    /// `canRead()`: `doc.annotate.read`, allowed without a document.
    pub fn can_read(&self) -> bool {
        self.doc().is_none_or(|d| d.security().allows("doc.annotate.read"))
    }

    fn run_hydration(&self, bump_ap: bool) -> Promise<()> {
        let Some(doc) = self.doc() else { return Promise::resolved(()) };
        if !self.can_read() {
            self.set_hydration(AnnotationHydration::Forbidden);
            return Promise::resolved(());
        }
        self.0.hydration.window.set(true);
        self.set_hydration(AnnotationHydration::Loading);
        let call = doc.call("annotations.listRawAll", None, Vec::new());
        let this = self.clone();
        // `.then(ingest)`
        let ingested: Promise<()> = Promise::spawn(async move {
            let snap = call.await?;
            this.ingest_snapshot(&snap, bump_ap);
            let head = get(&snap, "auditHead").cloned();
            let queued = std::mem::take(&mut *this.0.hydration.pending.borrow_mut());
            this.0.hydration.window.set(false);
            for event in queued {
                let server_id = match event.get("origin") {
                    Some(origin) => origin.get("serverId").cloned().unwrap_or_else(epdf_kernel::undefined),
                    None => Value::Null,
                };
                if let Some(head) = &head
                    && !server_id.is_null()
                    && !epdf_kernel::is_undefined(&server_id)
                    && js_le(&server_id, head)
                {
                    continue;
                }
                this.apply_remote_annotation_event(&event);
            }
            this.set_hydration(AnnotationHydration::Complete);
            Ok(())
        });
        let this = self.clone();
        // `.catch(degrade)`
        let caught: Promise<()> = Promise::spawn(async move {
            if let Err(error) = ingested.await {
                this.0.hydration.window.set(false);
                let queued = std::mem::take(&mut *this.0.hydration.pending.borrow_mut());
                for event in queued {
                    this.apply_remote_annotation_event(&event);
                }
                this.set_hydration(AnnotationHydration::Error(error));
            }
            Ok(())
        });
        let this = self.clone();
        // `.finally(chain)`: the callback one turn after, the result two
        // more (the resolved callback value is adopted).
        let run: Promise<()> = Promise::spawn(async move {
            let result = caught.await;
            *this.0.hydration.running.borrow_mut() = None;
            if this.0.hydration.again.get() {
                this.0.hydration.again.set(false);
                let again = this.run_hydration(true);
                *this.0.hydration.running.borrow_mut() = Some(again);
            }
            rt::tick().await;
            rt::tick().await;
            result
        });
        *self.0.hydration.running.borrow_mut() = Some(run.clone());
        run
    }

    /// `rehydrate()`: re-run whole-document hydration; while one is in
    /// flight the fresh run is chained after it.
    pub fn rehydrate(&self) -> Promise<()> {
        let running = self.0.hydration.running.borrow().clone();
        if let Some(running) = running {
            self.0.hydration.again.set(true);
            return running;
        }
        let run = self.run_hydration(true);
        *self.0.hydration.running.borrow_mut() = Some(run.clone());
        run
    }

    fn upsert_remote(&self, dtos: &[Value], bump_ap: bool) {
        let mut bump = Vec::new();
        let mut keep = Vec::new();
        for dto in dtos {
            let Some(crop) = ref_pon(dto).and_then(|p| self.crop_of(p)) else { continue };
            let a = self.ingest_dto(dto, crop, Source::Baked);
            if bump_ap && !is_substrate_only(&a) { bump.push(a) } else { keep.push(a) }
        }
        if !bump.is_empty() {
            self.apply(Msg::Upsert { annots: bump, bump_ap: Some(true) });
        }
        if !keep.is_empty() {
            self.apply(Msg::Upsert { annots: keep, bump_ap: None });
        }
    }

    fn apply_remote_annotation_event(&self, event: &Value) {
        match str_of(event, "type") {
            Some("annotation.created") => {
                self.upsert_remote(&[get(event, "created").cloned().unwrap_or(Value::Null)], true)
            }
            Some("annotation.updated") => {
                let changed = get(event, "appearance").and_then(|a| get(a, "changed"));
                self.upsert_remote(&[get(event, "updated").cloned().unwrap_or(Value::Null)], truthy(changed))
            }
            Some("annotation.moved") => {
                let moved = get(event, "moved").and_then(Value::as_array).cloned().unwrap_or_default();
                self.upsert_remote(&moved, false)
            }
            Some("annotation.deleted") => {
                let Some(deleted) = get(event, "deleted").filter(|d| truthy(Some(d))) else { return };
                let key = match str_of(deleted, "kind") {
                    Some("objectNumber") => {
                        format!("obj:{}", epdf_geometry::js::number_to_string(f64_of(deleted, "value")))
                    }
                    _ => format!("nm:{}", get(deleted, "value").and_then(Value::as_str).unwrap_or_default()),
                };
                if self.model().by_id.contains_key(&key) {
                    self.apply(Msg::Remove { ids: vec![key] });
                }
            }
            _ => {}
        }
    }

    // ── the stamp tool and click placement ────────────────────────────────────

    /// `desiredStampSize(meta, targetWidth, intrinsicOverride)`.
    fn desired_stamp_size(
        meta: &crate::binary::BinaryMetadata,
        target_width: Option<f64>,
        intrinsic_override: Option<Size>,
    ) -> Size {
        let intrinsic = match (intrinsic_override, meta.size) {
            (Some(o), _) if o.width > 0.0 && o.height > 0.0 => o,
            (_, Some((w, h))) if w > 0.0 => Size { width: w, height: h },
            _ => Size { width: target_width.unwrap_or(150.0), height: target_width.unwrap_or(150.0) },
        };
        match target_width {
            None => intrinsic,
            Some(tw) => Size { width: tw, height: tw * (intrinsic.height / intrinsic.width) },
        }
    }

    /// `uprightRotFor(displayRotation)`.
    fn upright_rot_for(&self, display_rotation: Option<f64>) -> f64 {
        let Some(dr) = display_rotation.filter(|r| *r != 0.0 && !r.is_nan()) else { return 0.0 };
        match self.active_tool() {
            Some(tool) if tool.upright => upright_rotation(dr),
            _ => 0.0,
        }
    }

    fn select_created(&self, res: &Value) {
        let created = get(res, "created").cloned().unwrap_or(Value::Null);
        self.sync_dto(&created, Source::Baked, false);
        if let Some(key) = get(&created, "ref").and_then(ref_key_of) {
            self.apply(Msg::Select { ids: vec![key], add: None });
        }
    }

    /// `createStampAt(pon, point, source, desired, rotCW)`.
    fn create_stamp_at(&self, pon: PageObjectNumber, point: Point, source: Value, desired: Size, rot_cw: f64) -> bool {
        let (Some(doc), Some(crop)) = (self.doc(), self.crop_of(pon)) else { return false };
        let bx = fit_stamp_box(point, desired, Self::page_size(crop), rot_cw);
        let mut draft = Map::new();
        draft.insert("subtype".into(), json!("stamp"));
        draft.extend(box_geom_fields(bx, rot_cw, crop));
        draft.insert("source".into(), source);
        draft.insert("fit".into(), json!("contain"));
        let Ok(call) = Self::page_call(&doc, pon, "create", vec![Value::Object(draft)]) else {
            return true;
        };
        let this = self.clone();
        then(call, move |result| match result {
            Ok(res) => this.select_created(&res),
            Err(err) => this.console_error("[annotation] stamp placement failed:", Some(&err)),
        });
        true
    }

    /// `placeStampSource(pon, point, source, rotCW, targetWidth)`.
    fn place_stamp_source(&self, pon: PageObjectNumber, point: Point, source: Value, rot_cw: f64) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let resolved = resolve_binary_source(&source).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                this.console_error("[annotation] stamp source must be PNG, JPEG, or single-page PDF bytes", None);
                return Ok(());
            };
            this.create_stamp_at(pon, point, source, Self::desired_stamp_size(&meta, None, None), rot_cw);
            Ok(())
        })
    }

    /// `promptFileAt(tool, pon, point, place)`: the file-picker port, with
    /// the expiry rule.
    fn prompt_file_at(
        &self,
        tool: &ResolvedTool,
        pon: PageObjectNumber,
        point: Point,
        place: impl FnOnce(&AnnotationCapability, Value) + 'static,
    ) -> bool {
        let Some(provider) = self.0.file_picker.borrow().clone() else { return false };
        let accept = match &tool.source {
            Some(StampSourceSpec::Prompt(p)) => p.accept.clone(),
            _ => None,
        };
        let req = FilePromptRequest {
            tool_id: tool.id.clone(),
            subtype: tool.subtype.clone(),
            accept,
            pon,
            point,
        };
        let doc_at_click = self.doc();
        let answer = provider(req);
        let this = self.clone();
        let tool_id = tool.id.clone();
        then(answer, move |result| match result {
            Ok(picked) => {
                if !truthy(Some(&picked)) {
                    return;
                }
                let same_doc = match (this.doc(), &doc_at_click) {
                    (Some(a), Some(b)) => Rc::ptr_eq(&a, b),
                    (None, None) => true,
                    _ => false,
                };
                if !same_doc {
                    return;
                }
                if this.active_tool_id().as_deref() != Some(tool_id.as_str()) {
                    return;
                }
                place(&this, picked);
            }
            Err(err) => this.console_error("[annotation] file-picker provider failed:", Some(&err)),
        });
        true
    }

    /// `createIconAt(tool, pon, point, rotCW, file)`.
    fn create_icon_at(&self, tool: &ResolvedTool, pon: PageObjectNumber, point: Point, rot_cw: f64, file: Option<Value>) -> bool {
        let (Some(doc), Some(crop)) = (self.doc(), self.crop_of(pon)) else { return false };
        if !is_icon_place_kind(&tool.subtype) {
            return false;
        }
        let bx = fit_stamp_box(point, ICON_PLACE_SIZE, Self::page_size(crop), rot_cw);
        let defaults = defaults_for(&self.model(), &tool.preset);
        let Ok(draft) = icon_placement_draft(
            &tool.subtype,
            box_geom_fields(bx, rot_cw, crop),
            &defaults,
            tool.flags.as_ref(),
            file,
        ) else {
            return false;
        };
        let Ok(call) = Self::page_call(&doc, pon, "create", vec![Value::Object(draft)]) else {
            return true;
        };
        let this = self.clone();
        then(call, move |result| match result {
            Ok(res) => this.select_created(&res),
            Err(err) => this.console_error("[annotation] icon placement failed:", Some(&err)),
        });
        true
    }

    fn dispatch_vector_ghost(&self, pon: PageObjectNumber, tool_id: &str, geom: Geom) {
        let preset = self.registry_get(tool_id).map_or_else(|| tool_id.to_string(), |t| t.preset);
        let style = style_from_props(&defaults_for(&self.model(), &preset));
        let bx = geom_visual_bounds(&geom, style.stroke_width, Some(&style.border));
        self.ctx().dispatch(AnnotationAction::SetToolGhost {
            ghost: Some(Rc::new(ToolGhost {
                pon,
                box_: bx,
                rot: 0.0,
                kind: ToolGhostKind::Vector { tool_id: tool_id.to_string(), geom },
            })),
        });
    }

    // ── comments ──────────────────────────────────────────────────────────────

    fn layout_pons(&self) -> Vec<f64> {
        self.ctx()
            .document()
            .and_then(|d| d.pages.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|p| f64_of(p, "pageObjectNumber"))
            .collect()
    }

    /// `computeThreadsIndex()`: committed truth only, display-ordered.
    fn threads_index(&self) -> (Vec<CommentThread>, Vec<(Id, usize)>) {
        let m = self.model();
        let dtos: Vec<Value> =
            m.order.iter().filter_map(|id| m.by_id.get(id)).filter_map(|a| a.data.clone()).collect();
        let user = self.current_user_id();
        let mut threads = build_comment_threads(&dtos, user.as_deref());
        let pons = self.layout_pons();
        let index = |t: &CommentThread| -> f64 {
            let pon = t.page_object_number.as_f64();
            pons.iter()
                .position(|p| Some(*p) == pon)
                .map_or(9_007_199_254_740_991.0, |i| i as f64)
        };
        let top = |t: &CommentThread| get(&t.root, "rect").map_or(f64::NAN, |r| f64_of(r, "top"));
        let created = |t: &CommentThread| str_of(&t.root, "created").unwrap_or_default().to_string();
        threads.sort_by(|a, b| {
            let (pa, pb) = (index(a), index(b));
            if pa != pb {
                return (pa - pb).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal);
            }
            let (ta, tb) = (top(a), top(b));
            if ta != tb {
                return (tb - ta).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal);
            }
            let (ca, cb) = (created(a), created(b));
            ca.encode_utf16().cmp(cb.encode_utf16())
        });
        let mut by_member: Vec<(Id, usize)> = Vec::new();
        let mut set = |key: Option<String>, i: usize| {
            if let Some(key) = key {
                match by_member.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = i,
                    None => by_member.push((key, i)),
                }
            }
        };
        for (i, t) in threads.iter().enumerate() {
            set(get(&t.root, "ref").and_then(ref_key_of), i);
            for r in &t.replies {
                set(get(r, "ref").and_then(ref_key_of), i);
            }
            for g in &t.grouped_parts {
                set(get(g, "ref").and_then(ref_key_of), i);
            }
            for s in &t.review.status_refs {
                set(ref_key_of(s), i);
            }
        }
        (threads, by_member)
    }

    fn thread_containing(&self, r: &AnnotationRef) -> Option<CommentThread> {
        let (threads, by_member) = self.threads_index();
        let key = ref_key(r);
        by_member.iter().find(|(k, _)| *k == key).map(|(_, i)| threads[*i].clone())
    }

    fn thread_of(&self, r: &AnnotationRef) -> Result<CommentThread, Error> {
        self.thread_containing(r)
            .ok_or_else(|| Error::new("[annotation] no comment thread contains this ref"))
    }

    fn create_conversation_annot(&self, pon: PageObjectNumber, draft: Value) -> Promise<Value> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let call = Self::page_call(&doc, pon, "create", vec![draft])?;
            let res = await_engine(call).await?;
            let created = get(&res, "created").cloned().unwrap_or(Value::Null);
            this.sync_dto(&created, Source::Baked, false);
            Ok(created)
        })
    }

    fn delete_one(&self, r: Value) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let pon = ref_pon(&r).unwrap_or_default();
            let call = Self::page_call(&doc, pon, "delete", vec![r.clone()])?;
            await_engine(call).await?;
            if let Some(key) = ref_key_of(&r) {
                this.apply(Msg::Remove { ids: vec![key] });
            }
            Ok(())
        })
    }

    fn thread_member_refs(t: &CommentThread) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        out.extend(t.replies.iter().map(|r| get(r, "ref").cloned().unwrap_or(Value::Null)));
        out.extend(t.grouped_parts.iter().map(|g| get(g, "ref").cloned().unwrap_or(Value::Null)));
        out.extend(t.review.status_refs.iter().cloned());
        out.push(get(&t.root, "ref").cloned().unwrap_or(Value::Null));
        out
    }

    fn thread_pon(t: &CommentThread) -> PageObjectNumber {
        t.page_object_number.as_f64().map_or(0, |p| p as PageObjectNumber)
    }

    fn root_rect(t: &CommentThread) -> Value {
        get(&t.root, "rect").cloned().unwrap_or(Value::Null)
    }

    fn root_ref(t: &CommentThread) -> Value {
        get(&t.root, "ref").cloned().unwrap_or(Value::Null)
    }
}

/// `scheduleLinkSync`'s intent: an explicit target, or follow whatever the
/// substrate holds at run time.
#[derive(Clone, Debug)]
enum LinkIntent {
    Keep,
    Target(Option<epdf_annotation_core::PdfLinkTarget>),
}

/// `commitScriptEffects`: the script patch as the engine's per-kind patch
/// vocabulary.
fn engine_script_patch(subtype: &str, patch: &Value) -> Result<Value, Error> {
    let mut out = Map::new();
    out.insert("subtype".into(), json!(subtype));
    let to_engine_color = |c: &Value| -> Value {
        let rgb = serde_json::from_value::<ScriptColorArray>(c.clone())
            .ok()
            .and_then(|c| script_color_to_rgb(&c));
        match rgb {
            Some(rgb) => json!({
                "r": num_json(epdf_geometry::js::round(rgb.r * 255.0)),
                "g": num_json(epdf_geometry::js::round(rgb.g * 255.0)),
                "b": num_json(epdf_geometry::js::round(rgb.b * 255.0)),
            }),
            None => Value::Null,
        }
    };
    if let Some(stroke) = get(patch, "strokeColor").filter(|v| truthy(Some(v))) {
        let color = to_engine_color(stroke);
        if color.is_null() {
            return Err(Error::new("transparent stroke colours are not supported"));
        }
        out.insert("color".into(), color);
    }
    if let Some(fill) = get(patch, "fillColor").filter(|v| truthy(Some(v))) {
        out.insert("interiorColor".into(), to_engine_color(fill));
    }
    if let Some(v) = get(patch, "opacity") {
        out.insert("opacity".into(), v.clone());
    }
    if let Some(v) = get(patch, "width") {
        out.insert("strokeWidth".into(), v.clone());
    }
    if let Some(v) = get(patch, "borderStyle").filter(|v| truthy(Some(v))) {
        let style = if v.as_str() == Some("D") { "dashed" } else { "solid" };
        out.insert("borderStyle".into(), json!(style));
    }
    if let Some(v) = get(patch, "dash").filter(|v| truthy(Some(v))) {
        out.insert("dashArray".into(), v.clone());
    }
    if let Some(r) = get(patch, "rect").filter(|v| truthy(Some(v))) {
        let at = |i: usize| r.get(i).cloned().unwrap_or(Value::Null);
        out.insert("rect".into(), json!({ "left": at(0), "bottom": at(1), "right": at(2), "top": at(3) }));
    }
    if let Some(v) = get(patch, "contents") {
        out.insert("contents".into(), v.clone());
    }
    if let Some(v) = get(patch, "flags").filter(|v| truthy(Some(v))) {
        out.insert("flags".into(), v.clone());
    }
    Ok(Value::Object(out))
}

// ── the public surface, in the TypeScript object literal's order ─────────────

impl AnnotationCapability {
    /// `create(pon, draft)`: create on a page; resolves to the durable ref.
    /// An absent `/F` defaults to `print`.
    pub fn create(&self, pon: PageObjectNumber, draft: Value) -> Promise<Value> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let draft = if truthy(get(&draft, "flags")) {
                draft
            } else {
                let mut d = draft.as_object().cloned().unwrap_or_default();
                d.insert("flags".into(), json!({ "print": true }));
                Value::Object(d)
            };
            let call = Self::page_call(&doc, pon, "create", vec![draft])?;
            let res = await_engine(call).await?;
            let created = get(&res, "created").cloned().unwrap_or(Value::Null);
            let source =
                if str_of(&created, "subtype") == Some("stamp") { Source::Baked } else { Source::Vector };
            this.sync_dto(&created, source, false);
            Ok(get(&created, "ref").cloned().unwrap_or_else(epdf_kernel::undefined))
        })
    }

    /// `armStamp(input)`: validate and arm a stamp payload, then activate the
    /// `stamp` tool.
    pub fn arm_stamp(&self, input: StampToolInput) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let resolved = resolve_binary_source(&input.source).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                return Err(Error::new(
                    "[annotation] stamp source must be PNG, JPEG, or single-page PDF bytes",
                ));
            };
            let preview = match &input.preview {
                Some(p) if truthy(Some(p)) => {
                    let p = resolve_binary_source(p).await?;
                    Some(ArmedStampPreview { bytes: p.bytes, mime_type: p.mime_type })
                }
                _ if meta.mime_type != "application/pdf" => Some(ArmedStampPreview {
                    bytes: resolved.bytes.clone(),
                    mime_type: Some(meta.mime_type.to_string()),
                }),
                _ => None,
            };
            let size = Self::desired_stamp_size(&meta, input.target_width, input.intrinsic_size);
            *this.0.armed.borrow_mut() =
                Some(ArmedStamp { source: input.source, width: size.width, height: size.height, preview });
            this.ctx().dispatch(AnnotationAction::StampArmChanged);
            if let Some(ix) = this.ctx().try_get(&INTERACTION)? {
                ix.activate_tool("stamp")?;
            }
            Ok(())
        })
    }

    /// `disarmStamp()`.
    pub fn disarm_stamp(&self) {
        if self.0.armed.borrow().is_none() {
            return;
        }
        *self.0.armed.borrow_mut() = None;
        self.ctx().dispatch(AnnotationAction::StampArmChanged);
    }

    /// `placeArmedStamp(pon, point, displayRotation)` (host).
    pub fn place_armed_stamp(&self, pon: PageObjectNumber, point: Point, display_rotation: Option<f64>) -> bool {
        let Some(armed) = self.0.armed.borrow().clone() else { return false };
        let rot = self.upright_rot_for(display_rotation);
        self.create_stamp_at(
            pon,
            point,
            armed.source,
            Size { width: armed.width, height: armed.height },
            rot,
        )
    }

    /// `requestStampAt(pon, point, displayRotation)` (host).
    pub fn request_stamp_at(&self, pon: PageObjectNumber, point: Point, display_rotation: Option<f64>) -> bool {
        let Some(tool) = self.active_tool() else { return false };
        let Some(spec) = tool.source.clone() else { return false };
        let rot = match display_rotation {
            Some(dr) if tool.upright && dr != 0.0 && !dr.is_nan() => upright_rotation(dr),
            _ => 0.0,
        };
        match spec {
            StampSourceSpec::Bytes { source } => {
                let _ = self.place_stamp_source(pon, point, source, rot);
                true
            }
            StampSourceSpec::Prompt(_) => self.prompt_file_at(&tool, pon, point, move |this, picked| {
                let data = get(&picked, "data").cloned().unwrap_or_else(epdf_kernel::undefined);
                let _ = this.place_stamp_source(pon, point, data, rot);
            }),
        }
    }

    /// `placeAt(pon, point, displayRotation)` (host): the one click-to-place
    /// entry.
    pub fn place_at(&self, pon: PageObjectNumber, point: Point, display_rotation: Option<f64>) -> bool {
        if self.place_armed_stamp(pon, point, display_rotation) {
            return true;
        }
        let Some(tool) = self.active_tool() else { return false };
        if tool.subtype == "stamp" {
            return self.request_stamp_at(pon, point, display_rotation);
        }
        if !is_icon_place_kind(&tool.subtype) {
            return false;
        }
        let rot = match display_rotation {
            Some(dr) if tool.upright && dr != 0.0 && !dr.is_nan() => upright_rotation(dr),
            _ => 0.0,
        };
        if tool.subtype == "text" {
            return self.create_icon_at(&tool, pon, point, rot, None);
        }
        let placing = tool.clone();
        self.prompt_file_at(&tool, pon, point, move |this, picked| {
            this.create_icon_at(&placing, pon, point, rot, Some(picked));
        })
    }

    /// `hasArmedStamp()` (host).
    pub fn has_armed_stamp(&self) -> bool {
        self.0.armed.borrow().is_some()
    }

    /// `ghostHoverAt(toolId, pon, point, displayRotation)` (host).
    pub fn ghost_hover_at(&self, tool_id: &str, pon: PageObjectNumber, point: Point, display_rotation: Option<f64>) {
        let tool = self.registry_get(tool_id);
        let crop = self.crop_of(pon);
        let (Some(tool), Some(crop)) = (tool, crop) else {
            self.clear_ghost();
            return;
        };
        if tool.ghost == GhostPolicy::Off {
            self.clear_ghost();
            return;
        }
        let page = Self::page_size(crop);
        let armed = self.0.armed.borrow().clone();
        if let Some(armed) = armed {
            let rot = self.upright_rot_for(display_rotation);
            let bx = fit_stamp_box(point, Size { width: armed.width, height: armed.height }, page, rot);
            self.ctx().dispatch(AnnotationAction::SetToolGhost {
                ghost: Some(Rc::new(ToolGhost { pon, box_: bx, rot, kind: ToolGhostKind::Image })),
            });
            return;
        }
        if is_icon_place_kind(&tool.subtype) {
            let rot = self.upright_rot_for(display_rotation);
            let bx = fit_stamp_box(point, ICON_PLACE_SIZE, page, rot);
            self.dispatch_vector_ghost(pon, tool_id, Geom::Rect { rect: bx, ellipse: false, rot: None });
            return;
        }
        let Some(policy) = tool.click_policy() else {
            self.clear_ghost();
            return;
        };
        let placement = resolve_click_placement(
            point,
            &policy,
            &PlacementOptions {
                page_box: Some(Rect { x: 0.0, y: 0.0, width: page.width, height: page.height }),
                upright: Some(tool.upright),
                display_rotation,
            },
        );
        match click_create_geom(&tool.subtype, &placement, &defaults_for(&self.model(), &tool.preset)) {
            Some(geom) => self.dispatch_vector_ghost(pon, tool_id, geom),
            None => self.clear_ghost(),
        }
    }

    /// `clearGhost()` (host).
    pub fn clear_ghost(&self) {
        if self.state().tool_ghost.is_some() {
            self.ctx().dispatch(AnnotationAction::SetToolGhost { ghost: None });
        }
    }

    /// `setPlacementPreview(toolId, pon, box)` (host): the page-clamped box
    /// an externally owned creation gesture would commit.
    pub fn set_placement_preview(&self, tool_id: &str, pon: PageObjectNumber, bx: Rect) {
        let Some(crop) = self.crop_of(pon) else { return };
        let page = Self::page_size(crop);
        use epdf_geometry::js::{max, min};
        let x = max(0.0, min(bx.x, page.width));
        let y = max(0.0, min(bx.y, page.height));
        let rect = Rect {
            x,
            y,
            width: max(0.0, min(bx.x + bx.width, page.width) - x),
            height: max(0.0, min(bx.y + bx.height, page.height) - y),
        };
        self.dispatch_vector_ghost(pon, tool_id, Geom::Rect { rect, ellipse: false, rot: None });
    }

    /// `clearPlacementPreview()` (host).
    pub fn clear_placement_preview(&self) {
        self.clear_ghost();
    }

    /// `toolGhost(pon)` (host).
    pub fn tool_ghost(&self, pon: PageObjectNumber) -> Option<Rc<ToolGhost>> {
        self.state().tool_ghost.clone().filter(|g| g.pon == pon)
    }

    /// `armedStampPreview()` (host).
    pub fn armed_stamp_preview(&self) -> Option<ArmedStampPreview> {
        self.0.armed.borrow().as_ref().and_then(|a| a.preview.clone())
    }

    /// `stampArmEpoch()` (host).
    pub fn stamp_arm_epoch(&self) -> f64 {
        self.state().stamp_arm_epoch
    }

    /// `setFilePickerProvider(provider)`: one slot, last write wins.
    pub fn set_file_picker_provider(&self, provider: Option<FilePickerProvider>) {
        *self.0.file_picker.borrow_mut() = provider;
    }

    /// `downloadAttachment(ref)`. The TypeScript feature-detects
    /// `downloadFile` on the page service; the Rust engine contract has no
    /// member test for page services, so the call is always made.
    pub fn download_attachment(&self, r: AnnotationRef) -> Promise<Value> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let pon = this
                .pon_for_ref(&r)
                .ok_or_else(|| Error::new("[annotation] cannot resolve page for ref"))?;
            let call = Self::page_call(&doc, pon, "downloadFile", vec![ref_json(&r)])?;
            // `return annotations.downloadFile(ref)`: an async function
            // resolving with a thenable adopts it.
            rt::tick().await;
            call.await
        })
    }

    /// `tools()` (host): every resolved tool, in registry order.
    pub fn tools(&self) -> Vec<ResolvedTool> {
        self.0.registry.borrow().values().cloned().collect()
    }

    /// `tool(id)` (host).
    pub fn tool(&self, id: &str) -> Option<ResolvedTool> {
        self.registry_get(id)
    }

    /// `toolSubtype(id)` (host): the routing subtype, else the id.
    pub fn tool_subtype(&self, id: &str) -> String {
        self.registry_get(id).map_or_else(|| id.to_string(), |t| t.subtype)
    }

    /// `registerTool(def)`: register (or replace by id) at runtime; the
    /// returned `Unregister` removes it again.
    pub fn register_tool(&self, def: AnnotationToolDef) -> Result<Unregister, Error> {
        let mut all = self.0.config_tools.clone();
        all.push(def.clone());
        let registry = build_tool_registry(&all)?;
        let resolved = registry
            .get(&def.id)
            .cloned()
            .ok_or_else(|| Error::new(format!("[annotation] could not resolve tool '{}'", def.id)))?;
        self.0.registry.borrow_mut().set(resolved.clone());
        let un = match self.ctx().try_get(&INTERACTION)? {
            Some(ix) => {
                let mut tool = Tool::new(
                    resolved.id.clone(),
                    resolved.cursor.clone(),
                    resolved.enables.iter().map(String::as_str),
                );
                tool.touch_direct = Some(is_touch_direct(&resolved.enables));
                Some(ix.register_tool(tool))
            }
            None => None,
        };
        if let Some(defaults) = resolved.defaults.clone() {
            self.apply(Msg::SetDefaults { subtype: resolved.preset.clone(), patch: defaults });
        }
        let weak = Rc::downgrade(&self.0);
        let id = resolved.id.clone();
        Ok(Unregister::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.registry.borrow_mut().delete(&id);
            }
            if let Some(un) = &un {
                un.call();
            }
        }))
    }

    /// `update(ref, patch)`.
    pub fn update(&self, r: AnnotationRef, patch: Value) -> Promise<()> {
        self.update_one(r, patch)
    }

    /// `updateSelection(patch)`: one flat props patch over the selection.
    pub fn update_selection(&self, patch: AnnotationPropsPatch) {
        self.apply(Msg::SetProps { patch });
    }

    /// `updateSelectionFlags(patch)`.
    pub fn update_selection_flags(&self, patch: AnnotationFlagsPatch) {
        self.apply(Msg::SetFlags { patch, ids: None });
    }

    /// `getSelectionFlags()`: `None` with nothing selected.
    pub fn get_selection_flags(&self) -> Option<SelectionFlags> {
        let m = self.model();
        let members: Vec<&Annot> = m.selected.iter().filter_map(|id| m.by_id.get(id)).collect();
        if members.is_empty() {
            return None;
        }
        let flags: Vec<Value> =
            members.iter().map(|a| serde_json::to_value(a.flags).unwrap_or_default()).collect();
        let mut out = Map::new();
        for key in FLAG_KEYS {
            let first = flags[0].get(key).cloned().unwrap_or(Value::Null);
            let agree = flags.iter().all(|f| f.get(key) == Some(&first));
            out.insert(key.to_string(), if agree { first } else { Value::Null });
        }
        Some(out)
    }

    /// `delete(ref)`.
    pub fn delete(&self, r: AnnotationRef) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let pon = this
                .pon_for_ref(&r)
                .ok_or_else(|| Error::new("[annotation] cannot resolve page for ref"))?;
            let call = Self::page_call(&doc, pon, "delete", vec![ref_json(&r)])?;
            await_engine(call).await?;
            this.apply(Msg::Remove { ids: vec![ref_key(&r)] });
            Ok(())
        })
    }

    /// `canCreate()`: the caller's own create authority.
    pub fn can_create(&self) -> bool {
        self.allows_create()
    }

    /// `canEdit(ref)`: authority and flags, the fused transform predicate.
    pub fn can_edit(&self, r: &AnnotationRef) -> bool {
        self.model().by_id.get(&ref_key(r)).is_some_and(annot_transformable)
    }

    /// `canDelete(ref)`.
    pub fn can_delete(&self, r: &AnnotationRef) -> bool {
        self.model().by_id.get(&ref_key(r)).is_some_and(annot_deletable)
    }

    // ── comments (the `comments` member) ──

    /// `comments.threads()`.
    pub fn comment_threads(&self) -> Vec<CommentThread> {
        self.threads_index().0
    }

    /// `comments.thread(ref)`.
    pub fn comment_thread(&self, r: &AnnotationRef) -> Option<CommentThread> {
        self.thread_containing(r)
    }

    /// `comments.reply(ref, text)`: a flat `/IRT` to the thread root.
    pub fn comment_reply(&self, r: AnnotationRef, text: String) -> Promise<Value> {
        let this = self.clone();
        promise::spawn(async move {
            let t = this.thread_of(&r)?;
            let draft = json!({
                "subtype": "text",
                "rect": Self::root_rect(&t),
                "icon": "comment",
                "contents": text,
                "inReplyTo": Self::root_ref(&t),
                "flags": { "print": true, "noZoom": true, "noRotate": true },
            });
            let created = this.create_conversation_annot(Self::thread_pon(&t), draft).await?;
            Ok(get(&created, "ref").cloned().unwrap_or_else(epdf_kernel::undefined))
        })
    }

    /// `comments.edit(ref, text)`.
    pub fn comment_edit(&self, r: AnnotationRef, text: String) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let subtype = this
                .model()
                .by_id
                .get(&ref_key(&r))
                .and_then(|a| a.data.as_ref().and_then(|d| str_of(d, "subtype")).map(str::to_string))
                .filter(|s| !s.is_empty());
            let Some(subtype) = subtype else {
                return Err(Error::new("[annotation] cannot edit an uncommitted annotation"));
            };
            this.update_one(r, json!({ "subtype": subtype, "contents": text })).await
        })
    }

    /// `comments.setStatus(ref, state)`: this session's review status, chained
    /// to its previous one.
    pub fn comment_set_status(&self, r: AnnotationRef, state: String) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let t = this.thread_of(&r)?;
            let user = this.current_user_id();
            let previous = user.as_deref().and_then(|u| t.review.by_reviewer.get(u).map(|s| s.ref_.clone()));
            let draft = json!({
                "subtype": "text",
                "rect": Self::root_rect(&t),
                "inReplyTo": previous.unwrap_or_else(|| Self::root_ref(&t)),
                "state": state,
                "stateModel": "review",
                "flags": { "hidden": true, "noZoom": true, "noRotate": true },
            });
            this.create_conversation_annot(Self::thread_pon(&t), draft).await?;
            Ok(())
        })
    }

    /// `comments.setMarked(ref, marked)`.
    pub fn comment_set_marked(&self, r: AnnotationRef, marked: bool) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let t = this.thread_of(&r)?;
            let draft = json!({
                "subtype": "text",
                "rect": Self::root_rect(&t),
                "inReplyTo": Self::root_ref(&t),
                "state": if marked { "marked" } else { "unmarked" },
                "stateModel": "marked",
                "flags": { "hidden": true, "noZoom": true, "noRotate": true },
            });
            this.create_conversation_annot(Self::thread_pon(&t), draft).await?;
            Ok(())
        })
    }

    /// `comments.remove(ref)`: exactly one annotation.
    pub fn comment_remove(&self, r: Value) -> Promise<()> {
        self.delete_one(r)
    }

    /// `comments.removeThread(ref)`: children first, preflighted.
    pub fn comment_remove_thread(&self, r: AnnotationRef) -> Promise<ThreadDeleteResult> {
        let this = self.clone();
        promise::spawn(async move {
            let t = this.thread_of(&r)?;
            let members = Self::thread_member_refs(&t);
            let blocked: Vec<Value> = members.iter().filter(|m| !this.deletable_one(m)).cloned().collect();
            if !blocked.is_empty() {
                return Ok(ThreadDeleteResult {
                    deleted: Vec::new(),
                    failed: blocked
                        .into_iter()
                        .map(|r| ThreadDeleteFailure { ref_: r, error: Error::new("delete not permitted") })
                        .collect(),
                });
            }
            let mut deleted = Vec::new();
            let mut failed = Vec::new();
            for member in members {
                if !failed.is_empty() {
                    break;
                }
                match this.delete_one(member.clone()).await {
                    Ok(()) => deleted.push(member),
                    Err(error) => failed.push(ThreadDeleteFailure { ref_: member, error }),
                }
            }
            Ok(ThreadDeleteResult { deleted, failed })
        })
    }

    /// `comments.permissionsFor(ref)`.
    pub fn comment_permissions_for(&self, r: &AnnotationRef) -> CommentPermissions {
        let t = self.thread_containing(r);
        let m = self.model();
        let a = m.by_id.get(&ref_key(r));
        CommentPermissions {
            can_reply: self.allows_create(),
            can_set_status: self.allows_create(),
            can_edit_text: a.is_some_and(annot_contents_editable),
            can_delete: self.deletable_one(&ref_json(r)),
            can_delete_thread: t.is_some_and(|t| {
                Self::thread_member_refs(&t).iter().all(|m| self.deletable_one(m))
            }),
        }
    }

    // ── links (the `links` member) ──

    /// `links.of(ref)`.
    pub fn links_of(&self, r: &AnnotationRef) -> Option<epdf_annotation_core::PdfLinkTarget> {
        let m = self.model();
        let a = m.by_id.get(&ref_key(r))?;
        if a.subtype == "link" { a.link.as_option().cloned() } else { link_of(&m, &a.id) }
    }

    /// `links.set(ref, target)`: resolves when the children are committed.
    pub fn links_set(&self, r: &AnnotationRef, target: epdf_annotation_core::PdfLinkTarget) -> Promise<()> {
        self.schedule_link_sync(&ref_key(r), LinkIntent::Target(Some(target)))
    }

    /// `links.clear(ref)`.
    pub fn links_clear(&self, r: &AnnotationRef) -> Promise<()> {
        self.schedule_link_sync(&ref_key(r), LinkIntent::Target(None))
    }

    /// `getSelection()`: the selection as durable refs.
    pub fn get_selection(&self) -> Vec<AnnotationRef> {
        let m = self.model();
        m.selected.iter().filter_map(|id| m.by_id.get(id)?.ref_.clone()).collect()
    }

    /// `get(ref)`: the committed DTO, or `None`.
    pub fn get(&self, r: &AnnotationRef) -> Option<Value> {
        self.model().by_id.get(&ref_key(r))?.data.clone()
    }

    /// `list(pon)`: every committed annotation on a page, in z-order.
    pub fn list(&self, pon: PageObjectNumber) -> Vec<Value> {
        let m = self.model();
        m.order
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter(|a| a.pon == pon)
            .filter_map(|a| a.data.clone())
            .collect()
    }

    /// `getSelected()`.
    pub fn get_selected(&self) -> Vec<Value> {
        let m = self.model();
        m.selected.iter().filter_map(|id| m.by_id.get(id)?.data.clone()).collect()
    }

    /// `getSelectionProps()`.
    pub fn get_selection_props(&self) -> SelectionProps {
        let m = self.model();
        let members: Vec<&Annot> = m.selected.iter().filter_map(|id| m.by_id.get(id)).collect();
        let subtypes: Vec<String> = members.iter().map(|a| a.subtype.clone()).collect();
        let specs = shared_props(&subtypes);
        let value_of = |a: &Annot, key: PropKey| -> Option<Value> {
            if key == PropKey::Link && a.subtype != "link" {
                return Some(
                    link_of(&m, &a.id).map_or(Value::Null, |t| serde_json::to_value(t).unwrap_or_default()),
                );
            }
            read_prop(a, key).map(|v| serde_json::to_value(v).unwrap_or_default())
        };
        let mut values = Map::new();
        let mut mixed = Vec::new();
        for spec in &specs {
            let first = value_of(members[0], spec.key);
            if let Some(v) = &first {
                values.insert(spec.key.as_str().into(), v.clone());
            }
            if members.iter().any(|a| value_of(a, spec.key) != first) {
                mixed.push(spec.key);
            }
        }
        SelectionProps { specs, values, mixed }
    }

    /// `propsForTool(toolId)`: the specs of the tool's props kind.
    pub fn props_for_tool(&self, tool_id: &str) -> Vec<PropSpec> {
        let kind = self.registry_get(tool_id).map_or_else(|| tool_id.to_string(), |t| t.props_kind);
        props_for(&kind).to_vec()
    }

    /// `pageItems(pon, view)` (host): the render items, the vector ghost
    /// included; memoized by input identity.
    pub fn page_items(&self, pon: PageObjectNumber, view: Option<ViewEnv>) -> Vec<RenderItem> {
        let m = self.model();
        let g = self.state().tool_ghost.clone();
        let (zoom, rotation) = (view.map(|v| v.zoom), view.map(|v| v.rotation));
        if let Some(c) = self.0.caches.borrow().items.get(&pon)
            && Rc::ptr_eq(&c.model, &m)
            && same_rc(&c.ghost, &g)
            && same_num(c.zoom, zoom)
            && same_num(c.rotation, rotation)
        {
            return c.v.clone();
        }
        let mut v = page_items(&m, pon, view);
        if let Some(ghost) = g.as_ref().filter(|g| g.pon == pon)
            && let ToolGhostKind::Vector { tool_id, geom } = &ghost.kind
        {
            let tool = self.registry_get(tool_id);
            let preset = tool.as_ref().map_or_else(|| tool_id.clone(), |t| t.preset.clone());
            let style = style_from_props(&defaults_for(&m, &preset));
            v.push(RenderItem {
                id: "tool-ghost".into(),
                ref_: None,
                subtype: tool.map_or_else(|| "square".into(), |t| t.subtype),
                geom: geom.clone(),
                box_: ghost.box_,
                ap_box: None,
                ap_rot: None,
                style,
                text: None,
                source: RenderSource::Ghost,
                selected: false,
                hovered: None,
                label: None,
                rot: None,
                blend: None,
            });
        }
        self.0.caches.borrow_mut().items.insert(
            pon,
            ItemsCache { model: m, ghost: g, zoom, rotation, v: v.clone() },
        );
        v
    }

    /// `chrome(pon, scale, rotation, zoom)` (host).
    pub fn chrome(&self, pon: PageObjectNumber, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>) -> Vec<ChromeNode> {
        let m = self.model();
        let cs = self.chrome_settings_rc();
        if let Some(c) = self.0.caches.borrow().chrome.get(&pon)
            && Rc::ptr_eq(&c.model, &m)
            && Rc::ptr_eq(&c.cs, &cs)
            && same_num(c.scale, scale)
            && same_num(c.rotation, rotation)
            && same_num(c.zoom, zoom)
        {
            return c.v.clone();
        }
        let mut v = epdf_annotation_core::chrome(
            &m,
            pon,
            self.page_box_of(pon),
            Some(self.chrome_geom_at(scale, 1.0).knob_offset),
            view_env(zoom, rotation),
        );
        if !cs.guides.enabled {
            v.retain(|n| !matches!(n, ChromeNode::RotateGuides { .. }));
        }
        self.0.caches.borrow_mut().chrome.insert(
            pon,
            ChromeCache { model: m, cs, scale, rotation, zoom, v: v.clone() },
        );
        v
    }

    /// `selectionAnchor(scale, rotation, zoom)` (host).
    pub fn selection_anchor(&self, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>) -> Option<SelectionAnchor> {
        let m = self.model();
        let cs = self.chrome_settings_rc();
        if let Some(c) = self.0.caches.borrow().anchor.as_ref()
            && Rc::ptr_eq(&c.model, &m)
            && Rc::ptr_eq(&c.cs, &cs)
            && same_num(c.scale, scale)
            && same_num(c.rotation, rotation)
            && same_num(c.zoom, zoom)
        {
            return c.v.clone();
        }
        let this = self.clone();
        let page_box = move |pon: PageObjectNumber| this.page_box_of(pon);
        let offset = self.chrome_geom_at(scale, 1.0).knob_offset;
        let knob = move |_: PageObjectNumber| Some(offset);
        let view = view_env(zoom, rotation);
        let view_of = move |_: PageObjectNumber| view;
        let v = selection_anchor(&m, Some(&page_box), Some(&knob), Some(&view_of));
        self.0.caches.borrow_mut().anchor =
            Some(ChromeCache2 { model: m, cs, scale, rotation, zoom, v: v.clone() });
        v
    }

    /// `creationDraftAnchor()` (host).
    pub fn creation_draft_anchor(&self) -> Option<CreationDraftAnchor> {
        creation_draft_anchor(&self.model())
    }

    /// `selection()`: the selected ids.
    pub fn selection(&self) -> Vec<Id> {
        self.model().selected.clone()
    }

    fn hit(&self, pon: PageObjectNumber, point: Point, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>, boost: f64, inert: bool) -> Target {
        let m = self.model();
        let inert = if inert { self.inert_ids_at(pon) } else { None };
        hit_test(
            &m,
            pon,
            point,
            &self.chrome_geom_at(scale, boost),
            m.hit_margin,
            self.page_box_of(pon),
            inert.as_ref(),
            view_env(zoom, rotation),
        )
    }

    /// `hitKind(...)` (host): what is under a content point.
    #[allow(clippy::too_many_arguments)]
    pub fn hit_kind(&self, pon: PageObjectNumber, point: Point, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>, touch: Option<bool>) -> &'static str {
        match self.hit(pon, point, scale, rotation, zoom, Self::grab_boost(touch), true) {
            Target::Handle { .. } => "handle",
            Target::Rotate { .. } => "rotate",
            Target::GroupHandle { .. } => "group-handle",
            Target::Annot { .. } => "annot",
            Target::Empty => "empty",
        }
    }

    /// `claimsTouchAt(...)` (host): touch consent.
    pub fn claims_touch_at(&self, pon: PageObjectNumber, point: Point, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>) -> bool {
        let m = self.model();
        match self.hit(pon, point, scale, rotation, zoom, TOUCH_GRAB_BOOST, true) {
            Target::Handle { .. } | Target::Rotate { .. } | Target::GroupHandle { .. } => true,
            Target::Annot { id } => m.selected.contains(&id) && m.selected.iter().all(|s| can_move(&m, s)),
            Target::Empty => false,
        }
    }

    /// `cursorAt(...)` (host).
    pub fn cursor_at(&self, pon: PageObjectNumber, point: Point, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>) -> Option<String> {
        let m = self.model();
        let inert = self.inert_ids_at(pon);
        cursor_at(
            &m,
            pon,
            point,
            &self.chrome_geom_at(scale, 1.0),
            m.hit_margin,
            self.page_box_of(pon),
            inert.as_ref(),
            view_env(zoom, rotation),
        )
    }

    /// `hoverAt(at)` (host): dispatches on change only.
    pub fn hover_at(&self, at: Option<HoverAt>) {
        let m = self.model();
        let mut id: Option<String> = None;
        if let Some(at) = at
            && let Target::Annot { id: hit } =
                self.hit(at.pon, at.point, at.scale, at.rotation, at.zoom, 1.0, true)
        {
            id = Some(hit);
        }
        if m.hovered != id {
            if let Some(feed) = &self.0.hover_feed {
                feed.hover(id.as_deref());
            }
            self.apply(Msg::Hover { id });
        }
    }

    /// `behaviorFor(a)` (host).
    pub fn behavior_for(&self, subtype: &str, r: Option<&AnnotationRef>) -> Option<Rc<dyn Behavior>> {
        let behaviors = self.0.behaviors.borrow().clone();
        behaviors.into_iter().find(|b| b.matches(subtype, r) && b.engaged())
    }

    /// `linkItemsOn(pon)` (host): the navigation plane's feed.
    pub fn link_items_on(&self, pon: PageObjectNumber) -> Vec<LinkNavItem> {
        let m = self.model();
        let mut v = Vec::new();
        for id in &m.order {
            let Some(a) = m.by_id.get(id) else { continue };
            if a.pon != pon || a.subtype != "link" || !viewable(&a.flags, false) {
                continue;
            }
            let data = a.data.as_ref();
            let target = match &a.link {
                Nullable::Value(t) => serde_json::to_value(t).unwrap_or_default(),
                _ => match data {
                    Some(d) if str_of(d, "subtype") == Some("link") => {
                        get(d, "target").cloned().unwrap_or(Value::Null)
                    }
                    _ => Value::Null,
                },
            };
            let Geom::Rect { rect, .. } = a.geom else { continue };
            if target.is_null() {
                continue;
            }
            let actions = data.and_then(|d| get(d, "actions"));
            let activate = actions.and_then(|x| get(x, "activate")).filter(|v| truthy(Some(v))).cloned();
            let ref_ = a.ref_.clone().or_else(|| data.and_then(|d| get(d, "ref")).and_then(ref_from));
            let root = |k: &str| truthy(actions.and_then(|x| get(x, k)).and_then(|t| get(t, "root")));
            let (enter, exit) = (root("cursorEnter"), root("cursorExit"));
            v.push(LinkNavItem {
                id: id.clone(),
                rect,
                target,
                attached: a.group.is_some(),
                activate,
                ref_,
                hover_events: (enter || exit).then_some(HoverFlags { enter, exit }),
            });
        }
        v
    }

    /// `appearanceEpoch(pon)` (host): the committed id and `/AP` version of
    /// every baked annotation on the page.
    pub fn appearance_epoch(&self, pon: PageObjectNumber) -> String {
        let m = self.model();
        let mut parts: Vec<String> = Vec::new();
        for id in &m.order {
            let Some(a) = m.by_id.get(id) else { continue };
            if a.pon != pon || a.source != Source::Baked || a.ref_.is_none() || is_substrate_only(a) {
                continue;
            }
            parts.push(format!(
                "{id}@{}",
                epdf_geometry::js::number_to_string(a.ap_version.unwrap_or(0.0))
            ));
        }
        parts.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        parts.join("|")
    }

    /// `bakeScale(renderScale)` (host): the document's render policy
    /// snapped; identity under `continuous`.
    pub fn bake_scale(&self, render_scale: f64) -> Result<f64, Error> {
        let policy = self
            .ctx()
            .document()
            .and_then(|d| serde_json::from_value::<EngineRenderPolicy>(d.render_policy).ok())
            .unwrap_or(EngineRenderPolicy::Continuous);
        snap_appearance_scale(&policy, render_scale)
    }

    /// `appearances(pon, scale, signal)` (host): the baked `/AP` images, `[]`
    /// on any failure. `page(pon)` refuses an unknown page synchronously.
    pub fn appearances(&self, pon: PageObjectNumber, scale: f64, signal: Option<AbortSignal>) -> Result<Promise<Value>, Error> {
        let Some(doc) = self.doc() else { return Ok(Promise::resolved(json!([]))) };
        let task = Self::page_call(&doc, pon, "renderAppearanceImages", vec![json!({ "scale": num_json(scale) })])?;
        if let Some(signal) = signal {
            if signal.aborted() {
                task.abort(signal.reason().map(|r| r.to_canonical()));
            } else {
                let t = task.clone();
                signal.on_abort(Rc::new(move |reason: &Error| t.abort(Some(reason.to_canonical()))));
            }
        }
        let (out, resolver) = Promise::pending();
        then(task, move |result| match result {
            Ok(r) => resolver.resolve(get(&r, "appearances").cloned().unwrap_or_else(epdf_kernel::undefined)),
            Err(_) => resolver.resolve(json!([])),
        });
        Ok(out)
    }

    /// `toContentBox(pon, rect)` (host).
    pub fn to_content_box(&self, pon: PageObjectNumber, rect: PdfRect) -> Option<Rect> {
        self.crop_of(pon).map(|crop| pdf_to_content_rect(rect, crop))
    }

    /// `editPointer(...)` (host).
    #[allow(clippy::too_many_arguments)]
    pub fn edit_pointer(&self, phase: Phase, pon: PageObjectNumber, point: Point, shift: bool, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>, touch: Option<bool>) {
        let input = PointerInput {
            pon,
            point,
            shift,
            finish: None,
            page_box: self.page_box_of(pon),
            chrome: Some(self.chrome_geom_at(scale, Self::grab_boost(touch))),
            display_rotation: rotation,
            zoom,
            upright: None,
            inert: self.inert_ids_at(pon),
        };
        self.apply(Msg::EditPointer { phase, input });
    }

    /// `marqueePointer(...)` (host).
    #[allow(clippy::too_many_arguments)]
    pub fn marquee_pointer(&self, phase: Phase, pon: PageObjectNumber, point: Point, shift: bool, _scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>) {
        let input = PointerInput {
            pon,
            point,
            shift,
            finish: None,
            page_box: self.page_box_of(pon),
            chrome: None,
            display_rotation: rotation,
            zoom,
            upright: None,
            inert: self.inert_ids_at(pon),
        };
        self.apply(Msg::MarqueePointer { phase, input });
    }

    /// `createPointer(tool, phase, pon, point, finish, displayRotation)`
    /// (host): inert without create authority.
    pub fn create_pointer(&self, tool: &str, phase: Phase, pon: PageObjectNumber, point: Point, finish: bool, display_rotation: Option<f64>) {
        if !self.allows_create() {
            return;
        }
        let t = self.registry_get(tool);
        let ink = t.as_ref().and_then(|t| t.ink);
        self.apply(Msg::CreatePointer {
            phase,
            subtype: t.as_ref().map_or_else(|| tool.to_string(), |t| t.subtype.clone()),
            preset: Some(t.as_ref().map_or_else(|| tool.to_string(), |t| t.preset.clone())),
            intent: t
                .as_ref()
                .and_then(|t| t.intent.clone())
                .and_then(|i| serde_json::from_value::<Intent>(json!(i)).ok()),
            click_create: t.as_ref().map(|t| t.click_create),
            flags: t.as_ref().and_then(|t| t.flags),
            defer_ink_commit: Some(ink.and_then(|i| i.group_strokes_ms).unwrap_or(0.0) > 0.0),
            straighten_ink: ink.and_then(|i| i.straighten),
            input: PointerInput {
                pon,
                point,
                shift: false,
                finish: Some(finish),
                page_box: self.page_box_of(pon),
                chrome: None,
                display_rotation,
                zoom: None,
                upright: t.as_ref().map(|t| t.upright),
                inert: None,
            },
        });
    }

    /// `finishCreationDraft()`.
    pub fn finish_creation_draft(&self) {
        self.apply(Msg::FinishCreationDraft);
    }

    /// `finishInkDraft()` (host).
    pub fn finish_ink_draft(&self) {
        self.apply(Msg::FinishInkDraft);
    }

    /// `cancelCreationDraft()`.
    pub fn cancel_creation_draft(&self) {
        self.apply(Msg::Cancel);
    }

    /// `markupFromSelection(subtype, preset)`: one markup per selected page,
    /// then clear the selection.
    pub fn markup_from_selection(&self, subtype: &str, preset: Option<&str>) -> Result<bool, Error> {
        if !self.allows_create() {
            return Ok(false);
        }
        let Some(selection) = self.ctx().try_get(&SELECTION)? else { return Ok(false) };
        if !selection.has_selection() {
            return Ok(false);
        }
        let snapshot = selection.snapshot();
        let mut created = false;
        for page in &snapshot.pages {
            if page.segments.is_empty() {
                continue;
            }
            self.apply(Msg::CreateMarkup {
                subtype: subtype.to_string(),
                pon: page.pon,
                quads: page.segments.iter().map(|s| s.quad).collect(),
                preset: preset.map(str::to_string),
                flags: preset.and_then(|p| self.registry_get(p)).and_then(|t| t.flags),
            });
            created = true;
        }
        if created {
            selection.clear();
        }
        Ok(created)
    }

    /// `createMarkup(subtype, pon, quads, preset)` (host).
    pub fn create_markup(&self, subtype: &str, pon: PageObjectNumber, quads: Vec<TextQuad>, preset: Option<&str>) {
        if !self.allows_create() {
            return;
        }
        self.apply(Msg::CreateMarkup {
            subtype: subtype.to_string(),
            pon,
            quads,
            preset: preset.map(str::to_string),
            flags: preset.and_then(|p| self.registry_get(p)).and_then(|t| t.flags),
        });
    }

    /// `createCaret(pon, anchor)` (host).
    pub fn create_caret(&self, pon: PageObjectNumber, anchor: TextEndAnchor) {
        if !self.allows_create() {
            return;
        }
        self.apply(Msg::CreateCaret { pon, anchor, flags: None });
    }

    /// `createReplaceText(pon, quads, anchor, preset)` (host).
    pub fn create_replace_text(&self, pon: PageObjectNumber, quads: Vec<TextQuad>, anchor: TextEndAnchor, preset: Option<&str>) {
        if !self.allows_create() {
            return;
        }
        self.apply(Msg::CreateReplaceText { pon, quads, anchor, preset: preset.map(str::to_string) });
    }

    /// `previewMarkup(subtype, quadsByPage, preset)` (host).
    pub fn preview_markup(&self, subtype: &str, quads_by_page: Record<Vec<TextQuad>>, preset: Option<&str>) {
        self.apply(Msg::SetMarkupPreview {
            subtype: subtype.to_string(),
            quads_by_page,
            preset: preset.map(str::to_string),
        });
    }

    /// `clearMarkupPreview()` (host).
    pub fn clear_markup_preview(&self) {
        self.apply(Msg::ClearMarkupPreview);
    }

    /// `setDefaults(subtype, patch)`.
    pub fn set_defaults(&self, subtype: &str, patch: AnnotationPropsPatch) {
        self.apply(Msg::SetDefaults { subtype: subtype.to_string(), patch });
    }

    /// `currentDefaults(toolId)`: through the tool's preset.
    pub fn current_defaults(&self, tool_id: &str) -> AnnotationProps {
        let preset = self.registry_get(tool_id).map_or_else(|| tool_id.to_string(), |t| t.preset);
        defaults_for(&self.model(), &preset)
    }

    /// `setSnap(patch)`.
    pub fn set_snap(&self, patch: SnapSettingsPatch) {
        self.apply(Msg::SetSnap { patch });
    }

    /// `snapSettings()`.
    pub fn snap_settings(&self) -> SnapSettings {
        self.model().snap.clone()
    }

    /// `setChrome(patch)`.
    pub fn set_chrome(&self, patch: ChromeSettingsPatch) {
        self.ctx().dispatch(AnnotationAction::SetChrome { patch });
    }

    /// `chromeSettings()`.
    pub fn chrome_settings(&self) -> ChromeSettings {
        (*self.chrome_settings_rc()).clone()
    }

    /// `deleteSelection()`.
    pub fn delete_selection(&self) {
        self.apply(Msg::Delete);
    }

    /// `deselect()`.
    pub fn deselect(&self) {
        self.apply(Msg::Deselect { ids: None });
    }

    /// `select(ref, { add })`.
    pub fn select(&self, r: &AnnotationRef, add: Option<bool>) {
        self.apply(Msg::Select { ids: vec![ref_key(r)], add });
    }

    /// `pruneEngagedSelection()` (host).
    pub fn prune_engaged_selection(&self) {
        let m = self.model();
        let drop: Vec<Id> = m
            .selected
            .iter()
            .filter(|id| m.by_id.get(*id).is_some_and(|a| self.engaged_for(a)))
            .cloned()
            .collect();
        if !drop.is_empty() {
            self.apply(Msg::Deselect { ids: Some(drop) });
        }
    }

    /// `cancel()`.
    pub fn cancel(&self) {
        self.apply(Msg::Cancel);
    }

    /// `rotateSelection90()`.
    pub fn rotate_selection_90(&self) {
        self.apply(Msg::Rotate90);
    }

    /// `resetSelectionRotation()`.
    pub fn reset_selection_rotation(&self) {
        self.apply(Msg::ResetRotation);
    }

    /// `group()`: the bottom-most member becomes the primary.
    pub fn group(&self) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let m = this.model();
            let members = this.selected_committed();
            if members.len() < 2 {
                return Ok(());
            }
            let pon = members[0].pon;
            if members.iter().any(|a| a.pon != pon) {
                return Ok(());
            }
            let index = |a: &Annot| m.order.iter().position(|id| *id == a.id).map_or(-1.0, |i| i as f64);
            let mut ordered = members.clone();
            ordered.sort_by(|a, b| {
                (index(a) - index(b)).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            let primary = ordered[0].clone();
            let Some(primary_ref) = primary.ref_.clone() else { return Ok(()) };
            let writes = ordered[1..]
                .iter()
                .map(|a| this.write_relationship(a.clone(), Some(primary_ref.clone()), true))
                .collect();
            promise::all(writes).await?;
            Ok(())
        })
    }

    /// `ungroup()`: every subordinate of a touched group becomes top-level.
    pub fn ungroup(&self) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let m = this.model();
            let subs: Vec<Annot> = expand_groups(&m, &m.selected)
                .iter()
                .filter_map(|id| m.by_id.get(id))
                .filter(|a| a.ref_.is_some() && a.data.is_some() && a.group.is_some())
                .cloned()
                .collect();
            let writes = subs.into_iter().map(|a| this.write_relationship(a, None, false)).collect();
            promise::all(writes).await?;
            Ok(())
        })
    }

    /// `canGroup()`.
    pub fn can_group(&self) -> bool {
        let m = self.model();
        let members = self.selected_committed();
        if members.len() < 2 {
            return false;
        }
        if members.iter().any(|a| a.pon != members[0].pon) {
            return false;
        }
        if !members.iter().all(|a| a.ref_.as_ref().is_some_and(|r| self.allows_mutation("update", r))) {
            return false;
        }
        let mut keys: Vec<Option<Id>> = Vec::new();
        for id in &m.selected {
            let k = group_key_of(&m, id);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        !(keys.len() == 1 && !keys.contains(&None))
    }

    /// `canUngroup()`.
    pub fn can_ungroup(&self) -> bool {
        let m = self.model();
        let subs: Vec<&Annot> = expand_groups(&m, &m.selected)
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter(|a| a.group.is_some())
            .collect();
        !subs.is_empty()
            && subs.iter().all(|a| a.ref_.as_ref().is_some_and(|r| self.allows_mutation("update", r)))
    }

    /// `ensurePage(pon)` (host): a no-op since whole-document hydration.
    pub fn ensure_page(&self, _pon: PageObjectNumber) {}

    /// `reloadPage(pon)` (host): drop and re-read one page.
    pub fn reload_page(&self, pon: PageObjectNumber) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let (Some(doc), Some(crop)) = (this.doc(), this.crop_of(pon)) else { return Ok(()) };
            let run = async {
                let call = Self::page_call(&doc, pon, "list", Vec::new())?;
                let snap = await_engine(call).await?;
                let m = this.model();
                let stale: Vec<Id> =
                    m.order.iter().filter(|id| m.by_id.get(*id).is_some_and(|a| a.pon == pon)).cloned().collect();
                if !stale.is_empty() {
                    this.apply(Msg::Remove { ids: stale });
                }
                let annots = get(&snap, "annotations")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|d| this.ingest_dto(d, crop, Source::Baked))
                    .collect();
                this.apply(Msg::Loaded { annots });
                Ok::<(), Error>(())
            };
            let _ = run.await;
            Ok(())
        })
    }

    /// `hydration()` (host).
    pub fn hydration(&self) -> AnnotationHydration {
        self.state().hydration.clone()
    }

    /// `ensureHydrated()` (host): once per document.
    pub fn ensure_hydrated(&self) {
        if self.0.hydration.started.get() {
            return;
        }
        self.0.hydration.started.set(true);
        let _ = self.run_hydration(false);
    }

    /// `deliverRemoteAnnotationEvent(event)` (host): queued during a
    /// hydration window, applied otherwise.
    pub fn deliver_remote_annotation_event(&self, event: Value) {
        if self.0.hydration.window.get() {
            self.0.hydration.pending.borrow_mut().push(event);
            return;
        }
        self.apply_remote_annotation_event(&event);
    }

    /// `textItems(pon, view)` (host).
    pub fn text_items(&self, pon: PageObjectNumber, view: Option<ViewEnv>) -> Vec<TextItem> {
        build_text_items(&self.model(), pon, view)
    }

    /// `currentEditing()` (host).
    pub fn current_editing(&self) -> Option<Id> {
        self.model().editing.clone()
    }

    /// `beginTextEdit(ref)` (host).
    pub fn begin_text_edit(&self, r: &AnnotationRef) {
        self.apply(Msg::BeginTextEdit { id: ref_key(r) });
    }

    /// `beginTextEditAt(...)` (host): whether a free-text box was opened.
    pub fn begin_text_edit_at(&self, pon: PageObjectNumber, point: Point, scale: Option<f64>, rotation: Option<f64>, zoom: Option<f64>) -> bool {
        let m = self.model();
        let id = match self.hit(pon, point, scale, rotation, zoom, 1.0, false) {
            Target::Annot { id } | Target::Handle { id, .. } => Some(id),
            _ => None,
        };
        if let Some(id) = id
            && m.by_id.get(&id).is_some_and(|a| matches!(a.geom, Geom::Text { .. }))
        {
            self.apply(Msg::BeginTextEdit { id });
            return true;
        }
        false
    }

    /// `setContents(ref, text)` (host): optimistic now, debounced to the
    /// engine.
    pub fn set_contents(&self, r: AnnotationRef, text: String) {
        self.apply(Msg::SetText { id: ref_key(&r), text: text.clone() });
        let key = ref_key(&r);
        let previous = self.0.text_timers.borrow().iter().find(|(k, _)| *k == key).map(|(_, t)| *t);
        if let Some(t) = previous {
            rt::clear_timer(t);
        }
        let weak = Rc::downgrade(&self.0);
        let timer_key = key.clone();
        let timer = rt::set_timeout(TEXT_COMMIT_DEBOUNCE_MS, move || {
            let Some(inner) = weak.upgrade() else { return };
            let this = AnnotationCapability(inner);
            this.0.text_timers.borrow_mut().retain(|(k, _)| *k != timer_key);
            if let (Some(pon), Some(doc)) = (this.pon_for_ref(&r), this.doc()) {
                let _ = Self::page_call(
                    &doc,
                    pon,
                    "update",
                    vec![ref_json(&r), json!({ "subtype": "free-text", "contents": text })],
                );
            }
        });
        let mut timers = self.0.text_timers.borrow_mut();
        match timers.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = timer,
            None => timers.push((key, timer)),
        }
    }

    /// `endTextEdit()` (host): flush and leave edit mode.
    pub fn end_text_edit(&self) {
        let timers: Vec<TimerId> = self.0.text_timers.borrow().iter().map(|(_, t)| *t).collect();
        for t in timers {
            rt::clear_timer(t);
        }
        let m = self.model();
        let a = m.editing.as_ref().and_then(|id| m.by_id.get(id)).cloned();
        if let Some(a) = a
            && let Some(r) = a.ref_.clone()
        {
            let pon = self.pon_for_ref(&r);
            let text = a
                .data
                .as_ref()
                .and_then(|d| get(d, "contents"))
                .filter(|c| !c.is_null())
                .cloned()
                .unwrap_or_else(|| json!(""));
            if let (Some(pon), Some(doc)) = (pon, self.doc()) {
                let _ = Self::page_call(
                    &doc,
                    pon,
                    "update",
                    vec![ref_json(&r), json!({ "subtype": "free-text", "contents": text })],
                );
            }
        }
        self.0.text_timers.borrow_mut().clear();
        self.apply(Msg::EndTextEdit);
    }

    /// `commitScriptEffects(entries)` (host): the actions plane's document
    /// commit door. Per entry, stop on failure; reconciles its own model.
    pub fn commit_script_effects(&self, entries: Vec<AnnotCommitEntry>) -> Promise<AnnotCommitResult> {
        let this = self.clone();
        promise::spawn(async move {
            let entry_result = |n: f64, status: AnnotCommitStatus, error: Option<String>| AnnotCommitResultEntry {
                annot_object_number: n,
                status,
                error,
            };
            let Some(doc) = this.doc() else {
                return Ok(AnnotCommitResult {
                    results: entries
                        .iter()
                        .map(|e| entry_result(e.annot_object_number, AnnotCommitStatus::Failed, Some("no document".into())))
                        .collect(),
                });
            };
            let mut results = Vec::new();
            let mut touched: Vec<PageObjectNumber> = Vec::new();
            let mut failed = false;
            for entry in &entries {
                let n = entry.annot_object_number;
                if failed {
                    results.push(entry_result(n, AnnotCommitStatus::Skipped, None));
                    continue;
                }
                let key = format!("obj:{}", epdf_geometry::js::number_to_string(n));
                let loaded = this.model().by_id.get(&key).cloned();
                let pon: Option<PageObjectNumber> = loaded
                    .as_ref()
                    .map(|a| a.pon)
                    .or(entry.page_object_number.map(|p| p as PageObjectNumber));
                let mut r: Option<Value> = loaded.as_ref().and_then(|a| a.ref_.as_ref().map(ref_json));
                let mut subtype: Option<String> = loaded.as_ref().map(|a| a.subtype.clone());
                if (r.is_none() || subtype.as_deref().is_none_or(str::is_empty))
                    && let Some(pon) = pon
                {
                    let listed = async {
                        let call = Self::page_call(&doc, pon, "list", Vec::new())?;
                        await_engine(call).await
                    };
                    if let Ok(listed) = listed.await {
                        let found = get(&listed, "annotations").and_then(Value::as_array).and_then(|list| {
                            list.iter()
                                .find(|c| {
                                    let cr = get(c, "ref");
                                    cr.and_then(|x| str_of(x, "kind")) == Some("objectNumber")
                                        && cr.and_then(|x| get(x, "annotObjectNumber")).and_then(Value::as_f64)
                                            == Some(n)
                                })
                                .cloned()
                        });
                        if let Some(dto) = found {
                            r = get(&dto, "ref").cloned();
                            subtype = str_of(&dto, "subtype").map(str::to_string);
                        }
                    }
                }
                let (Some(pon), Some(r), Some(subtype)) =
                    (pon, r.filter(|v| truthy(Some(v))), subtype.filter(|s| !s.is_empty()))
                else {
                    results.push(entry_result(
                        n,
                        AnnotCommitStatus::Failed,
                        Some("annotation not resolved (page not loaded)".into()),
                    ));
                    continue;
                };
                let patch = match engine_script_patch(&subtype, &entry.patch) {
                    Ok(p) => p,
                    Err(e) => {
                        results.push(entry_result(n, AnnotCommitStatus::Failed, Some(e.message)));
                        continue;
                    }
                };
                let write = async {
                    let call = Self::page_call(&doc, pon, "update", vec![r.clone(), patch])?;
                    await_engine(call).await
                };
                match write.await {
                    Ok(_) => {
                        if !touched.contains(&pon) {
                            touched.push(pon);
                        }
                        results.push(entry_result(n, AnnotCommitStatus::Applied, None));
                    }
                    Err(error) => {
                        failed = true;
                        results.push(entry_result(n, AnnotCommitStatus::Failed, Some(error.message)));
                    }
                }
            }
            for pon in touched {
                this.reload_page(pon).await?;
            }
            Ok(AnnotCommitResult { results })
        })
    }

    /// `registerBehavior(b)` (host): the returned function unregisters it.
    pub fn register_behavior(&self, b: Rc<dyn Behavior>) -> Unregister {
        self.0.behaviors.borrow_mut().push(b.clone());
        let weak = Rc::downgrade(&self.0);
        Unregister::new(move || {
            if let Some(inner) = weak.upgrade() {
                let mut behaviors = inner.behaviors.borrow_mut();
                if let Some(i) = behaviors.iter().position(|x| Rc::ptr_eq(x, &b)) {
                    behaviors.remove(i);
                }
            }
        })
    }
}

/// `hoverAt`'s argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverAt {
    pub pon: PageObjectNumber,
    pub point: Point,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub zoom: Option<f64>,
}
