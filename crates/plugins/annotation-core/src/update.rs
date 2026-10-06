//! `update.ts`: the pure reducer, `update(model, msg) -> (model, effects)`.
//!
//! The TypeScript returns the model it was given when nothing changed, and a
//! store skips its notification for that. [`update`] returns `None` for that
//! branch: `Some` is a new model even when its contents equal the old one's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::anchor::{anchor_mode_of, anchored_geom, anchored_stroke_width, unanchored_geom};
use crate::flags::{
    DRAWN_FLAGS, annot_contents_editable, annot_deletable, annot_transformable, merge_flags,
};
use crate::geometry::{
    DEFAULT_CHROME_GEOM, ap_size_changed, caret_geom_from_anchor, geom_drag_handle,
    geom_reset_rotation, geom_rotate_about, geom_rotation, geom_scale_about, geom_translate,
    geom_visual_bounds, group_resize_anchor, group_resize_box, group_resize_factors, normalize_deg,
    quad_intersects_rect, rect_from_points, rotate_point, rotated_aabb, shape_rect_for,
    transposed_about_center, union_rect, upright_anchored_rect, upright_rotation,
};
use crate::group::{expand_groups, group_members};
use crate::hit::{Target, can_move, group_union_bounds, hit_test, is_selectable};
use crate::ink::straighten_ink_stroke;
use crate::js::{self, IdSet, Nullable, Record};
use crate::kinds::caps_for;
use crate::links::link_children_of;
use crate::measure::Unavailable;
use crate::measurement::{DistanceAppearance, MeasurementAppearance, distance_leader_length};
use crate::measurement_shape::{
    move_measurement_caption, shape_measurement_readout, transform_measurement_caption,
};
use crate::placement::{
    PlacementOptions, clamp_rect_to_box, click_create_geom, resolve_click_placement,
};
use crate::plane::is_substrate_only;
use crate::props::{
    apply_props_tracked, kind_takes_link, resolve_props, style_from_props, text_style_from_props,
};
use crate::richtext::{
    RichTextDocumentInput, normalize_doc, paragraphs_from_plain_text, plain_text_of,
};
use crate::selection::annotation_selection_frame;
use crate::snap::compute_move_snap;
use crate::types::{
    Annot, AnnotationFlagsPatch, AnnotationProps, AnnotationPropsPatch, AnnotationRef, BlendMode,
    Border, CalloutStep, ClickCreate, ClickCreateOption, DistanceStep, Draft, Effect, Geom,
    GroupOp, Id, InkStraightenOptions, Intent, MarkupPreview, Model, Msg, PageRef, PatchScope,
    Phase, Point, PointerInput, Rect, RotateDraft, SnapSettings, Source, Style, TextEndAnchor,
    TextQuad, ViewEnv,
};

/// The click and drag threshold (content units).
pub const MIN_DRAG: f64 = 3.0;

fn is_poly_subtype(subtype: &str) -> bool {
    subtype == "polygon" || subtype == "polyline"
}

pub fn initial_style() -> Style {
    Style {
        color: "#e5484d".into(),
        interior_color: None,
        stroke_width: 2.0,
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        border: Border::Solid,
    }
}

pub fn initial_model() -> Model {
    Model {
        by_id: BTreeMap::new(),
        order: Vec::new(),
        selected: Vec::new(),
        hovered: None,
        draft: None,
        preview: None,
        seq: 0.0,
        style: initial_style(),
        defaults: BTreeMap::new(),
        hit_margin: 6.0,
        editing: None,
        snap: SnapSettings {
            guides: true,
            guide_threshold: 5.0,
            rotation: true,
            rotation_angles: vec![0.0, 90.0, 180.0, 270.0],
            rotation_threshold: 4.0,
        },
    }
}

/// A tool's effective defaults as a full props bag: the base style and the
/// text base, with the per-tool override on top.
pub fn defaults_for(m: &Model, subtype: &str) -> AnnotationProps {
    resolve_props(&m.style, m.defaults.get(subtype))
}

fn to_vector(a: Annot) -> Annot {
    Annot { source: Source::Vector, ..a }
}

/// Take ownership of the appearance after a geometry edit: vector kinds go
/// live, opaque-body kinds stay baked with the raster box following.
fn own_geometry(a: Annot) -> Annot {
    if !caps_for(&a.subtype).opaque_body {
        return to_vector(a);
    }
    match a.geom.rect() {
        Some(rect) => Annot { ap_box: Some(*rect), ..a },
        None => a,
    }
}

fn patch_fx(id: &str, next: &Annot, before: &Geom) -> Effect {
    Effect::Patch {
        id: id.into(),
        scope: PatchScope::Geometry,
        ap_changed: next.source == Source::Baked && ap_size_changed(before, &next.geom),
    }
}

fn sub(a: Point, b: Point) -> Point {
    Point { x: a.x - b.x, y: a.y - b.y }
}

fn commit_view_gesture(a: &Annot, view: Option<ViewEnv>, op: impl Fn(&Geom) -> Geom) -> Geom {
    let mode = anchor_mode_of(a);
    unanchored_geom(&op(&anchored_geom(&a.geom, mode, view)), mode, view)
}

fn clamp_axis(v: f64, lo: f64, hi: f64) -> f64 {
    js::max(lo, js::min(hi, v))
}

fn clamp_point_to_box(p: Point, bx: Option<Rect>) -> Point {
    match bx {
        Some(b) => Point {
            x: clamp_axis(p.x, b.x, b.x + b.width),
            y: clamp_axis(p.y, b.y, b.y + b.height),
        },
        None => p,
    }
}

fn view_of(input: &PointerInput) -> Option<ViewEnv> {
    (input.zoom.is_some() || input.display_rotation.is_some()).then(|| ViewEnv {
        zoom: input.zoom.unwrap_or(1.0),
        rotation: input.display_rotation.unwrap_or(0.0),
    })
}

/// Corners of the region a move keeps inside the page: the selection frame,
/// or a callout's visual bounds (box, leader and arrowhead).
fn move_clamp_corners(a: &Annot, view: Option<ViewEnv>) -> [Point; 4] {
    let mode = anchor_mode_of(a);
    let geom = anchored_geom(&a.geom, mode, view);
    if geom.is_callout() {
        let v = geom_visual_bounds(
            &geom,
            anchored_stroke_width(a.style.stroke_width, mode, view),
            Some(&a.style.border),
        );
        return [
            Point { x: v.x, y: v.y },
            Point { x: v.x + v.width, y: v.y },
            Point { x: v.x + v.width, y: v.y + v.height },
            Point { x: v.x, y: v.y + v.height },
        ];
    }
    annotation_selection_frame(a, view).corners
}

fn union_bounds_of(m: &Model, ids: &[Id], view: Option<ViewEnv>) -> Option<Rect> {
    let mut corners = Vec::new();
    for id in ids {
        let Some(a) = m.by_id.get(id) else { continue };
        corners.extend(move_clamp_corners(a, view));
    }
    (!corners.is_empty()).then(|| union_rect(&corners))
}

fn clamp_move_delta(
    m: &Model,
    ids: &[Id],
    delta: Point,
    page: Option<Rect>,
    view: Option<ViewEnv>,
) -> Point {
    let Some(page) = page else {
        return delta;
    };
    let Some(b) = union_bounds_of(m, ids, view) else {
        return delta;
    };
    Point {
        x: clamp_axis(delta.x, page.x - b.x, page.x + page.width - (b.x + b.width)),
        y: clamp_axis(delta.y, page.y - b.y, page.y + page.height - (b.y + b.height)),
    }
}

fn edit_draft_page(m: &Model, d: &Draft) -> Option<PageRef> {
    let id = d.id().or_else(|| d.ids().and_then(|ids| ids.first()))?;
    m.by_id.get(id).map(|a| a.page)
}

/// `JSON.stringify(a) === JSON.stringify(b)`: non-finite numbers print as
/// null and member order is the types' own.
fn geom_equal(a: &Geom, b: &Geom) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

const RAD2DEG: f64 = 180.0 / std::f64::consts::PI;

fn angle_at(pivot: Point, p: Point) -> f64 {
    (p.y - pivot.y).atan2(p.x - pivot.x) * RAD2DEG
}

fn arc_to(a: f64, b: f64) -> f64 {
    ((b - a + 540.0) % 360.0) - 180.0
}

/// A rotate draft's live rotation with snapping.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RotateDelta {
    pub delta: f64,
    pub angle: f64,
    pub snapped: bool,
}

/// The live rotation of a rotate draft, snapping applied: the one angle rule
/// the preview, the commit and the angle chip share.
pub fn rotate_draft_delta(m: &Model, d: &RotateDraft) -> RotateDelta {
    let raw = angle_at(d.pivot, d.cur) - angle_at(d.pivot, d.start);
    let one = if d.ids.len() == 1 { m.by_id.get(&d.ids[0]) } else { None };
    let base =
        one.map_or(0.0, |a| geom_rotation(&anchored_geom(&a.geom, anchor_mode_of(a), d.view)));
    let angle = normalize_deg(base + raw);
    if !m.snap.rotation || d.free == Some(true) {
        return RotateDelta { delta: raw, angle, snapped: false };
    }
    for &target in &m.snap.rotation_angles {
        let adjust = arc_to(angle, normalize_deg(target));
        if adjust.abs() <= m.snap.rotation_threshold {
            return RotateDelta {
                delta: raw + adjust,
                angle: normalize_deg(target),
                snapped: true,
            };
        }
    }
    RotateDelta { delta: raw, angle, snapped: false }
}

/// A group resize is isotropic when any member is rotated (a missing member
/// reads as an unrotated caret).
fn selection_has_rotation(m: &Model, ids: &[Id]) -> bool {
    ids.iter().any(|id| m.by_id.get(id).map_or(0.0, |a| geom_rotation(&a.geom)) != 0.0)
}

type Step = (Option<Model>, Vec<Effect>);

fn same() -> Step {
    (None, Vec::new())
}

fn changed(m: Model) -> Step {
    (Some(m), Vec::new())
}

/// `update(model, msg)`: the next model (`None` when the TypeScript returns
/// the same object) and the effects to perform.
pub fn update(m: &Model, msg: &Msg) -> (Option<Model>, Vec<Effect>) {
    match msg {
        Msg::EditPointer { phase, input } => edit_pointer(m, *phase, input),
        Msg::MarqueePointer { phase, input } => marquee_pointer(m, *phase, input),
        Msg::CreatePointer {
            measure,
            capture,
            phase,
            subtype,
            preset,
            intent,
            click_create,
            flags,
            defer_ink_commit,
            straighten_ink,
            input,
        } => create_pointer(
            m,
            *phase,
            subtype,
            input,
            preset.clone().unwrap_or_else(|| subtype.clone()),
            *intent,
            defer_ink_commit.unwrap_or(false),
            straighten_ink.as_ref(),
            *click_create,
            flags.as_ref(),
            measure.as_ref(),
            capture.as_deref(),
        ),
        Msg::FinishInkDraft => finish_ink_create(m),
        Msg::FinishCreationDraft => finish_poly_create(m),
        Msg::CreateCaret { page, anchor, flags } => create_caret(m, *page, anchor, flags.as_ref()),
        Msg::CreateReplaceText { page, quads, anchor, preset } => create_replace_text(
            m,
            *page,
            quads,
            anchor,
            preset.as_deref().unwrap_or("replace-text"),
        ),
        Msg::CreateMarkup { subtype, page, quads, preset, flags } => create_markup(
            m,
            subtype,
            *page,
            quads,
            preset.as_deref().unwrap_or(subtype),
            flags.as_ref(),
        ),
        Msg::CreateAnnot { page, subtype, geom, preset, props, flags, select } => create_annot(
            m,
            *page,
            subtype,
            geom,
            preset.as_deref(),
            props.as_ref(),
            flags.as_ref(),
            *select == Some(true),
        ),
        Msg::SetMarkupPreview { subtype, quads_by_page, preset } => {
            set_markup_preview(m, subtype, quads_by_page, preset.as_deref().unwrap_or(subtype))
        }
        Msg::ClearMarkupPreview => {
            if m.preview.is_some() {
                changed(Model { preview: None, ..m.clone() })
            } else {
                same()
            }
        }
        Msg::Select { ids, add } => {
            let valid: Vec<Id> = ids.iter().filter(|id| is_selectable(m, id)).cloned().collect();
            let ids = expand_groups(m, &valid);
            if ids.is_empty() {
                return same();
            }
            let selected = if *add == Some(true) {
                let mut out: Vec<Id> = Vec::new();
                for id in m.selected.iter().chain(ids.iter()) {
                    if !out.contains(id) {
                        out.push(id.clone());
                    }
                }
                out
            } else {
                ids
            };
            changed(Model { selected, ..m.clone() })
        }
        Msg::Deselect { ids } => {
            if m.selected.is_empty() {
                return same();
            }
            let Some(drop) = ids else {
                return changed(Model { selected: Vec::new(), ..m.clone() });
            };
            let selected: Vec<Id> =
                m.selected.iter().filter(|id| !drop.contains(id)).cloned().collect();
            if selected.len() == m.selected.len() {
                same()
            } else {
                changed(Model { selected, ..m.clone() })
            }
        }
        Msg::SetProps { patch } => set_props(m, patch),
        Msg::SetFlags { patch, ids } => set_flags(m, patch, ids.as_deref()),
        Msg::SetDefaults { subtype, patch } => set_defaults(m, subtype, patch),
        Msg::SetSnap { patch } => {
            let s = &m.snap;
            let snap = SnapSettings {
                guides: patch.guides.unwrap_or(s.guides),
                guide_threshold: patch.guide_threshold.unwrap_or(s.guide_threshold),
                rotation: patch.rotation.unwrap_or(s.rotation),
                rotation_angles: patch
                    .rotation_angles
                    .clone()
                    .unwrap_or_else(|| s.rotation_angles.clone()),
                rotation_threshold: patch.rotation_threshold.unwrap_or(s.rotation_threshold),
            };
            changed(Model { snap, ..m.clone() })
        }
        Msg::Rotate90 => rotate_selection(m, 90.0),
        Msg::ResetRotation => reset_rotation(m),
        Msg::Delete => delete_selection(m),
        Msg::Cancel => changed(Model { draft: None, ..m.clone() }),
        Msg::Loaded { annots } => changed(merge_loaded(m, annots)),
        Msg::Hydrated { annots, bump_ap } => {
            changed(hydrate_annots(m, annots, bump_ap.unwrap_or(false)))
        }
        Msg::Created { temp_id, id, ref_ } => match reconcile(m, temp_id, id, ref_) {
            Some(next) => changed(next),
            None => same(),
        },
        Msg::CreateFailed { temp_id } => changed(remove_annots(m, std::slice::from_ref(temp_id))),
        Msg::Upsert { annots, bump_ap } => {
            changed(upsert_annots(m, annots, bump_ap.unwrap_or(false)))
        }
        Msg::BumpAp { ids } => match bump_ap(m, ids) {
            Some(next) => changed(next),
            None => same(),
        },
        Msg::Hover { id } => {
            if m.hovered == *id {
                same()
            } else {
                changed(Model { hovered: id.clone(), ..m.clone() })
            }
        }
        Msg::Remove { ids } => {
            let next = remove_annots(m, ids);
            let dangling =
                next.hovered.as_ref().is_some_and(|h| !h.is_empty() && !next.by_id.contains_key(h));
            changed(if dangling { Model { hovered: None, ..next } } else { next })
        }
        Msg::BeginTextEdit { id } => {
            if m.by_id.get(id).is_some_and(annot_contents_editable) {
                changed(Model {
                    editing: Some(id.clone()),
                    selected: vec![id.clone()],
                    draft: None,
                    ..m.clone()
                })
            } else {
                same()
            }
        }
        Msg::SetText { id, text } => set_text(m, id, text),
        Msg::SetRichText { id, doc } => set_rich_text(m, id, doc),
        Msg::EndTextEdit => {
            if m.editing.as_ref().is_some_and(|e| !e.is_empty()) {
                changed(Model { editing: None, ..m.clone() })
            } else {
                same()
            }
        }
    }
}

fn with_annot(m: &Model, id: &str, a: Annot) -> BTreeMap<Id, Annot> {
    let mut by_id = m.by_id.clone();
    by_id.insert(id.to_string(), a);
    by_id
}

fn set_text(m: &Model, id: &str, text: &str) -> Step {
    let Some(a) = m.by_id.get(id) else {
        return same();
    };
    let mut next = a.clone();
    // A DTO is an object; the spread of a scalar one is not ported.
    if let Some(Value::Object(data)) = &mut next.data {
        let free_text = data.get("subtype").and_then(Value::as_str) == Some("free-text");
        data.insert("contents".into(), Value::String(text.into()));
        if free_text {
            // `{ ...data.richText, paragraphs }`: the body and any other
            // member stay.
            let mut rich = match data.get("richText") {
                Some(Value::Object(r)) => r.clone(),
                _ => serde_json::Map::new(),
            };
            rich.insert(
                "paragraphs".into(),
                serde_json::to_value(paragraphs_from_plain_text(text)).unwrap_or(Value::Null),
            );
            data.insert("richText".into(), Value::Object(rich));
        }
    }
    changed(Model { by_id: with_annot(m, id, to_vector(next)), ..m.clone() })
}

/// Apply the editor's rich document: the DTO's body with the input's partial
/// body layered on, the normalised paragraphs, and `contents` as the plain
/// projection.
fn set_rich_text(m: &Model, id: &str, doc: &RichTextDocumentInput) -> Step {
    let Some(a) = m.by_id.get(id) else {
        return same();
    };
    let Some(Value::Object(data)) = &a.data else {
        return same();
    };
    if data.get("subtype").and_then(Value::as_str) != Some("free-text") {
        return same();
    }
    let normalized = normalize_doc(doc);
    let mut body = match data.get("richText").and_then(|r| r.get("body")) {
        Some(Value::Object(b)) => b.clone(),
        _ => serde_json::Map::new(),
    };
    for (k, v) in normalized.body.iter().flatten() {
        body.insert(k.clone(), v.clone());
    }
    let mut rich = serde_json::Map::new();
    rich.insert("body".into(), Value::Object(body));
    rich.insert(
        "paragraphs".into(),
        serde_json::to_value(&normalized.paragraphs).unwrap_or(Value::Null),
    );
    let mut data = data.clone();
    data.insert("richText".into(), Value::Object(rich));
    data.insert("contents".into(), Value::String(plain_text_of(&normalized.paragraphs)));
    let next = Annot { data: Some(Value::Object(data)), ..a.clone() };
    changed(Model { by_id: with_annot(m, id, to_vector(next)), ..m.clone() })
}

fn edit_pointer(m: &Model, phase: Phase, input: &PointerInput) -> Step {
    match phase {
        Phase::Down => edit_down(m, input),
        Phase::Move => {
            if m.draft.is_some() {
                edit_move(m, input)
            } else {
                same()
            }
        }
        Phase::Up => {
            if m.draft.is_some() {
                edit_up(m)
            } else {
                same()
            }
        }
    }
}

fn edit_down(m: &Model, input: &PointerInput) -> Step {
    let view = view_of(input);
    let hit = hit_test(
        m,
        input.page,
        input.point,
        input.chrome.as_ref().unwrap_or(&DEFAULT_CHROME_GEOM),
        m.hit_margin,
        input.page_box,
        input.inert.as_ref(),
        view,
    );
    match hit {
        Target::Handle { id, handle, .. } if handle == "caption" => {
            let draft = Draft::Caption { id, start: input.point, delta: Point { x: 0.0, y: 0.0 } };
            changed(Model { draft: Some(draft), ..m.clone() })
        }
        Target::Handle { id, handle, .. } if handle == "leader-start" || handle == "leader-end" => {
            let draft = Draft::Leader { id, start: input.point, delta: 0.0 };
            changed(Model { draft: Some(draft), ..m.clone() })
        }
        Target::Handle { id, handle, .. } => {
            let a = &m.by_id[&id];
            let base = anchored_geom(&a.geom, anchor_mode_of(a), view);
            let draft = Draft::Handle { id, handle, cur: base.clone(), base, view };
            changed(Model { draft: Some(draft), ..m.clone() })
        }
        Target::Rotate { ids, pivot } => {
            let draft = Draft::Rotate {
                ids,
                pivot,
                start: input.point,
                cur: input.point,
                free: None,
                view,
            };
            changed(Model { draft: Some(draft), ..m.clone() })
        }
        Target::GroupHandle { ids, handle, box_, .. } => {
            let anchor = group_resize_anchor(box_, &handle);
            let draft = Draft::Group {
                op: GroupOp::Resize,
                ids,
                handle,
                anchor,
                base: box_,
                cur: box_,
                view,
            };
            changed(Model { draft: Some(draft), ..m.clone() })
        }
        Target::Annot { id } => {
            let grp = group_members(m, &id);
            let in_sel = m.selected.contains(&id);
            let selected: Vec<Id> = if input.shift {
                if in_sel {
                    m.selected.iter().filter(|x| !grp.contains(x)).cloned().collect()
                } else {
                    let mut s = m.selected.clone();
                    s.extend(grp.iter().filter(|x| !m.selected.contains(x)).cloned());
                    s
                }
            } else if in_sel {
                m.selected.clone()
            } else {
                grp
            };
            let movable = !selected.is_empty() && selected.iter().all(|id| can_move(m, id));
            let draft = movable.then(|| Draft::Move {
                ids: selected.clone(),
                start: input.point,
                delta: Point { x: 0.0, y: 0.0 },
                guides: Vec::new(),
            });
            changed(Model { selected, draft, ..m.clone() })
        }
        Target::Empty => changed(Model { selected: Vec::new(), ..m.clone() }),
    }
}

fn with_draft(m: &Model, draft: Draft) -> Step {
    changed(Model { draft: Some(draft), ..m.clone() })
}

fn edit_move(m: &Model, input: &PointerInput) -> Step {
    let d = m.draft.as_ref().expect("a draft");
    if let Some(home) = edit_draft_page(m, d)
        && input.page.pon() != home.pon()
    {
        return same();
    }
    if let Draft::Move { ids, start, .. } = d {
        if ids.iter().any(|id| !m.by_id.contains_key(id)) {
            return changed(Model { draft: None, ..m.clone() });
        }
        let view = view_of(input);
        let raw = clamp_move_delta(m, ids, sub(input.point, *start), input.page_box, view);
        if !m.snap.guides || input.shift {
            return with_draft(
                m,
                Draft::Move { ids: ids.clone(), start: *start, delta: raw, guides: Vec::new() },
            );
        }
        let snap =
            compute_move_snap(m, ids, input.page, raw, m.snap.guide_threshold, input.page_box);
        let delta = clamp_move_delta(m, ids, snap.delta, input.page_box, view);
        let guides = snap
            .guides
            .into_iter()
            .filter(|g| match g.axis {
                crate::types::Axis::X => delta.x == snap.delta.x,
                crate::types::Axis::Y => delta.y == snap.delta.y,
            })
            .collect();
        return with_draft(m, Draft::Move { ids: ids.clone(), start: *start, delta, guides });
    }
    let point = clamp_point_to_box(input.point, input.page_box);
    match d {
        Draft::Leader { id, start, .. } => {
            let Some(a) = m.by_id.get(id) else {
                return changed(Model { draft: None, ..m.clone() });
            };
            let geom = &a.geom;
            let delta = distance_leader_length(geom, point) - distance_leader_length(geom, *start);
            with_draft(m, Draft::Leader { id: id.clone(), start: *start, delta })
        }
        Draft::Caption { id, start, .. } => with_draft(
            m,
            Draft::Caption {
                id: id.clone(),
                start: *start,
                delta: Point { x: point.x - start.x, y: point.y - start.y },
            },
        ),
        Draft::Handle { id, handle, base, view, .. } => with_draft(
            m,
            Draft::Handle {
                id: id.clone(),
                handle: handle.clone(),
                base: base.clone(),
                cur: geom_drag_handle(base, handle, point),
                view: *view,
            },
        ),
        Draft::Rotate { ids, pivot, start, view, .. } => with_draft(
            m,
            Draft::Rotate {
                ids: ids.clone(),
                pivot: *pivot,
                start: *start,
                cur: input.point,
                free: Some(input.shift),
                view: *view,
            },
        ),
        Draft::Group { op, ids, handle, anchor, base, view, .. } => {
            let iso = selection_has_rotation(m, ids);
            with_draft(
                m,
                Draft::Group {
                    op: *op,
                    ids: ids.clone(),
                    handle: handle.clone(),
                    anchor: *anchor,
                    base: *base,
                    cur: group_resize_box(*base, handle, point, iso),
                    view: *view,
                },
            )
        }
        _ => same(),
    }
}

fn edit_up(m: &Model) -> Step {
    let d = m.draft.as_ref().expect("a draft");
    let cleared = || Model { draft: None, ..m.clone() };
    match d {
        Draft::Leader { id, delta, .. } => {
            let Some(a) = m.by_id.get(id) else {
                return changed(cleared());
            };
            let Some(MeasurementAppearance::Distance(measure)) = &a.measure else {
                return changed(cleared());
            };
            if *delta == 0.0 {
                return changed(cleared());
            }
            let length = measure.leader.map_or(0.0, |l| l.length) + delta;
            let updated = Annot {
                source: Source::Vector,
                measure: Some(MeasurementAppearance::Distance(measure.with_leader_length(length))),
                ..a.clone()
            };
            let fx = Effect::Patch { id: id.clone(), scope: PatchScope::Leader, ap_changed: false };
            (Some(Model { draft: None, by_id: with_annot(m, id, updated), ..m.clone() }), vec![fx])
        }
        Draft::Caption { id, delta, .. } => {
            let Some(a) = m.by_id.get(id) else {
                return changed(cleared());
            };
            let Some(measure) = &a.measure else {
                return changed(cleared());
            };
            let moved = delta.x != 0.0 && !delta.x.is_nan() || delta.y != 0.0 && !delta.y.is_nan();
            if !moved {
                return changed(cleared());
            }
            let updated = Annot {
                source: Source::Vector,
                measure: Some(move_measurement_caption(&a.geom, measure, *delta, &a.style)),
                ..a.clone()
            };
            let fx =
                Effect::Patch { id: id.clone(), scope: PatchScope::Caption, ap_changed: false };
            (Some(Model { draft: None, by_id: with_annot(m, id, updated), ..m.clone() }), vec![fx])
        }
        Draft::Handle { id, base, cur, view, .. } => {
            if geom_equal(base, cur) {
                return changed(cleared());
            }
            let Some(before) = m.by_id.get(id) else {
                return changed(cleared());
            };
            let stored = unanchored_geom(cur, anchor_mode_of(before), *view);
            if let Some(MeasurementAppearance::Shape(shape)) = &before.measure
                && shape.is_polygon()
                && shape_measurement_readout(&stored, shape).unavailable()
                    == Some(Unavailable::InvalidGeometry)
            {
                return changed(cleared());
            }
            let a = own_geometry(Annot { geom: stored, ..before.clone() });
            let fx = patch_fx(id, &a, &before.geom);
            (Some(Model { by_id: with_annot(m, id, a), draft: None, ..m.clone() }), vec![fx])
        }
        Draft::Rotate { .. } => {
            let rd = d.as_rotate().expect("a rotate draft");
            let delta = rotate_draft_delta(m, &rd).delta;
            if delta.abs() < 0.01 {
                return changed(cleared());
            }
            let mut by_id = m.by_id.clone();
            let mut fx = Vec::new();
            for id in rd.ids {
                let Some(a) = by_id.get(id).cloned() else { continue };
                let geom =
                    commit_view_gesture(&a, rd.view, |g| geom_rotate_about(g, rd.pivot, delta));
                let measure = transform_measurement_caption(a.measure.as_ref(), |p| {
                    rotate_point(p, rd.pivot, delta)
                });
                let next = own_geometry(Annot { geom, measure, ..a.clone() });
                fx.push(patch_fx(id, &next, &a.geom));
                by_id.insert(id.clone(), next);
            }
            (Some(Model { by_id, draft: None, ..m.clone() }), fx)
        }
        Draft::Group { ids, anchor, base, cur, view, .. } => {
            let f = group_resize_factors(*base, *cur);
            if (f.sx - 1.0).abs() < 1e-4 && (f.sy - 1.0).abs() < 1e-4 {
                return changed(cleared());
            }
            let mut by_id = m.by_id.clone();
            let mut fx = Vec::new();
            for id in ids {
                let Some(a) = by_id.get(id).cloned() else { continue };
                let geom =
                    commit_view_gesture(&a, *view, |g| geom_scale_about(g, *anchor, f.sx, f.sy));
                let measure = transform_measurement_caption(a.measure.as_ref(), |p| Point {
                    x: anchor.x + (p.x - anchor.x) * f.sx,
                    y: anchor.y + (p.y - anchor.y) * f.sy,
                });
                let next = own_geometry(Annot { geom, measure, ..a.clone() });
                fx.push(patch_fx(id, &next, &a.geom));
                by_id.insert(id.clone(), next);
            }
            (Some(Model { by_id, draft: None, ..m.clone() }), fx)
        }
        Draft::Move { ids, delta, .. } => {
            if js::hypot(delta.x, delta.y) < 0.01 {
                return changed(cleared());
            }
            if ids.iter().any(|id| !m.by_id.contains_key(id)) {
                return changed(cleared());
            }
            let mut by_id = m.by_id.clone();
            let mut fx = Vec::new();
            for id in ids {
                let a = by_id[id].clone();
                let moved = Annot {
                    geom: geom_translate(&a.geom, *delta),
                    measure: transform_measurement_caption(a.measure.as_ref(), |p| Point {
                        x: p.x + delta.x,
                        y: p.y + delta.y,
                    }),
                    ap_box: a.ap_box.map(|r| Rect { x: r.x + delta.x, y: r.y + delta.y, ..r }),
                    ..a
                };
                by_id.insert(id.clone(), moved);
                fx.push(Effect::Patch {
                    id: id.clone(),
                    scope: PatchScope::Geometry,
                    ap_changed: false,
                });
            }
            (Some(Model { by_id, draft: None, ..m.clone() }), fx)
        }
        _ => changed(cleared()),
    }
}

fn marquee_pointer(m: &Model, phase: Phase, input: &PointerInput) -> Step {
    let point = clamp_point_to_box(input.point, input.page_box);
    if phase == Phase::Down {
        return with_draft(m, Draft::Marquee { page: input.page, from: point, to: point });
    }
    let Some(Draft::Marquee { page, from, .. }) = &m.draft else {
        return same();
    };
    if page.pon() != input.page.pon() {
        return same();
    }
    if phase == Phase::Move {
        return with_draft(m, Draft::Marquee { page: *page, from: *from, to: point });
    }
    let hits = expand_groups(
        m,
        &annots_in_box(m, *page, *from, point, input.inert.as_ref(), view_of(input)),
    );
    let selected = if input.shift { toggle_selection(&m.selected, &hits) } else { hits };
    changed(Model { selected, draft: None, ..m.clone() })
}

fn toggle_selection(base: &[Id], ids: &[Id]) -> Vec<Id> {
    let mut next: Vec<Id> = Vec::new();
    for id in base {
        if !next.contains(id) {
            next.push(id.clone());
        }
    }
    for id in ids {
        if let Some(i) = next.iter().position(|x| x == id) {
            next.remove(i);
        } else {
            next.push(id.clone());
        }
    }
    next
}

fn drawn_flags(flags: Option<&AnnotationFlagsPatch>) -> crate::types::AnnotationFlags {
    flags.map_or(DRAWN_FLAGS, |f| merge_flags(&DRAWN_FLAGS, f))
}

fn tmp_id(seq: f64) -> Id {
    format!("tmp:{}", js::number_to_string(seq))
}

fn fresh_annot(id: Id, page: PageRef, subtype: String, geom: Geom, style: Style) -> Annot {
    Annot {
        id,
        ref_: None,
        page,
        subtype,
        geom,
        style,
        text: None,
        label: None,
        measure: None,
        icon: None,
        flags: DRAWN_FLAGS,
        source: Source::Vector,
        ap_box: None,
        ap_rot: None,
        ap_version: None,
        authority: None,
        data: None,
        intent: None,
        link: Nullable::Undefined,
        irt: None,
        group: None,
    }
}

/// Add a freshly created annotation: selected, the draft cleared.
fn commit_created(m: &Model, annot: Annot, editing: Option<Id>, clear_preview: bool) -> Step {
    let id = annot.id.clone();
    let mut order = m.order.clone();
    order.push(id.clone());
    let next = Model {
        seq: m.seq + 1.0,
        by_id: with_annot(m, &id, annot),
        order,
        selected: vec![id.clone()],
        draft: None,
        editing,
        preview: if clear_preview { None } else { m.preview.clone() },
        ..m.clone()
    };
    (Some(next), vec![Effect::Create { id }])
}

/// Distance creation in two stages: the endpoint drag's release advances to
/// offset placement, and the next click commits.
fn distance_pointer(
    m: &Model,
    phase: Phase,
    input: &PointerInput,
    preset: String,
    measure: Option<&DistanceAppearance>,
    flags: Option<&AnnotationFlagsPatch>,
) -> Step {
    let Some(Draft::CreateDistance { step, page, from, to, measure: draft_measure, .. }) = &m.draft
    else {
        let Some(measure) = measure.filter(|_| phase == Phase::Down) else {
            return same();
        };
        let draft = Draft::CreateDistance {
            step: DistanceStep::Endpoints,
            subtype: "line".into(),
            preset,
            page: input.page,
            from: input.point,
            to: input.point,
            measure: measure.with_leader_length(0.0),
            flags: flags.copied(),
        };
        return changed(Model { selected: Vec::new(), draft: Some(draft), ..m.clone() });
    };
    let draft = m.draft.as_ref().expect("a distance draft");
    if input.page.pon() != page.pon() {
        return same();
    }
    let with = |f: &dyn Fn(&mut Draft)| {
        let mut next = draft.clone();
        f(&mut next);
        next
    };
    if *step == DistanceStep::Endpoints {
        if phase == Phase::Down {
            return same();
        }
        let moved = with(&|d| {
            if let Draft::CreateDistance { to, .. } = d {
                *to = input.point;
            }
        });
        if phase == Phase::Move {
            return with_draft(m, moved);
        }
        if js::hypot(input.point.x - from.x, input.point.y - from.y) < MIN_DRAG {
            return changed(Model { draft: None, ..m.clone() });
        }
        let offset = with(&|d| {
            if let Draft::CreateDistance { to, step, .. } = d {
                *to = input.point;
                *step = DistanceStep::Offset;
            }
        });
        return with_draft(m, offset);
    }
    if phase == Phase::Up {
        return same();
    }
    let Draft::CreateDistance { preset, flags, .. } = draft else { unreachable!() };
    let defaults = defaults_for(m, preset);
    let geom = Geom::Line { a: *from, b: *to, ends: Some(defaults.line_endings), rot: None };
    let appearance = draft_measure.with_leader_length(distance_leader_length(&geom, input.point));
    if phase == Phase::Move {
        let next = with(&|d| {
            if let Draft::CreateDistance { measure, .. } = d {
                *measure = appearance.clone();
            }
        });
        return with_draft(m, next);
    }
    let id = tmp_id(m.seq + 1.0);
    let mut style = style_from_props(&defaults);
    style.interior_color = Some(defaults.interior_color.clone().unwrap_or(defaults.color.clone()));
    let mut annot = fresh_annot(id.clone(), *page, "line".into(), geom, style);
    annot.measure = Some(MeasurementAppearance::Distance(appearance));
    annot.flags = drawn_flags(flags.as_ref());
    let mut order = m.order.clone();
    order.push(id.clone());
    let next = Model {
        seq: m.seq + 1.0,
        by_id: with_annot(m, &id, annot),
        order,
        selected: vec![id.clone()],
        draft: None,
        ..m.clone()
    };
    (Some(next), vec![Effect::Create { id }])
}

#[allow(clippy::too_many_arguments)]
fn create_pointer(
    m: &Model,
    phase: Phase,
    subtype: &str,
    input: &PointerInput,
    preset: String,
    intent: Option<Intent>,
    defer_ink_commit: bool,
    straighten_ink: Option<&InkStraightenOptions>,
    click_create: Option<ClickCreateOption>,
    flags: Option<&AnnotationFlagsPatch>,
    measure: Option<&MeasurementAppearance>,
    capture: Option<&str>,
) -> Step {
    if phase != Phase::Down
        && let Some(page) = m.draft.as_ref().and_then(Draft::page)
        && page.pon() != input.page.pon()
    {
        return same();
    }
    let mut input = input.clone();
    if input.page_box.is_some() {
        input.point = clamp_point_to_box(input.point, input.page_box);
    }
    let input = &input;
    let distance = measure.and_then(MeasurementAppearance::as_distance);
    if matches!(m.draft, Some(Draft::CreateDistance { .. }))
        || (distance.is_some() && capture.is_none_or(str::is_empty))
    {
        return distance_pointer(m, phase, input, preset, distance, flags);
    }
    if subtype == "free-text-callout" {
        return callout_pointer(m, phase, input, preset, flags);
    }
    match phase {
        Phase::Down => {
            if is_poly_subtype(subtype) {
                if input.finish == Some(true) {
                    return finish_poly_create(m);
                }
                if let Some(Draft::CreatePoly { subtype: s, preset: p, page, .. }) = &m.draft
                    && s == subtype
                    && p.as_deref() == Some(preset.as_str())
                    && page.pon() == input.page.pon()
                {
                    let mut next = m.draft.clone().expect("a poly draft");
                    if let Draft::CreatePoly { points, cur, .. } = &mut next {
                        points.push(input.point);
                        *cur = input.point;
                    }
                    return with_draft(m, next);
                }
                let draft = Draft::CreatePoly {
                    measure: measure.and_then(MeasurementAppearance::as_shape).cloned(),
                    subtype: subtype.into(),
                    preset: Some(preset),
                    page: input.page,
                    points: vec![input.point],
                    cur: input.point,
                    closed: subtype == "polygon",
                    flags: flags.copied(),
                };
                return changed(Model { selected: Vec::new(), draft: Some(draft), ..m.clone() });
            }
            let draft = match subtype {
                "line" => Some(Draft::CreateLine {
                    measure: measure.cloned(),
                    capture: capture.map(str::to_string),
                    subtype: subtype.into(),
                    preset: Some(preset),
                    page: input.page,
                    from: input.point,
                    to: input.point,
                    click_create,
                    flags: flags.copied(),
                }),
                "ink" => match m.draft.clone() {
                    Some(Draft::CreateInk {
                        subtype: s,
                        preset: p,
                        page,
                        mut strokes,
                        intent: i,
                        flags: f,
                    }) if s == subtype
                        && p.as_deref() == Some(preset.as_str())
                        && page.pon() == input.page.pon() =>
                    {
                        strokes.push(vec![input.point]);
                        Some(Draft::CreateInk {
                            subtype: s,
                            preset: p,
                            page,
                            strokes,
                            intent: i,
                            flags: f,
                        })
                    }
                    _ => Some(Draft::CreateInk {
                        subtype: subtype.into(),
                        preset: Some(preset),
                        page: input.page,
                        strokes: vec![vec![input.point]],
                        intent,
                        flags: flags.copied(),
                    }),
                },
                "square" | "circle" | "free-text" | "redact" | "link" => {
                    let upright = input.upright == Some(true)
                        && input.display_rotation.is_some_and(|r| r != 0.0 && !r.is_nan());
                    Some(Draft::CreateRect {
                        subtype: subtype.into(),
                        preset: Some(preset),
                        page: input.page,
                        from: input.point,
                        to: input.point,
                        ellipse: subtype == "circle",
                        display_rotation: if upright { input.display_rotation } else { None },
                        upright: upright.then_some(true),
                        click_create,
                        flags: flags.copied(),
                    })
                }
                _ => None,
            };
            match draft {
                Some(draft) => {
                    changed(Model { selected: Vec::new(), draft: Some(draft), ..m.clone() })
                }
                None => same(),
            }
        }
        Phase::Move => match m.draft.clone() {
            Some(mut d @ Draft::CreatePoly { .. }) => {
                if let Draft::CreatePoly { cur, .. } = &mut d {
                    *cur = input.point;
                }
                with_draft(m, d)
            }
            Some(mut d @ (Draft::CreateRect { .. } | Draft::CreateLine { .. })) => {
                if let Draft::CreateRect { to, .. } | Draft::CreateLine { to, .. } = &mut d {
                    *to = input.point;
                }
                with_draft(m, d)
            }
            Some(mut d @ Draft::CreateInk { .. }) => {
                if let Draft::CreateInk { strokes, .. } = &mut d
                    && let Some(last) = strokes.last_mut()
                {
                    last.push(input.point);
                }
                with_draft(m, d)
            }
            _ => same(),
        },
        Phase::Up => create_up(m, input, defer_ink_commit, straighten_ink),
    }
}

fn create_up(
    m: &Model,
    input: &PointerInput,
    defer_ink_commit: bool,
    straighten_ink: Option<&InkStraightenOptions>,
) -> Step {
    let Some(d) = &m.draft else {
        return same();
    };
    if let Draft::CreateInk { subtype, preset, page, strokes, intent, flags } = d {
        let Some(opts) = straighten_ink.filter(|_| !strokes.is_empty()) else {
            return if defer_ink_commit { same() } else { finish_ink_create(m) };
        };
        let mut strokes = strokes.clone();
        let last = strokes.len() - 1;
        strokes[last] = straighten_ink_stroke(&strokes[last], opts);
        let next = Model {
            draft: Some(Draft::CreateInk {
                subtype: subtype.clone(),
                preset: preset.clone(),
                page: *page,
                strokes,
                intent: *intent,
                flags: *flags,
            }),
            ..m.clone()
        };
        if defer_ink_commit {
            return changed(next);
        }
        // `finishInkCreate(next)` builds a new model either way.
        let (model, fx) = finish_ink_create(&next);
        return (Some(model.unwrap_or(next)), fx);
    }
    let (subtype, preset, page, from, to, click_create, flags, rect_extra) = match d {
        Draft::CreateRect {
            subtype,
            preset,
            page,
            from,
            to,
            ellipse,
            display_rotation,
            upright,
            click_create,
            flags,
        } => (
            subtype,
            preset,
            *page,
            *from,
            *to,
            *click_create,
            *flags,
            Some((*ellipse, *display_rotation, *upright)),
        ),
        Draft::CreateLine { subtype, preset, page, from, to, click_create, flags, .. } => {
            (subtype, preset, *page, *from, *to, *click_create, *flags, None)
        }
        _ => return same(),
    };
    let def = defaults_for(m, preset.as_deref().unwrap_or(subtype));
    let style = style_from_props(&def);
    let up_rot = match rect_extra {
        Some((_, Some(dr), Some(true))) if dr != 0.0 && !dr.is_nan() => upright_rotation(dr),
        _ => 0.0,
    };
    let upright_box = |dragged: Rect| {
        if up_rot == 90.0 || up_rot == 270.0 { transposed_about_center(dragged) } else { dragged }
    };
    let click_geom = |policy: &ClickCreate| {
        let opts = PlacementOptions {
            page_box: input.page_box,
            upright: rect_extra.and_then(|(_, _, u)| u),
            display_rotation: rect_extra.and_then(|(_, dr, _)| dr),
        };
        click_create_geom(subtype, &resolve_click_placement(from, policy, &opts), &def)
    };
    let rot_of = (up_rot != 0.0 && !up_rot.is_nan()).then_some(up_rot);
    let mut geom: Option<Geom> = None;
    match rect_extra {
        Some(_) if subtype == "free-text" => {
            let dragged = rect_from_points(from, to);
            let is_click = dragged.width < MIN_DRAG && dragged.height < MIN_DRAG;
            if !is_click {
                geom = Some(Geom::Text { rect: upright_box(dragged), callout: None, rot: rot_of });
            } else if click_create != Some(ClickCreateOption::Off) {
                let policy =
                    ClickCreateOption::box_policy(click_create.as_ref()).unwrap_or_else(|| {
                        ClickCreate::boxed(180.0, 40.0, Some(crate::types::BoxAnchor::TopLeft))
                    });
                geom = click_geom(&policy);
            }
        }
        Some((ellipse, _, _)) => {
            let dragged = rect_from_points(from, to);
            if dragged.width >= MIN_DRAG || dragged.height >= MIN_DRAG {
                geom = Some(Geom::Rect {
                    rect: shape_rect_for(upright_box(dragged), ellipse, &style),
                    ellipse,
                    rot: rot_of,
                });
            } else if let Some(policy) = ClickCreateOption::box_policy(click_create.as_ref()) {
                geom = click_geom(&policy);
            }
        }
        None => {
            if js::hypot(to.x - from.x, to.y - from.y) >= MIN_DRAG {
                geom = Some(Geom::Line { a: from, b: to, ends: Some(def.line_endings), rot: None });
            } else if let Some(policy) = ClickCreateOption::segment_policy(click_create.as_ref()) {
                geom = click_geom(&policy);
            }
        }
    }
    let Some(geom) = geom else {
        return changed(Model { draft: None, ..m.clone() });
    };
    let (line_measure, capture) = match d {
        Draft::CreateLine { measure, capture, .. } => (measure.clone(), capture.clone()),
        _ => (None, None),
    };
    if let Some(tool) = capture.filter(|c| !c.is_empty()) {
        return (
            Some(Model { draft: None, ..m.clone() }),
            vec![Effect::Captured { tool, page, geom }],
        );
    }
    let id = tmp_id(m.seq + 1.0);
    let is_text = matches!(geom, Geom::Text { .. });
    let mut annot = fresh_annot(id.clone(), page, subtype.clone(), geom, style);
    annot.measure = line_measure;
    if is_text {
        annot.text = Some(text_style_from_props(&def));
    }
    if subtype == "link" {
        annot.link = Nullable::from_option(def.link.as_option().cloned());
    }
    annot.flags = drawn_flags(flags.as_ref());
    let editing = if is_text { Some(id) } else { m.editing.clone() };
    commit_created(m, annot, editing, false)
}

/// Commit all strokes of an ink draft.
fn finish_ink_create(m: &Model) -> Step {
    let Some(Draft::CreateInk { subtype, preset, page, strokes, intent, flags }) = &m.draft else {
        return same();
    };
    let points: Vec<Point> = strokes.concat();
    if !strokes.iter().any(|s| s.len() >= 2) || points.is_empty() {
        return changed(Model { draft: None, ..m.clone() });
    }
    let bounds = union_rect(&points);
    if js::max(bounds.width, bounds.height) < MIN_DRAG {
        return changed(Model { draft: None, ..m.clone() });
    }
    let id = tmp_id(m.seq + 1.0);
    let style = style_from_props(&defaults_for(m, preset.as_deref().unwrap_or(subtype)));
    let mut annot = fresh_annot(
        id,
        *page,
        subtype.clone(),
        Geom::Ink { strokes: strokes.clone(), rot: None },
        style,
    );
    annot.intent = *intent;
    annot.flags = drawn_flags(flags.as_ref());
    commit_created(m, annot, m.editing.clone(), false)
}

const CALLOUT_BOX_WIDTH: f64 = 150.0;
const CALLOUT_BOX_HEIGHT: f64 = 40.0;

/// The fields of a callout creation draft, for [`callout_box`].
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalloutDraft {
    pub cur: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_from: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_to: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upright: Option<bool>,
    /// The home page's box, captured at the tip click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_box: Option<Rect>,
}

impl CalloutDraft {
    pub fn of(d: &Draft) -> Option<CalloutDraft> {
        match d {
            Draft::CreateCallout {
                cur,
                box_from,
                box_to,
                display_rotation,
                upright,
                page_box,
                ..
            } => Some(CalloutDraft {
                cur: *cur,
                box_from: *box_from,
                box_to: *box_to,
                display_rotation: *display_rotation,
                upright: *upright,
                page_box: *page_box,
            }),
            _ => None,
        }
    }
}

/// The callout draft's upright counter-rotation (0 for none).
pub fn callout_upright_rot(d: &CalloutDraft) -> f64 {
    match (d.upright, d.display_rotation) {
        (Some(true), Some(r)) if r != 0.0 && !r.is_nan() => upright_rotation(r),
        _ => 0.0,
    }
}

/// The text-box rect for a callout's box step: a drag past [`MIN_DRAG`]
/// sizes it; otherwise the default box hangs at the press point, or at the
/// cursor before the press.
pub fn callout_box(d: &CalloutDraft) -> Rect {
    let rot = callout_upright_rot(d);
    let quarter = rot == 90.0 || rot == 270.0;
    let default_box = |at: Point| {
        let placed = if rot != 0.0 && !rot.is_nan() {
            upright_anchored_rect(
                at,
                CALLOUT_BOX_WIDTH,
                CALLOUT_BOX_HEIGHT,
                d.display_rotation.unwrap_or(f64::NAN),
            )
        } else {
            Rect { x: at.x, y: at.y, width: CALLOUT_BOX_WIDTH, height: CALLOUT_BOX_HEIGHT }
        };
        slide_callout_footprint(placed, rot, d.page_box)
    };
    if let Some(from) = d.box_from {
        let dragged = d.box_to.map(|to| rect_from_points(from, to));
        if let Some(dragged) = dragged
            && (dragged.width >= MIN_DRAG || dragged.height >= MIN_DRAG)
        {
            return if quarter { transposed_about_center(dragged) } else { dragged };
        }
        return default_box(from);
    }
    default_box(d.cur)
}

/// Shift `rect` so its displayed footprint (the box turned about its centre)
/// sits inside `page`.
fn slide_callout_footprint(rect: Rect, rot: f64, page: Option<Rect>) -> Rect {
    let Some(page) = page else {
        return rect;
    };
    let foot = rotated_aabb(rect, rot);
    let placed = clamp_rect_to_box(foot, Some(page));
    let dx = placed.x - foot.x;
    let dy = placed.y - foot.y;
    if dx == 0.0 && dy == 0.0 {
        return rect;
    }
    Rect { x: rect.x + dx, y: rect.y + dy, ..rect }
}

fn callout_pointer(
    m: &Model,
    phase: Phase,
    input: &PointerInput,
    preset: String,
    flags: Option<&AnnotationFlagsPatch>,
) -> Step {
    let d = m.draft.as_ref();
    let callout = match d {
        Some(Draft::CreateCallout { .. }) => d,
        _ => None,
    };
    match phase {
        Phase::Down => {
            let restart = match callout {
                Some(Draft::CreateCallout { page, .. }) => page.pon() != input.page.pon(),
                _ => true,
            };
            if restart {
                let upright = input.upright == Some(true)
                    && input.display_rotation.is_some_and(|r| r != 0.0 && !r.is_nan());
                let draft = Draft::CreateCallout {
                    subtype: "free-text-callout".into(),
                    preset: Some(preset),
                    page: input.page,
                    step: CalloutStep::Knee,
                    tip: input.point,
                    knee: None,
                    cur: input.point,
                    box_from: None,
                    box_to: None,
                    page_box: input.page_box,
                    display_rotation: if upright { input.display_rotation } else { None },
                    upright: upright.then_some(true),
                    flags: flags.copied(),
                };
                return changed(Model { selected: Vec::new(), draft: Some(draft), ..m.clone() });
            }
            let Some(Draft::CreateCallout { step, .. }) = callout else { unreachable!() };
            let mut next = callout.cloned().expect("a callout draft");
            if let Draft::CreateCallout { knee, step: s, cur, box_from, box_to, .. } = &mut next {
                if *step == CalloutStep::Knee {
                    *knee = Some(input.point);
                    *s = CalloutStep::Box;
                    *cur = input.point;
                } else {
                    *box_from = Some(input.point);
                    *box_to = Some(input.point);
                }
            }
            with_draft(m, next)
        }
        Phase::Move => {
            let Some(draft) = callout else {
                return same();
            };
            let mut next = draft.clone();
            if let Draft::CreateCallout { step, box_from, box_to, cur, .. } = &mut next {
                if *step == CalloutStep::Box && box_from.is_some() {
                    *box_to = Some(input.point);
                } else {
                    *cur = input.point;
                }
            }
            with_draft(m, next)
        }
        Phase::Up => {
            let Some(Draft::CreateCallout {
                step: CalloutStep::Box,
                box_from: Some(_),
                page,
                tip,
                knee,
                preset,
                flags,
                ..
            }) = callout
            else {
                return same();
            };
            let cd = CalloutDraft::of(callout.expect("a callout draft")).expect("a callout draft");
            let rect = callout_box(&cd);
            let rot = callout_upright_rot(&cd);
            let def = defaults_for(m, preset.as_deref().unwrap_or("free-text-callout"));
            let ending = if def.line_endings.end != crate::types::LineEnding::None {
                def.line_endings.end
            } else {
                crate::types::LineEnding::OpenArrow
            };
            let id = tmp_id(m.seq + 1.0);
            let geom = Geom::Text {
                rect,
                callout: Some(crate::types::Callout { tip: *tip, knee: *knee, ending }),
                rot: (rot != 0.0 && !rot.is_nan()).then_some(rot),
            };
            let mut annot =
                fresh_annot(id.clone(), *page, "free-text".into(), geom, style_from_props(&def));
            annot.text = Some(text_style_from_props(&def));
            annot.flags = drawn_flags(flags.as_ref());
            commit_created(m, annot, Some(id), false)
        }
    }
}

fn finish_poly_create(m: &Model) -> Step {
    let Some(Draft::CreatePoly { subtype, preset, page, points, closed, flags, measure, .. }) =
        &m.draft
    else {
        return same();
    };
    let min_points = if *closed { 3 } else { 2 };
    if points.len() < min_points {
        return changed(Model { draft: None, ..m.clone() });
    }
    let def = defaults_for(m, preset.as_deref().unwrap_or(subtype));
    let geom = Geom::Poly {
        points: points.clone(),
        closed: *closed,
        ends: if *closed { None } else { Some(def.line_endings) },
        rot: None,
    };
    if let Some(me) = measure
        && shape_measurement_readout(&geom, me).unavailable().is_some()
    {
        return same();
    }
    let id = tmp_id(m.seq + 1.0);
    let mut annot = fresh_annot(id, *page, subtype.clone(), geom, style_from_props(&def));
    annot.measure = measure.clone().map(MeasurementAppearance::Shape);
    annot.flags = drawn_flags(flags.as_ref());
    commit_created(m, annot, m.editing.clone(), false)
}

/// Drop degenerate quads (zero cross product of the two edge vectors).
fn usable_quads(quads: &[TextQuad]) -> Vec<TextQuad> {
    quads
        .iter()
        .filter(|q| {
            let ux = q.upper_end.x - q.upper_start.x;
            let uy = q.upper_end.y - q.upper_start.y;
            let sx = q.lower_start.x - q.upper_start.x;
            let sy = q.lower_start.y - q.upper_start.y;
            (ux * sy - uy * sx).abs() > 0.0
        })
        .copied()
        .collect()
}

fn create_markup(
    m: &Model,
    subtype: &str,
    page: PageRef,
    segment_quads: &[TextQuad],
    preset: &str,
    flags: Option<&AnnotationFlagsPatch>,
) -> Step {
    let quads = usable_quads(segment_quads);
    if quads.is_empty() {
        return same();
    }
    let id = tmp_id(m.seq + 1.0);
    let mut annot = fresh_annot(
        id,
        page,
        subtype.into(),
        Geom::Quads { quads },
        style_from_props(&defaults_for(m, preset)),
    );
    annot.flags = drawn_flags(flags);
    commit_created(m, annot, m.editing.clone(), true)
}

fn create_replace_text(
    m: &Model,
    page: PageRef,
    segment_quads: &[TextQuad],
    anchor: &TextEndAnchor,
    preset: &str,
) -> Step {
    let quads = usable_quads(segment_quads);
    if quads.is_empty() {
        return same();
    }
    let primary_id = tmp_id(m.seq + 1.0);
    let strikeout_id = tmp_id(m.seq + 2.0);
    let style = style_from_props(&defaults_for(m, preset));
    let mut caret = fresh_annot(
        primary_id.clone(),
        page,
        "caret".into(),
        caret_geom_from_anchor(anchor),
        style.clone(),
    );
    caret.intent = Some(Intent::Replace);
    let mut strikeout =
        fresh_annot(strikeout_id.clone(), page, "strikeout".into(), Geom::Quads { quads }, style);
    strikeout.intent = Some(Intent::StrikeoutTextEdit);
    strikeout.irt = Some(primary_id.clone());
    strikeout.group = Some(primary_id.clone());
    let mut by_id = m.by_id.clone();
    by_id.insert(primary_id.clone(), caret);
    by_id.insert(strikeout_id.clone(), strikeout);
    let mut order = m.order.clone();
    order.push(primary_id.clone());
    order.push(strikeout_id.clone());
    let next = Model {
        seq: m.seq + 2.0,
        by_id,
        order,
        selected: vec![primary_id.clone(), strikeout_id.clone()],
        draft: None,
        preview: None,
        ..m.clone()
    };
    (Some(next), vec![Effect::CreateGroup { primary: primary_id, members: vec![strikeout_id] }])
}

fn create_caret(
    m: &Model,
    page: PageRef,
    anchor: &TextEndAnchor,
    flags: Option<&AnnotationFlagsPatch>,
) -> Step {
    let caret_geom = caret_geom_from_anchor(anchor);
    if let Some(r) = caret_geom.rect()
        && (r.width <= 0.0 || r.height <= 0.0)
    {
        return same();
    }
    let id = tmp_id(m.seq + 1.0);
    let mut annot = fresh_annot(
        id,
        page,
        "caret".into(),
        caret_geom,
        style_from_props(&defaults_for(m, "caret")),
    );
    annot.flags = drawn_flags(flags);
    commit_created(m, annot, m.editing.clone(), true)
}

fn set_markup_preview(
    m: &Model,
    subtype: &str,
    quads_by_page: &Record<Vec<TextQuad>>,
    preset: &str,
) -> Step {
    let mut by_page: BTreeMap<String, Vec<TextQuad>> = BTreeMap::new();
    for (k, quads) in quads_by_page.iter() {
        let quads = usable_quads(quads);
        if !quads.is_empty() {
            by_page.insert(js::number_to_string(js::string_to_number(k)), quads);
        }
    }
    let preview = MarkupPreview { subtype: subtype.into(), preset: preset.into(), by_page };
    changed(Model { preview: Some(preview), ..m.clone() })
}

/// Apply a flat property patch to the selection: each member takes the
/// keys its kind declares; changed members flip to vector (opaque-body kinds
/// excepted) and emit one `props` patch each; a link write on a non-link
/// kind rides `syncLink`.
fn set_props(m: &Model, patch: &AnnotationPropsPatch) -> Step {
    if m.selected.is_empty() {
        return same();
    }
    let mut by_id = m.by_id.clone();
    let mut fx = Vec::new();
    let keys = patch.keys();
    for id in &m.selected {
        let Some(a) = by_id.get(id).cloned() else { continue };
        let link_intent = !patch.link.is_undefined()
            && a.subtype != "link"
            && kind_takes_link(&a.subtype)
            && annot_transformable(&a);
        let target = || patch.link.as_option().cloned();
        let Some((next, what)) = apply_props_tracked(&a, patch) else {
            if link_intent {
                fx.push(Effect::SyncLink { id: id.clone(), target: target() });
            }
            continue;
        };
        let other_changed = what.style || what.geom || what.text || what.icon;
        let stored =
            if caps_for(&a.subtype).opaque_body || !other_changed { next } else { to_vector(next) };
        by_id.insert(id.clone(), stored);
        if other_changed || (what.link && a.subtype == "link") {
            fx.push(Effect::Patch {
                id: id.clone(),
                scope: PatchScope::Props { keys: keys.clone() },
                ap_changed: false,
            });
        }
        if link_intent {
            fx.push(Effect::SyncLink { id: id.clone(), target: target() });
        }
    }
    if fx.is_empty() { same() } else { (Some(Model { by_id, ..m.clone() }), fx) }
}

/// Merge a `/F` patch into the selection (or `ids`); not gated by `locked`.
fn set_flags(m: &Model, patch: &AnnotationFlagsPatch, ids: Option<&[Id]>) -> Step {
    let targets = ids.unwrap_or(&m.selected);
    if targets.is_empty() {
        return same();
    }
    let mut fx = Vec::new();
    let mut by_id: Option<BTreeMap<Id, Annot>> = None;
    for id in targets {
        let Some(a) = by_id.as_ref().unwrap_or(&m.by_id).get(id).cloned() else { continue };
        let flags = merge_flags(&a.flags, patch);
        if flags == a.flags {
            continue;
        }
        let map = by_id.get_or_insert_with(|| m.by_id.clone());
        let has_ref = a.ref_.is_some();
        map.insert(id.clone(), Annot { flags, ..a });
        if has_ref {
            fx.push(Effect::Flags { id: id.clone() });
        }
    }
    match by_id {
        Some(by_id) => (Some(Model { by_id, ..m.clone() }), fx),
        None => same(),
    }
}

fn set_defaults(m: &Model, subtype: &str, patch: &AnnotationPropsPatch) -> Step {
    let prev = m.defaults.get(subtype).cloned().unwrap_or_default();
    let mut next = prev.spread(patch);
    if let Some(le) = &patch.line_endings {
        next.line_endings = Some(prev.line_endings.unwrap_or_default().merge(le));
    }
    let mut defaults = m.defaults.clone();
    defaults.insert(subtype.into(), next);
    changed(Model { defaults, ..m.clone() })
}

/// Rotate the selection by a fixed angle about its centre (a single shape's
/// selection centre, a group's union-box centre).
fn rotate_selection(m: &Model, delta_deg: f64) -> Step {
    let ids: Vec<&Id> = m
        .selected
        .iter()
        .filter(|id| {
            m.by_id
                .get(*id)
                .is_some_and(|a| annot_transformable(a) && caps_for(&a.subtype).rotatable)
        })
        .collect();
    if ids.is_empty() {
        return same();
    }
    let pivot = if ids.len() == 1 {
        annotation_selection_frame(&m.by_id[ids[0]], None).center
    } else {
        let page = m.by_id[ids[0]].page;
        let scoped = Model { selected: ids.iter().map(|id| (*id).clone()).collect(), ..m.clone() };
        let Some(union) = group_union_bounds(&scoped, page, None) else {
            return same();
        };
        Point { x: union.x + union.width / 2.0, y: union.y + union.height / 2.0 }
    };
    let mut by_id = m.by_id.clone();
    let mut fx = Vec::new();
    for id in ids {
        let a = by_id[id].clone();
        let next = own_geometry(Annot {
            geom: geom_rotate_about(&a.geom, pivot, delta_deg),
            measure: transform_measurement_caption(a.measure.as_ref(), |p| {
                rotate_point(p, pivot, delta_deg)
            }),
            ..a.clone()
        });
        fx.push(patch_fx(id, &next, &a.geom));
        by_id.insert(id.clone(), next);
    }
    (Some(Model { by_id, ..m.clone() }), fx)
}

fn reset_rotation(m: &Model) -> Step {
    let mut by_id = m.by_id.clone();
    let mut fx = Vec::new();
    for id in &m.selected {
        let Some(a) = by_id.get(id).cloned() else { continue };
        if !annot_transformable(&a) || geom_rotation(&a.geom) == 0.0 {
            continue;
        }
        let center = annotation_selection_frame(&a, None).center;
        let rot = geom_rotation(&a.geom);
        let next = own_geometry(Annot {
            geom: geom_reset_rotation(&a.geom, Some(center)),
            measure: transform_measurement_caption(a.measure.as_ref(), |p| {
                rotate_point(p, center, -rot)
            }),
            ..a.clone()
        });
        fx.push(patch_fx(id, &next, &a.geom));
        by_id.insert(id.clone(), next);
    }
    if fx.is_empty() { same() } else { (Some(Model { by_id, ..m.clone() }), fx) }
}

fn delete_selection(m: &Model) -> Step {
    let deletable: Vec<Id> = m
        .selected
        .iter()
        .filter(|id| m.by_id.get(*id).is_some_and(annot_deletable))
        .cloned()
        .collect();
    if deletable.is_empty() {
        return same();
    }
    let mut with_children = deletable.clone();
    for id in &deletable {
        with_children.extend(link_children_of(m, id).into_iter().map(|c| c.id.clone()));
    }
    let fx = with_children
        .iter()
        .filter_map(|id| m.by_id.get(id)?.ref_.clone())
        .map(|ref_| Effect::Delete { ref_ })
        .collect();
    (Some(remove_annots(m, &with_children)), fx)
}

/// Marquee helper: the selectable page annotations whose oriented selection
/// quad meets the box.
pub fn annots_in_box(
    m: &Model,
    page: PageRef,
    a: Point,
    b: Point,
    inert: Option<&IdSet>,
    view: Option<ViewEnv>,
) -> Vec<Id> {
    let bx = rect_from_points(a, b);
    m.order
        .iter()
        .filter(|id| {
            let Some(annot) = m.by_id.get(*id) else { return false };
            if annot.page.pon() != page.pon()
                || inert.is_some_and(|s| s.has(id))
                || !is_selectable(m, id)
            {
                return false;
            }
            if is_substrate_only(annot) {
                return false;
            }
            quad_intersects_rect(&annotation_selection_frame(annot, view).corners, bx)
        })
        .cloned()
        .collect()
}

fn merge_loaded(m: &Model, annots: &[Annot]) -> Model {
    let mut by_id = m.by_id.clone();
    let mut order = m.order.clone();
    for a in annots {
        if by_id.contains_key(&a.id) {
            continue;
        }
        by_id.insert(a.id.clone(), a.clone());
        order.push(a.id.clone());
    }
    Model { by_id, order, ..m.clone() }
}

fn hydrate_annots(m: &Model, annots: &[Annot], bump: bool) -> Model {
    let incoming: Vec<&str> = annots.iter().map(|a| a.id.as_str()).collect();
    let locked = draft_ids(m.draft.as_ref());
    let reaped: Vec<&Id> = m
        .order
        .iter()
        .filter(|id| {
            !incoming.contains(&id.as_str())
                && !locked.contains(id)
                && m.by_id.get(*id).is_some_and(|a| a.ref_.is_some())
        })
        .collect();
    let mut next = m.clone();
    if !reaped.is_empty() {
        let gone = |id: &Id| reaped.contains(&id);
        let mut by_id = m.by_id.clone();
        for id in &reaped {
            by_id.remove(*id);
        }
        let clear = |x: &Option<Id>| {
            x.as_ref().filter(|h| !h.is_empty() && gone(h)).map_or_else(|| x.clone(), |_| None)
        };
        next = Model {
            by_id,
            order: m.order.iter().filter(|id| !gone(id)).cloned().collect(),
            selected: m.selected.iter().filter(|id| !gone(id)).cloned().collect(),
            hovered: clear(&m.hovered),
            editing: clear(&m.editing),
            ..m.clone()
        };
    }
    upsert_annots(&next, annots, bump)
}

fn upsert_annots(m: &Model, annots: &[Annot], bump: bool) -> Model {
    let dragging = draft_ids(m.draft.as_ref());
    let mut by_id = m.by_id.clone();
    let mut order = m.order.clone();
    for a in annots {
        if dragging.contains(&a.id) {
            continue;
        }
        let prev = by_id.get(&a.id).and_then(|p| p.ap_version);
        if !by_id.contains_key(&a.id) {
            order.push(a.id.clone());
        }
        let ap_version = prev.unwrap_or(0.0) + if bump { 1.0 } else { 0.0 };
        by_id.insert(a.id.clone(), Annot { ap_version: Some(ap_version), ..a.clone() });
    }
    Model { by_id, order, ..m.clone() }
}

fn bump_ap(m: &Model, ids: &[Id]) -> Option<Model> {
    let mut by_id: Option<BTreeMap<Id, Annot>> = None;
    for id in ids {
        let Some(a) = m.by_id.get(id) else { continue };
        let map = by_id.get_or_insert_with(|| m.by_id.clone());
        // Read the model's original entry, as the TypeScript does: a repeated
        // id bumps once.
        map.insert(
            id.clone(),
            Annot { ap_version: Some(a.ap_version.unwrap_or(0.0) + 1.0), ..a.clone() },
        );
    }
    by_id.map(|by_id| Model { by_id, ..m.clone() })
}

fn draft_ids(draft: Option<&Draft>) -> Vec<Id> {
    match draft {
        Some(Draft::Move { ids, .. } | Draft::Rotate { ids, .. } | Draft::Group { ids, .. }) => {
            ids.clone()
        }
        Some(Draft::Handle { id, .. } | Draft::Caption { id, .. } | Draft::Leader { id, .. }) => {
            vec![id.clone()]
        }
        _ => Vec::new(),
    }
}

fn remove_annots(m: &Model, ids: &[Id]) -> Model {
    let mut by_id = m.by_id.clone();
    for id in ids {
        by_id.remove(id);
    }
    let editing = match &m.editing {
        Some(e) if !e.is_empty() && ids.contains(e) => None,
        other => other.clone(),
    };
    Model {
        by_id,
        order: m.order.iter().filter(|id| !ids.contains(id)).cloned().collect(),
        selected: m.selected.iter().filter(|id| !ids.contains(id)).cloned().collect(),
        draft: None,
        editing,
        ..m.clone()
    }
}

fn reconcile(m: &Model, temp_id: &str, id: &str, ref_: &AnnotationRef) -> Option<Model> {
    let a = m.by_id.get(temp_id)?;
    let mut by_id = m.by_id.clone();
    by_id.remove(temp_id);
    by_id.insert(
        id.to_string(),
        Annot { id: id.to_string(), ref_: Some(ref_.clone()), ..a.clone() },
    );
    for other in by_id.values_mut() {
        if other.irt.as_deref() == Some(temp_id) {
            other.irt = Some(id.to_string());
        }
        if other.group.as_deref() == Some(temp_id) {
            other.group = Some(id.to_string());
        }
    }
    let swap = |x: &Id| if x == temp_id { id.to_string() } else { x.clone() };
    // A gesture that began before the ack keeps working on the durable id.
    let mut draft = m.draft.clone();
    match &mut draft {
        Some(Draft::Move { ids, .. } | Draft::Rotate { ids, .. } | Draft::Group { ids, .. }) => {
            for x in ids.iter_mut() {
                *x = swap(x);
            }
        }
        Some(
            Draft::Handle { id: x, .. }
            | Draft::Caption { id: x, .. }
            | Draft::Leader { id: x, .. },
        ) => *x = swap(x),
        _ => {}
    }
    Some(Model {
        by_id,
        draft,
        order: m.order.iter().map(swap).collect(),
        selected: m.selected.iter().map(swap).collect(),
        editing: if m.editing.as_deref() == Some(temp_id) {
            Some(id.to_string())
        } else {
            m.editing.clone()
        },
        ..m.clone()
    })
}

/// The model `update` left: `next` when it changed, else `m` itself.
pub fn apply(m: &Model, msg: &Msg) -> (Model, Vec<Effect>) {
    let (next, fx) = update(m, msg);
    (next.unwrap_or_else(|| m.clone()), fx)
}

/// The data API's create: page-space geometry in, the same optimistic
/// annotation and `create` effect a draw tool commits out. Defaults come from
/// the preset (the subtype without one), `props` override them, and a line or
/// open poly without endings takes the preset's.
#[allow(clippy::too_many_arguments)]
fn create_annot(
    m: &Model,
    page: PageRef,
    subtype: &str,
    geom: &Geom,
    preset: Option<&str>,
    props: Option<&AnnotationPropsPatch>,
    flags: Option<&AnnotationFlagsPatch>,
    select: bool,
) -> Step {
    let def = defaults_for(m, preset.unwrap_or(subtype));
    let mut geom = geom.clone();
    if let Geom::Line { ends, .. } | Geom::Poly { ends, closed: false, .. } = &mut geom
        && ends.is_none()
    {
        *ends = Some(def.line_endings);
    }
    let id = tmp_id(m.seq + 1.0);
    let is_text = matches!(geom, Geom::Text { .. });
    let mut base = fresh_annot(id.clone(), page, subtype.into(), geom, style_from_props(&def));
    if is_text {
        base.text = Some(text_style_from_props(&def));
    }
    if subtype == "link" {
        base.link = Nullable::from_option(def.link.as_option().cloned());
    }
    base.flags = drawn_flags(flags);
    let annot = match props {
        Some(p) => crate::props::apply_props(&base, p).unwrap_or(base),
        None => base,
    };
    let mut order = m.order.clone();
    order.push(id.clone());
    let next = Model {
        seq: m.seq + 1.0,
        by_id: with_annot(m, &id, annot),
        order,
        selected: if select { vec![id.clone()] } else { m.selected.clone() },
        ..m.clone()
    };
    (Some(next), vec![Effect::Create { id }])
}
