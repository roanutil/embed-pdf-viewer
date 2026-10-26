//! `tools/handlers.ts`: draw-to-place. Drag a box or just click; the commit
//! creates the field and its widget through `form.create()`, styled from
//! the tool's live defaults. Where the field goes is the annotation core's
//! `gesturePlacement`: the dragged box once the gesture is a drag, else the
//! tool's `clickCreate` box. What follows is the annotation plugin's
//! `afterCreate` setting: select the new widget, and keep the tool or go
//! back to design's resting tool.
//!
//! The palette's early return is kept as the TypeScript has it: with no
//! annotation plugin, a field the view already shows ends the placement
//! before `created` is awaited, so an engine refusal of that create reaches
//! no console line (the TypeScript raises an unhandled rejection; Rust has
//! no such channel, and the refusal goes unobserved). That edge is the Task
//! 14 decision; `tests/behavior.rs` pins all three paths.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use epdf_annotation_core::placement::{GestureForm, PlacementFrame, gesture_placement};
use epdf_annotation_core::record::widget_appearance_of;
use epdf_annotation_core::types::{ClickCreate, ClickPolicy, Placement};
use epdf_geometry::{Point, Rect};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::AnnotationCapability;
use epdf_plugin_interaction::{
    InteractionCapability, InteractionHandler, PointerSample, Tool, sample_point_on,
};
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::model::Box;
use crate::tools::definitions::{FormToolDef, form_tool_by_id};
use crate::value::{get, items};

/// `nextFieldName(family, taken)`: `text_1`, `text_2`, ..., the first one
/// the form doesn't have.
pub fn next_field_name(family: &str, taken: &std::collections::HashSet<String>) -> String {
    let mut count = 1;
    while taken.contains(&format!("{family}_{count}")) {
        count += 1;
    }
    format!("{family}_{count}")
}

/// `clampToPage(box, page)`: the part of a box on the page; `None` when too
/// little of it is.
pub fn clamp_to_page(bx: Box, page: Box) -> Option<Box> {
    use epdf_geometry::js::{max, min};
    let x = max(page.x, min(bx.x, page.width));
    let y = max(page.y, min(bx.y, page.height));
    let clamped = Box {
        x,
        y,
        width: max(0.0, min(bx.x + bx.width, page.width) - x),
        height: max(0.0, min(bx.y + bx.height, page.height) - y),
    };
    (clamped.width >= 1.0 && clamped.height >= 1.0).then_some(clamped)
}

/// `toolDraft(tool, name, widget)`: the field a palette tool places.
pub fn tool_draft(tool: &FormToolDef, name: &str, widget: Value) -> Value {
    let with = |key: &'static str, value: Value| crate::value::spread(&widget, [(key, value)]);
    match tool.family {
        "radio" => json!({
            "family": "radio", "name": name, "widgets": [with("exportValue", json!("Choice1"))],
        }),
        "combobox" | "listbox" => json!({
            "family": tool.family, "name": name, "widgets": [widget],
            "options": [
                { "label": "Option 1", "value": "Option 1" },
                { "label": "Option 2", "value": "Option 2" },
            ],
        }),
        "pushbutton" => json!({
            "family": "pushbutton", "name": name, "widgets": [with("caption", json!("Button"))],
        }),
        family => json!({ "family": family, "name": name, "widgets": [widget] }),
    }
}

struct Gesture {
    page: PageRef,
    start: Point,
    last: Point,
}

/// The `form-place` handler.
pub struct PlaceHandler {
    form: Weak<Core>,
    interaction: InteractionCapability,
    annotation: Option<AnnotationCapability>,
    origin: RefCell<Option<Gesture>>,
}

impl PlaceHandler {
    fn end_gesture(&self) {
        *self.origin.borrow_mut() = None;
        if let Some(annotation) = &self.annotation {
            annotation.clear_placement_preview();
            annotation.clear_ghost();
        }
    }

    fn can_design(&self) -> bool {
        self.form.upgrade().is_some_and(|form| form.can_design())
    }
}

/// `place(tool, page, box)`.
fn place(
    form: Rc<Core>,
    interaction: InteractionCapability,
    annotation: Option<AnnotationCapability>,
    tool: &'static FormToolDef,
    page: PageRef,
    bx: Box,
) -> Promise<()> {
    Promise::spawn(async move {
        let page_json = page.to_json();
        let Some(rect) =
            form.get_page_box(&page_json).and_then(|page_box| clamp_to_page(bx, page_box))
        else {
            return Ok(());
        };
        // Style from the tool's live defaults when the annotation plugin
        // holds them; without it, the tool table's.
        let defaults = match &annotation {
            Some(annotation) => annotation.tools_get_defaults(tool.id),
            None => tool.defaults.clone(),
        };
        let look = widget_appearance_of(defaults.as_object().unwrap_or(&Map::new()));
        let taken =
            form.list(None).iter().map(|f| crate::value::string_of(get(f, "name"))).collect();
        let name = next_field_name(tool.family, &taken);
        let mut widget = Map::new();
        widget.insert("page".into(), page_json.clone());
        widget.insert("rect".into(), rect.to_json());
        widget.extend(look);
        // The new field shows at once when the document holds the numbers
        // it takes: then it is selected in the same frame.
        let created = form.create(tool_draft(tool, &name, Value::Object(widget)), None);
        let field = match form.get(&json!({ "kind": "fqn", "name": name })) {
            Some(field) => field,
            None => get(&created.clone().await?, "field").cloned().unwrap_or(Value::Null),
        };
        // Skip the follow-up when the tool changed meanwhile.
        let Some(annotation) = annotation else { return Ok(()) };
        if interaction.get_active_tool_id() != tool.id {
            return Ok(());
        }
        let settings = annotation.get_settings();
        let policy = settings.get("afterCreate").cloned().unwrap_or(Value::Null);
        let widget = items(&field, "widgets").iter().find(|w| {
            get(w, "page").and_then(|p| get(p, "objectNumber")).and_then(Value::as_f64)
                == Some(f64::from(page.page_object_number))
        });
        if crate::value::truthy(get(&policy, "select"))
            && let Some(reference) =
                widget.and_then(|w| get(w, "ref")).filter(|r| crate::value::truthy(Some(r)))
        {
            annotation.selection_set(std::slice::from_ref(reference));
        }
        // Back to design's resting tool, not the default pointer: that one
        // fills, which would end design and drop the new field's selection.
        if get(&policy, "tool").and_then(Value::as_str) == Some("default") {
            interaction.activate_tool("form-edit")?;
        }
        created.await?;
        Ok(())
    })
}

impl InteractionHandler for PlaceHandler {
    fn id(&self) -> &str {
        "form-place"
    }

    /// Above the annotation edit handler (100): while a palette tool is
    /// active, a drag on empty page means "place a field".
    fn priority(&self) -> f64 {
        95.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("form-place")
    }

    fn on_hover(&self, sample: &PointerSample) {
        let Some(annotation) = &self.annotation else { return };
        let tool_id = self.interaction.get_active_tool_id();
        match &sample.page {
            Some(hit)
                if form_tool_by_id(&tool_id).is_some()
                    && self.can_design()
                    && !self.interaction.has_cursor_claim() =>
            {
                annotation.hover_ghost_at(
                    &tool_id,
                    &hit.page.to_json(),
                    hit.point,
                    hit.rotation.map(|r| r.0),
                    hit.zoom,
                );
            }
            _ => annotation.clear_ghost(),
        }
    }

    fn on_down(&self, sample: &PointerSample) -> bool {
        // No capture without a page, a known palette tool, or design
        // permission: declining lets editing, panning or selection take it.
        let Some(hit) = &sample.page else { return false };
        if form_tool_by_id(&self.interaction.get_active_tool_id()).is_none() {
            return false;
        }
        if !self.can_design() {
            return false;
        }
        *self.origin.borrow_mut() =
            Some(Gesture { page: hit.page, start: hit.point, last: hit.point });
        true
    }

    fn on_move(&self, sample: &PointerSample) {
        let (page, start) = {
            let mut origin = self.origin.borrow_mut();
            let Some(gesture) = origin.as_mut() else { return };
            let Some(point) = sample_point_on(sample, gesture.page) else { return };
            gesture.last = point;
            (gesture.page, gesture.start)
        };
        if let Some(annotation) = &self.annotation {
            let point = self.origin.borrow().as_ref().map(|g| g.last).unwrap_or(start);
            annotation.preview_placement(
                &self.interaction.get_active_tool_id(),
                &page.to_json(),
                start,
                point,
            );
        }
    }

    fn on_up(&self, sample: &PointerSample) {
        let Some(gesture) = self.origin.borrow_mut().take() else { return };
        self.end_gesture();
        let Some(tool) = form_tool_by_id(&self.interaction.get_active_tool_id()) else { return };
        // The up sample is the final point; a release over the gap falls
        // back to the last resolved point.
        let end = sample_point_on(sample, gesture.page).unwrap_or(gesture.last);
        let Some(form) = self.form.upgrade() else { return };
        let page_box = form.get_page_box(&gesture.page.to_json()).map(|b| Rect {
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
        });
        let mut click = Map::new();
        click.insert("width".into(), json!(tool.click_create.0));
        click.insert("height".into(), json!(tool.click_create.1));
        let policy = ClickPolicy::On(ClickCreate(click));
        let frame = PlacementFrame { page_box, ..PlacementFrame::default() };
        let placement =
            gesture_placement(GestureForm::Box, gesture.start, end, Some(&policy), &frame);
        let Some(Placement::Box(placed)) = placement else { return };
        let bx = Box {
            x: placed.rect.x,
            y: placed.rect.y,
            width: placed.rect.width,
            height: placed.rect.height,
        };
        let placing =
            place(form, self.interaction.clone(), self.annotation.clone(), tool, gesture.page, bx);
        epdf_kernel::rt::spawn(async move {
            if let Err(error) = placing.reaction().await {
                crate::console::error("[form] placing the field failed:", error.to_canonical());
            }
        });
    }

    /// Aborted (a second finger, a system cancel): nothing is placed.
    fn on_cancel(&self, _sample: &PointerSample) {
        self.end_gesture();
    }
}

/// `createPlaceHandler(form, interaction, annotation)`.
pub fn create_place_handler(
    form: &Core,
    interaction: InteractionCapability,
    annotation: Option<AnnotationCapability>,
) -> Rc<PlaceHandler> {
    Rc::new(PlaceHandler { form: form.weak(), interaction, annotation, origin: RefCell::new(None) })
}
