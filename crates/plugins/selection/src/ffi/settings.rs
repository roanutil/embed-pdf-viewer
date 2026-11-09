//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionHandleSettings {
    pub color: Option<String>,
    pub shadow: String,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSettings {
    pub drag_threshold_px: f64,
    pub color: Option<String>,
    pub handles: SelectionHandleSettings,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SelectionHandleSettingsPatch {
    #[uniffi(default = None)]
    pub color: Option<String>,
    /// Clear overrides the supplied value; omission keeps the current color.
    #[uniffi(default = false)]
    pub clear_color: bool,
    #[uniffi(default = None)]
    pub shadow: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SelectionSettingsPatch {
    #[uniffi(default = None)]
    pub drag_threshold_px: Option<f64>,
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = false)]
    pub clear_color: bool,
    #[uniffi(default = None)]
    pub handles: Option<SelectionHandleSettingsPatch>,
}
impl From<SelectionSettingsPatch> for crate::SelectionConfig {
    fn from(patch: SelectionSettingsPatch) -> Self {
        Self {
            drag_threshold_px: patch.drag_threshold_px,
            color: if patch.clear_color { Some(None) } else { patch.color.map(Some) },
            handles: patch.handles.map(|handles| crate::SelectionHandleConfig {
                color: if handles.clear_color { Some(None) } else { handles.color.map(Some) },
                shadow: handles.shadow,
            }),
        }
    }
}
#[uniffi::export]
impl SelectionHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<SelectionSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open(c)?.get_settings();
                Ok(SelectionSettings {
                    drag_threshold_px: settings.drag_threshold_px,
                    color: settings.color,
                    handles: SelectionHandleSettings {
                        color: settings.handles.color,
                        shadow: settings.handles.shadow,
                    },
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: SelectionSettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_settings(&(crate::SelectionConfig::from(patch)));
                Ok(())
            })
            .await
    }
    /// Restore the values supplied when the plugin was registered.
    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.reset_settings();
                Ok(())
            })
            .await
    }
}
