//! `tools/behaviors.ts`: a sibling plugin (forms, links) or an interactive
//! renderer marks some annotations as interactive. While a behavior is
//! engaged its annotations render their own DOM and are not
//! geometry-editable: hit-test, marquee and the selection must not see them.

use std::rc::Rc;

use epdf_annotation_core::js::IdSet;
use epdf_kernel::Unsubscribe;
use serde_json::json;

use crate::controller::Core;
use crate::host_contract::Behavior;
use crate::js::Object;

impl Core {
    /// The behavior that owns the annotation now: one that matches it and is engaged.
    fn owner(&self, annotation: &Object) -> Option<Rc<dyn Behavior>> {
        let behaviors = self.behaviors.borrow().clone();
        behaviors.into_iter().find(|b| b.matches(annotation) && b.engaged(annotation))
    }

    /// `engagedIdsOn(page)`: ids on a page whose behavior is engaged now; `None` without behaviors.
    pub(crate) fn engaged_ids_on(&self, page: f64) -> Option<IdSet> {
        if self.behaviors.borrow().is_empty() {
            return None;
        }
        let model = self.model();
        let mut out: Option<Vec<String>> = None;
        for id in model.order.iter() {
            let Some(record) = model.by_id.get(id) else { continue };
            if record.page_number() != Some(page) {
                continue;
            }
            if self.owner(&record.annotation).is_some() {
                out.get_or_insert_with(Vec::new).push(id.clone());
            }
        }
        out.map(|ids| IdSet::of_value(&json!(ids)))
    }

    /// `registerBehavior(behavior)`.
    pub fn register_behavior(&self, behavior: Rc<dyn Behavior>) -> Unsubscribe {
        self.behaviors.borrow_mut().push(behavior.clone());
        let me = self.weak();
        Unsubscribe::new(move || {
            if let Some(core) = me.upgrade() {
                let mut list = core.behaviors.borrow_mut();
                if let Some(i) = list.iter().position(|b| Rc::ptr_eq(b, &behavior)) {
                    list.remove(i);
                }
            }
        })
    }

    /// `getBehaviorFor(annotation)`.
    pub fn get_behavior_for(&self, annotation: &Object) -> Option<Rc<dyn Behavior>> {
        self.owner(annotation)
    }

    /// `pruneEngagedSelection()`: engaged annotations leave the selection.
    pub fn prune_engaged_selection(&self) {
        let model = self.model();
        let drop: Vec<String> = model
            .selected
            .iter()
            .filter(|id| model.by_id.get(*id).is_some_and(|r| self.owner(&r.annotation).is_some()))
            .cloned()
            .collect();
        if !drop.is_empty() {
            self.commit_or_log(json!({ "type": "deselect", "ids": drop }));
        }
    }
}
