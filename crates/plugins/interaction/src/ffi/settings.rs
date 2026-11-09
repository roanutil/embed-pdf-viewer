//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct InteractionSettings {
    pub default_tool: String,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct InteractionSettingsPatch {
    #[uniffi(default = None)]
    pub default_tool: Option<String>,
}
#[uniffi::export]
impl InteractionHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<InteractionSettings, ViewerError> {
        self.bound
            .call(|c| {
                let settings = open(&c.cap)?.get_settings();
                Ok(InteractionSettings { default_tool: settings.default_tool })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(
        &self,
        patch: InteractionSettingsPatch,
    ) -> Result<(), ViewerError> {
        self.bound
            .call(move |c| {
                open(&c.cap)?.update_settings(
                    &(crate::InteractionConfig { default_tool: patch.default_tool, tools: None }),
                );
                Ok(())
            })
            .await
    }
    /// Restore the values supplied when the plugin was registered.
    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.bound
            .call(|c| {
                open(&c.cap)?.reset_settings();
                Ok(())
            })
            .await
    }
}
