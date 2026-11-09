//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionTextSettings {
    pub color: String,
    pub font_family: String,
    pub font_size: f64,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionSettings {
    pub fill: String,
    pub overlay_text: RedactionTextSettings,
}
#[uniffi::export]
impl RedactionHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<RedactionSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open(c)?.get_settings();
                Ok(RedactionSettings {
                    fill: settings.overlay.fill,
                    overlay_text: RedactionTextSettings {
                        color: settings.overlay.text.color,
                        font_family: settings.overlay.text.font_family,
                        font_size: settings.overlay.text.font_size,
                    },
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: RedactionConfig) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_settings(&(crate::RedactionConfig::from(patch)));
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
