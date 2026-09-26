//! `services/events.ts`: the six change hooks and the one place they fire.
//! Every state write goes through [`StageCapability::dispatch`], which
//! compares the state before and after, so a new verb cannot forget an
//! event.

use epdf_kernel::Unsubscribe;

use crate::prelude::*;
use crate::settings::pick_settings;

impl StageCapability {
    /// The diffing `ctx.dispatch` every area writes through.
    pub(crate) fn dispatch(&self, action: StageAction) {
        let before = self.st();
        // Only a CAMERA action replaces the camera object, and it keeps the
        // same one only when told so (see `Inner::camera_kept`).
        let camera_replaced =
            matches!(action, StageAction::Camera { .. }) && !self.inner.camera_kept.replace(false);
        let viewport_replaced = matches!(action, StageAction::Vp { .. });
        self.ctx.dispatch(action);
        let after = self.st();
        self.emit_diffs(&before, &after, camera_replaced, viewport_replaced);
    }

    fn emit_diffs(
        &self,
        before: &Rc<StageState>,
        after: &Rc<StageState>,
        camera_replaced: bool,
        viewport_replaced: bool,
    ) {
        if Rc::ptr_eq(before, after) {
            return;
        }
        let events = &self.inner.events;
        if camera_replaced {
            events.camera_changed.emit(&StageCameraChangedEvent { camera: after.camera });
            if before.camera.zoom != after.camera.zoom {
                events.zoom_changed.emit(&StageZoomChangedEvent {
                    level: after.camera.zoom,
                    previous_level: before.camera.zoom,
                    mode: StageZoomMode::of(after.settings.zoom),
                });
            }
        }
        if before.cursor != after.cursor {
            let page = self.ctx.document().and_then(|doc| {
                doc.pages.as_array().and_then(|pages| pages.get(after.cursor)).cloned()
            });
            events.page_changed.emit(&StagePageChangedEvent {
                page,
                page_index: after.cursor,
                previous_page_index: before.cursor,
            });
        }
        if viewport_replaced {
            events.viewport_changed.emit(&StageViewportChangedEvent { size: after.vp });
        }
        let changed: Vec<SettingKey> = SETTING_KEYS
            .iter()
            .copied()
            .filter(|&k| !crate::settings::setting_eq(&before.settings, &after.settings, k))
            .collect();
        if !changed.is_empty() {
            events.settings_changed.emit(&StageSettingsChangedEvent {
                settings: pick_settings(&after.settings),
                changed,
            });
        }
    }

    // ── the hooks ─────────────────────────────────────────────────────────────

    pub fn on_page_changed(
        &self,
        listener: impl Fn(&StagePageChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.page_changed.on(listener)
    }

    pub fn on_zoom_changed(
        &self,
        listener: impl Fn(&StageZoomChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.zoom_changed.on(listener)
    }

    /// Coalesced per camera write, not per frame.
    pub fn on_camera_changed(
        &self,
        listener: impl Fn(&StageCameraChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.camera_changed.on(listener)
    }

    /// A tween or fling settled, or was stopped.
    pub fn on_motion_ended(
        &self,
        listener: impl Fn(&StageMotionEndedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.motion_ended.on(listener)
    }

    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&StageSettingsChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.settings_changed.on(listener)
    }

    pub fn on_viewport_changed(
        &self,
        listener: impl Fn(&StageViewportChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.viewport_changed.on(listener)
    }
}
