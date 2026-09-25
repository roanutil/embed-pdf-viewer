//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! tables for `plugins/selection-geometry.json` and
//! `plugins/selection-handles.json`, each entry named as the generator
//! (`tooling/plugin-conformance/src/suites/selection.ts`) names it.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_geometry::Point;
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::js::{from_value, to_value};
use plugin_conformance::{FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::geometry::{
    build_selection_page_geometry, content_point_to_pdf, to_content_segment, to_content_text_quad,
};
use crate::handles::*;
use crate::text_layout::*;
use crate::types::*;
use crate::{SelectionCapability, initial_selection_state, selection_plugin, selection_reducer};

fn arg<T: DeserializeOwned>(args: &[Value], i: usize) -> Result<T, Error> {
    let value = match args.get(i) {
        Some(v) if !is_undefined(v) => v.clone(),
        _ => Value::Null,
    };
    from_value(&value).map_err(|e| Error::new(format!("[harness] argument {i}: {}", e.message)))
}

fn out<T: Serialize>(value: T) -> Result<Value, Error> {
    Ok(to_value(&value))
}

/// A `select` argument: a span when it has a `pon`, else a range.
fn range_input(value: &Value) -> Result<SelectionRangeInput, Error> {
    let bad = |e: Error| Error::new(format!("[harness] select: {}", e.message));
    if value.get("pon").is_some() {
        Ok(SelectionRangeInput::Span {
            pon: from_value(&value["pon"]).map_err(bad)?,
            start: from_value(&value["start"]).map_err(bad)?,
            count: from_value(&value["count"]).map_err(bad)?,
        })
    } else {
        from_value::<TextRange>(value).map(SelectionRangeInput::Range).map_err(bad)
    }
}

// ── the adapter ───────────────────────────────────────────────────────────────

pub struct Selection;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "canSelect",
    "canCopy",
    "select",
    "selectAll",
    "clear",
    "snapshot",
    "menuAnchor",
    "isSelecting",
    "hasSelection",
    "selectedPages",
    "segmentsForPage",
    "rectsForPage",
    "readText",
    "onChange",
    "onCommit",
    "ensurePage",
    "isLoaded",
    "isOverText",
    "beginAt",
    "selectWordAt",
    "selectLineAt",
    "extendTo",
    "end",
    "setHighlightVisible",
    "highlightVisible",
];

fn subscription(h: &Harness, off: epdf_plugin_interaction::Unsubscriber) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| Outcome::Returned(json!(off.call())))))
}

fn call_selection(
    cap: &SelectionCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let pon = || arg::<PageObjectNumber>(args, 0);
    let point = || arg::<Point>(args, 1);
    Ok(match method {
        "canSelect" => Outcome::Returned(json!(cap.can_select())),
        "canCopy" => Outcome::Returned(json!(cap.can_copy())),
        "select" => {
            let input = range_input(args.first().unwrap_or(&Value::Null))?;
            match cap.select(input) {
                Ok(()) => Outcome::undefined(),
                Err(error) => Outcome::Threw(error),
            }
        }
        "selectAll" => match cap.select_all() {
            Ok(()) => Outcome::undefined(),
            Err(error) => Outcome::Threw(error),
        },
        "clear" => {
            cap.clear();
            Outcome::undefined()
        }
        "snapshot" => Outcome::Returned(to_value(&cap.snapshot())),
        "menuAnchor" => Outcome::Returned(to_value(&cap.menu_anchor())),
        "isSelecting" => Outcome::Returned(json!(cap.is_selecting())),
        "hasSelection" => Outcome::Returned(json!(cap.has_selection())),
        "selectedPages" => Outcome::Returned(json!(cap.selected_pages())),
        "segmentsForPage" => Outcome::Returned(to_value(&cap.segments_for_page(pon()?))),
        "rectsForPage" => Outcome::Returned(to_value(&cap.rects_for_page(pon()?))),
        "readText" => Outcome::promise(cap.read_text()),
        "onChange" | "onCommit" => {
            let callback = h.callback(args.first().unwrap_or(&Value::Null));
            let listener = move || {
                callback.call(Vec::new());
            };
            let off = if method == "onChange" {
                cap.on_change(listener)
            } else {
                cap.on_commit(listener)
            };
            subscription(h, off)
        }
        "ensurePage" => {
            cap.ensure_page(pon()?);
            Outcome::undefined()
        }
        "isLoaded" => Outcome::Returned(json!(cap.is_loaded(pon()?))),
        "isOverText" => Outcome::Returned(json!(cap.is_over_text(pon()?, point()?))),
        "beginAt" => Outcome::Returned(json!(cap.begin_at(pon()?, point()?))),
        "selectWordAt" => Outcome::Returned(json!(cap.select_word_at(pon()?, point()?))),
        "selectLineAt" => Outcome::Returned(json!(cap.select_line_at(pon()?, point()?))),
        "extendTo" => {
            cap.extend_to(pon()?, point()?);
            Outcome::undefined()
        }
        "end" => {
            cap.end();
            Outcome::undefined()
        }
        "setHighlightVisible" => {
            cap.set_highlight_visible(arg::<bool>(args, 0)?);
            Outcome::undefined()
        }
        "highlightVisible" => Outcome::Returned(json!(cap.highlight_visible())),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for Selection {
    fn factories(&self) -> Vec<&'static str> {
        vec!["selection"]
    }

    fn build(&self, _factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        Ok(selection_plugin())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![SELECTION.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<SelectionCapability>()?;
        Some(call_selection(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<SelectionCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── plugins/selection-geometry.json ───────────────────────────────────────────

/// The generator's `geomOf`: `{snapshot, crop, rotation, userUnit}`.
fn geom_of(value: &Value) -> Result<crate::SelectionPageGeometry, Error> {
    let field = |name: &str| value.get(name).cloned().unwrap_or(Value::Null);
    build_page_geometry(&[field("snapshot"), field("crop"), field("rotation"), field("userUnit")])
}

fn build_page_geometry(args: &[Value]) -> Result<crate::SelectionPageGeometry, Error> {
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
    out(selection_reducer(&state, &action).unwrap_or(state))
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
        ("initialSelectionState", initial_state_fn),
        ("selectionReducer", reducer_fn),
    ]
}

// ── plugins/selection-handles.json ────────────────────────────────────────────

/// The generator's `viewOf`: `matrices[pon]` projects, `pages` hit-test.
struct TableView(Value);

impl TableView {
    fn pages(&self) -> Vec<(PageObjectNumber, [f64; 4])> {
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
    fn to_overlay(&self, pon: PageObjectNumber, p: Point) -> Option<Point> {
        let m: [f64; 6] = from_value(self.0["matrices"].get(pon.to_string())?).ok()?;
        Some(Point { x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5] })
    }
    fn page_at(&self, o: Point) -> Option<PagePoint> {
        self.pages().into_iter().find_map(|(pon, [x, y, w, h])| {
            (o.x >= x && o.x < x + w && o.y >= y && o.y < y + h)
                .then_some(PagePoint { pon, point: Point { x: o.x - x, y: o.y - y } })
        })
    }
    fn point_on_page(&self, pon: PageObjectNumber, o: Point) -> Option<Point> {
        self.pages()
            .into_iter()
            .find(|(p, _)| *p == pon)
            .map(|(_, [x, y, _, _])| Point { x: o.x - x, y: o.y - y })
    }
}

/// The generator's recording target: `begins` answers `beginAt` per call.
struct RecordingTarget {
    begins: Vec<bool>,
    calls: Cell<usize>,
    log: RefCell<Vec<Value>>,
}

impl SelectionHandleTarget for RecordingTarget {
    fn begin_at(&self, pon: PageObjectNumber, point: Point) -> bool {
        let i = self.calls.get();
        self.calls.set(i + 1);
        let answer = self.begins[i.min(self.begins.len() - 1)];
        self.log.borrow_mut().push(json!({ "beginAt": [pon, to_value(&point)], "answer": answer }));
        answer
    }
    fn extend_to(&self, pon: PageObjectNumber, point: Point) {
        self.log.borrow_mut().push(json!({ "extendTo": [pon, to_value(&point)] }));
    }
    fn end(&self) {
        self.log.borrow_mut().push(json!({ "end": [] }));
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
    let session = create_selection_handle_drag(target.clone(), view, &opposite, arg(args, 2)?);
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
