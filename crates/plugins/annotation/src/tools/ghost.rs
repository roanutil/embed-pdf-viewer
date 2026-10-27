//! `tools/ghost.ts`: the active tool's ghost, what a click at the pointer
//! would make, painted see-through where it will land. It comes from the
//! very calls the commit makes (`resolveClickPlacement` and `placedShape`, an
//! icon's `iconPlaceAt`, an armed stamp's `fitStampBox`) and paints through
//! `unmadeItem`. The state holds only the pointer (`ghostAt`); the ghost is
//! derived from it and the tool's live defaults.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::geometry::fit_stamp_box;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::placement::{
    GestureForm, PlacementFrame, gesture_placement, is_drag, placed_shape, resolve_click_placement,
};
use epdf_annotation_core::types::{
    ClickCreate, ClickPolicy, Draft, ItemSource, Model, Placement, Point, Shape, ViewEnv,
};
use epdf_annotation_core::view::unmade_item;
use epdf_geometry::Size;
use serde_json::{Value, json};

use crate::controller::Core;
use crate::js::{num, page_object_number};
use crate::model::{ForeignPlacement, GhostPointer, set_ghost_at, set_placing};
use crate::read::chrome::view_env;
use crate::tools::definitions::ResolvedTool;
use crate::write::placement::{annotation_of_tool, icon_place_at, is_icon_place_kind};
use crate::write::stamps::ArmedStamp;

const GHOST_ID: &str = "tool-ghost";
const PLACING_ID: &str = "tool-placing";

type ImageGhostCache = Option<(Rc<GhostPointer>, Rc<ArmedStamp>, Value)>;

#[derive(Default)]
pub struct GhostState {
    image: RefCell<ImageGhostCache>,
}

/// `stillAClick(draft, placing)`: the gesture in progress is still a click.
fn still_a_click(draft: Option<&Draft>, placing: Option<&ForeignPlacement>) -> bool {
    if let Some(p) = placing
        && is_drag(GestureForm::Box, p.from, p.to)
    {
        return false;
    }
    match draft {
        None => true,
        Some(Draft::CreateRect(d)) => !is_drag(GestureForm::Box, d.from, d.to),
        Some(Draft::CreateLine(d)) => !is_drag(GestureForm::Segment, d.from, d.to),
        Some(_) => false,
    }
}

fn click_create_of(tool: &ResolvedTool) -> Option<ClickCreate> {
    tool.click_create.as_object().map(|o| ClickCreate(o.clone()))
}

impl Core {
    /// `clearGhost()`.
    pub fn clear_ghost(&self) {
        self.update_state(|s| set_ghost_at(s, None));
    }

    /// Whether a click with `tool` places something.
    fn click_places(&self, tool: &ResolvedTool) -> bool {
        self.stamps.armed.borrow().is_some() || is_icon_place_kind(&tool.subtype) || tool.clicks()
    }

    /// `hoverGhostAt(toolId, page, point, displayRotation, zoom)`.
    pub fn hover_ghost_at(
        &self,
        tool_id: &str,
        page: &Value,
        point: Point,
        display_rotation: Option<f64>,
        zoom: Option<f64>,
    ) {
        let tool = self.tool(tool_id);
        let show = tool.as_ref().is_some_and(|t| {
            t.ghost != Value::Bool(false)
                && self.click_places(t)
                && self.size_of(page_object_number(page)).is_some()
        });
        if !show {
            self.clear_ghost();
            return;
        }
        let pointer = Rc::new(GhostPointer {
            tool_id: tool_id.to_string(),
            page: page.clone(),
            point,
            display_rotation,
            zoom,
        });
        self.update_state(|s| set_ghost_at(s, Some(pointer)));
    }

    /// `clickShape(tool, pointer, model)`: the shape a click with `tool` makes.
    fn click_shape(
        &self,
        tool: &ResolvedTool,
        pointer: &GhostPointer,
        model: &Model,
    ) -> Option<Shape> {
        let page = page_object_number(&pointer.page);
        let size = self.size_of(page)?;
        let annotation = annotation_of_tool(model, tool);
        if is_icon_place_kind(&tool.subtype) {
            let view = view_env(pointer.zoom, pointer.display_rotation);
            let (_, rect) = icon_place_at(&annotation, pointer.point, size, view.as_ref());
            return placed_shape(&annotation, &Placement::boxed(rect, 0.0));
        }
        let policy = click_create_of(tool)?;
        let placement = resolve_click_placement(
            pointer.point,
            &policy,
            &PlacementFrame {
                page_box: self.page_box_of(page),
                upright: Some(tool.upright),
                display_rotation: pointer.display_rotation,
            },
        );
        placed_shape(&annotation, &placement)
    }

    /// `itemsOn(page, model, view)`: what the ghost and a sibling's placement paint on a page.
    pub(crate) fn ghost_items_on(
        &self,
        page: f64,
        model: &Model,
        view: Option<&ViewEnv>,
    ) -> Vec<Value> {
        let state = self.state();
        let mut items = Vec::new();
        let ghost_tool = state.ghost_at.as_ref().and_then(|g| self.tool(&g.tool_id));
        if let (Some(ghost_at), Some(tool)) = (&state.ghost_at, &ghost_tool)
            && page_object_number(&ghost_at.page) == page
            && tool.ghost_shows()
            && self.stamps.armed.borrow().is_none()
            && still_a_click(model.draft(), state.placing.as_deref())
            && let Some(shape) = self.click_shape(tool, ghost_at, model)
        {
            let item = unmade_item(
                GHOST_ID,
                &annotation_of_tool(model, tool),
                &shape,
                ItemSource::Ghost,
                view,
                None,
            );
            let mut value = item.json();
            value["ghostOpacity"] = tool.ghost_opacity();
            items.push(value);
        }
        if let Some(placing) = &state.placing
            && page_object_number(&placing.page) == page
            && let Some(tool) = self.tool(&placing.tool_id)
            && is_drag(GestureForm::Box, placing.from, placing.to)
        {
            let policy = click_create_of(&tool).map(ClickPolicy::On).unwrap_or(ClickPolicy::Off);
            let placement = gesture_placement(
                GestureForm::Box,
                placing.from,
                placing.to,
                Some(&policy),
                &PlacementFrame {
                    page_box: self.page_box_of(page),
                    upright: None,
                    display_rotation: None,
                },
            );
            let annotation = annotation_of_tool(model, &tool);
            if let Some(shape) = placement.and_then(|p| placed_shape(&annotation, &p)) {
                items.push(
                    unmade_item(PLACING_ID, &annotation, &shape, ItemSource::Draft, view, None)
                        .json(),
                );
            }
        }
        items
    }

    /// `getImageGhost(page)`: the armed stamp's ghost, the same object while
    /// nothing it depends on changes.
    pub fn get_image_ghost(&self, page: &Value) -> Value {
        let state = self.state();
        let armed = self.stamps.armed.borrow().clone();
        let (Some(ghost_at), Some(armed)) = (state.ghost_at.clone(), armed) else {
            return Value::Null;
        };
        let number = page_object_number(page);
        if page_object_number(&ghost_at.page) != number {
            return Value::Null;
        }
        if !still_a_click(self.model().draft(), state.placing.as_deref()) {
            return Value::Null;
        }
        if let Some((g, a, v)) = &*self.ghost.image.borrow()
            && Rc::ptr_eq(g, &ghost_at)
            && Rc::ptr_eq(a, &armed)
        {
            return v.clone();
        }
        let tool = self.tool(&ghost_at.tool_id);
        let size = self.size_of(number);
        let rot = self.upright_rot_for(ghost_at.display_rotation);
        let value = match (tool.filter(|t| t.ghost_shows()), size) {
            (Some(tool), Some(size)) => json!({
                "page": page,
                "box": fit_stamp_box(
                    ghost_at.point,
                    Size { width: armed.width, height: armed.height },
                    Size { width: size.0, height: size.1 },
                    rot,
                ).json(),
                "rot": num(rot),
                "opacity": tool.ghost_opacity(),
            }),
            _ => Value::Null,
        };
        *self.ghost.image.borrow_mut() = Some((ghost_at, armed, value.clone()));
        value
    }

    /// `previewPlacement(toolId, page, from, to)`.
    pub fn preview_placement(&self, tool_id: &str, page: &Value, from: Point, to: Point) {
        let placing = Rc::new(ForeignPlacement {
            tool_id: tool_id.to_string(),
            page: page.clone(),
            from,
            to,
        });
        self.update_state(|s| set_placing(s, Some(placing)));
    }

    /// `clearPlacementPreview()`.
    pub fn clear_placement_preview(&self) {
        self.update_state(|s| set_placing(s, None));
    }
}
