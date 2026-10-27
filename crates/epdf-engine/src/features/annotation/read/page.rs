//! The per-page read loop, and the whole-document one over it.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/collectPageAnnotations.ts`
//! and the raw half of `RawAnnotationReader.ts`, in the record shape this
//! branch's wire had before the catch-up. No wire kind answers with it now
//! (upstream's `annotations.list` is [`super::list`]): it serves the
//! annotation writes' read-back (Tasks 8.14 to 8.16) and the kernel's
//! listings in `epdf-viewer-host`'s `translate.rs` (`kernel_annotations`,
//! until Task 13).
//!
//! The RAW path, which reads off the document handle and never loads a page.
//! Loading a page parses its content stream, and a listing of what is on the
//! page does not need that. The full path in `AnnotationReader.ts` differs
//! only in where the handles come from, so when it lands it will share this
//! loop rather than copy it, exactly as the TypeScript does.
//!
//! # The weak flag is a property of the page, not of an annotation
//!
//! One annotation with no durable address makes the whole page weak, because
//! every index on it shifts together: inserting an annotation at position two
//! renumbers everything after it, whatever their own identities are. So the
//! flag is computed across the loop and reported once, and a caller that sees
//! it knows to re-read before dereferencing any index on that page.

use pdfium::Document;

use super::base::read_annotation_base;
use super::dto::AnnotationDto;
use super::registry::read_annotation_kind;
use super::widget_join::join_widget_fields;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::identity::{AnnotationIdentityQuality, PageObjectNumber};
use crate::revision::{PageState, RevisionToken, WeakAnnotationState};
use serde::{Deserialize, Serialize};

/// One page's annotations, with the liveness envelope a client needs to
/// address them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationListPageSnapshot {
    pub page_state: PageState,
    pub annotations: Vec<AnnotationDto>,
}

/// Every page's annotations.
///
/// Page order is document order here. The TypeScript documents the order as
/// unspecified because its cloud engine serves manifest order, so a consumer
/// that cares joins on `pageState.page` rather than on position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationListSnapshot {
    pub pages: Vec<AnnotationListPageSnapshot>,
}

/// Read one page's annotations off the document.
///
/// `revision` is minted by whoever owns the page's generation counter, which
/// in Stage A is the TypeScript session. This reader does not mint one: a
/// token it invented would not be checkable against the counter a client's
/// next call is validated against.
pub fn read_page_annotations(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
    revision: &RevisionToken,
    budget: &mut ActionReadBudgetTracker,
    cancel: &CancelToken,
) -> Result<AnnotationListPageSnapshot, EngineError> {
    cancel.check()?;

    let count = document.raw_annotation_count(page_index).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::MalformedPdf,
            format!("page at index {page_index} has an unreadable /Annots"),
        )
    })?;

    let mut annotations = Vec::new();
    let mut has_any_weak = false;

    for index in 0..count {
        cancel.check()?;
        // A null handle is a gap in `/Annots`, not a failure of the listing:
        // an entry that is not a dictionary is something the file got wrong
        // and there is nothing to report about it. It still consumes its
        // index, because the index is a position in `/Annots` and a client
        // addressing by index has to agree with the file about what is where.
        let Some(annotation) = document.raw_annotation(page_index, index) else {
            continue;
        };

        let base = read_annotation_base(document, &annotation, page, index, revision, budget)?;
        if base.identity_quality == AnnotationIdentityQuality::Weak {
            has_any_weak = true;
        }
        let kind = read_annotation_kind(document, &annotation, &base, annotation.subtype_code())?;
        annotations.push(AnnotationDto { base, kind });
    }

    // After the loop, once, for the reason `widget_join` gives.
    join_widget_fields(document, &mut annotations);

    Ok(AnnotationListPageSnapshot {
        page_state: PageState {
            page_object_number: page,
            revision: revision.clone(),
            weak_annotation_state: WeakAnnotationState::Known {
                has_any_weak_annotations: has_any_weak,
            },
        },
        annotations,
    })
}

/// Read every page's annotations.
///
/// One budget across the document, for the reason `pages.list` uses one: the
/// limits PDFium enforces are per action model, and a document with an action
/// on every annotation of every page would otherwise build an unbounded
/// snapshot out of individually reasonable ones.
pub fn read_document_annotations(
    document: &Document,
    revision_for: impl Fn(PageObjectNumber) -> RevisionToken,
    cancel: &CancelToken,
) -> Result<AnnotationListSnapshot, EngineError> {
    cancel.check()?;
    let page_count = document.page_count();
    let mut pages = Vec::with_capacity(page_count as usize);
    let mut budget = ActionReadBudgetTracker::default();

    for page_index in 0..page_count {
        cancel.check()?;
        let page = PageObjectNumber::new(document.page_object_number_by_index(page_index))
            .map_err(|_| {
                EngineError::with_message(
                    EngineErrorCode::MalformedPdf,
                    format!(
                        "page at index {page_index} is a direct (non-indirect) PDF object; \
                         the engine requires every page to have a stable indirect object number"
                    ),
                )
            })?;
        let revision = revision_for(page);
        pages.push(read_page_annotations(
            document,
            page_index,
            page,
            &revision,
            &mut budget,
            cancel,
        )?);
    }

    Ok(AnnotationListSnapshot { pages })
}
