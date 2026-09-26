//! `write/ghost.ts`: the armed tool's footprint ghost. Where (and what) the
//! next click would place, computed by the same rules the placement uses.

use std::rc::Rc;

use epdf_annotation_core::{
    Geom, PageObjectNumber, PageRef, PlacementOptions, Point, Rect, click_create_geom,
    defaults_for, fit_stamp_box, geom_visual_bounds, resolve_click_placement, style_from_props,
};
use epdf_geometry::js::{max, min};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::model::AnnotationAction;
use crate::services::geometry::page_size_of;
use crate::tools::definitions::GhostPolicy;
use crate::write::placement::{ICON_PLACE_SIZE, is_icon_place_kind};

impl AnnotationCapability {
    /// `clearGhost()` (host).
    pub fn clear_ghost(&self) {
        if self.state().tool_ghost.is_some() {
            self.cx().dispatch(AnnotationAction::SetToolGhost { ghost: None });
        }
    }

    /// `showVectorGhost(pon, toolId, geom)`.
    fn show_vector_ghost(&self, pon: PageObjectNumber, tool_id: &str, geom: Geom) {
        let preset = self.tool(tool_id).map_or_else(|| tool_id.to_string(), |t| t.preset);
        let style = style_from_props(&defaults_for(&self.model(), &preset));
        let bx = geom_visual_bounds(&geom, style.stroke_width, Some(&style.border));
        self.cx().dispatch(AnnotationAction::SetToolGhost {
            ghost: Some(Rc::new(ToolGhost {
                page: PageRef::new(pon),
                box_: bx,
                rot: 0.0,
                kind: ToolGhostKind::Vector { tool_id: tool_id.to_string(), geom },
            })),
        });
    }

    /// `hoverGhostAt(toolId, page, point, displayRotation)` (host).
    pub fn hover_ghost_at(
        &self,
        tool_id: &str,
        page: PageRef,
        point: Point,
        display_rotation: Option<f64>,
    ) {
        let pon = page.page_object_number;
        let (Some(tool), Some(crop)) = (self.tool(tool_id), self.crop_of(pon)) else {
            self.clear_ghost();
            return;
        };
        if tool.ghost == GhostPolicy::Off {
            self.clear_ghost();
            return;
        }
        let size = page_size_of(crop);
        if let Some(armed) = self.armed() {
            let rot = self.upright_rot_for(display_rotation);
            let bx = fit_stamp_box(point, armed.size(), size, rot);
            self.cx().dispatch(AnnotationAction::SetToolGhost {
                ghost: Some(Rc::new(ToolGhost { page, box_: bx, rot, kind: ToolGhostKind::Image })),
            });
            return;
        }
        if is_icon_place_kind(&tool.subtype) {
            let rot = self.upright_rot_for(display_rotation);
            let bx = fit_stamp_box(point, ICON_PLACE_SIZE, size, rot);
            self.show_vector_ghost(
                pon,
                tool_id,
                Geom::Rect { rect: bx, ellipse: false, rot: None },
            );
            return;
        }
        let Some(policy) = tool.click_policy() else {
            self.clear_ghost();
            return;
        };
        let placement = resolve_click_placement(
            point,
            &policy,
            &PlacementOptions {
                page_box: Some(Rect { x: 0.0, y: 0.0, width: size.width, height: size.height }),
                upright: Some(tool.upright),
                display_rotation,
            },
        );
        match click_create_geom(
            &tool.subtype,
            &placement,
            &defaults_for(&self.model(), &tool.preset),
        ) {
            Some(geom) => self.show_vector_ghost(pon, tool_id, geom),
            None => self.clear_ghost(),
        }
    }

    /// `setPlacementPreview(toolId, page, box)` (host): the page-clamped box
    /// an externally owned creation gesture would commit.
    pub fn set_placement_preview(&self, tool_id: &str, page: PageRef, bx: Rect) {
        let pon = page.page_object_number;
        let Some(crop) = self.crop_of(pon) else { return };
        let size = page_size_of(crop);
        let x = max(0.0, min(bx.x, size.width));
        let y = max(0.0, min(bx.y, size.height));
        let rect = Rect {
            x,
            y,
            width: max(0.0, min(bx.x + bx.width, size.width) - x),
            height: max(0.0, min(bx.y + bx.height, size.height) - y),
        };
        self.show_vector_ghost(pon, tool_id, Geom::Rect { rect, ellipse: false, rot: None });
    }

    /// `clearPlacementPreview()` (host).
    pub fn clear_placement_preview(&self) {
        self.clear_ghost();
    }

    /// `getToolGhost(page)` (host).
    pub fn get_tool_ghost(&self, page: PageRef) -> Option<Rc<ToolGhost>> {
        self.state().tool_ghost.clone().filter(|g| g.page.same_page(&page))
    }
}
