//! `controller.ts`: the selection controller. One object is both the public
//! lens and the host lens.
//!
//! State holds the range and its derived page-space segments. Engine data
//! lives outside state:
//!
//! - text geometry is a page mirror (`geometry`): each page's
//!   `page.text.layout` answer, loaded on demand, re-read when a confirmed
//!   event changes a page's content (redaction apply, flatten) and dropped
//!   when a page is deleted. The answer is the page-space geometry the
//!   layout is built from (a `$textLayout` marker or the bare snapshot); the
//!   built [`TextLayout`] is kept beside it, keyed by the answer's identity.
//! - `text_snapshots` holds per-page text promises for `read_text`, dropped
//!   on the same content changes and when a page leaves the registry.
//!
//! `recompute` rebuilds the segments of every loaded page in the span and
//! warms the rest, which recompute when their geometry arrives. The change
//! and clear events are derived from state changes in one place.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use epdf_geometry::js::number_to_string;
use epdf_geometry::{Point, Rect, bounds_of_rects, quad_bounds};
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::page_mirror::{Affected, PageMirror, PageMirrorChange, PageMirrorSpec};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::Value;

use crate::contract::*;
use crate::model::*;
use crate::text_layout::{
    PageGeometrySnapshot, PageTextSnapshot, TextLayout, TextRange as CharRange, create_text_layout,
    slice_text,
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

/// The page registry, decoded once per page list.
#[derive(Default)]
struct Registry {
    /// Each row's page, in order (`pages[i]?.ref`), `None` where its ref
    /// fails to decode: a malformed row costs only itself.
    pons: Vec<Option<PageObjectNumber>>,
    /// The first row naming each page, as `pages.find` finds it, with that
    /// row's `index` (`-1` when it has none, as `?.index ?? -1` reads).
    by_pon: HashMap<PageObjectNumber, i64>,
}

thread_local! {
    static REGISTRY_DECODES: Cell<usize> = const { Cell::new(0) };
}

/// How many times this thread has decoded a page registry. M22's guard
/// (`tests/scale.rs`) asserts a long select-all and read decode it a
/// small constant number of times, not once per page.
#[doc(hidden)]
pub fn registry_decodes() -> usize {
    REGISTRY_DECODES.with(Cell::get)
}

impl Registry {
    fn decode(pages: &Value) -> Self {
        REGISTRY_DECODES.with(|n| n.set(n.get() + 1));
        let rows = pages.as_array().map(Vec::as_slice).unwrap_or_default();
        let mut pons = Vec::with_capacity(rows.len());
        let mut by_pon = HashMap::with_capacity(rows.len());
        for row in rows {
            let pon = row.get("ref").and_then(PageRef::from_json).map(|r| r.page_object_number);
            if let Some(pon) = pon {
                let index = row.get("index").and_then(Value::as_f64).map_or(-1, |i| i as i64);
                by_pon.entry(pon).or_insert(index);
            }
            pons.push(pon);
        }
        Registry { pons, by_pon }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
    start: GlyphPosition,
    end: GlyphPosition,
    direction: SelectionDirection,
}

/// Everything the read model derives from one state object.
struct ReadModel {
    snapshot: SelectionSnapshot,
    pages: Vec<PageRef>,
    anchor: Option<SelectionAnchor>,
}

/// The read model's inputs: the range, the segments (by identity) and the
/// registry revision.
struct ModelKey {
    selection: Option<SelectionRange>,
    segments: SegmentsByPage,
    revision: u64,
}

impl ModelKey {
    fn matches(&self, state: &SelectionState, revision: u64) -> bool {
        self.revision == revision
            && self.selection == state.selection
            && same_segment_lists(&self.segments, &state.segments)
    }
}

/// Whether two segment maps hold the same lists: the identity
/// `previous.segments === next.segments` compares.
fn same_segment_lists(a: &SegmentsByPage, b: &SegmentsByPage) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka == kb && Rc::ptr_eq(va, vb))
}

/// A page's mirror answer and the layout built from it (`None` when the
/// answer held no geometry).
type BuiltLayout = (Rc<Value>, Option<Rc<TextLayout>>);

struct Core {
    cx: Controller<SelectionState, Value>,
    settings: InstanceSettings,
    geometry: PageMirror,
    /// Each page's built layout, beside the mirror answer it was built from.
    layouts: RefCell<HashMap<PageObjectNumber, BuiltLayout>>,
    text_snapshots: RefCell<HashMap<PageObjectNumber, Promise<Rc<PageTextSnapshot>>>>,
    /// Bumped on every content change; a text read in flight across one
    /// fails (M23).
    epoch: Cell<u64>,
    model: RefCell<Option<(ModelKey, Rc<ReadModel>)>>,
    /// The decoded registry and the page list it came from (M22).
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
/// element's reaction, or with the first rejection, and at once when empty
/// (S9). The same as `epdf_plugin_annotation::promise::all`, which this
/// crate can't reach: the annotation plugin depends on this one.
fn all<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Promise<Vec<T>> {
    if promises.is_empty() {
        return Promise::resolved(Vec::new());
    }
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

/// A content change landed while a text read was in flight: the pages
/// already read may hold text the change removed (a redaction). The
/// TypeScript returns that text; this is a Rust-only refusal (M23).
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

fn union_of(segments: Option<&Rc<Vec<SelectionSegment>>>) -> Option<Rect> {
    let rects: Vec<Rect> = segments?.iter().map(|s| s.rect).collect();
    bounds_of_rects(&rects)
}

fn page_json(page: PageRef) -> Value {
    page.to_json()
}

/// The mirror's key for a page: it reads only `objectNumber`.
fn page_key(page: PageRef) -> Value {
    serde_json::json!({ "objectNumber": page.page_object_number })
}

/// A `page.text.layout` answer read into a layout: the `$textLayout`
/// marker the conformance fake answers, or the bare page-space snapshot.
/// `None` when it is neither, which reads as no geometry.
fn hydrate(answer: &Value) -> Option<TextLayout> {
    let snapshot = answer.get("$textLayout").unwrap_or(answer);
    serde_json::from_value::<PageGeometrySnapshot>(snapshot.clone()).ok().map(create_text_layout)
}

/// The settings the store holds, and why the whole value did not decode
/// when it didn't: then they are read member by member, and a member of
/// the wrong type keeps its default.
fn settings_of(value: &Value) -> (SelectionSettings, Option<Error>) {
    serde_json::from_value(value.clone()).map(|settings| (settings, None)).unwrap_or_else(|e| {
        let defaults = selection_defaults();
        let config = SelectionConfig::from_json(value);
        let handles = config.handles.unwrap_or_default();
        let lenient = SelectionSettings {
            drag_threshold: config.drag_threshold.unwrap_or(defaults.drag_threshold),
            color: config.color.unwrap_or(defaults.color),
            handles: SelectionHandleSettings {
                color: handles.color.unwrap_or(defaults.handles.color),
                shadow: handles.shadow.unwrap_or(defaults.handles.shadow),
            },
        };
        (lenient, Some(Error::new(format!("[selection] malformed settings: {e}"))))
    })
}

/// `createSelectionController(ctx)`: the api and its `connect` half, which
/// watches the registry revision and the content-changing events and
/// registers the pointer handler.
pub(crate) fn create_selection_controller(
    cx: Controller<SelectionState, Value>,
) -> Result<(SelectionCapability, ConnectFn), Error> {
    let settings = cx.settings()?;
    let slot: Rc<RefCell<Weak<Core>>> = Rc::default();
    let on_change = slot.clone();
    let geometry = cx.page_mirror(PageMirrorSpec {
        name: "geometry".into(),
        load: Rc::new(|doc, page, _signal| {
            let Some(doc) = doc else {
                return Promise::rejected(Error::new("[selection] no document"));
            };
            let pon = PageRef::from_json(&page).map(|p| p.page_object_number);
            doc.call("page.text.layout", pon, Vec::new())
        }),
        affected: Rc::new(|event| match content_changed_pages_of(event) {
            Some(pages) => Affected::Pages(pages.into_iter().map(page_json).collect()),
            None => Affected::None,
        }),
        fold: None,
        predict: None,
        changed: Some(Rc::new(move |change: PageMirrorChange| {
            // A page's geometry arrived: a boundary page's segments can fill in.
            if change.cause != "load" {
                return Ok(());
            }
            if let Some(core) = on_change.borrow().upgrade() {
                let api = SelectionCapability(core);
                if let Some(current) = api.state().selection {
                    api.recompute(current);
                }
            }
            Ok(())
        })),
    })?;
    let core = Rc::new(Core {
        changed: cx.event_source(),
        committed: cx.event_source(),
        cleared: cx.event_source(),
        settings,
        geometry,
        cx,
        layouts: RefCell::default(),
        text_snapshots: RefCell::default(),
        epoch: Cell::new(0),
        model: RefCell::new(None),
        registry: RefCell::new(None),
    });
    *slot.borrow_mut() = Rc::downgrade(&core);
    let api = SelectionCapability(core);

    // ── events, derived from each state change ──
    let weak = Rc::downgrade(&api.0);
    api.0.cx.on_state_change(move |next, previous| {
        if previous.selection == next.selection
            && same_segment_lists(&previous.segments, &next.segments)
        {
            return Ok(());
        }
        let Some(core) = weak.upgrade() else { return Ok(()) };
        let api = SelectionCapability(core);
        let model = api.read_model();
        api.0.changed.emit(&SelectionChangedEvent {
            range: model.snapshot.range,
            pages: model.pages.clone(),
        });
        if previous.selection.is_some() && next.selection.is_none() {
            api.0.cleared.emit(&SelectionClearedEvent {});
        }
        Ok(())
    });

    let this = api.clone();
    let connect: ConnectFn = Rc::new(move || {
        let cx = &this.0.cx;
        // Registry changes (rotate, move, delete, insert) bump the revision:
        // `ctx.watch(() => ctx.document()?.revision ?? 0, onPagesUpdated)`.
        let (reader, watcher) = (Rc::downgrade(&this.0), Rc::downgrade(&this.0));
        cx.watch(
            move || Ok(reader.upgrade().map(|core| SelectionCapability(core).revision())),
            move |_, _| {
                if let Some(core) = watcher.upgrade() {
                    SelectionCapability(core).on_pages_updated();
                }
                Ok(())
            },
            |a, b| a == b,
        )?;
        let doc = cx.doc()?;
        let listener = Rc::downgrade(&this.0);
        cx.listen(doc.subscribe(Rc::new(move |event: &Value| {
            if let (Some(core), Some(pages)) = (listener.upgrade(), content_changed_pages_of(event))
            {
                SelectionCapability(core).on_content_changed(&pages);
            }
        })));
        crate::connect::connect_selection(cx, this.clone())
    });
    Ok((api, connect))
}

impl SelectionCapability {
    fn cx(&self) -> &Controller<SelectionState, Value> {
        &self.0.cx
    }

    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &AbortSignal {
        self.0.cx.lifetime()
    }

    fn state(&self) -> Rc<SelectionState> {
        self.0.cx.state()
    }

    fn update(&self, transition: impl FnOnce(&Rc<SelectionState>) -> Rc<SelectionState>) {
        self.0.cx.update_state(|state| transition(&state));
    }

    // ── page addressing (the registry is the truth for order and layout) ──

    /// The document's revision and page list, read in place: `document()`
    /// copies the whole meta, and these are read once per page of a span.
    fn meta_of<R>(&self, read: impl FnOnce(u64, &Rc<Value>) -> R) -> Option<R> {
        let core = self.cx().core();
        match self.cx().document_id().and_then(|id| core.documents.get(id)) {
            Some(meta) => Some(read(meta.revision, &meta.pages)),
            None => self.cx().document().map(|meta| read(meta.revision, &meta.pages)),
        }
    }

    /// `ctx.document()?.revision ?? 0`.
    fn revision(&self) -> u64 {
        self.meta_of(|revision, _| revision).unwrap_or(0)
    }

    /// The registry decoded once per page list: a lookup per page or per
    /// index would rescan and redecode it, which made a span O(n^2) and a
    /// settling select-all O(n^3) (M22). Keyed on the shared list's identity.
    fn registry(&self) -> Rc<Registry> {
        self.meta_of(|_, pages| {
            if let Some(cached) = self.0.registry.borrow().as_ref()
                && Rc::ptr_eq(&cached.0, pages)
            {
                return cached.1.clone();
            }
            let registry = Rc::new(Registry::decode(pages));
            *self.0.registry.borrow_mut() = Some((pages.clone(), registry.clone()));
            registry
        })
        .unwrap_or_default()
    }

    /// `pageIndexOf(page)`: `ctx.getPage(page)?.index ?? -1`.
    fn page_index_of(&self, page: PageRef) -> i64 {
        self.registry().by_pon.get(&page.page_object_number).copied().unwrap_or(-1)
    }

    /// `pageAtIndex(i)`: `ctx.document()?.pages[i]?.ref`.
    fn page_at_index(&self, i: i64) -> Option<PageRef> {
        let i = usize::try_from(i).ok()?;
        self.registry().pons.get(i).copied().flatten().map(PageRef::new)
    }

    /// `ctx.getPage(target)?.ref`: `None` for a page not in the document.
    fn get_page(&self, target: PageTarget) -> Option<PageRef> {
        match target {
            PageTarget::Ref(page) => {
                self.registry().by_pon.contains_key(&page.page_object_number).then_some(page)
            }
            PageTarget::Index(index) => {
                if index.fract() != 0.0 || index < 0.0 {
                    return None;
                }
                self.page_at_index(index as i64)
            }
        }
    }

    /// `ctx.pageOf(target).ref`: `not-found` for a page not in the document.
    fn page_of(&self, target: PageTarget) -> Result<PageRef, Error> {
        self.get_page(target).ok_or_else(|| {
            let message = match target {
                PageTarget::Ref(page) => {
                    format!("page {} is not in this document", page.page_object_number)
                }
                PageTarget::Index(index) => {
                    format!("there is no page {} in this document", number_to_string(index))
                }
            };
            plugin_error(code::NOT_FOUND, "selection", message, None, None)
        })
    }

    // ── text geometry ──

    /// A page's layout while it is loaded and current: a page whose re-read
    /// failed has none, exactly as `is_loaded` says.
    fn geometry_for(&self, page: PageRef) -> Option<Rc<TextLayout>> {
        let key = page_key(page);
        if self.0.geometry.get_status(&key) != ResourceStatus::Ready {
            return None;
        }
        let answer = self.0.geometry.get(&key)?;
        let pon = page.page_object_number;
        if let Some((built_from, layout)) = self.0.layouts.borrow().get(&pon)
            && Rc::ptr_eq(built_from, &answer)
        {
            return layout.clone();
        }
        let layout = hydrate(&answer).map(Rc::new);
        self.0.layouts.borrow_mut().insert(pon, (answer, layout.clone()));
        layout
    }

    fn glyph_at(layout: &TextLayout, point: Point) -> Option<i64> {
        layout.char_at(point).map(|i| i as i64)
    }

    /// Start loading a page's geometry, if this caller may read it and the
    /// page is in the document: the load `ensure_loaded` waits for.
    fn start_load(&self, page: PageRef) -> Option<Promise<()>> {
        // Authorized-only warming: without doc.text.select the read is
        // guaranteed to be refused, so it is not issued.
        if !self.can_select() || self.get_page(PageTarget::Ref(page)).is_none() {
            return None;
        }
        Some(self.0.geometry.ensure_loaded(page_json(page)))
    }

    /// `void ensureLoaded(page)`: warm a page nobody waits on. Its arrival
    /// recomputes through the mirror's `changed`.
    fn warm(&self, page: PageRef) {
        let _ = self.start_load(page);
    }

    // ── the range model ──

    fn ordered_ends(&self, selection: &SelectionRange) -> Ordered {
        let anchor_page = self.page_index_of(selection.anchor.page);
        let focus_page = self.page_index_of(selection.focus.page);
        let anchor_first = anchor_page < focus_page
            || (anchor_page == focus_page && selection.anchor.glyph <= selection.focus.glyph);
        if anchor_first {
            Ordered {
                start: selection.anchor,
                end: selection.focus,
                direction: SelectionDirection::Forward,
            }
        } else {
            Ordered {
                start: selection.focus,
                end: selection.anchor,
                direction: SelectionDirection::Backward,
            }
        }
    }

    /// Clamp a position into its page's real character range once the
    /// geometry is known.
    fn clamp_position(&self, position: GlyphPosition) -> GlyphPosition {
        let Some(layout) = self.geometry_for(position.page) else { return position };
        let max = (layout.char_count() as i64 - 1).max(0);
        GlyphPosition { page: position.page, glyph: position.glyph.min(max).max(0) }
    }

    /// Rebuild merged line segments for every loaded page in the span; warm
    /// the rest. Clears when an endpoint's page has left the registry.
    fn recompute(&self, selection: SelectionRange) {
        let clamped = SelectionRange {
            anchor: self.clamp_position(selection.anchor),
            focus: self.clamp_position(selection.focus),
        };
        let Ordered { start, end, .. } = self.ordered_ends(&clamped);
        let start_page = self.page_index_of(start.page);
        let end_page = self.page_index_of(end.page);
        if start_page < 0 || end_page < 0 {
            self.update(clear_selection);
            return;
        }
        let mut segments = SegmentsByPage::new();
        let registry = self.registry();
        for i in start_page..=end_page {
            let page =
                usize::try_from(i).ok().and_then(|i| registry.pons.get(i).copied().flatten());
            let Some(page) = page.map(PageRef::new) else { continue };
            let Some(layout) = self.geometry_for(page) else {
                self.warm(page); // recomputes on arrival
                continue;
            };
            let from = if i == start_page { start.glyph } else { 0 };
            let to = if i == end_page { end.glyph } else { layout.char_count() as i64 - 1 };
            let range = CharRange { start: from, count: to.saturating_sub(from).saturating_add(1) };
            segments.insert(page.page_object_number, Rc::new(layout.segments(range)));
        }
        self.update(|state| set_selection(state, clamped, segments));
    }

    // ── the read model, built once per selection and registry revision ──

    fn read_model(&self) -> Rc<ReadModel> {
        let state = self.state();
        let revision = self.revision();
        if let Some((key, model)) = self.0.model.borrow().as_ref()
            && key.matches(&state, revision)
        {
            return model.clone();
        }
        let model = Rc::new(self.build_read_model(&state));
        let key =
            ModelKey { selection: state.selection, segments: state.segments.clone(), revision };
        *self.0.model.borrow_mut() = Some((key, model.clone()));
        model
    }

    fn build_read_model(&self, state: &SelectionState) -> ReadModel {
        let segments = &state.segments;
        let mut with_segments: Vec<PageRef> = segments
            .iter()
            .filter(|(_, list)| !list.is_empty())
            .map(|(pon, _)| PageRef::new(*pon))
            .collect();
        // A stable sort by page index, as `Array.prototype.sort` is.
        let registry = self.registry();
        with_segments.sort_by_key(|page| {
            registry.by_pon.get(&page.page_object_number).copied().unwrap_or(-1)
        });
        let snapshot_pages: Vec<SnapshotPage> = with_segments
            .iter()
            .map(|page| SnapshotPage {
                page: *page,
                segments: segments[&page.page_object_number].clone(),
            })
            .collect();
        let Some(selection) = state.selection else {
            return ReadModel {
                snapshot: SelectionSnapshot {
                    pages: snapshot_pages,
                    start: None,
                    end: None,
                    direction: SelectionDirection::Forward,
                    range: None,
                },
                pages: with_segments,
                anchor: None,
            };
        };
        let Ordered { start, end, direction } = self.ordered_ends(&selection);
        let snapshot = SelectionSnapshot {
            pages: snapshot_pages,
            start: self.endpoint_for(segments, start, Which::Start),
            end: self.endpoint_for(segments, end, Which::End),
            direction,
            range: Some(TextRange {
                start: TextPosition { page: start.page, index: start.glyph },
                end: TextPosition { page: end.page, index: end.glyph.saturating_add(1) },
            }),
        };
        // Prefer the gesture's end page; while its geometry is still
        // loading, fall back to the last page (document order) with
        // segments, so the anchor never jumps backwards mid-drag.
        let mut anchor = union_of(segments.get(&end.page.page_object_number))
            .map(|bounds| SelectionAnchor { page: end.page, bounds });
        if anchor.is_none()
            && let Some(last) = with_segments.last()
            && let Some(bounds) = union_of(segments.get(&last.page_object_number))
        {
            anchor = Some(SelectionAnchor { page: *last, bounds });
        }
        ReadModel { snapshot, pages: with_segments, anchor }
    }

    fn endpoint_for(
        &self,
        segments_by_page: &SegmentsByPage,
        position: GlyphPosition,
        which: Which,
    ) -> Option<SelectionEndpoint> {
        let segments = segments_by_page.get(&position.page.page_object_number)?;
        let segment = if which == Which::Start { segments.first()? } else { segments.last()? };
        // The boundary glyph's own cell, so caret placement lands on the
        // exact character edge; the segment when the glyph has no box.
        let cell = self.geometry_for(position.page).and_then(|l| l.char_quad(position.glyph));
        Some(match cell {
            Some(cell) => SelectionEndpoint {
                page: position.page,
                glyph_quad: cell,
                advance: segment.advance,
                rect: quad_bounds(cell),
            },
            None => SelectionEndpoint {
                page: position.page,
                glyph_quad: segment.quad,
                advance: segment.advance,
                rect: segment.rect,
            },
        })
    }

    // ── span writes ──

    /// Word or line around a point. Whether a span engaged (geometry
    /// present and a glyph under the point): what haptics key on.
    fn select_span_at(
        &self,
        target: PageTarget,
        point: Point,
        expand: Expand,
    ) -> Result<bool, Error> {
        let operation = match expand {
            Expand::Word => "selection.selectWordAt",
            Expand::Line => "selection.selectLineAt",
        };
        self.cx().assert_allowed(SELECT_SCOPE, operation)?;
        let page = self.page_of(target)?;
        let Some(layout) = self.geometry_for(page) else { return Ok(false) };
        let Some(glyph) = Self::glyph_at(&layout, point) else { return Ok(false) };
        let span = match expand {
            Expand::Word => layout.word_at(glyph),
            Expand::Line => layout.line_at(glyph),
        };
        let Some(span) = span else { return Ok(false) };
        self.recompute(SelectionRange {
            anchor: GlyphPosition { page, glyph: span.start },
            focus: GlyphPosition { page, glyph: span.start.saturating_add(span.count) - 1 },
        });
        Ok(true)
    }

    // ── text extraction ──

    /// Per-page text, cached as a promise so concurrent readers share one
    /// fetch; a rejection is evicted.
    fn page_text(&self, page: PageRef) -> Promise<Rc<PageTextSnapshot>> {
        let pon = page.page_object_number;
        if let Some(p) = self.0.text_snapshots.borrow().get(&pon) {
            return p.clone();
        }
        let call = match self.cx().doc() {
            Ok(doc) => doc.call_answer("page.text.get", Some(pon), Vec::new()),
            Err(error) => Promise::rejected(error),
        };
        // `Promise.resolve(abortable)`: an AbortablePromise is a subclass, so
        // this adopts it as a thenable, one turn more than awaiting.
        let pending = Promise::spawn(async move {
            rt::tick().await;
            let value = call.await?;
            value.decode::<PageTextSnapshot>().map(Rc::new)
        });
        let core = Rc::downgrade(&self.0);
        let watched = pending.clone();
        rt::spawn(async move {
            if watched.clone().await.is_err()
                && let Some(core) = core.upgrade()
            {
                let mut cache = core.text_snapshots.borrow_mut();
                if cache.get(&pon).is_some_and(|p| p.same(&watched)) {
                    cache.remove(&pon);
                }
            }
        });
        self.0.text_snapshots.borrow_mut().insert(pon, pending.clone());
        pending
    }

    /// A range's text, read a few pages at a time. Cancelling stops the
    /// wait at once; the page reads it started finish into the cache.
    async fn read_range(
        &self,
        range: TextRange,
        options: OperationOptions,
    ) -> Result<String, Error> {
        // The content generation the read starts in (M23).
        let at = self.0.epoch.get();
        self.cx().assert_page_ref(range.start.page)?;
        self.cx().assert_page_ref(range.end.page)?;
        let start_page = self.page_index_of(range.start.page);
        let mut end_page = self.page_index_of(range.end.page);
        let mut end_index = range.end.index;
        if end_page < start_page || (end_page == start_page && end_index <= range.start.index) {
            return Ok(String::new());
        }
        if end_index == 0 {
            end_page -= 1; // a range ending at a page boundary includes nothing of that page
            end_index = OPEN_END;
        }
        // Per-page half-open character spans; interior pages span their
        // whole text, which the slice clamps to.
        let mut spans: Vec<(PageRef, i64, i64)> = Vec::new();
        for i in start_page..=end_page {
            let Some(page) = self.page_at_index(i) else { continue };
            let from = if i == start_page { range.start.index.max(0) } else { 0 };
            let to = if i == end_page { end_index } else { OPEN_END };
            spans.push((page, from, to));
        }
        let signal = options.signal;
        let mut parts = Vec::with_capacity(spans.len());
        for batch in spans.chunks(TEXT_READ_CONCURRENCY) {
            // A signal that has fired starts no more reads.
            if signal.as_ref().is_some_and(AbortSignal::aborted) {
                return Err(plugin_error(
                    code::OPERATION_CANCELLED,
                    "selection",
                    "reading the text was cancelled",
                    None,
                    None,
                ));
            }
            let reads = all(batch.iter().map(|(page, _, _)| self.page_text(*page)).collect());
            let snapshots = cancellable("selection", signal.clone(), reads).await?;
            throw_if_stale(at, self.0.epoch.get())?;
            for (snapshot, (_, from, to)) in snapshots.iter().zip(batch) {
                parts.push(slice_text(snapshot, *from as f64, (*to as f64) - (*from as f64)));
            }
        }
        Ok(parts.join("\n"))
    }

    // ── invalidation ──

    /// A structural registry change (rotate, move, delete, insert): drop
    /// the text of pages that left, then recompute. A deleted endpoint page
    /// clears through recompute's registry check.
    fn on_pages_updated(&self) {
        let alive: HashSet<PageObjectNumber> =
            self.registry().pons.iter().flatten().copied().collect();
        self.0.text_snapshots.borrow_mut().retain(|pon, _| alive.contains(pon));
        self.0.layouts.borrow_mut().retain(|pon, _| alive.contains(pon));
        if let Some(current) = self.state().selection {
            self.recompute(current);
        }
    }

    /// Page content changed: the pages' text is stale, and a live range
    /// points into a character space that no longer exists, so it clears.
    /// The geometry mirror re-reads those pages itself.
    fn on_content_changed(&self, pages: &[PageRef]) {
        self.0.epoch.set(self.0.epoch.get() + 1);
        for page in pages {
            self.0.text_snapshots.borrow_mut().remove(&page.page_object_number);
        }
        self.update(clear_selection);
    }

    fn set_gesture(&self, active: bool) {
        self.update(|state| set_selecting(state, active));
    }

    // ── settings ──

    /// The settings, for every open document. The typed API only writes
    /// well-typed members, but a host can write any JSON through the
    /// kernel's settings store; a member of the wrong type reads as its
    /// default here (where the TypeScript hands the value back), and
    /// `on_settings_changed` reports it.
    pub fn get_settings(&self) -> SelectionSettings {
        settings_of(&self.0.settings.get()).0
    }

    /// Merge `changes` into the settings, for every document. Fires
    /// `on_settings_changed` once when a setting changed.
    pub fn update_settings(&self, changes: &SelectionConfig) {
        self.0.settings.store.update_settings(&changes.to_json());
    }

    /// Go back to what the app registered.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    /// Fires once for each call that changed a setting. This instance's
    /// listeners go when its document closes.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&SelectionSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.settings.events.on(move |event| {
            let (settings, malformed) = settings_of(&event.settings);
            listener(&SelectionSettingsChanged { settings, changed: event.changed.clone() })?;
            // After the listener, so a malformed member is reported without
            // keeping the change from it.
            malformed.map_or(Ok(()), Err)
        })
    }

    /// The drag threshold, read when a press would become a drag.
    pub(crate) fn drag_threshold(&self) -> f64 {
        self.0.settings.get().get("dragThreshold").and_then(Value::as_f64).unwrap_or(4.0)
    }

    // ── public lens: authorization ──

    /// Whether this caller may create selections (`doc.text.select`).
    pub fn can_select(&self) -> bool {
        self.cx().allows(SELECT_SCOPE)
    }

    /// Whether this caller may extract literal text (`doc.text.copy`).
    pub fn can_copy(&self) -> bool {
        self.cx().allows(COPY_SCOPE)
    }

    // ── public lens: writes ──

    /// Select a range. An empty range clears. `permission-denied` without
    /// `doc.text.select`, `not-found` for a page not in this document.
    pub fn select(&self, input: SelectionRangeInput) -> Result<(), Error> {
        self.cx().assert_allowed(SELECT_SCOPE, "selection.select")?;
        let range = match input {
            SelectionRangeInput::Range(range) => range,
            SelectionRangeInput::Span { page, start, count } => TextRange {
                start: TextPosition { page, index: start },
                // S3: saturate where the TypeScript's float sum never overflows.
                end: TextPosition { page, index: start.saturating_add(count) },
            },
        };
        self.cx().assert_page_ref(range.start.page)?;
        self.cx().assert_page_ref(range.end.page)?;
        let start_page = self.page_index_of(range.start.page);
        let mut end_page = self.page_index_of(range.end.page);
        let mut end_index = range.end.index;
        if end_page < start_page || (end_page == start_page && end_index <= range.start.index) {
            self.update(clear_selection); // an empty range is no selection
            return Ok(());
        }
        if end_index == 0 {
            // A half-open end exactly at a page boundary: the last included
            // character is the previous page's last one, which settles by
            // clamping on load.
            end_page -= 1;
            if end_page < start_page {
                self.update(clear_selection);
                return Ok(());
            }
            end_index = OPEN_END;
        }
        let Some(focus_page) = self.page_at_index(end_page) else { return Ok(()) };
        self.recompute(SelectionRange {
            anchor: GlyphPosition { page: range.start.page, glyph: range.start.index.max(0) },
            focus: GlyphPosition { page: focus_page, glyph: end_index.saturating_sub(1) },
        });
        Ok(())
    }

    /// Select every character of the document.
    pub fn select_all(&self) -> Result<(), Error> {
        self.cx().assert_allowed(SELECT_SCOPE, "selection.selectAll")?;
        let registry = self.registry();
        let (Some(Some(first)), Some(Some(last))) = (registry.pons.first(), registry.pons.last())
        else {
            return Ok(());
        };
        self.recompute(SelectionRange {
            anchor: GlyphPosition { page: PageRef::new(*first), glyph: 0 },
            focus: GlyphPosition { page: PageRef::new(*last), glyph: OPEN_END },
        });
        Ok(())
    }

    /// Select every character on one page.
    pub fn select_page(&self, target: impl Into<PageTarget>) -> Result<(), Error> {
        self.cx().assert_allowed(SELECT_SCOPE, "selection.selectPage")?;
        let page = self.page_of(target.into())?;
        self.recompute(SelectionRange {
            anchor: GlyphPosition { page, glyph: 0 },
            focus: GlyphPosition { page, glyph: OPEN_END },
        });
        Ok(())
    }

    /// Select the word around a page point; false when the point has no
    /// selectable text (geometry not loaded yet, or no glyph there).
    pub fn select_word_at(
        &self,
        target: impl Into<PageTarget>,
        point: Point,
    ) -> Result<bool, Error> {
        self.select_span_at(target.into(), point, Expand::Word)
    }

    /// Select the visual line around a page point.
    pub fn select_line_at(
        &self,
        target: impl Into<PageTarget>,
        point: Point,
    ) -> Result<bool, Error> {
        self.select_span_at(target.into(), point, Expand::Line)
    }

    /// Extend the current selection to a page point. No selection is a
    /// no-op; a page whose geometry has not loaded is warmed and the
    /// extension lands when it arrives.
    pub fn extend_to(&self, target: impl Into<PageTarget>, point: Point) -> Result<(), Error> {
        self.cx().assert_allowed(SELECT_SCOPE, "selection.extendTo")?;
        let page = self.page_of(target.into())?;
        let Some(current) = self.state().selection else { return Ok(()) };
        let Some(layout) = self.geometry_for(page) else {
            self.warm(page); // lands on arrival
            return Ok(());
        };
        let Some(glyph) = Self::glyph_at(&layout, point) else { return Ok(()) }; // off-text
        self.recompute(SelectionRange {
            anchor: current.anchor,
            focus: GlyphPosition { page, glyph },
        });
        Ok(())
    }

    /// Clear the selection. Always allowed.
    pub fn clear(&self) {
        self.update(clear_selection);
    }

    // ── public lens: reads ──

    pub fn has_selection(&self) -> bool {
        self.state().selection.is_some()
    }

    /// Whether the user is still selecting: from the press until it lifts.
    pub fn is_selecting(&self) -> bool {
        self.state().selecting
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

    /// Per-line oriented segments for a page, in page space. A page that
    /// isn't in the document has none.
    pub fn list_segments(&self, target: impl Into<PageTarget>) -> Vec<SelectionSegment> {
        let Some(page) = self.get_page(target.into()) else { return Vec::new() };
        self.state()
            .segments
            .get(&page.page_object_number)
            .map(|list| (**list).clone())
            .unwrap_or_default()
    }

    /// The segments' boxes.
    pub fn list_rects(&self, target: impl Into<PageTarget>) -> Vec<Rect> {
        self.list_segments(target).iter().map(|s| s.rect).collect()
    }

    /// Where selection-scoped floating UI attaches: the union box on the
    /// end page, else the last page with segments.
    pub fn get_anchor(&self) -> Option<SelectionAnchor> {
        self.read_model().anchor
    }

    // ── public lens: text extraction ──

    /// The selected text: `""` with nothing selected. Rejects
    /// `permission-denied` without `doc.text.copy`, `operation-cancelled`
    /// when the signal fires.
    pub fn read_text(&self, options: OperationOptions) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            this.cx().assert_allowed(COPY_SCOPE, "selection.readText")?;
            match this.read_model().snapshot.range {
                Some(range) => this.read_range(range, options).await,
                None => Ok(String::new()),
            }
        })
    }

    /// Text for any range, without touching the selection. Pages join with
    /// `\n`. Rejects as `read_text` does, and `not-found` for a page not in
    /// this document.
    pub fn read_text_in_range(
        &self,
        range: TextRange,
        options: OperationOptions,
    ) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            this.cx().assert_allowed(COPY_SCOPE, "selection.readTextInRange")?;
            this.read_range(range, options).await
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

    /// Warm a page's text geometry. Resolves once loaded; at once without
    /// `doc.text.select` or for an unknown page; and on a failed read.
    pub fn ensure_loaded(&self, page: PageRef) -> Promise<()> {
        let Some(load) = self.start_load(page) else { return Promise::resolved(()) };
        let load = load.reaction();
        // refused or failed: nothing to paint, and a later call retries
        Promise::spawn(async move {
            let _ = load.await;
            Ok(())
        })
    }

    /// Whether the page's text geometry is loaded and current.
    pub fn is_loaded(&self, page: PageRef) -> bool {
        self.0.geometry.get_status(&page_key(page)) == ResourceStatus::Ready
    }

    /// Is a page-space point on (or near) text? Drives the I-beam cursor.
    pub fn is_over_text(&self, page: PageRef, point: Point) -> bool {
        self.geometry_for(page).is_some_and(|layout| Self::glyph_at(&layout, point).is_some())
    }

    /// A pointer gesture that drives the selection opened.
    pub fn begin_gesture(&self) {
        self.set_gesture(true);
    }

    /// Open a caret selection at a page point; false when the point has no
    /// text, so the caller lets the gesture go.
    pub fn begin_gesture_at(&self, page: PageRef, point: Point) -> bool {
        if !self.can_select() {
            return false;
        }
        let Some(layout) = self.geometry_for(page) else { return false };
        let Some(glyph) = Self::glyph_at(&layout, point) else { return false };
        // The gesture opens before the selection changes, so listeners see
        // a coherent pair.
        self.set_gesture(true);
        self.recompute(SelectionRange {
            anchor: GlyphPosition { page, glyph },
            focus: GlyphPosition { page, glyph },
        });
        true
    }

    /// The gesture ended: settle first, so commit listeners observe
    /// `is_selecting() == false`, then commit a selection in place.
    pub fn end_gesture(&self) {
        self.set_gesture(false);
        if let Some(range) = self.read_model().snapshot.range {
            self.0.committed.emit(&SelectionCommittedEvent { range });
        }
    }

    /// Suppress or restore the default highlight visual.
    pub fn set_highlight_visible(&self, visible: bool) {
        self.update(|state| set_highlight_hidden(state, !visible));
    }

    pub fn is_highlight_visible(&self) -> bool {
        !self.state().highlight_hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_of_nothing_settles_empty() {
        rt::reset();
        let none = all::<u32>(Vec::new());
        rt::run_until_stalled();
        assert_eq!(none.peek().expect("settled").expect("resolved"), Vec::<u32>::new());
    }
}
