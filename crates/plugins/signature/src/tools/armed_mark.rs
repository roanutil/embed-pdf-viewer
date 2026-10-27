//! `tools/armed-mark.ts`: the armed mark over a field.
//!
//! The annotation plugin places an armed stamp on click (`annotation-place`,
//! priority 95) and selects an existing annotation under the pointer before
//! that (`annotation-edit`, 100; a widget is an annotation). This handler
//! sits above both, enabled for the armed stamp's tool only, and captures
//! exactly when the armed asset is a person's mark (a library of kind
//! `signatures`) and the pointer is over a signature field: then the
//! destination decides, and the mark goes into the field (`placeMark` by
//! mode) instead of onto the page. Everywhere else it declines and the click
//! stays a stamp placement, as in Preview.

use epdf_kernel::js::to_value;
use epdf_plugin_annotation::{ANNOTATION_EDIT_PRIORITY, ARMED_STAMP_TOOL_ID};
use epdf_plugin_form::FormCapability;
use epdf_plugin_interaction::{InteractionHandler, PointerSample, Tool};
use epdf_plugin_stamp::StampCapability;
use serde_json::Value;

use crate::contract::{Mark, OperationOptions, PlaceTarget, SIGNATURES_LIBRARY_KIND};
use crate::controller::SignatureCapability;
use crate::js::truthy;

/// `createArmedMarkHandler(documentId, signature, form, stamp)`.
pub struct ArmedMarkHandler {
    document_id: String,
    signature: SignatureCapability,
    form: FormCapability,
    stamp: StampCapability,
}

impl ArmedMarkHandler {
    pub fn new(
        document_id: String,
        signature: SignatureCapability,
        form: FormCapability,
        stamp: StampCapability,
    ) -> Self {
        ArmedMarkHandler { document_id, signature, form, stamp }
    }
}

impl InteractionHandler for ArmedMarkHandler {
    fn id(&self) -> &str {
        "signature:armed-mark-on-field"
    }

    fn priority(&self) -> f64 {
        ANNOTATION_EDIT_PRIORITY + 1.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.id == ARMED_STAMP_TOOL_ID
    }

    fn on_down(&self, sample: &PointerSample) -> bool {
        let Some(page) = &sample.page else { return false };
        let Some(armed) = self.stamp.get_armed_asset(Some(&self.document_id)) else {
            return false;
        };
        if self.stamp.get_library(&armed.library_id).map(|library| library.kind).as_deref()
            != Some(SIGNATURES_LIBRARY_KIND)
        {
            return false;
        }
        let Some(hit) = self.form.get_widget_at(&to_value(&page.page), page.point.x, page.point.y)
        else {
            return false;
        };
        let field = &hit.field;
        if field.get("family").and_then(Value::as_str) != Some("signature") {
            return false;
        }
        // A signed field is final: consume the click (no stamp lands on it)
        // and do nothing.
        let field_ref = field.get("ref").cloned().unwrap_or_else(epdf_kernel::undefined);
        let signed = self
            .signature
            .get_signature(&field_ref)
            .is_some_and(|s| s.get("signed").is_some_and(truthy));
        let value_kind =
            field.get("valueEntry").and_then(|v| v.get("kind")).and_then(Value::as_str);
        if signed || value_kind != Some("none") {
            return true;
        }
        self.stamp.disarm(Some(&self.document_id));
        crate::promise::catch(
            self.signature.place_mark(
                Mark::Asset { asset_id: armed.id },
                PlaceTarget::Field(field_ref),
                OperationOptions::default(),
            ),
            |error| crate::console::error("[signature] placing the mark failed:", &error),
        );
        true
    }
}
