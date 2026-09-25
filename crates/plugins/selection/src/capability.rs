//! `capability.ts`: one object that is both the public lens and the host
//! lens.
//!
//! The slice holds the selection range and the derived content-space
//! segments; the per-page caches live here, split by what invalidates them:
//! `raw_geometry` (PDF-space snapshots, dropped when a page leaves the
//! registry or its content changes), `derived` (the transform and layout,
//! keyed by crop, rotation and `userUnit`, so a view rotation re-derives with
//! no refetch) and `text_snapshots` (for `read_text`).
//!
//! Permissions: `doc.text.select` gates geometry, `doc.text.copy` text.
//! Host gesture functions are inert without select; `select` and
//! `select_all` return `PermissionDenied`; `clear` is always allowed.
//!
//! Selection is cross-page: glyphs are ordered by (page index, glyph), and
//! `recompute` rebuilds every loaded page in the span, warms the rest, and
//! clamps stored endpoints once their page's geometry is known.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use epdf_geometry::js::{self, number_to_string};
use epdf_geometry::{PageRotation, Point, Rect, text_quad_bounds};
use epdf_kernel::engine::decode;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Ctx, Error};
use epdf_plugin_interaction::{CallbackSet, Unsubscriber};
use serde::Deserialize;
use serde_json::Value;

use crate::geometry::{
    SelectionPageGeometry, SelectionSegment, build_selection_page_geometry, content_point_to_pdf,
    to_content_segment, to_content_text_quad,
};
use crate::text_layout::{
    DEFAULT_TOLERANCE_FACTOR, PageGeometrySnapshot, PageTextSnapshot, PdfRect,
    expand_text_range_to_line, expand_text_range_to_word, slice_text_by_chars, text_glyph_at,
    text_glyph_quad, text_segments_for_range,
};
use crate::types::*;

/// How many page-text reads `read_text` keeps in flight at once.
const TEXT_READ_CONCURRENCY: usize = 8;

/// `Number.MAX_SAFE_INTEGER`: the open end `select_all` settles by clamping.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

const SELECT_CAPABILITY: &str = "doc.text.select";
const COPY_CAPABILITY: &str = "doc.text.copy";

/// The members of a `PageLayout` the plugin reads.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageLayout {
    page_object_number: PageObjectNumber,
    rotation: PageRotation,
    user_unit: f64,
    boxes: PageBoxes,
}

#[derive(Clone, Debug, Deserialize)]
struct PageBoxes {
    crop: PdfRect,
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

#[derive(Default)]
struct Caches {
    raw_geometry: RefCell<HashMap<PageObjectNumber, Rc<PageGeometrySnapshot>>>,
    derived: RefCell<HashMap<PageObjectNumber, (String, Rc<SelectionPageGeometry>)>>,
    pending: RefCell<HashSet<PageObjectNumber>>,
    text_snapshots: RefCell<HashMap<PageObjectNumber, Promise<Rc<PageTextSnapshot>>>>,
    /// Bumped on content invalidation; reads in flight from before are dropped.
    epoch: Cell<u64>,
    change_cbs: CallbackSet,
    commit_cbs: CallbackSet,
    last_revision: Cell<f64>,
}

/// The selection capability: `SelectionHostCapability`, which includes the
/// public `SelectionCapability`. Cheap to clone; clones share the caches.
#[derive(Clone)]
pub struct SelectionCapability {
    ctx: Ctx<SelectionState, SelectionAction>,
    caches: Rc<Caches>,
}

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

/// `createSelectionCapability(ctx)`: the caches, the lifecycle wiring
/// (registry revision watch, content invalidation events), the capability.
pub fn create_selection_capability(
    ctx: Ctx<SelectionState, SelectionAction>,
) -> SelectionCapability {
    let cap = SelectionCapability { ctx, caches: Rc::new(Caches::default()) };
    cap.caches.last_revision.set(cap.revision().unwrap_or(-1.0));

    let this = cap.clone();
    let off_store = cap.ctx.subscribe(Rc::new(move || {
        let Some(revision) = this.revision() else { return Ok(()) };
        if revision == this.caches.last_revision.get() {
            return Ok(());
        }
        this.caches.last_revision.set(revision);
        this.on_pages_updated();
        Ok(())
    }));
    cap.ctx.cleanup_fn(move || off_store.call());

    if let Some(doc) = cap.ctx.doc() {
        let this = cap.clone();
        let off_events = doc.subscribe(Rc::new(move |event: &Value| {
            let kind = event.get("type").and_then(Value::as_str);
            if matches!(kind, Some("redaction.applied" | "pages.flattened")) {
                this.invalidate_content();
            }
        }));
        cap.ctx.cleanup_fn(move || off_events.call());
    }
    cap
}

impl SelectionCapability {
    // ── the page registry ──

    /// `ctx.document()?.revision`, read without cloning the page list.
    fn revision(&self) -> Option<f64> {
        let core = self.ctx.core();
        match self.ctx.document_id().and_then(|id| core.documents.get(id)) {
            Some(meta) => Some(meta.revision as f64),
            None => self.ctx.document().map(|meta| meta.revision as f64),
        }
    }

    fn pages(&self) -> Vec<PageLayout> {
        let Some(meta) = self.ctx.document() else { return Vec::new() };
        serde_json::from_value(meta.pages).unwrap_or_default()
    }

    fn layout_of(&self, pon: PageObjectNumber) -> Option<PageLayout> {
        self.pages().into_iter().find(|p| p.page_object_number == pon)
    }

    fn page_index_of(&self, pon: PageObjectNumber) -> i64 {
        self.pages().iter().position(|p| p.page_object_number == pon).map_or(-1, |i| i as i64)
    }

    fn pon_at_index(&self, i: i64) -> Option<PageObjectNumber> {
        let i = usize::try_from(i).ok()?;
        self.pages().get(i).map(|p| p.page_object_number)
    }

    // ── geometry ──

    /// The content-space geometry for a page, derived on demand from the raw
    /// snapshot and the current layout. A page whose crop box has no area
    /// has none (the TypeScript throws there).
    fn geometry_for(&self, pon: PageObjectNumber) -> Option<Rc<SelectionPageGeometry>> {
        let raw = self.caches.raw_geometry.borrow().get(&pon).cloned()?;
        let layout = self.layout_of(pon)?;
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
        if let Some((hit, geom)) = self.caches.derived.borrow().get(&pon)
            && *hit == key
        {
            return Some(geom.clone());
        }
        let geom = Rc::new(
            build_selection_page_geometry(&raw, crop, layout.rotation, layout.user_unit).ok()?,
        );
        self.caches.derived.borrow_mut().insert(pon, (key, geom.clone()));
        Some(geom)
    }

    fn glyph_at(geom: &SelectionPageGeometry, point: Point) -> Option<i64> {
        text_glyph_at(&geom.layout, content_point_to_pdf(geom, point), DEFAULT_TOLERANCE_FACTOR)
            .map(|i| i as i64)
    }

    /// Order the two ends by document position (page, then glyph).
    fn ordered_ends(&self, sel: &SelectionRange) -> Ordered {
        let ai = self.page_index_of(sel.anchor.pon);
        let fi = self.page_index_of(sel.focus.pon);
        let anchor_first = ai < fi || (ai == fi && sel.anchor.glyph <= sel.focus.glyph);
        if anchor_first {
            Ordered { start: sel.anchor, end: sel.focus, direction: SelectionDirection::Forward }
        } else {
            Ordered { start: sel.focus, end: sel.anchor, direction: SelectionDirection::Backward }
        }
    }

    fn endpoint_for(&self, ptr: GlyphPointer, which: Which) -> Option<SelectionEndpoint> {
        let state = self.ctx.state();
        let segments = state.segments.get(&ptr.pon).map(Vec::as_slice).unwrap_or_default();
        let segment = if which == Which::Start { segments.first()? } else { segments.last()? };
        // The boundary glyph's own cell, or the segment when the glyph is
        // degenerate (a generated space).
        let geom = self.geometry_for(ptr.pon);
        let cell = geom.as_ref().and_then(|g| text_glyph_quad(&g.layout, ptr.glyph));
        if let (Some(geom), Some(cell)) = (geom, cell) {
            let glyph_quad = to_content_text_quad(&geom, &cell);
            return Some(SelectionEndpoint {
                pon: ptr.pon,
                glyph_quad,
                advance: segment.advance,
                rect: text_quad_bounds(glyph_quad),
            });
        }
        Some(SelectionEndpoint {
            pon: ptr.pon,
            glyph_quad: segment.quad,
            advance: segment.advance,
            rect: segment.rect,
        })
    }

    /// Union box of a page's segments, or `None` when none have materialized.
    fn page_bounds(&self, pon: PageObjectNumber) -> Option<Rect> {
        let state = self.ctx.state();
        let segs = state.segments.get(&pon).filter(|s| !s.is_empty())?;
        let (mut x1, mut y1) = (f64::INFINITY, f64::INFINITY);
        let (mut x2, mut y2) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for s in segs {
            x1 = js::min(x1, s.rect.x);
            y1 = js::min(y1, s.rect.y);
            x2 = js::max(x2, s.rect.x + s.rect.width);
            y2 = js::max(y2, s.rect.y + s.rect.height);
        }
        Some(Rect { x: x1, y: y1, width: x2 - x1, height: y2 - y1 })
    }

    fn clamp_pointer(&self, ptr: GlyphPointer) -> GlyphPointer {
        let Some(geom) = self.geometry_for(ptr.pon) else { return ptr };
        let max = (geom.layout.glyphs.len() as i64 - 1).max(0);
        let glyph = ptr.glyph.min(max).max(0);
        GlyphPointer { pon: ptr.pon, glyph }
    }

    /// Rebuild the merged segments of every loaded page in the span; warm
    /// the rest. Clears when an endpoint's page has left the registry.
    fn recompute(&self, sel: SelectionRange) {
        let clamped = SelectionRange {
            anchor: self.clamp_pointer(sel.anchor),
            focus: self.clamp_pointer(sel.focus),
        };
        let Ordered { start, end, .. } = self.ordered_ends(&clamped);
        let si = self.page_index_of(start.pon);
        let ei = self.page_index_of(end.pon);
        if si < 0 || ei < 0 {
            self.clear_selection();
            return;
        }
        let mut segments: BTreeMap<PageObjectNumber, Vec<SelectionSegment>> = BTreeMap::new();
        for i in si..=ei {
            let Some(pon) = self.pon_at_index(i) else { continue };
            let Some(geom) = self.geometry_for(pon) else {
                self.ensure_page(pon); // not loaded yet: it recomputes when ready
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
        self.ctx.dispatch(SelectionAction::Set { selection: clamped, segments });
        self.caches.change_cbs.for_each();
    }

    fn recompute_current(&self) {
        let current = self.ctx.state().selection;
        if let Some(sel) = current {
            self.recompute(sel);
        }
    }

    /// The gesture-in-flight fact, set before the recompute that fires
    /// `on_change`.
    fn set_selecting(&self, selecting: bool) {
        self.ctx.dispatch(SelectionAction::SetSelecting { selecting });
    }

    fn clear_selection(&self) {
        self.set_selecting(false);
        self.ctx.dispatch(SelectionAction::Clear);
        self.caches.change_cbs.for_each();
    }

    fn select_span_at(&self, pon: PageObjectNumber, point: Point, expand: Expand) -> bool {
        let Some(geom) = self.geometry_for(pon) else { return false };
        let Some(i) = Self::glyph_at(&geom, point) else { return false };
        let (from, to) = match expand {
            Expand::Word => expand_text_range_to_word(&geom.layout, i),
            Expand::Line => expand_text_range_to_line(&geom.layout, i),
        };
        self.set_selecting(true);
        self.recompute(SelectionRange {
            anchor: GlyphPointer { pon, glyph: from },
            focus: GlyphPointer { pon, glyph: to },
        });
        true
    }

    fn assert_can_select(&self, context: &str) -> Result<(), Error> {
        if self.can_select() {
            Ok(())
        } else {
            Err(Error::permission_denied(SELECT_CAPABILITY, Some(context)))
        }
    }

    /// Per-page text, cached as a promise so concurrent readers share one
    /// fetch; a rejection is evicted.
    fn page_text_snapshot(&self, pon: PageObjectNumber) -> Promise<Rc<PageTextSnapshot>> {
        if let Some(p) = self.caches.text_snapshots.borrow().get(&pon) {
            return p.clone();
        }
        let Some(doc) = self.ctx.doc() else {
            return Promise::rejected(Error::new("readText: document is not open"));
        };
        let call = doc.call("page.text.read", Some(pon), Vec::new());
        // `Promise.resolve(abortable)`: an AbortablePromise is a subclass, so
        // this adopts it as a thenable, which is one turn more than awaiting.
        let p = Promise::spawn(async move {
            rt::tick().await;
            let value = call.await?;
            decode::<PageTextSnapshot>(value).map(Rc::new)
        });
        let caches = Rc::downgrade(&self.caches);
        let watched = p.clone();
        rt::spawn(async move {
            if watched.await.is_err()
                && let Some(caches) = caches.upgrade()
            {
                caches.text_snapshots.borrow_mut().remove(&pon);
            }
        });
        self.caches.text_snapshots.borrow_mut().insert(pon, p.clone());
        p
    }

    /// A structural registry change: drop the caches of pages that left,
    /// then recompute. Raw snapshots are never refetched here.
    fn on_pages_updated(&self) {
        let alive: HashSet<PageObjectNumber> =
            self.pages().iter().map(|p| p.page_object_number).collect();
        let gone: Vec<PageObjectNumber> = self
            .caches
            .raw_geometry
            .borrow()
            .keys()
            .filter(|p| !alive.contains(p))
            .copied()
            .collect();
        for pon in gone {
            self.caches.raw_geometry.borrow_mut().remove(&pon);
            self.caches.derived.borrow_mut().remove(&pon);
            self.caches.text_snapshots.borrow_mut().remove(&pon);
        }
        self.recompute_current();
    }

    /// Page content changed: every cache is stale and any live range points
    /// into a character space that no longer exists.
    fn invalidate_content(&self) {
        self.caches.epoch.set(self.caches.epoch.get() + 1);
        self.caches.raw_geometry.borrow_mut().clear();
        self.caches.derived.borrow_mut().clear();
        self.caches.pending.borrow_mut().clear();
        self.caches.text_snapshots.borrow_mut().clear();
        if self.ctx.state().selection.is_some() {
            self.clear_selection();
        }
    }

    // ── public lens: authorization ──

    /// Whether this caller may create selections (`doc.text.select`).
    pub fn can_select(&self) -> bool {
        self.ctx.doc().is_some_and(|doc| doc.security().allows(SELECT_CAPABILITY))
    }

    /// Whether this caller may extract text (`doc.text.copy`).
    pub fn can_copy(&self) -> bool {
        self.ctx.doc().is_some_and(|doc| doc.security().allows(COPY_CAPABILITY))
    }

    // ── public lens: programmatic selection ──

    /// Select a range. An empty range clears. `PermissionDenied` without
    /// `doc.text.select`; an error for a page not in the registry.
    pub fn select(&self, input: SelectionRangeInput) -> Result<(), Error> {
        self.assert_can_select("selection.select")?;
        let range = match input {
            SelectionRangeInput::Range(range) => range,
            SelectionRangeInput::Span { pon, start, count } => TextRange {
                start: TextPosition { pon, index: start },
                end: TextPosition { pon, index: start + count },
            },
        };
        let si = self.page_index_of(range.start.pon);
        let mut ei = self.page_index_of(range.end.pon);
        if si < 0 || ei < 0 {
            let pon = if si < 0 { range.start.pon } else { range.end.pon };
            return Err(Error::new(format!("select: unknown page object {pon}")));
        }
        let mut end_index = range.end.index;
        if ei < si || (ei == si && end_index <= range.start.index) {
            self.clear_selection(); // empty range = no selection
            return Ok(());
        }
        if end_index == 0 {
            // A half-open end exactly at a page boundary: the last included
            // character is the previous page's last one.
            ei -= 1;
            if ei < si {
                self.clear_selection();
                return Ok(());
            }
            end_index = MAX_SAFE_INTEGER;
        }
        let Some(focus_pon) = self.pon_at_index(ei) else {
            return Err(Error::new(format!("select: unknown page at index {ei}")));
        };
        self.set_selecting(false); // programmatic selections are born settled
        self.recompute(SelectionRange {
            anchor: GlyphPointer { pon: range.start.pon, glyph: range.start.index.max(0) },
            focus: GlyphPointer { pon: focus_pon, glyph: end_index - 1 },
        });
        Ok(())
    }

    /// Select every character of the document.
    pub fn select_all(&self) -> Result<(), Error> {
        self.assert_can_select("selection.selectAll")?;
        let pages = self.pages();
        let (Some(first), Some(last)) = (pages.first(), pages.last()) else { return Ok(()) };
        self.set_selecting(false);
        self.recompute(SelectionRange {
            anchor: GlyphPointer { pon: first.page_object_number, glyph: 0 },
            // settles to the real last character on load
            focus: GlyphPointer { pon: last.page_object_number, glyph: MAX_SAFE_INTEGER },
        });
        Ok(())
    }

    /// Clear the selection. Always allowed.
    pub fn clear(&self) {
        self.clear_selection();
    }

    // ── public lens: reads ──

    pub fn snapshot(&self) -> SelectionSnapshot {
        let state = self.ctx.state();
        let pages = state
            .segments
            .iter()
            .filter(|(_, segs)| !segs.is_empty())
            .map(|(pon, segs)| SnapshotPage { pon: *pon, segments: segs.clone() })
            .collect();
        let Some(selection) = state.selection else {
            return SelectionSnapshot {
                pages,
                start: None,
                end: None,
                direction: SelectionDirection::Forward,
                range: None,
            };
        };
        drop(state);
        let Ordered { start, end, direction } = self.ordered_ends(&selection);
        SelectionSnapshot {
            pages,
            start: self.endpoint_for(start, Which::Start),
            end: self.endpoint_for(end, Which::End),
            direction,
            range: Some(TextRange {
                start: TextPosition { pon: start.pon, index: start.glyph },
                end: TextPosition { pon: end.pon, index: end.glyph + 1 },
            }),
        }
    }

    /// The selection's floating-UI anchor: its end page's union box, else
    /// the last page (document order) with materialized segments.
    pub fn menu_anchor(&self) -> Option<SelectionMenuAnchor> {
        let sel = self.ctx.state().selection?;
        let Ordered { end, .. } = self.ordered_ends(&sel);
        if let Some(bounds) = self.page_bounds(end.pon) {
            return Some(SelectionMenuAnchor { pon: end.pon, bounds });
        }
        self.pages().iter().rev().find_map(|p| {
            let pon = p.page_object_number;
            self.page_bounds(pon).map(|bounds| SelectionMenuAnchor { pon, bounds })
        })
    }

    /// Whether a selection gesture is in flight.
    pub fn is_selecting(&self) -> bool {
        self.ctx.state().selecting
    }

    pub fn has_selection(&self) -> bool {
        self.ctx.state().selection.is_some()
    }

    /// The pages with at least one segment.
    pub fn selected_pages(&self) -> Vec<PageObjectNumber> {
        let state = self.ctx.state();
        state.segments.iter().filter(|(_, s)| !s.is_empty()).map(|(pon, _)| *pon).collect()
    }

    pub fn segments_for_page(&self, pon: PageObjectNumber) -> Vec<SelectionSegment> {
        self.ctx.state().segments.get(&pon).cloned().unwrap_or_default()
    }

    pub fn rects_for_page(&self, pon: PageObjectNumber) -> Vec<Rect> {
        self.ctx
            .state()
            .segments
            .get(&pon)
            .map_or_else(Vec::new, |s| s.iter().map(|s| s.rect).collect())
    }

    // ── public lens: text extraction ──

    /// The selected text, pages joined with `\n`: "" with nothing selected,
    /// `PermissionDenied` without `doc.text.copy`.
    pub fn read_text(&self) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            let Some(sel) = this.ctx.state().selection else { return Ok(String::new()) };
            if !this.can_copy() {
                return Err(Error::permission_denied(COPY_CAPABILITY, Some("selection.readText")));
            }
            let Ordered { start, end, .. } = this.ordered_ends(&sel);
            let si = this.page_index_of(start.pon);
            let ei = this.page_index_of(end.pon);
            if si < 0 || ei < 0 {
                return Ok(String::new());
            }
            // Per-page half-open character spans; interior pages span their
            // whole character count, which the slice clamps to.
            let mut spans: Vec<(PageObjectNumber, i64, i64)> = Vec::new();
            for i in si..=ei {
                let Some(pon) = this.pon_at_index(i) else { continue };
                let from = if i == si { start.glyph } else { 0 };
                let to = if i == ei { end.glyph + 1 } else { MAX_SAFE_INTEGER };
                spans.push((pon, from, to));
            }
            let mut parts = Vec::with_capacity(spans.len());
            for batch in spans.chunks(TEXT_READ_CONCURRENCY) {
                let reads = batch.iter().map(|(pon, _, _)| this.page_text_snapshot(*pon)).collect();
                let snapshots = all(reads).await?;
                for (snap, (_, from, to)) in snapshots.iter().zip(batch) {
                    parts.push(slice_text_by_chars(snap, *from as f64, *to as f64));
                }
            }
            Ok(parts.join("\n"))
        })
    }

    // ── public lens: signals ──

    /// Fires whenever the selection segments change.
    pub fn on_change(&self, callback: impl Fn() + 'static) -> Unsubscriber {
        self.caches.change_cbs.add(Rc::new(callback))
    }

    /// Fires when a selection gesture ends (pointer-up).
    pub fn on_commit(&self, callback: impl Fn() + 'static) -> Unsubscriber {
        self.caches.commit_cbs.add(Rc::new(callback))
    }

    // ── host lens ──

    /// Warm a page's text geometry. Idempotent; inert without
    /// `doc.text.select`.
    pub fn ensure_page(&self, pon: PageObjectNumber) {
        if self.caches.raw_geometry.borrow().contains_key(&pon)
            || self.caches.pending.borrow().contains(&pon)
        {
            return;
        }
        if !self.can_select() {
            return;
        }
        let Some(doc) = self.ctx.doc() else { return };
        if self.layout_of(pon).is_none() {
            return;
        }
        self.caches.pending.borrow_mut().insert(pon);
        let at = self.caches.epoch.get();
        let read = doc.call("page.geometry.read", Some(pon), Vec::new());
        let this = self.clone();
        rt::spawn(async move {
            let result = read.await;
            this.caches.pending.borrow_mut().remove(&pon);
            // doc closed / read aborted / denied: ignore
            let Ok(value) = result else { return };
            if at != this.caches.epoch.get() {
                return; // content changed while in flight: stale
            }
            let Ok(snapshot) = decode::<PageGeometrySnapshot>(value) else { return };
            this.caches.raw_geometry.borrow_mut().insert(pon, Rc::new(snapshot));
            this.ctx.dispatch(SelectionAction::PageLoaded { pon });
            // a mid-span page arrived: fill its segments
            this.recompute_current();
        });
    }

    pub fn is_loaded(&self, pon: PageObjectNumber) -> bool {
        self.ctx.state().loaded.get(&pon).copied().unwrap_or(false)
    }

    /// Is a content-space point on (or near) text?
    pub fn is_over_text(&self, pon: PageObjectNumber, point: Point) -> bool {
        self.geometry_for(pon).is_some_and(|geom| Self::glyph_at(&geom, point).is_some())
    }

    /// Begin a caret selection at a page point; false when not near text.
    pub fn begin_at(&self, pon: PageObjectNumber, point: Point) -> bool {
        let Some(geom) = self.geometry_for(pon) else { return false };
        let Some(i) = Self::glyph_at(&geom, point) else { return false };
        self.set_selecting(true); // the gesture opened
        self.recompute(SelectionRange {
            anchor: GlyphPointer { pon, glyph: i },
            focus: GlyphPointer { pon, glyph: i },
        });
        true
    }

    /// Double-click or long-press: select the word; false when nothing was
    /// selected.
    pub fn select_word_at(&self, pon: PageObjectNumber, point: Point) -> bool {
        self.select_span_at(pon, point, Expand::Word)
    }

    /// Triple-click: select the visual line.
    pub fn select_line_at(&self, pon: PageObjectNumber, point: Point) -> bool {
        self.select_span_at(pon, point, Expand::Line)
    }

    /// Extend the current selection to a page point (drag).
    pub fn extend_to(&self, pon: PageObjectNumber, point: Point) {
        let Some(cur) = self.ctx.state().selection else { return };
        let Some(geom) = self.geometry_for(pon) else {
            self.ensure_page(pon); // dragged onto a page not loaded yet
            return;
        };
        let Some(i) = Self::glyph_at(&geom, point) else { return }; // off-text: keep the focus
        self.recompute(SelectionRange {
            anchor: cur.anchor,
            focus: GlyphPointer { pon, glyph: i },
        });
    }

    /// The gesture ended: settle first, so commit listeners see
    /// `is_selecting() == false`, then notify them.
    pub fn end(&self) {
        self.set_selecting(false);
        self.caches.commit_cbs.for_each();
    }

    /// Suppress or restore the default highlight visual.
    pub fn set_highlight_visible(&self, visible: bool) {
        self.ctx.dispatch(SelectionAction::SetHighlightHidden { hidden: !visible });
    }

    pub fn highlight_visible(&self) -> bool {
        !self.ctx.state().highlight_hidden
    }
}
