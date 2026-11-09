//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct I18nSettings {
    pub locale: Option<String>,
    pub fallback_locale: String,
    pub locales: Vec<LocalePack>,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct I18nSettingsPatch {
    #[uniffi(default = None)]
    pub locale: Option<String>,
    #[uniffi(default = false)]
    pub clear_locale: bool,
    #[uniffi(default = None)]
    pub fallback_locale: Option<String>,
    #[uniffi(default = None)]
    pub locales: Option<Vec<LocalePack>>,
}
#[uniffi::export]
impl I18nHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<I18nSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open_settings(c)?.get_settings();
                Ok(I18nSettings {
                    locale: settings.locale.clone(),
                    fallback_locale: settings.fallback_locale.clone(),
                    locales: settings.locales.iter().map(LocalePack::from).collect(),
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: I18nSettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open_settings(c)?.update_settings(
                    &(crate::I18nConfig {
                        locale: if patch.clear_locale {
                            Some(None)
                        } else {
                            patch.locale.map(Some)
                        },
                        fallback_locale: patch.fallback_locale,
                        locales: patch
                            .locales
                            .map(|packs| {
                                packs
                                    .into_iter()
                                    .map(Locale::try_from)
                                    .collect::<Result<Vec<_>, _>>()
                            })
                            .transpose()?,
                        loaders: None,
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

fn open_settings(cap: &I18nCapability) -> Result<&I18nCapability, epdf_kernel::Error> {
    if cap.lifetime().aborted() {
        return Err(epdf_kernel::error::plugin_error(
            "instance-closed",
            "i18n",
            "the workspace is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}
