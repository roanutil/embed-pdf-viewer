//! `sync/remote.ts`: folds remote annotation events into the model.
//! Hydration queues and replays through the same path, so a remote change
//! lands identically whether it arrived live or inside a hydration window.

use epdf_annotation_core::{AnnotationRef, Msg, PageRef, Source, is_substrate_only};
use serde_json::Value;

use crate::controller::AnnotationCapability;
use crate::repository::annotation_key;
use crate::services::events::origin_of;
use crate::services::records::page_of_dto;
use crate::value::{get, str_of, truthy};

impl AnnotationCapability {
    /// `upsertRemote(dtos, bumpAp)`: `bump` re-fetches rasters, `keep`
    /// doesn't; conversation-plane records never bump.
    fn upsert_remote(&self, dtos: &[Value], bump_ap: bool) {
        let mut bump = Vec::new();
        let mut keep = Vec::new();
        for dto in dtos {
            let Some(crop) = page_of_dto(dto).and_then(|p| self.crop_of(p)) else { continue };
            let a = self.ingest(dto, crop, Source::Baked);
            if bump_ap && !is_substrate_only(&a) { bump.push(a) } else { keep.push(a) }
        }
        if !bump.is_empty() {
            self.commit(Msg::Upsert { annots: bump, bump_ap: Some(true) });
        }
        if !keep.is_empty() {
            self.commit(Msg::Upsert { annots: keep, bump_ap: None });
        }
    }

    /// `remote.apply(event)`.
    pub(crate) fn apply_remote(&self, event: &Value) {
        match str_of(event, "type") {
            Some("annotation.created") => {
                let dto = get(event, "created").cloned().unwrap_or(Value::Null);
                self.upsert_remote(std::slice::from_ref(&dto), true);
                self.announce_created(&dto, origin_of(event));
            }
            Some("annotation.updated") => {
                let dto = get(event, "updated").cloned().unwrap_or(Value::Null);
                let changed = get(event, "appearance").and_then(|a| get(a, "changed"));
                self.upsert_remote(std::slice::from_ref(&dto), truthy(changed));
                self.announce_updated(&dto, origin_of(event));
            }
            Some("annotation.moved") => {
                let moved =
                    get(event, "moved").and_then(Value::as_array).cloned().unwrap_or_default();
                self.upsert_remote(&moved, false);
            }
            Some("annotation.deleted") => {
                let Some(deleted) = get(event, "deleted").filter(|d| truthy(Some(d))) else {
                    return;
                };
                let page: PageRef = get(event, "page")
                    .and_then(|p| serde_json::from_value(p.clone()).ok())
                    .unwrap_or_default();
                let r = ref_from_stable_id(page, deleted);
                let key = annotation_key(&r);
                let gone = self.model().by_id.get(&key).cloned();
                if let Some(gone) = gone {
                    self.commit(Msg::Remove { ids: vec![key] });
                    if let Some(gone_ref) = gone.ref_ {
                        self.announce_deleted(gone_ref, gone.page, origin_of(event));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `refFromStableId(page, id)`: an address rebuilt from an event's page and
/// stable id.
pub fn ref_from_stable_id(page: PageRef, id: &Value) -> AnnotationRef {
    if str_of(id, "kind") == Some("objectNumber") {
        AnnotationRef::ObjectNumber {
            page,
            annot_object_number: get(id, "value").and_then(Value::as_f64).unwrap_or(f64::NAN),
        }
    } else {
        AnnotationRef::Nm {
            page,
            nm: match get(id, "value") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => "undefined".into(),
            },
        }
    }
}
