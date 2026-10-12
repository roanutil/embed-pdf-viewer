//! `comments/threads.ts`: the comments lens's index. A derived threads
//! index over the substrate, display-ordered against the live layout.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use epdf_annotation_core::{Annot, AnnotationRef, Id, Model};
use epdf_kernel::Error;
use serde_json::Value;

use crate::comments::build::{CommentThread, build_comment_threads};
use crate::controller::AnnotationCapability;
use crate::repository::{annotation_key, annotation_key_of};
use crate::value::{f64_of, get, str_of};

/// A thread index: the threads and each member's thread.
pub(crate) struct ThreadsIndex {
    pub threads: Vec<CommentThread>,
    by_member: HashMap<Id, usize>,
}

impl ThreadsIndex {
    pub fn by_member(&self, key: &str) -> Option<&CommentThread> {
        self.by_member.get(key).map(|i| &self.threads[*i])
    }
}

/// The last index and what it was built from. As in the TypeScript, the
/// memo keys on the model's annotation tables by identity (hover and drafts
/// share them, so they don't invalidate), the page layout and the user.
#[derive(Default)]
pub(crate) struct ThreadsMemo(RefCell<Option<MemoEntry>>);

struct MemoEntry {
    by_id: Rc<BTreeMap<Id, Annot>>,
    order: Rc<Vec<Id>>,
    /// Each page's object number as bits, so a NaN still compares equal.
    layout: Vec<u64>,
    user: Option<String>,
    index: Rc<ThreadsIndex>,
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
    /// then top of page, then creation). Memoized like the TypeScript's.
    pub(crate) fn threads_index(&self) -> Rc<ThreadsIndex> {
        let m = self.model();
        let pages = self.page_layout();
        let user = self.current_user_id();
        let memo = &self.0.threads_memo.0;
        if let Some(entry) = &*memo.borrow()
            && Rc::ptr_eq(&entry.by_id, &m.by_id)
            && Rc::ptr_eq(&entry.order, &m.order)
            && entry.user == user
            && entry.layout.len() == pages.len()
            && entry.layout.iter().zip(&pages).all(|(a, b)| *a == b.to_bits())
        {
            return entry.index.clone();
        }
        let index = Rc::new(compute_threads_index(&m, &pages, user.as_deref()));
        *memo.borrow_mut() = Some(MemoEntry {
            by_id: m.by_id.clone(),
            order: m.order.clone(),
            layout: pages.iter().map(|p| p.to_bits()).collect(),
            user,
            index: index.clone(),
        });
        index
    }

    /// Each page's object number in display order (`NaN` where a page has
    /// none).
    fn page_layout(&self) -> Vec<f64> {
        self.cx()
            .document()
            .and_then(|d| d.pages.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|p| get(p, "ref").and_then(page_pon).unwrap_or(f64::NAN))
            .collect()
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

fn compute_threads_index(m: &Model, pages: &[f64], user: Option<&str>) -> ThreadsIndex {
    let dtos: Vec<Value> =
        m.order.iter().filter_map(|id| m.by_id.get(id)).filter_map(|a| a.data.clone()).collect();
    let mut threads = build_comment_threads(&dtos, user);
    // First position of each page; NaN never matches, as `==` would not.
    let mut position: HashMap<u64, usize> = HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        if !p.is_nan() {
            // 0.0 and -0.0 compare equal, so key them alike.
            position.entry((p + 0.0).to_bits()).or_insert(i);
        }
    }
    let index = |t: &CommentThread| -> f64 {
        page_pon(&t.page)
            .and_then(|pon| position.get(&(pon + 0.0).to_bits()))
            .map_or(9_007_199_254_740_991.0, |i| *i as f64)
    };
    let top = |t: &CommentThread| get(&t.root, "rect").map_or(f64::NAN, |r| f64_of(r, "top"));
    let created = |t: &CommentThread| str_of(&t.root, "created").unwrap_or_default().to_string();
    // V8's sort, so a NaN `top` (which makes the comparator answer NaN, read
    // as 0) leaves the threads where the TypeScript leaves them.
    epdf_annotation_core::js::sort_by(&mut threads, |a, b| {
        let (pa, pb) = (index(a), index(b));
        if pa != pb {
            return pa - pb;
        }
        let (ta, tb) = (top(a), top(b));
        if ta != tb {
            return tb - ta;
        }
        let (ca, cb) = (created(a), created(b));
        ca.encode_utf16().cmp(cb.encode_utf16()) as i8 as f64
    });
    let mut by_member: HashMap<Id, usize> = HashMap::new();
    let mut set = |key: Option<String>, i: usize| {
        if let Some(key) = key {
            by_member.insert(key, i);
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
