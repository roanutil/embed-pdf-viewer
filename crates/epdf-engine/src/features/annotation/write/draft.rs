//! Who writes an annotation.
//!
//! G11. A create's data is upstream's v3 draft, checked and resolved in
//! [`super::prepare`], and an update's its patch, in [`super::patch`]. A
//! widget's place and look are [`super::widget`]'s. What is left here is
//! [`AnnotationActor`].

use serde::{Deserialize, Serialize};

/// Who is creating it.
///
/// Supplied by whoever authenticated the caller, never by the caller. The
/// engine writes `/T` from the display name and `/EMBD_Metadata` from the
/// ids, which is why none of the three is on the draft.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotationActor {
    pub display_name: Option<String>,
    pub user_id: Option<String>,
    pub group_id: Option<String>,
}
