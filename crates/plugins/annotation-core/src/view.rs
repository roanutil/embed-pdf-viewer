//! `view.ts`: pure view selectors. [`page_items`] is the per-annotation
//! render list with the live gesture applied; [`chrome`] is the selection
//! overlay.

use serde::{Deserialize, Serialize};

use crate::anchor::{
    anchor_mode_of, anchor_of, anchored_box, anchored_geom, anchored_stroke_width,
};
use crate::flags::{annot_transformable, viewable};
use crate::geometry::{
    Knob, ROTATE_KNOB_OFFSET, chord_through, geom_handles, geom_rotate_about, geom_rotation,
    geom_scale_about, geom_translate, geom_visual_bounds, group_resize_factors, normalize_deg,
    obb_from_geom, place_rotate_knob, rect_from_points, rect_handles_for, rotate_point,
    selection_bounds, selection_quad, shape_rect_for, union_rect,
};
use crate::group::group_caps;
use crate::hit::{is_selectable, paint_order};
use crate::js;
use crate::kinds::caps_for;
use crate::props::style_from_props;
use crate::scene::blend_for;
use crate::types::{
    Annot, CalloutStep, ChromeNode, CreationDraftAnchor, CreationDraftKind, Draft, Geom, GuideLine,
    GuideRole, Id, LineEnding, LineEndings, Model, PageObjectNumber, Point, PolySubtype, Rect,
    RenderItem, RenderSource, Source, Style, ViewEnv,
};
use crate::update::{
    CalloutDraft, callout_box, callout_upright_rot, defaults_for, rotate_draft_delta,
};

const DRAFT_ID: &str = "__draft__";
const PREVIEW_ID: &str = "__markup_preview__";

fn poly_preview_points(points: &[Point], cur: Point) -> Vec<Point> {
    let mut out = points.to_vec();
    if let Some(last) = points.last()
        && (cur.x != last.x || cur.y != last.y)
    {
        out.push(cur);
    }
    out
}

/// The gesture-effective geometry in view space: project first, then apply
/// the live gesture.
fn eff_geom(m: &Model, id: &str, view: Option<ViewEnv>) -> Geom {
    let a = &m.by_id[id];
    let g = anchored_geom(&a.geom, anchor_mode_of(a), view);
    let Some(d) = &m.draft else {
        return g;
    };
    let has = |ids: &[Id]| ids.iter().any(|x| x == id);
    match d {
        Draft::Move { ids, delta, .. } if has(ids) => geom_translate(&g, *delta),
        Draft::Handle { id: hid, cur, .. } if hid == id => cur.clone(),
        Draft::Rotate { ids, pivot, .. } if has(ids) => {
            let rd = d.as_rotate().expect("a rotate draft");
            geom_rotate_about(&g, *pivot, rotate_draft_delta(m, &rd).delta)
        }
        Draft::Group { ids, anchor, base, cur, .. } if has(ids) => {
            let f = group_resize_factors(*base, *cur);
            geom_scale_about(&g, *anchor, f.sx, f.sy)
        }
        _ => g,
    }
}

fn eff_style(a: &Annot, view: Option<ViewEnv>) -> Style {
    let mode = anchor_mode_of(a);
    match (mode, view) {
        (Some(mode), Some(_)) if mode.zoom => Style {
            stroke_width: anchored_stroke_width(a.style.stroke_width, Some(mode), view),
            ..a.style.clone()
        },
        _ => a.style.clone(),
    }
}

/// `x || undefined` for a number.
fn truthy(x: f64) -> Option<f64> {
    (x != 0.0 && !x.is_nan()).then_some(x)
}

fn eff_ap(m: &Model, id: &str, view: Option<ViewEnv>) -> (Option<Rect>, Option<f64>) {
    let a = &m.by_id[id];
    if caps_for(&a.subtype).opaque_body {
        let g = eff_geom(m, id, view);
        return (g.rect().copied().or(a.ap_box), truthy(geom_rotation(&g)));
    }
    let Some(ap_box) = a.ap_box else {
        return (None, a.ap_rot);
    };
    let projected = anchored_box(ap_box, anchor_of(&a.geom), anchor_mode_of(a), view);
    let mut bx = projected.map_or(ap_box, |p| p.box_);
    let rot = truthy(normalize_deg(a.ap_rot.unwrap_or(0.0) + projected.map_or(0.0, |p| p.rot)));
    if let Some(Draft::Move { ids, delta, .. }) = &m.draft
        && ids.iter().any(|x| x == id)
    {
        bx = Rect { x: bx.x + delta.x, y: bx.y + delta.y, ..bx };
    }
    (Some(bx), rot)
}

fn eff_source(m: &Model, id: &str) -> Source {
    let a = &m.by_id[id];
    if caps_for(&a.subtype).opaque_body {
        return a.source;
    }
    if m.editing.as_deref() == Some(id) && a.geom.is_callout() {
        return Source::Vector;
    }
    match &m.draft {
        Some(Draft::Handle { id: hid, .. }) if hid == id => Source::Vector,
        Some(Draft::Rotate { ids, .. } | Draft::Group { ids, .. })
            if ids.iter().any(|x| x == id) =>
        {
            Source::Vector
        }
        _ => a.source,
    }
}

fn text_is_live(m: &Model, id: &str) -> bool {
    m.editing.as_deref() == Some(id) || eff_source(m, id) == Source::Vector
}

fn ghost(subtype: &str, geom: Geom, style: Style) -> RenderItem {
    RenderItem {
        id: DRAFT_ID.into(),
        ref_: None,
        subtype: subtype.into(),
        box_: geom_visual_bounds(&geom, style.stroke_width, Some(&style.border)),
        geom,
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
    }
}

/// The render list for a page: annotations in paint order (live free text
/// excepted), then the creation ghost, then the markup preview.
pub fn page_items(m: &Model, pon: PageObjectNumber, view: Option<ViewEnv>) -> Vec<RenderItem> {
    let mut items = Vec::new();
    for id in paint_order(m, pon) {
        let a = &m.by_id[&id];
        if matches!(a.geom, Geom::Text { callout: None, .. }) && text_is_live(m, &id) {
            continue;
        }
        let geom = eff_geom(m, &id, view);
        let style = eff_style(a, view);
        let (ap_box, ap_rot) = eff_ap(m, &id, view);
        items.push(RenderItem {
            ref_: a.ref_.clone(),
            subtype: a.subtype.clone(),
            box_: geom_visual_bounds(&geom, style.stroke_width, Some(&style.border)),
            ap_box,
            text: a.text.clone(),
            label: a.label.clone(),
            source: eff_source(m, &id).into(),
            selected: m.selected.contains(&id),
            hovered: (m.hovered.as_deref() == Some(id.as_str())).then_some(true),
            rot: Some(geom_rotation(&geom)),
            ap_rot: ap_rot.and_then(truthy),
            blend: blend_for(&a.style),
            geom,
            style,
            id,
        });
    }
    match &m.draft {
        Some(
            d @ (Draft::CreateRect { pon: p, .. }
            | Draft::CreateLine { pon: p, .. }
            | Draft::CreatePoly { pon: p, .. }
            | Draft::CreateInk { pon: p, .. }),
        ) if *p == pon => {
            let (subtype, preset) = match d {
                Draft::CreateRect { subtype, preset, .. }
                | Draft::CreateLine { subtype, preset, .. }
                | Draft::CreatePoly { subtype, preset, .. }
                | Draft::CreateInk { subtype, preset, .. } => (subtype, preset),
                _ => unreachable!(),
            };
            let def = defaults_for(m, preset.as_deref().unwrap_or(subtype));
            let style = style_from_props(&def);
            let geom = match d {
                Draft::CreateRect { from, to, ellipse, .. } => {
                    let dragged = rect_from_points(*from, *to);
                    (dragged.width > 0.0 || dragged.height > 0.0).then(|| Geom::Rect {
                        rect: shape_rect_for(dragged, *ellipse, &style),
                        ellipse: *ellipse,
                        rot: None,
                    })
                }
                Draft::CreateLine { from, to, .. } => {
                    Some(Geom::Line { a: *from, b: *to, ends: Some(def.line_endings), rot: None })
                }
                Draft::CreatePoly { points, cur, closed, .. } => Some(Geom::Poly {
                    points: poly_preview_points(points, *cur),
                    closed: *closed,
                    ends: if *closed { None } else { Some(def.line_endings) },
                    rot: None,
                }),
                Draft::CreateInk { strokes, .. } => {
                    Some(Geom::Ink { strokes: strokes.clone(), rot: None })
                }
                _ => None,
            };
            if let Some(geom) = geom {
                items.push(ghost(subtype, geom, style));
            }
        }
        Some(d @ Draft::CreateCallout { pon: p, subtype, preset, step, tip, knee, cur, .. })
            if *p == pon =>
        {
            let def = defaults_for(m, preset.as_deref().unwrap_or(subtype));
            let style = style_from_props(&def);
            let ending = if def.line_endings.end != LineEnding::None {
                def.line_endings.end
            } else {
                LineEnding::OpenArrow
            };
            let cd = CalloutDraft::of(d).expect("a callout draft");
            let rot = callout_upright_rot(&cd);
            let geom = if *step == CalloutStep::Knee {
                Geom::Line {
                    a: *tip,
                    b: *cur,
                    ends: Some(LineEndings { start: ending, end: LineEnding::None }),
                    rot: None,
                }
            } else {
                Geom::Text {
                    rect: callout_box(&cd),
                    callout: Some(crate::types::Callout { tip: *tip, knee: *knee, ending }),
                    rot: truthy(rot),
                }
            };
            items.push(ghost(subtype, geom, style));
        }
        _ => {}
    }
    if let Some(preview) = &m.preview
        && let Some(quads) = preview.by_page.get(&js::number_to_string(f64::from(pon)))
        && !quads.is_empty()
    {
        let geom = Geom::Quads { quads: quads.clone() };
        let style = style_from_props(&defaults_for(m, &preview.preset));
        items.push(RenderItem {
            id: PREVIEW_ID.into(),
            subtype: preview.subtype.clone(),
            box_: geom_visual_bounds(&geom, 0.0, None),
            ..ghost("", geom, style)
        });
    }
    items
}

/// One free-text box, geometry only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextBox {
    pub id: Id,
    #[serde(rename = "box")]
    pub box_: Rect,
    pub editing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rot: Option<f64>,
}

/// The live free-text boxes on a page.
pub fn text_boxes(m: &Model, pon: PageObjectNumber, view: Option<ViewEnv>) -> Vec<TextBox> {
    let mut out = Vec::new();
    for id in &m.order {
        let a = &m.by_id[id];
        if a.pon != pon || !matches!(a.geom, Geom::Text { .. }) {
            continue;
        }
        if !viewable(&a.flags, m.selected.contains(id)) {
            continue;
        }
        if !text_is_live(m, id) {
            continue;
        }
        let g = eff_geom(m, id, view);
        let Geom::Text { rect, .. } = &g else { continue };
        out.push(TextBox {
            id: id.clone(),
            box_: *rect,
            editing: m.editing.as_deref() == Some(id.as_str()),
            rot: Some(geom_rotation(&g)),
        });
    }
    out
}

/// The selected annotations as render items, cross-page.
pub fn selected_items(m: &Model, view: Option<ViewEnv>) -> Vec<RenderItem> {
    let mut items = Vec::new();
    for id in &m.selected {
        let Some(a) = m.by_id.get(id) else { continue };
        let geom = eff_geom(m, id, view);
        let style = eff_style(a, view);
        items.push(RenderItem {
            id: id.clone(),
            ref_: a.ref_.clone(),
            subtype: a.subtype.clone(),
            box_: geom_visual_bounds(&geom, style.stroke_width, Some(&style.border)),
            rot: Some(geom_rotation(&geom)),
            geom,
            ap_box: None,
            ap_rot: None,
            style,
            text: None,
            source: a.source.into(),
            selected: true,
            hovered: None,
            label: None,
            blend: None,
        });
    }
    items
}

fn box_corners(r: Rect) -> [Point; 4] {
    [
        Point { x: r.x, y: r.y },
        Point { x: r.x + r.width, y: r.y },
        Point { x: r.x + r.width, y: r.y + r.height },
        Point { x: r.x, y: r.y + r.height },
    ]
}

fn union_bounds_of(
    m: &Model,
    pon: PageObjectNumber,
    geom_of: &dyn Fn(&str) -> Geom,
    view: Option<ViewEnv>,
) -> Option<Rect> {
    let mut corners = Vec::new();
    for id in &m.selected {
        let Some(a) = m.by_id.get(id) else { continue };
        if a.pon != pon {
            continue;
        }
        corners.extend(selection_quad(
            &geom_of(id),
            eff_style(a, view).stroke_width,
            Some(&a.style.border),
        ));
    }
    (!corners.is_empty()).then(|| union_rect(&corners))
}

fn selectable_on_page(m: &Model, pon: PageObjectNumber) -> Vec<Id> {
    m.selected
        .iter()
        .filter(|id| is_selectable(m, id) && m.by_id[*id].pon == pon)
        .cloned()
        .collect()
}

fn place_selection_knob(
    m: &Model,
    pon: PageObjectNumber,
    page_box: Option<Rect>,
    knob_offset: f64,
    geom_of: &dyn Fn(&str) -> Geom,
    view: Option<ViewEnv>,
) -> Option<Knob> {
    let sel = selectable_on_page(m, pon);
    if sel.len() == 1 {
        let a = &m.by_id[&sel[0]];
        if !caps_for(&a.subtype).rotatable || !annot_transformable(a) {
            return None;
        }
        let obb = obb_from_geom(
            &geom_of(&sel[0]),
            eff_style(a, view).stroke_width,
            Some(&a.style.border),
        )?;
        return Some(place_rotate_knob(&obb.corners, knob_offset, page_box));
    }
    if sel.len() > 1
        && group_caps(m, &sel).rotatable
        && let Some(union) = union_bounds_of(m, pon, geom_of, view)
    {
        return Some(place_rotate_knob(&box_corners(union), knob_offset, page_box));
    }
    None
}

/// The rotate knob for the selection on `pon`, or `None`. During a live
/// rotate it rides the committed placement, turned by the snapped delta.
pub fn selection_knob(
    m: &Model,
    pon: PageObjectNumber,
    page_box: Option<Rect>,
    knob_offset: Option<f64>,
    view: Option<ViewEnv>,
) -> Option<Knob> {
    let knob_offset = knob_offset.unwrap_or(ROTATE_KNOB_OFFSET);
    if let Some(d @ Draft::Rotate { ids, pivot, .. }) = &m.draft
        && ids.first().and_then(|id| m.by_id.get(id)).is_some_and(|a| a.pon == pon)
    {
        let committed = |id: &str| {
            let a = &m.by_id[id];
            anchored_geom(&a.geom, anchor_mode_of(a), view)
        };
        let rest = place_selection_knob(m, pon, page_box, knob_offset, &committed, view)?;
        let delta = rotate_draft_delta(m, &d.as_rotate().expect("a rotate draft")).delta;
        return Some(Knob {
            at: rotate_point(rest.at, *pivot, delta),
            from: rotate_point(rest.from, *pivot, delta),
        });
    }
    place_selection_knob(m, pon, page_box, knob_offset, &|id| eff_geom(m, id, view), view)
}

/// The selection overlay for a page.
pub fn chrome(
    m: &Model,
    pon: PageObjectNumber,
    page_box: Option<Rect>,
    knob_offset: Option<f64>,
    view: Option<ViewEnv>,
) -> Vec<ChromeNode> {
    let mut nodes = Vec::new();
    if let Some(Draft::Marquee { pon: p, from, to }) = &m.draft
        && *p == pon
    {
        nodes.push(ChromeNode::Marquee { rect: rect_from_points(*from, *to) });
    }
    let first_on_page =
        |ids: &[Id]| ids.first().and_then(|id| m.by_id.get(id)).is_some_and(|a| a.pon == pon);
    if let Some(Draft::Move { ids, guides, .. }) = &m.draft
        && !guides.is_empty()
        && first_on_page(ids)
    {
        for g in guides {
            nodes.push(ChromeNode::Guide { axis: g.axis, at: g.at, lo: g.lo, hi: g.hi });
        }
    }
    let rd = match &m.draft {
        Some(d @ Draft::Rotate { ids, .. }) if first_on_page(ids) => d.as_rotate(),
        _ => None,
    };
    if let Some(rd) = &rd {
        let angle = rotate_draft_delta(m, rd).angle;
        nodes.push(ChromeNode::AngleChip { at: rd.cur, angle: js::round(angle) });
        let span = page_box.unwrap_or(Rect {
            x: rd.pivot.x - 300.0,
            y: rd.pivot.y - 300.0,
            width: 600.0,
            height: 600.0,
        });
        let mut lines = Vec::new();
        for (deg, role) in
            [(0.0, GuideRole::Axis), (90.0, GuideRole::Axis), (angle, GuideRole::Indicator)]
        {
            if let Some(c) = chord_through(span, rd.pivot, deg) {
                lines.push(GuideLine { a: c.a, b: c.b, role });
            }
        }
        nodes.push(ChromeNode::RotateGuides { center: rd.pivot, angle, lines });
    }
    let sel = selectable_on_page(m, pon);
    if sel.len() == 1 {
        let a = &m.by_id[&sel[0]];
        let g = eff_geom(m, &sel[0], view);
        let style = eff_style(a, view);
        let caps = caps_for(&a.subtype);
        let rot = geom_rotation(&g);
        match obb_from_geom(&g, style.stroke_width, Some(&a.style.border)) {
            Some(obb) if rot != 0.0 => {
                nodes.push(ChromeNode::Obb { corners: obb.corners, angle: obb.angle })
            }
            _ => nodes.push(ChromeNode::Outline {
                rect: selection_bounds(&g, style.stroke_width, Some(&a.style.border)),
            }),
        }
        if rd.is_none() && annot_transformable(a) && (caps.resizable || caps.vertex_editable) {
            for h in geom_handles(&g) {
                nodes.push(ChromeNode::Handle { at: h.at, cursor: h.cursor, rot: truthy(rot) });
            }
        }
    } else if sel.len() > 1
        && rd.is_none()
        && let Some(union) = union_bounds_of(m, pon, &|id| eff_geom(m, id, view), view)
    {
        nodes.push(ChromeNode::Outline { rect: union });
        if group_caps(m, &sel).resizable {
            for h in rect_handles_for(union) {
                nodes.push(ChromeNode::Handle { at: h.at, cursor: h.cursor, rot: None });
            }
        }
    }
    if rd.is_none()
        && let Some(knob) = selection_knob(m, pon, page_box, knob_offset, view)
    {
        nodes.push(ChromeNode::RotateKnob { at: knob.at, from: knob.from });
    }
    nodes
}

/// The union of the selectable selection on `pon`, or `None`.
pub fn selection_bounds_on_page(
    m: &Model,
    pon: PageObjectNumber,
    view: Option<ViewEnv>,
) -> Option<Rect> {
    let sel = selectable_on_page(m, pon);
    if sel.is_empty() {
        return None;
    }
    let corners: Vec<Point> = sel
        .iter()
        .flat_map(|id| {
            let a = &m.by_id[id];
            selection_quad(
                &eff_geom(m, id, view),
                eff_style(a, view).stroke_width,
                Some(&a.style.border),
            )
        })
        .collect();
    Some(union_rect(&corners))
}

/// The anchor for a selection-aware floating menu.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionAnchor {
    pub pon: PageObjectNumber,
    pub bounds: Rect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knob: Option<Point>,
}

/// The primary page of the selection, its bounds there, and the knob. The
/// three lookups resolve per page (`undefined` for none).
pub fn selection_anchor(
    m: &Model,
    page_box_of: Option<&dyn Fn(PageObjectNumber) -> Option<Rect>>,
    knob_offset_of: Option<&dyn Fn(PageObjectNumber) -> Option<f64>>,
    view_of: Option<&dyn Fn(PageObjectNumber) -> Option<ViewEnv>>,
) -> Option<SelectionAnchor> {
    if matches!(m.draft, Some(Draft::Rotate { .. })) {
        return None;
    }
    let id = m.selected.iter().find(|x| is_selectable(m, x))?;
    let pon = m.by_id[id].pon;
    let view = view_of.and_then(|f| f(pon));
    let bounds = selection_bounds_on_page(m, pon, view)?;
    let knob = selection_knob(
        m,
        pon,
        page_box_of.and_then(|f| f(pon)),
        Some(knob_offset_of.and_then(|f| f(pon)).unwrap_or(ROTATE_KNOB_OFFSET)),
        view,
    );
    Some(SelectionAnchor { pon, bounds, knob: knob.map(|k| k.at) })
}

/// Anchor for controls that finish or cancel a multi-click poly creation.
pub fn creation_draft_anchor(m: &Model) -> Option<CreationDraftAnchor> {
    let Some(Draft::CreatePoly { pon, points, closed, .. }) = &m.draft else {
        return None;
    };
    if points.is_empty() {
        return None;
    }
    let min_points = if *closed { 3 } else { 2 };
    Some(CreationDraftAnchor {
        kind: CreationDraftKind::Poly,
        subtype: if *closed { PolySubtype::Polygon } else { PolySubtype::Polyline },
        pon: *pon,
        bounds: union_rect(points),
        point_count: points.len(),
        min_points,
        can_finish: points.len() >= min_points,
    })
}
