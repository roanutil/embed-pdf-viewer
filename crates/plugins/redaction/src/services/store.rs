//! `services/store.ts`: slice reads and the engine doors.

use std::rc::Rc;

use epdf_annotation_core::PageRef;
use epdf_kernel::Error;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error};
use serde_json::Value;

use crate::controller::RedactionCapability;
use crate::model::RedactionState;

/// Applying destroys content: its own granted power, narrower than
/// annotate. The engine's apply asserts all three (the redact grant, the
/// page-content rewrite, the annotation consumption); `can_apply` mirrors
/// the full set.
pub const APPLY_CAPABILITIES: [&str; 3] = ["doc.redact", "doc.pages.modify", "doc.annotate.modify"];

fn page_ref_of(layout: &Value) -> Option<PageRef> {
    serde_json::from_value(layout.get("ref")?.clone()).ok()
}

impl RedactionCapability {
    pub(crate) fn state(&self) -> Rc<RedactionState> {
        self.0.cx.state()
    }

    /// `requireDoc()`: the guarded handle, or `not-ready`.
    pub(crate) fn require_doc(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.0.cx.doc().map_err(|_| {
            plugin_error(code::NOT_READY, "redaction", "no document bound", None, None)
        })
    }

    /// `requireService()`: the handle when it has a `redaction` service, or
    /// `unsupported`.
    pub(crate) fn require_service(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        let doc = self.require_doc()?;
        if !doc.has("redaction") {
            return Err(plugin_error(
                code::UNSUPPORTED,
                "redaction",
                "this engine has no redaction service",
                None,
                None,
            ));
        }
        Ok(doc)
    }

    /// The document's `PageLayout[]`, empty without a document.
    pub(crate) fn layouts(&self) -> Vec<Value> {
        self.0.cx.document().and_then(|d| d.pages.as_array().cloned()).unwrap_or_default()
    }

    /// `pages()`: every page's ref, in display order.
    pub(crate) fn pages(&self) -> Vec<PageRef> {
        self.layouts().iter().filter_map(page_ref_of).collect()
    }

    /// The layout of the page `page` names.
    pub(crate) fn layout_of(&self, page: &PageRef) -> Option<Value> {
        self.layouts().into_iter().find(|l| page_ref_of(l).is_some_and(|r| r.same_page(page)))
    }

    /// `pageIndexOf(pon)`: the display index, `-1` for a page the registry
    /// lacks.
    pub(crate) fn page_index_of(&self, page: &PageRef) -> i64 {
        self.layout_of(page).and_then(|l| l.get("index").and_then(Value::as_i64)).unwrap_or(-1)
    }

    /// Engine support present and every capability the engine's apply
    /// asserts.
    pub fn can_apply(&self) -> bool {
        let Ok(doc) = self.0.cx.doc() else { return false };
        if !doc.has("redaction") {
            return false;
        }
        let security = doc.security();
        APPLY_CAPABILITIES.iter().all(|cap| security.allows(cap))
    }
}
