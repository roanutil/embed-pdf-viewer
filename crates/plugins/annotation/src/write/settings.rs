//! `write/settings.ts`: the live-adjustable settings, snapping and the
//! selection chrome, both seeded by the registration config.

use epdf_annotation_core::{Msg, SnapSettings, SnapSettingsPatch};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::model::AnnotationAction;

impl AnnotationCapability {
    /// `getSnapSettings()`.
    pub fn get_snap_settings(&self) -> SnapSettings {
        self.model().snap.clone()
    }

    /// `updateSnapSettings(patch)`.
    pub fn update_snap_settings(&self, patch: SnapSettingsPatch) {
        self.commit(Msg::SetSnap { patch });
    }

    /// `getChromeSettings()`.
    pub fn get_chrome_settings(&self) -> ChromeSettings {
        (*self.state().chrome).clone()
    }

    /// `updateChromeSettings(patch)`.
    pub fn update_chrome_settings(&self, patch: ChromeSettingsPatch) {
        self.cx().dispatch(AnnotationAction::SetChrome { patch });
    }
}
