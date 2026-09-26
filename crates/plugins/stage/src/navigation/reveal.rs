//! `navigation/reveal.ts`: reveals. A bare reveal is not navigation:
//! minimal visibility, cursor untouched (in paged flow revealing a page is
//! navigating to it). A positioned reveal (rect, anchor or zoom) is an
//! arrival: the cursor is set up front, the camera lands per the anchor
//! policy, and a resolved zoom becomes the zoom intent.

use crate::prelude::*;

#[derive(Clone, Copy)]
struct RevealPlacement {
    camera: Camera,
    bounds: Rect,
    zoom_changed: bool,
}

impl StageCapability {
    /// One axis of a positioned-reveal camera.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reveal_axis(
        &self,
        a: Option<RevealAnchorValue>,
        cam_pos: f64,
        rect_pos: f64,
        rect_extent: f64,
        vp_extent: f64,
        zoom: f64,
        zoom_changed: bool,
    ) -> f64 {
        let mut a = a;
        if a == Some(RevealAnchorValue::Word(RevealWord::Keep)) {
            return cam_pos;
        }
        let p = self.pad();
        if a.is_none() {
            if !zoom_changed {
                let lo = cam_pos + p / zoom;
                let hi = cam_pos + (vp_extent - p) / zoom;
                if rect_pos >= lo && rect_pos + rect_extent <= hi {
                    return cam_pos;
                }
                if rect_extent > hi - lo || rect_pos < lo {
                    return rect_pos - p / zoom;
                }
                return rect_pos + rect_extent - (vp_extent - p) / zoom;
            }
            a = Some(RevealAnchorValue::Word(RevealWord::Center));
        }
        match a {
            Some(RevealAnchorValue::Word(RevealWord::Start)) => rect_pos - p / zoom,
            Some(RevealAnchorValue::Word(RevealWord::End)) => {
                rect_pos + rect_extent - (vp_extent - p) / zoom
            }
            other => {
                let f = match other {
                    Some(RevealAnchorValue::Fraction(f)) => js::min(1.0, js::max(0.0, f)),
                    _ => 0.5,
                };
                rect_pos + rect_extent / 2.0 - (vp_extent * f) / zoom
            }
        }
    }
}

// ── the area's api ────────────────────────────────────────────────────────────

impl StageCapability {
    /// `reveal` by display index, clamped into range. Bare: minimal
    /// movement, cursor untouched. Positioned (rect, zoom or anchor set):
    /// the target lands at the anchor, optionally re-zooming, and the
    /// cursor follows.
    pub fn reveal_index(&self, page_index: isize, opts: Option<RevealOptions>) {
        self.mark_cause(MotionCause::Programmatic);
        let page_count = self.page_count_now();
        if page_count == 0 {
            return;
        }
        let target = page_index.clamp(0, page_count as isize - 1) as usize;
        let positioned = opts.is_some_and(|o| {
            o.rect.is_some()
                || o.anchor.is_some()
                || o.zoom.is_some_and(|z| z != RevealZoom::Word(RevealZoomWord::Keep))
        });
        let behavior_of = |o: Option<RevealOptions>| {
            o.and_then(|o| o.behavior).unwrap_or(self.st().settings.scroll_behavior)
        };

        if !positioned {
            if self.paged() {
                self.go_to_target(
                    target as isize,
                    opts.map(|o| GoToOptions { behavior: o.behavior, ..Default::default() }),
                );
                return;
            }
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return;
            }
            let page = Self::page_rect_of(&scene.items[self.item_index_of_page(target)], target);
            // Reveal the outer box: the pageFrame bands belong to the page.
            let m = self.world_page_frame();
            let bx = Rect {
                x: page.x - m.left,
                y: page.y - m.top,
                width: page.width + m.left + m.right,
                height: page.height + m.top + m.bottom,
            };
            let camera = sc::reveal_camera(self.cam(), bx, self.vp(), self.pad());
            let current = self.cam();
            if camera.x == current.x && camera.y == current.y {
                return;
            }
            self.cancel_anim();
            if behavior_of(opts) == ScrollBehaviorKind::Smooth {
                self.animate_to(camera, self.scene_rect(), None);
            } else {
                self.set_cam(camera, self.scene_rect());
            }
            return;
        }

        let opts = opts.unwrap_or_default();
        self.cancel_anim();
        if target != self.st().cursor {
            self.dispatch(StageAction::Cursor { cursor: target });
        }

        let place = || -> Option<RevealPlacement> {
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return None;
            }
            let item = if self.paged() {
                &scene.items[0]
            } else {
                &scene.items[self.item_index_of_page(target)]
            };
            let world = match opts.rect {
                Some(rect) => Self::world_rect_for_content(item, target, rect),
                None => Self::page_rect_of(item, target),
            };
            let zd = opts.zoom.unwrap_or(RevealZoom::Word(RevealZoomWord::Keep));
            let avail_w = js::max(1.0, self.vp().width - 2.0 * self.pad());
            let avail_h = js::max(1.0, self.vp().height - 2.0 * self.pad());
            let mut zoom = match zd {
                RevealZoom::Level { level } => level,
                RevealZoom::Word(RevealZoomWord::Fit) => {
                    js::min(avail_w / world.width, avail_h / world.height)
                }
                RevealZoom::Word(RevealZoomWord::FitWidth) => avail_w / world.width,
                RevealZoom::Word(RevealZoomWord::FitHeight) => avail_h / world.height,
                RevealZoom::Word(RevealZoomWord::Keep) => self.cam().zoom,
            };
            // A degenerate target with a fit directive pans only.
            if !zoom.is_finite() || zoom <= 0.0 {
                zoom = self.cam().zoom;
            }
            let zoom_changed = zd != RevealZoom::Word(RevealZoomWord::Keep);
            let a = opts.anchor.unwrap_or_default();
            let c = self.cam();
            let v = self.vp();
            Some(RevealPlacement {
                camera: Camera {
                    x: self.reveal_axis(
                        a.x,
                        c.x,
                        world.x,
                        world.width,
                        v.width,
                        zoom,
                        zoom_changed,
                    ),
                    y: self.reveal_axis(
                        a.y,
                        c.y,
                        world.y,
                        world.height,
                        v.height,
                        zoom,
                        zoom_changed,
                    ),
                    zoom,
                },
                bounds: self.bounds_for(item),
                zoom_changed,
            })
        };

        let Some(first) = place() else { return };
        if first.zoom_changed {
            self.dispatch(StageAction::Patch {
                patch: PartialSettings::zoom(ZoomSpec::Level { level: first.camera.zoom }),
            });
        }
        if behavior_of(Some(opts)) == ScrollBehaviorKind::Smooth {
            let p = place().unwrap_or(first);
            self.animate_to(p.camera, p.bounds, None);
        } else {
            self.stabilized(|| {
                if let Some(p) = place() {
                    self.set_cam(p.camera, p.bounds);
                }
            });
        }
    }

    /// Bring a page, or a page-space rect on it, into view. A page the
    /// document does not have is a no-op.
    pub fn reveal(&self, page: PageRef, opts: Option<RevealOptions>) {
        if let Some(index) = self.index_of_page(page) {
            self.reveal_index(index as isize, opts);
        }
    }

    /// Sugar: a positioned reveal of a page-space rect.
    pub fn reveal_rect(&self, page: PageRef, rect: Rect, opts: Option<RevealOptions>) {
        self.reveal(page, Some(RevealOptions { rect: Some(rect), ..opts.unwrap_or_default() }));
    }
}
