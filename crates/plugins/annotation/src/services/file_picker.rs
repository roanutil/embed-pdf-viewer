//! `services/file-picker.ts`: the one file-picker port (a file dialog, wired
//! by the framework adapter). Every click-then-pick tool resolves its bytes
//! through this slot.

use std::rc::Rc;

use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::Point;
use epdf_kernel::Unsubscribe;
use epdf_plugin_interaction::INTERACTION;
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::engine::to_page_ref;
use crate::host_contract::FilePickerProvider;
use crate::tools::definitions::ResolvedTool;

impl Core {
    /// `setFilePickerProvider(provider)`: returns the function that removes it.
    pub fn set_file_picker_provider(&self, provider: Option<FilePickerProvider>) -> Unsubscribe {
        *self.file_picker.borrow_mut() = provider.clone();
        let me = self.weak();
        Unsubscribe::new(move || {
            let Some(core) = me.upgrade() else { return };
            let current = core.file_picker.borrow().clone();
            let same = match (&current, &provider) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                *core.file_picker.borrow_mut() = None;
            }
        })
    }

    /// `promptAt(tool, page, point, place)`: run the provider; the
    /// placement drops if the picker cancels or the document or the active
    /// tool changed meanwhile. No provider declines the click.
    pub(crate) fn prompt_at(
        &self,
        tool: &ResolvedTool,
        page: f64,
        point: Point,
        place: Rc<dyn Fn(Value)>,
    ) -> bool {
        let Some(provider) = self.file_picker.borrow().clone() else { return false };
        let mut request = Map::new();
        request.insert("toolId".into(), json!(tool.id));
        request.insert("subtype".into(), json!(tool.subtype));
        let accept = tool
            .source
            .as_ref()
            .filter(|s| s.get("kind").and_then(Value::as_str) == Some("prompt"))
            .and_then(|s| s.get("accept"))
            .cloned()
            .unwrap_or_else(epdf_kernel::undefined);
        request.insert("accept".into(), accept);
        request.insert("page".into(), to_page_ref(page));
        request.insert("point".into(), point.json());
        // Held weakly: the address stays the handle's while it is compared.
        let doc_at_click = self.doc().map(|d| Rc::downgrade(&d));
        let picked = provider(Value::Object(request));
        let me = self.weak();
        let tool_id = tool.id.clone();
        epdf_kernel::rt::spawn(async move {
            let result = picked.await;
            let Some(core) = me.upgrade() else { return };
            match result {
                Ok(None) => {}
                Ok(Some(picked)) => {
                    if picked.is_null() {
                        return;
                    }
                    let same_doc = match (core.doc(), &doc_at_click) {
                        (Some(d), Some(w)) => std::ptr::addr_eq(Rc::as_ptr(&d), w.as_ptr()),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same_doc {
                        return;
                    }
                    let active = core
                        .cx
                        .try_get(&INTERACTION)
                        .ok()
                        .flatten()
                        .map(|i| i.get_active_tool_id());
                    if active.as_deref() != Some(tool_id.as_str()) {
                        return;
                    }
                    place(picked);
                }
                Err(error) => core
                    .log_error("[annotation] file-picker provider failed:", error.to_canonical()),
            }
        });
        true
    }
}
