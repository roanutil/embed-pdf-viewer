//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

pub use crate::{ActionTriggers, OpenSequence, TriggerGates};
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct ActionsScriptIdentity {
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub login_name: Option<String>,
    #[uniffi(default = None)]
    pub corporation: Option<String>,
    #[uniffi(default = None)]
    pub email: Option<String>,
}
impl From<epdf_acrojs::PartialScriptIdentity> for ActionsScriptIdentity {
    fn from(identity: epdf_acrojs::PartialScriptIdentity) -> Self {
        Self {
            name: identity.name,
            login_name: identity.login_name,
            corporation: identity.corporation,
            email: identity.email,
        }
    }
}
impl From<ActionsScriptIdentity> for epdf_acrojs::PartialScriptIdentity {
    fn from(identity: ActionsScriptIdentity) -> Self {
        Self {
            name: identity.name,
            login_name: identity.login_name,
            corporation: identity.corporation,
            email: identity.email,
        }
    }
}
/// A value override or a provider kept at the registration boundary.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionsScriptIdentitySetting {
    Value {
        identity: ActionsScriptIdentity,
    },
    /// Reading settings never executes the provider.
    RegisteredProvider,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionsJavaScriptSettings {
    pub enabled: bool,
    /// The scalar overrides, or a marker for a registered dynamic provider.
    pub identity: Option<ActionsScriptIdentitySetting>,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionsSettings {
    pub policy: ActionPolicy,
    pub triggers: ActionTriggers,
    pub open_sequence: OpenSequence,
    pub javascript: ActionsJavaScriptSettings,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct ActionsJavaScriptSettingsPatch {
    /// Applies to documents opened after the update, as in the Rust contract.
    #[uniffi(default = None)]
    pub enabled: Option<bool>,
    #[uniffi(default = None)]
    pub identity: Option<ActionsScriptIdentity>,
    #[uniffi(default = false)]
    pub clear_identity: bool,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct ActionsSettingsPatch {
    #[uniffi(default = None)]
    pub policy: Option<ActionPolicyPatch>,
    #[uniffi(default = None)]
    pub triggers: Option<TriggerGates>,
    #[uniffi(default = None)]
    pub open_sequence: Option<OpenSequence>,
    #[uniffi(default = None)]
    pub javascript: Option<ActionsJavaScriptSettingsPatch>,
}
impl From<ActionsSettingsPatch> for crate::ActionsSettingsPatch {
    fn from(patch: ActionsSettingsPatch) -> Self {
        Self {
            policy: patch.policy,
            triggers: patch.triggers,
            open_sequence: patch.open_sequence,
            javascript: patch.javascript.map(|javascript| crate::JavaScriptSettingsPatch {
                enabled: javascript.enabled,
                identity: if javascript.clear_identity {
                    Some(None)
                } else {
                    javascript
                        .identity
                        .map(|identity| Some(crate::ActionsScriptIdentity::Value(identity.into())))
                },
            }),
        }
    }
}
#[uniffi::export]
impl ActionsHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<ActionsSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open(c)?.get_settings();
                Ok(ActionsSettings {
                    policy: settings.policy,
                    triggers: settings.triggers,
                    open_sequence: settings.open_sequence,
                    javascript: ActionsJavaScriptSettings {
                        enabled: settings.javascript.enabled,
                        identity: settings.javascript.identity.map(|identity| match identity {
                            crate::ActionsScriptIdentity::Value(value) => {
                                ActionsScriptIdentitySetting::Value { identity: value.into() }
                            }
                            crate::ActionsScriptIdentity::Function(_) => {
                                ActionsScriptIdentitySetting::RegisteredProvider
                            }
                        }),
                    },
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: ActionsSettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_settings(&(crate::ActionsSettingsPatch::from(patch)));
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
