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
use crate::js::{Object, key_or_none, page_object_number};
use crate::read::chrome::core_page;
use crate::rich_text::{rich_doc_of, run_delta_for_fields};
use crate::services::staging::StoreChange;
use crate::services::store::{Commit, refs_of_in};
use crate::write::outcomes::{applied_or_throw, batch_result_of, throw_if_failed};

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
        let range = self.active_text_range(&model);
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
            if !delta.is_empty() {
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
        let on = self.selection_properties()["values"][format] != json!(true);
        let range = self.active_text_range(&self.model());
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
