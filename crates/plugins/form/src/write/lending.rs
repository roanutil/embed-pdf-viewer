//! `write/lending.ts`: design mode, the form's widgets lent to the
//! annotation plugin (`AnnotationLender`, its host contract). While the
//! session may design and the active tool doesn't fill, the annotation
//! plugin shows the widgets among its records and edits them with its
//! tools: select, move, resize, restyle, reorder, delete. Each edit comes
//! back here as the form's ops, in the same change as the rest of it.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use epdf_annotation_core::json::Object;
use epdf_annotation_core::record::ref_of;
use epdf_annotation_core::types::{Authority, Effect, Model, Painter};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_annotation::AnnotationLender;
use epdf_plugin_annotation::services::lending::LentRecords;
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::controller::Core;
use crate::model::{FieldIndex, field_for_widget, field_key_of};
use crate::sync::predict::undo_look_of;
use crate::value::{get, items, str_of};

/// `objectNumberOf(ref)`.
fn object_number_of(reference: &Value) -> f64 {
    if str_of(reference, "kind") == Some("objectNumber") {
        get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)
    } else {
        -1.0
    }
}

/// The form plugin's lender. The lent lists keep their identity while the
/// form and the lending don't change, as the TypeScript's `memo` does.
pub struct FormLender {
    core: Weak<Core>,
    #[allow(clippy::type_complexity)]
    memo: RefCell<Option<(bool, Rc<FieldIndex>, Rc<FieldIndex>, Option<LentRecords>)>>,
}

impl FormLender {
    pub fn new(core: &Core) -> Rc<FormLender> {
        Rc::new(FormLender { core: core.weak(), memo: RefCell::new(None) })
    }

    fn core(&self) -> Option<Rc<Core>> {
        self.core.upgrade()
    }
}

fn rows(index: &FieldIndex) -> Rc<Vec<Rc<Value>>> {
    Rc::new(index.widgets().to_vec())
}

impl Core {
    /// `deleteOps(deleted)`: deleted widgets as the form's ops: a widget
    /// leaves its field, and a field whose every widget goes is deleted whole.
    fn delete_ops(&self, deleted: &[Value]) -> Vec<Value> {
        let index = self.view();
        let mut ops: Vec<Value> = Vec::new();
        let mut by_field: IndexMap<String, (Value, Vec<Value>)> = IndexMap::new();
        for row in deleted {
            let reference = get(row, "ref").cloned().unwrap_or(Value::Null);
            match field_for_widget(&index, object_number_of(&reference)) {
                None => ops.push(json!({ "type": "forms.deleteWidget", "widget": reference })),
                Some(field) => {
                    by_field
                        .entry(field_key_of(field))
                        .or_insert_with(|| (field.clone(), Vec::new()))
                        .1
                        .push(row.clone());
                }
            }
        }
        for (field, widgets) in by_field.into_values() {
            let going: Vec<f64> = widgets
                .iter()
                .map(|row| object_number_of(get(row, "ref").unwrap_or(&Value::Null)))
                .collect();
            let all = items(&field, "widgets").iter().all(|w| {
                get(w, "objectNumber").and_then(Value::as_f64).is_some_and(|n| going.contains(&n))
            });
            if all {
                ops.push(json!({ "type": "forms.delete", "field": get(&field, "ref").cloned().unwrap_or(Value::Null) }));
            } else {
                ops.extend(widgets.iter().map(|row| {
                    json!({ "type": "forms.deleteWidget", "widget": get(row, "ref").cloned().unwrap_or(Value::Null) })
                }));
            }
        }
        ops
    }
}

impl AnnotationLender for FormLender {
    fn owner(&self) -> String {
        "form".into()
    }

    fn records(&self) -> Option<LentRecords> {
        let core = self.core()?;
        let lending = core.lending();
        let (confirmed, shown) = (core.truth(), core.view());
        if let Some((l, c, s, records)) = &*self.memo.borrow()
            && *l == lending
            && Rc::ptr_eq(c, &confirmed)
            && Rc::ptr_eq(s, &shown)
        {
            return records.clone();
        }
        let records = (lending && confirmed.snapshot.is_some() && shown.snapshot.is_some())
            .then(|| LentRecords { confirmed: rows(&confirmed), shown: rows(&shown) });
        *self.memo.borrow_mut() = Some((lending, confirmed, shown, records.clone()));
        records
    }

    fn ops_of(&self, effects: &[Effect], model: &Model, before: &Model) -> Vec<Value> {
        let Some(core) = self.core() else { return Vec::new() };
        let mut ops: Vec<Value> = Vec::new();
        let mut deleted: Vec<Value> = Vec::new();
        for effect in effects {
            match effect {
                Effect::Patch(patch) => {
                    let Some(reference) = ref_of(model.by_id.get(&patch.id).map(|r| &**r)) else {
                        continue;
                    };
                    let mut widget_patch = patch.patch.clone();
                    widget_patch.shift_remove("subtype");
                    ops.push(json!({ "type": "forms.updateWidget", "widget": reference, "patch": widget_patch }));
                }
                Effect::Delete(delete) => {
                    if let Some(record) = before.by_id.get(&delete.id) {
                        deleted.push(Value::Object((*record.annotation).clone()));
                    }
                }
                _ => {}
            }
        }
        ops.extend(core.delete_ops(&deleted));
        ops
    }

    fn undo_of(&self, ops: &[Value]) -> Vec<Value> {
        match self.core() {
            Some(core) => undo_look_of(&core.view(), ops),
            None => Vec::new(),
        }
    }

    fn reorder_ops(&self, page: &Value, refs: &[Value], position: &Value) -> Vec<Value> {
        vec![
            json!({ "type": "forms.reorderWidgets", "page": page, "widgets": refs, "position": position }),
        ]
    }

    fn authority(&self, _record: &Object) -> Authority {
        let may = self.core().is_some_and(|core| core.allows("doc.forms.modify"));
        Authority { update: may, delete: may, order: Default::default() }
    }

    fn picture_epoch(&self, page: &Value) -> String {
        self.core().map(|core| core.get_appearance_epoch(page)).unwrap_or_default()
    }

    fn picture_version(&self, reference: &Value) -> f64 {
        self.core().map_or(0.0, |core| core.picture_version_of_ref(reference))
    }

    fn pending(&self, reference: &Value) -> bool {
        self.core().is_some_and(|core| core.widget_is_pending(reference))
    }

    fn painter(&self, record: &Rc<Object>) -> Painter {
        let Some(core) = self.core() else { return Painter(None) };
        if record.get("subtype").and_then(Value::as_str) != Some("widget") {
            return Painter(None);
        }
        core.painter_of(&Value::Object((**record).clone()))
    }

    fn render_appearances(
        &self,
        page: &Value,
        scale: f64,
        signal: Option<AbortSignal>,
    ) -> Promise<Vec<Value>> {
        let Some(core) = self.core() else { return Promise::resolved(Vec::new()) };
        let pictures = core.render_appearances(page, scale, signal);
        pictures.convert(|list| {
            // A picture that does not serialise rejects the read, rather
            // than lending a `null` picture.
            list.iter()
                .map(|p| {
                    serde_json::to_value(p).map_err(|e| {
                        Error::new(format!("[form] a widget picture did not serialise: {e}"))
                    })
                })
                .collect()
        })
    }
}
