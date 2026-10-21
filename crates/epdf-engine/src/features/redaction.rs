//! Applying a redaction, which is the destructive half.
//!
//! G13. Ports `services/src/features/redaction/RedactionApplier.ts`.
//!
//! A `/Redact` annotation is a MARKING: it says what should be removed and
//! what the region must look like afterwards, and creating one removes
//! nothing. Everything in [`super::annotation::write`] deals in markings.
//! This is the other call, the one that takes the content out, and it cannot
//! be undone.
//!
//! # Two scopes, and they do not mix on one page
//!
//! A `pages` scope applies every marking on each page named. An
//! `annotations` scope applies exactly the markings named. A page cannot
//! appear in both, because "every marking here" and "these three markings
//! here" are two different instructions for the same page and there is no
//! reading that satisfies both.
//!
//! # Positional references cannot be batched
//!
//! Applying a marking removes annotations, which shifts every index after
//! it. So a batch of several references on one page is refused when any of
//! them is positional: by the time the second one is resolved the index it
//! named belongs to a different annotation. One per call is fine, and a
//! durable reference is fine in any number.

use serde::{Deserialize, Serialize};

use pdfium::Document;

use crate::access::PageAccess;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::identity::{AnnotationRef, PageObjectNumber};

/// What to apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[serde(rename_all_fields = "camelCase")]
pub enum RedactionScope {
    /// Every marking on each page.
    Pages {
        #[serde(rename = "pages", with = "crate::identity::page_refs")]
        page_object_numbers: Vec<PageObjectNumber>,
    },
    /// Exactly these markings.
    Annotations { refs: Vec<AnnotationRef> },
}

/// What applying did to one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RedactionStatus {
    Applied,
    /// The page had no markings, which is not a failure.
    Unchanged,
    Failed,
    /// Not attempted, because an earlier page failed.
    Skipped,
}

/// One page's outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionItem {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub status: RedactionStatus,
    /// Annotations removed ALONGSIDE the markings: popups cascading and
    /// widgets detaching. Not a count of the markings themselves.
    pub removed_annotation_count: u32,
    /// Why, for a failure: a `SerializedEngineError`, as `RedactionApplier`
    /// writes it with `serializeError`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

/// What a whole apply did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionApplyResult {
    pub scope: RedactionScope,
    pub results: Vec<RedactionItem>,
    pub removed_annotation_count: u32,
}

/// One page's share of the work: every marking, or these markings.
type Plan = Vec<(PageObjectNumber, Option<Vec<AnnotationRef>>)>;

/// Apply.
///
/// `open_page` acquires a page, because the pages a scope names are the
/// caller's to hold and there may be several.
pub fn apply<P: PageAccess>(
    document: &Document,
    open_page: impl Fn(PageObjectNumber) -> Result<P, EngineError>,
    scope: &RedactionScope,
    cancel: &CancelToken,
) -> Result<RedactionApplyResult, EngineError> {
    cancel.check()?;
    let plan = build_plan(scope)?;
    // Every page resolves before anything is applied: a scope naming one
    // page that does not exist must redact nothing.
    for (page, _) in &plan {
        super::pages_write::require_page(document, *page)?;
    }
    preflight(&open_page, &plan, cancel)?;

    let mut results = Vec::with_capacity(plan.len());
    let mut total = 0u32;
    let mut stop = false;

    for (page_object_number, refs) in &plan {
        if stop || cancel.check().is_err() {
            stop = true;
            results.push(RedactionItem {
                page_object_number: *page_object_number,
                status: RedactionStatus::Skipped,
                removed_annotation_count: 0,
                error: None,
            });
            continue;
        }

        let page = open_page(*page_object_number)?;
        let view = PageAccess::view(&page);
        let outcome = match refs {
            None => apply_every_marking(view),
            Some(refs) => apply_named_markings(view, refs),
        };
        match outcome {
            Ok((status, removed)) => {
                if status == RedactionStatus::Applied {
                    // Inert, like every other `generate_content` in this
                    // port. `applying_a_redaction_rebuilds_the_page_content`
                    // renders the page before and after and the two differ
                    // whether or not this line runs, so PDFium's apply has
                    // already rewritten the stream by the time it returns.
                    // It stays because the TypeScript makes the call and
                    // because a fork that stopped rewriting eagerly would
                    // otherwise leave the removal invisible.
                    view.generate_content();
                    // Always zero on this corpus. The count is
                    // COLLATERAL: popups cascading and widgets detaching,
                    // and no marking in the fixture has either. The
                    // fixture's annotations are direct dictionaries in
                    // `/Annots`, so giving one a `/Popup` would mean
                    // restructuring it into indirect objects.
                    total += removed;
                }
                results.push(RedactionItem {
                    page_object_number: *page_object_number,
                    status,
                    removed_annotation_count: removed,
                    error: None,
                });
            }
            Err(error) => {
                stop = true;
                results.push(RedactionItem {
                    page_object_number: *page_object_number,
                    status: RedactionStatus::Failed,
                    removed_annotation_count: 0,
                    error: Some(error.to_wire()),
                });
            }
        }
    }

    Ok(RedactionApplyResult { scope: scope.clone(), results, removed_annotation_count: total })
}

/// Normalize a scope into per-page work, in the order it was given.
fn build_plan(scope: &RedactionScope) -> Result<Plan, EngineError> {
    match scope {
        RedactionScope::Pages { page_object_numbers } => {
            if page_object_numbers.is_empty() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "redaction.apply needs at least one page",
                ));
            }
            let mut plan: Plan = Vec::with_capacity(page_object_numbers.len());
            for page in page_object_numbers {
                if plan.iter().any(|(existing, _)| existing == page) {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        format!("redaction.apply names page {} twice", page.get()),
                    ));
                }
                plan.push((*page, None));
            }
            Ok(plan)
        }
        RedactionScope::Annotations { refs } => {
            if refs.is_empty() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "redaction.apply needs at least one annotation reference",
                ));
            }
            let mut plan: Plan = Vec::new();
            for reference in refs {
                let page = reference.page();
                match plan.iter_mut().find(|(existing, _)| *existing == page) {
                    Some((_, Some(bucket))) => bucket.push(reference.clone()),
                    // Unreachable through this constructor: the pages scope
                    // builds no reference buckets and this one builds no
                    // `None`s, so the two never meet on one page.
                    Some((_, None)) => {
                        return Err(EngineError::with_message(
                            EngineErrorCode::InvalidArg,
                            format!("mixed redaction scopes on page {}", page.get()),
                        ));
                    }
                    None => plan.push((page, Some(vec![reference.clone()]))),
                }
            }
            for (page, refs) in &plan {
                let Some(refs) = refs else { continue };
                // Applying shifts every index after the annotation it
                // removed, so the second positional reference in a batch
                // names something else by the time it is resolved. A
                // `baseIndex` is positional here too: upstream keeps it
                // stable by promoting the page first, which this engine
                // cannot do yet (`resolve::base_index_position`).
                if refs.len() > 1
                    && refs.iter().any(|r| {
                        matches!(r, AnnotationRef::Index { .. } | AnnotationRef::BaseIndex { .. })
                    })
                {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        format!(
                            "several redactions on page {} cannot be addressed by positional \
                             references: applying one shifts the rest. Use object-number or \
                             name references, or one call per positional reference",
                            page.get()
                        ),
                    ));
                }
            }
            Ok(plan)
        }
    }
}

/// Every reference resolves, and every one names a marking.
///
/// Before the first write, and it refuses the WHOLE call rather than the one
/// page: a scope that names a highlight is a caller that has confused two
/// annotations, and redacting the other pages anyway would be acting on half
/// of an instruction nobody meant.
fn preflight<P: PageAccess>(
    open_page: &impl Fn(PageObjectNumber) -> Result<P, EngineError>,
    plan: &Plan,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    use crate::features::annotation::subtype::AnnotationSubtype;

    for (page_object_number, refs) in plan {
        let Some(refs) = refs else { continue };
        cancel.check()?;
        let page = open_page(*page_object_number)?;
        let view = PageAccess::view(&page);
        for reference in refs {
            let annotation =
                crate::features::annotation::write::resolve::resolve_annotation(view, reference)?;
            let subtype = AnnotationSubtype::from_code(annotation.subtype_code());
            if subtype != AnnotationSubtype::Redact {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "a redaction reference on page {} resolves to a {} annotation, \
                         not a redaction marking",
                        page_object_number.get(),
                        subtype.as_str()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Apply every marking on a page.
fn apply_every_marking(page: &pdfium::PageView<'_>) -> Result<(RedactionStatus, u32), EngineError> {
    use crate::features::annotation::subtype::AnnotationSubtype;

    // Looked at BEFORE the call, because PDFium's false is overloaded: it
    // means either that there were no markings or that applying one failed.
    // A sanitizer failure reported as an unchanged page is a document
    // somebody believes was redacted.
    let has_markings = (0..page.annotation_count()).any(|index| {
        page.annotation(index).is_some_and(|annotation| {
            AnnotationSubtype::from_code(annotation.subtype_code()) == AnnotationSubtype::Redact
        })
    });

    match page.apply_redactions() {
        Some(removed) => Ok((RedactionStatus::Applied, removed)),
        // Unexercised: nothing in the corpus makes PDFium refuse an apply
        // on a page that has markings. The branch is the whole reason the
        // count is taken first, because without it a sanitizer failure
        // would be reported as an unchanged page and somebody would
        // believe a document was redacted.
        None if has_markings => Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "PDFium refused to apply the page's redactions",
        )),
        None => Ok((RedactionStatus::Unchanged, 0)),
    }
}

/// Apply the markings named.
fn apply_named_markings(
    page: &pdfium::PageView<'_>,
    refs: &[AnnotationRef],
) -> Result<(RedactionStatus, u32), EngineError> {
    let mut removed = 0;
    for reference in refs {
        // Re-resolved at apply time rather than carried from preflight: an
        // earlier apply on this page may have taken collateral annotations
        // with it, and a durable reference survives that.
        let annotation =
            crate::features::annotation::write::resolve::resolve_annotation(page, reference)?;
        removed += page.apply_redaction(&annotation).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                "PDFium refused to apply a redaction that passed preflight",
            )
        })?;
    }
    Ok((RedactionStatus::Applied, removed))
}
