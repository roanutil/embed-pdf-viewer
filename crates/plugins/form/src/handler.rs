//! `handler.ts`: draw-to-place. Drag a box (previewed through the annotation
//! plane's placement ghost) or click (the tool's `clickCreate` size, through
//! the shared placement layer); the commit creates field and widget with
//! `placeField`, styled from the tool's live defaults, and selects the new
//! widget. Gesture rules match the annotation handlers: page-anchored via
//! the sample projection, the up sample is the final point, and a click is
//! width and height both under the drag threshold.

use std::cell::RefCell;

use epdf_annotation_core::{
    AnnotationPropsPatch, AnnotationRef, ClickCreate, ClickPlacement, MIN_DRAG, PlacementOptions,
    resolve_click_placement,
};
use epdf_geometry::js::{max, min};
use epdf_geometry::{Point, Rect};
use epdf_kernel::rt;
use epdf_plugin_annotation::{AnnotationCapability, widget_appearance_from_props};
use epdf_plugin_interaction::{
    InteractionCapability, InteractionHandler, PageObjectNumber, PointerSample, Tool,
    sample_point_on,
};

use crate::capability::FormCapability;
use crate::tools::{ClickSize, form_tool_by_id};
use crate::types::PlaceFieldInput;

fn rect_from(a: Point, b: Point) -> Rect {
    Rect { x: min(a.x, b.x), y: min(a.y, b.y), width: (b.x - a.x).abs(), height: (b.y - a.y).abs() }
}

#[derive(Clone, Copy)]
struct Origin {
    pon: PageObjectNumber,
    start: Point,
    last: Point,
}

/// The `form-place` handler.
pub struct PlaceHandler {
    form: FormCapability,
    interaction: InteractionCapability,
    annotation: Option<AnnotationCapability>,
    origin: RefCell<Option<Origin>>,
}

/// `createPlaceHandler(form, interaction, annotation)`.
pub fn create_place_handler(
    form: FormCapability,
    interaction: InteractionCapability,
    annotation: Option<AnnotationCapability>,
) -> PlaceHandler {
    PlaceHandler { form, interaction, annotation, origin: RefCell::new(None) }
}

/// The click box through the shared placement layer.
fn box_of_click(point: Point, policy: ClickSize, page_box: Option<Rect>) -> Rect {
    let click = ClickCreate {
        width: Some(policy.width),
        height: Some(policy.height),
        ..ClickCreate::default()
    };
    let options = PlacementOptions { page_box, ..PlacementOptions::default() };
    match resolve_click_placement(point, &click, &options) {
        ClickPlacement::Box { rect, .. } => rect,
        ClickPlacement::Segment { .. } => {
            Rect { x: point.x, y: point.y, width: policy.width, height: policy.height }
        }
    }
}

impl InteractionHandler for PlaceHandler {
    fn id(&self) -> &str {
        "form-place"
    }

    /// 95, as the TypeScript sets it: while a palette tool is active a drag
    /// on an empty page places a field rather than starting a marquee.
    fn priority(&self) -> f64 {
        95.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("form-place")
    }

    fn on_down(&self, s: &PointerSample) -> bool {
        // No capture without a page, a palette tool, or the design grant.
        let Some(page) = &s.page else { return false };
        if form_tool_by_id(&self.interaction.active_tool_id()).is_none() {
            return false;
        }
        if !self.form.can_design() {
            return false;
        }
        *self.origin.borrow_mut() =
            Some(Origin { pon: page.pon, start: page.point, last: page.point });
        true
    }

    fn on_move(&self, s: &PointerSample) {
        let Some(origin) = *self.origin.borrow() else { return };
        let Some(point) = sample_point_on(s, origin.pon) else { return };
        if let Some(o) = self.origin.borrow_mut().as_mut() {
            o.last = point;
        }
        let bx = rect_from(origin.start, point);
        if let Some(annotation) = &self.annotation {
            if max(bx.width, bx.height) >= MIN_DRAG {
                annotation.set_placement_preview(
                    &self.interaction.active_tool_id(),
                    origin.pon,
                    bx,
                );
            } else {
                annotation.clear_placement_preview();
            }
        }
    }

    fn on_up(&self, s: &PointerSample) {
        let Some(o) = self.origin.borrow_mut().take() else { return };
        if let Some(annotation) = &self.annotation {
            annotation.clear_placement_preview();
        }
        let tool_id = self.interaction.active_tool_id();
        let Some(tool) = form_tool_by_id(&tool_id) else { return };
        let end = sample_point_on(s, o.pon).unwrap_or(o.last);
        let dragged = rect_from(o.start, end);
        let is_click = dragged.width < MIN_DRAG && dragged.height < MIN_DRAG;
        let pon = f64::from(o.pon);
        let page_box = self.form.page_box(pon);
        let bx =
            if is_click { box_of_click(o.start, tool.click_create, page_box) } else { dragged };
        let props: AnnotationPropsPatch = match &self.annotation {
            Some(annotation) => epdf_kernel::js::from_value(&epdf_kernel::js::to_value(
                &annotation.current_defaults(&tool_id),
            ))
            .unwrap_or_default(),
            None => epdf_kernel::js::from_value(&tool.defaults).unwrap_or_default(),
        };
        let placing = self.form.place_field(PlaceFieldInput {
            family: tool.family.to_string(),
            page_object_number: pon,
            bx,
            appearance: Some(widget_appearance_from_props(&props)),
        });
        let (form, interaction, annotation) =
            (self.form.clone(), self.interaction.clone(), self.annotation.clone());
        rt::spawn(async move {
            match placing.await {
                Ok(placed) => {
                    // Auto-select only while the world has not moved on.
                    let Some(annotation) = annotation else { return };
                    if interaction.active_tool_id() != tool_id {
                        return;
                    }
                    let Some(widget) = placed.widget else { return };
                    let n = widget.get("annotObjectNumber").and_then(serde_json::Value::as_f64);
                    let Some(n) = n.filter(|&n| n > 0.0) else { return };
                    annotation.select(
                        &AnnotationRef::ObjectNumber {
                            page_object_number: o.pon,
                            annot_object_number: n,
                        },
                        None,
                    );
                }
                Err(error) => {
                    // `.then(...).catch(...)`: the rejection passes through
                    // the `then` promise first.
                    rt::tick().await;
                    form.console_error("[form] placeField failed:", &error);
                }
            }
        });
    }
}
