//! `types.ts`: a view is one pane, owning a tab strip of documents and one
//! active document. A document belongs to one view: views partition the
//! open documents.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use epdf_kernel::Token;
use serde::{Deserialize, Serialize};

use crate::capability::ViewManagerCapability;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: String,
    /// Documents in this pane, in tab order.
    pub document_ids: Vec<String>,
    /// The document currently shown in this pane.
    pub active_document_id: Option<String>,
}

/// The public read-only shape of a view: a copy.
pub type ViewInfo = View;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewManagerState {
    pub views: BTreeMap<String, View>,
    /// Display order of the panes.
    pub order: Vec<String>,
    pub focused_view_id: Option<String>,
    /// Monotonic counter for stable view ids.
    pub seq: u64,
}

/// Index arguments are JavaScript numbers: any `f64`, NaN included, which
/// canonical JSON spells `{"$number": "NaN"}`.
pub(crate) mod js_number {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::{Value, json};

    fn to_json(n: f64) -> Value {
        if n.is_nan() {
            json!({ "$number": "NaN" })
        } else if n.is_infinite() {
            json!({ "$number": if n > 0.0 { "Infinity" } else { "-Infinity" } })
        } else {
            json!(n)
        }
    }

    fn from_json(value: &Value) -> Option<f64> {
        value.as_f64().or_else(|| match value.get("$number")?.as_str()? {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        })
    }

    pub fn serialize<S: Serializer>(n: &f64, s: S) -> Result<S::Ok, S::Error> {
        to_json(*n).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        from_json(&Value::deserialize(d)?).ok_or_else(|| D::Error::custom("expected a number"))
    }

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(n: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
            n.map(to_json).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
            match Value::deserialize(d)? {
                Value::Null => Ok(None),
                value => {
                    from_json(&value).map(Some).ok_or_else(|| D::Error::custom("expected a number"))
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ViewManagerAction {
    #[serde(rename = "CREATE")]
    Create { view: View },
    #[serde(rename = "REMOVE")]
    Remove { id: String },
    #[serde(rename = "REORDER_VIEWS")]
    ReorderViews { order: Vec<String> },
    #[serde(rename = "SET_FOCUSED")]
    SetFocused { id: Option<String> },
    #[serde(rename = "SET_ACTIVE_DOC", rename_all = "camelCase")]
    SetActiveDoc { view_id: String, document_id: Option<String> },
    #[serde(rename = "ADD_DOC", rename_all = "camelCase")]
    AddDoc {
        view_id: String,
        document_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "js_number::option")]
        index: Option<f64>,
    },
    #[serde(rename = "REMOVE_DOC", rename_all = "camelCase")]
    RemoveDoc { view_id: String, document_id: String },
    #[serde(rename = "MOVE_DOC_WITHIN", rename_all = "camelCase")]
    MoveDocWithin {
        view_id: String,
        document_id: String,
        #[serde(with = "js_number")]
        to_index: f64,
    },
    #[serde(rename = "MOVE_DOC_BETWEEN", rename_all = "camelCase")]
    MoveDocBetween {
        from_view_id: String,
        to_view_id: String,
        document_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "js_number::option")]
        to_index: Option<f64>,
    },
    /// Reconcile views against the open-document set (effect-driven).
    #[serde(rename = "RECONCILE", rename_all = "camelCase")]
    Reconcile { open: Vec<String>, prefer_view_id: Option<String> },
}

pub static VIEW_MANAGER: LazyLock<Token<ViewManagerCapability>> =
    LazyLock::new(|| Token::new("view-manager"));
