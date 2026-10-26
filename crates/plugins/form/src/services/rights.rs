//! `services/fill-rights.ts`: who may fill in, and sign, which field: the
//! engine's own answer per field (`ctx.allowsField`), so a control gated on
//! it never disagrees with the write. A field's group decides: a
//! `fields:fill` (`fields:sign`) permission for its group, or
//! `doc.forms.fill` (`doc.sign`) for every field.

use epdf_kernel::Error;
use serde_json::Value;

impl crate::controller::Core {
    /// `rights.mayFill(field)`.
    pub fn may_fill(&self, field: &Value) -> bool {
        self.cx.allows_field("fill", field)
    }

    /// `rights.maySign(field)`.
    pub fn may_sign(&self, field: &Value) -> bool {
        self.cx.allows_field("sign", field)
    }

    /// `rights.assertMayFill(field, operation)`.
    pub(crate) fn assert_may_fill(&self, field: &Value, operation: &str) -> Result<(), Error> {
        self.cx.assert_allowed_field("fill", field, operation)
    }
}
