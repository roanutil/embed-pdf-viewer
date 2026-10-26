//! `sync/fields.ts`: the field tree, mirrored from the engine. It changes
//! only by loads and by confirmed form events, whoever caused them: the
//! plugin's own writes, the document's scripts, or another session. Its
//! view shows this session's pending changes on top (`predict_form_op`).
//! This is also the one place the value and field events fire.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::mirror::{Fold, Loaded, Mirror, MirrorSpec};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Controller, Error};
use serde_json::{Value, json};

use crate::model::{FieldIndex, Folded, fold_form_event, index_loaded_fields};
use crate::services::events::Events;
use crate::services::index_cache::IndexCache;
use crate::sync::predict::predict_form_op;
use crate::value::{get, str_of};

/// `createFieldsMirror(ctx, events)`.
pub fn create_fields_mirror(
    cx: &Controller<Value, Value>,
    events: &Events,
    indexes: &Rc<IndexCache>,
) -> Result<Mirror, Error> {
    // What `fields.get()` reads inside a load: the value every applied
    // change left (the mirror announces each one), so the closures need no
    // handle on the mirror itself.
    let empty = Rc::new(FieldIndex::default().to_mirror());
    let latest: Rc<RefCell<Rc<Value>>> = Rc::new(RefCell::new(empty.clone()));

    let (load_latest, load_indexes) = (latest.clone(), indexes.clone());
    let load: epdf_kernel::mirror::Load = Rc::new(move |doc, _signal| {
        let Some(doc) = doc else {
            return Promise::rejected(Error::new("[form] no document to read"));
        };
        let call = doc.call("forms.list", None, Vec::new());
        let (latest, indexes) = (load_latest.clone(), load_indexes.clone());
        Promise::spawn(async move {
            let snapshot = call.await?;
            let previous = indexes.index(&latest.borrow().clone());
            let next = Rc::new(index_loaded_fields(&previous, Rc::new(snapshot)));
            Ok(Loaded { value: (*indexes.value(&next)).clone(), cursor: None })
        })
    });

    let fold_indexes = indexes.clone();
    let fold: Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>> =
        Rc::new(move |value: Rc<Value>, event: &Value| {
            let index = fold_indexes.index(&value);
            Ok(match fold_form_event(&index, event) {
                Folded::Reload => Fold::Reload,
                Folded::Index(next) if Rc::ptr_eq(&next, &index) => Fold::Value(value),
                Folded::Index(next) => Fold::Value(fold_indexes.value(&next)),
            })
        });

    let mut spec = MirrorSpec::new("fields", (*empty).clone(), load, fold);
    // A reviewer-shaped token without `doc.forms.read` is a common narrowed
    // scope: skip the doomed read and report `forbidden`.
    let readable_cx = cx.cx.clone();
    spec.readable = Some(Rc::new(move || readable_cx.allows("doc.forms.read")));
    let predict_indexes = indexes.clone();
    spec.predict = Some(Rc::new(move |value: Rc<Value>, op: &Value| {
        let index = predict_indexes.index(&value);
        let next = predict_form_op(&index, op);
        Ok(if Rc::ptr_eq(&next, &index) { value } else { predict_indexes.value(&next) })
    }));

    let value_changed = events.value_changed.clone();
    let field_created = events.field_created.clone();
    let field_updated = events.field_updated.clone();
    let field_deleted = events.field_deleted.clone();
    let resynced = events.resynced.clone();
    let changed_indexes = indexes.clone();
    spec.changed = Some(Rc::new(move |change| {
        *latest.borrow_mut() = change.next.clone();
        if change.cause == "load" {
            let next = changed_indexes.index(&change.next);
            if let Some(snapshot) = &next.snapshot {
                resynced.emit(&json!({ "snapshot": **snapshot }));
            }
            return Ok(());
        }
        let Some(event) = &change.event else { return Ok(()) };
        let Some(origin) = event.get("origin") else { return Ok(()) };
        let field = || get(event, "field").cloned().unwrap_or(Value::Null);
        let payload = |field: Value| json!({ "field": field, "origin": origin });
        match str_of(event, "type").unwrap_or("") {
            "forms.valueSet" => value_changed.emit(&payload(field())),
            "forms.created" => field_created.emit(&payload(field())),
            "forms.updated" | "forms.widgetAdded" | "forms.widgetRemoved" => {
                field_updated.emit(&payload(field()));
            }
            "forms.widgetDeleted" | "forms.widgetRestored" => {
                if crate::value::truthy(get(event, "field")) {
                    field_updated.emit(&payload(field()));
                }
            }
            "forms.deleted" => {
                if let Some(deleted) =
                    get(event, "deleted").filter(|d| crate::value::truthy(Some(d)))
                {
                    field_deleted.emit(&json!({ "ref": deleted, "origin": origin }));
                }
            }
            _ => {}
        }
        Ok(())
    }));
    cx.mirror(spec)
}
