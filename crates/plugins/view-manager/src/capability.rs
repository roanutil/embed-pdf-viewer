//! `capability.ts`: selectors (pure reads) and intents (the only writers).
//! It only arranges document ids into panes.

use epdf_kernel::Ctx;

use crate::reducer::{js_max, js_min, slice_end};
use crate::types::{View, ViewInfo, ViewManagerAction, ViewManagerState};

#[derive(Clone)]
pub struct ViewManagerCapability {
    ctx: Ctx<ViewManagerState, ViewManagerAction>,
}

impl ViewManagerCapability {
    pub(crate) fn new(ctx: Ctx<ViewManagerState, ViewManagerAction>) -> Self {
        ViewManagerCapability { ctx }
    }

    // ── selectors ──

    /// Views in display order.
    pub fn list(&self) -> Vec<ViewInfo> {
        let state = self.ctx.state();
        state.order.iter().filter_map(|id| state.views.get(id).cloned()).collect()
    }

    pub fn order(&self) -> Vec<String> {
        self.ctx.state().order.clone()
    }

    pub fn get(&self, id: &str) -> Option<ViewInfo> {
        self.ctx.state().views.get(id).cloned()
    }

    pub fn focused_view_id(&self) -> Option<String> {
        self.ctx.state().focused_view_id.clone()
    }

    /// Which view (pane) holds this document, if any.
    pub fn document_view(&self, document_id: &str) -> Option<String> {
        let state = self.ctx.state();
        state
            .order
            .iter()
            .find(|id| {
                state
                    .views
                    .get(*id)
                    .is_some_and(|v| v.document_ids.iter().any(|d| d == document_id))
            })
            .cloned()
    }

    // ── view lifecycle ──

    /// Create an (empty) pane. Returns the new view id.
    pub fn create_view(&self) -> String {
        let id = format!("view-{}", self.ctx.state().seq + 1);
        self.ctx.dispatch(ViewManagerAction::Create {
            view: View { id: id.clone(), document_ids: Vec::new(), active_document_id: None },
        });
        id
    }

    pub fn remove_view(&self, id: &str) {
        self.ctx.dispatch(ViewManagerAction::Remove { id: id.to_string() });
    }

    /// Move a pane to a new position in the order. Unknown ids are ignored.
    pub fn move_view(&self, id: &str, to_index: f64) {
        let current = self.ctx.state().order.clone();
        let without: Vec<String> = current.iter().filter(|x| *x != id).cloned().collect();
        if without.len() == current.len() {
            return;
        }
        let clamped = js_max(0.0, js_min(to_index, without.len() as f64));
        let at = slice_end(clamped, without.len());
        let mut order = without[..at].to_vec();
        order.push(id.to_string());
        order.extend_from_slice(&without[at..]);
        self.ctx.dispatch(ViewManagerAction::ReorderViews { order });
    }

    pub fn set_focused(&self, id: &str) {
        self.ctx.dispatch(ViewManagerAction::SetFocused { id: Some(id.to_string()) });
    }

    // ── documents within a view ──

    /// The tab the pane shows.
    pub fn set_active_document(&self, view_id: &str, document_id: Option<&str>) {
        self.ctx.dispatch(ViewManagerAction::SetActiveDoc {
            view_id: view_id.to_string(),
            document_id: document_id.map(str::to_string),
        });
    }

    pub fn add_document(&self, view_id: &str, document_id: &str, index: Option<f64>) {
        self.ctx.dispatch(ViewManagerAction::AddDoc {
            view_id: view_id.to_string(),
            document_id: document_id.to_string(),
            index,
        });
    }

    pub fn remove_document(&self, view_id: &str, document_id: &str) {
        self.ctx.dispatch(ViewManagerAction::RemoveDoc {
            view_id: view_id.to_string(),
            document_id: document_id.to_string(),
        });
    }

    /// Reorder a tab inside its pane.
    pub fn move_document_within(&self, view_id: &str, document_id: &str, to_index: f64) {
        self.ctx.dispatch(ViewManagerAction::MoveDocWithin {
            view_id: view_id.to_string(),
            document_id: document_id.to_string(),
            to_index,
        });
    }

    /// Drag a tab from one pane into another.
    pub fn move_document_between(
        &self,
        from_view_id: &str,
        to_view_id: &str,
        document_id: &str,
        to_index: Option<f64>,
    ) {
        self.ctx.dispatch(ViewManagerAction::MoveDocBetween {
            from_view_id: from_view_id.to_string(),
            to_view_id: to_view_id.to_string(),
            document_id: document_id.to_string(),
            to_index,
        });
    }
}
