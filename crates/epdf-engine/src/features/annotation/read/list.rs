//! `annotations.list`: the annotations of some pages, or of every page.
//!
//! G7. Ports `services/src/features/annotations/RawAnnotationReader.ts`
//! (`list`, `listOne`) and `internal/read/collectPageAnnotations.ts`, with
//! the `'annotations'` family: every subtype but `/Widget`, whose rows are
//! the form's (1233d1349). Read off the document, so no page is loaded.
//!
//! The answer is `AnnotationList` (`core/src/annotation/AnnotationList.ts`):
//! one flat list in which every annotation names its `page`, beside the
//! pages it listed. There is no per-page state: upstream's read has none
//! (83f6598d2).

use pdfium::Document;
use serde::Serialize;

use super::kinds::{AnnotationRecord, is_widget_code, read_annotation_record_kind};
use super::record::read_annotation_record_base;
use crate::access::OwnedDocument;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::identity::PageObjectNumber;

/// What `annotations.list` answers, in the file's coordinates.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnotationList {
    pub annotations: Vec<AnnotationRecord>,
    #[serde(with = "crate::identity::page_refs")]
    pub pages: Vec<PageObjectNumber>,
}

/// The annotations of `pages`, in their order, or of every page in document
/// order (`RawAnnotationReader.list`). A page the document doesn't have is
/// `NotFound`, as `resolvePageRef` answers it.
pub fn list_annotations(
    document: &OwnedDocument,
    pages: Option<&[PageObjectNumber]>,
    cancel: &CancelToken,
) -> Result<AnnotationList, EngineError> {
    cancel.check()?;
    let raw = document.document();
    let listed: Vec<(PageObjectNumber, u32)> = match pages {
        Some(pages) => pages
            .iter()
            .map(|page| Ok((*page, document.page_index(*page)?)))
            .collect::<Result<_, EngineError>>()?,
        None => (0..raw.page_count())
            .filter_map(|index| {
                PageObjectNumber::new(raw.page_object_number_by_index(index))
                    .ok()
                    .map(|page| (page, index))
            })
            .collect(),
    };
    let mut list = AnnotationList { annotations: Vec::new(), pages: Vec::new() };
    for (page, index) in listed {
        cancel.check()?;
        list.annotations.extend(read_page_annotation_records(raw, index, page, cancel)?);
        list.pages.push(page);
    }
    Ok(list)
}

/// One page's annotations, widgets left out (`listOne` and
/// `collectPageAnnotations` with the `'annotations'` family): the subtype is
/// checked before anything else is read, so a widget's row is never built.
pub fn read_page_annotation_records(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
    cancel: &CancelToken,
) -> Result<Vec<AnnotationRecord>, EngineError> {
    let count = document.raw_annotation_count(page_index).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFPage_GetAnnotCountRaw refused page {}", page.get()),
        )
    })?;
    // One budget per page, as `collectPageAnnotations` keeps one.
    let mut budget = ActionReadBudgetTracker::default();
    let mut annotations = Vec::new();
    for index in 0..count {
        cancel.check()?;
        // A gap in `/Annots` still takes its index.
        let Some(annotation) = document.raw_annotation(page_index, index) else {
            continue;
        };
        let subtype = annotation.subtype_code();
        if is_widget_code(subtype) {
            continue;
        }
        let base = read_annotation_record_base(document, &annotation, page, index, &mut budget)?;
        let kind = read_annotation_record_kind(document, &annotation, &base, subtype)?;
        annotations.push(AnnotationRecord { base, kind });
    }
    Ok(annotations)
}
