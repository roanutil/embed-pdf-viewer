//! Annotations out of a document as a bundle, and a bundle's annotations
//! into one.
//!
//! Task 8.22. Ports `services/src/features/annotations/AnnotationExporter.ts`
//! and `AnnotationImporter.ts` (83f6598d2, 843deedb4, 73f05157f).
//! Operations: `annotations.export`, `annotations.import`.
//!
//! No page is loaded or parsed either way: annotations are read raw, each
//! resource is exported from a raw handle, and an import makes each
//! annotation on a raw handle ([`super::write::create::create_batch`]). A
//! bundle is page space: an export measures each item from the top-left of
//! its page, and an import measures each create on the page it goes to.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::read::list::read_page_annotation_records;
use super::write::create::{
    AnnotationMutationMeta, BatchAttribution, BatchCreate, CreateServices, Planned,
    RestoredAttribution, create_batch, name_taken,
};
use super::write::draft::AnnotationActor;
use super::write::prepare::{DraftResources, check_draft};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::changes::op::Attribution;
use crate::features::transfer::annotation_bundle::{
    ANNOTATION_BUNDLE_FORMAT, ANNOTATION_BUNDLE_VERSION,
};
use crate::features::transfer::bundle::{Bundle, BundleImportPages, BundleKind};
use crate::features::transfer::export_selection::{
    AnnotationExportSelection, close_export_selection, ref_of,
};
use crate::features::transfer::limits::{
    BundleLimits, Limit, assert_within_limit, manifest_bytes_of, past_limit,
};
use crate::features::transfer::plan_annotation::{PlanInput, plan_annotation_import};
use crate::features::transfer::resources::{
    BundleResources, assert_image_within_limit, bundle_pages_of, check_wire_bundle, import_targets,
};
use crate::identity::PageRef;
use crate::session::document::DocumentSession;

fn unserializable(e: impl std::fmt::Display) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, format!("a record did not serialize: {e}"))
}

/// `annotations.export`: the annotations a selection takes
/// ([`close_export_selection`]) and the bytes beside them, as one bundle
/// (`AnnotationExporter.export`). A drawing is one resource however many
/// stamps place it, and equal bytes are one resource under their hash. It
/// is checked against `limits` as it is built, so an export never makes one
/// an import refuses.
pub fn export_annotations(
    session: &DocumentSession,
    selection: &AnnotationExportSelection,
    limits: &BundleLimits,
    cancel: &CancelToken,
) -> Result<Bundle, EngineError> {
    cancel.check()?;
    let owned = session.document()?;
    let document = owned.document();
    let pages: Vec<PageRef> = import_targets(owned).into_iter().map(|target| target.page).collect();
    // A bundle holds no widgets: the form travels in a form bundle.
    let annotations = close_export_selection(selection, &pages, &mut |page| {
        let page_index = owned.page_index(page.object_number())?;
        read_page_annotation_records(document, page_index, page.object_number(), cancel)?
            .iter()
            .map(|record| serde_json::to_value(record).map_err(unserializable))
            .collect()
    })?;
    assert_within_limit(BundleKind::Annotation, limits, Limit::Items, annotations.len() as u64)?;

    let mut collector = ResourceCollector {
        resources: BundleResources::new(BundleKind::Annotation, *limits),
        drawings: HashMap::new(),
    };
    let mut box_of = crate::features::pages::visible_box_reader(document);
    let mut items = Vec::with_capacity(annotations.len());
    for annotation in annotations {
        cancel.check()?;
        let resources = collector.of(owned, &annotation)?;
        let data = crate::page_space::annotations::page_annotation_of(annotation, &mut box_of)?;
        items.push(json!({ "data": data, "resources": resources }));
    }

    let datas: Vec<Value> = items.iter().map(|item| item["data"].clone()).collect();
    let pages = serde_json::to_value(bundle_pages_of(owned, &datas)?).map_err(unserializable)?;
    let page_count = pages.as_array().map_or(0, Vec::len) as u64;
    assert_within_limit(BundleKind::Annotation, limits, Limit::Pages, page_count)?;
    let manifest_bytes = manifest_bytes_of(&json!({ "pages": pages, "items": items }));
    assert_within_limit(BundleKind::Annotation, limits, Limit::ManifestBytes, manifest_bytes)?;
    assert_within_limit(
        BundleKind::Annotation,
        limits,
        Limit::BundleBytes,
        manifest_bytes + collector.resources.total_bytes,
    )?;
    let mut manifest = Map::new();
    manifest.insert("format".into(), json!(ANNOTATION_BUNDLE_FORMAT));
    manifest.insert("version".into(), json!(ANNOTATION_BUNDLE_VERSION));
    manifest.insert("pages".into(), pages);
    manifest.insert("items".into(), Value::Array(items));
    let Value::Object(manifest) =
        crate::features::transfer::bundle::js_numbers(Value::Object(manifest))
    else {
        unreachable!("an object stays an object")
    };
    Ok(Bundle { manifest, resources: collector.resources.by_id })
}

/// The resources an export's annotations name: drawings and attached files
/// (`ResourceCollector`).
struct ResourceCollector {
    resources: BundleResources,
    /// The id each shared drawing (by object number) was exported under.
    drawings: HashMap<u32, String>,
}

impl ResourceCollector {
    /// The resources `annotation` (a read, in the file's coordinates)
    /// names, by role: a stamp's appearance, a file attachment's file.
    fn of(
        &mut self,
        owned: &crate::access::OwnedDocument,
        annotation: &Value,
    ) -> Result<Value, EngineError> {
        let subtype = annotation["subtype"].as_str().unwrap_or_default();
        if subtype != "stamp" && subtype != "file-attachment" {
            return Ok(json!({}));
        }
        let reference =
            ref_of(annotation).ok_or_else(|| unserializable("a record names no ref"))?;
        let document = owned.document();
        let page_index = owned.page_index(reference.page())?;
        let index =
            super::write::resolve::resolve_annotation_index_raw(document, page_index, &reference)?;
        let raw = document
            .raw_annotation(page_index, index)
            .ok_or_else(|| unserializable("an annotation is not where it was read"))?;
        let mut found = Map::new();
        if subtype == "stamp" {
            if let Some(id) = self.appearance_of(document, &raw)? {
                found.insert("appearance".into(), json!(id));
            }
        } else if let Some(id) = self.file_of(&raw)? {
            found.insert("file".into(), json!(id));
        }
        Ok(Value::Object(found))
    }

    /// A stamp placing a drawing exports the drawing, once however many
    /// stamps place it; any other appearance exports as the page shows it,
    /// unturned (`appearanceOf`).
    fn appearance_of(
        &mut self,
        document: &pdfium::Document,
        annotation: &pdfium::Annotation<'_>,
    ) -> Result<Option<String>, EngineError> {
        let drawing = annotation.stamp_drawing();
        if let Some(known) = drawing.and_then(|drawing| self.drawings.get(&drawing)) {
            return Ok(Some(known.clone()));
        }
        let saved_failed = || {
            EngineError::with_message(EngineErrorCode::DocOpenFailed, "failed to save a drawing")
        };
        let bytes = match drawing {
            Some(drawing) => match document.export_drawing(drawing) {
                Some(exported) => Some(
                    exported
                        .save_to_buffer(pdfium::PdfSaveMode::Full)
                        .map_err(|_| saved_failed())?,
                ),
                None => None,
            },
            None => super::appearance::unturned_appearance(annotation)?,
        };
        let Some(bytes) = bytes else { return Ok(None) };
        let id = self.resources.keep(bytes)?;
        if let Some(drawing) = drawing {
            self.drawings.insert(drawing, id.clone());
        }
        Ok(Some(id))
    }

    /// A file attachment's file, within `resourceBytes` (`fileOf`).
    fn file_of(
        &mut self,
        annotation: &pdfium::Annotation<'_>,
    ) -> Result<Option<String>, EngineError> {
        let Some(attachment) = annotation.file_attachment() else { return Ok(None) };
        let max = self.resources.limits.resource_bytes;
        let bytes = attachment.extract(max).map_err(|error| match error {
            pdfium::AttachmentExtractError::SizeLimitExceeded => past_limit(
                format!(
                    "the annotation bundle is past its resourceBytes limit: an attached file is larger than {max}"
                ),
                json!({ "limit": "resourceBytes", "max": max }),
            ),
            other => crate::features::files::attachments::extract_error(other, Some(max)),
        })?;
        self.resources.keep(bytes).map(Some)
    }
}

/// What an import checks of each create before the first write
/// (`authorizeCreate`).
pub type AuthorizeCreate<'a> = dyn FnMut(&Map<String, Value>) -> Result<(), EngineError> + 'a;

/// An annotation import as its op asks for it (`AnnotationImportRequest`).
pub struct AnnotationImportRequest<'a> {
    /// Page space, as bundles are: each item converts on the page it goes
    /// to.
    pub bundle: &'a Bundle,
    pub pages: &'a BundleImportPages,
    pub attribution: Attribution,
    /// The session: whom `stamp` attributes each annotation to, as a create
    /// does, and whose user `restore` records as `importedBy`.
    pub actor: Option<&'a AnnotationActor>,
    pub limits: &'a BundleLimits,
}

/// `AnnotationImportResult`, in the file's coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationImportResult {
    /// In bundle order, each as it is now.
    pub annotations: Vec<Value>,
    /// Each imported annotation's ref in the bundle, and its ref here.
    pub ref_map: Vec<Value>,
    /// What was left out, in bundle order.
    pub dropped: Vec<Value>,
    pub meta: AnnotationMutationMeta,
}

/// `annotations.import`: a bundle's annotations, created in this document
/// as one change (`AnnotationImporter.import`). The bundle is checked first,
/// cheapest checks first: its shape, counts and sizes against the limits,
/// each resource against its id, and each image's decoded size, before any
/// image is decoded. Then [`plan_annotation_import`] maps pages and works
/// out what to leave out, each create is measured on the page it goes to,
/// `authorize_create` sees each create's draft before anything is written,
/// and [`create_batch`] writes them, all or nothing.
pub fn import_annotations(
    session: &DocumentSession,
    request: AnnotationImportRequest<'_>,
    authorize_create: &mut AuthorizeCreate<'_>,
    services: &CreateServices<'_>,
    cancel: &CancelToken,
) -> Result<AnnotationImportResult, EngineError> {
    cancel.check()?;
    let AnnotationImportRequest { bundle, pages, attribution, actor, limits } = request;
    check_wire_bundle(BundleKind::Annotation, bundle, limits)?;
    let mut items: Vec<Value> =
        bundle.manifest.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    for item in &items {
        if let Some(id) = item.pointer("/resources/appearance").and_then(Value::as_str)
            && let Some(bytes) = bundle.resource(id)
        {
            assert_image_within_limit(BundleKind::Annotation, limits, bytes)?;
        }
    }
    // A copy is a new annotation, so it gets a fresh name: a name the bundle
    // carries is neither kept nor checked.
    if attribution == Attribution::Stamp {
        for item in &mut items {
            item["data"]["nm"] = Value::Null;
        }
    }
    let owned = session.document()?;
    let document = owned.document();
    let bundle_pages = crate::features::transfer::bundle::bundle_pages_of(&bundle.manifest)?;
    let targets = import_targets(owned);
    let mut box_of = crate::features::pages::visible_box_reader(document);
    let plan = {
        let mut taken = |page: PageRef, nm: &str| -> Result<bool, EngineError> {
            Ok(name_taken(document, owned.page_index(page.object_number())?, nm))
        };
        let mut check = |page: PageRef, draft: &Map<String, Value>| -> Result<(), EngineError> {
            let visible = box_of(page.object_number())?;
            let converted = crate::page_space::annotations::pdf_annotation_draft_of(
                draft.clone(),
                visible,
                &mut box_of,
            )?;
            check_draft(converted).map(|_| ())
        };
        plan_annotation_import(PlanInput {
            pages: &bundle_pages,
            items: &items,
            page_mapping: pages,
            target: &targets,
            name_taken: &mut taken,
            check: &mut check,
        })?
    };

    let importer = actor.and_then(|actor| actor.user_id.clone()).filter(|id| !id.is_empty());
    let mut creates = Vec::with_capacity(plan.creates.len());
    for planned in &plan.creates {
        let item = &items[planned.item];
        let data = &item["data"];
        let visible = box_of(planned.page.object_number())?;
        let draft = crate::page_space::annotations::pdf_annotation_draft_of(
            planned.draft.clone(),
            visible,
            &mut box_of,
        )?;
        let role = |name: &str| {
            item.pointer(&format!("/resources/{name}"))
                .and_then(Value::as_str)
                .and_then(|id| bundle.resource(id))
        };
        let attribution = match attribution {
            // The session, in the group the item names, as `create` stamps it.
            Attribution::Stamp => {
                let group = draft.get("groupId").and_then(Value::as_str).map(str::to_owned);
                let stamped = match (actor, group) {
                    (Some(actor), Some(group)) => {
                        Some(AnnotationActor { group_id: Some(group), ..actor.clone() })
                    }
                    (None, Some(group)) => {
                        Some(AnnotationActor { group_id: Some(group), ..Default::default() })
                    }
                    (actor, None) => actor.cloned(),
                };
                BatchAttribution::Stamp(stamped)
            }
            Attribution::Restore => BatchAttribution::Restore {
                from: RestoredAttribution::of(data),
                imported_by: importer.clone(),
            },
        };
        let file_dates =
            (data["subtype"] == "file-attachment" && data["file"].is_object()).then(|| {
                let date = |key: &str| data["file"][key].as_str().map(str::to_owned);
                (date("createdAt"), date("modifiedAt"))
            });
        creates.push(BatchCreate {
            page: planned.page.object_number(),
            draft,
            reply_to: planned.reply_to.map(|(at, reply_type)| (Planned(at), reply_type)),
            parent: planned.parent.map(Planned),
            resources: DraftResources { appearance: role("appearance"), file: role("file") },
            attribution,
            file_dates,
            label: format!("import: item {}", planned.item),
        });
    }
    for create in &creates {
        authorize_create(&create.draft)?;
    }
    let created = create_batch(owned, &session.write_stamp()?, creates, services, cancel)?;
    let annotations: Vec<Value> = created
        .created
        .iter()
        .map(|record| serde_json::to_value(record).map_err(unserializable))
        .collect::<Result<_, _>>()?;
    let ref_map = plan
        .creates
        .iter()
        .zip(&annotations)
        .map(|(planned, annotation)| {
            json!({ "from": items[planned.item]["data"]["ref"], "to": annotation["ref"] })
        })
        .collect();
    Ok(AnnotationImportResult { annotations, ref_map, dropped: plan.dropped, meta: created.meta })
}
