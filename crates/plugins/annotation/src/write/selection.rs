//! `write/selection.ts`: what is selected, and the verbs that restyle,
//! flag, delete, rotate, group and ungroup it as one. Every member goes
//! through the `update` → patch effect → `toPatch` path a gesture takes.

use epdf_annotation_core::richtext::{RichTextDocumentInput, RichTextRange, apply_style_to_range};
use epdf_annotation_core::{
    Annot, AnnotationFlagsPatch, AnnotationPropsPatch, AnnotationRef, Id, Msg, PageRef, Rect,
    expand_groups, geom_visual_bounds, group_key_of, is_selectable,
};
use epdf_geometry::intersect_rects;
use epdf_kernel::rt::Promise;
use serde_json::json;

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::promise;
use crate::repository::annotation_key;
use crate::rich_text::{TextFormat, rich_doc_of, run_delta_for_props};
use crate::services::store::refs_of_in;
use crate::write::crud::rotation_of;

impl AnnotationCapability {
    fn selected_refs(&self) -> Vec<AnnotationRef> {
        let m = self.model();
        refs_of_in(&m, &m.selected)
    }

    /// `restyle(patch)`: one flat props patch through the core. While the
    /// editor holds a range, the range keys restyle the runs it covers.
    fn restyle(&self, patch: AnnotationPropsPatch) {
        let m = self.model();
        if let Some(range) = self.active_text_range(&m) {
            let a = m.by_id.get(&range.id).cloned().expect("an active range names a model entry");
            let fonts = self.fonts();
            let (delta, rest) = run_delta_for_props(&patch, Some(&fonts));
            if delta != Default::default() {
                let paragraphs = rich_doc_of(&a, Some(&fonts)).paragraphs;
                let next = apply_style_to_range(
                    &paragraphs,
                    RichTextRange { start: range.start, end: range.end },
                    &delta,
                    &[],
                )
                .unwrap_or(paragraphs);
                self.commit(Msg::SetRichText {
                    id: range.id.clone(),
                    doc: RichTextDocumentInput { body: None, paragraphs: next },
                });
                if let Some(r) = &a.ref_ {
                    self.schedule_text_commit(r);
                }
            }
            if !rest.keys().is_empty() {
                let _ = self.flush_text_commits();
                self.commit(Msg::SetProps { patch: rest });
            }
            return;
        }
        if m.editing.is_some() {
            let _ = self.flush_text_commits();
        }
        self.commit(Msg::SetProps { patch });
    }

    /// `select(refs, { add })`: group-aware.
    pub fn select(&self, refs: &[AnnotationRef], add: Option<bool>) {
        self.commit(Msg::Select { ids: refs.iter().map(annotation_key).collect(), add });
    }

    /// `selectAll(page)`: everything selectable, or one page's.
    pub fn select_all(&self, page: Option<PageRef>) {
        let m = self.model();
        let ids: Vec<Id> = m
            .order
            .iter()
            .filter(|id| {
                m.by_id.get(*id).is_some_and(|a| page.is_none_or(|p| a.page.same_page(&p)))
                    && is_selectable(&m, id)
            })
            .cloned()
            .collect();
        self.commit(Msg::Select { ids, add: None });
    }

    /// `selectInRect(page, rect, { add })`: whatever intersects the rect.
    pub fn select_in_rect(&self, page: PageRef, rect: Rect, add: Option<bool>) {
        let m = self.model();
        let ids: Vec<Id> = m
            .order
            .iter()
            .filter(|id| {
                let Some(a) = m.by_id.get(*id) else { return false };
                if !a.page.same_page(&page) || !is_selectable(&m, id) {
                    return false;
                }
                let hit = intersect_rects(
                    geom_visual_bounds(&a.geom, a.style.stroke_width, Some(&a.style.border)),
                    rect,
                );
                hit.width > 0.0 && hit.height > 0.0
            })
            .cloned()
            .collect();
        if !ids.is_empty() || add != Some(true) {
            self.commit(Msg::Select { ids, add });
        }
    }

    /// `clearSelection()`.
    pub fn clear_selection(&self) {
        self.commit(Msg::Deselect { ids: None });
    }

    /// `updateSelection(patch)`.
    pub fn update_selection(
        &self,
        patch: AnnotationPropsPatch,
    ) -> Promise<BatchResult<AnnotationRef>> {
        let refs = self.selected_refs();
        let ((), pending) = self.collect_writes(|| self.restyle(patch));
        self.settle_writes(refs, pending)
    }

    /// `updateSelectionFlags(patch)`.
    pub fn update_selection_flags(
        &self,
        patch: AnnotationFlagsPatch,
    ) -> Promise<BatchResult<AnnotationRef>> {
        let refs = self.selected_refs();
        let (_, pending) = self.collect_writes(|| self.commit(Msg::SetFlags { patch, ids: None }));
        self.settle_writes(refs, pending)
    }

    /// `deleteSelection()`.
    pub fn delete_selection(&self) -> Promise<BatchResult<AnnotationRef>> {
        let refs = self.selected_refs();
        let (_, pending) = self.collect_writes(|| self.commit(Msg::Delete));
        self.settle_writes(refs, pending)
    }

    /// `rotateSelectionBy(delta)`: a quarter turn. `-90` rotates each member
    /// through `setRotation`.
    pub fn rotate_selection_by(&self, turn: QuarterTurn) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            if turn == QuarterTurn::Plus90 {
                let (_, pending) = this.collect_writes(|| this.commit(Msg::Rotate90));
                return this.await_all_writes(pending).await;
            }
            for r in this.selected_refs() {
                let a = this.loaded_or_throw(&r)?;
                this.set_rotation(r, rotation_of(&a) - 90.0).await?;
            }
            Ok(())
        })
    }

    /// `resetSelectionRotation()`.
    pub fn reset_selection_rotation(&self) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let (_, pending) = this.collect_writes(|| this.commit(Msg::ResetRotation));
            this.await_all_writes(pending).await
        })
    }

    /// `toggleTextFormat(format)`: flip it from the current value.
    pub fn toggle_text_format(&self, format: TextFormat) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let key = format.key();
            let current = this.get_selection_props()?.values.get(key.as_str()).cloned();
            let on = crate::value::truthy(current.as_ref());
            let patch: AnnotationPropsPatch =
                serde_json::from_value(json!({ key.as_str(): !on })).unwrap_or_default();
            let ((), pending) = this.collect_writes(|| this.restyle(patch));
            this.await_all_writes(pending).await
        })
    }

    /// `group()`: the bottom-most member becomes the primary.
    pub fn group(&self) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let m = this.model();
            let members = this.selected_committed();
            if members.len() < 2 {
                return Ok(());
            }
            let pon = members[0].page.page_object_number;
            if members.iter().any(|a| a.page.page_object_number != pon) {
                return Ok(());
            }
            let index =
                |a: &Annot| m.order.iter().position(|id| *id == a.id).map_or(-1.0, |i| i as f64);
            let mut ordered = members.clone();
            ordered.sort_by(|a, b| {
                (index(a) - index(b)).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            let primary = ordered[0].clone();
            let Some(primary_ref) = primary.ref_.clone() else { return Ok(()) };
            let writes = ordered[1..]
                .iter()
                .map(|a| this.write_relationship(a.clone(), Some(primary_ref.clone()), true))
                .collect();
            promise::all(writes).await?;
            Ok(())
        })
    }

    /// `ungroup()`: every subordinate of a touched group becomes top-level.
    pub fn ungroup(&self) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let m = this.model();
            let subs: Vec<Annot> = expand_groups(&m, &m.selected)
                .iter()
                .filter_map(|id| m.by_id.get(id))
                .filter(|a| a.ref_.is_some() && a.data.is_some() && a.group.is_some())
                .cloned()
                .collect();
            let writes =
                subs.into_iter().map(|a| this.write_relationship(a, None, false)).collect();
            promise::all(writes).await?;
            Ok(())
        })
    }

    /// `canGroup()`.
    pub fn can_group(&self) -> bool {
        let m = self.model();
        let members = self.selected_committed();
        if members.len() < 2 {
            return false;
        }
        if members.iter().any(|a| a.page.page_object_number != members[0].page.page_object_number) {
            return false;
        }
        if !members
            .iter()
            .all(|a| a.ref_.as_ref().is_some_and(|r| self.allows_mutation("update", r)))
        {
            return false;
        }
        let mut keys: Vec<Option<Id>> = Vec::new();
        for id in &m.selected {
            let k = group_key_of(&m, id);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        !(keys.len() == 1 && !keys.contains(&None))
    }

    /// `canUngroup()`.
    pub fn can_ungroup(&self) -> bool {
        let m = self.model();
        let subs: Vec<&Annot> = expand_groups(&m, &m.selected)
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter(|a| a.group.is_some())
            .collect();
        !subs.is_empty()
            && subs
                .iter()
                .all(|a| a.ref_.as_ref().is_some_and(|r| self.allows_mutation("update", r)))
    }
}
