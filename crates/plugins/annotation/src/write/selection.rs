//! `write/selection.ts`: the selection. What is selected, and the verbs that
//! change its fields, link, delete, rotate, group and ungroup it as one,
//! every member through the same `update` and patch effect path a gesture
//! takes. Each verb is one change.

use std::rc::Rc;

use epdf_annotation_core::group::{expand_groups, group_key_of};
use epdf_annotation_core::hit::is_selectable;
use epdf_annotation_core::record::group_of;
use epdf_annotation_core::richtext::{RichTextRange, RichTextStyleDelta, apply_style_to_range};
use epdf_annotation_core::types::{Id, ModelAnnotation, Rect};
use epdf_annotation_core::update::selection_in_box;
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::js::{Object, key_or_none, num, page_object_number};
use crate::read::chrome::core_page;
use crate::rich_text::{rich_doc_of, run_delta_for_fields};
use crate::services::staging::StoreChange;
use crate::services::store::{Commit, refs_of_in};
use crate::write::crud::DuplicateOptions;
use crate::write::outcomes::{applied_or_throw, batch_result_of, throw_if_failed};

/// `DUPLICATE_OFFSET`: how far a duplicate lands from its original, in page points.
const DUPLICATE_OFFSET: f64 = 10.0;

/// A record's `rect` as a box.
fn rect_of(value: &Value) -> Option<Rect> {
    let read = |key: &str| value.get(key).and_then(epdf_annotation_core::json::number);
    Some(Rect { x: read("x")?, y: read("y")?, width: read("width")?, height: read("height")? })
}

/// A patch for the selection: the same for each member, or one per member.
pub enum SelectionPatch {
    Fields(Object),
    PerMember(Rc<dyn Fn(&Value) -> Object>),
}

impl Core {
    fn selected_refs(&self) -> Vec<Value> {
        let model = self.model();
        refs_of_in(&model, &model.selected)
    }

    /// `commitOverSelection(commit, signal)`: the outcome over the refs that were selected.
    fn commit_over_selection(
        &self,
        commit: impl FnOnce() -> Result<Vec<Commit>, Error>,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let refs = self.selected_refs();
        let commits = match commit() {
            Ok(c) => c,
            Err(e) => return Promise::rejected(e),
        };
        let all = crate::promise::all(commits.into_iter().map(|c| c.written).collect());
        let all = self.cancellable(signal, all);
        Promise::spawn(async move {
            let outcomes = all.await?;
            let failed: Vec<_> = outcomes.into_iter().flat_map(|o| o.failed).collect();
            Ok(batch_result_of(&refs, &failed))
        })
    }

    /// `restyle(patch)`: the selection's fields, a patch per member.
    fn restyle(&self, patch: &SelectionPatch) -> Result<Vec<Commit>, Error> {
        let model = self.model();
        let range = self.active_text_range(&model).or_else(|| self.active_caret(&model));
        let mut commits = Vec::new();
        let mut patches = Map::new();
        for id in model.selected.iter() {
            let Some(record) = model.by_id.get(id) else { continue };
            let own = match patch {
                SelectionPatch::Fields(f) => f.clone(),
                SelectionPatch::PerMember(f) => f(&Value::Object((*record.annotation).clone())),
            };
            if range.as_ref().map(|r| r.id.as_str()) != Some(id.as_str()) {
                patches.insert(id.clone(), Value::Object(own));
                continue;
            }
            let range = range.as_ref().expect("a range");
            let (delta, rest) = run_delta_for_fields(&own, self.fonts().as_deref());
            if !delta.is_empty() && range.start == range.end {
                // At the caret the style is the text typed next: nothing is written.
                self.style_typing(id, &delta);
            } else if !delta.is_empty() {
                let doc = rich_doc_of(&record.annotation, self.fonts().as_deref());
                let typed_delta: RichTextStyleDelta =
                    serde_json::from_value(Value::Object(delta.clone())).unwrap_or_default();
                let next = apply_style_to_range(
                    &doc.typed(),
                    RichTextRange { start: range.start, end: range.end },
                    &typed_delta,
                    &[],
                );
                let paragraphs = match next {
                    Some(p) => serde_json::to_value(p).unwrap_or(Value::Array(Vec::new())),
                    None => Value::Array(doc.paragraphs.clone()),
                };
                commits.push(self.type_text(
                    id,
                    json!({ "type": "setRichText", "id": id, "doc": { "paragraphs": paragraphs } }),
                )?);
            }
            patches.insert(id.clone(), Value::Object(rest));
        }
        if patches.values().any(|p| p.as_object().is_some_and(|o| !o.is_empty())) {
            commits.push(self.commit_json(json!({ "type": "setFields", "patches": patches }))?);
        }
        Ok(commits)
    }

    fn select_refs(&self, refs: &[Value], add: bool) {
        let ids: Vec<String> = refs.iter().map(key_or_none).collect();
        self.commit_or_log(json!({ "type": "select", "ids": ids, "add": add }));
    }

    /// `toggleFormat(format, options)`.
    pub fn toggle_format(&self, format: &str, signal: Option<AbortSignal>) -> Promise<()> {
        let properties = self.selection_properties();
        let on = properties["values"][format] != json!(true)
            || properties["mixed"]
                .as_array()
                .is_some_and(|m| m.iter().any(|k| k.as_str() == Some(format)));
        let model = self.model();
        let range = self.active_text_range(&model).or_else(|| self.active_caret(&model));
        let commits = if range.is_some() {
            let mut fields = Map::new();
            fields.insert(format.to_string(), json!(on));
            self.restyle(&SelectionPatch::Fields(fields))
        } else {
            self.commit_json(json!({ "type": "setTextFormat", "format": format, "on": on }))
                .map(|c| vec![c])
        };
        let commits = match commits {
            Ok(c) => c,
            Err(e) => return Promise::rejected(e),
        };
        let all = self.cancellable(
            signal,
            crate::promise::all(commits.into_iter().map(|c| c.written).collect()),
        );
        Promise::spawn(async move {
            for outcome in all.await? {
                throw_if_failed(&outcome)?;
            }
            Ok(())
        })
    }

    /// `selection.set(refs)`: `set([])` selects nothing.
    pub fn selection_set(&self, refs: &[Value]) {
        if refs.is_empty() {
            self.commit_or_log(json!({ "type": "deselect" }));
        } else {
            self.select_refs(refs, false);
        }
    }

    /// `selection.add(refs)`.
    pub fn selection_add(&self, refs: &[Value]) {
        self.select_refs(refs, true);
    }

    /// `selection.selectAll(page)`.
    pub fn selection_select_all(&self, page: Option<&Value>) {
        let only = page.map(|p| self.get_page(p));
        if let Some(None) = only {
            self.commit_or_log(json!({ "type": "deselect" }));
            return;
        }
        let only_number = only.flatten().and_then(|info| info.get("ref").map(page_object_number));
        let model = self.model();
        let ids: Vec<Id> = model
            .order
            .iter()
            .filter(|id| {
                model
                    .by_id
                    .get(*id)
                    .is_some_and(|r| only_number.is_none_or(|n| r.page_number() == Some(n)))
                    && is_selectable(&model, id)
            })
            .cloned()
            .collect();
        self.commit_or_log(json!({ "type": "select", "ids": ids }));
    }

    /// `selection.selectInRect(page, rect)`.
    pub fn selection_select_in_rect(&self, page: &Value, rect: Rect) -> Result<(), Error> {
        let reference = self.page_ref_of(page)?;
        let engaged = self.engaged_ids_on(page_object_number(&reference));
        let ids =
            selection_in_box(&self.model(), &core_page(&reference), rect, engaged.as_ref(), None);
        self.commit_json(json!({ "type": "select", "ids": ids }))?;
        Ok(())
    }

    /// `selection.clear()`.
    pub fn selection_clear(&self) {
        self.commit_or_log(json!({ "type": "deselect" }));
    }

    /// `selection.update(changes, options)`.
    pub fn selection_update(
        &self,
        patch: SelectionPatch,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        self.commit_over_selection(|| self.restyle(&patch), signal)
    }

    /// `selection.updateLink(target, options)`.
    pub fn selection_update_link(
        &self,
        target: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        self.commit_over_selection(
            || self.commit_json(json!({ "type": "setLink", "target": target })).map(|c| vec![c]),
            signal,
        )
    }

    /// `selection.delete(options)`.
    pub fn selection_delete(&self, signal: Option<AbortSignal>) -> Promise<Value> {
        self.commit_over_selection(
            || self.commit_json(json!({ "type": "delete" })).map(|c| vec![c]),
            signal,
        )
    }

    fn commit_and_throw(&self, message: Value, signal: Option<AbortSignal>) -> Promise<()> {
        let commit = match self.commit_json(message) {
            Ok(c) => c,
            Err(e) => return Promise::rejected(e),
        };
        let written = self.cancellable(signal, commit.written);
        Promise::spawn(async move { throw_if_failed(&written.await?) })
    }

    /// `selection.rotateBy(degrees, options)`.
    pub fn selection_rotate_by(&self, degrees: f64, signal: Option<AbortSignal>) -> Promise<()> {
        self.commit_and_throw(
            json!({ "type": "rotateSelection", "degrees": crate::js::num(degrees) }),
            signal,
        )
    }

    /// `selection.resetRotation(options)`.
    pub fn selection_reset_rotation(&self, signal: Option<AbortSignal>) -> Promise<()> {
        self.commit_and_throw(json!({ "type": "resetRotation" }), signal)
    }

    /// `selection.getAnchor()`.
    pub fn selection_get_anchor(&self) -> Value {
        self.selection_anchor_of(None, None, None)
    }

    /// `selection.group(options)`: one relationship written onto every subordinate.
    pub fn selection_group(&self, signal: Option<AbortSignal>) -> Promise<()> {
        let model = self.model();
        let members = self.selected_records();
        if members.len() < 2 || members.iter().any(|r| self.lending.owns(&r.id)) {
            return Promise::resolved(());
        }
        let page = members[0].page_number();
        if members.iter().any(|r| r.page_number() != page) {
            return Promise::resolved(());
        }
        let mut ordered = members.clone();
        let position = |id: &str| model.order.iter().position(|o| o == id).map_or(-1, |p| p as i64);
        ordered.sort_by_key(|r| position(&r.id));
        let primary = &ordered[0];
        let Some(primary_ref) = primary.annotation.get("ref").cloned() else {
            return Promise::resolved(());
        };
        self.write_relationships(
            &ordered[1..],
            json!({ "to": primary_ref, "type": "group" }),
            signal,
        )
    }

    fn subordinates(&self) -> Vec<Rc<ModelAnnotation>> {
        let model = self.model();
        expand_groups(&model, &model.selected)
            .iter()
            .filter_map(|id| model.by_id.get(id).cloned())
            .filter(|r| group_of(&r.annotation).is_some())
            .collect()
    }

    /// `selection.ungroup(options)`.
    pub fn selection_ungroup(&self, signal: Option<AbortSignal>) -> Promise<()> {
        let subs = self.subordinates();
        self.write_relationships(&subs, Value::Null, signal)
    }

    /// `selection.canGroup()`.
    pub fn selection_can_group(&self) -> bool {
        let model = self.model();
        let members = self.selected_records();
        if members.len() < 2 || members.iter().any(|r| self.lending.owns(&r.id)) {
            return false;
        }
        if members.iter().any(|r| r.page_number() != members[0].page_number()) {
            return false;
        }
        if !members.iter().all(|r| {
            r.annotation
                .get("ref")
                .is_some_and(|reference| self.allows_mutation("update", reference))
        }) {
            return false;
        }
        let mut keys: Vec<Option<Id>> = Vec::new();
        for id in model.selected.iter() {
            let key = group_key_of(&model, id);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        !(keys.len() == 1 && keys[0].is_some())
    }

    /// `selection.canUngroup()`.
    /// `byOwner(refs)`: this plugin's own records and the lent ones (a
    /// form's widgets, in design mode).
    fn by_owner(&self, refs: Vec<Value>) -> (Vec<Value>, Vec<Value>) {
        refs.into_iter().partition(|reference| !self.lending.owns(&key_or_none(reference)))
    }

    /// `beside()`: where copies of the selection land: right and down of the
    /// originals, or left or up where that keeps on its page a copy that
    /// would leave it.
    fn beside(&self) -> Value {
        let boxes: Vec<(Rect, Rect)> = self
            .selected_records()
            .iter()
            .filter_map(|record| {
                let page = self.page_box_of(record.page_number()?)?;
                let rect = rect_of(record.annotation.get("rect")?)?;
                Some((rect, page))
            })
            .collect();
        let flips = |far: fn(&Rect) -> f64, near: fn(&Rect) -> f64, size: fn(&Rect) -> f64| {
            boxes.iter().any(|(rect, page)| far(rect) + DUPLICATE_OFFSET > size(page))
                && boxes.iter().all(|(rect, _)| near(rect) - DUPLICATE_OFFSET >= 0.0)
        };
        let x = if flips(|r| r.x + r.width, |r| r.x, |p| p.width) {
            -DUPLICATE_OFFSET
        } else {
            DUPLICATE_OFFSET
        };
        let y = if flips(|r| r.y + r.height, |r| r.y, |p| p.height) {
            -DUPLICATE_OFFSET
        } else {
            DUPLICATE_OFFSET
        };
        json!({ "x": num(x), "y": num(y) })
    }

    /// `selection.duplicate({ offset, signal })`: copy the selected
    /// annotations beside the originals and select the copies, as one step
    /// to undo. A lent record (a form's widget) is its lender's to copy, in
    /// the same change.
    pub fn selection_duplicate(
        &self,
        offset: Option<Value>,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let (own, lent) = self.by_owner(self.selected_refs());
        let offset = offset.unwrap_or_else(|| self.beside());
        self.duplicate(
            own,
            DuplicateOptions { page: None, offset: Some(offset), select: true, signal },
            lent,
        )
    }

    /// `selection.canDuplicate()`: something is selected, the user may
    /// create annotations, and nothing selected is locked.
    pub fn selection_can_duplicate(&self) -> bool {
        let (own, lent) = self.by_owner(self.selected_refs());
        if own.is_empty() && lent.is_empty() {
            return false;
        }
        if !own.is_empty() && !self.can_duplicate(&own) {
            return false;
        }
        let lender = self.lending.lender();
        let model = self.model();
        // A locked copy could neither be moved nor deleted by hand, whoever owns it.
        lent.iter().all(|reference| {
            model.by_id.get(&key_or_none(reference)).is_some_and(|record| {
                !epdf_annotation_core::json::truthy(record.annotation.get("locked"))
                    && lender.as_ref().is_some_and(|l| l.may_copy(&record.annotation))
            })
        })
    }

    pub fn selection_can_ungroup(&self) -> bool {
        let subs = self.subordinates();
        !subs.is_empty()
            && subs.iter().all(|r| {
                r.annotation
                    .get("ref")
                    .is_some_and(|reference| self.allows_mutation("update", reference))
            })
    }

    /// `writeRelationships(targets, reply, options)`: one change.
    fn write_relationships(
        &self,
        targets: &[Rc<ModelAnnotation>],
        reply: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        let changes: Vec<StoreChange> = targets
            .iter()
            .filter_map(|record| {
                let reference = record.annotation.get("ref")?.clone();
                let mut patch = Map::new();
                patch.insert("subtype".into(), json!(record.subtype()));
                patch.insert("reply".into(), reply.clone());
                Some(StoreChange::Update { reference, patch, resources: None })
            })
            .collect();
        if changes.is_empty() {
            return Promise::resolved(());
        }
        let applied = match self.apply(&changes, false) {
            Ok(a) => a,
            Err(e) => return Promise::rejected(e),
        };
        let done = self.cancellable(signal, applied_or_throw(&applied));
        Promise::spawn(async move {
            done.await?;
            Ok(())
        })
    }
}
