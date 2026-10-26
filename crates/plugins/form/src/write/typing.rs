//! `write/typing.ts`: the text someone is typing in a field, before it's
//! written. A text field commits on blur or Enter, when its scripts run (as
//! in Acrobat), so what is typed waits here, one draft per field. A
//! download writes the drafts first (`ctx.onSettle`), the way Acrobat
//! commits the field being edited before it saves.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::model::canonical_key;

impl crate::controller::Core {
    /// `keyOf(ref)`: the key a field is known by, whichever ref names it.
    pub(crate) fn key_of(&self, reference: &Value) -> String {
        canonical_key(&self.view(), reference)
    }

    /// `draftText(field, text)`.
    pub fn draft_text(&self, field: &Value, text: &str) {
        let key = self.key_of(field);
        self.drafts.borrow_mut().insert(key, (field.clone(), text.to_string()));
    }

    /// `discardDraftText(field)`.
    pub fn discard_draft_text(&self, field: &Value) {
        let key = self.key_of(field);
        self.drafts.borrow_mut().shift_remove(&key);
    }

    /// `commitDraftText(field)`: write the field's draft, if it has one;
    /// `null` when there is nothing to write.
    pub fn commit_draft_text(&self, field: &Value) -> Promise<Value> {
        let key = self.key_of(field);
        let draft = self.drafts.borrow_mut().shift_remove(&key);
        let me = self.rc();
        Promise::spawn(async move {
            let Some((field, text)) = draft else { return Ok(Value::Null) };
            // `return setText(...)` from an async function: adopted.
            let write = me.commit_value(field, json!({ "value": text }), "form.setValue", None);
            crate::promise::adopt(write).await
        })
    }

    /// The download flush: every draft is written first.
    pub(crate) fn wire_typing(&self) -> Result<(), Error> {
        let me = self.weak();
        self.cx.on_settle(Rc::new(move |_signal| {
            let Some(core) = me.upgrade() else { return Ok(None) };
            let fields: Vec<Value> =
                core.drafts.borrow().values().map(|(f, _)| f.clone()).collect();
            let writes: Vec<Promise<Value>> =
                fields.iter().map(|f| core.commit_draft_text(f)).collect();
            let all = epdf_plugin_annotation::promise::all(writes);
            Ok(Some(Promise::spawn(async move {
                all.await?;
                Ok(())
            })))
        }))
    }
}
