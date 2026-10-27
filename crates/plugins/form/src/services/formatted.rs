//! `services/formatted.ts`: the texts this session's format scripts wrote.
//! The engine draws a field's formatted text into its picture but doesn't
//! say what it was (a `forms.setAppearanceText` answers as
//! `forms.updated`), so the painter takes it from here: the text each change
//! wrote, for the value the field had. Looked up by the value the field has
//! now, so a refused change, or another session's fill, falls back to the
//! value itself.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use epdf_kernel::Error;
use indexmap::IndexMap;
use serde_json::Value;

use crate::model::{FieldKey, field_by_ref, field_key_of};
use crate::value::{get, str_of};

/// The values remembered per field: enough for an undo or two.
const KEPT_PER_FIELD: usize = 8;

#[derive(Default)]
pub struct FormattedTexts {
    texts: RefCell<HashMap<FieldKey, IndexMap<String, String>>>,
    version: Cell<u64>,
}

/// A text or combo box field's value; none for another family.
fn value_of(field: &Value) -> Option<String> {
    match str_of(field, "family") {
        Some("text" | "combobox") => Some(crate::value::string_of(get(field, "value"))),
        _ => None,
    }
}

impl FormattedTexts {
    /// `textOf(field)`: the text the field shows for its value, as this
    /// session formatted it.
    pub fn text_of(&self, field: &Value) -> Option<String> {
        let value = value_of(field)?;
        self.texts.borrow().get(&field_key_of(field)).and_then(|kept| kept.get(&value).cloned())
    }

    /// Moves when a text is remembered.
    pub fn version(&self) -> u64 {
        self.version.get()
    }
}

impl crate::controller::Core {
    /// `ctx.listen(ctx.changes.onStaged, ...)`: staged, the view shows the
    /// change, its fields at the values it wrote.
    pub(crate) fn watch_formatted_texts(&self) -> Result<(), Error> {
        let me = self.weak();
        let off = self.cx.changes()?.on_staged().on(move |change| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            let view = core.view();
            for op in change.shows() {
                if str_of(&op, "type") != Some("forms.setAppearanceText") {
                    continue;
                }
                let Some(field) = field_by_ref(&view, get(&op, "field").unwrap_or(&Value::Null))
                else {
                    continue;
                };
                let Some(value) = value_of(field) else { continue };
                let key = field_key_of(field);
                let text = crate::value::string_of(get(&op, "text"));
                let mut texts = core.formatted.texts.borrow_mut();
                let kept = texts.entry(key).or_default();
                kept.shift_remove(&value);
                kept.insert(value, text);
                while kept.len() > KEPT_PER_FIELD {
                    kept.shift_remove_index(0);
                }
                drop(texts);
                core.formatted.version.set(core.formatted.version.get() + 1);
            }
            Ok(())
        });
        self.cx.listen(off);
        Ok(())
    }
}
