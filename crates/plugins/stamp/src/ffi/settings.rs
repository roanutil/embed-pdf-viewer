//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampSettings {
    pub preview_width: f64,
    pub dynamic: bool,
}
#[uniffi::export]
impl StampHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<StampSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open_settings(c)?.get_settings();
                Ok(StampSettings {
                    preview_width: settings.preview_width,
                    dynamic: settings.dynamic,
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: StampConfig) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open_settings(c)?.update_settings(
                    &(crate::StampConfig {
                        preview_width: patch.preview_width,
                        dynamic: patch.dynamic,
                        asset_engine: None,
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

fn open_settings(cap: &StampCapability) -> Result<&StampCapability, epdf_kernel::Error> {
    if cap.lifetime().aborted() {
        return Err(epdf_kernel::error::plugin_error(
            "instance-closed",
            "stamp",
            "the workspace is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}
