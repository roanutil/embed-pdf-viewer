//! `tools/hover-feed.ts`: the annotation plane's cursor enter/exit trigger feed,
//! a thin adapter from the one pointer-driven hover seam (`hoverAt`'s diff)
//! onto the actions plugin's shared hover pump.

use epdf_annotation_core::Annot;
use epdf_plugin_actions::{
    ActionsCapability, HoverEvents, HoverPump, HoverTarget, create_hover_pump,
};

use crate::value::{get, truthy};

/// `AnnotationHoverFeed`.
#[derive(Clone)]
pub struct AnnotationHoverFeed {
    pump: HoverPump,
    annot_of: std::rc::Rc<dyn Fn(&str) -> Option<Annot>>,
}

impl AnnotationHoverFeed {
    /// `targetOf(id)`: drafts, widgets, links and tree-less annotations are
    /// not hover targets here.
    fn target_of(&self, id: Option<&str>) -> Option<HoverTarget> {
        let annot = (self.annot_of)(id?)?;
        let annotation_ref = serde_json::to_value(annot.ref_.as_ref()?).ok()?;
        if annot.subtype.starts_with("widget") || annot.subtype == "link" {
            return None;
        }
        let actions = annot.data.as_ref().and_then(|d| get(d, "actions"));
        let root = |k: &str| truthy(actions.and_then(|a| get(a, k)).and_then(|t| get(t, "root")));
        let (enter, exit) = (root("cursorEnter"), root("cursorExit"));
        if !enter && !exit {
            return None;
        }
        Some(HoverTarget {
            annotation_ref,
            page: epdf_kernel::geometry::PageRef::new(annot.page.page_object_number),
            source: None,
            events: Some(HoverEvents { enter: Some(enter), exit: Some(exit) }),
        })
    }

    /// Report the pointer-driven hover target.
    pub fn hover(&self, id: Option<&str>) {
        self.pump.hover(self.target_of(id));
    }
}

/// `createAnnotationHoverFeed(actions, annotOf)`.
pub fn create_annotation_hover_feed(
    actions: ActionsCapability,
    annot_of: impl Fn(&str) -> Option<Annot> + 'static,
) -> AnnotationHoverFeed {
    let pump = create_hover_pump(move |trigger| actions.dispatch(trigger));
    AnnotationHoverFeed { pump, annot_of: std::rc::Rc::new(annot_of) }
}
