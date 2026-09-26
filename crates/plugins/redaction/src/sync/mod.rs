//! `sync/document-events.ts`: a remote collaborator's apply arrives only as
//! a document event (own applies flow through the apply area). Annotation
//! state and raster invalidation are handled by their own planes; here it
//! is surfaced. Marks are annotations, so the pending view changes whenever
//! a `redact` annotation does.

use std::rc::Rc;

use epdf_kernel::types::origin_of;
use serde_json::Value;

use crate::contract::{RedactionAppliedEvent, RedactionPendingChangedEvent};
use crate::controller::RedactionCapability;
use crate::model::RedactionAction;

impl RedactionCapability {
    /// `subscribeChanges(ctx, services)`: run once, from the plugin's
    /// `connect`.
    pub fn subscribe_changes(&self) {
        let cx = &self.0.cx;
        if let Ok(doc) = cx.doc() {
            let this = self.clone();
            let off = doc.subscribe(Rc::new(move |event: &Value| {
                if event.get("type").and_then(Value::as_str) != Some("redaction.applied") {
                    return;
                }
                let origin = event.get("origin").and_then(|o| o.get("kind"));
                if origin.and_then(Value::as_str) != Some("remote") {
                    return;
                }
                this.0.cx.dispatch(RedactionAction::ApplyFinished { result: Some(event.clone()) });
                this.0.events.applied.emit(&RedactionAppliedEvent {
                    result: event.clone(),
                    origin: origin_of(event),
                });
            }));
            cx.listen(off);
        }

        let pending = self.0.events.pending_changed.clone();
        let mark = move |event: &epdf_plugin_annotation::AnnotationChangedEvent| {
            if event.subtype.as_str() == Some("redact") {
                pending.emit(&RedactionPendingChangedEvent { pages: vec![event.page] });
            }
            Ok(())
        };
        cx.listen(self.annotation().on_created(mark.clone()));
        cx.listen(self.annotation().on_updated(mark));
        let pending = self.0.events.pending_changed.clone();
        cx.listen(self.annotation().on_deleted(move |event| {
            // A deleted record has no subtype any more; the page's list
            // changed either way.
            pending.emit(&RedactionPendingChangedEvent { pages: vec![event.page] });
            Ok(())
        }));
    }
}
