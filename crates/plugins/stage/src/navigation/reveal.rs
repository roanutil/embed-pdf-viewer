//! `navigation/reveal.ts`: reveals and destinations. A bare reveal is not
//! navigation: minimal visibility, cursor untouched (in paged flow
//! revealing a page is navigating to it). A positioned reveal (rect, anchor
//! or zoom: a search hit) is an arrival: the cursor is set up front, the
//! camera lands per the anchor policy, and a resolved zoom becomes the zoom
//! intent. A destination is a positioned reveal of the spot it names.

use crate::destination::reveal_of_destination;
use crate::model::{CameraObject, patch_settings, set_cursor};
use crate::prelude::*;

#[derive(Clone, Copy)]
struct RevealPlacement {
    camera: CameraObject,
    bounds: Rect,
    zoom_changed: bool,
}

impl StageCapability {
    /// One axis of a positioned-reveal camera. `None` is 'nearest' (move
    /// only when the target is outside the padded view), unless the zoom
    /// just changed: then the slack-axis rule (center) applies. 'keep' never
    /// moves the axis.
    #[allow(clippy::too_many_arguments)]
    fn reveal_axis(
        &self,
        anchor: Option<RevealAnchorValue>,
        camera_position: f64,
        rect_position: f64,
        rect_extent: f64,
        viewport_extent: f64,
        zoom: f64,
        zoom_changed: bool,
    ) -> f64 {
        let mut anchor = anchor;
        if anchor == Some(RevealAnchorValue::Word(RevealWord::Keep)) {
            return camera_position;
        }
        let inset = self.padding();
        if anchor.is_none() {
            if !zoom_changed {
                let low = camera_position + inset / zoom;
                let high = camera_position + (viewport_extent - inset) / zoom;
                if rect_position >= low && rect_position + rect_extent <= high {
                    return camera_position; // already visible
                }
                if rect_extent > high - low || rect_position < low {
                    return rect_position - inset / zoom;
                }
                return rect_position + rect_extent - (viewport_extent - inset) / zoom;
            }
            anchor = Some(RevealAnchorValue::Word(RevealWord::Center));
        }
        match anchor {
            Some(RevealAnchorValue::Word(RevealWord::Start)) => rect_position - inset / zoom,
            Some(RevealAnchorValue::Word(RevealWord::End)) => {
                rect_position + rect_extent - (viewport_extent - inset) / zoom
            }
            other => {
                let fraction = match other {
                    Some(RevealAnchorValue::Fraction(f)) => js::min(1.0, js::max(0.0, f)),
                    _ => 0.5,
                };
                rect_position + rect_extent / 2.0 - (viewport_extent * fraction) / zoom
            }
        }
    }

    pub(crate) fn reveal_at(&self, page_index: isize, options: Option<RevealOptions>) {
        self.mark_cause(MotionCause::Programmatic);
        let page_count = self.page_count();
        if page_count == 0 {
            return;
        }
        let target = page_index.clamp(0, page_count as isize - 1) as usize;
        // `rect: null` means "no rect", the same as absent.
        let positioned = options.is_some_and(|o| {
            o.rect.is_some()
                || o.anchor.is_some()
                || o.zoom.is_some_and(|z| z != RevealZoom::Word(RevealZoomWord::Keep))
        });
        let behavior_of = |o: Option<RevealOptions>| {
            o.and_then(|o| o.behavior).unwrap_or(self.st().settings.scroll_behavior)
        };

        if !positioned {
            // A bare reveal is not navigation: minimal visibility, cursor
            // untouched.
            if self.paged() {
                // The page is not in the one-item slice: revealing it is
                // navigating to it.
                self.go_to_target(
                    target as isize,
                    options.map(|o| GoToOptions { behavior: o.behavior, ..Default::default() }),
                );
                return;
            }
            let current = self.build_scene();
            if current.item_count == 0 {
                return;
            }
            let Some(item) = current.items.get(self.item_index_of_page(target)) else { return };
            let page = Self::page_rect_of(item, target);
            // Reveal the outer box: pageFrame chrome belongs to the page.
            let frame = self.world_page_frame();
            let outer = Rect {
                x: page.x - frame.left,
                y: page.y - frame.top,
                width: page.width + frame.left + frame.right,
                height: page.height + frame.top + frame.bottom,
            };
            let next = sc::reveal_camera(self.camera(), outer, self.viewport(), self.padding());
            let now = self.camera();
            if next.x == now.x && next.y == now.y {
                return; // already visible
            }
            self.cancel_animation();
            let next = CameraObject::zoom_xy(next);
            if behavior_of(options) == ScrollBehaviorKind::Smooth {
                self.animate_to(next, self.scene_rect(), None);
            } else {
                self.write_camera(next, self.scene_rect());
            }
            return;
        }

        // A positioned reveal is an arrival at a rect or point: the cursor
        // is intent, set up front.
        let options = options.unwrap_or_default();
        self.cancel_animation();
        self.update(|s| set_cursor(s, target));

        let place = || -> Option<RevealPlacement> {
            let current = self.build_scene();
            if current.item_count == 0 {
                return None;
            }
            let item = if self.paged() {
                &current.items[0]
            } else {
                current.items.get(self.item_index_of_page(target))?
            };
            let world = match options.rect {
                Some(rect) => Self::world_rect_for_content(item, target, rect),
                None => Self::page_rect_of(item, target),
            };
            let directive = options.zoom.unwrap_or(RevealZoom::Word(RevealZoomWord::Keep));
            let available_width = js::max(1.0, self.viewport().width - 2.0 * self.padding());
            let available_height = js::max(1.0, self.viewport().height - 2.0 * self.padding());
            let mut zoom = match directive {
                RevealZoom::Level { level } => level,
                RevealZoom::Word(RevealZoomWord::Fit) => {
                    js::min(available_width / world.width, available_height / world.height)
                }
                RevealZoom::Word(RevealZoomWord::FitWidth) => available_width / world.width,
                RevealZoom::Word(RevealZoomWord::FitHeight) => available_height / world.height,
                RevealZoom::Word(RevealZoomWord::Keep) => self.camera().zoom,
            };
            // A degenerate target (a point with a fit directive) only pans.
            if !zoom.is_finite() || zoom <= 0.0 {
                zoom = self.camera().zoom;
            }
            let zoom_changed = directive != RevealZoom::Word(RevealZoomWord::Keep);
            let anchor = options.anchor.unwrap_or_default();
            let camera = self.camera();
            let size = self.viewport();
            Some(RevealPlacement {
                camera: CameraObject::xy_zoom(Camera {
                    x: self.reveal_axis(
                        anchor.x,
                        camera.x,
                        world.x,
                        world.width,
                        size.width,
                        zoom,
                        zoom_changed,
                    ),
                    y: self.reveal_axis(
                        anchor.y,
                        camera.y,
                        world.y,
                        world.height,
                        size.height,
                        zoom,
                        zoom_changed,
                    ),
                    zoom,
                }),
                bounds: self.bounds_for(item),
                zoom_changed,
            })
        };

        let Some(first) = place() else { return };
        // A resolved zoom becomes the zoom intent.
        if first.zoom_changed {
            let level = first.camera.camera.zoom;
            self.update(|s| {
                patch_settings(s, &StageSettingsPatch::zoom(ZoomSpec::Level { level }))
            });
        }
        if behavior_of(Some(options)) == ScrollBehaviorKind::Smooth {
            // If the zoom patch re-wrapped the scene, recompute once.
            let landing = place().unwrap_or(first);
            self.animate_to(landing.camera, landing.bounds, None);
        } else {
            self.stabilized(|| {
                if let Some(landing) = place() {
                    self.write_camera(landing.camera, landing.bounds);
                }
            });
        }
    }

    fn when_placed_reveal(
        &self,
        options: Option<RevealOptions>,
        run: impl Fn(&StageCapability, Option<RevealOptions>) + 'static,
    ) {
        self.when_placed(
            options,
            |options| RevealOptions {
                behavior: Some(ScrollBehaviorKind::Instant),
                ..options.unwrap_or_default()
            },
            run,
        );
    }
}

// ── the area's api ──

impl StageCapability {
    /// Bring a page (its ref or its display index), or a spot on it, into
    /// view. Bare, it scrolls only when the page is off screen and leaves
    /// the current page alone; with `rect`, `zoom` or `anchor` it lands on
    /// that spot and the current page follows.
    pub fn reveal(&self, page: impl Into<PageTarget>, options: Option<RevealOptions>) {
        if let Some(index) = self.index_of_target(page.into()) {
            self.when_placed_reveal(options, move |stage, go| stage.reveal_at(index, go));
        }
    }

    /// Go where a destination says. It may change the zoom, as the
    /// destination asks. A destination on a page the document does not have
    /// is a no-op.
    pub fn go_to_destination(
        &self,
        destination: &PageDestination,
        options: Option<GoToDestinationOptions>,
    ) {
        let Some(layout) = self.get_page(PageTarget::Ref(destination.page())) else { return };
        let Some(size) =
            layout.get("size").and_then(|size| serde_json::from_value::<Size>(size.clone()).ok())
        else {
            return;
        };
        let Some(index) = layout
            .get("index")
            .and_then(serde_json::Value::as_f64)
            .filter(|index| *index >= 0.0 && index.fract() == 0.0)
        else {
            return;
        };
        let spot = reveal_of_destination(destination, size);
        let spot = RevealOptions { behavior: options.and_then(|o| o.behavior), ..spot };
        self.when_placed_reveal(Some(spot), move |stage, go| stage.reveal_at(index as isize, go));
    }
}
