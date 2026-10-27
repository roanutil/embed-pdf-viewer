//! `sync/document-events.ts`: every confirmed apply, from this session or
//! another, arrives as one `redaction.applied` document event, the only
//! place `last_result` changes and `on_applied` fires. Annotation state and
//! raster invalidation are handled by their own plugins. Marks are
//! annotations, so the pending view changes whenever a `redact` annotation
//! does.

use std::rc::Rc;

use epdf_kernel::Error;
use serde_json::{Map, Value};

use crate::contract::{RedactionAppliedEvent, RedactionPendingChangedEvent};
use crate::controller::RedactionCapability;
use crate::model::set_last_result;

impl RedactionCapability {
    /// `subscribeChanges(ctx, services)`: run once, from the plugin's
    /// `connect`.
    pub(crate) fn subscribe_changes(&self) -> Result<(), Error> {
        let core = self.core();
        let cx = &core.cx;

        let this = self.clone();
        let off = cx.doc()?.subscribe(Rc::new(move |event: &Value| {
            if event.get("type").and_then(Value::as_str) != Some("redaction.applied") {
                return;
            }
            // `const { type, origin, ...result } = event`.
            let members = event.as_object().into_iter().flatten();
            let result: Map<String, Value> = members
                .filter(|(key, _)| key.as_str() != "type" && key.as_str() != "origin")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let origin = event.get("origin").cloned().unwrap_or_else(epdf_kernel::undefined);
            let result = Rc::new(Value::Object(result));
            let core = this.core();
            core.cx.update_state(|state| set_last_result(state, &result));
            core.events.applied.emit(&RedactionAppliedEvent { result, origin });
        }));
        cx.listen(off);

        let annotation = &core.siblings.annotation;
        let mark_changed = |pending: epdf_kernel::event_hook::EventHookSource<
            RedactionPendingChangedEvent,
        >| {
            move |event: &Value| {
                let changed = event.get("annotation");
                if changed.and_then(|a| a.get("subtype")).and_then(Value::as_str) == Some("redact")
                {
                    let page = changed.and_then(|a| a.get("page")).cloned();
                    pending.emit(&RedactionPendingChangedEvent {
                        pages: vec![page.unwrap_or_else(epdf_kernel::undefined)],
                    });
                }
                Ok(())
            }
        };
        cx.listen(annotation.on_created(mark_changed(core.events.pending_changed.clone())));
        cx.listen(annotation.on_updated(mark_changed(core.events.pending_changed.clone())));
        // A deleted record has no subtype any more; the page's list changed
        // either way.
        let pending = core.events.pending_changed.clone();
        cx.listen(annotation.on_deleted(move |event: &Value| {
            let page = event.get("page").cloned().unwrap_or_else(epdf_kernel::undefined);
            pending.emit(&RedactionPendingChangedEvent { pages: vec![page] });
            Ok(())
        }));
        Ok(())
    }
}
