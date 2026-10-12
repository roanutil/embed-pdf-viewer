//! `navigation/zoom.ts`: the zoom verbs. Pointer-less zooms magnify around
//! the zoomAlign focal point (pinch and wheel pass their own pointer to
//! `zoom_around`); fit modes are zoom intents written through the settings;
//! the double-tap ladder ascends reading postures.

use crate::prelude::*;

impl StageCapability {
    /// The double-tap ladder: automatic, fit-width, 2.5 times automatic,
    /// wrapping; off the ladder it resets to the base rung. `Err` where
    /// the TypeScript throws: no item under the tapped point.
    pub fn double_tap_zoom(&self, pt: Point) -> Result<(), Error> {
        self.cancel_anim();
        let sc0 = self.build_scene();
        if sc0.item_count == 0 {
            return Ok(());
        }
        let item = if self.paged() {
            &sc0.items[0]
        } else {
            &sc0.items[self.item_index_of_page(self.st().cursor)]
        };
        let fit = self.fit_box(item);
        let auto = sc::resolve_zoom(
            ZoomSpec::Mode { mode: ZoomMode::Automatic },
            fit,
            self.vp(),
            self.pad(),
        );
        let fit_w = sc::resolve_zoom(
            ZoomSpec::Mode { mode: ZoomMode::FitWidth },
            fit,
            self.vp(),
            self.pad(),
        );
        let mut sorted = [auto, fit_w, js::min(auto * 2.5, sc::ZOOM_MAX)];
        // `Array#sort` with `(a, b) => a - b`: stable, and a NaN comparison
        // reads as equal.
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut stops = Vec::new();
        for (i, &z) in sorted.iter().enumerate() {
            if i == 0 || z > sorted[i - 1] * 1.1 {
                stops.push(z);
            }
        }
        let near = |z: f64, stop: f64| z > stop / 1.1 && z < stop * 1.1;
        let zoom = self.cam().zoom;
        let target = match stops.iter().position(|&s| near(zoom, s)) {
            Some(on_rung) => stops[(on_rung + 1) % stops.len()],
            None => stops[0],
        };
        // The intent commits up front.
        self.dispatch(StageAction::Patch {
            patch: PartialSettings::zoom(ZoomSpec::Level { level: target }),
        });
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return Ok(());
        }
        let focal =
            sc::anchor_at_point(&scene, sc::to_world(self.cam(), pt)).map_err(stage_error)?;
        // A throw in a frame callback is uncaught in the TypeScript.
        self.animate_zoom_anchored(
            focal,
            pt,
            target,
            Some(Rc::new(|s: &Self| {
                let _ = s.sync_cursor_from_camera();
            })),
        );
        Ok(())
    }

    /// Magnify by 1.2 around the zoomAlign point.
    pub fn zoom_in(&self) -> Result<(), Error> {
        self.zoom_around(self.align_point(self.st().settings.zoom_align, self.vp()), 1.2)
    }

    pub fn zoom_out(&self) -> Result<(), Error> {
        self.zoom_around(self.align_point(self.st().settings.zoom_align, self.vp()), 1.0 / 1.2)
    }

    /// Set a fixed level (a bare number) or a fit mode. A bare level with
    /// `around` zooms holding that viewport point; without it the level is
    /// a zoom-intent update like a fit mode.
    pub fn zoom_to(&self, zoom: ZoomTarget, options: Option<ZoomToOptions>) -> Result<(), Error> {
        match zoom {
            ZoomTarget::Level(level) => match options.and_then(|o| o.around) {
                Some(around) => self.zoom_around(around, level / self.cam().zoom),
                None => self.update_settings(PartialSettings::zoom(ZoomSpec::Level { level })),
            },
            ZoomTarget::Spec(spec) => self.update_settings(PartialSettings::zoom(spec)),
        }
    }

    /// Multiply the zoom, holding a viewport point fixed (default: the
    /// zoomAlign point).
    pub fn zoom_by(&self, factor: f64, options: Option<ZoomToOptions>) -> Result<(), Error> {
        let around = match options.and_then(|o| o.around) {
            Some(around) => around,
            None => self.align_point(self.st().settings.zoom_align, self.vp()),
        };
        self.zoom_around(around, factor)
    }

    pub fn fit_width(&self) -> Result<(), Error> {
        self.update_settings(PartialSettings::zoom(ZoomSpec::Mode { mode: ZoomMode::FitWidth }))
    }

    pub fn fit_page(&self) -> Result<(), Error> {
        self.update_settings(PartialSettings::zoom(ZoomSpec::Mode { mode: ZoomMode::FitPage }))
    }

    /// Fit the whole scene (every page) in view.
    pub fn fit_all(&self) -> Result<(), Error> {
        self.update_settings(PartialSettings::zoom(ZoomSpec::Mode { mode: ZoomMode::FitAll }))
    }

    /// Fit width but never upscale past 100% (Adobe's "Automatic").
    pub fn fit_automatic(&self) -> Result<(), Error> {
        self.update_settings(PartialSettings::zoom(ZoomSpec::Mode { mode: ZoomMode::Automatic }))
    }

    /// Set the view rotation to an absolute quarter-turn: an
    /// anchor-preserving reframe.
    pub fn set_view_rotation(&self, rotation: PageRotation) -> Result<(), Error> {
        self.update_settings(PartialSettings {
            view_rotation: Some(rotation),
            ..Default::default()
        })
    }

    /// Rotate the view a quarter-turn: 90 clockwise, anything else counter.
    pub fn rotate_view_by(&self, delta: f64) -> Result<(), Error> {
        let turn = if delta == 90.0 { PageRotation::DEG_90 } else { PageRotation::DEG_270 };
        self.set_view_rotation(add_rotations(self.st().settings.view_rotation, turn))
    }
}
