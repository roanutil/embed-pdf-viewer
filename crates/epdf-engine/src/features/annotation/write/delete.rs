//! Removing an annotation with everything that goes with it.
//!
//! G11. Ports `AnnotationMutator.delete` and `deleteReleasedWidget`
//! (`services/src/features/annotations/AnnotationMutator.ts`) and
//! `internal/write/promoteInlineAnnotations.ts`. Operation:
//! `annotations.delete`, as a change op ([`crate::features::changes`]).
//!
//! A delete takes the annotation's thread with it in one change
//! (bd4976e4e): its replies and theirs, grouped parts, review states and
//! every popup ([`deleted_with`]). Nothing is renamed and nothing is
//! stamped: every annotation keeps the name it had (bc0057f3a), so a delete
//! answers only the names of what went.

use pdfium::Document;
use serde::Serialize;
use serde_json::Value;

use super::create::AnnotationMutationMeta;
use super::resolve::resolve_annotation_index_raw;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::kinds::is_widget_code;
use crate::features::annotation::read::relationship::deleted_with;
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::session::document::WriteStamp;

/// What a delete answers (`AnnotationDeleteResult`): nothing exists after
/// it, so only `meta`, whose `changed` names everything that went, the
/// annotation first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnotationDeleteResult {
    pub meta: AnnotationMutationMeta,
}

/// `promoteInlineAnnotations`: make each inline annotation of a page an
/// object, in place, before a write that moves entries of its `/Annots`: a
/// delete, a reorder, a flatten, a redaction. An inline annotation is named
/// by its position, which holds while no entry has moved; a layer records
/// those positions (its births) at the first promotion, so a `baseIndex`
/// keeps naming it. A page with nothing inline is left as it is. Answers
/// whether the page changed.
///
/// Upstream closes the page pool's idle pages after a promotion, so no
/// page loaded before it holds the inline entries. This engine's pool keeps
/// no page nobody holds, and every write here reads off the document.
pub fn promote_inline_annotations(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
) -> Result<bool, EngineError> {
    let moved = document.promote_inline_annotations_raw(page_index).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the inline annotations of page {} could not be promoted", page.get()),
        )
    })?;
    Ok(moved > 0)
}

/// What deleting `reference` deletes with it, as a listing of its page
/// reads them, the annotation last (`deletedWith` over `listOne`). Empty
/// when the page has no annotation `reference` names.
pub fn members_of(
    document: &Document,
    page_index: u32,
    reference: &AnnotationRef,
    cancel: &CancelToken,
) -> Result<Vec<Value>, EngineError> {
    let listed = crate::features::annotation::read::list::read_page_annotation_records(
        document,
        page_index,
        reference.page(),
        cancel,
    )?;
    let annotations: Vec<Value> = listed
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()
        .map_err(|e| EngineError::with_message(EngineErrorCode::Unknown, e.to_string()))?;
    Ok(deleted_with(&annotations, reference))
}

/// The ref a member record names.
pub fn member_ref(member: &Value) -> Result<AnnotationRef, EngineError> {
    serde_json::from_value(member.get("ref").cloned().unwrap_or_default()).map_err(|e| {
        EngineError::with_message(EngineErrorCode::Unknown, format!("a record names no ref: {e}"))
    })
}

/// `AnnotationMutator.delete`: delete `reference` with everything that goes
/// with it, in one change. `authorize` sees every member before the first
/// write (`authorizeAnnotationDelete`). A popup deleted without the
/// annotation it shows leaves that annotation, which stops naming it.
pub fn delete_annotation(
    document: &Document,
    page_index: u32,
    reference: &AnnotationRef,
    authorize: &dyn Fn(&[Value]) -> Result<(), EngineError>,
    stamp: &WriteStamp,
    cancel: &CancelToken,
) -> Result<AnnotationDeleteResult, EngineError> {
    cancel.check()?;
    let page = reference.page();
    let members = members_of(document, page_index, reference, cancel)?;
    if members.is_empty() {
        return Err(missing_annotation(reference));
    }
    authorize(&members)?;
    let refs: Vec<AnnotationRef> = members.iter().map(member_ref).collect::<Result<_, _>>()?;
    let positions: Vec<u32> = refs
        .iter()
        .map(|member| resolve_annotation_index_raw(document, page_index, member))
        .collect::<Result<_, _>>()?;
    let going: Vec<String> = refs.iter().map(AnnotationRef::key).collect();
    let mut kept_parents = Vec::new();
    for member in &members {
        if member.get("subtype").and_then(Value::as_str) != Some("popup") {
            continue;
        }
        let Some(parent) = member
            .get("parent")
            .and_then(|parent| serde_json::from_value::<AnnotationRef>(parent.clone()).ok())
        else {
            continue;
        };
        if going.contains(&parent.key()) {
            continue;
        }
        // A parent that isn't on the page is no parent to unlink.
        if let Ok(at) = resolve_annotation_index_raw(document, page_index, &parent) {
            kept_parents.push(at);
        }
    }

    // Apply boundary. Promotion keeps every position; then the highest
    // position first, so the positions still to go hold. The raw remove
    // also deletes the indirect object.
    cancel.check()?;
    promote_inline_annotations(document, page_index, page)?;
    let mut descending = positions.clone();
    descending.sort_unstable_by(|a, b| b.cmp(a));
    for index in descending {
        if !document.remove_annotation_raw(page_index, index) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("failed to remove annotation {index} on page {}", page.get()),
            ));
        }
    }
    // After the removals, at the position each kept parent now has.
    for parent in kept_parents {
        let before = positions.iter().filter(|index| **index < parent).count() as u32;
        if let Some(annotation) = document.raw_annotation(page_index, parent - before) {
            let _ = annotation.remove_key("Popup");
        }
    }
    // The annotation first, then what went with it.
    let changed = refs.into_iter().rev().collect();
    Ok(AnnotationDeleteResult { meta: AnnotationMutationMeta::of(stamp, page, changed) })
}

/// `deleteReleasedWidget`: remove a widget of the field `forms.delete` has
/// just unlinked from the field tree (`released_field`; 0 for a widget
/// `forms.deleteWidget` took out of its field). The cascade runs past that
/// write's apply boundary, so it takes no cancel token. A widget still
/// attached to another field is refused, as `assertNotAttachedWidget`
/// refuses it; the released field's own merged dictionary goes.
pub fn delete_released_widget(
    document: &Document,
    page_index: u32,
    reference: &AnnotationRef,
    released_field: u32,
) -> Result<(), EngineError> {
    let index = resolve_annotation_index_raw(document, page_index, reference)?;
    if let Some(field) = attached_field(document, page_index, index)
        && field != released_field
    {
        let annotation = document.raw_annotation(page_index, index);
        let merged = annotation.is_some_and(|a| a.object_number() == field);
        return Err(widget_attached(merged));
    }
    promote_inline_annotations(document, page_index, reference.page())?;
    if !document.remove_annotation_raw(page_index, index) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("failed to remove annotation {}", reference.key()),
        ));
    }
    Ok(())
}

/// Why a widget isn't deleted as an annotation, naming the form op that
/// removes it (`widgetDeleteRefused`). `merged`: the widget is its field's
/// own dictionary. `None`: a widget attached to no field.
pub fn widget_delete_refused(attached: Option<bool>) -> EngineError {
    match attached {
        Some(merged) => widget_attached(merged),
        None => EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a widget is the form's: it isn't deleted as an annotation",
        ),
    }
}

fn widget_attached(merged: bool) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        if merged {
            "widget is its form field's own dictionary (a merged field/widget) - delete the field \
             with doc.forms.delete"
        } else {
            "widget is attached to a form field - use doc.forms.removeWidget or doc.forms.delete"
        },
    )
}

/// `missingAnnotation`.
pub fn missing_annotation(reference: &AnnotationRef) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::NotFound,
        format!("no annotation {} on page {}", reference.key(), reference.page().get()),
    )
}

/// The field the widget at `index` of the page is attached to, by object
/// number (`resolveWidgetFieldObjectNumber`); `None` for one in no field, or
/// an annotation that is not a widget.
fn attached_field(document: &Document, page_index: u32, index: u32) -> Option<u32> {
    let annotation = document.raw_annotation(page_index, index)?;
    if !is_widget_code(annotation.subtype_code()) {
        return None;
    }
    let object_number = annotation.object_number();
    if object_number == 0 {
        return None;
    }
    let model = document.form_model()?;
    let at = model.field_index_for_widget(object_number)?;
    model.field(at).map(|field| field.object_number()).filter(|field| *field > 0)
}

/// The widget at `index` of the page, as `widgetDeleteRefused` names it:
/// `Some(None)` for one attached to no field, `Some(Some(merged))` for one
/// attached, `None` for an annotation that is not a widget.
pub fn widget_attachment(document: &Document, page_index: u32, index: u32) -> Option<Option<bool>> {
    let annotation = document.raw_annotation(page_index, index)?;
    if !is_widget_code(annotation.subtype_code()) {
        return None;
    }
    let object_number = annotation.object_number();
    let field = (object_number > 0)
        .then(|| {
            document.form_model().and_then(|model| {
                let at = model.field_index_for_widget(object_number)?;
                model.field(at).map(|field| field.object_number())
            })
        })
        .flatten()
        .filter(|field| *field > 0);
    Some(field.map(|field| field == object_number))
}
