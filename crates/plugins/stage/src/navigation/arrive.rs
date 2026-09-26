//! `navigation/arrive.ts`: anchors and arrivals. The anchor is the durable
//! "what am I looking at": captured before a structural change, re-applied
//! after. Navigation is canonical: `go_to_page_index(n)` ends in the same
//! camera whatever the zoom or the origin.

use crate::prelude::*;

/// Where a placement lands and what it clamps against.
#[derive(Clone, Copy)]
struct Placement {
    camera: Camera,
    bounds: Rect,
}

impl StageCapability {
    /// Re-run a placement until the scene it placed against is the scene
    /// that results (at most three passes).
    pub(crate) fn stabilized(&self, place: impl Fn()) {
        for _ in 0..3 {
            let scene = self.build_scene();
            place();
            if Rc::ptr_eq(&self.build_scene(), &scene) {
                return;
            }
        }
    }

    pub(crate) fn apply_anchor(&self, anchor: Anchor, align: Option<AlignmentValue>) {
        self.stabilized(|| {
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return;
            }
            let item_index = scene.item_of_page(anchor.page_index).expect("a scene with items");
            let item = &scene.items[item_index];
            let zoom = sc::resolve_zoom(
                self.st().settings.zoom,
                self.fit_box(item),
                self.vp(),
                self.pad(),
            );
            let at = self.align_point(align.unwrap_or(self.st().settings.anchor_align), self.vp());
            let camera = sc::camera_from_anchor(anchor, &scene, self.vp(), zoom, Some(at))
                .expect("a scene with items places an anchor");
            self.set_cam(camera, self.bounds_for(item));
        });
    }

    pub(crate) fn reapply(&self, anchor: Anchor, align: Option<AlignmentValue>) {
        if self.is_fit_all() {
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else {
            self.apply_anchor(anchor, align);
        }
    }

    pub(crate) fn go_to_target(&self, page_index: isize, opts: Option<GoToOptions>) {
        self.mark_cause(MotionCause::Programmatic);
        self.cancel_anim();
        let page_count = self.page_count_now();
        if page_count == 0 {
            return;
        }
        let target = page_index.clamp(0, page_count as isize - 1) as usize;
        if target != self.st().cursor {
            self.dispatch(StageAction::Cursor { cursor: target });
        }

        // Restore path: exact viewpoint instead of a fresh placement.
        if let Some(viewpoint) = opts.and_then(|o| o.viewpoint) {
            self.dispatch(StageAction::Patch { patch: PartialSettings::zoom(viewpoint.zoom) });
            self.apply_anchor(viewpoint.anchor, None);
            return;
        }

        let placement = || -> Option<Placement> {
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return None;
            }
            let item = if self.paged() {
                &scene.items[0]
            } else {
                &scene.items[self.item_index_of_page(target)]
            };
            let zoom = sc::resolve_zoom(
                self.st().settings.zoom,
                self.fit_box(item),
                self.vp(),
                self.pad(),
            );
            let subject = if self.is_fit_all() {
                self.scene_rect()
            } else if self.fits(Self::item_rect(item), zoom) {
                Self::item_rect(item)
            } else {
                Self::page_rect_of(item, target)
            };
            let setting = self.st().settings.arrival_align;
            let over = opts.and_then(|o| o.arrival_align).unwrap_or_default();
            let want_x = over.x.unwrap_or(setting.x);
            let want_y = over.y.unwrap_or(setting.y);
            let align_of = |w: ArrivalAlignValue| match w {
                ArrivalAlignValue::Keep(_) => AlignValue::Named(Align::Start),
                ArrivalAlignValue::Align(a) => a,
            };
            let placed = sc::place_camera(
                subject,
                self.vp(),
                zoom,
                self.pad(),
                AlignmentValue { x: align_of(want_x), y: align_of(want_y) },
                self.st().settings.direction,
            );
            // Arrivals clamp within the true bounds even unbounded.
            let bounds = self.bounds_for(item);
            let settled = sc::clamp_camera(
                placed,
                bounds,
                self.vp(),
                CameraConstraint { bounded: true, ..self.constraint() },
            );
            let keep = |w: ArrivalAlignValue| matches!(w, ArrivalAlignValue::Keep(_));
            Some(Placement {
                camera: Camera {
                    zoom,
                    x: if keep(want_x) { self.cam().x } else { settled.x },
                    y: if keep(want_y) { self.cam().y } else { settled.y },
                },
                bounds,
            })
        };

        let behavior = opts.and_then(|o| o.behavior).unwrap_or(self.st().settings.scroll_behavior);
        if behavior == ScrollBehaviorKind::Smooth {
            if let Some(p) = placement() {
                self.animate_to(p.camera, p.bounds, None);
            }
        } else {
            self.stabilized(|| {
                if let Some(p) = placement() {
                    self.set_cam(p.camera, p.bounds);
                }
            });
        }
    }

    /// Step by the navigation unit: the item when it fits, else the page.
    pub(crate) fn step(&self, direction: isize, opts: Option<GoToOptions>) {
        let cursor = self.st().cursor;
        let Some(item) = self.cursor_item() else { return };
        let g = self.grouping();
        if self.is_fit_all() || self.fits(Self::item_rect(&item), self.cam().zoom) {
            let idx = (self.item_index_of_page(cursor) as isize + direction)
                .min(g.grouping.len() as isize - 1)
                .max(0) as usize;
            self.go_to_target(g.grouping[idx][0] as isize, opts);
        } else {
            self.go_to_target(cursor as isize + direction, opts);
        }
    }
}

// ── the area's api ────────────────────────────────────────────────────────────

impl StageCapability {
    /// Navigate by zero-based display index, clamped into range. A
    /// viewpoint restores; otherwise a fresh arrival at arrivalAlign.
    pub fn go_to_page_index(&self, page_index: isize, opts: Option<GoToOptions>) {
        self.go_to_target(page_index, opts);
    }

    /// Navigate to a page by identity. A page the document does not have is
    /// a no-op.
    pub fn go_to_page(&self, page: PageRef, opts: Option<GoToOptions>) {
        if let Some(index) = self.index_of_page(page) {
            self.go_to_target(index as isize, opts);
        }
    }

    pub fn go_to_first_page(&self, opts: Option<GoToOptions>) {
        self.go_to_target(0, opts);
    }

    pub fn go_to_last_page(&self, opts: Option<GoToOptions>) {
        let page_count = self.ctx.document().map_or(1, |d| d.page_count as isize);
        self.go_to_target((page_count - 1).max(0), opts);
    }

    pub fn can_go_next(&self) -> bool {
        (self.st().cursor as isize) < self.page_count_now() as isize - 1
    }

    pub fn can_go_previous(&self) -> bool {
        self.st().cursor > 0
    }

    /// Step forward by the navigation unit: the item when it fits the
    /// viewport, else the page.
    pub fn next_page(&self, opts: Option<GoToOptions>) {
        self.step(1, opts);
    }

    /// Step backward by the navigation unit.
    pub fn previous_page(&self, opts: Option<GoToOptions>) {
        self.step(-1, opts);
    }

    /// Back to page 0, placed fresh at the current zoom intent.
    pub fn reset_view(&self) {
        self.go_to_target(0, Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)));
    }
}
