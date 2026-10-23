//! `services/events.ts`: the stage's six events. Five of them are derived
//! in one state-change listener, from the state before and after each
//! committed change, so a new verb can never forget to announce a page,
//! zoom, camera, settings or viewport change. `motionEnded` is an
//! occurrence: the camera animation emits it where a tween or fling ends.

use epdf_kernel::Unsubscribe;
use epdf_kernel::geometry::PageRef;

use crate::prelude::*;
use crate::settings::{pick_settings, setting_eq};

impl StageCapability {
    /// `ctx.state.onChange(...)`, registered once at create.
    pub(crate) fn connect_events(&self) {
        let page_changed = self.inner.events.page_changed.clone();
        let zoom_changed = self.inner.events.zoom_changed.clone();
        let camera_changed = self.inner.events.camera_changed.clone();
        let settings_changed = self.inner.events.settings_changed.clone();
        let viewport_changed = self.inner.events.viewport_changed.clone();
        let ctx = self.ctx.cx.clone();
        self.ctx.on_state_change(move |next, previous| {
            if previous.camera_id != next.camera_id {
                camera_changed.emit(&StageCameraChangedEvent {
                    camera: next.camera,
                    order: next.camera_order,
                });
                if previous.camera.zoom != next.camera.zoom {
                    zoom_changed.emit(&StageZoomChangedEvent {
                        level: next.camera.zoom,
                        previous_level: previous.camera.zoom,
                        mode: StageZoomMode::of(next.settings.zoom),
                    });
                }
            }
            if previous.cursor != next.cursor {
                let page = ctx.document().and_then(|document| {
                    document
                        .pages
                        .as_array()
                        .and_then(|pages| pages.get(next.cursor))
                        .and_then(|page| page.get("ref"))
                        .and_then(PageRef::from_json)
                });
                page_changed.emit(&StagePageChangedEvent {
                    page,
                    page_index: next.cursor,
                    previous_page_index: previous.cursor,
                });
            }
            if previous.viewport_writes != next.viewport_writes {
                viewport_changed.emit(&StageViewportChangedEvent { size: next.viewport });
            }
            let changed: Vec<SettingKey> = SETTING_KEYS
                .iter()
                .copied()
                .filter(|&key| !setting_eq(&previous.settings, &next.settings, key))
                .collect();
            if !changed.is_empty() {
                settings_changed.emit(&StageSettingsChangedEvent {
                    settings: pick_settings(&next.settings),
                    changed,
                });
            }
            Ok(())
        });
    }

    // ── the hooks ──

    /// The cursor page changed.
    pub fn on_page_changed(
        &self,
        listener: impl Fn(&StagePageChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.page_changed.on(listener)
    }

    /// The camera's zoom level changed.
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

    /// One or more settings changed value; `changed` names them.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&StageSettingsChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.settings_changed.on(listener)
    }

    /// The host reported a new viewport size.
    pub fn on_viewport_changed(
        &self,
        listener: impl Fn(&StageViewportChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.events.viewport_changed.on(listener)
    }
}
