//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormFillable {
    All,
    None,
    Kinds { families: Vec<FormFieldFamily> },
}
impl FormFillable {
    fn from_json(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Bool(true) => Ok(Self::All),
            Value::Bool(false) => Ok(Self::None),
            Value::Array(kinds) => kinds
                .iter()
                .map(|kind| {
                    kind.as_str()
                        .map(FormFieldFamily::parse)
                        .ok_or_else(|| invalid_state("invalid fillable kind".into()))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|families| Self::Kinds { families }),
            _ => Err(invalid_state("invalid fillable setting".into())),
        }
    }
    fn to_json(&self) -> Value {
        match self {
            Self::All => json!(true),
            Self::None => json!(false),
            Self::Kinds { families } => {
                json!(families.iter().map(|family| family.as_str()).collect::<Vec<_>>())
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Deserialize, uniffi::Record)]
pub struct FormFocusSettings {
    pub color: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, uniffi::Record)]
pub struct FormFieldSettings {
    pub border: Option<String>,
    pub background: String,
    pub color: String,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormSettings {
    pub validation: FormValidation,
    pub focus: FormFocusSettings,
    pub fields: FormFieldSettings,
    pub fillable: FormFillable,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFocusSettingsPatch {
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = false)]
    pub clear_color: bool,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFieldSettingsPatch {
    #[uniffi(default = None)]
    pub border: Option<String>,
    #[uniffi(default = false)]
    pub clear_border: bool,
    #[uniffi(default = None)]
    pub background: Option<String>,
    #[uniffi(default = None)]
    pub color: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormSettingsPatch {
    #[uniffi(default = None)]
    pub validation: Option<FormValidation>,
    #[uniffi(default = None)]
    pub focus: Option<FormFocusSettingsPatch>,
    #[uniffi(default = None)]
    pub fields: Option<FormFieldSettingsPatch>,
    #[uniffi(default = None)]
    pub fillable: Option<FormFillable>,
}
impl FormSettingsPatch {
    fn into_json(self) -> Value {
        let mut out = Map::new();
        if let Some(validation) = self.validation {
            out.insert(
                "validation".into(),
                json!(match validation {
                    FormValidation::Scripts => "scripts",
                    FormValidation::None => "none",
                }),
            );
        }
        if let Some(focus) = self.focus {
            let mut group = Map::new();
            if focus.clear_color {
                group.insert("color".into(), Value::Null);
            } else if let Some(color) = focus.color {
                group.insert("color".into(), json!(color));
            }
            out.insert("focus".into(), Value::Object(group));
        }
        if let Some(fields) = self.fields {
            let mut group = Map::new();
            if fields.clear_border {
                group.insert("border".into(), Value::Null);
            } else if let Some(border) = fields.border {
                group.insert("border".into(), json!(border));
            }
            if let Some(background) = fields.background {
                group.insert("background".into(), json!(background));
            }
            if let Some(color) = fields.color {
                group.insert("color".into(), json!(color));
            }
            out.insert("fields".into(), Value::Object(group));
        }
        if let Some(fillable) = self.fillable {
            out.insert("fillable".into(), fillable.to_json());
        }
        Value::Object(out)
    }
}
#[uniffi::export]
impl FormHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<FormSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open(c)?.get_settings();
                let get = |key: &str| {
                    settings
                        .get(key)
                        .cloned()
                        .ok_or_else(|| invalid_state(format!("missing form setting {key}")))
                };
                let validation = match get("validation")?.as_str() {
                    Some("scripts") => FormValidation::Scripts,
                    Some("none") => FormValidation::None,
                    _ => return Err(invalid_state("invalid form validation setting".into())),
                };
                Ok(FormSettings {
                    validation,
                    focus: decode(get("focus")?)?,
                    fields: decode(get("fields")?)?,
                    fillable: FormFillable::from_json(&get("fillable")?)?,
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: FormSettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_settings(&(patch.into_json()));
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
