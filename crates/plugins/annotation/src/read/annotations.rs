//! `read/annotations.ts`: the public reads, the engine's records as the user
//! sees them. Each is the annotation a record in the view holds, so a read
//! is the same until its annotation changes, and a new annotation reads
//! before the engine confirms it. A lent record reads by its ref and in the
//! selection, and lists with its owner's.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::editable::is_editable_by_hand;
use epdf_annotation_core::types::{Model, ModelAnnotation};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::Value;

use crate::controller::{CAPABILITY, Core};
use crate::js::{key_or_none, page_object_number};
use crate::sync::records::{by_key, same_record};

#[derive(Default)]
pub struct ReadsState {
    list: RefCell<Option<(Rc<Model>, Rc<Vec<Value>>)>>,
    page_lists: RefCell<HashMap<String, (Rc<Model>, Rc<Vec<Value>>)>>,
    selected: RefCell<Option<(Rc<Model>, Rc<Vec<Value>>)>>,
}

/// `AnnotationFilter`: pages (refs or indexes) and a subtype.
#[derive(Clone, Debug, Default)]
pub struct AnnotationFilter {
    pub pages: Option<Vec<Value>>,
    pub subtype: Option<String>,
}

impl AnnotationFilter {
    pub fn read(value: &Value) -> AnnotationFilter {
        let present = |k: &str| value.get(k).filter(|v| !epdf_kernel::is_undefined(v));
        AnnotationFilter {
            pages: present("pages").and_then(Value::as_array).cloned(),
            subtype: present("subtype").and_then(Value::as_str).map(str::to_string),
        }
    }
}

fn annotation_value(record: &ModelAnnotation) -> Value {
    Value::Object((*record.annotation).clone())
}

impl Core {
    /// `get(ref)`.
    pub fn get(&self, reference: &Value) -> Option<Value> {
        self.model().by_id.get(&key_or_none(reference)).map(|r| annotation_value(r))
    }

    /// `isPending(ref)`: it shows other than the engine has it.
    pub fn is_pending(&self, reference: &Value) -> bool {
        let key = key_or_none(reference);
        let shown = self.records.view();
        let truths = self.records.get();
        !same_record(by_key(&shown).get(&key), by_key(&truths).get(&key))
    }

    /// The page numbers a filter names; `None` without a page filter.
    fn pages_of(&self, filter: &AnnotationFilter) -> Option<Vec<f64>> {
        let pages = filter.pages.as_ref()?;
        Some(
            pages
                .iter()
                .filter_map(|p| self.get_page(p))
                .filter_map(|info| info.get("ref").map(page_object_number))
                .collect(),
        )
    }

    /// `listAnnots(filter)`: the records in drawing order, lent ones aside.
    pub(crate) fn list_annots(&self, filter: &AnnotationFilter) -> Vec<Rc<ModelAnnotation>> {
        let model = self.model();
        let pages = self.pages_of(filter);
        model
            .order
            .iter()
            .filter_map(|id| model.by_id.get(id))
            .filter(|record| {
                !self.lending.owns(&record.id)
                    && pages
                        .as_ref()
                        .is_none_or(|p| record.page_number().is_some_and(|n| p.contains(&n)))
                    && filter.subtype.as_ref().is_none_or(|s| record.subtype() == s)
            })
            .cloned()
            .collect()
    }

    /// `list(filter)`: unfiltered and one-page lists are memoized per model.
    pub fn list(&self, filter: Option<&AnnotationFilter>) -> Rc<Vec<Value>> {
        let model = self.model();
        let values = |records: Vec<Rc<ModelAnnotation>>| -> Rc<Vec<Value>> {
            Rc::new(records.iter().map(|r| annotation_value(r)).collect())
        };
        let unfiltered = filter.is_none_or(|f| f.pages.is_none() && f.subtype.is_none());
        if unfiltered {
            if let Some((m, v)) = &*self.reads.list.borrow()
                && Rc::ptr_eq(m, &model)
            {
                return v.clone();
            }
            let list = values(self.list_annots(&AnnotationFilter::default()));
            *self.reads.list.borrow_mut() = Some((model, list.clone()));
            return list;
        }
        let filter = filter.expect("a filter");
        let one_page =
            filter.pages.as_ref().filter(|p| p.len() == 1).and_then(|p| self.get_page(&p[0]));
        if let Some(page) = one_page
            && filter.subtype.is_none()
        {
            let key = crate::js::stringify(page.get("ref").and_then(|r| r.get("objectNumber")));
            if let Some((m, v)) = self.reads.page_lists.borrow().get(&key)
                && Rc::ptr_eq(m, &model)
            {
                return v.clone();
            }
            let list = values(self.list_annots(filter));
            self.reads.page_lists.borrow_mut().insert(key, (model, list.clone()));
            return list;
        }
        values(self.list_annots(filter))
    }

    /// `listSelected()`: the selected annotations, memoized per model.
    pub fn list_selected(&self) -> Rc<Vec<Value>> {
        let model = self.model();
        if let Some((m, v)) = &*self.reads.selected.borrow()
            && Rc::ptr_eq(m, &model)
        {
            return v.clone();
        }
        let list: Rc<Vec<Value>> = Rc::new(
            model
                .selected
                .iter()
                .filter_map(|id| model.by_id.get(id))
                .map(|r| annotation_value(r))
                .collect(),
        );
        *self.reads.selected.borrow_mut() = Some((model, list.clone()));
        list
    }

    /// `pageOf(ref)` (reads): where the record was read, else the ref's own page.
    pub(crate) fn annotation_page_of(&self, reference: &Value) -> Value {
        self.model()
            .by_id
            .get(&key_or_none(reference))
            .and_then(|r| r.annotation.get("page").cloned())
            .or_else(|| reference.get("page").cloned())
            .unwrap_or(Value::Null)
    }

    /// `loadedOrThrow(ref)`.
    #[allow(dead_code)]
    pub(crate) fn loaded_or_throw(&self, reference: &Value) -> Result<Rc<ModelAnnotation>, Error> {
        self.model().by_id.get(&key_or_none(reference)).cloned().ok_or_else(|| {
            plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                "annotation is not loaded in this document",
                None,
                None,
            )
        })
    }

    /// The records in the current selection.
    pub(crate) fn selected_records(&self) -> Vec<Rc<ModelAnnotation>> {
        let model = self.model();
        model.selected.iter().filter_map(|id| model.by_id.get(id).cloned()).collect()
    }

    /// `getHovered()`.
    pub fn get_hovered(&self) -> Option<Value> {
        let model = self.model();
        model.hovered.as_ref().and_then(|id| model.by_id.get(id)).map(|r| annotation_value(r))
    }

    /// `text.getEditing()`.
    pub fn get_editing(&self) -> Option<Value> {
        let model = self.model();
        model.editing.as_ref().and_then(|id| model.by_id.get(id)).map(|r| annotation_value(r))
    }

    /// `isEditable(ref)`: whether the `editable` setting lets people change it by hand.
    pub fn is_editable(&self, reference: &Value) -> bool {
        let model = self.model();
        model
            .by_id
            .get(&key_or_none(reference))
            .is_some_and(|r| is_editable_by_hand(&model.editable, &r.annotation))
    }
}
