//! `navigation/zoom-drag.ts`: the zoom tool's drag, a box to zoom into or a
//! click to zoom one step. The drag is held in page coordinates (see
//! [`ZoomDrag`]); `get_zoom_box` hands it to the adapters in viewport px
//! only once it is big enough to be a box, so what shows is what the
//! release does. A box zooms in about one point, so it grows straight out
//! until it fills the view.

use epdf_kernel::geometry::PageRef;

use crate::model::{CameraObject, ZoomDrag, patch_settings, set_zoom_drag};
use crate::prelude::*;

/// A drag shorter than this on both axes, in viewport px, is a click.
const BOX_THRESHOLD_PX: f64 = 4.0;

fn rect_between(from: Point, to: Point) -> Rect {
    Rect {
        x: js::min(from.x, to.x),
        y: js::min(from.y, to.y),
        width: (to.x - from.x).abs(),
        height: (to.y - from.y).abs(),
    }
}

fn is_box(r: &Rect) -> bool {
    r.width >= BOX_THRESHOLD_PX || r.height >= BOX_THRESHOLD_PX
}

impl StageCapability {
    /// A page point to viewport px through the page as it is drawn: the
    /// exact inverse of `viewport_to_page`, so the box sits under the
    /// pointer and its middle lands on the point the user saw there.
    fn drag_to_viewport(&self, page: PageRef, point: Point) -> Option<Point> {
        let frame = self.get_page_frame(page)?;
        let view = frame.transform.page_to_view(point);
        Some(Point { x: frame.screen_x + view.x, y: frame.screen_y + view.y })
    }

    /// The drag's corners in viewport px; `None` when its page is not laid
    /// out.
    fn drag_viewport_rect(&self, drag: &ZoomDrag) -> Option<Rect> {
        let from = self.drag_to_viewport(drag.page, drag.from)?;
        let to = self.drag_to_viewport(drag.page, drag.to)?;
        Some(rect_between(from, to))
    }

    /// The drag's box in world units, the camera's own space; `None` when
    /// its page is not laid out.
    fn drag_world_rect(&self, drag: &ZoomDrag) -> Option<Rect> {
        let from = self.page_to_world(drag.page, drag.from)?;
        let to = self.page_to_world(drag.page, drag.to)?;
        Some(rect_between(from, to))
    }

    /// Zoom into the box: the zoom that fits it inside the padding, the
    /// camera that centers it (clamped, as every camera is), and the one
    /// point that is at the same place on screen before and after. A tween
    /// that holds that point while the zoom changes is a pure zoom.
    fn zoom_into(&self, drag: &ZoomDrag) -> Result<(), Error> {
        let Some(sized) = self.drag_world_rect(drag) else { return Ok(()) };
        let view = self.viewport();
        let inset = self.padding();
        let fit = js::min(
            js::max(1.0, view.width - inset - inset) / sized.width,
            js::max(1.0, view.height - inset - inset) / sized.height,
        );
        if !fit.is_finite() {
            return Ok(());
        }
        let zoom = js::min(sc::ZOOM_MAX, js::max(sc::ZOOM_MIN, fit));
        // The level becomes the zoom intent up front, as on a double-tap: a
        // tween caught halfway must not leave a fit mode stored.
        self.update(|s| {
            patch_settings(s, &StageSettingsPatch::zoom(ZoomSpec::Level { level: zoom }))
        });
        // A wrapped grid lays out again at a new zoom: place the box in the
        // scene as it is now.
        let r = self.drag_world_rect(drag).unwrap_or(sized);
        let from = self.camera();
        // Centered in the padded viewport.
        let target = sc::clamp_camera(
            Camera {
                zoom,
                x: r.x + r.width / 2.0 - (view.width + inset - inset) / (2.0 * zoom),
                y: r.y + r.height / 2.0 - (view.height + inset - inset) / (2.0 * zoom),
            },
            self.stay_bounds()?,
            view,
            self.constraint(),
        );
        self.mark_cause(MotionCause::User);
        if self.st().settings.scroll_behavior == ScrollBehaviorKind::Instant
            || (target.zoom - from.zoom).abs() < 1e-9
        {
            self.write_camera_stay(CameraObject::zoom_xy(target))?;
            return self.sync_cursor_from_camera();
        }
        // The fixed point w solves (w − from)·from.zoom = (w − target)·target.zoom
        // on each axis.
        let fixed_on = |start: f64, end: f64| {
            (end * target.zoom - start * from.zoom) / (target.zoom - from.zoom)
        };
        let fixed = Point { x: fixed_on(from.x, target.x), y: fixed_on(from.y, target.y) };
        let current = self.build_scene();
        if current.item_count == 0 {
            return Ok(());
        }
        let focal = sc::anchor_at_point(&current, fixed).map_err(stage_error)?;
        // A throw in a frame callback is uncaught in the TypeScript.
        self.animate_zoom_anchored(
            focal,
            sc::to_screen(from, fixed),
            target.zoom,
            Some(Rc::new(|stage: &StageCapability| {
                let _ = stage.sync_cursor_from_camera();
            })),
        )
    }

    /// Begin a drag of the `zoom` tool at a viewport point. Until it moves
    /// [`StageCapability::get_zoom_box`]'s threshold, it is a click.
    pub fn begin_zoom_drag(&self, point: ViewportPoint) {
        self.cancel_animation(); // the content holds still under the box
        // A press between pages anchors on a visible page: every page shares
        // the scene's plane, so any of them can hold the box.
        let page = match self.get_page_at(point) {
            Some(hit) => Some(hit.page_ref),
            None => self.list_visible_pages().first().map(|p| p.page_ref),
        };
        let from = page.and_then(|page| self.viewport_to_page(page, point));
        let drag = match (page, from) {
            (Some(page), Some(from)) => Some(ZoomDrag { page, from, to: from }),
            _ => None,
        };
        self.update(|s| set_zoom_drag(s, drag));
    }

    /// The zoom-tool drag moved to a viewport point; nothing without one.
    pub fn move_zoom_drag(&self, point: ViewportPoint) {
        let Some(drag) = self.st().zoom_drag else { return };
        // Off screen, the drag's page can't take the point: the box keeps
        // its last corner.
        if let Some(to) = self.viewport_to_page(drag.page, point) {
            self.update(|s| set_zoom_drag(s, Some(ZoomDrag { to, ..drag })));
        }
    }

    /// End the zoom-tool drag: zoom so its box fills the view, or, for a
    /// click, zoom one step around it (out with `zoom_out`). Nothing
    /// without a drag.
    pub fn end_zoom_drag(&self, options: Option<EndZoomDragOptions>) -> Result<(), Error> {
        let Some(drag) = self.st().zoom_drag else { return Ok(()) };
        self.update(|s| set_zoom_drag(s, None));
        let Some(r) = self.drag_viewport_rect(&drag) else { return Ok(()) };
        if !is_box(&r) {
            let around = Some(ZoomToOptions { around: Some(Point { x: r.x, y: r.y }) });
            return if options.and_then(|o| o.zoom_out) == Some(true) {
                self.zoom_out(around)
            } else {
                self.zoom_in(around)
            };
        }
        self.zoom_into(&drag)
    }

    /// Drop the zoom-tool drag without zooming.
    pub fn cancel_zoom_drag(&self) {
        self.update(|s| set_zoom_drag(s, None));
    }

    /// The box the zoom-tool drag would zoom to, in viewport px, for the
    /// adapter to draw; `None` without a drag, and while the drag is
    /// shorter than 4 px on both axes (a click). The TypeScript memoizes it
    /// for reference-stable reads; a Rust reader compares the value.
    pub fn get_zoom_box(&self) -> Option<Rect> {
        let drag = self.st().zoom_drag?;
        let r = self.drag_viewport_rect(&drag)?;
        is_box(&r).then_some(r)
    }
}
