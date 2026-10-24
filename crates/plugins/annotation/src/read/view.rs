//! `read/view.ts`: what the user sees, as one pure function of the records
//! and the session.
//!
//! `records.get()` (what the engine confirmed) and `records.view()` (that,
//! with this session's pending changes on top) make the `AnnotationView`,
//! each record drawn as the change on it says; with the session it is the
//! core's `Model`. Every read and gesture takes this model. It is memoized,
//! and each record keeps its identity until its own inputs change.
//!
//! A lender's records come after the annotations, so on each page they
//! paint above them, as form widgets do.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use epdf_annotation_core::appearance::{drawn_after, source_of_confirmed, source_of_new};
use epdf_annotation_core::engine::resolve::annotation_patch_between;
use epdf_annotation_core::record::from_dto;
use epdf_annotation_core::types::{Authority, ById, Id, Model, ModelAnnotation, Session, Source};
use serde_json::{Value, json};

use crate::js::{Object, key_of};
use crate::services::lending::LentRecords;
use crate::sync::records::{by_key, dto_of, identity, order, picture_version};

/// `AnnotationView`: the records as shown.
#[derive(Clone)]
pub struct AnnotationView {
    pub by_id: Rc<ById>,
    pub order: Rc<Vec<Id>>,
}

/// What the view is computed from, compared by identity.
#[derive(Clone)]
pub struct ViewInputs {
    pub truths: Rc<Value>,
    pub shown: Rc<Value>,
    pub vector: Rc<indexmap::IndexMap<Id, ()>>,
    pub document: Option<(Rc<Value>, u64)>,
    pub lent: Option<LentRecords>,
}

impl ViewInputs {
    fn same(&self, other: &ViewInputs) -> bool {
        Rc::ptr_eq(&self.truths, &other.truths)
            && Rc::ptr_eq(&self.shown, &other.shown)
            && Rc::ptr_eq(&self.vector, &other.vector)
            && match (&self.document, &other.document) {
                (None, None) => true,
                (Some((a, ra)), Some((b, rb))) => Rc::ptr_eq(a, b) && ra == rb,
                _ => false,
            }
            && match (&self.lent, &other.lent) {
                (None, None) => true,
                (Some(a), Some(b)) => a.same(b),
                _ => false,
            }
    }
}

/// The lender's answers the view needs (`AnnotationLender`'s drawing members).
pub trait LentDrawing {
    fn picture_version(&self, reference: &Value) -> f64;
    fn authority(&self, record: &Object) -> Authority;
    fn painter(&self, record: &Rc<Object>) -> epdf_annotation_core::types::Painter;
    fn pending(&self, reference: &Value) -> bool;
}

/// What one page's slice of the model is made of.
#[derive(Clone)]
struct PageParts {
    records: Vec<Rc<ModelAnnotation>>,
    selected: Vec<Id>,
    hovered: Option<Id>,
    editing: Option<Id>,
    draft: Option<Rc<epdf_annotation_core::types::Draft>>,
    preview: Option<Rc<epdf_annotation_core::types::MarkupPreview>>,
    settings: (usize, f64, usize, usize),
}

fn opt_ptr<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl PageParts {
    fn same(&self, other: &PageParts) -> bool {
        self.records.len() == other.records.len()
            && self.records.iter().zip(&other.records).all(|(a, b)| Rc::ptr_eq(a, b))
            && self.selected == other.selected
            && self.hovered == other.hovered
            && self.editing == other.editing
            && opt_ptr(&self.draft, &other.draft)
            && opt_ptr(&self.preview, &other.preview)
            && (self.settings.0 == other.settings.0
                && (self.settings.1 == other.settings.1
                    || (self.settings.1.is_nan() && other.settings.1.is_nan()))
                && self.settings.2 == other.settings.2
                && self.settings.3 == other.settings.3)
    }
}

/// A cached lent record: its shown row (held weakly, as a `WeakMap` key is),
/// the confirmed row it was made from, and the record.
struct LentEntry {
    shown: std::rc::Weak<Value>,
    truth: Option<Rc<Value>>,
    record: Rc<ModelAnnotation>,
}

/// The view's caches.
#[derive(Default)]
pub struct View {
    /// A confirmed record as the model has it, per stored identity and render preference.
    confirmed: RefCell<HashMap<u64, (bool, Rc<ModelAnnotation>)>>,
    /// A record a pending change made or changed, per shown identity, truth identity and preference.
    predicted: RefCell<HashMap<u64, (Option<u64>, bool, Rc<ModelAnnotation>)>>,
    /// A lent record, per shown row and the confirmed row it was made from,
    /// both by identity (a `WeakMap` on the shown row).
    lent: RefCell<HashMap<*const Value, LentEntry>>,
    /// Every lent key ever confirmed.
    confirmed_once: RefCell<HashSet<Id>>,
    view_memo: RefCell<Option<(ViewInputs, AnnotationView)>>,
    model_memo: RefCell<Option<(Session, usize, Rc<Model>)>>,
    /// Keyed by the whole model; held weakly so its address is not reused.
    by_page: RefCell<Option<(std::rc::Weak<Model>, Rc<HashMap<String, Vec<Rc<ModelAnnotation>>>>)>>,
    page_slices: RefCell<HashMap<String, (std::rc::Weak<Model>, PageParts, Rc<Model>)>>,
}

fn page_key(page: f64) -> String {
    epdf_annotation_core::js::number_to_string(page)
}

impl View {
    /// `authorityOf(ctx, dto)`: this session's authority over a record.
    pub fn authority_of(
        security: Option<&Rc<dyn epdf_kernel::engine::DocumentSecurity>>,
        dto: &Value,
    ) -> Option<Authority> {
        let security = security?;
        let target = owner_of(dto);
        Some(Authority {
            update: security.allows_annotation("update", &target),
            delete: security.allows_annotation("delete", &target),
            order: Default::default(),
        })
    }

    fn confirmed_record(
        &self,
        stored: &Value,
        vector: bool,
        security: Option<&Rc<dyn epdf_kernel::engine::DocumentSecurity>>,
    ) -> Option<Rc<ModelAnnotation>> {
        let id = identity(stored);
        if let Some((v, record)) = self.confirmed.borrow().get(&id)
            && *v == vector
        {
            return Some(record.clone());
        }
        let dto = dto_of(stored);
        let mut record = from_dto(Rc::new(dto.as_object().cloned().unwrap_or_default()))?;
        record.source = source_of_confirmed(&record.annotation, vector);
        record.picture_version = Some(picture_version(stored));
        record.authority = View::authority_of(security, dto);
        let record = Rc::new(record);
        self.confirmed.borrow_mut().insert(id, (vector, record.clone()));
        Some(record)
    }

    fn predicted_record(
        &self,
        shown: &Value,
        truth: Option<&Value>,
        vector: bool,
        security: Option<&Rc<dyn epdf_kernel::engine::DocumentSecurity>>,
    ) -> Option<Rc<ModelAnnotation>> {
        let id = identity(shown);
        let truth_id = truth.map(identity);
        if let Some((t, v, record)) = self.predicted.borrow().get(&id)
            && *t == truth_id
            && *v == vector
        {
            return Some(record.clone());
        }
        let dto = Rc::new(dto_of(shown).as_object().cloned().unwrap_or_default());
        let record = match truth {
            None => {
                let mut record = from_dto(dto.clone())?;
                record.unconfirmed = Some(true);
                record.source = source_of_new(&dto);
                record
            }
            Some(truth) => {
                let base = self.confirmed_record(truth, vector, security)?;
                let truth_dto = dto_of(truth).as_object().cloned().unwrap_or_default();
                let mut patch = annotation_patch_between(&truth_dto, &dto);
                patch.insert(
                    "subtype".into(),
                    truth_dto.get("subtype").cloned().unwrap_or(Value::Null),
                );
                let mut record = (*base).clone();
                record.annotation = dto.clone();
                if let Ok(drawn) = drawn_after(&base, &patch) {
                    if let Some(source) = drawn.source {
                        record.source = source;
                    }
                    if let Some(ap_box) = drawn.ap_box {
                        record.ap_box = Some(ap_box);
                    }
                }
                record
            }
        };
        let record = Rc::new(record);
        self.predicted.borrow_mut().insert(id, (truth_id, vector, record.clone()));
        Some(record)
    }

    fn lent_record(
        &self,
        shown_row: &Rc<Value>,
        truth_row: Option<&Rc<Value>>,
        lender: &dyn LentDrawing,
    ) -> Option<Rc<ModelAnnotation>> {
        let cache_key = Rc::as_ptr(shown_row);
        if let Some(entry) = self.lent.borrow().get(&cache_key)
            && entry.shown.upgrade().is_some_and(|s| Rc::ptr_eq(&s, shown_row))
            && match (&entry.truth, truth_row) {
                (None, None) => true,
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                _ => false,
            }
        {
            return Some(entry.record.clone());
        }
        let shown: &Value = shown_row;
        let truth: Option<&Value> = truth_row.map(|t| &**t);
        let shown_dto = Rc::new(shown.as_object().cloned().unwrap_or_default());
        let reference = shown.get("ref").cloned().unwrap_or(Value::Null);
        let record = match truth {
            None => {
                let mut record = from_dto(shown_dto.clone())?;
                record.unconfirmed = Some(true);
                let key = key_of(&reference).unwrap_or_default();
                record.source = if self.confirmed_once.borrow().contains(&key) {
                    Source::Baked
                } else {
                    Source::Vector
                };
                record.picture_version = Some(lender.picture_version(&reference));
                record.authority = Some(lender.authority(&shown_dto));
                record.painter = lender.painter(&shown_dto);
                record
            }
            Some(truth) => {
                let truth_dto = Rc::new(truth.as_object().cloned().unwrap_or_default());
                let mut base = from_dto(truth_dto.clone())?;
                base.source = source_of_confirmed(&truth_dto, false);
                base.picture_version =
                    Some(lender.picture_version(&truth.get("ref").cloned().unwrap_or(Value::Null)));
                base.authority = Some(lender.authority(&truth_dto));
                base.painter = lender.painter(&shown_dto);
                // `shown === truth`: the same row object.
                let mut record = if truth_row.is_some_and(|t| Rc::ptr_eq(t, shown_row)) {
                    base
                } else {
                    let mut patch = annotation_patch_between(&truth_dto, &shown_dto);
                    patch.insert(
                        "subtype".into(),
                        truth_dto.get("subtype").cloned().unwrap_or(Value::Null),
                    );
                    let mut next = base.clone();
                    next.annotation = shown_dto.clone();
                    if let Ok(drawn) = drawn_after(&base, &patch) {
                        if let Some(source) = drawn.source {
                            next.source = source;
                        }
                        if let Some(ap_box) = drawn.ap_box {
                            next.ap_box = Some(ap_box);
                        }
                    }
                    next
                };
                if lender.pending(&reference) {
                    record.source = Source::Vector;
                }
                record
            }
        };
        let record = Rc::new(record);
        let mut lent = self.lent.borrow_mut();
        lent.retain(|_, entry| entry.shown.strong_count() > 0);
        lent.insert(
            cache_key,
            LentEntry {
                shown: Rc::downgrade(shown_row),
                truth: truth_row.cloned(),
                record: record.clone(),
            },
        );
        Some(record)
    }

    /// The view over these inputs, memoized on their identity.
    pub fn view(
        &self,
        inputs: ViewInputs,
        security: Option<&Rc<dyn epdf_kernel::engine::DocumentSecurity>>,
        lender: Option<&dyn LentDrawing>,
    ) -> AnnotationView {
        if let Some((key, view)) = &*self.view_memo.borrow()
            && key.same(&inputs)
        {
            return view.clone();
        }
        let truths = by_key(&inputs.truths);
        let shown = by_key(&inputs.shown);
        let mut by_id = ById::new();
        let mut order_out: Vec<Id> = Vec::new();
        let mut live: HashSet<u64> = HashSet::new();
        for key in order(&inputs.shown) {
            let Some(stored) = shown.get(&key) else { continue };
            let truth = truths.get(&key);
            let vector = inputs.vector.contains_key(&key);
            let record = if truth.is_some_and(|t| identity(t) == identity(stored)) {
                self.confirmed_record(stored, vector, security)
            } else {
                self.predicted_record(stored, truth, vector, security)
            };
            live.insert(identity(stored));
            if let Some(t) = truth {
                live.insert(identity(t));
            }
            let Some(record) = record else { continue };
            by_id.insert(key.clone(), record);
            order_out.push(key);
        }
        // The caches keep what the records still hold.
        self.confirmed.borrow_mut().retain(|id, _| live.contains(id));
        self.predicted.borrow_mut().retain(|id, _| live.contains(id));
        if let (Some(lent), Some(lender)) = (&inputs.lent, lender) {
            let mut confirmed: HashMap<String, Rc<Value>> = HashMap::new();
            for row in lent.confirmed.iter() {
                if let Some(key) = row.get("ref").and_then(key_of) {
                    self.confirmed_once.borrow_mut().insert(key.clone());
                    confirmed.insert(key, row.clone());
                }
            }
            for row in lent.shown.iter() {
                let Some(key) = row.get("ref").and_then(key_of) else { continue };
                if by_id.contains_key(&key) {
                    continue;
                }
                if let Some(record) = self.lent_record(row, confirmed.get(&key), lender) {
                    by_id.insert(key.clone(), record);
                    order_out.push(key);
                }
            }
        }
        let view = AnnotationView { by_id: Rc::new(by_id), order: Rc::new(order_out) };
        *self.view_memo.borrow_mut() = Some((inputs, view.clone()));
        view
    }

    /// `{ ...session, ...view }`, memoized on the session and the view's identity.
    pub fn model(&self, session: &Session, view: &AnnotationView) -> Rc<Model> {
        let view_ptr = Rc::as_ptr(&view.by_id) as usize;
        if let Some((s, v, model)) = &*self.model_memo.borrow()
            && *v == view_ptr
            && Rc::ptr_eq(&model.order, &view.order)
            && s.same(session)
        {
            return model.clone();
        }
        let model = Rc::new(Model::of(session.clone(), view.by_id.clone(), view.order.clone()));
        *self.model_memo.borrow_mut() = Some((session.clone(), view_ptr, model.clone()));
        model
    }

    fn records_by_page(&self, whole: &Rc<Model>) -> Rc<HashMap<String, Vec<Rc<ModelAnnotation>>>> {
        if let Some((p, pages)) = &*self.by_page.borrow()
            && std::ptr::eq(p.as_ptr(), Rc::as_ptr(whole))
        {
            return pages.clone();
        }
        let mut pages: HashMap<String, Vec<Rc<ModelAnnotation>>> = HashMap::new();
        for id in whole.order.iter() {
            let Some(record) = whole.by_id.get(id) else { continue };
            let page = record.page_number().unwrap_or(f64::NAN);
            pages.entry(page_key(page)).or_default().push(record.clone());
        }
        let pages = Rc::new(pages);
        *self.by_page.borrow_mut() = Some((Rc::downgrade(whole), pages.clone()));
        pages
    }

    /// `pageModel(pageObjectNumber)`: one page's slice of the model, the same
    /// object while nothing on the page changed.
    pub fn page_model(&self, whole: &Rc<Model>, page: f64) -> Rc<Model> {
        let key = page_key(page);
        if let Some((w, _, model)) = self.page_slices.borrow().get(&key)
            && std::ptr::eq(w.as_ptr(), Rc::as_ptr(whole))
        {
            return model.clone();
        }
        let records = self.records_by_page(whole).get(&key).cloned().unwrap_or_default();
        let on_page = |id: &str| whole.by_id.get(id).is_some_and(|r| r.page_number() == Some(page));
        let draft = whole.draft.clone().filter(|draft| match draft.page() {
            Some(p) => p.object_number == page,
            None => draft.ids().iter().any(|id| on_page(id)),
        });
        let parts = PageParts {
            records,
            selected: whole.selected.iter().filter(|id| on_page(id)).cloned().collect(),
            hovered: whole.hovered.clone().filter(|h| on_page(h)),
            editing: whole.editing.clone().filter(|e| on_page(e)),
            draft,
            preview: whole.preview.clone().filter(|p| p.by_page.get(&key).is_some()),
            settings: (
                Rc::as_ptr(&whole.defaults) as usize,
                whole.hit_margin,
                Rc::as_ptr(&whole.snap) as usize,
                editable_identity(&whole.editable),
            ),
        };
        let cached = self.page_slices.borrow().get(&key).map(|(_, p, m)| (p.clone(), m.clone()));
        if let Some((previous, model)) = cached
            && previous.same(&parts)
        {
            self.page_slices
                .borrow_mut()
                .insert(key, (Rc::downgrade(whole), previous, model.clone()));
            return model;
        }
        let mut by_id = ById::new();
        for record in &parts.records {
            by_id.insert(record.id.clone(), record.clone());
        }
        let slice = Model {
            by_id: Rc::new(by_id),
            order: Rc::new(parts.records.iter().map(|r| r.id.clone()).collect()),
            selected: Rc::new(parts.selected.clone()),
            hovered: parts.hovered.clone(),
            editing: parts.editing.clone(),
            draft: parts.draft.clone(),
            preview: parts.preview.clone(),
            ..(**whole).clone()
        };
        let slice = Rc::new(slice);
        self.page_slices.borrow_mut().insert(key, (Rc::downgrade(whole), parts, slice.clone()));
        slice
    }
}

/// A boolean `editable` rule compares by value, a list by identity.
fn editable_identity(editable: &Rc<epdf_annotation_core::types::EditableKinds>) -> usize {
    match &**editable {
        epdf_annotation_core::types::EditableKinds::All(true) => 1,
        epdf_annotation_core::types::EditableKinds::All(false) => 0,
        _ => Rc::as_ptr(editable) as usize,
    }
}

/// `{ userId?, groupId? }`: a record's stamped owner.
pub fn owner_of(dto: &Value) -> Value {
    let mut target = serde_json::Map::new();
    for key in ["userId", "groupId"] {
        if let Some(v) = dto.get(key).filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v)) {
            target.insert(key.into(), v.clone());
        }
    }
    let _ = json!(null);
    Value::Object(target)
}
