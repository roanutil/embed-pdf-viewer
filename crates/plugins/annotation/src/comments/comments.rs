//! `comments/comments.ts`: the conversation plane's verbs. Every one
//! compiles down to plain annotation creates, patches and deletes (one
//! optimistic pipeline, no second write path), and each announces the
//! thread it touched (its root, as it was before the change).

use epdf_annotation_core::measure::is_dimension;
use epdf_annotation_core::{AnnotationRef, Msg, PageObjectNumber, Source, annot_contents_editable};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::{Value, json};

use crate::comments::build::CommentThread;
use crate::comments::threads::member_refs_of;
use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::promise::{self, await_engine};
use crate::repository::annotation_key;
use crate::value::{get, str_of};

fn ref_of(v: &Value) -> Option<AnnotationRef> {
    serde_json::from_value(v.clone()).ok()
}

fn thread_pon(t: &CommentThread) -> PageObjectNumber {
    get(&t.page, "pageObjectNumber").and_then(Value::as_f64).map_or(0, |p| p as PageObjectNumber)
}

fn root_rect(t: &CommentThread) -> Value {
    get(&t.root, "rect").cloned().unwrap_or(Value::Null)
}

fn root_ref(t: &CommentThread) -> Value {
    get(&t.root, "ref").cloned().unwrap_or(Value::Null)
}

fn dimension_dto(d: &Value) -> bool {
    is_dimension(str_of(d, "subtype").unwrap_or_default(), str_of(d, "intent").unwrap_or_default())
}

/// `annotation.comments`: the conversation lens.
#[derive(Clone)]
pub struct CommentsLens(pub(crate) AnnotationCapability);

impl AnnotationCapability {
    /// `comments`.
    pub fn comments(&self) -> CommentsLens {
        CommentsLens(self.clone())
    }

    /// `createConversationAnnot(pon, draft)`: they never paint, so the
    /// render source is immaterial ('baked').
    fn create_conversation_annot(&self, pon: PageObjectNumber, draft: Value) -> Promise<Value> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            doc.page_index(pon)?;
            let res =
                await_engine(doc.call("page.annotations.create", Some(pon), vec![draft])).await?;
            let created = get(&res, "created").cloned().unwrap_or(Value::Null);
            this.sync(&created, Source::Baked, false);
            Ok(created)
        })
    }

    /// `deleteOne(ref)`.
    fn delete_one(&self, r: AnnotationRef) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
            let pon = match &r {
                AnnotationRef::ObjectNumber { page, .. }
                | AnnotationRef::Nm { page, .. }
                | AnnotationRef::Index { page, .. } => page.page_object_number,
            };
            doc.page_index(pon)?;
            await_engine(doc.call(
                "page.annotations.delete",
                Some(pon),
                vec![serde_json::to_value(&r).unwrap_or_default()],
            ))
            .await?;
            this.commit(Msg::Remove { ids: vec![annotation_key(&r)] });
            Ok(())
        })
    }

    fn announce_thread(&self, root: AnnotationRef, change: ThreadChange) {
        self.0.events.thread_changed.emit(&CommentThreadChangedEvent { root_ref: root, change });
    }
}

// Screen-anchored like a sticky note; `print` for Acrobat parity.
fn reply_flags() -> Value {
    json!({ "print": true, "noZoom": true, "noRotate": true })
}

// Status annotations are metadata: hidden everywhere.
fn status_flags() -> Value {
    json!({ "hidden": true, "noZoom": true, "noRotate": true })
}

impl CommentsLens {
    /// `listThreads()`: every thread, display-ordered.
    pub fn list_threads(&self) -> Vec<CommentThread> {
        self.0.threads_index().threads.clone()
    }

    /// `getThread(ref)`: the thread containing a member.
    pub fn get_thread(&self, r: &AnnotationRef) -> Option<CommentThread> {
        self.0.threads_index().by_member(&annotation_key(r)).cloned()
    }

    /// `onThreadChanged(listener)`.
    pub fn on_thread_changed(
        &self,
        listener: impl Fn(&CommentThreadChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.0.events.thread_changed.on(listener)
    }

    /// `reply(ref, text)`: flat, to the root, whatever member was passed.
    pub fn reply(&self, r: AnnotationRef, text: String) -> Promise<AnnotationRef> {
        let this = self.0.clone();
        promise::spawn(async move {
            let root = this.root_ref_of(&r);
            let t = this.thread_of(&r)?;
            let draft = json!({
                "subtype": "text",
                "rect": root_rect(&t),
                "icon": "comment",
                "contents": text,
                "inReplyTo": root_ref(&t),
                "flags": reply_flags(),
            });
            let created = this.create_conversation_annot(thread_pon(&t), draft).await?;
            this.announce_thread(root, ThreadChange::Reply);
            get(&created, "ref").and_then(ref_of).ok_or_else(|| {
                Error::named("TypeError", "Cannot read properties of undefined (reading 'ref')")
            })
        })
    }

    /// `setText(ref, text)`: a measurement's contents are derived.
    pub fn set_text(&self, r: AnnotationRef, text: String) -> Promise<()> {
        let this = self.0.clone();
        promise::spawn(async move {
            let root = this.root_ref_of(&r);
            let data = this.model().by_id.get(&annotation_key(&r)).and_then(|a| a.data.clone());
            if data.as_ref().is_some_and(dimension_dto) {
                return Err(Error::new("[annotation] measurement contents are derived"));
            }
            let subtype =
                data.as_ref().and_then(|d| str_of(d, "subtype")).filter(|s| !s.is_empty());
            let Some(subtype) = subtype else {
                return Err(Error::new("[annotation] cannot edit an uncommitted annotation"));
            };
            this.update_raw(r, json!({ "subtype": subtype, "contents": text })).await?;
            this.announce_thread(root, ThreadChange::Text);
            Ok(())
        })
    }

    /// `setStatus(ref, state)`: this session's review status, chained to
    /// its previous one.
    pub fn set_status(&self, r: AnnotationRef, state: String) -> Promise<()> {
        let this = self.0.clone();
        promise::spawn(async move {
            let root = this.root_ref_of(&r);
            let t = this.thread_of(&r)?;
            let user = this.current_user_id();
            let previous =
                user.as_deref().and_then(|u| t.review.by_reviewer.get(u).map(|s| s.ref_.clone()));
            let draft = json!({
                "subtype": "text",
                "rect": root_rect(&t),
                "inReplyTo": previous.unwrap_or_else(|| root_ref(&t)),
                "state": state,
                "stateModel": "review",
                "flags": status_flags(),
            });
            this.create_conversation_annot(thread_pon(&t), draft).await?;
            this.announce_thread(root, ThreadChange::Status);
            Ok(())
        })
    }

    /// `setMarked(ref, marked)`: this session's checkmark.
    pub fn set_marked(&self, r: AnnotationRef, marked: bool) -> Promise<()> {
        let this = self.0.clone();
        promise::spawn(async move {
            let root = this.root_ref_of(&r);
            let t = this.thread_of(&r)?;
            let draft = json!({
                "subtype": "text",
                "rect": root_rect(&t),
                "inReplyTo": root_ref(&t),
                "state": if marked { "marked" } else { "unmarked" },
                "stateModel": "marked",
                "flags": status_flags(),
            });
            this.create_conversation_annot(thread_pon(&t), draft).await?;
            this.announce_thread(root, ThreadChange::Marked);
            Ok(())
        })
    }

    /// `delete(ref)`: exactly one annotation.
    pub fn delete(&self, r: AnnotationRef) -> Promise<()> {
        let this = self.0.clone();
        promise::spawn(async move {
            let root = this.root_ref_of(&r);
            this.delete_one(r).await?;
            this.announce_thread(root, ThreadChange::Deleted);
            Ok(())
        })
    }

    /// `deleteThread(ref)`: children first, preflighted all-or-nothing.
    pub fn delete_thread(&self, r: AnnotationRef) -> Promise<ThreadDeleteResult> {
        let this = self.0.clone();
        promise::spawn(async move {
            let root = this.root_ref_of(&r);
            let t = this.thread_of(&r)?;
            let members: Vec<AnnotationRef> =
                member_refs_of(&t).iter().filter_map(ref_of).collect();
            let blocked: Vec<AnnotationRef> =
                members.iter().filter(|m| !this.can_delete(m)).cloned().collect();
            if !blocked.is_empty() {
                this.announce_thread(root, ThreadChange::Deleted);
                return Ok(ThreadDeleteResult {
                    deleted: Vec::new(),
                    failed: blocked
                        .into_iter()
                        .map(|r| ThreadDeleteFailure {
                            ref_: r,
                            error: Error::new("delete not permitted"),
                        })
                        .collect(),
                });
            }
            let mut deleted = Vec::new();
            let mut failed = Vec::new();
            for member in members {
                if !failed.is_empty() {
                    break;
                }
                match this.delete_one(member.clone()).await {
                    Ok(()) => deleted.push(member),
                    Err(error) => failed.push(ThreadDeleteFailure { ref_: member, error }),
                }
            }
            this.announce_thread(root, ThreadChange::Deleted);
            Ok(ThreadDeleteResult { deleted, failed })
        })
    }

    /// `getPermissions(ref)`: per-thread action gates for this session.
    pub fn get_permissions(&self, r: &AnnotationRef) -> CommentPermissions {
        let this = &self.0;
        let index = this.threads_index();
        let t = index.by_member(&annotation_key(r));
        let m = this.model();
        let a = m.by_id.get(&annotation_key(r));
        CommentPermissions {
            can_reply: this.can_create(),
            can_set_status: this.can_create(),
            can_edit_text: a.is_some_and(|a| {
                !a.data.as_ref().is_some_and(dimension_dto) && annot_contents_editable(a)
            }),
            can_delete: this.can_delete(r),
            can_delete_thread: t.is_some_and(|t| {
                member_refs_of(t).iter().all(|m| ref_of(m).is_some_and(|m| this.can_delete(&m)))
            }),
        }
    }
}
