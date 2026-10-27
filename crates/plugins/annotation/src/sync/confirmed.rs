//! `sync/confirmed.ts`: what happens when the engine confirms a change,
//! whoever made it. How the record renders follows who changed it (this
//! session's creates render live, another session's changes from the
//! engine's raster), and the record events fire. The data itself is already
//! in the records mirror when this runs.

use serde_json::{Value, json};

use crate::controller::Core;
use crate::js::key_of;
use crate::model::{prefer_baked, prefer_vector};

impl Core {
    /// `followConfirmedChanges(ctx, services, announce)`.
    pub(crate) fn follow_confirmed_changes(&self) {
        let me = self.weak();
        let off = self.events.records_changed.on(move |change| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            if change.cause == "load" {
                let pages =
                    if change.pages.is_null() { json!("all") } else { change.pages.clone() };
                core.events.resynced.emit(&json!({ "pages": pages }));
                return Ok(());
            }
            let Some(event) = &change.event else { return Ok(()) };
            let Some(origin) = event.get("origin") else { return Ok(()) };
            let remote = origin.get("kind").and_then(Value::as_str) == Some("remote");
            match event.get("type").and_then(Value::as_str).unwrap_or("") {
                "annotations.created" => {
                    let annotation = event.get("annotation").cloned().unwrap_or(Value::Null);
                    if let Some(key) = annotation.get("ref").and_then(key_of) {
                        let ids = [key];
                        core.update_state(|s| {
                            if remote { prefer_baked(s, &ids) } else { prefer_vector(s, &ids) }
                        });
                    }
                    core.events
                        .created
                        .emit(&json!({ "annotation": annotation, "origin": origin }));
                }
                "annotations.updated" => {
                    let annotation = event.get("annotation").cloned().unwrap_or(Value::Null);
                    if remote && let Some(key) = annotation.get("ref").and_then(key_of) {
                        let ids = [key];
                        core.update_state(|s| prefer_baked(s, &ids));
                    }
                    core.events
                        .updated
                        .emit(&json!({ "annotation": annotation, "origin": origin }));
                }
                "annotations.deleted" => {
                    core.events.deleted.emit(&json!({
                        "refs": event.get("deleted").cloned().unwrap_or(json!([])),
                        "page": event.get("page").cloned().unwrap_or(Value::Null),
                        "origin": origin,
                    }));
                }
                "annotations.reordered" => {
                    core.events.reordered.emit(&json!({
                        "page": event.get("page").cloned().unwrap_or(Value::Null),
                        "order": event.get("order").cloned().unwrap_or(json!([])),
                        "origin": origin,
                    }));
                }
                _ => {}
            }
            Ok(())
        });
        self.cx.listen(off);
    }
}
