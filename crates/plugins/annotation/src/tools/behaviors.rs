//! `tools/behaviors.ts`: a sibling plugin (forms, links) marks some
//! annotations as interactive. While a behavior is engaged its annotations
//! render their own DOM and are not geometry-editable: hit-test, marquee and
//! the selection must not see them.

use std::rc::Rc;

use epdf_annotation_core::js::IdSet;
use epdf_annotation_core::{Annot, AnnotationRef, Id, Msg, PageObjectNumber};
use epdf_plugin_interaction::Unregister;

use crate::contract::Behavior;
use crate::controller::AnnotationCapability;

impl AnnotationCapability {
    fn behavior_matches(b: &Rc<dyn Behavior>, subtype: &str, r: Option<&AnnotationRef>) -> bool {
        b.matches(subtype, r) && b.engaged()
    }

    fn engaged(&self, a: &Annot) -> bool {
        let behaviors = self.0.behaviors.borrow().clone();
        behaviors.iter().any(|b| Self::behavior_matches(b, &a.subtype, a.ref_.as_ref()))
    }

    /// `engagedIdsOn(pon)`: ids on a page whose behavior is engaged,
    /// resolved per event.
    pub(crate) fn engaged_ids_on(&self, pon: PageObjectNumber) -> Option<IdSet> {
        if self.0.behaviors.borrow().is_empty() {
            return None;
        }
        let m = self.model();
        let mut out: Option<IdSet> = None;
        for id in m.order.iter() {
            let Some(a) = m.by_id.get(id) else { continue };
            if a.page.page_object_number != pon {
                continue;
            }
            if self.engaged(a) {
                out.get_or_insert_with(IdSet::default).0.push(id.clone());
            }
        }
        out
    }

    /// `registerBehavior(b)` (host): the returned function unregisters it.
    pub fn register_behavior(&self, b: Rc<dyn Behavior>) -> Unregister {
        self.0.behaviors.borrow_mut().push(b.clone());
        let weak = Rc::downgrade(&self.0);
        Unregister::new(move || {
            if let Some(core) = weak.upgrade() {
                let mut behaviors = core.behaviors.borrow_mut();
                if let Some(i) = behaviors.iter().position(|x| Rc::ptr_eq(x, &b)) {
                    behaviors.remove(i);
                }
            }
        })
    }

    /// `getBehaviorFor(a)` (host).
    pub fn get_behavior_for(
        &self,
        subtype: &str,
        r: Option<&AnnotationRef>,
    ) -> Option<Rc<dyn Behavior>> {
        let behaviors = self.0.behaviors.borrow().clone();
        behaviors.into_iter().find(|b| Self::behavior_matches(b, subtype, r))
    }

    /// `pruneEngagedSelection()` (host): engaged annotations drop out of
    /// the selection.
    pub fn prune_engaged_selection(&self) {
        let m = self.model();
        let drop: Vec<Id> = m
            .selected
            .iter()
            .filter(|id| m.by_id.get(*id).is_some_and(|a| self.engaged(a)))
            .cloned()
            .collect();
        if !drop.is_empty() {
            self.commit(Msg::Deselect { ids: Some(drop) });
        }
    }
}
