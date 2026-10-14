//! The form records more than one plugin returns: how a field is addressed
//! and what kind of control it is. Signature fields are form fields, so the
//! signature plugin hands these back too.

use epdf_kernel::Error;
use serde_json::{Value, json};

/// How a field is addressed: by object number, or by fully qualified name
/// for a field the document stored as a direct object.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FormFieldRef {
    ObjectNumber { field_object_number: u32 },
    Fqn { name: String },
}

impl FormFieldRef {
    /// The plugin's (and the engine's) spelling.
    pub fn to_json(&self) -> Value {
        match self {
            FormFieldRef::ObjectNumber { field_object_number } => {
                json!({ "kind": "objectNumber", "fieldObjectNumber": field_object_number })
            }
            FormFieldRef::Fqn { name } => json!({ "kind": "fqn", "name": name }),
        }
    }

    pub fn from_json(value: &Value) -> Result<Self, Error> {
        let bad = || {
            Error::named("InvalidState", format!("[form] not a FormFieldRef: {value}"))
                .with("code", "invalid-state")
        };
        match value.get("kind").and_then(Value::as_str) {
            Some("objectNumber") => {
                let n = value.get("fieldObjectNumber").and_then(Value::as_f64).ok_or_else(bad)?;
                if n.fract() != 0.0 || n < 0.0 || n > u32::MAX as f64 {
                    return Err(bad());
                }
                Ok(FormFieldRef::ObjectNumber { field_object_number: n as u32 })
            }
            Some("fqn") => {
                let name = value.get("name").and_then(Value::as_str).ok_or_else(bad)?;
                Ok(FormFieldRef::Fqn { name: name.into() })
            }
            _ => Err(bad()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormFieldFamily {
    Text,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
    Pushbutton,
    Signature,
    /// A field type this build does not model.
    Unknown,
}

impl FormFieldFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            FormFieldFamily::Text => "text",
            FormFieldFamily::Checkbox => "checkbox",
            FormFieldFamily::Radio => "radio",
            FormFieldFamily::Combobox => "combobox",
            FormFieldFamily::Listbox => "listbox",
            FormFieldFamily::Pushbutton => "pushbutton",
            FormFieldFamily::Signature => "signature",
            FormFieldFamily::Unknown => "unknown",
        }
    }

    pub fn parse(name: &str) -> Self {
        match name {
            "text" => FormFieldFamily::Text,
            "checkbox" => FormFieldFamily::Checkbox,
            "radio" => FormFieldFamily::Radio,
            "combobox" => FormFieldFamily::Combobox,
            "listbox" => FormFieldFamily::Listbox,
            "pushbutton" => FormFieldFamily::Pushbutton,
            "signature" => FormFieldFamily::Signature,
            _ => FormFieldFamily::Unknown,
        }
    }
}
