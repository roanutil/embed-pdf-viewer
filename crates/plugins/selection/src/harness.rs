//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! tables for `plugins/selection-geometry.json` and
//! `plugins/selection-handles.json`, each entry named as the generator
//! (`tooling/plugin-conformance/src/suites/selection.ts`) names it.
//!
//! It mirrors the generator's shim: a `readText` or `readTextInRange` option
//! `signal: {"$abortSignal": name}` is the signal of a controller named
//! `name`, which the `selection-signals` test plugin's `abort(name)` aborts.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::Point;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::geometry::{
    build_selection_page_geometry, content_point_to_pdf, to_content_segment, to_content_text_quad,
};
use crate::handles::*;
use crate::text_layout::*;
use crate::*;

fn arg<T: DeserializeOwned>(args: &[Value], i: usize) -> Result<T, Error> {
    let value = match args.get(i) {
        Some(v) if !is_undefined(v) => v.clone(),
        _ => Value::Null,
    };
    from_value(&value).map_err(|e| Error::new(format!("[harness] argument {i}: {}", e.message)))
}

fn present(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v) && !v.is_null())
}

fn out<T: Serialize>(value: T) -> Result<Value, Error> {
    Ok(to_value(&value))
}

fn page_arg(args: &[Value], i: usize) -> Result<PageRef, Error> {
    present(args, i)
        .and_then(PageRef::from_json)
        .ok_or_else(|| Error::named("TypeError", "[selection] not a PageRef"))
}

/// A `select` argument: a span when it has a `page` member, else a range.
fn range_input(value: &Value) -> Result<SelectionRangeInput, Error> {
    let bad = |e: Error| Error::new(format!("[harness] select: {}", e.message));
    if value.get("page").is_some() {
        Ok(SelectionRangeInput::Span {
            page: page_arg(std::slice::from_ref(&value["page"]), 0)?,
            start: from_value(&value["start"]).map_err(bad)?,
            count: from_value(&value["count"]).map_err(bad)?,
        })
    } else {
        from_value::<TextRange>(value).map(SelectionRangeInput::Range).map_err(bad)
    }
}

// ── abort signals ─────────────────────────────────────────────────────────────

thread_local! {
    /// The case's abort controllers by name; a selection build starts a case.
    static CONTROLLERS: RefCell<HashMap<String, AbortSignal>> = RefCell::new(HashMap::new());
}

fn controller_named(name: &str) -> AbortSignal {
    CONTROLLERS.with(|c| c.borrow_mut().entry(name.to_string()).or_default().clone())
}

fn options_of(options: Option<&Value>) -> OperationOptions {
    let signal = options
        .and_then(|o| o.get("signal"))
        .and_then(|s| s.get("$abortSignal"))
        .and_then(Value::as_str)
        .map(controller_named);
    OperationOptions { signal }
}

/// The `selection-signals` test plugin's capability.
#[derive(Clone)]
pub struct Signals;

pub static SIGNALS: LazyLock<Token<Signals>> =
    LazyLock::new(|| Token::new("selection-test/signals"));

// ── the adapter ───────────────────────────────────────────────────────────────

pub struct Selection;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "canSelect",
    "canCopy",
    "select",
    "selectAll",
    "selectPage",
    "selectWordAt",
    "selectLineAt",
    "extendTo",
    "clear",
    "hasSelection",
    "getSnapshot",
    "getRange",
    "listSelectedPages",
    "listSegments",
    "listRects",
    "getAnchor",
    "readText",
    "readTextInRange",
    "onChanged",
    "onCommitted",
    "onCleared",
    "ensureLoaded",
    "isLoaded",
    "isOverText",
    "beginGesture",
    "beginGestureAt",
    "endGesture",
    "isGestureActive",
    "setHighlightVisible",
    "isHighlightVisible",
];

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

fn call_selection(
    cap: &SelectionCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let page = || page_arg(args, 0);
    let point = || arg::<Point>(args, 1);
    Ok(match method {
        "canSelect" => Outcome::Returned(json!(cap.can_select())),
        "canCopy" => Outcome::Returned(json!(cap.can_copy())),
        "select" => unit(cap.select(range_input(args.first().unwrap_or(&Value::Null))?)),
        "selectAll" => unit(cap.select_all()),
        "selectPage" => unit(cap.select_page(page()?)),
        "selectWordAt" => {
            Outcome::from_result(cap.select_word_at(page()?, point()?).map(|b| json!(b)))
        }
        "selectLineAt" => {
            Outcome::from_result(cap.select_line_at(page()?, point()?).map(|b| json!(b)))
        }
        "extendTo" => unit(cap.extend_to(page()?, point()?)),
        "clear" => {
            cap.clear();
            Outcome::undefined()
        }
        "hasSelection" => Outcome::Returned(json!(cap.has_selection())),
        "getSnapshot" => Outcome::Returned(to_value(&cap.get_snapshot())),
        "getRange" => Outcome::Returned(to_value(&cap.get_range())),
        "listSelectedPages" => Outcome::Returned(json!(
            cap.list_selected_pages().into_iter().map(PageRef::to_json).collect::<Vec<_>>()
        )),
        "listSegments" => Outcome::Returned(to_value(&cap.list_segments(page()?))),
        "listRects" => Outcome::Returned(to_value(&cap.list_rects(page()?))),
        "getAnchor" => Outcome::Returned(to_value(&cap.get_anchor())),
        "readText" => Outcome::promise(cap.read_text(options_of(present(args, 0)))),
        "readTextInRange" => {
            let range: TextRange = arg(args, 0)?;
            Outcome::promise(cap.read_text_in_range(range, options_of(present(args, 1))))
        }
        "onChanged" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_changed(f))
        }
        "onCommitted" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_committed(f))
        }
        "onCleared" => {
            let cap = cap.clone();
            hook(h, present(args, 0), move |f| cap.on_cleared(f))
        }
        "ensureLoaded" => Outcome::promise_unit(cap.ensure_loaded(page()?)),
        "isLoaded" => Outcome::Returned(json!(cap.is_loaded(page()?))),
        "isOverText" => Outcome::Returned(json!(cap.is_over_text(page()?, point()?))),
        "beginGesture" => {
            cap.begin_gesture();
            Outcome::undefined()
        }
        "beginGestureAt" => Outcome::Returned(json!(cap.begin_gesture_at(page()?, point()?))),
        "endGesture" => {
            cap.end_gesture();
            Outcome::undefined()
        }
        "isGestureActive" => Outcome::Returned(json!(cap.is_gesture_active())),
        "setHighlightVisible" => {
            cap.set_highlight_visible(arg::<bool>(args, 0)?);
            Outcome::undefined()
        }
        "isHighlightVisible" => Outcome::Returned(json!(cap.is_highlight_visible())),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for Selection {
    fn factories(&self) -> Vec<&'static str> {
        vec!["selection", "selection-signals"]
    }

    fn build(&self, factory: &str, config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "selection-signals" {
            return Ok(define::<Value, Value>("selection-test/signals")
                .capability(&SIGNALS, |_| Ok(Signals))
                .build());
        }
        CONTROLLERS.with(|c| c.borrow_mut().clear());
        let config: SelectionConfig = if is_undefined(config) || config.is_null() {
            SelectionConfig::default()
        } else {
            serde_json::from_value(config.clone())
                .map_err(|e| Error::new(format!("[selection] bad config: {e}")))?
        };
        Ok(selection_plugin(config))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![SELECTION.any().clone(), SIGNALS.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if capability.downcast_ref::<Signals>().is_some() {
            return Some(match method {
                "abort" => {
                    let name = args.first().and_then(Value::as_str).unwrap_or_default();
                    controller_named(name).abort(Error::abort(None));
                    Outcome::undefined()
                }
                other => {
                    Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
                }
            });
        }
        let cap = capability.downcast_ref::<SelectionCapability>()?;
        Some(call_selection(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if capability.downcast_ref::<Signals>().is_some() {
            return Some(vec!["abort"]);
        }
        capability.downcast_ref::<SelectionCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── plugins/selection-geometry.json ───────────────────────────────────────────

/// The generator's `geomOf`: `{snapshot, crop, rotation, userUnit}`.
fn geom_of(value: &Value) -> Result<SelectionPageGeometry, Error> {
    let field = |name: &str| value.get(name).cloned().unwrap_or(Value::Null);
    build_page_geometry(&[field("snapshot"), field("crop"), field("rotation"), field("userUnit")])
}

fn build_page_geometry(args: &[Value]) -> Result<SelectionPageGeometry, Error> {
    let snapshot: PageGeometrySnapshot = arg(args, 0)?;
    build_selection_page_geometry(&snapshot, arg(args, 1)?, arg(args, 2)?, arg(args, 3)?)
        .map_err(|e| Error::new(e.to_string()))
}

fn layout_arg(args: &[Value]) -> Result<PageTextLayout, Error> {
    Ok(build_page_text_layout(&arg(args, 0)?))
}

fn build_selection_page_geometry_fn(args: &[Value]) -> Result<Value, Error> {
    out(build_page_geometry(args)?)
}

fn content_point_to_pdf_fn(args: &[Value]) -> Result<Value, Error> {
    out(content_point_to_pdf(&geom_of(&args[0])?, arg(args, 1)?))
}

fn to_content_text_quad_fn(args: &[Value]) -> Result<Value, Error> {
    out(to_content_text_quad(&geom_of(&args[0])?, &arg(args, 1)?))
}

fn to_content_segment_fn(args: &[Value]) -> Result<Value, Error> {
    out(to_content_segment(&geom_of(&args[0])?, &arg(args, 1)?))
}

fn build_page_text_layout_fn(args: &[Value]) -> Result<Value, Error> {
    out(layout_arg(args)?)
}

fn text_glyph_at_fn(args: &[Value]) -> Result<Value, Error> {
    let tolerance: Option<f64> = arg(args, 2)?;
    let tolerance = tolerance.unwrap_or(DEFAULT_TOLERANCE_FACTOR);
    out(text_glyph_at(&layout_arg(args)?, arg(args, 1)?, tolerance))
}

fn expand_word_fn(args: &[Value]) -> Result<Value, Error> {
    let (from, to) = expand_text_range_to_word(&layout_arg(args)?, arg(args, 1)?);
    out([from, to])
}

fn expand_line_fn(args: &[Value]) -> Result<Value, Error> {
    let (from, to) = expand_text_range_to_line(&layout_arg(args)?, arg(args, 1)?);
    out([from, to])
}

fn text_glyph_quad_fn(args: &[Value]) -> Result<Value, Error> {
    out(text_glyph_quad(&layout_arg(args)?, arg(args, 1)?))
}

fn text_segments_fn(args: &[Value]) -> Result<Value, Error> {
    out(text_segments_for_range(&layout_arg(args)?, arg(args, 1)?, arg(args, 2)?))
}

fn slice_text_fn(args: &[Value]) -> Result<Value, Error> {
    let snapshot: PageTextSnapshot = arg(args, 0)?;
    out(slice_text_by_chars(&snapshot, arg(args, 1)?, arg(args, 2)?))
}

fn initial_state_fn(_: &[Value]) -> Result<Value, Error> {
    out(initial_selection_state())
}

fn reducer_fn(args: &[Value]) -> Result<Value, Error> {
    // Integer-keyed records: plain serde_json reads their string keys.
    let plain = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let state: SelectionState =
        serde_json::from_value(plain(0)).map_err(|e| Error::new(e.to_string()))?;
    let action: SelectionAction =
        serde_json::from_value(plain(1)).map_err(|e| Error::new(e.to_string()))?;
    out(reduce_selection(&state, &action).unwrap_or(state))
}

/// The generator's `layoutSweep`: segments from every start in `[-1, n]`
/// for the counts `{0, 1, 2, 3, 5, 8, n - from, n + 1 - from}` (a `Set`,
/// so in that order, once each), then word, line and quad per glyph.
fn layout_sweep_fn(args: &[Value]) -> Result<Value, Error> {
    let layout = layout_arg(args)?;
    let n = layout.glyphs.len() as i64;
    let mut segments = Vec::new();
    for from in -1..=n {
        let mut counts: Vec<i64> = Vec::new();
        for count in [0, 1, 2, 3, 5, 8, n - from, n + 1 - from] {
            if !counts.contains(&count) {
                counts.push(count);
            }
        }
        for count in counts {
            segments.push(to_value(&text_segments_for_range(&layout, from, count)));
        }
    }
    let glyphs = -1..=n;
    let words: Vec<[i64; 2]> = glyphs
        .clone()
        .map(|g| {
            let (a, b) = expand_text_range_to_word(&layout, g);
            [a, b]
        })
        .collect();
    let lines: Vec<[i64; 2]> = glyphs
        .clone()
        .map(|g| {
            let (a, b) = expand_text_range_to_line(&layout, g);
            [a, b]
        })
        .collect();
    let quads: Vec<Option<PdfQuad>> = glyphs.map(|g| text_glyph_quad(&layout, g)).collect();
    Ok(
        json!({ "segments": segments, "words": to_value(&words), "lines": to_value(&lines), "quads": to_value(&quads) }),
    )
}

/// The generator's `glyphAtGrid`: row-major, the steps accumulated as the
/// TypeScript's `+=` does.
fn glyph_at_grid_fn(args: &[Value]) -> Result<Value, Error> {
    let layout = layout_arg(args)?;
    let grid = args.get(1).cloned().unwrap_or(Value::Null);
    let n = |k: &str| grid[k].as_f64().unwrap_or(f64::NAN);
    let tolerance: Option<f64> = arg(args, 2)?;
    let tolerance = tolerance.unwrap_or(DEFAULT_TOLERANCE_FACTOR);
    let mut hits = Vec::new();
    let mut y = n("y0");
    while y <= n("y1") {
        let mut x = n("x0");
        while x <= n("x1") {
            hits.push(text_glyph_at(&layout, Point { x, y }, tolerance));
            x += n("step");
        }
        y += n("step");
    }
    out(hits)
}

/// The generator's `pairProbe`.
fn pair_probe_fn(args: &[Value]) -> Result<Value, Error> {
    let layout = layout_arg(args)?;
    let n = layout.glyphs.len() as i64;
    let (a, b) = expand_text_range_to_line(&layout, 0);
    let (c, d) = expand_text_range_to_line(&layout, n - 1);
    Ok(json!({
        "segments": to_value(&text_segments_for_range(&layout, 0, n)),
        "first": [a, b],
        "last": [c, d],
    }))
}

/// The generator's `sliceSweep`: every `(from, to)` in `[-1, charCount + 1]`.
fn slice_sweep_fn(args: &[Value]) -> Result<Value, Error> {
    let snapshot: PageTextSnapshot = arg(args, 0)?;
    let mut out = Vec::new();
    let mut from = -1.0;
    while from <= snapshot.char_count + 1.0 {
        let mut to = from;
        while to <= snapshot.char_count + 1.0 {
            out.push(slice_text_by_chars(&snapshot, from, to));
            to += 1.0;
        }
        from += 1.0;
    }
    Ok(json!(out))
}

pub fn geometry_functions() -> FunctionTable {
    vec![
        ("buildSelectionPageGeometry", build_selection_page_geometry_fn),
        ("contentPointToPdf", content_point_to_pdf_fn),
        ("toContentTextQuad", to_content_text_quad_fn),
        ("toContentSegment", to_content_segment_fn),
        ("buildPageTextLayout", build_page_text_layout_fn),
        ("textGlyphAt", text_glyph_at_fn),
        ("expandTextRangeToWord", expand_word_fn),
        ("expandTextRangeToLine", expand_line_fn),
        ("textGlyphQuad", text_glyph_quad_fn),
        ("textSegmentsForRange", text_segments_fn),
        ("sliceTextByChars", slice_text_fn),
        ("layoutSweep", layout_sweep_fn),
        ("glyphAtGrid", glyph_at_grid_fn),
        ("pairProbe", pair_probe_fn),
        ("sliceSweep", slice_sweep_fn),
        ("initialSelectionState", initial_state_fn),
        ("reduceSelection", reducer_fn),
    ]
}

// ── plugins/selection-handles.json ────────────────────────────────────────────

/// The generator's `viewOf`: `matrices[pon]` projects, `pages` hit-test.
struct TableView(Value);

impl TableView {
    fn pages(&self) -> Vec<(u32, [f64; 4])> {
        self.0["pages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let r = &p["rect"];
                let n = |k: &str| r[k].as_f64();
                Some((p["pon"].as_u64()? as u32, [n("x")?, n("y")?, n("width")?, n("height")?]))
            })
            .collect()
    }
}

impl SelectionHandleView for TableView {
    fn to_overlay(&self, page: PageRef, p: Point) -> Option<Point> {
        let key = page.page_object_number.to_string();
        let m: [f64; 6] = from_value(self.0["matrices"].get(key)?).ok()?;
        Some(Point { x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5] })
    }
    fn page_at(&self, o: Point) -> Option<PagePoint> {
        self.pages().into_iter().find_map(|(pon, [x, y, w, h])| {
            (o.x >= x && o.x < x + w && o.y >= y && o.y < y + h).then_some(PagePoint {
                page: PageRef::new(pon),
                point: Point { x: o.x - x, y: o.y - y },
            })
        })
    }
    fn point_on_page(&self, page: PageRef, o: Point) -> Option<Point> {
        self.pages()
            .into_iter()
            .find(|(p, _)| *p == page.page_object_number)
            .map(|(_, [x, y, _, _])| Point { x: o.x - x, y: o.y - y })
    }
}

/// The generator's recording target: `begins` answers `beginGestureAt` per call.
struct RecordingTarget {
    begins: Vec<bool>,
    calls: Cell<usize>,
    log: RefCell<Vec<Value>>,
}

impl SelectionHandleTarget for RecordingTarget {
    fn begin_gesture_at(&self, page: PageRef, point: Point) -> bool {
        let i = self.calls.get();
        self.calls.set(i + 1);
        let answer = self.begins[i.min(self.begins.len() - 1)];
        self.log.borrow_mut().push(
            json!({ "beginGestureAt": [page.to_json(), to_value(&point)], "answer": answer }),
        );
        answer
    }
    fn extend_to(&self, page: PageRef, point: Point) {
        self.log.borrow_mut().push(json!({ "extendTo": [page.to_json(), to_value(&point)] }));
    }
    fn end_gesture(&self) {
        self.log.borrow_mut().push(json!({ "endGesture": [] }));
    }
}

fn constant(value: f64) -> Result<Value, Error> {
    out(value)
}

fn head_fn(_: &[Value]) -> Result<Value, Error> {
    constant(HANDLE_HEAD)
}

fn bar_fn(_: &[Value]) -> Result<Value, Error> {
    constant(HANDLE_BAR)
}

fn pad_fn(_: &[Value]) -> Result<Value, Error> {
    constant(HANDLE_PAD)
}

fn handle_geom_fn(args: &[Value]) -> Result<Value, Error> {
    let view = TableView(args.first().cloned().unwrap_or(Value::Null));
    let ep: SelectionHandleEndpoint = arg(args, 1)?;
    out(selection_handle_geom(&view, &ep, arg(args, 2)?))
}

fn handle_drag_fn(args: &[Value]) -> Result<Value, Error> {
    let view = Rc::new(TableView(args.first().cloned().unwrap_or(Value::Null)));
    let opposite: SelectionHandleEndpoint = arg(args, 1)?;
    let target = Rc::new(RecordingTarget {
        begins: arg(args, 3)?,
        calls: Cell::new(0),
        log: RefCell::default(),
    });
    let dragged = PageRef::new(arg(args, 2)?);
    let session = create_selection_handle_drag(target.clone(), view, &opposite, dragged);
    let moves: Vec<Value> = arg(args, 4)?;
    for m in moves {
        if m == "end" {
            session.end();
        } else {
            session.move_to(from_value(&m).map_err(|e| Error::new(e.message))?);
        }
    }
    let log = target.log.borrow().clone();
    Ok(Value::Array(log))
}

pub fn handle_functions() -> FunctionTable {
    vec![
        ("HANDLE_HEAD", head_fn),
        ("HANDLE_BAR", bar_fn),
        ("HANDLE_PAD", pad_fn),
        ("selectionHandleGeom", handle_geom_fn),
        ("handleDrag", handle_drag_fn),
    ]
}
