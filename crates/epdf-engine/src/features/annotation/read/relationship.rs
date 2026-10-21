//! `/IRT` and `/RT`: the edge from a reply to what it replies to.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readAnnotationRelationship.ts`.
//!
//! The edge is flat. A reply is a first-class annotation in the page list that
//! happens to name a parent, not a child nested inside one, so a comment
//! sidebar composes threads from these edges rather than receiving a tree.
//!
//! Two invariants hold together and neither survives alone:
//!
//! - `reply_type` is `Some` exactly when `in_reply_to` is. `/RT` read without
//!   `/IRT` is meaningless, and PDFium will answer anyway: its
//!   `EPDFAnnot_GetReplyType` reports `/RT`'s own default, so an annotation
//!   that replies to nothing would come back saying `reply`.
//! - A parent with neither an object number nor an `/NM` is reported as no
//!   relationship at all. `/IRT` is by construction an indirect reference, so
//!   this cannot happen in a well-formed file; in a malformed one the choice
//!   is between an unaddressable parent and no parent, and an address a client
//!   cannot dereference is worse than an absent one.

use pdfium::Annotation;

use super::dictionary_read;
use crate::error::EngineError;
use crate::features::annotation::vocabulary::AnnotationReplyType;
use crate::identity::{AnnotationRef, PageObjectNumber};

/// The `/IRT` edge, or nothing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotationRelationship {
    pub in_reply_to: Option<AnnotationRef>,
    pub reply_type: Option<AnnotationReplyType>,
}

/// Read the edge.
///
/// `page` is this annotation's page, and it is also the parent's: §12.5.6.2
/// requires an `/IRT` target to live on the same page, so the parent's address
/// is built from the same page number rather than from a second lookup.
pub fn read_annotation_relationship(
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
) -> Result<AnnotationRelationship, EngineError> {
    let Some(parent) = dictionary_read("IRT", annotation.linked_annotation("IRT"))? else {
        return Ok(AnnotationRelationship::default());
    };

    let Some(in_reply_to) = parent_ref(&parent, page)? else {
        return Ok(AnnotationRelationship::default());
    };

    Ok(AnnotationRelationship {
        in_reply_to: Some(in_reply_to),
        reply_type: Some(AnnotationReplyType::from_code(annotation.reply_type_code())),
    })
}

/// The parent's address, by the same precedence [`super::identity`] uses
/// minus its last step. There is no index fallback: an index would address
/// the parent by its position in a list this read is not walking.
fn parent_ref(
    parent: &Annotation<'_>,
    page: PageObjectNumber,
) -> Result<Option<AnnotationRef>, EngineError> {
    let object_number = parent.object_number();
    if object_number > 0 {
        return Ok(Some(AnnotationRef::ObjectNumber { page, annot: object_number }));
    }
    let nm = dictionary_read("NM", parent.string_value("NM"))?;
    Ok(nm.filter(|name| !name.is_empty()).map(|nm| AnnotationRef::Nm { page, nm }))
}

/// The key of the ref a record field holds (`annotationKey`), or `None`
/// when it holds no ref.
fn key_at(value: Option<&serde_json::Value>) -> Option<String> {
    let reference: AnnotationRef = serde_json::from_value(value?.clone()).ok()?;
    Some(reference.key())
}

/// An annotation and everything deleted with it (`deletedWith`,
/// `core/src/annotation/relationships.ts`), from a page's records as a
/// listing reads them: every annotation whose `reply.to` leads to it
/// (replies, their replies, grouped parts, review states) and every popup of
/// these. Children come before their parent and a popup before the
/// annotation it shows, so the annotation itself is last. Empty when
/// `reference` names nothing in `annotations`.
pub fn deleted_with(
    annotations: &[serde_json::Value],
    reference: &AnnotationRef,
) -> Vec<serde_json::Value> {
    let keys: Vec<Option<String>> =
        annotations.iter().map(|annotation| key_at(annotation.get("ref"))).collect();
    let find =
        |key: Option<String>| key.and_then(|key| keys.iter().position(|k| *k == Some(key.clone())));
    let Some(target) = find(Some(reference.key())) else { return Vec::new() };
    let is_popup = |annotation: &serde_json::Value| {
        annotation.get("subtype").and_then(|s| s.as_str()) == Some("popup")
    };
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); annotations.len()];
    let mut popups: Vec<Vec<usize>> = vec![Vec::new(); annotations.len()];
    for (at, annotation) in annotations.iter().enumerate() {
        if let Some(parent) = find(key_at(annotation.pointer("/reply/to")))
            && parent != at
        {
            children[parent].push(at);
        }
        // A popup belongs to the annotation it shows, by its `/Parent` or by
        // that annotation's `/Popup`.
        if is_popup(annotation)
            && let Some(shows) = find(key_at(annotation.get("parent")))
            && shows != at
        {
            popups[shows].push(at);
        }
        if let Some(own) = find(key_at(annotation.get("popup")))
            && own != at
            && is_popup(&annotations[own])
        {
            popups[at].push(own);
        }
    }
    let mut members = Vec::new();
    let mut seen = vec![false; annotations.len()];
    fn visit(
        at: usize,
        children: &[Vec<usize>],
        popups: &[Vec<usize>],
        seen: &mut [bool],
        members: &mut Vec<usize>,
    ) {
        if seen[at] {
            return;
        }
        seen[at] = true;
        for child in &children[at] {
            visit(*child, children, popups, seen, members);
        }
        for popup in &popups[at] {
            if seen[*popup] {
                continue;
            }
            seen[*popup] = true;
            members.push(*popup);
        }
        members.push(at);
    }
    visit(target, &children, &popups, &mut seen, &mut members);
    members.into_iter().map(|at| annotations[at].clone()).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn object(annot: u32) -> Value {
        json!({ "kind": "objectNumber", "page": { "kind": "objectNumber", "objectNumber": 3 },
            "objectNumber": annot })
    }

    fn born(base_index: u32) -> Value {
        json!({ "kind": "baseIndex", "page": { "kind": "objectNumber", "objectNumber": 3 },
            "baseIndex": base_index })
    }

    fn names(members: &[Value]) -> Vec<Value> {
        members.iter().map(|member| member["ref"].clone()).collect()
    }

    #[test]
    fn a_thread_goes_whole_children_before_their_parent_and_a_popup_before_what_it_shows() {
        let annotations = [
            json!({ "ref": object(5), "subtype": "text", "popup": object(6) }),
            json!({ "ref": object(6), "subtype": "popup", "parent": object(5) }),
            json!({ "ref": object(7), "subtype": "text", "reply": { "to": object(5) } }),
            // A reply named by the birth name of what it replies to.
            json!({ "ref": object(8), "subtype": "text", "reply": { "to": object(7) } }),
            json!({ "ref": born(4), "subtype": "square" }),
            json!({ "ref": object(9), "subtype": "popup", "parent": born(4) }),
        ];
        let reference: AnnotationRef = serde_json::from_value(object(5)).unwrap();
        assert_eq!(
            names(&deleted_with(&annotations, &reference)),
            [object(8), object(7), object(6), object(5)]
        );
        let reference: AnnotationRef = serde_json::from_value(born(4)).unwrap();
        assert_eq!(names(&deleted_with(&annotations, &reference)), [object(9), born(4)]);
        let reference: AnnotationRef = serde_json::from_value(object(7)).unwrap();
        assert_eq!(names(&deleted_with(&annotations, &reference)), [object(8), object(7)]);
    }

    #[test]
    fn a_ref_that_names_nothing_takes_nothing_and_a_cycle_ends() {
        let annotations = [
            json!({ "ref": object(5), "subtype": "text", "reply": { "to": object(7) } }),
            json!({ "ref": object(7), "subtype": "text", "reply": { "to": object(5) } }),
        ];
        let nothing: AnnotationRef = serde_json::from_value(object(99)).unwrap();
        assert!(deleted_with(&annotations, &nothing).is_empty());
        let reference: AnnotationRef = serde_json::from_value(object(5)).unwrap();
        assert_eq!(names(&deleted_with(&annotations, &reference)), [object(7), object(5)]);
    }
}
