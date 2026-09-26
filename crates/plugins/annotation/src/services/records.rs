//! `services/records.ts`: the bridge from the engine's records to the
//! model. Ingest a DTO with this session's authority projected onto it,
//! and fold a confirmed record back into the model.

use epdf_annotation_core::{
    Annot, AnnotationRef, Authority, Msg, PageObjectNumber, PdfRect, Source,
};
use serde_json::Value;

use crate::controller::AnnotationCapability;
use crate::repository::{annotation_key, from_dto};
use crate::services::authority::owner_of;
use crate::value::get;

/// The page object number a wire record names.
pub(crate) fn page_of_dto(dto: &Value) -> Option<PageObjectNumber> {
    get(dto, "page")
        .and_then(|p| get(p, "pageObjectNumber"))
        .and_then(Value::as_f64)
        .map(|p| p as PageObjectNumber)
}

impl AnnotationCapability {
    /// `records.ingest(dto, crop, source)`: the DTO with this session's
    /// per-record authority projected onto it.
    pub(crate) fn ingest(&self, dto: &Value, crop: PdfRect, source: Source) -> Annot {
        let mut a = from_dto(dto, crop, source);
        let Some(doc) = self.doc() else { return a };
        let sec = doc.security();
        let target = owner_of(Some(dto));
        a.authority = Some(Authority {
            update: sec.allows_annotation_mutation("update", &target),
            delete: sec.allows_annotation_mutation("delete", &target),
        });
        a
    }

    /// `records.sync(dto, source, bumpAp)`: re-sync one annotation from the
    /// authoritative DTO.
    pub(crate) fn sync(&self, dto: &Value, source: Source, bump_ap: bool) {
        let Some(pon) = page_of_dto(dto) else { return };
        if let Some(crop) = self.crop_of(pon) {
            let annot = self.ingest(dto, crop, source);
            self.commit(Msg::Upsert { annots: vec![annot], bump_ap: Some(bump_ap) });
        }
    }

    /// `records.pageOf(ref)`: the page it was ingested on, else the ref's own.
    pub(crate) fn page_of(&self, r: &AnnotationRef) -> PageObjectNumber {
        if let Some(a) = self.model().by_id.get(&annotation_key(r)) {
            return a.page.page_object_number;
        }
        match r {
            AnnotationRef::ObjectNumber { page, .. }
            | AnnotationRef::Nm { page, .. }
            | AnnotationRef::Index { page, .. } => page.page_object_number,
        }
    }
}
