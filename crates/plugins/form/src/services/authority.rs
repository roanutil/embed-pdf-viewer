//! `services/authority.ts`: authority reads for the twins, the hydration
//! gate, the fused fill projection and the write gate.

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::json;

use crate::controller::FormCapability;

impl FormCapability {
    /// `authority.can(cap)`: wildcard-aware, false without a document.
    pub(crate) fn can(&self, capability: &str) -> bool {
        self.doc().is_some_and(|doc| doc.security().allows(capability))
    }

    /// `authority.assertFill(operation)`: the optimistic gate, refusing
    /// before the spinner and the queued engine call.
    pub(crate) fn assert_fill(&self, operation: &str) -> Result<(), Error> {
        if self.can("doc.forms.fill") {
            return Ok(());
        }
        Err(plugin_error(
            code::PERMISSION_DENIED,
            "form",
            format!("{operation} requires doc.forms.fill"),
            None,
            Some(json!({ "required": "doc.forms.fill" })),
        ))
    }

    /// `canRead()`: the model hydrates at all.
    pub fn can_read(&self) -> bool {
        self.can("doc.forms.read")
    }

    /// `canFill()`: value writes would succeed.
    pub fn can_fill(&self) -> bool {
        self.can("doc.forms.fill")
    }

    /// `canDesign()`: the field-design family would succeed.
    pub fn can_design(&self) -> bool {
        self.can("doc.forms.modify")
    }
}
