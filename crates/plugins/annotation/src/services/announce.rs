//! `services/announce.ts`: confirmed record changes with their page-space
//! projection, the one shape `onCreated`, `onUpdated` and `onDeleted`
//! carry, whoever caused the change.

use epdf_annotation_core::{AnnotationRef, PageRef};
use serde_json::Value;

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::value::get;

impl AnnotationCapability {
    fn changed_event(&self, dto: &Value, origin: ChangeOrigin) -> Option<AnnotationChangedEvent> {
        let ref_: AnnotationRef =
            get(dto, "ref").and_then(|r| serde_json::from_value(r.clone()).ok())?;
        let page: PageRef =
            get(dto, "page").and_then(|p| serde_json::from_value(p.clone()).ok())?;
        Some(AnnotationChangedEvent {
            annotation: self.project_ref(&ref_),
            ref_,
            page,
            subtype: get(dto, "subtype").cloned().unwrap_or(Value::Null),
            raw: dto.clone(),
            origin,
        })
    }

    pub(crate) fn announce_created(&self, dto: &Value, origin: ChangeOrigin) {
        if let Some(event) = self.changed_event(dto, origin) {
            self.0.events.created.emit(&event);
        }
    }

    pub(crate) fn announce_updated(&self, dto: &Value, origin: ChangeOrigin) {
        if let Some(event) = self.changed_event(dto, origin) {
            self.0.events.updated.emit(&event);
        }
    }

    pub(crate) fn announce_deleted(
        &self,
        ref_: AnnotationRef,
        page: PageRef,
        origin: ChangeOrigin,
    ) {
        self.0.events.deleted.emit(&AnnotationDeletedEvent { ref_, page, origin });
    }
}
