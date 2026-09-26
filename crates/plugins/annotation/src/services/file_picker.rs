//! `services/file-picker.ts`: the one file-picker port every
//! click-then-pick tool resolves its bytes through (a DOM file dialog, wired
//! by the framework adapter).

use std::rc::Rc;

use epdf_annotation_core::{PageObjectNumber, PageRef, Point};
use epdf_plugin_interaction::INTERACTION;
use serde_json::Value;

use crate::contract::{FilePickerProvider, FilePromptRequest};
use crate::controller::AnnotationCapability;
use crate::promise::then;
use crate::tools::definitions::{ResolvedTool, StampSourceSpec};
use crate::value::truthy;

impl AnnotationCapability {
    /// `setFilePickerProvider(provider)`: one slot, last write wins. The
    /// remover clears only the provider it installed.
    pub fn set_file_picker_provider(
        &self,
        provider: Option<FilePickerProvider>,
    ) -> epdf_kernel::Unsubscribe {
        let serial = self.0.file_picker_serial.get() + 1;
        self.0.file_picker_serial.set(serial);
        *self.0.file_picker.borrow_mut() = provider;
        let weak = Rc::downgrade(&self.0);
        epdf_kernel::Unsubscribe::new(move || {
            let Some(core) = weak.upgrade() else { return };
            if core.file_picker_serial.get() == serial {
                *core.file_picker.borrow_mut() = None;
            }
        })
    }

    /// `promptAt(tool, pon, point, place)`: run the installed provider with
    /// the expiry rule (a cancel, a changed document or a changed tool drops
    /// the placement). No provider declines the click.
    pub(crate) fn prompt_at(
        &self,
        tool: &ResolvedTool,
        pon: PageObjectNumber,
        point: Point,
        place: impl FnOnce(&AnnotationCapability, Value) + 'static,
    ) -> bool {
        let Some(provider) = self.0.file_picker.borrow().clone() else { return false };
        let accept = match &tool.source {
            Some(StampSourceSpec::Prompt(p)) => p.accept.clone(),
            _ => None,
        };
        let req = FilePromptRequest {
            tool_id: tool.id.clone(),
            subtype: tool.subtype.clone(),
            accept,
            page: PageRef::new(pon),
            point,
        };
        let doc_at_click = self.doc();
        let answer = provider(req);
        let this = self.clone();
        let tool_id = tool.id.clone();
        then(answer, move |result| match result {
            Ok(picked) => {
                if !truthy(Some(&picked)) {
                    return;
                }
                let same_doc = match (this.doc(), &doc_at_click) {
                    (Some(a), Some(b)) => Rc::ptr_eq(&a, b),
                    (None, None) => true,
                    _ => false,
                };
                if !same_doc {
                    return;
                }
                let active = this
                    .cx()
                    .try_get(&INTERACTION)
                    .ok()
                    .flatten()
                    .map(|ix| ix.get_active_tool_id());
                if active.as_deref() != Some(tool_id.as_str()) {
                    return;
                }
                place(&this, picked);
            }
            Err(err) => this.console_error("[annotation] file-picker provider failed:", Some(&err)),
        });
        true
    }
}
