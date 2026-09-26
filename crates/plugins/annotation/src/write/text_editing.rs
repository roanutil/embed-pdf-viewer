//! `write/text-editing.ts`: free-text editing. The editor's draft path
//! (optimistic model updates while typing) and the one debounced engine
//! write per annotation both editors share. The write is the rich
//! paragraphs (`textCommitPatch`); its echo is not re-ingested, since it
//! may already be behind the keyboard.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::richtext::RichTextDocumentInput;
use epdf_annotation_core::{AnnotationRef, Geom, Id, Msg, PageRef, Point, Target};
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise, TimerId};
use serde_json::{Map, Value, json};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::model::AnnotationAction;
use crate::promise::{self, all_settled, await_engine};
use crate::read::chrome::{HitView, Inert};
use crate::repository::annotation_key;
use crate::rich_text::{css_font_family_for_face, rich_doc_of, text_commit_patch};

const TEXT_COMMIT_DEBOUNCE_MS: f64 = 250.0;

/// Per-annotation debounce timers, in `Map` insertion order.
#[derive(Default)]
pub(crate) struct TextTimers(RefCell<Vec<(String, TimerId)>>);

impl TextTimers {
    fn take(&self, key: &str) -> Option<TimerId> {
        let mut timers = self.0.borrow_mut();
        let i = timers.iter().position(|(k, _)| k == key)?;
        Some(timers.remove(i).1)
    }
}

impl AnnotationCapability {
    /// `commitText(ref)`: the one engine text write, now. `None` when the
    /// annotation is gone.
    pub(crate) fn commit_text(&self, r: &AnnotationRef) -> Option<Promise<Value>> {
        let key = annotation_key(r);
        if let Some(t) = self.0.text_timers.take(&key) {
            rt::clear_timer(t);
        }
        let a = self.model().by_id.get(&key).cloned()?;
        let pon = self.page_of(r);
        let fonts = self.fonts();
        let patch = text_commit_patch(&a, &rich_doc_of(&a, Some(&fonts)).paragraphs, Some(&fonts));
        let doc = self.doc()?;
        doc.page_index(pon).ok()?;
        let mut full = Map::new();
        full.insert("subtype".into(), json!("free-text"));
        if let Value::Object(p) = patch {
            full.extend(p);
        }
        let write = doc.call(
            "page.annotations.update",
            Some(pon),
            vec![serde_json::to_value(r).unwrap_or_default(), Value::Object(full)],
        );
        self.note_write(Some(r.clone()), write.clone());
        write.ignore();
        Some(write)
    }

    /// `scheduleTextCommit(ref)`.
    pub(crate) fn schedule_text_commit(&self, r: &AnnotationRef) {
        let key = annotation_key(r);
        if let Some(t) = self.0.text_timers.take(&key) {
            rt::clear_timer(t);
        }
        let weak = Rc::downgrade(&self.0);
        let owned = r.clone();
        let timer = rt::set_timeout(TEXT_COMMIT_DEBOUNCE_MS, move || {
            if let Some(core) = weak.upgrade() {
                let _ = AnnotationCapability(core).commit_text(&owned);
            }
        });
        self.0.text_timers.0.borrow_mut().push((key, timer));
    }

    /// `flushTextCommits()`: every scheduled write, now.
    pub(crate) fn flush_text_commits(&self) -> Vec<Promise<Value>> {
        let keys: Vec<String> =
            self.0.text_timers.0.borrow().iter().map(|(k, _)| k.clone()).collect();
        let mut pending = Vec::new();
        for key in keys {
            let r = self.model().by_id.get(&key).and_then(|a| a.ref_.clone());
            match r {
                Some(r) => {
                    if let Some(write) = self.commit_text(&r) {
                        pending.push(write);
                    }
                }
                None => {
                    if let Some(t) = self.0.text_timers.take(&key) {
                        rt::clear_timer(t);
                    }
                }
            }
        }
        pending
    }

    /// `await commitText(ref)`: an engine write adopts; `undefined` costs
    /// the one turn awaiting a non-promise does.
    async fn await_commit(write: Option<Promise<Value>>) -> Result<(), Error> {
        match write {
            Some(write) => await_engine(write).await.map(|_| ()),
            None => {
                rt::tick().await;
                Ok(())
            }
        }
    }

    /// `setContents(ref, text)`: plain text, resolved once the engine
    /// confirmed the write.
    pub fn set_contents(&self, r: AnnotationRef, text: String) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let a = this.loaded_or_throw(&r)?;
            this.commit(Msg::SetText { id: a.id.clone(), text });
            let write = this.commit_text(a.ref_.as_ref().expect("loaded"));
            Self::await_commit(write).await
        })
    }

    /// `setRichText(ref, doc)`.
    pub fn set_rich_text(&self, r: AnnotationRef, doc: RichTextParagraphs) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let a = this.loaded_or_throw(&r)?;
            this.commit(Msg::SetRichText {
                id: a.id.clone(),
                doc: RichTextDocumentInput { body: None, paragraphs: doc.paragraphs },
            });
            let write = this.commit_text(a.ref_.as_ref().expect("loaded"));
            Self::await_commit(write).await
        })
    }

    /// `beginTextEdit(ref)`.
    pub fn begin_text_edit(&self, r: &AnnotationRef) {
        self.commit(Msg::BeginTextEdit { id: annotation_key(r) });
    }

    /// `beginTextEditAt(...)` (host): whether a free-text box was opened.
    pub fn begin_text_edit_at(
        &self,
        page: PageRef,
        point: Point,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> bool {
        let m = self.model();
        let h = self.hit_at(page, point, HitView { scale, rotation, zoom }, 1.0, Inert::None);
        let id = match h {
            Target::Annot { id } | Target::Handle { id, .. } => Some(id),
            _ => None,
        };
        if let Some(id) = id
            && m.by_id.get(&id).is_some_and(|a| matches!(a.geom, Geom::Text { .. }))
        {
            self.commit(Msg::BeginTextEdit { id });
            return true;
        }
        false
    }

    /// `endTextEdit()`: flush the pending writes and leave edit mode.
    pub fn end_text_edit(&self) -> Promise<()> {
        let pending = self.flush_text_commits();
        if self.state().text_selection.is_some() {
            self.cx().dispatch(AnnotationAction::SetTextSelection { selection: None });
        }
        self.commit(Msg::EndTextEdit);
        promise::spawn(async move {
            all_settled(pending).await?;
            Ok(())
        })
    }

    /// `getEditingRef()`.
    pub fn get_editing_ref(&self) -> Option<AnnotationRef> {
        let m = self.model();
        m.editing.as_ref().and_then(|id| m.by_id.get(id)).and_then(|a| a.ref_.clone())
    }

    /// `getEditingId()` (host).
    pub fn get_editing_id(&self) -> Option<Id> {
        self.model().editing.clone()
    }

    /// `draftContents(ref, text)` (host): optimistic now, debounced to the
    /// engine.
    pub fn draft_contents(&self, r: AnnotationRef, text: String) {
        self.commit(Msg::SetText { id: annotation_key(&r), text });
        self.schedule_text_commit(&r);
    }

    /// `draftRichText(ref, doc)` (host).
    pub fn draft_rich_text(&self, r: AnnotationRef, doc: RichTextParagraphs) {
        self.commit(Msg::SetRichText {
            id: annotation_key(&r),
            doc: RichTextDocumentInput { body: None, paragraphs: doc.paragraphs },
        });
        self.schedule_text_commit(&r);
    }

    /// `setTextSelection(ref, range)` (host): a repeat is a no-op.
    pub fn set_text_selection(&self, r: &AnnotationRef, range: Option<(f64, f64)>) {
        let id = annotation_key(r);
        let prev = self.state().text_selection.clone();
        let next = range.map(|(start, end)| TextSelection { id, start, end });
        let same = match (&prev, &next) {
            (None, None) => true,
            (Some(p), Some(n)) => p.id == n.id && p.start == n.start && p.end == n.end,
            _ => false,
        };
        if same {
            return;
        }
        self.cx().dispatch(AnnotationAction::SetTextSelection { selection: next.map(Rc::new) });
    }

    /// `getCssFontFamily(family)` (host).
    pub fn get_css_font_family(&self, family: &str) -> String {
        let fonts = self.fonts();
        css_font_family_for_face(family, Some(&fonts))
    }
}
