//! The conformance adapter: the function table the vectors call by the
//! TypeScript names, with arguments and results in canonical JSON. The
//! wrappers mirror the ones in
//! `tooling/plugin-conformance/src/suites/annotation-core-*.ts`: a model
//! argument is a start layered over `initialModel`, and the `*Probe`,
//! `*Calls`, `runs` and `viewMany` entries evaluate many calls on one model.

use std::collections::BTreeMap;

use epdf_kernel::Error;
use plugin_conformance::FunctionTable;
use plugin_conformance::js::{arg, from_value, to_value};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::js::{IdSet, array_index};
use crate::*;

type Out = Result<Value, Error>;

fn ok<T: Serialize>(value: T) -> Out {
    Ok(to_value(&value))
}

/// `undefined` for `None`: what a TypeScript function that may return
/// `undefined` prints.
fn ok_or_undefined<T: Serialize>(value: Option<T>) -> Out {
    Ok(value.map_or_else(epdf_kernel::undefined, |v| to_value(&v)))
}

/// `{ ...initialModel, ...start }` as JSON.
fn merged(start: &Value) -> Map<String, Value> {
    let Value::Object(mut base) = to_value(&initial_model()) else {
        unreachable!("a model is an object")
    };
    if let Value::Object(over) = start {
        for (k, v) in over {
            base.insert(k.clone(), v.clone());
        }
    }
    base
}

fn model_of(start: &Value) -> Result<Model, Error> {
    from_value(&Value::Object(merged(start)))
}

fn model(a: &[Value], i: usize) -> Result<Model, Error> {
    model_of(a.get(i).unwrap_or(&Value::Null))
}

/// A suite wrapper's page argument: the object number it passes to
/// `pageRef(pon)`.
fn page(a: &[Value], i: usize) -> Result<PageRef, Error> {
    Ok(PageRef::new(arg(a, i)?))
}

fn geom(a: &[Value], i: usize) -> Result<Geom, Error> {
    arg(a, i)
}

fn border(a: &[Value], i: usize) -> Result<Option<Border>, Error> {
    arg(a, i)
}

// ── geometry ────────────────────────────────────────────────────────────────

fn geom_hit_probe(a: &[Value]) -> Out {
    let g = geom(a, 0)?;
    let (margin, filled, sw): (f64, bool, f64) = (arg(a, 1)?, arg(a, 2)?, arg(a, 3)?);
    let points: Vec<Point> = arg(a, 4)?;
    ok(points.iter().map(|p| geom_hit(&g, *p, margin, filled, sw)).collect::<Vec<_>>())
}

// ── model helpers ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HitCall {
    selected: Vec<Id>,
    pon: PageObjectNumber,
    p: Point,
    gm: ChromeGeom,
    margin: f64,
    #[serde(default, rename = "pageBox")]
    page_box: Option<Rect>,
    #[serde(default)]
    inert: Option<IdSet>,
    #[serde(default)]
    view: Option<ViewEnv>,
}

fn hit_calls(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let calls: Vec<HitCall> = arg(a, 1)?;
    let out: Vec<Value> = calls
        .into_iter()
        .map(|c| {
            let m = Model { selected: c.selected, ..m.clone() };
            let pg = PageRef::new(c.pon);
            let t = hit_test(&m, pg, c.p, &c.gm, c.margin, c.page_box, c.inert.as_ref(), c.view);
            let cur = cursor_at(&m, pg, c.p, &c.gm, c.margin, c.page_box, c.inert.as_ref(), c.view);
            Value::Array(vec![to_value(&t), to_value(&cur)])
        })
        .collect();
    Ok(Value::Array(out))
}

fn hit_probe(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let pon = page(a, 1)?;
    let selections: Vec<Vec<Id>> = arg(a, 2)?;
    let points: Vec<Point> = arg(a, 3)?;
    let gm: ChromeGeom = arg(a, 4)?;
    let margin: f64 = arg(a, 5)?;
    let out: Vec<Value> = selections
        .into_iter()
        .map(|selected| {
            let m = Model { selected, ..m.clone() };
            Value::Array(
                points
                    .iter()
                    .map(|p| {
                        let t = hit_test(&m, pon, *p, &gm, margin, None, None, None);
                        let c = cursor_at(&m, pon, *p, &gm, margin, None, None, None);
                        Value::Array(vec![to_value(&t), to_value(&c)])
                    })
                    .collect(),
            )
        })
        .collect();
    Ok(Value::Array(out))
}

fn hit_test_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let inert: Option<IdSet> = arg(a, 6)?;
    ok(hit_test(
        &m,
        page(a, 1)?,
        arg(a, 2)?,
        &arg(a, 3)?,
        arg(a, 4)?,
        arg(a, 5)?,
        inert.as_ref(),
        arg(a, 7)?,
    ))
}

fn cursor_at_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let inert: Option<IdSet> = arg(a, 6)?;
    ok(cursor_at(
        &m,
        page(a, 1)?,
        arg(a, 2)?,
        &arg(a, 3)?,
        arg(a, 4)?,
        arg(a, 5)?,
        inert.as_ref(),
        arg(a, 7)?,
    ))
}

fn read_props(a: &[Value]) -> Out {
    let annot: Annot = arg(a, 0)?;
    let keys: Vec<PropKey> = arg(a, 1)?;
    let out: Vec<Value> = keys
        .into_iter()
        .map(|k| {
            let v = read_prop(&annot, k).map_or_else(epdf_kernel::undefined, |v| to_value(&v));
            Value::Array(vec![to_value(&k), v])
        })
        .collect();
    Ok(Value::Array(out))
}

fn group_probe(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let ids: Vec<Id> = arg(a, 1)?;
    let out: Vec<Value> = ids
        .iter()
        .map(|id| {
            let children: Vec<&Id> = link_children_of(&m, id).into_iter().map(|c| &c.id).collect();
            Value::Array(vec![
                to_value(&group_key_of(&m, id)),
                to_value(&group_members(&m, id)),
                to_value(&children),
                to_value(&link_of(&m, id)),
            ])
        })
        .collect();
    Ok(Value::Array(out))
}

fn selectability(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let ids: Vec<Id> = arg(a, 1)?;
    ok(ids.iter().map(|id| [is_selectable(&m, id), can_move(&m, id)]).collect::<Vec<_>>())
}

// ── view ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewAll {
    page_items: Vec<RenderItem>,
    chrome: Vec<ChromeNode>,
    selected_items: Vec<RenderItem>,
    text_boxes: Vec<TextBox>,
    selection_bounds_on_page: Option<Rect>,
    selection_knob: Option<Knob>,
    creation_draft_anchor: Option<CreationDraftAnchor>,
}

fn view_all_of(
    m: &Model,
    pon: PageObjectNumber,
    page_box: Option<Rect>,
    knob_offset: Option<f64>,
    view: Option<ViewEnv>,
) -> ViewAll {
    let pon = PageRef::new(pon);
    ViewAll {
        page_items: page_items(m, pon, view),
        chrome: chrome(m, pon, page_box, knob_offset, view),
        selected_items: selected_items(m, view),
        text_boxes: text_boxes(m, pon, view),
        selection_bounds_on_page: selection_bounds_on_page(m, pon, view),
        selection_knob: selection_knob(m, pon, page_box, knob_offset, view),
        creation_draft_anchor: creation_draft_anchor(m),
    }
}

/// `{ ...page, ...over }`.
fn layered(page: &Value, over: &Value) -> Value {
    let mut out = match page {
        Value::Object(p) => p.clone(),
        _ => Map::new(),
    };
    if let Value::Object(o) = over {
        for (k, v) in o {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

#[derive(Deserialize)]
struct ViewCall {
    name: String,
    over: Value,
    pon: PageObjectNumber,
    #[serde(default, rename = "pageBox")]
    page_box: Option<Rect>,
    #[serde(default, rename = "knobOffset")]
    knob_offset: Option<f64>,
    #[serde(default)]
    view: Option<ViewEnv>,
}

fn with_name(name: &str, value: Value) -> Value {
    let mut out = Map::new();
    out.insert("name".into(), Value::String(name.into()));
    if let Value::Object(v) = value {
        out.extend(v);
    }
    Value::Object(out)
}

fn view_many(a: &[Value]) -> Out {
    let page = a.first().cloned().unwrap_or(Value::Null);
    let calls: Vec<ViewCall> = arg(a, 1)?;
    let mut out = Vec::new();
    for c in calls {
        let m = model_of(&layered(&page, &c.over))?;
        out.push(with_name(
            &c.name,
            to_value(&view_all_of(&m, c.pon, c.page_box, c.knob_offset, c.view)),
        ));
    }
    Ok(Value::Array(out))
}

type Table<T> = BTreeMap<String, T>;

fn anchor_with(
    m: &Model,
    boxes: Option<&Table<Rect>>,
    offsets: Option<&Table<f64>>,
    views: Option<&Table<ViewEnv>>,
) -> Option<SelectionAnchor> {
    let key = |page: PageRef| page.pon().to_string();
    let box_of = |page: PageRef| boxes.and_then(|t| t.get(&key(page)).copied());
    let offset_of = |page: PageRef| offsets.and_then(|t| t.get(&key(page)).copied());
    let view_of = |page: PageRef| views.and_then(|t| t.get(&key(page)).copied());
    selection_anchor(
        m,
        boxes.map(|_| &box_of as &dyn Fn(PageRef) -> Option<Rect>),
        offsets.map(|_| &offset_of as &dyn Fn(PageRef) -> Option<f64>),
        views.map(|_| &view_of as &dyn Fn(PageRef) -> Option<ViewEnv>),
    )
}

fn selection_anchor_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let boxes: Option<Table<Rect>> = arg(a, 1)?;
    let offsets: Option<Table<f64>> = arg(a, 2)?;
    let views: Option<Table<ViewEnv>> = arg(a, 3)?;
    ok(anchor_with(&m, boxes.as_ref(), offsets.as_ref(), views.as_ref()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnchorCall {
    name: String,
    over: Value,
    #[serde(default)]
    page_boxes: Option<Table<Rect>>,
    #[serde(default)]
    knob_offsets: Option<Table<f64>>,
    #[serde(default)]
    views: Option<Table<ViewEnv>>,
}

fn selection_anchors(a: &[Value]) -> Out {
    let page = a.first().cloned().unwrap_or(Value::Null);
    let calls: Vec<AnchorCall> = arg(a, 1)?;
    let mut out = Vec::new();
    for c in calls {
        let m = model_of(&layered(&page, &c.over))?;
        let anchor =
            anchor_with(&m, c.page_boxes.as_ref(), c.knob_offsets.as_ref(), c.views.as_ref());
        let mut entry = Map::new();
        entry.insert("name".into(), Value::String(c.name));
        entry.insert("anchor".into(), to_value(&anchor));
        out.push(Value::Object(entry));
    }
    Ok(Value::Array(out))
}

// ── update ──────────────────────────────────────────────────────────────────

/// `Object.keys(byId)`: integer-like keys first, then insertion order.
fn js_keys(by_id: Option<&Value>) -> Vec<String> {
    let Some(Value::Object(map)) = by_id else { return Vec::new() };
    let mut indexed: Vec<(u32, String)> =
        map.keys().filter_map(|k| array_index(k).map(|i| (i, k.clone()))).collect();
    indexed.sort_by_key(|(i, _)| *i);
    let mut keys: Vec<String> = indexed.into_iter().map(|(_, k)| k).collect();
    keys.extend(map.keys().filter(|k| array_index(k).is_none()).cloned());
    keys
}

fn run_one(start: &Value, msgs: &[Msg]) -> Result<Value, Error> {
    let start_json = merged(start);
    let first = model_of(start)?;
    let mut m = first.clone();
    let mut steps = Vec::new();
    for msg in msgs {
        let (next, effects) = update(&m, msg);
        let same = next.is_none();
        if let Some(next) = next {
            m = next;
        }
        let mut step = Map::new();
        step.insert("same".into(), Value::Bool(same));
        step.insert("effects".into(), to_value(&effects));
        step.insert("draft".into(), to_value(&m.draft));
        step.insert("selected".into(), to_value(&m.selected));
        steps.push(Value::Object(step));
    }
    let changed: BTreeMap<&Id, &Annot> = m
        .by_id
        .iter()
        .filter(|(id, a)| first.by_id.get(*id).is_none_or(|b| to_value(*a) != to_value(b)))
        .collect();
    let removed: Vec<String> = js_keys(start_json.get("byId"))
        .into_iter()
        .filter(|id| !m.by_id.contains_key(id))
        .collect();
    let Value::Object(mut model) = to_value(&m) else { unreachable!("a model is an object") };
    model.insert("byId".into(), to_value(&changed));
    let mut out = Map::new();
    out.insert("steps".into(), Value::Array(steps));
    out.insert("model".into(), Value::Object(model));
    out.insert("removed".into(), to_value(&removed));
    Ok(Value::Object(out))
}

fn run(a: &[Value]) -> Out {
    let msgs: Vec<Msg> = arg(a, 1)?;
    run_one(a.first().unwrap_or(&Value::Null), &msgs)
}

#[derive(Deserialize)]
struct RunCall {
    name: String,
    over: Value,
    msgs: Vec<Msg>,
}

fn runs(a: &[Value]) -> Out {
    let page = a.first().cloned().unwrap_or(Value::Null);
    let calls: Vec<RunCall> = arg(a, 1)?;
    let mut out = Vec::new();
    for c in calls {
        out.push(with_name(&c.name, run_one(&layered(&page, &c.over), &c.msgs)?));
    }
    Ok(Value::Array(out))
}

/// A rotate draft as the vectors spell it.
#[derive(Deserialize)]
struct RotateDraftArg {
    ids: Vec<Id>,
    pivot: Point,
    start: Point,
    cur: Point,
    #[serde(default)]
    free: Option<bool>,
    #[serde(default)]
    view: Option<ViewEnv>,
}

impl RotateDraftArg {
    fn draft(&self) -> RotateDraft<'_> {
        RotateDraft {
            ids: &self.ids,
            pivot: self.pivot,
            start: self.start,
            cur: self.cur,
            free: self.free,
            view: self.view,
        }
    }
}

fn rotate_draft_delta_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let d: RotateDraftArg = arg(a, 1)?;
    ok(rotate_draft_delta(&m, &d.draft()))
}

fn rotate_draft_deltas(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let ds: Vec<RotateDraftArg> = arg(a, 1)?;
    ok(ds.iter().map(|d| rotate_draft_delta(&m, &d.draft())).collect::<Vec<_>>())
}

fn annots_in_boxes(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let calls: Vec<Vec<Value>> = arg(a, 1)?;
    let mut out = Vec::new();
    for c in calls {
        let inert: Option<IdSet> = arg(&c, 3)?;
        out.push(to_value(&annots_in_box(
            &m,
            page(&c, 0)?,
            arg(&c, 1)?,
            arg(&c, 2)?,
            inert.as_ref(),
            arg(&c, 4)?,
        )));
    }
    Ok(Value::Array(out))
}

fn annots_in_box_fn(a: &[Value]) -> Out {
    let m = model(a, 0)?;
    let inert: Option<IdSet> = arg(a, 4)?;
    ok(annots_in_box(&m, page(a, 1)?, arg(a, 2)?, arg(a, 3)?, inert.as_ref(), arg(a, 5)?))
}

// ── the table ───────────────────────────────────────────────────────────────

pub fn functions() -> FunctionTable {
    vec![
        // geometry basics
        ("RECT_HANDLES", |_| ok(RECT_HANDLES)),
        ("ROTATE_KNOB_OFFSET", |_| ok(ROTATE_KNOB_OFFSET)),
        ("DEFAULT_CHROME_GEOM", |_| ok(DEFAULT_CHROME_GEOM)),
        ("MITER_LIMIT", |_| ok(MITER_LIMIT)),
        ("rotatedHandleCursor", |a| {
            ok_or_undefined(rotated_handle_cursor(&arg::<String>(a, 0)?, arg(a, 1)?))
        }),
        ("rectFromPoints", |a| ok(rect_from_points(arg(a, 0)?, arg(a, 1)?))),
        ("rectsIntersect", |a| ok(rects_intersect(arg(a, 0)?, arg(a, 1)?))),
        ("unionRect", |a| ok(union_rect(&arg::<Vec<Point>>(a, 0)?))),
        ("normalizeDeg", |a| ok(normalize_deg(arg(a, 0)?))),
        ("rotatePoint", |a| ok(rotate_point(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("apSizeChanged", |a| ok(ap_size_changed(&geom(a, 0)?, &geom(a, 1)?))),
        ("rotatedAabb", |a| ok(rotated_aabb(arg(a, 0)?, arg(a, 1)?))),
        ("uprightRotation", |a| ok(upright_rotation(arg(a, 0)?))),
        ("transposedAboutCenter", |a| ok(transposed_about_center(arg(a, 0)?))),
        ("uprightAnchoredRect", |a| {
            ok(upright_anchored_rect(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("fitStampBox", |a| ok(fit_stamp_box(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))),
        ("rectHandlesFor", |a| ok(rect_handles_for(arg(a, 0)?))),
        ("groupResizeAnchor", |a| ok(group_resize_anchor(arg(a, 0)?, &arg::<String>(a, 1)?))),
        ("groupResizeBox", |a| {
            ok(group_resize_box(arg(a, 0)?, &arg::<String>(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("groupResizeFactors", |a| ok(group_resize_factors(arg(a, 0)?, arg(a, 1)?))),
        ("rotateKnob", |a| ok(rotate_knob(&arg(a, 0)?, arg(a, 1)?))),
        ("chordThrough", |a| ok(chord_through(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("placeRotateKnob", |a| ok(place_rotate_knob(&arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("shapeRectFor", |a| ok(shape_rect_for(arg(a, 0)?, arg(a, 1)?, &arg(a, 2)?))),
        ("pointInQuad", |a| ok(point_in_quad(arg(a, 0)?, &arg(a, 1)?))),
        ("quadIntersectsRect", |a| ok(quad_intersects_rect(&arg(a, 0)?, arg(a, 1)?))),
        // per-geom
        ("geomBounds", |a| ok(geom_bounds(&geom(a, 0)?))),
        ("geomVisualBounds", |a| {
            ok(geom_visual_bounds(&geom(a, 0)?, arg(a, 1)?, border(a, 2)?.as_ref()))
        }),
        ("selectionBounds", |a| {
            ok(selection_bounds(&geom(a, 0)?, arg(a, 1)?, border(a, 2)?.as_ref()))
        }),
        ("selectionQuad", |a| ok(selection_quad(&geom(a, 0)?, arg(a, 1)?, border(a, 2)?.as_ref()))),
        ("selectionCenter", |a| ok(selection_center(&geom(a, 0)?, arg(a, 1)?))),
        ("obbFromGeom", |a| ok(obb_from_geom(&geom(a, 0)?, arg(a, 1)?, border(a, 2)?.as_ref()))),
        ("centroidOf", |a| ok(centroid_of(&geom(a, 0)?))),
        ("geomRotation", |a| ok(geom_rotation(&geom(a, 0)?))),
        ("isRotatableGeom", |a| ok(is_rotatable_geom(&geom(a, 0)?))),
        ("geomHandles", |a| ok(geom_handles(&geom(a, 0)?))),
        ("geomScene", |a| {
            let sw: Option<f64> = arg(a, 1)?;
            ok(geom_scene(&geom(a, 0)?, sw.unwrap_or(0.0), border(a, 2)?.as_ref()))
        }),
        ("geomTranslate", |a| ok(geom_translate(&geom(a, 0)?, arg(a, 1)?))),
        ("geomRotateAbout", |a| ok(geom_rotate_about(&geom(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("geomResetRotation", |a| ok(geom_reset_rotation(&geom(a, 0)?, arg(a, 1)?))),
        ("geomScaleAbout", |a| {
            ok(geom_scale_about(&geom(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("geomDragHandle", |a| {
            ok(geom_drag_handle(&geom(a, 0)?, &arg::<String>(a, 1)?, arg(a, 2)?))
        }),
        ("calloutLinePoints", |a| ok(callout_line_points(&geom(a, 0)?))),
        ("geomHitProbe", geom_hit_probe),
        // text anchors and the PDF bridge
        ("calloutConnection", |a| {
            let rot: Option<f64> = arg(a, 2)?;
            ok(callout_connection(arg(a, 0)?, arg(a, 1)?, rot.unwrap_or(0.0)))
        }),
        ("caretRectFromTextEnd", |a| ok(caret_rect_from_text_end(arg(a, 0)?))),
        ("caretRectFromAnchor", |a| ok(caret_rect_from_anchor(&arg(a, 0)?))),
        ("caretGeomFromAnchor", |a| ok(caret_geom_from_anchor(&arg(a, 0)?))),
        ("pdfToContentPoint", |a| ok(pdf_to_content_point(arg(a, 0)?, arg(a, 1)?))),
        ("contentToPdfPoint", |a| ok(content_to_pdf_point(arg(a, 0)?, arg(a, 1)?))),
        ("pdfToContentRect", |a| ok(pdf_to_content_rect(arg(a, 0)?, arg(a, 1)?))),
        ("contentToPdfRect", |a| ok(content_to_pdf_rect(arg(a, 0)?, arg(a, 1)?))),
        ("geomPdfBounds", |a| {
            ok(geom_pdf_bounds(&geom(a, 0)?, arg(a, 1)?, arg(a, 2)?, border(a, 3)?.as_ref()))
        }),
        // endings and cloudy borders
        ("endingPoints", |a| {
            ok(endings::ending_points(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("endingNodes", |a| {
            ok(endings::ending_nodes(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("cloudyBorderExtent", |a| ok(cloudy_border_extent(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("cloudyPath", |a| ok(cloudy_path(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))),
        ("cloudyPolyPath", |a| {
            ok(cloudy_poly_path(&arg::<Vec<Point>>(a, 0)?, arg(a, 1)?, arg(a, 2)?))
        }),
        // kinds
        ("KINDS", |_| ok(kinds::KindTable)),
        ("capsFor", |a| ok(caps_for(&arg::<String>(a, 0)?))),
        ("propsFor", |a| ok(props_for(&arg::<String>(a, 0)?))),
        ("isMarkup", |a| ok(is_markup(&arg::<String>(a, 0)?))),
        ("kindTakesLink", |a| ok(kind_takes_link(&arg::<String>(a, 0)?))),
        ("sharedProps", |a| ok(shared_props(&arg::<Vec<String>>(a, 0)?))),
        // flags and planes
        ("NO_ANNOTATION_FLAGS", |_| ok(NO_ANNOTATION_FLAGS)),
        ("DRAWN_FLAGS", |_| ok(DRAWN_FLAGS)),
        ("FLAG_KEYS", |_| ok(FLAG_KEYS)),
        ("flagsEqual", |a| ok(flags_equal(&arg(a, 0)?, &arg(a, 1)?))),
        ("mergeFlags", |a| ok(merge_flags(&arg(a, 0)?, &arg(a, 1)?))),
        ("viewable", |a| {
            let engaged: Option<bool> = arg(a, 1)?;
            ok(viewable(&arg(a, 0)?, engaged.unwrap_or(false)))
        }),
        ("interactive", |a| ok(interactive(&arg(a, 0)?))),
        ("annotInteractive", |a| ok(annot_interactive(&arg::<FlagFields>(a, 0)?))),
        ("annotTransformable", |a| ok(annot_transformable(&arg::<FlagFields>(a, 0)?))),
        ("annotDeletable", |a| ok(annot_deletable(&arg::<FlagFields>(a, 0)?))),
        ("annotContentsEditable", |a| ok(annot_contents_editable(&arg::<FlagFields>(a, 0)?))),
        ("isConversationOnly", |a| ok(is_conversation_only(&arg::<plane::PlaneParts>(a, 0)?))),
        ("isAttachedLink", |a| ok(is_attached_link(&arg::<plane::PlaneParts>(a, 0)?))),
        ("isSubstrateOnly", |a| ok(is_substrate_only(&arg::<plane::PlaneParts>(a, 0)?))),
        // groups and links
        ("groupCaps", |a| ok(group_caps(&model(a, 0)?, &arg::<Vec<Id>>(a, 1)?))),
        ("groupKeyOf", |a| ok(group_key_of(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("groupMembers", |a| ok(group_members(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("expandGroups", |a| ok(expand_groups(&model(a, 0)?, &arg::<Vec<Id>>(a, 1)?))),
        ("linkChildrenOf", |a| {
            let m = model(a, 0)?;
            ok(link_children_of(&m, &arg::<String>(a, 1)?))
        }),
        ("linkOf", |a| ok(link_of(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("groupProbe", group_probe),
        // props
        ("initialTextStyle", |_| ok(initial_text_style())),
        ("styleFromProps", |a| ok(style_from_props(&arg(a, 0)?))),
        ("textStyleFromProps", |a| ok(text_style_from_props(&arg(a, 0)?))),
        ("readProp", |a| ok_or_undefined(read_prop(&arg(a, 0)?, arg(a, 1)?))),
        ("readProps", read_props),
        ("applyProps", |a| ok(apply_props(&arg(a, 0)?, &arg(a, 1)?))),
        ("defaultsFor", |a| ok(defaults_for(&model(a, 0)?, &arg::<String>(a, 1)?))),
        // anchors
        ("anchorModeOf", |a| ok(anchor_mode_of(&arg::<FlagFields>(a, 0)?))),
        ("anchorOf", |a| ok(anchor_of(&geom(a, 0)?))),
        ("anchoredGeom", |a| ok(anchored_geom(&geom(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("unanchoredGeom", |a| ok(unanchored_geom(&geom(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("anchoredStrokeWidth", |a| ok(anchored_stroke_width(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("anchoredBox", |a| ok(anchored_box(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))),
        // snapping
        ("computeMoveSnap", |a| {
            ok(compute_move_snap(
                &model(a, 0)?,
                &arg::<Vec<Id>>(a, 1)?,
                page(a, 2)?,
                arg(a, 3)?,
                arg(a, 4)?,
                arg(a, 5)?,
            ))
        }),
        // hit testing
        ("paintOrder", |a| ok(paint_order(&model(a, 0)?, page(a, 1)?))),
        ("isSelectable", |a| ok(is_selectable(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("canMove", |a| ok(can_move(&model(a, 0)?, &arg::<String>(a, 1)?))),
        ("selectability", selectability),
        ("groupUnionBounds", |a| ok(group_union_bounds(&model(a, 0)?, page(a, 1)?, arg(a, 2)?))),
        ("hitTest", hit_test_fn),
        ("cursorAt", cursor_at_fn),
        ("hitCalls", hit_calls),
        ("hitProbe", hit_probe),
        // scene
        ("scene", |a| ok(scene(&arg(a, 0)?))),
        ("blendFor", |a| ok_or_undefined(blend_for(&arg(a, 0)?))),
        ("layoutRedactLabel", |a| {
            let text: Option<TextStyle> = arg(a, 2)?;
            ok(layout_redact_label(arg(a, 0)?, &arg(a, 1)?, text.as_ref()))
        }),
        // placement and ink
        ("clampRectToBox", |a| ok(clamp_rect_to_box(arg(a, 0)?, arg(a, 1)?))),
        ("resolveClickPlacement", |a| {
            let opts: Option<PlacementOptions> = arg(a, 2)?;
            ok(resolve_click_placement(arg(a, 0)?, &arg(a, 1)?, &opts.unwrap_or_default()))
        }),
        ("clickCreateGeom", |a| {
            ok(click_create_geom(&arg::<String>(a, 0)?, &arg(a, 1)?, &arg(a, 2)?))
        }),
        ("straightenInkStroke", |a| {
            ok(straighten_ink_stroke(&arg::<Vec<Point>>(a, 0)?, &arg(a, 1)?))
        }),
        // view
        ("pageItems", |a| ok(page_items(&model(a, 0)?, page(a, 1)?, arg(a, 2)?))),
        ("chrome", |a| ok(chrome(&model(a, 0)?, page(a, 1)?, arg(a, 2)?, arg(a, 3)?, arg(a, 4)?))),
        ("selectedItems", |a| ok(selected_items(&model(a, 0)?, arg(a, 1)?))),
        ("textBoxes", |a| ok(text_boxes(&model(a, 0)?, page(a, 1)?, arg(a, 2)?))),
        ("selectionBoundsOnPage", |a| {
            ok(selection_bounds_on_page(&model(a, 0)?, page(a, 1)?, arg(a, 2)?))
        }),
        ("selectionKnob", |a| {
            ok(selection_knob(&model(a, 0)?, page(a, 1)?, arg(a, 2)?, arg(a, 3)?, arg(a, 4)?))
        }),
        ("creationDraftAnchor", |a| ok(creation_draft_anchor(&model(a, 0)?))),
        ("selectionAnchor", selection_anchor_fn),
        ("selectionAnchors", selection_anchors),
        ("viewAll", |a| {
            ok(view_all_of(&model(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?, arg(a, 4)?))
        }),
        ("viewMany", view_many),
        // update
        ("MIN_DRAG", |_| ok(MIN_DRAG)),
        ("initialStyle", |_| ok(initial_style())),
        ("initialModel", |_| ok(initial_model())),
        ("run", run),
        ("runs", runs),
        ("rotateDraftDelta", rotate_draft_delta_fn),
        ("rotateDraftDeltas", rotate_draft_deltas),
        ("calloutBox", |a| ok(callout_box(&arg(a, 0)?))),
        ("calloutUprightRot", |a| ok(callout_upright_rot(&arg(a, 0)?))),
        ("annotsInBox", annots_in_box_fn),
        ("annotsInBoxes", annots_in_boxes),
    ]
}
