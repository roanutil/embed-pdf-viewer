//! `write/text-editing.ts`: free-text editing. Every keystroke is an
//! ordinary commit of the core's `setText` / `setRichText`, into one change
//! still being made (a hold): the view shows the text at once, and the
//! change keeps its place in line holding the latest text. It is sent after
//! a pause in typing, when editing ends, when anything else is staged, and
//! before a download.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::types::{Id, Model, Point};
use epdf_kernel::Unsubscribe;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Value, json};

use crate::controller::Core;
use crate::js::key_or_none;
use crate::model::set_text_selection;
use crate::read::chrome::HitView;
use crate::rich_text::{TextSelection, css_font_family_for_face, rich_doc_of, text_commit_patch};
use crate::services::store::{Commit, StoreHold, Written};

const TEXT_WRITE_DELAY_MS: f64 = 250.0;

/// What is being typed.
struct Typing {
    id: Id,
    hold: StoreHold,
    written: Option<Promise<Written>>,
    cancel_pause: Option<Unsubscribe>,
}

#[derive(Default)]
pub struct TextState {
    typing: RefCell<Option<Rc<RefCell<Typing>>>>,
}

impl Core {
    /// `sendTyping()`: send what was typed now. Resolves once the engine
    /// answered it; never rejects.
    pub(crate) fn send_typing(&self) -> Promise<()> {
        let sent = self.text.typing.borrow_mut().take();
        let Some(sent) = sent else { return Promise::resolved(()) };
        let (written, hold, cancel) = {
            let mut typing = sent.borrow_mut();
            (typing.written.clone(), typing.hold.clone(), typing.cancel_pause.take())
        };
        if let Some(cancel) = cancel {
            cancel.call();
        }
        hold.send();
        Promise::spawn(async move {
            if let Some(written) = written {
                let _ = written.await;
            }
            Ok(())
        })
    }

    /// `type(id, message)`: one edit of the record's text into its typing.
    pub(crate) fn type_text(&self, id: &str, message: Value) -> Result<Commit, epdf_kernel::Error> {
        let current = self.text.typing.borrow().clone();
        if let Some(t) = &current {
            let (other, open) = {
                let t = t.borrow();
                (t.id != id, t.hold.open())
            };
            if other || !open {
                let _ = self.send_typing();
            }
        }
        if self.text.typing.borrow().is_none() {
            let hold = self.hold_change(
                json!({ "key": "annotation.text" }),
                Some(format!("annotation.text:{id}")),
            )?;
            *self.text.typing.borrow_mut() = Some(Rc::new(RefCell::new(Typing {
                id: id.to_string(),
                hold,
                written: None,
                cancel_pause: None,
            })));
        }
        let current = self.text.typing.borrow().clone().expect("typing");
        let hold = current.borrow().hold.clone();
        let commit = self.commit(message, None, Some(&hold))?;
        let previous = {
            let mut t = current.borrow_mut();
            t.written = Some(commit.written.clone());
            t.cancel_pause.take()
        };
        if let Some(cancel) = previous {
            cancel.call();
        }
        let me = self.weak();
        let weak_typing = Rc::downgrade(&current);
        let cancel = self.cx.clock().after(TEXT_WRITE_DELAY_MS, move || {
            let Some(core) = me.upgrade() else { return };
            let still = core
                .text
                .typing
                .borrow()
                .as_ref()
                .is_some_and(|t| weak_typing.upgrade().is_some_and(|w| Rc::ptr_eq(t, &w)));
            if still {
                let _ = core.send_typing();
            }
        });
        current.borrow_mut().cancel_pause = Some(cancel);
        Ok(commit)
    }

    /// The `text` effect's ops: a record's text as the engine writes it.
    pub(crate) fn text_ops(&self, id: &str, model: &Model) -> Vec<Value> {
        let Some(record) = model.by_id.get(id) else { return Vec::new() };
        let Some(reference) = record.annotation.get("ref").cloned() else { return Vec::new() };
        let paragraphs = rich_doc_of(&record.annotation, self.fonts()).paragraphs;
        let patch = text_commit_patch(&record.annotation, &paragraphs, self.fonts());
        let mut full = serde_json::Map::new();
        full.insert("subtype".into(), json!("free-text"));
        if let Some(rich) = patch.get("richText") {
            full.insert("richText".into(), rich.clone());
        }
        vec![json!({ "type": "annotations.update", "ref": reference, "patch": full })]
    }

    /// `text.begin(ref)`.
    pub fn text_begin(&self, reference: &Value) {
        self.commit_or_log(json!({ "type": "beginTextEdit", "id": key_or_none(reference) }));
    }

    /// `text.end(options)`: stop typing, and save what was typed.
    pub fn text_end(&self, signal: Option<AbortSignal>) -> Promise<()> {
        let sent = self.send_typing();
        if self.state().text_selection.is_some() {
            self.update_state(|s| set_text_selection(s, None));
        }
        self.commit_or_log(json!({ "type": "endTextEdit" }));
        // `async` with `await ctx.cancellable(…)`: settles a turn after it.
        let sent = self.cancellable(signal, sent);
        Promise::spawn(async move { sent.await })
    }

    /// `beginTextEditAt(page, point, scale, rotation, zoom)`.
    pub fn begin_text_edit_at(&self, page: &Value, point: Point, view: HitView) -> bool {
        if let Some(id) = self.text_box_at(page, point, view) {
            self.commit_or_log(json!({ "type": "beginTextEdit", "id": id }));
            if self.model().editing.as_deref() == Some(id.as_str()) {
                return true;
            }
        }
        false
    }

    /// `getEditingId()`.
    pub fn get_editing_id(&self) -> Option<Id> {
        self.model().editing.clone()
    }

    /// `draftContents(ref, text)`.
    pub fn draft_contents(&self, reference: &Value, text: &str) -> Result<(), epdf_kernel::Error> {
        let id = key_or_none(reference);
        self.type_text(&id, json!({ "type": "setText", "id": id, "text": text })).map(|_| ())
    }

    /// `draftRichText(ref, doc)`.
    pub fn draft_rich_text(
        &self,
        reference: &Value,
        paragraphs: Value,
    ) -> Result<(), epdf_kernel::Error> {
        let id = key_or_none(reference);
        self.type_text(
            &id,
            json!({ "type": "setRichText", "id": id, "doc": { "paragraphs": paragraphs } }),
        )
        .map(|_| ())
    }

    /// `setTextSelection(ref, range)`.
    pub fn set_text_selection(&self, reference: &Value, range: Option<(f64, f64)>) {
        let id = key_or_none(reference);
        let previous = self.state().text_selection.clone();
        let next = range.map(|(start, end)| TextSelection { id, start, end });
        let same = match (&previous, &next) {
            (None, None) => true,
            (Some(p), Some(n)) => p.id == n.id && p.start == n.start && p.end == n.end,
            _ => false,
        };
        if same {
            return;
        }
        let next = next.map(Rc::new);
        self.update_state(|s| set_text_selection(s, next));
    }

    /// `getCssFontFamily(family)`.
    pub fn get_css_font_family(&self, family: &str) -> String {
        css_font_family_for_face(family, self.fonts())
    }
}
