//! `navigation/zoom.ts`: the zoom verbs. Pointer-less zooms magnify around
//! the zoomAlign focal point (pinch and wheel pass their own pointer to
//! `zoom_around`); fit modes are zoom intents written through the settings;
//! zoom_in and zoom_out step through round levels; the double-tap ladder
//! ascends reading postures. Every zoom stays inside the minZoom/maxZoom
//! limits.

use crate::model::patch_settings;
use crate::prelude::*;

/// The levels zoom_in and zoom_out land on, from ZOOM_MIN to ZOOM_MAX:
/// round percentages, about 1.25 to 1.5 times apart. From a level between
/// two of them (a fit, a pinch), a step goes to the next one, so 137% zooms
/// in to 150%.
const ZOOM_STEPS: [f64; 23] = [
    0.05,
    0.1,
    0.15,
    0.2,
    0.25,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    0.75,
    1.0,
    1.25,
    1.5,
    2.0,
    3.0,
    4.0,
    6.0,
    8.0,
    12.0,
    16.0,
    24.0,
    32.0,
    48.0,
    64.0,
];
/// The smallest change a step makes: a level within 5% of a round level
/// steps past it (98% zooms in to 125%, not 100%).
const MIN_STEP: f64 = 1.05;
/// Float slack for comparing a zoom with a limit it was clamped to.
const EPSILON: f64 = 1e-9;

/// The next round level above `zoom`, or `limits.max` when that comes first.
fn step_in(zoom: f64, limits: sc::ZoomLimits) -> f64 {
    let level = ZOOM_STEPS.iter().copied().find(|&level| level > zoom * MIN_STEP);
    sc::clamp_zoom(level.unwrap_or(limits.max), limits)
}

/// The next round level below `zoom`, or `limits.min` when that comes first.
fn step_out(zoom: f64, limits: sc::ZoomLimits) -> f64 {
    let level = ZOOM_STEPS.iter().rev().copied().find(|&level| level < zoom / MIN_STEP);
    sc::clamp_zoom(level.unwrap_or(limits.min), limits)
}

impl StageCapability {
    fn zoom_align_point(&self) -> Point {
        self.align_point(self.st().settings.zoom_align, self.viewport())
    }

    /// A step to `target`, around `around` (default: the zoomAlign point);
    /// none when the limit holds the zoom.
    fn step_to(&self, target: f64, around: Option<ViewportPoint>) -> Result<(), Error> {
        let around = match around {
            Some(around) => around,
            None => self.zoom_align_point(),
        };
        // `target !== camera().zoom`.
        if target != self.camera().zoom {
            self.zoom_around(around, target / self.camera().zoom)?;
        }
        Ok(())
    }

    /// The touch double-tap zoom ladder around a viewport point: automatic,
    /// fit-width, 2.5 times automatic, wrapping; off the ladder it resets
    /// to the base rung. `Err` where the TypeScript throws: no item under
    /// the tapped point.
    pub fn double_tap_zoom(&self, point: ViewportPoint) -> Result<(), Error> {
        self.cancel_animation();
        let before = self.build_scene();
        if before.item_count == 0 {
            return Ok(());
        }
        let item = if self.paged() {
            before.items.first()
        } else {
            before.items.get(self.item_index_of_page(self.st().cursor))
        };
        let fit = self.fit_box(item);
        let limits = self.zoom_limits();
        let automatic = sc::resolve_zoom_within(
            ZoomSpec::Mode { mode: ZoomMode::Automatic },
            fit,
            self.viewport(),
            self.padding(),
            limits,
        );
        let fit_width = sc::resolve_zoom_within(
            ZoomSpec::Mode { mode: ZoomMode::FitWidth },
            fit,
            self.viewport(),
            self.padding(),
            limits,
        );
        let mut sorted = [automatic, fit_width, sc::clamp_zoom(automatic * 2.5, limits)];
        // `Array#sort` with `(left, right) => left - right`: stable, and a
        // NaN comparison reads as equal.
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut stops = Vec::new();
        for (index, &zoom) in sorted.iter().enumerate() {
            if index == 0 || zoom > sorted[index - 1] * 1.1 {
                stops.push(zoom);
            }
        }
        // The same ±10% band the dedupe uses.
        let near = |zoom: f64, stop: f64| zoom > stop / 1.1 && zoom < stop * 1.1;
        let current = self.camera().zoom;
        let target = match stops.iter().position(|&stop| near(current, stop)) {
            Some(on_rung) => stops[(on_rung + 1) % stops.len()],
            None => stops[0],
        };
        // The intent commits up front.
        self.update(|s| {
            patch_settings(s, &StageSettingsPatch::zoom(ZoomSpec::Level { level: target }))
        });
        // Wrapped grids re-layout on the intent change: anchor the focal
        // point against the scene that will be on screen.
        let after = self.build_scene();
        if after.item_count == 0 {
            return Ok(());
        }
        let focal =
            sc::anchor_at_point(&after, sc::to_world(self.camera(), point)).map_err(stage_error)?;
        // The focal-anchored tween keeps the tapped point still. A throw in
        // a frame callback is uncaught in the TypeScript.
        self.animate_zoom_anchored(
            focal,
            point,
            target,
            Some(Rc::new(|stage: &StageCapability| {
                let _ = stage.sync_cursor_from_camera();
            })),
        )
    }

    /// Zoom in one step, holding a viewport point fixed (default: the
    /// zoomAlign point): to the next round level (100%, 125%, 150%, 200%,
    /// ...), or `max_zoom` when that comes first.
    pub fn zoom_in(&self, options: Option<ZoomToOptions>) -> Result<(), Error> {
        let target = step_in(self.camera().zoom, self.zoom_limits());
        self.step_to(target, options.and_then(|o| o.around))
    }

    /// Zoom out one step, holding a viewport point fixed (default: the
    /// zoomAlign point): the round levels down, to `min_zoom`.
    pub fn zoom_out(&self, options: Option<ZoomToOptions>) -> Result<(), Error> {
        let target = step_out(self.camera().zoom, self.zoom_limits());
        self.step_to(target, options.and_then(|o| o.around))
    }

    /// The zoom is below `max_zoom`, so `zoom_in` moves.
    pub fn can_zoom_in(&self) -> bool {
        self.camera().zoom < self.zoom_limits().max * (1.0 - EPSILON)
    }

    /// The zoom is above `min_zoom`, so `zoom_out` moves.
    pub fn can_zoom_out(&self) -> bool {
        self.camera().zoom > self.zoom_limits().min * (1.0 + EPSILON)
    }

    /// Set a fixed level (a bare number) or a fit mode. A bare level with
    /// `around` zooms holding that viewport point; without it the level is
    /// a zoom-intent update like a fit mode.
    pub fn zoom_to(&self, zoom: ZoomTarget, options: Option<ZoomToOptions>) -> Result<(), Error> {
        match zoom {
            ZoomTarget::Level(level) => match options.and_then(|o| o.around) {
                Some(around) => self.zoom_around(around, level / self.camera().zoom),
                None => self.update_settings(StageSettingsPatch::zoom(ZoomSpec::Level { level })),
            },
            ZoomTarget::Spec(spec) => self.update_settings(StageSettingsPatch::zoom(spec)),
        }
    }

    /// Multiply the zoom, holding a viewport point fixed (default: the
    /// zoomAlign point).
    pub fn zoom_by(&self, factor: f64, options: Option<ZoomToOptions>) -> Result<(), Error> {
        let around = match options.and_then(|o| o.around) {
            Some(around) => around,
            None => self.zoom_align_point(),
        };
        self.zoom_around(around, factor)
    }

    /// Zoom so the page width fills the viewport.
    pub fn fit_width(&self) -> Result<(), Error> {
        self.update_settings(StageSettingsPatch::zoom(ZoomSpec::Mode { mode: ZoomMode::FitWidth }))
    }

    /// Zoom so the whole page fits the viewport.
    pub fn fit_page(&self) -> Result<(), Error> {
        self.update_settings(StageSettingsPatch::zoom(ZoomSpec::Mode { mode: ZoomMode::FitPage }))
    }

    /// Fit the whole scene (every page) in view.
    pub fn fit_all(&self) -> Result<(), Error> {
        self.update_settings(StageSettingsPatch::zoom(ZoomSpec::Mode { mode: ZoomMode::FitAll }))
    }

    /// Fit width but never upscale past 100% (Adobe's "Automatic").
    pub fn fit_automatic(&self) -> Result<(), Error> {
        self.update_settings(StageSettingsPatch::zoom(ZoomSpec::Mode { mode: ZoomMode::Automatic }))
    }

    /// Set the lens's view rotation: an anchor-preserving reframe.
    pub fn set_view_rotation(&self, rotation: PageRotation) -> Result<(), Error> {
        self.update_settings(StageSettingsPatch {
            view_rotation: Some(rotation),
            ..Default::default()
        })
    }

    /// Turn the view rotation a quarter clockwise (90) or counter-clockwise
    /// (anything else, as the TypeScript's `delta === 90 ? 90 : 270`).
    pub fn rotate_view_by(&self, delta: f64) -> Result<(), Error> {
        let turn = if delta == 90.0 { PageRotation::DEG_90 } else { PageRotation::DEG_270 };
        self.set_view_rotation(add_rotations(self.st().settings.view_rotation, turn))
    }
}
