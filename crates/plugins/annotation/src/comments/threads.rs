//! `comments/threads.ts`: the comments lens's index. A derived threads
//! index over the substrate, display-ordered against the live layout.

use epdf_annotation_core::{AnnotationRef, Id};
use epdf_kernel::Error;
use serde_json::Value;

use crate::comments::build::{CommentThread, build_comment_threads};
use crate::controller::AnnotationCapability;
use crate::repository::{annotation_key, annotation_key_of};
use crate::value::{f64_of, get, str_of};

/// A thread index: the threads and each member's thread.
pub(crate) struct ThreadsIndex {
    pub threads: Vec<CommentThread>,
    by_member: Vec<(Id, usize)>,
}

impl ThreadsIndex {
    pub fn by_member(&self, key: &str) -> Option<&CommentThread> {
        self.by_member.iter().find(|(k, _)| k == key).map(|(_, i)| &self.threads[*i])
    }
}

fn page_pon(v: &Value) -> Option<f64> {
    get(v, "pageObjectNumber").and_then(Value::as_f64)
}

impl AnnotationCapability {
    /// `currentUserId()`.
    pub(crate) fn current_user_id(&self) -> Option<String> {
        let identity = self.doc()?.security().identity();
        str_of(&identity, "user_id").map(str::to_string)
    }

    /// `index()`: committed truth only, display-ordered (page position,
    /// then top of page, then creation).
    pub(crate) fn threads_index(&self) -> ThreadsIndex {
        let m = self.model();
        let dtos: Vec<Value> = m
            .order
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter_map(|a| a.data.clone())
            .collect();
        let user = self.current_user_id();
        let mut threads = build_comment_threads(&dtos, user.as_deref());
        let pages: Vec<f64> = self
            .cx()
            .document()
            .and_then(|d| d.pages.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|p| get(p, "ref").and_then(page_pon).unwrap_or(f64::NAN))
            .collect();
        let index = |t: &CommentThread| -> f64 {
            let pon = page_pon(&t.page);
            pages.iter().position(|p| Some(*p) == pon).map_or(9_007_199_254_740_991.0, |i| i as f64)
        };
        let top = |t: &CommentThread| get(&t.root, "rect").map_or(f64::NAN, |r| f64_of(r, "top"));
        let created =
            |t: &CommentThread| str_of(&t.root, "created").unwrap_or_default().to_string();
        threads.sort_by(|a, b| {
            let (pa, pb) = (index(a), index(b));
            if pa != pb {
                return (pa - pb).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal);
            }
            let (ta, tb) = (top(a), top(b));
            if ta != tb {
                return (tb - ta).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal);
            }
            let (ca, cb) = (created(a), created(b));
            ca.encode_utf16().cmp(cb.encode_utf16())
        });
        let mut by_member: Vec<(Id, usize)> = Vec::new();
        let mut set = |key: Option<String>, i: usize| {
            if let Some(key) = key {
                match by_member.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = i,
                    None => by_member.push((key, i)),
                }
            }
        };
        for (i, t) in threads.iter().enumerate() {
            set(get(&t.root, "ref").and_then(annotation_key_of), i);
            for r in &t.replies {
                set(get(r, "ref").and_then(annotation_key_of), i);
            }
            for g in &t.grouped_parts {
                set(get(g, "ref").and_then(annotation_key_of), i);
            }
            for s in &t.review.status_refs {
                set(annotation_key_of(s), i);
            }
        }
        ThreadsIndex { threads, by_member }
    }

    /// `threadOf(ref)`: the thread containing a member.
    pub(crate) fn thread_of(&self, r: &AnnotationRef) -> Result<CommentThread, Error> {
        self.threads_index()
            .by_member(&annotation_key(r))
            .cloned()
            .ok_or_else(|| Error::new("[annotation] no comment thread contains this ref"))
    }

    /// `rootRefOf(ref)`: the thread's root, else the ref itself.
    pub(crate) fn root_ref_of(&self, r: &AnnotationRef) -> AnnotationRef {
        self.threads_index()
            .by_member(&annotation_key(r))
            .and_then(|t| get(&t.root, "ref"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_else(|| r.clone())
    }
}

/// `memberRefsOf(t)`: replies, grouped parts, status refs, then the root
/// last (children first keeps foreign readers coherent).
pub(crate) fn member_refs_of(t: &CommentThread) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    out.extend(t.replies.iter().map(|r| get(r, "ref").cloned().unwrap_or(Value::Null)));
    out.extend(t.grouped_parts.iter().map(|g| get(g, "ref").cloned().unwrap_or(Value::Null)));
    out.extend(t.review.status_refs.iter().cloned());
    out.push(get(&t.root, "ref").cloned().unwrap_or(Value::Null));
    out
}
