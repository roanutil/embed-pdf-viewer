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
//! # Two scopes
//!
//! A `pages` scope applies every marking on each page named. An
//! `annotations` scope applies exactly the markings named. Every reference
//! of a page resolves before any of them applies, and they apply in one
//! native call ([`apply_named_markings`]), so a reference cannot name
//! something else by the time it is used.
//!
//! # All or nothing
//!
//! Like [`super::pages_write::flatten_pages`]: a page that fails, or a
//! cancel, refuses the whole call. A page with no markings is `unchanged`,
//! which is not a failure.

use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::write::delete::promote_inline_annotations;
use crate::identity::{AnnotationRef, PageObjectNumber, PageRef, WirePage};
use crate::mutation::MutationMeta;
use crate::session::document::DocumentSession;

/// What to apply (`RedactionApplyScope`): `{pages}` or `{annotations}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RedactionScope {
    /// Every marking on each page.
    Pages {
        #[serde(with = "crate::identity::page_refs")]
        pages: Vec<PageObjectNumber>,
    },
    /// Exactly these markings. Every ref carries its page.
    Annotations { annotations: Vec<AnnotationRef> },
}

/// A scope with `pages` is a pages scope, as `'pages' in scope` reads it;
/// otherwise it must have `annotations`.
impl<'de> Deserialize<'de> for RedactionScope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(default)]
            pages: Option<Vec<PageRef>>,
            #[serde(default)]
            annotations: Option<Vec<AnnotationRef>>,
        }
        match Wire::deserialize(d)? {
            Wire { pages: Some(pages), .. } => Ok(Self::Pages {
                pages: pages.into_iter().map(PageObjectNumber::from_page_ref).collect(),
            }),
            Wire { annotations: Some(annotations), .. } => Ok(Self::Annotations { annotations }),
            _ => Err(serde::de::Error::custom(
                "a redaction scope names its `pages` or its `annotations`",
            )),
        }
    }
}

/// What applying did to one page (`RedactionApplyStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RedactionStatus {
    Applied,
    /// The page had no markings, which is not a failure.
    Unchanged,
}

/// One page's outcome (`RedactionApplyItemResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionItem {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub status: RedactionStatus,
    /// Annotations removed ALONGSIDE the markings: popups cascading and
    /// widgets detaching. Not a count of the markings themselves.
    pub removed_annotation_count: u32,
}

/// What a whole apply did (`RedactionApplyResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionApplyResult {
    /// The request's scope, kept for audit and event replay.
    pub scope: RedactionScope,
    pub results: Vec<RedactionItem>,
    pub removed_annotation_count: u32,
    /// The pages promoted or applied. None names no page: the apply wrote
    /// nothing.
    pub meta: MutationMeta,
}

/// One page's share of the work: every marking, or these markings.
type Plan = Vec<(PageObjectNumber, Option<Vec<AnnotationRef>>)>;

/// `RedactionApplier.apply`, inside the write's transaction (its
/// [`DocumentSession::write_stamp`] stamps the meta).
pub fn apply(
    session: &DocumentSession,
    scope: &RedactionScope,
    cancel: &CancelToken,
) -> Result<RedactionApplyResult, EngineError> {
    use crate::access::{DocumentAccess, PageAccess};

    cancel.check()?;
    let plan = build_plan(scope)?;
    let owned = session.document()?;
    // Every page resolves before anything is applied: a scope naming one
    // page that does not exist must redact nothing.
    let indices =
        plan.iter().map(|(page, _)| owned.page_index(*page)).collect::<Result<Vec<_>, _>>()?;
    preflight(owned, &plan, cancel)?;

    let mut results = Vec::with_capacity(plan.len());
    let mut affected: Vec<PageObjectNumber> = Vec::new();
    let mut total = 0u32;

    for ((page, refs), page_index) in plan.iter().zip(indices) {
        let outcome = cancel.check().and_then(|()| {
            // A redaction removes entries from /Annots; promotion keeps
            // every position.
            if promote_inline_annotations(owned.document(), page_index, *page)? {
                affected.push(*page);
            }
            let opened = owned.page(*page)?;
            let view = opened.view();
            let outcome = match refs {
                None => apply_every_marking(view),
                Some(refs) => apply_named_markings(owned.document(), view, refs),
            }?;
            if outcome.0 == RedactionStatus::Applied {
                view.generate_content();
            }
            Ok(outcome)
        });
        let (status, removed) = match outcome {
            Ok(outcome) => outcome,
            Err(refused) => {
                return Err(super::pages_write::after_writes(session, &affected, refused));
            }
        };
        if status == RedactionStatus::Applied {
            if !affected.contains(page) {
                affected.push(*page);
            }
            total += removed;
        }
        results.push(RedactionItem {
            page_object_number: *page,
            status,
            removed_annotation_count: removed,
        });
    }

    let meta = MutationMeta::local(
        affected.into_iter().map(PageRef::new).collect(),
        &session.write_stamp()?,
    );
    Ok(RedactionApplyResult {
        scope: scope.clone(),
        results,
        removed_annotation_count: total,
        meta,
    })
}

/// Normalize a scope into per-page work, in the order it was given
/// (`buildPlan`, with its messages).
fn build_plan(scope: &RedactionScope) -> Result<Plan, EngineError> {
    match scope {
        RedactionScope::Pages { pages } => {
            if pages.is_empty() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "redaction.apply requires at least one page",
                ));
            }
            let mut plan: Plan = Vec::with_capacity(pages.len());
            for page in pages {
                if plan.iter().any(|(existing, _)| existing == page) {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        format!(
                            "redaction.apply was given duplicate page object number {}",
                            page.get()
                        ),
                    ));
                }
                plan.push((*page, None));
            }
            Ok(plan)
        }
        RedactionScope::Annotations { annotations } => {
            if annotations.is_empty() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "redaction.apply requires at least one annotation ref",
                ));
            }
            let mut plan: Plan = Vec::new();
            for reference in annotations {
                let page = reference.page();
                match plan.iter_mut().find(|(existing, _)| *existing == page) {
                    Some((_, Some(bucket))) => bucket.push(reference.clone()),
                    // Unreachable, as in the TypeScript: a scope is pages
                    // or annotations, never both.
                    Some((_, None)) => {
                        return Err(EngineError::with_message(
                            EngineErrorCode::InvalidArg,
                            "mixed redaction scopes on one page",
                        ));
                    }
                    None => plan.push((page, Some(vec![reference.clone()]))),
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
fn preflight(
    owned: &crate::access::OwnedDocument,
    plan: &Plan,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    use crate::access::{DocumentAccess, PageAccess};
    use crate::features::annotation::subtype::AnnotationSubtype;

    for (page_object_number, refs) in plan {
        let Some(refs) = refs else { continue };
        cancel.check()?;
        let page = owned.page(*page_object_number)?;
        let view = page.view();
        for reference in refs {
            let annotation = crate::features::annotation::write::resolve::resolve_annotation(
                owned.document(),
                view,
                reference,
            )?;
            let subtype = AnnotationSubtype::from_code(annotation.subtype_code());
            if subtype != AnnotationSubtype::Redact {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "redaction.apply ref on page {} resolves to a '{}' annotation, \
                         not a redact annotation",
                        page_object_number.get(),
                        subtype.as_str()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Apply every marking on a page (`applyAllOnPage`).
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
        // on a page that has markings.
        None if has_markings => Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "native page redaction apply failed",
        )),
        None => Ok((RedactionStatus::Unchanged, 0)),
    }
}

/// Apply the markings named, in one pass (`applyRefsOnPage`): every ref
/// resolves before any applies, so their regions are redacted together and
/// an image under several of them is read and written once.
fn apply_named_markings(
    document: &pdfium::Document,
    page: &pdfium::PageView<'_>,
    refs: &[AnnotationRef],
) -> Result<(RedactionStatus, u32), EngineError> {
    let marks = refs
        .iter()
        .map(|reference| {
            crate::features::annotation::write::resolve::resolve_annotation(
                document, page, reference,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let removed = page.apply_redaction_marks(&marks).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "native redaction apply failed after preflight",
        )
    })?;
    Ok((RedactionStatus::Applied, removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: u32) -> PageObjectNumber {
        PageObjectNumber::new(n).unwrap()
    }

    #[test]
    fn a_scope_reads_pages_first_and_refuses_neither() {
        let pages: RedactionScope = serde_json::from_value(serde_json::json!({
            "pages": [{ "kind": "objectNumber", "objectNumber": 3 }],
            "annotations": [],
        }))
        .unwrap();
        assert_eq!(pages, RedactionScope::Pages { pages: vec![page(3)] });
        let annotations: RedactionScope = serde_json::from_value(serde_json::json!({
            "annotations": [{ "kind": "baseIndex",
                "page": { "kind": "objectNumber", "objectNumber": 3 }, "baseIndex": 18 }],
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&annotations).unwrap(),
            serde_json::json!({ "annotations": [{ "kind": "baseIndex",
                "page": { "kind": "objectNumber", "objectNumber": 3 }, "baseIndex": 18 }] }),
        );
        assert!(serde_json::from_value::<RedactionScope>(serde_json::json!({})).is_err());
        assert!(
            serde_json::from_value::<RedactionScope>(serde_json::json!({ "kind": "pages" }))
                .is_err()
        );
    }

    #[test]
    fn a_plan_names_at_least_one_target_and_each_page_once() {
        let empty = build_plan(&RedactionScope::Pages { pages: vec![] }).unwrap_err();
        assert!(empty.is(EngineErrorCode::InvalidArg), "{empty}");
        assert_eq!(empty.message, "redaction.apply requires at least one page");
        let twice =
            build_plan(&RedactionScope::Pages { pages: vec![page(3), page(3)] }).unwrap_err();
        assert_eq!(twice.message, "redaction.apply was given duplicate page object number 3");
        let none = build_plan(&RedactionScope::Annotations { annotations: vec![] }).unwrap_err();
        assert_eq!(none.message, "redaction.apply requires at least one annotation ref");

        // Refs group by page, in the order their pages first appear.
        let by = |p, base_index| AnnotationRef::BaseIndex { page: page(p), base_index };
        let plan = build_plan(&RedactionScope::Annotations {
            annotations: vec![by(4, 0), by(3, 1), by(4, 2)],
        })
        .unwrap();
        assert_eq!(
            plan,
            vec![(page(4), Some(vec![by(4, 0), by(4, 2)])), (page(3), Some(vec![by(3, 1)]))]
        );
    }
}
