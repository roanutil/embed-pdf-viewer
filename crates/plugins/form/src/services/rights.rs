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

    /// `rights.key()`: what the session may fill in and sign, per group
    /// (`null` among them), as a key that changes when that does: for memos.
    pub(crate) fn rights_key(&self) -> String {
        let index = self.view();
        let mut groups: Vec<&Value> = Vec::new();
        for field in index.fields() {
            let group = field.get("groupId").unwrap_or(&Value::Null);
            if !groups.iter().any(|g| crate::value::strict_eq(Some(g), Some(group))) {
                groups.push(group);
            }
        }
        groups
            .into_iter()
            .map(|group| {
                let grouped = serde_json::json!({ "groupId": group });
                format!(
                    "{}{}",
                    u8::from(self.may_fill(&grouped)),
                    u8::from(self.may_sign(&grouped))
                )
            })
            .collect()
    }

    /// `rights.assertMayFill(field, operation)`.
    pub(crate) fn assert_may_fill(&self, field: &Value, operation: &str) -> Result<(), Error> {
        self.cx.assert_allowed_field("fill", field, operation)
    }
}
