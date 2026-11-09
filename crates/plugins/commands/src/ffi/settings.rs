//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

pub use crate::NotAllowedMode;
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CommandsSettings {
    pub disabled_categories: Vec<String>,
    pub disabled_commands: Vec<String>,
    pub not_allowed: NotAllowedMode,
    /// Open application data, including the kernel's undefined marker.
    pub app_state_json: String,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct CommandsSettingsPatch {
    #[uniffi(default = None)]
    pub disabled_categories: Option<Vec<String>>,
    #[uniffi(default = None)]
    pub disabled_commands: Option<Vec<String>>,
    #[uniffi(default = None)]
    pub not_allowed: Option<NotAllowedMode>,
    /// Omitted keeps the value; JSON null replaces it with null.
    #[uniffi(default = None)]
    pub app_state_json: Option<String>,
}
#[uniffi::export]
impl CommandsHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<CommandsSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open_settings(c)?.get_settings();
                Ok(CommandsSettings {
                    disabled_categories: (*settings.disabled_categories).clone(),
                    disabled_commands: (*settings.disabled_commands).clone(),
                    not_allowed: settings.not_allowed,
                    app_state_json: settings.app_state.to_string(),
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: CommandsSettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open_settings(c)?.update_settings(
                    &(crate::CommandsConfig {
                        disabled_categories: patch.disabled_categories,
                        disabled_commands: patch.disabled_commands,
                        not_allowed: patch.not_allowed,
                        app_state: patch
                            .app_state_json
                            .map(|text| {
                                serde_json::from_str(&text).map_err(|error| {
                                    invalid(format!("appState is not JSON: {error}"))
                                })
                            })
                            .transpose()?,
                        commands: None,
                    }),
                );
                Ok(())
            })
            .await
    }
    /// Restore the values supplied when the plugin was registered.
    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open_settings(c)?.reset_settings();
                Ok(())
            })
            .await
    }
}

fn open_settings(cap: &CommandsCapability) -> Result<&CommandsCapability, epdf_kernel::Error> {
    if cap.lifetime().aborted() {
        return Err(epdf_kernel::error::plugin_error(
            "instance-closed",
            "commands",
            "the workspace is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}
