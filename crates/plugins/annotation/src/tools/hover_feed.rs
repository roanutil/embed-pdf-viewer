//! `tools/hover-feed.ts`: the annotation plane's E/X trigger feed, a thin
//! adapter from the one pointer-driven hover seam (the `hoverAt` diff) onto
//! the shared hover pump. Reducer-side hover clears never pass through
//! `hoverAt`, so an effect-induced hover loss can never fire a cursor exit.

use epdf_annotation_core::record::kind_of;
use epdf_plugin_actions::{ACTIONS, HoverEvents, HoverPump, HoverTarget, create_hover_pump};
use serde_json::Value;

use crate::controller::Core;

/// The feed: the pump the actions plugin's dispatch drives.
pub struct HoverFeed {
    pump: HoverPump,
}

impl Core {
    /// `createAnnotationHoverFeed(actions, annotOf)`, when an actions plugin is present.
    pub(crate) fn create_hover_feed(&self) {
        let Ok(Some(actions)) = self.cx.try_get(&ACTIONS) else { return };
        let pump = create_hover_pump(move |trigger| actions.dispatch(trigger, None));
        *self.hover_feed.borrow_mut() = Some(HoverFeed { pump });
    }

    /// `targetOf(id)`: who is hoverable here.
    fn hover_target_of(&self, id: Option<&str>) -> Option<HoverTarget> {
        let id = id?;
        let model = self.model();
        let record = model.by_id.get(id)?;
        let reference = record.annotation.get("ref").filter(|r| !r.is_null())?.clone();
        if record.unconfirmed == Some(true) {
            return None;
        }
        let kind = kind_of(&record.annotation).name;
        if kind.starts_with("widget") || kind == "link" {
            return None;
        }
        let root = |name: &str| {
            record
                .annotation
                .get("actions")
                .and_then(|a| a.get(name))
                .and_then(|e| e.get("root"))
                .is_some_and(|r| epdf_annotation_core::json::truthy(Some(r)))
        };
        let (enter, exit) = (root("cursorEnter"), root("cursorExit"));
        if !enter && !exit {
            return None;
        }
        let page = epdf_kernel::geometry::PageRef::from_json(
            record.annotation.get("page").unwrap_or(&Value::Null),
        )?;
        Some(HoverTarget {
            annotation_ref: reference,
            page,
            source: None,
            events: Some(HoverEvents { enter: Some(enter), exit: Some(exit) }),
        })
    }

    /// `hoverFeed?.hover(id)`.
    pub(crate) fn hover_feed_hover(&self, id: Option<&str>) {
        if self.hover_feed.borrow().is_none() {
            return;
        }
        let target = self.hover_target_of(id);
        if let Some(feed) = &*self.hover_feed.borrow() {
            feed.pump.hover(target);
        }
    }
}
