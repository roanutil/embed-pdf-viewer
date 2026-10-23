//! `view.ts`: the pure view selectors. `pageItems` is the per-annotation
//! render list (the live gesture applied), `chrome` the selection overlay.

use serde_json::Value;

use crate::anchor::{anchor_mode_of, anchored_geom, anchored_scale, anchored_stroke_width};
use crate::appearance::{raster_placement, source_during};
use crate::editable::is_transformable_by_hand;
use crate::flags::viewable;
use crate::frame::placed;
use crate::geometry::{
    Knob, ROTATE_KNOB_OFFSET, chord_through, geom_handles, geom_rotate_about, geom_rotation,
    geom_scale_about, geom_translate, geom_visual_bounds, group_resize_factors, place_rotate_knob,
    rect_handles_for,
};
use crate::group::group_caps;
use crate::hit::{is_selectable, paint_order};
use crate::json::{self, Object};
use crate::kinds::blend_for;
use crate::measurement::{
    MeasurementAppearance, distance_handles, distance_layout, measurement_of,
};
use crate::measurement_shape::{
    measurement_layout, move_measurement_caption, shape_measurement_readout,
};
use crate::placement::{GestureForm, is_drag, placed_shape};
use crate::record::{kind_of, ref_of, shape_of, style_of, text_of};
use crate::rect::{MIN_SIZE, cursor_on_screen, rect_from_points, rotate_point, union_rect};
use crate::selection::{Live, SelectionFrame, annotation_selection_frame};
use crate::shapes::text_box::callout_shape;
use crate::types::{
    ChromeNode, CreationDraftAnchor, Draft, GuideLine, GuideNode, HandleNode, Id, InkShape,
    ItemSource, KnobNode, LineEndings, LinePoints, LineShape, Model, ObbNode, OutlineNode, PageRef,
    Point, PolyShape, QuadsShape, Rect, RedactLabel, RenderItem, RotateGuidesNode, RotationAnchor,
    Shape, Style, ViewEnv,
};
use crate::update::draw::{ShapeDraft, draft_placement};
use crate::update::{
    callout_box, callout_upright_rot, line_endings_of, rotate_draft_delta, tool_annotation,
};

pub const DRAFT_ID: &str = "__draft__";
pub const PREVIEW_ID: &str = "__markup_preview__";

fn poly_preview_points(points: &[Point], current: Point) -> Vec<Point> {
    match points.last() {
        Some(last) if current.x != last.x || current.y != last.y => {
            let mut out = points.to_vec();
            out.push(current);
            out
        }
        _ => points.to_vec(),
    }
}

/// A redaction's overlay text: the label its hover preview draws.
fn redaction_label_of(annotation: &Object) -> Option<RedactLabel> {
    if json::get_str(annotation, "subtype") != Some("redact") {
        return None;
    }
    let text = annotation.get("overlayText").filter(|v| json::truthy(Some(v)))?;
    Some(RedactLabel {
        text: text.as_str().unwrap_or("").into(),
        repeat: annotation.get("repeat").filter(|v| !json::undefined(Some(v))).cloned(),
        order: Default::default(),
    })
}

/// `iconOf(annotation)`: a note's or file attachment's icon.
pub fn icon_of(annotation: &Object) -> Option<Value> {
    matches!(json::get_str(annotation, "subtype"), Some("text" | "file-attachment"))
        .then(|| annotation.get("icon").filter(|v| json::truthy(Some(v))).cloned())
        .flatten()
}

fn eff_measure(model: &Model, id: &str) -> Option<MeasurementAppearance> {
    let record = model.record(id)?;
    let measure = measurement_of(&record.annotation)?;
    match model.draft() {
        Some(Draft::Caption(d)) if d.id == id => {
            let geometry = shape_of(&record.annotation);
            Some(
                move_measurement_caption(
                    &geometry,
                    &measure,
                    d.delta,
                    &style_of(&record.annotation),
                )
                .1,
            )
        }
        Some(Draft::Leader(d)) if d.id == id && measure.is_distance() => {
            Some(measure.with_leader_length(measure.leader_length() + d.delta))
        }
        _ => Some(measure),
    }
}

fn eff_geom(model: &Model, id: &str, view: Option<&ViewEnv>) -> Shape {
    let record = model.record(id).expect("a record");
    let geometry =
        anchored_geom(&shape_of(&record.annotation), anchor_mode_of(record).as_ref(), view);
    match model.draft() {
        Some(Draft::Move(d)) if d.ids.iter().any(|i| i == id) => geom_translate(&geometry, d.delta),
        Some(Draft::Handle(d)) if d.id == id => d.current.clone(),
        Some(Draft::Caption(d)) if d.id == id => match measurement_of(&record.annotation) {
            Some(m) => {
                move_measurement_caption(&geometry, &m, d.delta, &style_of(&record.annotation)).0
            }
            None => geometry,
        },
        Some(Draft::Rotate(d)) if d.ids.iter().any(|i| i == id) => {
            geom_rotate_about(&geometry, d.pivot, rotate_draft_delta(model, d).delta)
        }
        Some(Draft::Group(d)) if d.ids.iter().any(|i| i == id) => {
            let (sx, sy) = group_resize_factors(d.base, d.current);
            geom_scale_about(&geometry, d.anchor, sx, sy, Some(MIN_SIZE))
        }
        _ => geometry,
    }
}

fn eff_style(annotation: &Object, view: Option<&ViewEnv>) -> Style {
    let mode = anchor_mode_of(&AnnotationOnly(annotation));
    let style = style_of(annotation);
    match (mode, view) {
        (Some(m), Some(v)) if m.zoom => {
            let mut next = style.clone();
            next.stroke_width = anchored_stroke_width(style.stroke_width, Some(&m), Some(v));
            next
        }
        _ => style,
    }
}

/// A record's annotation alone, as a flag bearer with no authority.
struct AnnotationOnly<'a>(&'a Object);

impl crate::flags::FlagBearer for AnnotationOnly<'_> {
    fn bearer_annotation(&self) -> &Object {
        self.0
    }
    fn bearer_authority(&self) -> Option<&crate::types::Authority> {
        None
    }
}

/// `unmadeItem(id, annotation, shape, source, view, measure)`.
pub fn unmade_item(
    id: &str,
    annotation: &Object,
    shape: &Shape,
    source: ItemSource,
    view: Option<&ViewEnv>,
    measure: Option<&MeasurementAppearance>,
) -> RenderItem {
    let mode = anchor_mode_of(&AnnotationOnly(annotation));
    let geometry = anchored_geom(shape, mode.as_ref(), view);
    let style = eff_style(annotation, view);
    let distance = measure.and_then(|m| measurement_layout(&geometry, m, &style));
    let rot = geom_rotation(&geometry);
    let item = RenderItem {
        id: id.into(),
        reference: Some(Value::Null),
        annotation: None,
        subtype: kind_of(annotation).name,
        box_: distance
            .map_or_else(|| geom_visual_bounds(&geometry, style.stroke()), |d| d.visual_bounds()),
        geometry,
        ap_box: None,
        text: text_of(annotation),
        icon: icon_of(annotation),
        label: redaction_label_of(annotation),
        measure: measure.map(|m| m.0.clone()),
        source,
        picture_version: None,
        lent_scene: Default::default(),
        ghost_opacity: None,
        selected: false,
        hovered: None,
        rot: Some(rot),
        ap_rot: None,
        blend: blend_for(&style),
        style,
        frame: None,
        raster: None,
        order: Default::default(),
    }
    .ordered(&[
        "id", "ref", "subtype", "geometry", "box", "style", "text", "icon", "label", "measure",
        "source", "selected", "rot", "blend",
    ]);
    placed(item, anchored_scale(mode.as_ref(), view))
}

/// `textIsLive(model, id)`.
fn text_is_live(model: &Model, id: &str) -> bool {
    model.editing.as_deref() == Some(id) || source_during(model, id) == crate::types::Source::Vector
}

/// `pageItems(model, page, view)`.
pub fn page_items(model: &Model, page: &PageRef, view: Option<&ViewEnv>) -> Vec<RenderItem> {
    let mut items = Vec::new();
    for id in paint_order(model, page) {
        let record = model.record(&id).expect("a painted record");
        let text = text_of(&record.annotation);
        let geometry = eff_geom(model, &id, view);
        let style = eff_style(&record.annotation, view);
        let ap = raster_placement(model, &id, view, &geometry);
        let measure = eff_measure(model, &id);
        let distance = measure.as_ref().and_then(|m| measurement_layout(&geometry, m, &style));
        let box_ = distance
            .map_or_else(|| geom_visual_bounds(&geometry, style.stroke()), |d| d.visual_bounds());
        let lent = record.painter.0.clone().map(|paint| {
            let made: std::rc::Rc<std::cell::OnceCell<Vec<crate::types::SceneNode>>> =
                Default::default();
            let f: std::rc::Rc<dyn Fn() -> Vec<crate::types::SceneNode>> =
                std::rc::Rc::new(move || made.get_or_init(|| paint(box_)).clone());
            f
        });
        let rot = geom_rotation(&geometry);
        let item = RenderItem {
            id: id.clone(),
            reference: Some(ref_of(Some(record)).unwrap_or(Value::Null)),
            annotation: Some(record.annotation.clone()),
            subtype: kind_of(&record.annotation).name,
            geometry,
            box_,
            ap_box: ap.rect,
            text,
            icon: icon_of(&record.annotation),
            label: redaction_label_of(&record.annotation),
            measure: measure.map(|m| m.0),
            source: source_during(model, &id).into(),
            picture_version: Some(record.picture_version.unwrap_or(0.0)),
            lent_scene: crate::types::LentScene(lent),
            ghost_opacity: None,
            selected: model.is_selected(&id),
            hovered: (model.hovered.as_deref() == Some(id.as_str())).then_some(true),
            rot: Some(rot),
            ap_rot: ap.rot,
            blend: blend_for(&style),
            style,
            frame: None,
            raster: None,
            order: Default::default(),
        }
        .ordered(&[
            "id",
            "ref",
            "annotation",
            "subtype",
            "geometry",
            "box",
            "apBox",
            "style",
            "text",
            "icon",
            "label",
            "measure",
            "source",
            "pictureVersion",
            "lentScene",
            "selected",
            "hovered",
            "rot",
            "apRot",
            "blend",
        ]);
        items.push(placed(item, anchored_scale(anchor_mode_of(record).as_ref(), view)));
    }
    let draft = model.draft().cloned();
    let on_page = |p: &PageRef| p.object_number == page.object_number;
    match &draft {
        Some(Draft::CreateRect(d))
            if on_page(&d.page) && is_drag(GestureForm::Box, d.from, d.to) =>
        {
            let tool = json::spread(
                &tool_annotation(model, &d.subtype, d.preset.as_deref()),
                &d.flags.clone().unwrap_or_default(),
            );
            if let Some(shape) =
                draft_placement(ShapeDraft::Rect(d), None).and_then(|p| placed_shape(&tool, &p))
            {
                items.push(unmade_item(DRAFT_ID, &tool, &shape, ItemSource::Draft, view, None));
            }
        }
        Some(Draft::CreateLine(d))
            if on_page(&d.page) && is_drag(GestureForm::Segment, d.from, d.to) =>
        {
            let tool = json::spread(
                &tool_annotation(model, &d.subtype, d.preset.as_deref()),
                &d.flags.clone().unwrap_or_default(),
            );
            if let Some(shape) =
                draft_placement(ShapeDraft::Line(d), None).and_then(|p| placed_shape(&tool, &p))
            {
                let measure = d.measure.clone().map(MeasurementAppearance);
                items.push(unmade_item(
                    DRAFT_ID,
                    &tool,
                    &shape,
                    ItemSource::Draft,
                    view,
                    measure.as_ref(),
                ));
            }
        }
        _ => {}
    }
    let in_progress = match &draft {
        Some(Draft::CreateDistance(d)) if on_page(&d.page) => {
            let tool = tool_annotation(model, &d.subtype, Some(&d.preset));
            let geometry = Shape::Line(LineShape {
                line_points: LinePoints::new(d.from, d.to),
                line_endings: Some(line_endings_of(&tool)),
                rotation: 0.0,
                order: Default::default(),
            });
            Some((
                d.subtype.clone(),
                tool,
                geometry,
                Some(MeasurementAppearance(d.measure.clone())),
                true,
            ))
        }
        Some(Draft::CreatePoly(d)) if on_page(&d.page) => {
            let tool = tool_annotation(model, &d.subtype, d.preset.as_deref());
            let geometry = Shape::Poly(PolyShape {
                vertices: poly_preview_points(&d.points, d.current),
                closed: d.closed,
                line_endings: if d.closed { None } else { Some(line_endings_of(&tool)) },
                caption_center: None,
                rotation: 0.0,
                order: Default::default(),
            });
            Some((
                d.subtype.clone(),
                tool,
                geometry,
                d.measure.clone().map(MeasurementAppearance),
                false,
            ))
        }
        Some(Draft::CreateInk(d)) if on_page(&d.page) => {
            let tool = tool_annotation(model, &d.subtype, d.preset.as_deref());
            let geometry = Shape::Ink(InkShape {
                ink_list: d.strokes.clone(),
                rotation: 0.0,
                order: Default::default(),
            });
            Some((d.subtype.clone(), tool, geometry, None, false))
        }
        _ => None,
    };
    if let Some((subtype, tool, geometry, measure, distance_draft)) = in_progress {
        let mut style = style_of(&tool);
        if distance_draft && style.interior_color.is_null() {
            style.interior_color =
                crate::object::Nullable::from_option(style.color_str().map(str::to_string));
        }
        let layout = measure.as_ref().and_then(|m| measurement_layout(&geometry, m, &style));
        let box_ = layout
            .map_or_else(|| geom_visual_bounds(&geometry, style.stroke()), |l| l.visual_bounds());
        let item = RenderItem {
            id: DRAFT_ID.into(),
            reference: Some(Value::Null),
            annotation: None,
            subtype,
            geometry,
            box_,
            ap_box: None,
            style,
            text: None,
            icon: None,
            label: None,
            measure: measure.map(|m| m.0),
            source: ItemSource::Draft,
            picture_version: None,
            lent_scene: Default::default(),
            ghost_opacity: None,
            selected: false,
            hovered: None,
            rot: None,
            ap_rot: None,
            blend: None,
            frame: None,
            raster: None,
            order: Default::default(),
        }
        .ordered(&[
            "id", "ref", "measure", "subtype", "geometry", "box", "style", "source", "selected",
        ]);
        items.push(placed(item, 1.0));
    }
    if let Some(Draft::CreateCallout(d)) = &draft
        && on_page(&d.page)
    {
        let tool = tool_annotation(model, &d.subtype, d.preset.as_deref());
        let style = style_of(&tool);
        let tool_ending = if json::get_str(&tool, "subtype") == Some("free-text") {
            json::get_str(&tool, "lineEnding")
        } else {
            None
        };
        let ending = match tool_ending {
            Some(e) if !e.is_empty() && e != "none" => e.to_string(),
            _ => "open-arrow".into(),
        };
        let rot = callout_upright_rot(d);
        let geometry = if d.step == "knee" {
            Shape::Line(LineShape {
                line_points: LinePoints::new(d.tip, d.current),
                line_endings: Some(LineEndings::new(&ending, "none")),
                rotation: 0.0,
                order: Default::default(),
            })
        } else {
            Shape::TextBox(callout_shape(callout_box(d), rot, d.tip, d.knee, &ending))
        };
        let item = RenderItem {
            id: DRAFT_ID.into(),
            reference: Some(Value::Null),
            annotation: None,
            subtype: d.subtype.clone(),
            box_: geom_visual_bounds(&geometry, style.stroke()),
            geometry,
            ap_box: None,
            style,
            text: None,
            icon: None,
            label: None,
            measure: None,
            source: ItemSource::Draft,
            picture_version: None,
            lent_scene: Default::default(),
            ghost_opacity: None,
            selected: false,
            hovered: None,
            rot: None,
            ap_rot: None,
            blend: None,
            frame: None,
            raster: None,
            order: Default::default(),
        }
        .ordered(&["id", "ref", "subtype", "geometry", "box", "style", "source", "selected"]);
        items.push(placed(item, 1.0));
    }
    if let Some(preview) = &model.preview
        && let Some(quads) = preview
            .by_page
            .get(&crate::js::number_to_string(page.object_number))
            .filter(|q| !q.is_empty())
    {
        let geometry =
            Shape::Quads(QuadsShape { quad_points: quads.clone(), order: Default::default() });
        let item = RenderItem {
            id: PREVIEW_ID.into(),
            reference: Some(Value::Null),
            annotation: None,
            subtype: preview.subtype.clone(),
            box_: geom_visual_bounds(&geometry, crate::types::Stroke::width(0.0)),
            geometry,
            ap_box: None,
            style: style_of(&tool_annotation(model, &preview.subtype, Some(&preview.preset))),
            text: None,
            icon: None,
            label: None,
            measure: None,
            source: ItemSource::Draft,
            picture_version: None,
            lent_scene: Default::default(),
            ghost_opacity: None,
            selected: false,
            hovered: None,
            rot: None,
            ap_rot: None,
            blend: None,
            frame: None,
            raster: None,
            order: Default::default(),
        }
        .ordered(&["id", "ref", "subtype", "geometry", "box", "style", "source", "selected"]);
        items.push(placed(item, 1.0));
    }
    items
}

/// One free-text box (`TextBox`).
#[derive(Clone, Debug, PartialEq)]
pub struct TextBox {
    pub id: Id,
    pub rect: Rect,
    pub editing: bool,
    pub rot: f64,
}

impl TextBox {
    pub fn to_value(&self) -> Value {
        use crate::object::Json;
        Value::Object(json::object([
            ("id", Value::String(self.id.clone())),
            ("box", self.rect.json()),
            ("editing", Value::Bool(self.editing)),
            ("rot", json::num(self.rot)),
        ]))
    }
}

/// `textBoxes(model, page, view)`.
pub fn text_boxes(model: &Model, page: &PageRef, view: Option<&ViewEnv>) -> Vec<TextBox> {
    let mut out = Vec::new();
    for id in model.order.iter() {
        let Some(record) = model.record(id) else { continue };
        if record.page_number() != Some(page.object_number)
            || !matches!(shape_of(&record.annotation), Shape::TextBox(_))
        {
            continue;
        }
        if !viewable(&record.annotation, model.is_selected(id)) || !text_is_live(model, id) {
            continue;
        }
        let geometry = eff_geom(model, id, view);
        if let Shape::TextBox(t) = &geometry {
            out.push(TextBox {
                id: id.clone(),
                rect: t.box_,
                editing: model.editing.as_deref() == Some(id.as_str()),
                rot: geom_rotation(&geometry),
            });
        }
    }
    out
}

/// `selectedItems(model, view)`.
pub fn selected_items(model: &Model, view: Option<&ViewEnv>) -> Vec<RenderItem> {
    let mut items = Vec::new();
    for id in model.selected.iter() {
        let Some(record) = model.record(id) else { continue };
        let geometry = eff_geom(model, id, view);
        let style = eff_style(&record.annotation, view);
        let rot = geom_rotation(&geometry);
        let item = RenderItem {
            id: id.clone(),
            reference: Some(ref_of(Some(record)).unwrap_or(Value::Null)),
            annotation: Some(record.annotation.clone()),
            subtype: kind_of(&record.annotation).name,
            box_: geom_visual_bounds(&geometry, style.stroke()),
            geometry,
            ap_box: None,
            style,
            text: None,
            icon: None,
            label: None,
            measure: None,
            source: record.source.into(),
            picture_version: None,
            lent_scene: Default::default(),
            ghost_opacity: None,
            selected: true,
            hovered: None,
            rot: Some(rot),
            ap_rot: None,
            blend: None,
            frame: None,
            raster: None,
            order: Default::default(),
        }
        .ordered(&[
            "id",
            "ref",
            "annotation",
            "subtype",
            "geometry",
            "box",
            "style",
            "source",
            "selected",
            "rot",
        ]);
        items.push(placed(item, anchored_scale(anchor_mode_of(record).as_ref(), view)));
    }
    items
}

fn box_corners(rect: Rect) -> [Point; 4] {
    [
        Point { x: rect.x, y: rect.y },
        Point { x: rect.x + rect.width, y: rect.y },
        Point { x: rect.x + rect.width, y: rect.y + rect.height },
        Point { x: rect.x, y: rect.y + rect.height },
    ]
}

fn effective_selection_frame(
    model: &Model,
    id: &str,
    geometry: Shape,
    view: Option<&ViewEnv>,
) -> SelectionFrame {
    let record = model.record(id).expect("a record");
    let live = Live {
        geometry: Some(geometry),
        style: Some(eff_style(&record.annotation, view)),
        measure: Some(eff_measure(model, id)),
    };
    annotation_selection_frame(record, None, &live)
}

fn union_bounds_of(
    model: &Model,
    page: &PageRef,
    geom_of: &dyn Fn(&str) -> Shape,
    view: Option<&ViewEnv>,
) -> Option<Rect> {
    let mut corners = Vec::new();
    for id in model.selected.iter() {
        let Some(record) = model.record(id) else { continue };
        if record.page_number() != Some(page.object_number) {
            continue;
        }
        corners.extend(effective_selection_frame(model, id, geom_of(id), view).corners);
    }
    (!corners.is_empty()).then(|| union_rect(&corners))
}

fn selectable_on_page(model: &Model, page: &PageRef) -> Vec<Id> {
    model
        .selected
        .iter()
        .filter(|id| {
            is_selectable(model, id)
                && model.record(id).is_some_and(|r| r.page_number() == Some(page.object_number))
        })
        .cloned()
        .collect()
}

fn place_selection_knob(
    model: &Model,
    page: &PageRef,
    page_box: Option<Rect>,
    knob_offset: f64,
    geom_of: &dyn Fn(&str) -> Shape,
    view: Option<&ViewEnv>,
) -> Option<Knob> {
    let selection = selectable_on_page(model, page);
    if selection.len() == 1 {
        let record = model.record(&selection[0])?;
        if !kind_of(&record.annotation).caps.rotatable
            || !is_transformable_by_hand(&model.editable, record)
        {
            return None;
        }
        let frame = effective_selection_frame(model, &record.id, geom_of(&record.id), view);
        return Some(place_rotate_knob(&frame.corners, knob_offset, page_box));
    }
    if selection.len() > 1
        && group_caps(model, &selection).rotatable
        && let Some(union) = union_bounds_of(model, page, geom_of, view)
    {
        return Some(place_rotate_knob(&box_corners(union), knob_offset, page_box));
    }
    None
}

/// `selectionKnob(model, page, pageBox, knobOffset, view)`.
pub fn selection_knob(
    model: &Model,
    page: &PageRef,
    page_box: Option<Rect>,
    knob_offset: f64,
    view: Option<&ViewEnv>,
) -> Option<Knob> {
    if let Some(Draft::Rotate(d)) = model.draft()
        && d.ids
            .first()
            .and_then(|i| model.record(i))
            .is_some_and(|r| r.page_number() == Some(page.object_number))
    {
        let rest_geom = |id: &str| {
            let record = model.record(id).expect("a record");
            anchored_geom(&shape_of(&record.annotation), anchor_mode_of(record).as_ref(), view)
        };
        let rest = place_selection_knob(model, page, page_box, knob_offset, &rest_geom, view)?;
        let delta = rotate_draft_delta(model, d).delta;
        return Some(Knob {
            at: rotate_point(rest.at, d.pivot, delta),
            from: rotate_point(rest.from, d.pivot, delta),
        });
    }
    place_selection_knob(
        model,
        page,
        page_box,
        knob_offset,
        &|id: &str| eff_geom(model, id, view),
        view,
    )
}

fn handle_role(id: &str) -> &'static str {
    match id {
        "nw" | "ne" | "se" | "sw" => "corner",
        "n" | "e" | "s" | "w" => "side",
        _ => "point",
    }
}

/// `chrome(model, page, pageBox, knobOffset, view)`.
pub fn chrome(
    model: &Model,
    page: &PageRef,
    page_box: Option<Rect>,
    knob_offset: f64,
    view: Option<&ViewEnv>,
) -> Vec<ChromeNode> {
    let mut nodes = Vec::new();
    let on_page = |p: &PageRef| p.object_number == page.object_number;
    let first_on_page = |ids: &[Id]| {
        ids.first()
            .and_then(|i| model.record(i))
            .is_some_and(|r| r.page_number() == Some(page.object_number))
    };
    match model.draft() {
        Some(Draft::Marquee(d)) if on_page(&d.page) => {
            nodes.push(ChromeNode::Marquee(OutlineNode {
                rect: rect_from_points(d.from, d.to),
                order: Default::default(),
            }))
        }
        Some(Draft::CreateRect(d)) if d.subtype == "link" && on_page(&d.page) => {
            nodes.push(ChromeNode::Marquee(OutlineNode {
                rect: rect_from_points(d.from, d.to),
                order: Default::default(),
            }))
        }
        Some(Draft::Move(d)) if !d.guides.is_empty() && first_on_page(&d.ids) => {
            for g in &d.guides {
                nodes.push(ChromeNode::Guide(GuideNode {
                    axis: g.axis.clone(),
                    at: g.at,
                    lo: g.lo,
                    hi: g.hi,
                    order: Default::default(),
                }));
            }
        }
        _ => {}
    }
    let rd = match model.draft() {
        Some(Draft::Rotate(d)) if first_on_page(&d.ids) => Some(d.clone()),
        _ => None,
    };
    if let Some(rd) = &rd {
        let angle = rotate_draft_delta(model, rd).angle;
        let span = page_box.unwrap_or(Rect {
            x: rd.pivot.x - 300.0,
            y: rd.pivot.y - 300.0,
            width: 600.0,
            height: 600.0,
        });
        let mut lines = Vec::new();
        for (deg, role) in [(0.0, "axis"), (90.0, "axis"), (angle, "indicator")] {
            if let Some(chord) = chord_through(span, rd.pivot, deg) {
                lines.push(GuideLine {
                    a: chord.a,
                    b: chord.b,
                    role: role.into(),
                    order: Default::default(),
                });
            }
        }
        nodes.push(ChromeNode::RotateGuides(RotateGuidesNode {
            center: rd.pivot,
            angle,
            lines,
            order: Default::default(),
        }));
    }
    let selection = selectable_on_page(model, page);
    let screen = view.map_or(0.0, |v| v.rotation);
    if selection.len() == 1 {
        let record = model.record(&selection[0]).expect("a record");
        let geometry = eff_geom(model, &selection[0], view);
        let style = eff_style(&record.annotation, view);
        let caps = kind_of(&record.annotation).caps;
        let rot = geom_rotation(&geometry);
        let measure = eff_measure(model, &record.id);
        let distance = measure
            .as_ref()
            .filter(|m| m.is_distance())
            .and_then(|m| distance_layout(&geometry, m, style.stroke_width));
        let frame = effective_selection_frame(model, &record.id, geometry.clone(), view);
        if frame.angle != 0.0 {
            nodes.push(ChromeNode::Obb(ObbNode {
                corners: frame.corners.to_vec(),
                angle: frame.angle,
                order: Default::default(),
            }));
        } else {
            nodes.push(ChromeNode::Outline(OutlineNode {
                rect: union_rect(&frame.corners),
                order: Default::default(),
            }));
        }
        if rd.is_none()
            && is_transformable_by_hand(&model.editable, record)
            && (caps.resizable || caps.vertex_editable)
        {
            let handles = match &distance {
                Some(d) => distance_handles(d),
                None => geom_handles(&geometry),
            };
            let dragged = match model.draft() {
                Some(Draft::Handle(d)) if d.id == record.id => Some(d.handle.clone()),
                _ => None,
            };
            for handle in handles {
                nodes.push(ChromeNode::Handle(HandleNode {
                    at: handle.at,
                    cursor: cursor_on_screen(&handle.cursor, screen),
                    rot: (rot != 0.0 && !rot.is_nan()).then_some(rot),
                    role: handle_role(&handle.id).into(),
                    active: dragged.as_deref() == Some(handle.id.as_str()),
                    order: Default::default(),
                }));
            }
        }
    } else if selection.len() > 1
        && rd.is_none()
        && let Some(union) =
            union_bounds_of(model, page, &|id: &str| eff_geom(model, id, view), view)
    {
        nodes.push(ChromeNode::Outline(OutlineNode { rect: union, order: Default::default() }));
        if group_caps(model, &selection).resizable {
            let dragged = match model.draft() {
                Some(Draft::Group(d)) if d.op == "resize" => Some(d.handle.clone()),
                _ => None,
            };
            for handle in rect_handles_for(union) {
                nodes.push(ChromeNode::Handle(HandleNode {
                    at: handle.at,
                    cursor: cursor_on_screen(&handle.cursor, screen),
                    rot: None,
                    role: handle_role(&handle.id).into(),
                    active: dragged.as_deref() == Some(handle.id.as_str()),
                    order: Default::default(),
                }));
            }
        }
    }
    if rd.is_none()
        && let Some(knob) = selection_knob(model, page, page_box, knob_offset, view)
    {
        nodes.push(ChromeNode::RotateKnob(KnobNode {
            at: knob.at,
            from: knob.from,
            order: Default::default(),
        }));
    }
    nodes
}

/// `selectionBoundsOnPage(model, page, view)`.
pub fn selection_bounds_on_page(
    model: &Model,
    page: &PageRef,
    view: Option<&ViewEnv>,
) -> Option<Rect> {
    let selection = selectable_on_page(model, page);
    if selection.is_empty() {
        return None;
    }
    let corners: Vec<Point> = selection
        .iter()
        .flat_map(|id| {
            effective_selection_frame(model, id, eff_geom(model, id, view), view).corners
        })
        .collect();
    Some(union_rect(&corners))
}

/// A selection-aware menu's anchor (`selectionAnchor`).
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionAnchor {
    pub page: PageRef,
    pub bounds: Rect,
    pub knob: Option<Point>,
}

/// `selectionAnchor(model, pageBoxOf, knobOffsetOf, viewOf)`.
pub fn selection_anchor(
    model: &Model,
    page_box_of: &dyn Fn(&PageRef) -> Option<Rect>,
    knob_offset_of: &dyn Fn(&PageRef) -> Option<f64>,
    view_of: &dyn Fn(&PageRef) -> Option<ViewEnv>,
) -> Option<SelectionAnchor> {
    if let Some(draft) = model.draft()
        && !draft.tag().starts_with("create-")
    {
        return None;
    }
    let id = model.selected.iter().find(|id| is_selectable(model, id))?;
    let page = model.record(id)?.page();
    let view = view_of(&page);
    let bounds = selection_bounds_on_page(model, &page, view.as_ref())?;
    let knob = selection_knob(
        model,
        &page,
        page_box_of(&page),
        knob_offset_of(&page).unwrap_or(ROTATE_KNOB_OFFSET),
        view.as_ref(),
    );
    Some(SelectionAnchor { page, bounds, knob: knob.map(|k| k.at) })
}

/// `rotationAnchor(model)`.
pub fn rotation_anchor(model: &Model) -> Option<RotationAnchor> {
    let Some(Draft::Rotate(d)) = model.draft() else { return None };
    let page = d.ids.first().and_then(|i| model.record(i))?.page();
    Some(RotationAnchor {
        page,
        at: d.current,
        angle: crate::js::round(rotate_draft_delta(model, d).angle),
        order: Default::default(),
    })
}

/// `creationDraftAnchor(model)`.
pub fn creation_draft_anchor(model: &Model) -> Option<CreationDraftAnchor> {
    let Some(Draft::CreatePoly(d)) = model.draft() else { return None };
    if d.points.is_empty() {
        return None;
    }
    let min_points = if d.closed { 3 } else { 2 };
    let can_finish = d.points.len() >= min_points
        && match &d.measure {
            None => true,
            Some(m) => {
                let shape = Shape::Poly(PolyShape {
                    vertices: d.points.clone(),
                    closed: d.closed,
                    line_endings: None,
                    caption_center: None,
                    rotation: 0.0,
                    order: Default::default(),
                });
                shape_measurement_readout(&shape, &MeasurementAppearance(m.clone()))
                    .unavailable()
                    .is_none()
            }
        };
    Some(CreationDraftAnchor {
        kind: "poly".into(),
        subtype: if d.closed { "polygon" } else { "polyline" }.into(),
        page: d.page.clone(),
        bounds: union_rect(&d.points),
        point_count: d.points.len() as f64,
        min_points: min_points as f64,
        can_finish,
        order: Default::default(),
    })
}

/// `annotationAnchor(model, id, view)`.
pub fn annotation_anchor(
    model: &Model,
    id: &str,
    view: Option<&ViewEnv>,
) -> Option<(PageRef, Rect)> {
    let record = model.record(id)?;
    let frame = effective_selection_frame(model, id, eff_geom(model, id, view), view);
    Some((record.page(), union_rect(&frame.corners)))
}
