//! `controller.ts`: the selection controller. One object is both the public
//! lens and the host lens.
//!
//! The model holds the range and its derived page-space segments; the
//! per-page caches live here, split by what invalidates them:
//! `raw_geometry` (PDF-space snapshots, dropped when a page leaves the
//! registry or its content changes), `derived` (the transform and layout,
//! keyed by crop, rotation and `userUnit`, so a view rotation re-derives with
//! no refetch) and `text_snapshots` (for `read_text`).
//!
//! `publish` is the one place the selection changes and `on_changed` /
//! `on_cleared` fire. `recompute` rebuilds the segments of every loaded page
//! in the span, warms the rest (they recompute on arrival, with a `system`
//! origin), and clamps stored endpoints once their page's geometry is known,
//! which is how `select_all`'s open end settles.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use epdf_geometry::js::{self, number_to_string};
use epdf_geometry::{PageRotation, Point, Rect, text_quad_bounds};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde::Deserialize;
use serde_json::Value;

use crate::contract::*;
use crate::geometry::{
    SelectionPageGeometry, SelectionSegment, build_selection_page_geometry, content_point_to_pdf,
    to_content_segment, to_content_text_quad,
};
use crate::model::*;
use crate::text_layout::{
    DEFAULT_TOLERANCE_FACTOR, PageGeometrySnapshot, PageTextSnapshot, PdfRect,
    expand_text_range_to_line, expand_text_range_to_word, slice_text_by_chars, text_glyph_at,
    text_glyph_quad, text_segments_for_range,
};

const SELECT_SCOPE: &str = "doc.text.select";
const COPY_SCOPE: &str = "doc.text.copy";
/// How many page-text reads a `read_text` keeps in flight at once.
const TEXT_READ_CONCURRENCY: usize = 8;
/// `Number.MAX_SAFE_INTEGER`: an open-ended focus, which settles to the
/// page's real last character once its geometry loads.
pub const OPEN_END: i64 = 9_007_199_254_740_991;

/// `OperationOptions`: what the read calls take.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// The members of a `PageLayout` the controller reads.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageLayout {
    index: f64,
    rotation: PageRotation,
    user_unit: f64,
    boxes: PageBoxes,
}

#[derive(Clone, Debug, Deserialize)]
struct PageBoxes {
    crop: PdfRect,
}

/// The page registry, decoded.
#[derive(Default)]
struct Registry {
    /// Each row's page, in order, `None` where its ref fails to decode
    /// (`pages[i]?.ref.pageObjectNumber`). A malformed row costs only
    /// itself, as in the TypeScript.
    pons: Vec<Option<PageObjectNumber>>,
    /// Each row on its own, `None` where it fails to decode.
    rows: Vec<Option<PageLayout>>,
    /// The first row naming each page, as a linear scan finds it.
    by_pon: HashMap<PageObjectNumber, usize>,
}

impl Registry {
    fn decode(pages: &Value) -> Self {
        // Read in place: `&Value` is a deserializer, so the shared layouts
        // aren't copied first.
        let rows: Vec<Option<PageLayout>> = pages
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| serde::Deserialize::deserialize(row).ok())
            .collect();
        let pons: Vec<Option<PageObjectNumber>> = pages
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| row.get("ref").and_then(PageRef::from_json).map(|r| r.page_object_number))
            .collect();
        let mut by_pon = HashMap::with_capacity(rows.len());
        for (i, pon) in pons.iter().enumerate() {
            if let Some(pon) = pon {
                by_pon.entry(*pon).or_insert(i);
            }
        }
        Registry { pons, rows, by_pon }
    }

    fn layout_of(&self, pon: PageObjectNumber) -> Option<&PageLayout> {
        self.rows.get(*self.by_pon.get(&pon)?)?.as_ref()
    }
}

#[derive(Clone, Copy)]
enum Expand {
    Word,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Start,
    End,
}

struct Ordered {
    start: GlyphPointer,
    end: GlyphPointer,
    direction: SelectionDirection,
}

/// Everything the read model derives from one state object.
struct ReadModel {
    state: Rc<SelectionState>,
    snapshot: SelectionSnapshot,
    pages: Vec<PageRef>,
    anchor: Option<SelectionAnchor>,
}

struct Core {
    cx: Controller<SelectionState, SelectionAction>,
    raw_geometry: RefCell<HashMap<PageObjectNumber, Rc<PageGeometrySnapshot>>>,
    derived: RefCell<HashMap<PageObjectNumber, (String, Rc<SelectionPageGeometry>)>>,
    loads: RefCell<HashMap<PageObjectNumber, Promise<()>>>,
    text_snapshots: RefCell<HashMap<PageObjectNumber, Promise<Rc<PageTextSnapshot>>>>,
    /// Bumped on content invalidation; reads in flight from before are dropped.
    epoch: Cell<u64>,
    model: RefCell<Option<Rc<ReadModel>>>,
    /// The decoded registry and the page list it came from.
    registry: RefCell<Option<(Rc<Value>, Rc<Registry>)>>,
    changed: EventHookSource<SelectionChangedEvent>,
    committed: EventHookSource<SelectionCommittedEvent>,
    cleared: EventHookSource<SelectionClearedEvent>,
}

/// `SelectionHostCapability`, which includes the public
/// `SelectionCapability`. Cheap to clone; clones share the caches.
#[derive(Clone)]
pub struct SelectionCapability(Rc<Core>);

/// `Promise.all` over promises of one type: settles one turn after the last
/// element's reaction, or with the first rejection.
fn all<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<Vec<T>> {
    let (out, resolver) = Promise::pending();
    let slots: Rc<RefCell<Vec<Option<T>>>> = Rc::new(RefCell::new(vec![None; promises.len()]));
    let remaining = Rc::new(Cell::new(promises.len()));
    for (i, promise) in promises.into_iter().enumerate() {
        let (slots, remaining, resolver) = (slots.clone(), remaining.clone(), resolver.clone());
        rt::spawn(async move {
            match promise.await {
                Ok(value) => {
                    slots.borrow_mut()[i] = Some(value);
                    remaining.set(remaining.get() - 1);
                    if remaining.get() == 0 {
                        resolver.resolve(slots.borrow_mut().drain(..).flatten().collect());
                    }
                }
                Err(error) => resolver.reject(error),
            }
        });
    }
    out
}

fn permission_denied(operation: &str, scope: &str) -> Error {
    plugin_error(
        code::PERMISSION_DENIED,
        "selection",
        format!("{operation} requires {scope}"),
        None,
        Some(serde_json::json!({ "required": scope })),
    )
}

/// A content change landed while a text read was in flight: the pages
/// already read may hold text the change removed (a redaction). The
/// TypeScript returns that text; this is a Rust-only refusal.
fn throw_if_stale(at: u64, now: u64) -> Result<(), Error> {
    if at != now {
        return Err(plugin_error(
            code::CONFLICT,
            "selection",
            "selection.readText: the page content changed while it was read",
            None,
            None,
        ));
    }
    Ok(())
}

fn throw_if_aborted(signal: Option<&AbortSignal>) -> Result<(), Error> {
    if signal.is_some_and(AbortSignal::aborted) {
        return Err(plugin_error(
            code::OPERATION_CANCELLED,
            "selection",
            "selection.readText was cancelled",
            None,
            None,
        ));
    }
    Ok(())
}

fn union_of(segments: Option<&Vec<SelectionSegment>>) -> Option<Rect> {
    let segments = segments.filter(|s| !s.is_empty())?;
    let (mut x1, mut y1) = (f64::INFINITY, f64::INFINITY);
    let (mut x2, mut y2) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for s in segments {
        x1 = js::min(x1, s.rect.x);
        y1 = js::min(y1, s.rect.y);
        x2 = js::max(x2, s.rect.x + s.rect.width);
        y2 = js::max(y2, s.rect.y + s.rect.height);
    }
    Some(Rect { x: x1, y: y1, width: x2 - x1, height: y2 - y1 })
}

/// `createSelectionController(ctx)`: the api and its `connect` half, which
/// watches the registry revision and the content-invalidating events.
pub fn create_selection_controller(
    cx: Controller<SelectionState, SelectionAction>,
) -> (SelectionCapability, ConnectFn) {
    let core = Rc::new(Core {
        changed: cx.event_source(),
        committed: cx.event_source(),
        cleared: cx.event_source(),
        cx,
        raw_geometry: RefCell::default(),
        derived: RefCell::default(),
        loads: RefCell::default(),
        text_snapshots: RefCell::default(),
        epoch: Cell::new(0),
        model: RefCell::new(None),
        registry: RefCell::new(None),
    });
    let api = SelectionCapability(core);
    let this = api.clone();
    let connect: ConnectFn = Rc::new(move || {
        let cx = &this.0.cx;
        // Registry changes (rotate/move/delete/insert bump the document revision).
        let last_revision = Rc::new(Cell::new(this.revision().unwrap_or(-1.0)));
        let watcher = this.clone();
        cx.listen(cx.subscribe(Rc::new(move || {
            let Some(revision) = watcher.revision() else { return Ok(()) };
            if revision == last_revision.get() {
                return Ok(());
            }
            last_revision.set(revision);
            watcher.on_pages_updated();
            Ok(())
        })));
        // Content changes: every cached snapshot is stale.
        let doc = cx.doc()?;
        let listener = this.clone();
        cx.listen(doc.subscribe(Rc::new(move |event: &Value| {
            let kind = event.get("type").and_then(Value::as_str);
            if matches!(kind, Some("redaction.applied" | "pages.flattened")) {
                listener.invalidate_content();
            }
        })));
        Ok(())
    });
    (api, connect)
}

impl SelectionCapability {
    fn cx(&self) -> &Controller<SelectionState, SelectionAction> {
        &self.0.cx
    }

    fn state(&self) -> Rc<SelectionState> {
        self.0.cx.state()
    }

    fn allows(&self, scope: &str) -> bool {
        self.cx().doc().is_ok_and(|doc| doc.security().allows(scope))
    }

    fn assert_scope(&self, scope: &str, operation: &str) -> Result<(), Error> {
        if self.allows(scope) { Ok(()) } else { Err(permission_denied(operation, scope)) }
    }

    // ── page addressing (the registry is the truth for order and layout) ──

    /// `ctx.document()?.revision`, read without cloning the page list.
    fn revision(&self) -> Option<f64> {
        let core = self.cx().core();
        match self.cx().document_id().and_then(|id| core.documents.get(id)) {
            Some(meta) => Some(meta.revision as f64),
            None => self.cx().document().map(|meta| meta.revision as f64),
        }
    }

    /// The registry decoded once per page list: a lookup per page or per
    /// index would rescan and redecode it, which made a span O(n^2) and a
    /// settling select-all O(n^3). Keyed on the shared list's identity.
    fn registry(&self) -> Rc<Registry> {
        let Some(meta) = self.cx().document() else { return Rc::new(Registry::default()) };
        if let Some(cached) = self.0.registry.borrow().as_ref()
            && Rc::ptr_eq(&cached.0, &meta.pages)
        {
            return cached.1.clone();
        }
        let registry = Rc::new(Registry::decode(&meta.pages));
        *self.0.registry.borrow_mut() = Some((meta.pages.clone(), registry.clone()));
        registry
    }

    fn pages(&self) -> Rc<Registry> {
        self.registry()
    }

    fn layout_of(&self, pon: PageObjectNumber) -> Option<PageLayout> {
        self.registry().layout_of(pon).cloned()
    }

    fn page_index_of(&self, pon: PageObjectNumber) -> i64 {
        self.registry().layout_of(pon).map_or(-1, |l| l.index as i64)
    }

    fn pon_at_index(&self, i: i64) -> Option<PageObjectNumber> {
        let i = usize::try_from(i).ok()?;
        self.registry().pons.get(i).copied().flatten()
    }

    // ── geometry ──

    /// Page-space geometry for a page, derived on demand from the raw
    /// snapshot and the current layout. A view rotation (or crop change)
    /// changes the key and re-derives; the engine is never asked again. A
    /// page whose crop box has no area has none (the TypeScript throws).
    fn geometry_for(&self, pon: PageObjectNumber) -> Option<Rc<SelectionPageGeometry>> {
        if !self.0.raw_geometry.borrow().contains_key(&pon) {
            return None;
        }
        self.geometry_with(pon, &self.layout_of(pon)?)
    }

    /// `geometry_for` against a layout the caller already decoded.
    fn geometry_with(
        &self,
        pon: PageObjectNumber,
        layout: &PageLayout,
    ) -> Option<Rc<SelectionPageGeometry>> {
        let raw = self.0.raw_geometry.borrow().get(&pon).cloned()?;
        let crop = layout.boxes.crop;
        let key = format!(
            "{}|{}|{},{},{},{}",
            number_to_string(layout.rotation.0),
            number_to_string(layout.user_unit),
            number_to_string(crop.left),
            number_to_string(crop.bottom),
            number_to_string(crop.right),
            number_to_string(crop.top),
        );
        if let Some((hit, geom)) = self.0.derived.borrow().get(&pon)
            && *hit == key
        {
            return Some(geom.clone());
        }
        let geom = Rc::new(
            build_selection_page_geometry(&raw, crop, layout.rotation, layout.user_unit).ok()?,
        );
        self.0.derived.borrow_mut().insert(pon, (key, geom.clone()));
        Some(geom)
    }

    fn glyph_at(geom: &SelectionPageGeometry, point: Point) -> Option<i64> {
        text_glyph_at(&geom.layout, content_point_to_pdf(geom, point), DEFAULT_TOLERANCE_FACTOR)
            .map(|i| i as i64)
    }

    fn load(&self, page: PageRef) -> Promise<()> {
        let pon = page.page_object_number;
        if self.0.raw_geometry.borrow().contains_key(&pon) {
            return Promise::resolved(());
        }
        if let Some(in_flight) = self.0.loads.borrow().get(&pon) {
            return in_flight.clone();
        }
        // Authorized-only warming: without doc.text.select the read is
        // guaranteed to be refused, so it isn't issued.
        if !self.allows(SELECT_SCOPE) || self.layout_of(pon).is_none() {
            return Promise::resolved(());
        }
        let Ok(doc) = self.cx().doc() else { return Promise::resolved(()) };
        let at = self.0.epoch.get();
        let read = doc.call_answer("page.geometry.read", Some(pon), Vec::new());
        let this = self.clone();
        let load = Promise::spawn(async move {
            let result = read.await;
            this.0.loads.borrow_mut().remove(&pon);
            // closed / aborted / refused: nothing to paint, a later call retries
            let Ok(value) = result else { return Ok(()) };
            if at != this.0.epoch.get() {
                return Ok(()); // content changed while in flight: stale
            }
            let Ok(snapshot) = value.decode::<PageGeometrySnapshot>() else { return Ok(()) };
            this.0.raw_geometry.borrow_mut().insert(pon, Rc::new(snapshot));
            this.cx().dispatch(SelectionAction::PageLoaded { page });
            // a mid-span page arrived: fill its segments
            if let Some(current) = this.state().selection {
                this.recompute(current, ChangeTrigger::System);
            }
            Ok(())
        });
        if !load.is_settled() {
            self.0.loads.borrow_mut().insert(pon, load.clone());
        }
        load
    }

    // ── the range model ──

    /// Order the two ends by document position (page, then glyph).
    fn ordered_ends(&self, sel: &SelectionRange) -> Ordered {
        let ai = self.page_index_of(sel.anchor.page.page_object_number);
        let fi = self.page_index_of(sel.focus.page.page_object_number);
        let anchor_first = ai < fi || (ai == fi && sel.anchor.glyph <= sel.focus.glyph);
        if anchor_first {
            Ordered { start: sel.anchor, end: sel.focus, direction: SelectionDirection::Forward }
        } else {
            Ordered { start: sel.focus, end: sel.anchor, direction: SelectionDirection::Backward }
        }
    }

    /// Clamp a pointer into its page's real character range once geometry is known.
    fn clamp_pointer(&self, ptr: GlyphPointer) -> GlyphPointer {
        let Some(geom) = self.geometry_for(ptr.page.page_object_number) else { return ptr };
        let max = (geom.layout.glyphs.len() as i64 - 1).max(0);
        GlyphPointer { page: ptr.page, glyph: ptr.glyph.min(max).max(0) }
    }

    /// The one place the selection changes: commit the model, then tell listeners.
    fn publish(
        &self,
        selection: Option<SelectionRange>,
        segments: BTreeMap<PageObjectNumber, Vec<SelectionSegment>>,
        trigger: ChangeTrigger,
    ) {
        let before = self.state();
        match selection {
            Some(selection) => self.cx().dispatch(SelectionAction::Set { selection, segments }),
            None => self.cx().dispatch(SelectionAction::Clear),
        }
        if Rc::ptr_eq(&self.state(), &before) {
            return; // clearing nothing is not a change
        }
        let origin = ChangeOrigin::local(trigger);
        let model = self.read_model();
        self.0.changed.emit(&SelectionChangedEvent {
            range: model.snapshot.range,
            pages: model.pages.clone(),
            origin: origin.clone(),
        });
        if selection.is_none() {
            self.0.cleared.emit(&SelectionClearedEvent { origin });
        }
    }

    /// Rebuild the merged segments of every loaded page in the span; warm
    /// the rest. Clears when an endpoint's page has left the registry.
    fn recompute(&self, sel: SelectionRange, trigger: ChangeTrigger) {
        let clamped = SelectionRange {
            anchor: self.clamp_pointer(sel.anchor),
            focus: self.clamp_pointer(sel.focus),
        };
        let Ordered { start, end, .. } = self.ordered_ends(&clamped);
        let si = self.page_index_of(start.page.page_object_number);
        let ei = self.page_index_of(end.page.page_object_number);
        if si < 0 || ei < 0 {
            self.publish(None, BTreeMap::new(), trigger);
            return;
        }
        let mut segments = BTreeMap::new();
        let pages = self.pages();
        for i in si..=ei {
            let Some(pon) = pages.pons.get(i as usize).copied().flatten() else { continue };
            let geom = match pages.layout_of(pon) {
                Some(layout) => self.geometry_with(pon, layout),
                None => None,
            };
            let Some(geom) = geom else {
                let _ = self.load(PageRef::new(pon)); // recomputes on arrival
                continue;
            };
            let from = if i == si { start.glyph } else { 0 };
            let to = if i == ei { end.glyph } else { geom.layout.glyphs.len() as i64 - 1 };
            let segs = text_segments_for_range(&geom.layout, from, to - from + 1)
                .iter()
                .map(|s| to_content_segment(&geom, s))
                .collect();
            segments.insert(pon, segs);
        }
        self.publish(Some(clamped), segments, trigger);
    }

    fn set_gesture(&self, active: bool) {
        self.cx().dispatch(SelectionAction::SetGestureActive { active });
    }

    /// Changes made while a gesture drives the selection are the user's;
    /// the rest come through the API.
    fn write_trigger(&self) -> ChangeTrigger {
        if self.state().gesture_active { ChangeTrigger::User } else { ChangeTrigger::Api }
    }

    // ── the read model, built once per state object ──

    fn read_model(&self) -> Rc<ReadModel> {
        let s = self.state();
        if let Some(model) = self.0.model.borrow().as_ref()
            && Rc::ptr_eq(&model.state, &s)
        {
            return model.clone();
        }
        let mut with_segments: Vec<PageObjectNumber> =
            s.segments.iter().filter(|(_, segs)| !segs.is_empty()).map(|(pon, _)| *pon).collect();
        // A stable sort by page index, as `Array.prototype.sort` is.
        let registry = self.registry();
        with_segments.sort_by_key(|pon| registry.layout_of(*pon).map_or(-1, |l| l.index as i64));
        let pages: Vec<PageRef> = with_segments.iter().map(|pon| PageRef::new(*pon)).collect();
        let snapshot_pages = with_segments
            .iter()
            .map(|pon| SnapshotPage { page: PageRef::new(*pon), segments: s.segments[pon].clone() })
            .collect();
        let mut anchor = None;
        let snapshot = match s.selection {
            None => SelectionSnapshot {
                pages: snapshot_pages,
                start: None,
                end: None,
                direction: SelectionDirection::Forward,
                range: None,
            },
            Some(selection) => {
                let Ordered { start, end, direction } = self.ordered_ends(&selection);
                // Prefer the gesture's end page; while its geometry is still
                // loading, fall back to the last page (document order) with
                // materialized segments.
                let end_pon = end.page.page_object_number;
                if let Some(bounds) = union_of(s.segments.get(&end_pon)) {
                    anchor = Some(SelectionAnchor { page: end.page, bounds });
                } else if let Some(last) = with_segments.last()
                    && let Some(bounds) = union_of(s.segments.get(last))
                {
                    anchor = Some(SelectionAnchor { page: PageRef::new(*last), bounds });
                }
                SelectionSnapshot {
                    pages: snapshot_pages,
                    start: self.endpoint_for(&s, start, Which::Start),
                    end: self.endpoint_for(&s, end, Which::End),
                    direction,
                    range: Some(TextRange {
                        start: TextPosition { page: start.page, index: start.glyph },
                        end: TextPosition { page: end.page, index: end.glyph + 1 },
                    }),
                }
            }
        };
        let model = Rc::new(ReadModel { state: s, snapshot, pages, anchor });
        *self.0.model.borrow_mut() = Some(model.clone());
        model
    }

    fn endpoint_for(
        &self,
        s: &SelectionState,
        ptr: GlyphPointer,
        which: Which,
    ) -> Option<SelectionEndpoint> {
        let pon = ptr.page.page_object_number;
        let segments = s.segments.get(&pon).map(Vec::as_slice).unwrap_or_default();
        let segment = if which == Which::Start { segments.first()? } else { segments.last()? };
        // The boundary glyph's own cell, so caret placement lands on the
        // exact character edge; the segment when the glyph is degenerate.
        let geom = self.geometry_for(pon);
        let cell = geom.as_ref().and_then(|g| text_glyph_quad(&g.layout, ptr.glyph));
        if let (Some(geom), Some(cell)) = (geom, cell) {
            let glyph_quad = to_content_text_quad(&geom, &cell);
            return Some(SelectionEndpoint {
                page: ptr.page,
                glyph_quad,
                advance: segment.advance,
                rect: text_quad_bounds(glyph_quad),
            });
        }
        Some(SelectionEndpoint {
            page: ptr.page,
            glyph_quad: segment.quad,
            advance: segment.advance,
            rect: segment.rect,
        })
    }

    // ── span writes ──

    /// Word or line around a point. Whether a span actually engaged
    /// (geometry present and a glyph under the point).
    fn select_span_at(&self, page: PageRef, point: Point, expand: Expand) -> Result<bool, Error> {
        let operation = match expand {
            Expand::Word => "selection.selectWordAt",
            Expand::Line => "selection.selectLineAt",
        };
        self.assert_scope(SELECT_SCOPE, operation)?;
        let Some(geom) = self.geometry_for(page.page_object_number) else { return Ok(false) };
        let Some(i) = Self::glyph_at(&geom, point) else { return Ok(false) };
        let (from, to) = match expand {
            Expand::Word => expand_text_range_to_word(&geom.layout, i),
            Expand::Line => expand_text_range_to_line(&geom.layout, i),
        };
        self.recompute(
            SelectionRange {
                anchor: GlyphPointer { page, glyph: from },
                focus: GlyphPointer { page, glyph: to },
            },
            self.write_trigger(),
        );
        Ok(true)
    }

    // ── text extraction ──

    /// Per-page text, cached as a promise so concurrent readers share one
    /// fetch; a rejection is evicted.
    fn page_text(&self, pon: PageObjectNumber) -> Promise<Rc<PageTextSnapshot>> {
        if let Some(p) = self.0.text_snapshots.borrow().get(&pon) {
            return p.clone();
        }
        let call = match self.cx().doc() {
            Ok(doc) => doc.call_answer("page.text.read", Some(pon), Vec::new()),
            Err(error) => Promise::rejected(error),
        };
        // `Promise.resolve(abortable)`: an AbortablePromise is a subclass, so
        // this adopts it as a thenable, which is one turn more than awaiting.
        let p = Promise::spawn(async move {
            rt::tick().await;
            let value = call.await?;
            value.decode::<PageTextSnapshot>().map(Rc::new)
        });
        let core = Rc::downgrade(&self.0);
        let watched = p.clone();
        rt::spawn(async move {
            if watched.await.is_err()
                && let Some(core) = core.upgrade()
            {
                core.text_snapshots.borrow_mut().remove(&pon);
            }
        });
        self.0.text_snapshots.borrow_mut().insert(pon, p.clone());
        p
    }

    // ── invalidation ──

    /// A structural registry change: drop the caches of pages that left,
    /// then recompute. Raw snapshots are never refetched here.
    fn on_pages_updated(&self) {
        let alive: HashSet<PageObjectNumber> =
            self.pages().pons.iter().flatten().copied().collect();
        let gone: Vec<PageObjectNumber> =
            self.0.raw_geometry.borrow().keys().filter(|p| !alive.contains(p)).copied().collect();
        for pon in gone {
            self.0.raw_geometry.borrow_mut().remove(&pon);
            self.0.derived.borrow_mut().remove(&pon);
            self.0.text_snapshots.borrow_mut().remove(&pon);
        }
        if let Some(current) = self.state().selection {
            self.recompute(current, ChangeTrigger::System);
        }
    }

    /// Page content changed: every cache is stale and any live range points
    /// into a character space that no longer exists.
    fn invalidate_content(&self) {
        self.0.epoch.set(self.0.epoch.get() + 1);
        self.0.raw_geometry.borrow_mut().clear();
        self.0.derived.borrow_mut().clear();
        self.0.loads.borrow_mut().clear();
        self.0.text_snapshots.borrow_mut().clear();
        self.publish(None, BTreeMap::new(), ChangeTrigger::System);
    }

    // ── public lens: authorization ──

    /// Whether this caller may create selections (`doc.text.select`).
    pub fn can_select(&self) -> bool {
        self.allows(SELECT_SCOPE)
    }

    /// Whether this caller may extract text (`doc.text.copy`).
    pub fn can_copy(&self) -> bool {
        self.allows(COPY_SCOPE)
    }

    // ── public lens: writes ──

    /// Select a range. An empty range clears. `permission-denied` without
    /// `doc.text.select`, `not-found` for a page not in this document.
    pub fn select(&self, input: SelectionRangeInput) -> Result<(), Error> {
        self.assert_scope(SELECT_SCOPE, "selection.select")?;
        let range = match input {
            SelectionRangeInput::Range(range) => range,
            SelectionRangeInput::Span { page, start, count } => TextRange {
                start: TextPosition { page, index: start },
                end: TextPosition { page, index: start.saturating_add(count) },
            },
        };
        self.cx().assert_page_ref(range.start.page)?;
        self.cx().assert_page_ref(range.end.page)?;
        let si = self.page_index_of(range.start.page.page_object_number);
        let mut ei = self.page_index_of(range.end.page.page_object_number);
        let mut end_index = range.end.index;
        if ei < si || (ei == si && end_index <= range.start.index) {
            self.publish(None, BTreeMap::new(), self.write_trigger()); // an empty range is no selection
            return Ok(());
        }
        if end_index == 0 {
            // A half-open end exactly at a page boundary: the last included
            // character is the previous page's last one, which settles by
            // clamping on load.
            ei -= 1;
            if ei < si {
                self.publish(None, BTreeMap::new(), self.write_trigger());
                return Ok(());
            }
            end_index = OPEN_END;
        }
        let Some(focus_pon) = self.pon_at_index(ei) else { return Ok(()) };
        self.recompute(
            SelectionRange {
                anchor: GlyphPointer { page: range.start.page, glyph: range.start.index.max(0) },
                focus: GlyphPointer {
                    page: PageRef::new(focus_pon),
                    glyph: end_index.saturating_sub(1),
                },
            },
            self.write_trigger(),
        );
        Ok(())
    }

    /// Select every character of the document.
    pub fn select_all(&self) -> Result<(), Error> {
        self.assert_scope(SELECT_SCOPE, "selection.selectAll")?;
        let pages = self.pages();
        let (Some(Some(first)), Some(Some(last))) = (pages.pons.first(), pages.pons.last()) else {
            return Ok(());
        };
        self.recompute(
            SelectionRange {
                anchor: GlyphPointer { page: PageRef::new(*first), glyph: 0 },
                focus: GlyphPointer { page: PageRef::new(*last), glyph: OPEN_END },
            },
            self.write_trigger(),
        );
        Ok(())
    }

    /// Select every character on one page.
    pub fn select_page(&self, page: PageRef) -> Result<(), Error> {
        self.assert_scope(SELECT_SCOPE, "selection.selectPage")?;
        self.cx().assert_page_ref(page)?;
        self.recompute(
            SelectionRange {
                anchor: GlyphPointer { page, glyph: 0 },
                focus: GlyphPointer { page, glyph: OPEN_END },
            },
            self.write_trigger(),
        );
        Ok(())
    }

    /// Select the word around a page point; false when the point has no
    /// selectable text.
    pub fn select_word_at(&self, page: PageRef, point: Point) -> Result<bool, Error> {
        self.select_span_at(page, point, Expand::Word)
    }

    /// Select the visual line around a page point.
    pub fn select_line_at(&self, page: PageRef, point: Point) -> Result<bool, Error> {
        self.select_span_at(page, point, Expand::Line)
    }

    /// Extend the current selection to a page point. No selection is a
    /// no-op; a page whose geometry has not loaded is warmed and the
    /// extension lands when it arrives.
    pub fn extend_to(&self, page: PageRef, point: Point) -> Result<(), Error> {
        self.assert_scope(SELECT_SCOPE, "selection.extendTo")?;
        let Some(current) = self.state().selection else { return Ok(()) };
        let Some(geom) = self.geometry_for(page.page_object_number) else {
            let _ = self.load(page);
            return Ok(());
        };
        let Some(i) = Self::glyph_at(&geom, point) else { return Ok(()) }; // off-text: keep the focus
        self.recompute(
            SelectionRange { anchor: current.anchor, focus: GlyphPointer { page, glyph: i } },
            self.write_trigger(),
        );
        Ok(())
    }

    /// Clear the selection. Always allowed.
    pub fn clear(&self) {
        self.publish(None, BTreeMap::new(), self.write_trigger());
    }

    // ── public lens: reads ──

    pub fn has_selection(&self) -> bool {
        self.state().selection.is_some()
    }

    /// Per-page segments, endpoints, direction and the range.
    pub fn get_snapshot(&self) -> SelectionSnapshot {
        self.read_model().snapshot.clone()
    }

    /// The selected character range, or `None`.
    pub fn get_range(&self) -> Option<TextRange> {
        self.read_model().snapshot.range
    }

    /// The pages with at least one segment, in document order.
    pub fn list_selected_pages(&self) -> Vec<PageRef> {
        self.read_model().pages.clone()
    }

    /// Per-line oriented segments for a page, in page space.
    pub fn list_segments(&self, page: PageRef) -> Vec<SelectionSegment> {
        self.state().segments.get(&page.page_object_number).cloned().unwrap_or_default()
    }

    /// The segments' boxes.
    pub fn list_rects(&self, page: PageRef) -> Vec<Rect> {
        self.state()
            .segments
            .get(&page.page_object_number)
            .map_or_else(Vec::new, |s| s.iter().map(|s| s.rect).collect())
    }

    /// Where selection-scoped floating UI attaches: the union box on the
    /// end page, else the last page with materialized segments.
    pub fn get_anchor(&self) -> Option<SelectionAnchor> {
        self.read_model().anchor
    }

    // ── public lens: text extraction ──

    /// The selected text: `""` with nothing selected.
    pub fn read_text(&self, options: OperationOptions) -> Promise<String> {
        match self.read_model().snapshot.range {
            Some(range) => self.read_text_in_range(range, options),
            None => Promise::resolved(String::new()),
        }
    }

    /// Text for any range, without touching the selection. Pages join with
    /// `\n`. Rejects `permission-denied` without `doc.text.copy`,
    /// `not-found` for a page not in this document, `operation-cancelled`
    /// when the signal aborts.
    pub fn read_text_in_range(
        &self,
        range: TextRange,
        options: OperationOptions,
    ) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            // The content generation the read starts in. A redaction or
            // flatten that lands while batches are in flight fails the read
            // rather than return text from before it.
            let at = this.0.epoch.get();
            this.assert_scope(COPY_SCOPE, "selection.readText")?;
            this.cx().assert_page_ref(range.start.page)?;
            this.cx().assert_page_ref(range.end.page)?;
            let si = this.page_index_of(range.start.page.page_object_number);
            let mut ei = this.page_index_of(range.end.page.page_object_number);
            let mut end_index = range.end.index;
            if ei < si || (ei == si && end_index <= range.start.index) {
                return Ok(String::new());
            }
            if end_index == 0 {
                ei -= 1; // a range ending at a page boundary includes nothing of that page
                end_index = OPEN_END;
            }
            // Per-page half-open character spans; interior pages span their
            // whole text, which the slice clamps to.
            let mut spans: Vec<(PageObjectNumber, i64, i64)> = Vec::new();
            let pages = this.pages();
            for i in si..=ei {
                let Some(pon) = pages.pons.get(i as usize).copied().flatten() else {
                    continue;
                };
                let from = if i == si { range.start.index.max(0) } else { 0 };
                let to = if i == ei { end_index } else { OPEN_END };
                spans.push((pon, from, to));
            }
            let signal = options.signal.as_ref();
            let mut parts = Vec::with_capacity(spans.len());
            for batch in spans.chunks(TEXT_READ_CONCURRENCY) {
                throw_if_aborted(signal)?;
                let reads = batch.iter().map(|(pon, _, _)| this.page_text(*pon)).collect();
                let snapshots = all(reads).await?;
                throw_if_stale(at, this.0.epoch.get())?;
                for (snap, (_, from, to)) in snapshots.iter().zip(batch) {
                    parts.push(slice_text_by_chars(snap, *from as f64, *to as f64));
                }
            }
            throw_if_aborted(signal)?;
            Ok(parts.join("\n"))
        })
    }

    // ── public lens: events ──

    pub fn on_changed(
        &self,
        listener: impl Fn(&SelectionChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.changed.on(listener)
    }

    pub fn on_committed(
        &self,
        listener: impl Fn(&SelectionCommittedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.committed.on(listener)
    }

    pub fn on_cleared(
        &self,
        listener: impl Fn(&SelectionClearedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.cleared.on(listener)
    }

    // ── host lens ──

    /// Warm a page's text geometry. Resolves once loaded; resolves at once
    /// without `doc.text.select` or for an unknown page, and on a failed read.
    pub fn ensure_loaded(&self, page: PageRef) -> Promise<()> {
        self.load(page)
    }

    pub fn is_loaded(&self, page: PageRef) -> bool {
        self.state().loaded.get(&page.page_object_number).copied().unwrap_or(false)
    }

    /// Is a page-space point on (or near) text?
    pub fn is_over_text(&self, page: PageRef, point: Point) -> bool {
        self.geometry_for(page.page_object_number)
            .is_some_and(|geom| Self::glyph_at(&geom, point).is_some())
    }

    /// A pointer gesture that drives the selection opened.
    pub fn begin_gesture(&self) {
        self.set_gesture(true);
    }

    /// Open a caret selection at a page point; false when the point has no
    /// text.
    pub fn begin_gesture_at(&self, page: PageRef, point: Point) -> bool {
        if !self.can_select() {
            return false;
        }
        let Some(geom) = self.geometry_for(page.page_object_number) else { return false };
        let Some(i) = Self::glyph_at(&geom, point) else { return false };
        self.set_gesture(true); // before the publish, so listeners see a coherent pair
        self.recompute(
            SelectionRange {
                anchor: GlyphPointer { page, glyph: i },
                focus: GlyphPointer { page, glyph: i },
            },
            ChangeTrigger::User,
        );
        true
    }

    /// The gesture ended: settle first, so commit listeners observe
    /// `is_gesture_active() == false`, then commit a selection in place.
    pub fn end_gesture(&self) {
        self.set_gesture(false);
        if let Some(range) = self.read_model().snapshot.range {
            self.0.committed.emit(&SelectionCommittedEvent { range });
        }
    }

    pub fn is_gesture_active(&self) -> bool {
        self.state().gesture_active
    }

    /// Suppress or restore the default highlight visual.
    pub fn set_highlight_visible(&self, visible: bool) {
        self.cx().dispatch(SelectionAction::SetHighlightHidden { hidden: !visible });
    }

    pub fn is_highlight_visible(&self) -> bool {
        !self.state().highlight_hidden
    }
}
