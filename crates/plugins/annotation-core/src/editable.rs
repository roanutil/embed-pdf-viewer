//! `editable.ts`: what people may change by hand, the session's `editable`
//! rule, beside the flags.

use crate::engine::measure::is_dimension;
use crate::flags::{annot_contents_editable, annot_deletable, annot_transformable};
use crate::json::{self, Object};
use crate::types::{EditableKinds, ModelAnnotation};

/// `editableKindOf(annotation)`: a measurement's intent, else its subtype.
pub fn editable_kind_of(annotation: &Object) -> String {
    let subtype = json::get_str(annotation, "subtype").unwrap_or("");
    let intent = json::get_str(annotation, "intent");
    if is_dimension(subtype, intent) { intent.unwrap_or("").into() } else { subtype.into() }
}

/// `isEditableByHand(model, annotation)`.
pub fn is_editable_by_hand(editable: &EditableKinds, annotation: &Object) -> bool {
    match editable {
        EditableKinds::All(all) => *all,
        EditableKinds::Kinds(kinds) => {
            let kind = editable_kind_of(annotation);
            kinds.contains(&kind)
        }
    }
}

pub fn is_transformable_by_hand(editable: &EditableKinds, record: &ModelAnnotation) -> bool {
    annot_transformable(record) && is_editable_by_hand(editable, &record.annotation)
}

pub fn is_deletable_by_hand(editable: &EditableKinds, record: &ModelAnnotation) -> bool {
    annot_deletable(record) && is_editable_by_hand(editable, &record.annotation)
}

pub fn is_contents_editable_by_hand(editable: &EditableKinds, record: &ModelAnnotation) -> bool {
    annot_contents_editable(record) && is_editable_by_hand(editable, &record.annotation)
}
