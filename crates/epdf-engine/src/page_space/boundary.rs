//! The host's one boundary between PDF space and page space
//! (`services/src/worker-host/pageSpaceBoundary.ts`).
//!
//! The handlers read and write the file in PDF space. Every request converts
//! here before its handler runs ([`request_in_file_space`]) and every result
//! converts here before it leaves the host ([`result_in_page_space`]), so
//! the wire answers page space as the TypeScript engine does. Both run once,
//! on [`crate::host::Host::call`]'s path; the kinds upstream removed
//! (`Host::call_legacy`) never cross it.
//!
//! Both matches are over [`WireKind`] with no catch-all arm, as upstream's
//! result switch is typed so that a result whose shape depends on the space
//! fails to compile until it converts. A kind added to [`WireKind`] does not
//! compile until it is placed in each match, as converting or as holding no
//! places.
//!
//! A family that has not moved to page space yet passes its answer through
//! unconverted under a `re-port (Task 8)` tag naming the sub-task that adds
//! its conversion here.

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::annotations::{page_annotation_list_of, page_annotation_of};
use super::destinations::page_document_actions_of;
use super::forms::page_form_snapshot_of;
use super::pages::page_list_of;
use super::rendering::{page_appearances_of, page_render_area_of, pdf_render_target_of};
use super::text::{page_geometry_of, page_search_slice_of};
use super::{PdfCoordinates, VisibleBoxOf};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::document::DocumentActionsSnapshot;
use crate::features::changes::item::ChangeItem;
use crate::features::changes::op::ChangeOp;
use crate::features::forms::snapshot::FormSnapshot;
use crate::features::geometry::PageGeometrySnapshot;
use crate::features::pages::PageListSnapshot;
use crate::features::render::geometry::RenderTarget;
use crate::features::search::reader::SearchSlice;
use crate::geom::{PageRect, PdfRect};
use crate::host::HostResponse;
use crate::identity::{PageObjectNumber, PageRef};

macro_rules! wire_kinds {
    ($($variant:ident => $kind:literal,)*) => {
        /// Every kind of upstream's `WorkerRequest` union, as
        /// `tooling/engine-conformance/vectors/wire/requests.json` records
        /// it. `tests/wire_contract.rs` checks the two agree.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum WireKind {
            $($variant,)*
        }

        impl WireKind {
            /// Every kind, with its wire spelling.
            pub const ALL: &'static [(&'static str, WireKind)] = &[$(($kind, WireKind::$variant),)*];

            /// The kind a request names, or `None` for one the wire does
            /// not have.
            pub fn of(kind: &str) -> Option<Self> {
                match kind {
                    $($kind => Some(WireKind::$variant),)*
                    _ => None,
                }
            }

            /// The wire spelling.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(WireKind::$variant => $kind,)*
                }
            }
        }
    };
}

wire_kinds! {
    Abort => "abort",
    ActionsRead => "actions.read",
    AnnotationsCreate => "annotations.create",
    AnnotationsDelete => "annotations.delete",
    AnnotationsExport => "annotations.export",
    AnnotationsExportAppearance => "annotations.exportAppearance",
    AnnotationsFlatten => "annotations.flatten",
    AnnotationsImport => "annotations.import",
    AnnotationsList => "annotations.list",
    AnnotationsReadAppearance => "annotations.readAppearance",
    AnnotationsReadFile => "annotations.readFile",
    AnnotationsRenderAppearances => "annotations.renderAppearances",
    AnnotationsRenderAppearancesEncoded => "annotations.renderAppearancesEncoded",
    AnnotationsReorder => "annotations.reorder",
    AnnotationsUpdate => "annotations.update",
    AttachmentsList => "attachments.list",
    AttachmentsReadFile => "attachments.readFile",
    Close => "close",
    DocumentApply => "document.apply",
    DocumentApplyChanges => "document.applyChanges",
    DocumentCheckPasswordPermissions => "document.checkPasswordPermissions",
    DocumentProbeSecurityFile => "document.probeSecurityFile",
    DocumentRenderPageFile => "document.renderPageFile",
    DocumentRenderPageFileEncoded => "document.renderPageFileEncoded",
    DocumentSaveBuffer => "document.saveBuffer",
    DocumentSaveFile => "document.saveFile",
    DocumentSaveLayerBuffer => "document.saveLayerBuffer",
    DocumentSetFontSettings => "document.setFontSettings",
    DocumentVersion => "document.version",
    FontsAddFallback => "fonts.addFallback",
    FontsAuthorizeEditing => "fonts.authorizeEditing",
    FontsClear => "fonts.clear",
    FontsClearFallbacks => "fonts.clearFallbacks",
    FontsRegister => "fonts.register",
    FormsAddWidget => "forms.addWidget",
    FormsCreateField => "forms.createField",
    FormsDeleteField => "forms.deleteField",
    FormsDeleteWidget => "forms.deleteWidget",
    FormsDetachWidget => "forms.detachWidget",
    FormsExport => "forms.export",
    FormsImport => "forms.import",
    FormsImportValues => "forms.importValues",
    FormsList => "forms.list",
    FormsReorderCalculations => "forms.reorderCalculations",
    FormsReorderWidgets => "forms.reorderWidgets",
    FormsRepair => "forms.repair",
    FormsReset => "forms.reset",
    FormsSetSignatureAppearance => "forms.setSignatureAppearance",
    FormsSetValue => "forms.setValue",
    FormsUpdateField => "forms.updateField",
    FormsUpdateWidget => "forms.updateWidget",
    LayerClose => "layer.close",
    MeasureSetScale => "measure.setScale",
    MeasureViewports => "measure.viewports",
    MetadataRead => "metadata.read",
    MetadataReadCustom => "metadata.readCustom",
    MetadataUpdate => "metadata.update",
    MetadataUpdateCustom => "metadata.updateCustom",
    ObjectNumbersReserve => "objectNumbers.reserve",
    OpenFatMem => "open.fatMem",
    OpenLayerFileBase => "open.layerFileBase",
    OpenLayerMemBase => "open.layerMemBase",
    OutlineRead => "outline.read",
    PagesExtract => "pages.extract",
    PagesFlatten => "pages.flatten",
    PagesGeometry => "pages.geometry",
    PagesList => "pages.list",
    PagesRemoveName => "pages.removeName",
    PagesRender => "pages.render",
    PagesRenderEncoded => "pages.renderEncoded",
    PagesSetName => "pages.setName",
    PagesText => "pages.text",
    PagesWorkingSet => "pages.workingSet",
    PieceInfoApplications => "pieceInfo.applications",
    PieceInfoDelete => "pieceInfo.delete",
    PieceInfoRead => "pieceInfo.read",
    PieceInfoUpdate => "pieceInfo.update",
    RedactionApply => "redaction.apply",
    SearchQuery => "search.query",
    Shutdown => "shutdown",
    SignaturesAnalyze => "signatures.analyze",
    SignaturesCancel => "signatures.cancel",
    SignaturesComplete => "signatures.complete",
    SignaturesContents => "signatures.contents",
    SignaturesDigest => "signatures.digest",
    SignaturesFinalizeCandidate => "signatures.finalizeCandidate",
    SignaturesList => "signatures.list",
    SignaturesPrepare => "signatures.prepare",
    SignaturesRevisionBytes => "signatures.revisionBytes",
}

/// A request as the handlers take it, each place a caller sent in page space
/// measured in the file's coordinates on its page (`requestInFileSpace`).
/// `box_of` is read only when a request holds a place.
pub fn request_in_file_space(
    kind: WireKind,
    body: &mut serde_json::Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<(), EngineError> {
    use WireKind as K;
    match kind {
        K::PagesRender | K::PagesRenderEncoded => render_options_in_file_space(body, box_of),

        // A file render opens its own document, so it converts its own
        // target there, on the page it opened (`renderOptionsInFileSpace`
        // in `handleDocumentRenderPageFile`).
        K::DocumentRenderPageFile | K::DocumentRenderPageFileEncoded => Ok(()),

        // A change converts each op as it runs, against the document as the
        // ops before it left it ([`change_op_in_file_space`], through
        // `changes::applier::ChangeSpace`), so its request crosses here
        // as it came.
        K::DocumentApply | K::DocumentApplyChanges => Ok(()),

        // The draft on the page it goes on (`pdfAnnotationDraftOf`).
        K::AnnotationsCreate => {
            let page: PageRef = from_value(body.get("page").cloned().unwrap_or_default())?;
            let visible = box_of(page.object_number())?;
            if let Some(serde_json::Value::Object(draft)) = body.get_mut("draft") {
                let converted = super::annotations::pdf_annotation_draft_of(
                    std::mem::take(draft),
                    visible,
                    box_of,
                )?;
                *draft = converted;
            }
            Ok(())
        }

        // The patch on the page of the annotation it changes
        // (`pdfAnnotationPatchOf`).
        K::AnnotationsUpdate => {
            let reference: crate::identity::AnnotationRef =
                from_value(body.get("ref").cloned().unwrap_or_default())?;
            let visible = box_of(reference.page())?;
            if let Some(serde_json::Value::Object(patch)) = body.get_mut("patch") {
                let converted = super::annotations::pdf_annotation_patch_of(
                    std::mem::take(patch),
                    visible,
                    box_of,
                )?;
                *patch = converted;
            }
            Ok(())
        }

        // re-port (Task 8): these take places in page space upstream and
        // convert here once their owners move them: widget placements and
        // patches and field drafts (8.21), a measure (8.25).
        K::FormsUpdateWidget | K::FormsCreateField | K::FormsAddWidget | K::MeasureSetScale => {
            Ok(())
        }

        // No places.
        K::Abort
        | K::ActionsRead
        | K::AnnotationsDelete
        | K::AnnotationsExport
        | K::AnnotationsExportAppearance
        | K::AnnotationsFlatten
        | K::AnnotationsImport
        | K::AnnotationsList
        | K::AnnotationsReadAppearance
        | K::AnnotationsReadFile
        | K::AnnotationsRenderAppearances
        | K::AnnotationsRenderAppearancesEncoded
        | K::AnnotationsReorder
        | K::AttachmentsList
        | K::AttachmentsReadFile
        | K::Close
        | K::DocumentCheckPasswordPermissions
        | K::DocumentProbeSecurityFile
        | K::DocumentSaveBuffer
        | K::DocumentSaveFile
        | K::DocumentSaveLayerBuffer
        | K::DocumentSetFontSettings
        | K::DocumentVersion
        | K::FontsAddFallback
        | K::FontsAuthorizeEditing
        | K::FontsClear
        | K::FontsClearFallbacks
        | K::FontsRegister
        | K::FormsDeleteField
        | K::FormsDeleteWidget
        | K::FormsDetachWidget
        | K::FormsExport
        | K::FormsImport
        | K::FormsImportValues
        | K::FormsList
        | K::FormsReorderCalculations
        | K::FormsReorderWidgets
        | K::FormsRepair
        | K::FormsReset
        | K::FormsSetSignatureAppearance
        | K::FormsSetValue
        | K::FormsUpdateField
        | K::LayerClose
        | K::MeasureViewports
        | K::MetadataRead
        | K::MetadataReadCustom
        | K::MetadataUpdate
        | K::MetadataUpdateCustom
        | K::ObjectNumbersReserve
        | K::OpenFatMem
        | K::OpenLayerFileBase
        | K::OpenLayerMemBase
        | K::OutlineRead
        | K::PagesExtract
        | K::PagesFlatten
        | K::PagesGeometry
        | K::PagesList
        | K::PagesRemoveName
        | K::PagesSetName
        | K::PagesText
        | K::PagesWorkingSet
        | K::PieceInfoApplications
        | K::PieceInfoDelete
        | K::PieceInfoRead
        | K::PieceInfoUpdate
        | K::RedactionApply
        | K::SearchQuery
        | K::Shutdown
        | K::SignaturesAnalyze
        | K::SignaturesCancel
        | K::SignaturesComplete
        | K::SignaturesContents
        | K::SignaturesDigest
        | K::SignaturesFinalizeCandidate
        | K::SignaturesList
        | K::SignaturesPrepare
        | K::SignaturesRevisionBytes => Ok(()),
    }
}

/// A result as the wire answers it, every place in page space
/// (`resultInPageSpace`). `box_of` gives the visible box of any page of the
/// request's document, read only when a result asks for one: a destination
/// is measured on the page it goes to. A page list carries every page's box
/// itself. `request` is the request as the handler took it: an answer that
/// does not name its page, as upstream's payload does, is measured on the
/// request's `page`.
pub fn result_in_page_space(
    kind: WireKind,
    request: &serde_json::Value,
    response: HostResponse,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<HostResponse, EngineError> {
    use WireKind as K;
    match kind {
        K::PagesList => convert_result(response, |snapshot: PageListSnapshot<PdfCoordinates>| {
            page_list_of(snapshot)
        }),
        K::ActionsRead => convert_result(response, |snapshot: DocumentActionsSnapshot| {
            page_document_actions_of(snapshot, box_of)
        }),
        K::PagesRender => convert_result(response, |mut answer: serde_json::Value| {
            let page: PageRef = from_value(answer.get("page").cloned().unwrap_or_default())?;
            let area: PdfRect = from_value(answer.get("area").cloned().unwrap_or_default())?;
            let visible = box_of(page.object_number())?;
            answer["area"] = to_value(&page_render_area_of(area, visible))?;
            Ok(answer)
        }),
        K::PagesGeometry => {
            convert_result(response, |snapshot: PageGeometrySnapshot<PdfCoordinates>| {
                let page: PageRef = from_value(request.get("page").cloned().unwrap_or_default())?;
                Ok(page_geometry_of(snapshot, box_of(page.object_number())?))
            })
        }
        K::SearchQuery => convert_result(response, |slice: SearchSlice<PdfCoordinates>| {
            page_search_slice_of(slice, box_of)
        }),
        K::FormsList => convert_result(response, |snapshot: FormSnapshot<PdfCoordinates>| {
            page_form_snapshot_of(snapshot, box_of)
        }),
        K::AnnotationsList => convert_result(response, |list: serde_json::Value| {
            page_annotation_list_of(list, box_of)
        }),
        // Raster or encoded, each appearance placed on the page it names
        // (`pageAppearancesOf(payload.result.appearances, boxOf(payload.page))`).
        K::AnnotationsRenderAppearances | K::AnnotationsRenderAppearancesEncoded => {
            convert_result(response, |mut answer: serde_json::Value| {
                let page: PageRef = from_value(answer.get("page").cloned().unwrap_or_default())?;
                let visible = box_of(page.object_number())?;
                if let Some(appearances) =
                    answer.get_mut("appearances").and_then(serde_json::Value::as_array_mut)
                {
                    page_appearances_of(appearances, visible).map_err(|e| {
                        unserializable(format!("an appearance's rect is not a rect: {e}"))
                    })?;
                }
                Ok(answer)
            })
        }

        // The new or changed annotation on its page (`pageAnnotationOf`), in
        // the result and in the event that carries it.
        K::AnnotationsCreate | K::AnnotationsUpdate => {
            convert_result(response, |mut answer: serde_json::Value| {
                for at in ["/result/annotation", "/event/annotation"] {
                    if let Some(annotation) = answer.pointer_mut(at) {
                        *annotation = page_annotation_of(annotation.take(), box_of)?;
                    }
                }
                Ok(answer)
            })
        }

        // re-port (Task 8): these answer places in PDF space until their
        // owners add the conversion here: page names' layouts (8.18),
        // the outline (8.24), viewports (8.25),
        // form fields and widgets of the writes (8.20 to 8.22, 8.26), and
        // imports (8.22).
        K::PagesSetName
        | K::PagesRemoveName
        | K::AnnotationsImport
        | K::OutlineRead
        | K::MeasureViewports
        | K::FormsImport
        | K::FormsImportValues
        | K::FormsSetValue
        | K::FormsCreateField
        | K::FormsUpdateField
        | K::FormsSetSignatureAppearance
        | K::FormsAddWidget
        | K::FormsDetachWidget
        | K::FormsReset
        | K::FormsUpdateWidget
        | K::FormsDeleteWidget => Ok(response),

        // A change answers page space already: each item converts right
        // after its op ran ([`change_item_in_page_space`]).
        K::DocumentApply | K::DocumentApplyChanges => Ok(response),

        // No places: values, flags, names, bytes, pixels, metadata.
        K::Abort
        | K::AnnotationsDelete
        | K::AnnotationsExport
        | K::AnnotationsExportAppearance
        | K::AnnotationsFlatten
        | K::AnnotationsReadAppearance
        | K::AnnotationsReadFile
        | K::AnnotationsReorder
        | K::AttachmentsList
        | K::AttachmentsReadFile
        | K::Close
        | K::DocumentCheckPasswordPermissions
        | K::DocumentProbeSecurityFile
        | K::DocumentRenderPageFile
        | K::DocumentRenderPageFileEncoded
        | K::DocumentSaveBuffer
        | K::DocumentSaveFile
        | K::DocumentSaveLayerBuffer
        | K::DocumentSetFontSettings
        | K::DocumentVersion
        | K::FontsAddFallback
        | K::FontsAuthorizeEditing
        | K::FontsClear
        | K::FontsClearFallbacks
        | K::FontsRegister
        | K::FormsDeleteField
        | K::FormsExport
        | K::FormsReorderCalculations
        | K::FormsReorderWidgets
        | K::FormsRepair
        | K::LayerClose
        | K::MeasureSetScale
        | K::MetadataRead
        | K::MetadataReadCustom
        | K::MetadataUpdate
        | K::MetadataUpdateCustom
        | K::ObjectNumbersReserve
        | K::OpenFatMem
        | K::OpenLayerFileBase
        | K::OpenLayerMemBase
        | K::PagesExtract
        | K::PagesFlatten
        | K::PagesRenderEncoded
        | K::PagesText
        | K::PagesWorkingSet
        | K::PieceInfoApplications
        | K::PieceInfoDelete
        | K::PieceInfoRead
        | K::PieceInfoUpdate
        | K::RedactionApply
        | K::Shutdown
        | K::SignaturesAnalyze
        | K::SignaturesCancel
        | K::SignaturesComplete
        | K::SignaturesContents
        | K::SignaturesDigest
        | K::SignaturesFinalizeCandidate
        | K::SignaturesList
        | K::SignaturesPrepare
        | K::SignaturesRevisionBytes => Ok(response),
    }
}

/// A change op's places in the file's coordinates, measured right before it
/// runs (`opInFileSpace`). Matched with no catch-all: an op type added to
/// [`ChangeOp`] does not compile until it is placed here.
pub fn change_op_in_file_space(
    op: ChangeOp,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<ChangeOp, EngineError> {
    match op {
        // The data on the page it goes on.
        ChangeOp::AnnotationsCreate { page, data, object_number, resources } => {
            let visible = box_of(page)?;
            let data = super::annotations::pdf_annotation_draft_of(data, visible, box_of)?;
            Ok(ChangeOp::AnnotationsCreate { page, data, object_number, resources })
        }
        // The patch, and what it expects, on the annotation's page.
        ChangeOp::AnnotationsUpdate { reference, patch, resources, expect } => {
            let visible = box_of(reference.page())?;
            let patch = super::annotations::pdf_annotation_patch_of(patch, visible, box_of)?;
            let expect = expect
                .map(|expect| super::annotations::pdf_annotation_patch_of(expect, visible, box_of))
                .transpose()?;
            Ok(ChangeOp::AnnotationsUpdate { reference, patch, resources, expect })
        }
        // No places.
        op @ (ChangeOp::MetadataUpdate { .. }
        | ChangeOp::MetadataUpdateCustom { .. }
        | ChangeOp::AttachmentsCreate { .. }
        | ChangeOp::AttachmentsDelete { .. }) => Ok(op),
    }
}

/// A change item's places in page space, measured right after its op ran
/// (`itemInPageSpace`). Matched with no catch-all, as
/// [`change_op_in_file_space`] is.
pub fn change_item_in_page_space(
    item: ChangeItem,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<ChangeItem, EngineError> {
    match item {
        ChangeItem::AnnotationsCreate { page, annotation, meta } => {
            Ok(ChangeItem::AnnotationsCreate {
                page,
                annotation: page_annotation_of(annotation, box_of)?,
                meta,
            })
        }
        ChangeItem::AnnotationsUpdate { page, annotation, appearance, meta, skipped } => {
            Ok(ChangeItem::AnnotationsUpdate {
                page,
                annotation: page_annotation_of(annotation, box_of)?,
                appearance,
                meta,
                skipped,
            })
        }
        // No places.
        item @ (ChangeItem::MetadataUpdate { .. }
        | ChangeItem::MetadataUpdateCustom { .. }
        | ChangeItem::AttachmentsCreate { .. }
        | ChangeItem::AttachmentsDelete { .. }
        | ChangeItem::AttachmentsRestore { .. }
        | ChangeItem::AnnotationsDelete { .. }
        | ChangeItem::Skipped { .. }) => Ok(item),
    }
}

/// `options.target` in the file's coordinates, measured on the request's
/// `page` (`withRenderOptionsInFileSpace`). The page's box is read only when
/// the target is a rect.
fn render_options_in_file_space(
    body: &mut serde_json::Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<(), EngineError> {
    let Some(target) = body.get("options").and_then(|o| o.get("target")).filter(|t| !t.is_null())
    else {
        return Ok(());
    };
    let target: RenderTarget<PageRect> = from_value(target.clone())?;
    let target = match target {
        RenderTarget::Page => RenderTarget::Page,
        rect @ RenderTarget::Rect { .. } => {
            let page: PageRef = from_value(body.get("page").cloned().unwrap_or_default())?;
            pdf_render_target_of(rect, box_of(page.object_number())?)
        }
    };
    body["options"]["target"] = to_value(&target)?;
    Ok(())
}

/// Read a response's `result`, convert it, and write it back. The payloads
/// and any event ride through untouched; an error answer is left as it is.
fn convert_result<From: DeserializeOwned, To: Serialize>(
    response: HostResponse,
    convert: impl FnOnce(From) -> Result<To, EngineError>,
) -> Result<HostResponse, EngineError> {
    let mut envelope: serde_json::Value = serde_json::from_str(&response.json)
        .map_err(|e| unserializable(format!("the answer is not JSON: {e}")))?;
    let Some(result) = envelope.get_mut("result") else {
        return Ok(response);
    };
    let converted = convert(from_value(result.take())?)?;
    *result = to_value(&converted)?;
    let json = serde_json::to_string(&envelope).map_err(|e| unserializable(e.to_string()))?;
    Ok(HostResponse { json, payloads: response.payloads })
}

/// A value the wire sent, read as the type a conversion takes. A request
/// that isn't that shape is the caller's `WireFormat`.
fn from_value<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, EngineError> {
    serde_json::from_value(value).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::WireFormat,
            format!("a place is not the shape this operation takes: {e}"),
        )
    })
}

fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, EngineError> {
    serde_json::to_value(value).map_err(|e| unserializable(e.to_string()))
}

fn unserializable(message: String) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::WireFormat,
        format!("the result could not be serialized: {message}"),
    )
}

/// The visible boxes of a request's document, read only when a request or a
/// result asks for one (`WorkerHost.visibleBoxesFor`). Each is one page's
/// read through the session's index ([`crate::features::pages::
/// visible_box_of`]). A request that names no document has none:
/// `InvalidArg`, as upstream's is.
pub fn session_boxes<'a>(
    sessions: &'a crate::session::registry::SessionRegistry,
    key: Option<crate::session::registry::SessionKey>,
    kind: &'a str,
) -> impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + 'a {
    // Within one call: an answer with many destinations to one page reads
    // that page's boxes once.
    let mut boxes: std::collections::HashMap<u32, PdfRect> = std::collections::HashMap::new();
    move |page| {
        if let Some(known) = boxes.get(&page.get()) {
            return Ok(*known);
        }
        let key = key.as_ref().ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("{kind} has no document"),
            )
        })?;
        let visible =
            crate::features::pages::visible_box_of(sessions.session(key)?.document()?, page)?;
        boxes.insert(page.get(), visible);
        Ok(visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_reads_back_as_itself() {
        for (spelling, kind) in WireKind::ALL {
            assert_eq!(WireKind::of(spelling), Some(*kind));
            assert_eq!(kind.as_str(), *spelling);
        }
        assert_eq!(WireKind::ALL.len(), 89);
        assert_eq!(WireKind::of("pages.move"), None, "a removed kind is not on the wire");
    }

    fn never(_: PageObjectNumber) -> Result<PdfRect, EngineError> {
        panic!("no box is read for this request")
    }

    #[test]
    fn a_render_target_rect_is_measured_on_the_requests_page() {
        let mut body = serde_json::json!({
            "kind": "pages.render",
            "page": { "kind": "objectNumber", "objectNumber": 7 },
            "options": {
                "target": { "kind": "rect", "rect": { "x": 10, "y": 20, "width": 100, "height": 50 } },
                "viewport": { "kind": "scale", "scale": 2 },
            },
        });
        let mut box_of = |page: PageObjectNumber| {
            assert_eq!(page.get(), 7);
            Ok(PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 })
        };
        request_in_file_space(WireKind::PagesRender, &mut body, &mut box_of).unwrap();
        assert_eq!(
            body["options"]["target"],
            serde_json::json!({ "kind": "rect", "rect": { "left": 60.0, "bottom": 662.0, "right": 160.0, "top": 712.0 } })
        );
        assert_eq!(body["options"]["viewport"], serde_json::json!({ "kind": "scale", "scale": 2 }));
    }

    #[test]
    fn a_whole_page_render_reads_no_box() {
        for options in [
            serde_json::json!({ "target": { "kind": "page" } }),
            serde_json::json!({ "viewport": { "kind": "scale", "scale": 1 } }),
        ] {
            let mut body =
                serde_json::json!({ "kind": "pages.renderEncoded", "options": options.clone() });
            request_in_file_space(WireKind::PagesRenderEncoded, &mut body, &mut never).unwrap();
            if options.get("target").is_some() {
                assert_eq!(body["options"]["target"], serde_json::json!({ "kind": "page" }));
            } else {
                assert!(body["options"].get("target").is_none());
            }
        }
    }

    #[test]
    fn a_render_answers_its_area_in_page_space_and_keeps_its_pixels() {
        let response = HostResponse {
            json: serde_json::json!({ "result": {
                "width": 10, "height": 5, "stride": 40,
                "page": { "kind": "objectNumber", "objectNumber": 3 },
                "area": { "left": 60.0, "bottom": 662.0, "right": 160.0, "top": 712.0 },
            } })
            .to_string(),
            payloads: vec![vec![1, 2, 3]],
        };
        let mut box_of = |_| Ok(PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 });
        let converted = result_in_page_space(
            WireKind::PagesRender,
            &serde_json::Value::Null,
            response,
            &mut box_of,
        )
        .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&converted.json).unwrap();
        assert_eq!(
            envelope["result"]["area"],
            serde_json::json!({ "x": 10.0, "y": 20.0, "width": 100.0, "height": 50.0 })
        );
        assert_eq!(envelope["result"]["stride"], 40);
        assert_eq!(converted.payloads, vec![vec![1, 2, 3]]);
    }

    #[test]
    fn a_geometry_answer_is_measured_on_the_requests_page() {
        let request = serde_json::json!({
            "kind": "pages.geometry",
            "page": { "kind": "objectNumber", "objectNumber": 9 },
        });
        let response = HostResponse {
            json: serde_json::json!({ "result": { "runs": [{
                "rect": { "left": 60.0, "bottom": 662.0, "right": 160.0, "top": 712.0 },
                "start": 0,
                "glyphs": [{ "loose": { "left": 60.0, "bottom": 662.0, "right": 160.0, "top": 712.0 }, "space": true }],
            }] } })
            .to_string(),
            payloads: Vec::new(),
        };
        let mut box_of = |page: PageObjectNumber| {
            assert_eq!(page.get(), 9);
            Ok(PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 })
        };
        let converted =
            result_in_page_space(WireKind::PagesGeometry, &request, response, &mut box_of).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&converted.json).unwrap();
        let page_box = serde_json::json!({ "x": 10.0, "y": 20.0, "width": 100.0, "height": 50.0 });
        assert_eq!(
            envelope["result"],
            serde_json::json!({ "runs": [{
                "rect": page_box,
                "start": 0,
                "glyphs": [{ "loose": page_box, "space": true }],
            }] })
        );
    }

    #[test]
    fn a_search_answer_measures_each_match_on_its_own_page() {
        let segment = serde_json::json!({
            "quad": {
                "upperLeft": { "x": 60.0, "y": 712.0 }, "upperRight": { "x": 160.0, "y": 712.0 },
                "lowerLeft": { "x": 60.0, "y": 662.0 }, "lowerRight": { "x": 160.0, "y": 662.0 },
            },
            "rect": { "left": 60.0, "bottom": 662.0, "right": 160.0, "top": 712.0 },
            "advance": 1,
        });
        let hit = |page: u32| {
            serde_json::json!({
                "page": { "kind": "objectNumber", "objectNumber": page },
                "start": 3, "count": 4, "segments": [segment.clone()],
                "snippet": { "before": "a ", "match": "word", "after": " here" },
            })
        };
        let response = HostResponse {
            json: serde_json::json!({ "result": {
                "matches": [hit(4), hit(6)],
                "nextCursor": "c", "pagesSearched": 2, "pageCount": 9,
            } })
            .to_string(),
            payloads: Vec::new(),
        };
        let mut box_of = |page: PageObjectNumber| {
            Ok(match page.get() {
                4 => PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 },
                _ => PdfRect { left: 0.0, bottom: 0.0, right: 612.0, top: 792.0 },
            })
        };
        let converted = result_in_page_space(
            WireKind::SearchQuery,
            &serde_json::Value::Null,
            response,
            &mut box_of,
        )
        .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&converted.json).unwrap();
        let result = &envelope["result"];
        assert_eq!(
            result["matches"][0]["segments"][0]["rect"],
            serde_json::json!({ "x": 10.0, "y": 20.0, "width": 100.0, "height": 50.0 })
        );
        assert_eq!(
            result["matches"][0]["segments"][0]["quad"]["lowerRight"],
            serde_json::json!({ "x": 110.0, "y": 70.0 })
        );
        assert_eq!(
            result["matches"][1]["segments"][0]["rect"],
            serde_json::json!({ "x": 60.0, "y": 80.0, "width": 100.0, "height": 50.0 })
        );
        assert_eq!(result["matches"][1]["snippet"]["match"], "word");
        assert_eq!(result["matches"][1]["start"], 3);
        assert_eq!(
            (&result["nextCursor"], &result["pagesSearched"], &result["pageCount"]),
            (&serde_json::json!("c"), &serde_json::json!(2), &serde_json::json!(9))
        );
    }

    #[test]
    fn an_error_answer_crosses_unchanged() {
        let json = r#"{"error":{"name":"EngineError","code":"NotFound","message":"no page"}}"#;
        let response = HostResponse { json: json.into(), payloads: Vec::new() };
        let converted = result_in_page_space(
            WireKind::PagesRender,
            &serde_json::Value::Null,
            response,
            &mut never,
        )
        .unwrap();
        assert_eq!(converted.json, json);
    }
}
