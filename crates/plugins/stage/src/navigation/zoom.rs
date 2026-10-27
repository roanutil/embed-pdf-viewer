//! `navigation/zoom.ts`: the zoom verbs. Pointer-less zooms magnify around
//! the zoomAlign focal point (pinch and wheel pass their own pointer to
//! `zoom_around`); fit modes are zoom intents written through the settings;
//! the double-tap ladder ascends reading postures.

use crate::model::patch_settings;
use crate::prelude::*;

/// The zoom factor of one zoomIn or zoomOut step.
const ZOOM_STEP: f64 = 1.2;

impl StageCapability {
    fn zoom_align_point(&self) -> Point {
        self.align_point(self.st().settings.zoom_align, self.viewport())
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
        let automatic = sc::resolve_zoom(
            ZoomSpec::Mode { mode: ZoomMode::Automatic },
            fit,
            self.viewport(),
            self.padding(),
        );
        let fit_width = sc::resolve_zoom(
            ZoomSpec::Mode { mode: ZoomMode::FitWidth },
            fit,
            self.viewport(),
            self.padding(),
        );
        let mut sorted = [automatic, fit_width, js::min(automatic * 2.5, sc::ZOOM_MAX)];
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

    /// Zoom in one step around the zoomAlign point.
    pub fn zoom_in(&self) -> Result<(), Error> {
        self.zoom_around(self.zoom_align_point(), ZOOM_STEP)
    }

    /// Zoom out one step around the zoomAlign point.
    pub fn zoom_out(&self) -> Result<(), Error> {
        self.zoom_around(self.zoom_align_point(), 1.0 / ZOOM_STEP)
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
