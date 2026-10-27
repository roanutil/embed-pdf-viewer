//! `write/icons.ts`: icon annotations (sticky notes, file attachments):
//! fixed-size placement at a click, and the one click-to-place entry the
//! place handler forwards every down to.

use std::rc::Rc;

use epdf_annotation_core::types::{Point, ViewEnv};
use epdf_annotation_core::update::defaults_for;
use serde_json::Value;

use crate::controller::Core;
use crate::engine::to_page_ref;
use crate::read::chrome::view_env;
use crate::services::staging::StoreChange;
use crate::tools::definitions::ResolvedTool;
use crate::write::outcomes::applied_or_throw;
use crate::write::placement::{
    annotation_of_tool, icon_place_at, icon_placement, is_icon_place_kind, rect_json,
};

impl Core {
    /// `placeIconAt(tool, page, point, view, file)`: an icon annotation at
    /// its usual size, shown centred on a page point.
    fn place_icon_at(
        &self,
        tool: &ResolvedTool,
        page: f64,
        point: Point,
        view: Option<ViewEnv>,
        file: Option<Value>,
    ) -> bool {
        let Some(size) = self.size_of(page) else { return false };
        if self.doc().is_none() || !is_icon_place_kind(&tool.subtype) {
            return false;
        }
        let model = self.model();
        let (_, rect) =
            icon_place_at(&annotation_of_tool(&model, tool), point, size, view.as_ref());
        let defaults = defaults_for(&model, &tool.preset);
        let placement = icon_placement(
            &tool.subtype,
            &rect_json(rect),
            &defaults,
            tool.flags.as_ref(),
            file.as_ref(),
        );
        let (data, resources) = match placement {
            Ok(p) => p,
            Err(error) => {
                self.log_error("[annotation] icon placement failed:", error.to_canonical());
                return true;
            }
        };
        let applied = self.apply_when_numbered(
            vec![StoreChange::Create { page: to_page_ref(page), draft: data, resources }],
            false,
        );
        let me = self.weak();
        let tool_id = tool.id.clone();
        epdf_kernel::rt::spawn(async move {
            let outcome = async {
                let applied = applied.await?;
                if let Some(core) = me.upgrade() {
                    core.after_create_placed(Some(&tool_id), &applied.ids);
                }
                applied_or_throw(&applied).await
            }
            .await;
            if let (Err(error), Some(core)) = (outcome, me.upgrade()) {
                core.log_error("[annotation] icon placement failed:", error.to_canonical());
            }
        });
        true
    }

    /// `placeAt(page, point, displayRotation, zoom)`: armed payload first,
    /// then the active tool's kind decides.
    pub fn place_at(
        &self,
        page: f64,
        point: Point,
        display_rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> bool {
        if self.place_armed_stamp(page, point, display_rotation) {
            return true;
        }
        let Some(tool) = self.active_tool() else { return false };
        if tool.subtype == "stamp" {
            return self.request_stamp_at(page, point, display_rotation);
        }
        if !is_icon_place_kind(&tool.subtype) {
            return false;
        }
        let view = view_env(zoom, display_rotation);
        if tool.subtype == "text" {
            return self.place_icon_at(&tool, page, point, view, None);
        }
        let me = self.weak();
        let picked_tool = tool.clone();
        self.prompt_at(
            &tool,
            page,
            point,
            Rc::new(move |picked: Value| {
                if let Some(core) = me.upgrade() {
                    core.place_icon_at(&picked_tool, page, point, view.clone(), Some(picked));
                }
            }),
        )
    }
}
