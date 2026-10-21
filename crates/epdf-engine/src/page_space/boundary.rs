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

use super::destinations::page_document_actions_of;
use super::pages::page_list_of;
use super::rendering::{page_render_area_of, pdf_render_target_of};
use super::{PdfCoordinates, VisibleBoxOf};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::document::DocumentActionsSnapshot;
use crate::features::pages::PageListSnapshot;
use crate::features::render::geometry::RenderTarget;
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

        // re-port (Task 8): these take places in page space upstream and
        // convert here once their owners move them: annotation drafts
        // (8.14), patches (8.15), widget placements and patches and field
        // drafts (8.21), a measure (8.25), and each `document.apply` op as
        // it runs (8.6, `ChangeSpace`).
        K::AnnotationsCreate
        | K::AnnotationsUpdate
        | K::FormsUpdateWidget
        | K::FormsCreateField
        | K::FormsAddWidget
        | K::MeasureSetScale
        | K::DocumentApply
        | K::DocumentApplyChanges => Ok(()),

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
/// itself.
pub fn result_in_page_space(
    kind: WireKind,
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

        // re-port (Task 8): these answer places in PDF space until their
        // owners add the conversion here: page names' layouts (8.18), page
        // geometry (8.9), search slices (8.11), annotation reads and writes
        // (8.12, 8.14, 8.15), appearance rects (8.13), the outline (8.24),
        // viewports (8.25), form snapshots, fields and widgets (8.19 to
        // 8.22, 8.26), and imports (8.22). `document.apply` answers page
        // space already, each op converting as it runs (8.6).
        K::PagesSetName
        | K::PagesRemoveName
        | K::PagesGeometry
        | K::SearchQuery
        | K::AnnotationsList
        | K::AnnotationsCreate
        | K::AnnotationsUpdate
        | K::AnnotationsImport
        | K::AnnotationsRenderAppearances
        | K::AnnotationsRenderAppearancesEncoded
        | K::OutlineRead
        | K::MeasureViewports
        | K::FormsList
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
        | K::FormsDeleteWidget
        | K::DocumentApply
        | K::DocumentApplyChanges => Ok(response),

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
/// result asks for one (`WorkerHost.visibleBoxesFor`). A request that names
/// no document has none: `InvalidArg`, as upstream's is.
pub fn session_boxes<'a>(
    sessions: &'a crate::session::registry::SessionRegistry,
    key: Option<crate::session::registry::SessionKey>,
    kind: &'a str,
) -> impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + 'a {
    type Reader<'r> = Box<dyn FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + 'r>;
    let mut reader: Option<Reader<'a>> = None;
    move |page| {
        if reader.is_none() {
            let key = key.as_ref().ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("{kind} has no document"),
                )
            })?;
            let document = sessions.session(key)?.document()?.document();
            reader = Some(Box::new(crate::features::pages::visible_box_reader(document)));
        }
        match reader.as_mut() {
            Some(read) => read(page),
            None => unreachable!("set above"),
        }
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
        let converted = result_in_page_space(WireKind::PagesRender, response, &mut box_of).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&converted.json).unwrap();
        assert_eq!(
            envelope["result"]["area"],
            serde_json::json!({ "x": 10.0, "y": 20.0, "width": 100.0, "height": 50.0 })
        );
        assert_eq!(envelope["result"]["stride"], 40);
        assert_eq!(converted.payloads, vec![vec![1, 2, 3]]);
    }

    #[test]
    fn an_error_answer_crosses_unchanged() {
        let json = r#"{"error":{"name":"EngineError","code":"NotFound","message":"no page"}}"#;
        let response = HostResponse { json: json.into(), payloads: Vec::new() };
        let converted = result_in_page_space(WireKind::PagesRender, response, &mut never).unwrap();
        assert_eq!(converted.json, json);
    }
}
