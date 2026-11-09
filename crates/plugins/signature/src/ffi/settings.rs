//! Scalar signature settings; signer and trust ports remain registered resources.
use super::*;
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureSettings {
    pub mode: Option<SignatureMode>,
    pub allow_certify: bool,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SignatureSettingsPatch {
    #[uniffi(default = None)]
    pub mode: Option<SignatureMode>,
    #[uniffi(default = false)]
    pub clear_mode: bool,
    #[uniffi(default = None)]
    pub allow_certify: Option<bool>,
}
#[uniffi::export]
impl SignatureHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<SignatureSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open(c)?.get_settings();
                Ok(SignatureSettings {
                    mode: settings.mode.map(Into::into),
                    allow_certify: settings.allow_certify,
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: SignatureSettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_settings(
                    &(crate::SignatureConfig {
                        mode: if patch.clear_mode {
                            Some(None)
                        } else {
                            patch.mode.map(|mode| Some(mode.into()))
                        },
                        allow_certify: patch.allow_certify,
                        ..Default::default()
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
                open(c)?.reset_settings();
                Ok(())
            })
            .await
    }
}
