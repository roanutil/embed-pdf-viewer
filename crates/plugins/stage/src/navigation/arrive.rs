//! `navigation/arrive.ts`: anchors and arrivals. The anchor is the durable
//! "what am I looking at": captured before a structural change and
//! re-applied after, one mechanism for layout, spread, zoom, resize and
//! restore. Navigation is canonical: `go_to_page(n)` always ends in the
//! same camera state whatever the zoom or the starting point.

use crate::model::{CameraObject, patch_settings, set_cursor};
use crate::prelude::*;

/// Where a placement lands and what it clamps against.
#[derive(Clone, Copy)]
struct Placement {
    camera: CameraObject,
    bounds: Rect,
}

impl StageCapability {
    /// Re-run a placement until the scene it placed against is the scene
    /// that results (at most three passes).
    pub(crate) fn stabilized(&self, place: impl Fn()) {
        for _ in 0..3 {
            let before = self.build_scene();
            place();
            if Rc::ptr_eq(&self.build_scene(), &before) {
                return;
            }
        }
    }

    pub(crate) fn apply_anchor(&self, anchor: Anchor, align: Option<AlignmentValue>) {
        self.stabilized(|| {
            let current = self.build_scene();
            if current.item_count == 0 {
                return;
            }
            // `current.items[current.itemOfPage(anchor.pageIndex)]`: the
            // anchor's page is clamped into the scene by stage-core.
            let Some(item_index) = current.item_of_page(anchor.page_index) else { return };
            let item = &current.items[item_index];
            let zoom = sc::resolve_zoom(
                self.st().settings.zoom,
                self.fit_box(Some(item)),
                self.viewport(),
                self.padding(),
            );
            // Resolved against the current viewport: a resize restores the
            // anchor to the new viewport's policy point.
            let at =
                self.align_point(align.unwrap_or(self.st().settings.anchor_align), self.viewport());
            let Ok(camera) =
                sc::camera_from_anchor(anchor, &current, self.viewport(), zoom, Some(at))
            else {
                return;
            };
            self.write_camera(CameraObject::zoom_xy(camera), self.bounds_for(item));
        });
    }

    /// Re-apply the view after a structural, zoom or viewport change:
    /// anchor-preserving, except under fit-all, which re-places.
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

    pub(crate) fn go_to_target(&self, page_index: isize, options: Option<GoToOptions>) {
        self.mark_cause(MotionCause::Programmatic);
        self.cancel_animation();
        let page_count = self.page_count();
        if page_count == 0 {
            return;
        }
        let target = page_index.clamp(0, page_count as isize - 1) as usize;
        self.update(|s| set_cursor(s, target));

        // The restore path (per-page view memory): the exact viewpoint
        // instead of a fresh placement.
        if let Some(viewpoint) = options.and_then(|o| o.viewpoint) {
            self.update(|s| patch_settings(s, &StageSettingsPatch::zoom(viewpoint.zoom)));
            self.apply_anchor(viewpoint.anchor, None);
            return;
        }

        // Placement against the current scene; `None` when there is nothing
        // to place.
        let placement = || -> Option<Placement> {
            let current = self.build_scene();
            if current.item_count == 0 {
                return None;
            }
            let item = if self.paged() {
                &current.items[0]
            } else {
                current.items.get(self.item_index_of_page(target))?
            };
            let zoom = sc::resolve_zoom(
                self.st().settings.zoom,
                self.fit_box(Some(item)),
                self.viewport(),
                self.padding(),
            );
            let subject = if self.is_fit_all() {
                self.scene_rect()
            } else if self.fits(Self::item_rect(item), zoom) {
                Self::item_rect(item)
            } else {
                Self::page_rect_of(item, target)
            };
            // The landing policy: a per-call override beats the setting,
            // and 'keep' pins that axis to the current camera.
            let setting = self.st().settings.arrival_align;
            let over = options.and_then(|o| o.arrival_align).unwrap_or_default();
            let landing_x = over.x.unwrap_or(setting.x);
            let landing_y = over.y.unwrap_or(setting.y);
            let align_of = |value: ArrivalAlignValue| match value {
                ArrivalAlignValue::Keep(_) => AlignValue::Named(Align::Start),
                ArrivalAlignValue::Align(align) => align,
            };
            let placed = sc::place_camera(
                subject,
                self.viewport(),
                zoom,
                self.padding(),
                AlignmentValue { x: align_of(landing_x), y: align_of(landing_y) },
                self.st().settings.direction,
            );
            // Arrivals are canonical even on an unbounded stage: the landing
            // clamps within the true bounds regardless.
            let bounds = self.bounds_for(item);
            let settled = sc::clamp_camera(
                placed,
                bounds,
                self.viewport(),
                CameraConstraint { bounded: true, ..self.constraint() },
            );
            let keep = |value: ArrivalAlignValue| matches!(value, ArrivalAlignValue::Keep(_));
            Some(Placement {
                camera: CameraObject::zoom_xy(Camera {
                    zoom,
                    x: if keep(landing_x) { self.camera().x } else { settled.x },
                    y: if keep(landing_y) { self.camera().y } else { settled.y },
                }),
                bounds,
            })
        };

        let behavior =
            options.and_then(|o| o.behavior).unwrap_or(self.st().settings.scroll_behavior);
        if behavior == ScrollBehaviorKind::Smooth {
            if let Some(landing) = placement() {
                self.animate_to(landing.camera, landing.bounds, None);
            }
        } else {
            // An instant placement converges with the scene.
            self.stabilized(|| {
                if let Some(landing) = placement() {
                    self.write_camera(landing.camera, landing.bounds);
                }
            });
        }
    }

    /// Step by the navigation unit: the item when it fits the viewport,
    /// else the page.
    fn step(&self, direction: isize, options: Option<GoToOptions>) {
        let Some(item) = self.cursor_item() else { return };
        let grouping = self.grouping();
        let groups = &grouping.grouping;
        if self.is_fit_all() || self.fits(Self::item_rect(&item), self.camera().zoom) {
            let index = (self.item_index_of_page(self.st().cursor) as isize + direction)
                .min(groups.len() as isize - 1)
                .max(0) as usize;
            self.go_to_target(groups[index][0] as isize, options);
        } else {
            self.go_to_target(self.st().cursor as isize + direction, options);
        }
    }

    /// Run a navigation verb now, or, before the first placement, when it
    /// happens (`whenPlaced`): the last call waits and lands instantly
    /// after the initial view.
    pub(crate) fn when_placed<O: Clone + 'static>(
        &self,
        options: Option<O>,
        instant: impl Fn(Option<O>) -> O + 'static,
        run: impl Fn(&StageCapability, Option<O>) + 'static,
    ) {
        if self.inner.placement.started.get() {
            run(self, options);
            return;
        }
        *self.inner.placement.pending.borrow_mut() =
            Some(Box::new(move |stage: &StageCapability| run(stage, Some(instant(options)))));
    }

    fn when_placed_go(
        &self,
        options: Option<GoToOptions>,
        run: impl Fn(&StageCapability, Option<GoToOptions>) + 'static,
    ) {
        self.when_placed(
            options,
            |options| GoToOptions {
                behavior: Some(ScrollBehaviorKind::Instant),
                ..options.unwrap_or_default()
            },
            run,
        );
    }
}

// ── the area's api ──

impl StageCapability {
    /// Go to a page: its `PageRef`, or its display index (an index past
    /// either end goes to that end). The page lands by the arrival rule;
    /// `viewpoint` restores a saved spot instead. A ref the document does
    /// not have, or an index that isn't whole, is a no-op.
    pub fn go_to_page(&self, page: impl Into<PageTarget>, options: Option<GoToOptions>) {
        if let Some(index) = self.index_of_target(page.into()) {
            self.when_placed_go(options, move |stage, go| stage.go_to_target(index, go));
        }
    }

    pub fn go_to_first_page(&self, options: Option<GoToOptions>) {
        self.when_placed_go(options, |stage, go| stage.go_to_target(0, go));
    }

    pub fn go_to_last_page(&self, options: Option<GoToOptions>) {
        self.when_placed_go(options, |stage, go| {
            let page_count = stage.ctx.document().map_or(1, |d| d.page_count as isize);
            stage.go_to_target((page_count - 1).max(0), go);
        });
    }

    /// The cursor is before the last page.
    pub fn can_go_next(&self) -> bool {
        (self.st().cursor as isize) < self.page_count() as isize - 1
    }

    /// The cursor is after the first page.
    pub fn can_go_previous(&self) -> bool {
        self.st().cursor > 0
    }

    /// Step forward by the navigation unit (the item if it fits the
    /// viewport, else the page).
    pub fn next_page(&self, options: Option<GoToOptions>) {
        self.when_placed_go(options, |stage, go| stage.step(1, go));
    }

    pub fn previous_page(&self, options: Option<GoToOptions>) {
        self.when_placed_go(options, |stage, go| stage.step(-1, go));
    }

    /// Back to the first page at the current zoom intent.
    pub fn reset_view(&self) {
        self.go_to_target(0, Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)));
    }
}
