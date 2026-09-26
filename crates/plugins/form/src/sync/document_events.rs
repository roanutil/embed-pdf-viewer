//! `sync/document-events.ts`: confirmed document facts, announced and then
//! reconciled. Own, script and remote writes all surface through the
//! document event stream, so every consumer sees one publication path.

use std::rc::Rc;

use serde_json::{Map, Value, json};

use crate::contract::{FormFieldChangedEvent, FormValueChangedEvent};
use crate::controller::FormCapability;
use crate::value::{get, member, str_of};

const STRUCTURAL: &[&str] = &[
    "form.fieldCreated",
    "form.fieldUpdated",
    "form.fieldDeleted",
    "form.widgetAttached",
    "form.widgetDetached",
    "form.repaired",
];

/// `originOf(event)`: the event's own `sessionId` member, so a missing one
/// stays missing, as the TypeScript's object literal leaves it.
fn origin_of(event: &Value) -> Value {
    let origin = event.get("origin").cloned().unwrap_or(Value::Null);
    let remote = origin.get("kind").and_then(Value::as_str) == Some("remote");
    let mut out = Map::new();
    out.insert("locality".into(), json!(if remote { "remote" } else { "local" }));
    out.insert("trigger".into(), json!("unknown"));
    if let Some(session) = get(&origin, "sessionId") {
        out.insert("sessionId".into(), session.clone());
    }
    out.insert(
        "actorId".into(),
        get(&origin, "sub").filter(|s| !s.is_null()).cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

fn announce(form: &FormCapability, event: &Value) {
    if event.get("origin").is_none() {
        return;
    }
    let origin = origin_of(event);
    let events = &form.0.events;
    let field = || get(event, "field").cloned().unwrap_or(Value::Null);
    let field_ref = || get(&field(), "ref").cloned().unwrap_or(Value::Null);
    match str_of(event, "type").unwrap_or_default() {
        "form.valueChanged" => events.value_changed.emit(&FormValueChangedEvent {
            field_ref: field_ref(),
            field: field(),
            origin,
        }),
        "form.fieldCreated" => events.field_created.emit(&FormFieldChangedEvent {
            field_ref: field_ref(),
            field: Some(field()),
            origin,
        }),
        "form.fieldUpdated" | "form.widgetAttached" | "form.widgetDetached" => events
            .field_updated
            .emit(&FormFieldChangedEvent { field_ref: field_ref(), field: Some(field()), origin }),
        "form.fieldDeleted" => events.field_deleted.emit(&FormFieldChangedEvent {
            field_ref: json!({
                "kind": "objectNumber",
                "fieldObjectNumber": member(event, "deletedFieldObjectNumber"),
            }),
            field: None,
            origin,
        }),
        _ => {}
    }
}

/// `subscribeDocumentEvents(ctx, services, hydration)`.
pub fn subscribe_document_events(form: &FormCapability) {
    let Some(doc) = form.doc() else { return };
    let this = form.clone();
    let off = doc.subscribe(Rc::new(move |event: &Value| {
        announce(&this, event);
        let kind = str_of(event, "type").unwrap_or_default();
        if kind == "stream.desynced" {
            this.refresh().ignore();
            return;
        }
        if kind == "document.versioned" || kind == "signature.completed" {
            this.refresh().ignore();
            return;
        }
        let Some(origin) = event.get("origin").filter(|_| kind.starts_with("form.")) else {
            return;
        };
        let structural = STRUCTURAL.contains(&kind);
        // Own non-structural writes refreshed already.
        if str_of(origin, "kind") != Some("remote") && !structural {
            return;
        }
        if structural {
            this.clear_geom();
        }
        this.refresh().ignore();
    }));
    form.defer_fn(move || off.call());
}
