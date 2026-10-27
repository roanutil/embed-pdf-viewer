//! `comments/comments.ts`: the conversation plane's verbs. Every one
//! compiles down to plain annotation creates, patches and deletes (one write
//! path), and each announces the thread it touched (its root, as it was
//! before the change).

use epdf_annotation_core::engine::measure::is_dimension;
use epdf_annotation_core::flags::annot_contents_editable;
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Map, Value, json};

use crate::controller::{CAPABILITY, Core};
use crate::engine::to_page_ref;
use crate::js::{Object, key_or_none, page_object_number};
use crate::services::staging::StoreChange;
use crate::write::outcomes::{applied_annotation_of, applied_or_throw, applied_ref_of};

/// Where a reply or a state sits: the usual 20x20 icon at its root's top-left corner.
fn reply_rect(root: &Value) -> Value {
    let rect = root.get("rect").cloned().unwrap_or(Value::Null);
    json!({ "x": rect.get("x").cloned().unwrap_or(Value::Null), "y": rect.get("y").cloned().unwrap_or(Value::Null), "width": 20, "height": 20 })
}

fn object(value: Value) -> Object {
    value.as_object().cloned().unwrap_or_default()
}

impl Core {
    fn announce_thread(&self, root: &Value, change: &str) {
        self.events.thread_changed.emit(&json!({ "rootRef": root, "change": change }));
    }

    /// `createConversationAnnot(page, draft)`.
    fn create_conversation_annot(&self, page: f64, draft: Object) -> Promise<Value> {
        let applied = self.apply_when_numbered(
            vec![StoreChange::Create { page: to_page_ref(page), draft, resources: None }],
            false,
        );
        Promise::spawn(async move {
            let applied = applied.await?;
            applied_ref_of(&applied, 0).await
        })
    }

    fn delete_one(&self, reference: &Value, signal: Option<AbortSignal>) -> Promise<()> {
        let applied =
            match self.apply(&[StoreChange::Delete { reference: reference.clone() }], false) {
                Ok(a) => a,
                Err(e) => return Promise::rejected(e),
            };
        let done = self.cancellable(signal, applied_or_throw(&applied));
        Promise::spawn(async move {
            done.await?;
            Ok(())
        })
    }

    /// `comments.listThreads()`.
    pub fn comments_list_threads(&self) -> Value {
        Value::Array((*self.threads_index().threads).clone())
    }

    /// `comments.getThread(ref)`.
    pub fn comments_get_thread(&self, reference: &Value) -> Value {
        self.thread_of_member(reference).unwrap_or(Value::Null)
    }

    /// `comments.canSetText(ref)`.
    pub fn comments_can_set_text(&self, reference: &Value) -> bool {
        let model = self.model();
        model.by_id.get(&key_or_none(reference)).is_some_and(|r| {
            let subtype = r.subtype();
            let intent = r.annotation.get("intent").and_then(Value::as_str);
            !is_dimension(subtype, intent) && annot_contents_editable(&**r)
        })
    }

    /// `comments.canDeleteThread(ref)`.
    pub fn comments_can_delete_thread(&self, reference: &Value) -> bool {
        match self.thread_of_member(reference) {
            Some(thread) => Core::member_refs_of(&thread).iter().all(|r| self.can_delete(r)),
            None => false,
        }
    }

    /// `comments.reply(ref, text, options)`.
    pub fn comments_reply(
        &self,
        reference: &Value,
        text: &str,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        if let Err(e) = self.assert_allowed("annotations:create", "annotation.comments.reply") {
            return Promise::rejected(e);
        }
        let root = self.root_ref_of(reference);
        let thread = match self.thread_of(reference) {
            Ok(t) => t,
            Err(e) => return Promise::rejected(e),
        };
        let page = page_object_number(thread.get("page").unwrap_or(&Value::Null));
        let draft = object(json!({
            "subtype": "text",
            "rect": reply_rect(&thread["root"]),
            "icon": "comment",
            "contents": text,
            "reply": { "to": thread.pointer("/root/ref").cloned().unwrap_or(Value::Null) },
            "noZoom": true,
            "noRotate": true,
        }));
        let applied = self.apply_when_numbered(
            vec![StoreChange::Create { page: to_page_ref(page), draft, resources: None }],
            false,
        );
        let me = self.weak();
        Promise::spawn(async move {
            let applied = applied.await?;
            let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
            let annotation = core.cancellable(signal, applied_annotation_of(&applied, 0));
            drop(core);
            let annotation = annotation.await?;
            if let Some(core) = me.upgrade() {
                core.announce_thread(&root, "reply");
            }
            match annotation {
                Some(annotation) => Ok(json!({ "annotation": annotation })),
                None => Err(plugin_error(
                    code::OPERATION_FAILED,
                    CAPABILITY,
                    "the reply could not be created",
                    None,
                    None,
                )),
            }
        })
    }

    /// `comments.setText(ref, text, options)`.
    pub fn comments_set_text(
        &self,
        reference: &Value,
        text: &str,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        let root = self.root_ref_of(reference);
        let model = self.model();
        let record = model.by_id.get(&key_or_none(reference)).cloned();
        if let Some(r) = &record
            && is_dimension(r.subtype(), r.annotation.get("intent").and_then(Value::as_str))
        {
            return Promise::rejected(plugin_error(
                code::INVALID_INPUT,
                CAPABILITY,
                "a measurement's text is worked out from its points",
                None,
                None,
            ));
        }
        let Some(record) = record.filter(|r| r.annotation.get("ref").is_some_and(|x| !x.is_null()))
        else {
            return Promise::rejected(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!("no annotation {}", key_or_none(reference)),
                None,
                None,
            ));
        };
        let mut patch = Map::new();
        patch.insert("subtype".into(), json!(record.subtype()));
        patch.insert("contents".into(), json!(text));
        let updated = self.update(reference, &patch, None, signal);
        let me = self.weak();
        Promise::spawn(async move {
            updated.await?;
            if let Some(core) = me.upgrade() {
                core.announce_thread(&root, "text");
            }
            Ok(())
        })
    }

    fn set_state(
        &self,
        reference: &Value,
        operation: &str,
        draft_of: impl FnOnce(&Value, Option<Value>) -> Object,
        change: &'static str,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        if let Err(e) = self.assert_allowed("annotations:create", operation) {
            return Promise::rejected(e);
        }
        let root = self.root_ref_of(reference);
        let thread = match self.thread_of(reference) {
            Ok(t) => t,
            Err(e) => return Promise::rejected(e),
        };
        let user = self.current_user_id();
        let previous = user.and_then(|u| {
            thread
                .pointer(&format!("/review/byReviewer/{}", u.replace('~', "~0").replace('/', "~1")))
                .and_then(|s| s.get("ref").cloned())
        });
        let page = page_object_number(thread.get("page").unwrap_or(&Value::Null));
        let created = self
            .cancellable(signal, self.create_conversation_annot(page, draft_of(&thread, previous)));
        let me = self.weak();
        Promise::spawn(async move {
            created.await?;
            if let Some(core) = me.upgrade() {
                core.announce_thread(&root, change);
            }
            Ok(())
        })
    }

    /// `comments.setStatus(ref, state, options)`: a reply to the user's
    /// previous state annotation when there is one, else to the root.
    pub fn comments_set_status(
        &self,
        reference: &Value,
        state: &str,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        let state = state.to_string();
        self.set_state(
            reference,
            "annotation.comments.setStatus",
            move |thread, previous| {
                object(json!({
                    "subtype": "text",
                    "rect": reply_rect(&thread["root"]),
                    "reply": { "to": previous.unwrap_or_else(|| thread.pointer("/root/ref").cloned().unwrap_or(Value::Null)) },
                    "state": state,
                    "stateModel": "review",
                    "hidden": true,
                    "noZoom": true,
                    "noRotate": true,
                }))
            },
            "status",
            signal,
        )
    }

    /// `comments.setMarked(ref, marked, options)`.
    pub fn comments_set_marked(
        &self,
        reference: &Value,
        marked: bool,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        self.set_state(
            reference,
            "annotation.comments.setMarked",
            move |thread, _| {
                object(json!({
                    "subtype": "text",
                    "rect": reply_rect(&thread["root"]),
                    "reply": { "to": thread.pointer("/root/ref").cloned().unwrap_or(Value::Null) },
                    "state": if marked { "marked" } else { "unmarked" },
                    "stateModel": "marked",
                    "hidden": true,
                    "noZoom": true,
                    "noRotate": true,
                }))
            },
            "marked",
            signal,
        )
    }

    /// `comments.delete(ref, options)`.
    pub fn comments_delete(&self, reference: &Value, signal: Option<AbortSignal>) -> Promise<()> {
        let root = self.root_ref_of(reference);
        let deleted = self.delete_one(reference, signal);
        let me = self.weak();
        Promise::spawn(async move {
            deleted.await?;
            if let Some(core) = me.upgrade() {
                core.announce_thread(&root, "deleted");
            }
            Ok(())
        })
    }

    /// `comments.deleteThread(ref, options)`: all of it or none.
    pub fn comments_delete_thread(
        &self,
        reference: &Value,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let root = self.root_ref_of(reference);
        let thread = match self.thread_of(reference) {
            Ok(t) => t,
            Err(e) => return Promise::rejected(e),
        };
        let members = Core::member_refs_of(&thread);
        let blocked: Vec<Value> = members.iter().filter(|r| !self.can_delete(r)).cloned().collect();
        if !blocked.is_empty() {
            self.announce_thread(&root, "deleted");
            let failed: Vec<Value> = blocked
                .iter()
                .map(|r| json!({ "ref": r, "error": Error::new("delete not permitted").to_canonical() }))
                .collect();
            return Promise::resolved(json!({ "deleted": [], "failed": failed }));
        }
        let deleted = self.delete_one(&root, signal);
        let me = self.weak();
        Promise::spawn(async move {
            let outcome = deleted.await;
            if let Some(core) = me.upgrade() {
                core.announce_thread(&root, "deleted");
            }
            Ok(match outcome {
                Ok(()) => json!({ "deleted": members, "failed": [] }),
                Err(error) => {
                    json!({ "deleted": [], "failed": [{ "ref": root, "error": error.to_canonical() }] })
                }
            })
        })
    }
}
