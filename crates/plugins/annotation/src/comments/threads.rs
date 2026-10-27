//! `comments/threads.ts`: the comments lens's index, a derived, memoized
//! threads index over the records. Every path (this session's changes,
//! engine confirmations, remote events, reloads) lands in the model, so the
//! sidebar updates with no extra wiring. The memo keys on the model's
//! records (`byId`/`order` identity), the page layout and the user.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::types::{ById, Id};
use epdf_kernel::Error;
use serde_json::Value;

use crate::comments::build::build_comment_threads;
use crate::controller::Core;
use crate::engine::compare_iso_date_time;
use crate::js::key_or_none;

/// The threads, in display order, and the thread of each member.
pub struct ThreadsIndex {
    pub threads: Rc<Vec<Value>>,
    pub by_member: HashMap<Id, usize>,
}

type ThreadsMemo = Option<(Rc<ById>, Rc<Vec<Id>>, String, Option<String>, Rc<ThreadsIndex>)>;

#[derive(Default)]
pub struct ThreadsState {
    memo: RefCell<ThreadsMemo>,
}

impl Core {
    /// `currentUserId()`: the identity's `userId`.
    pub(crate) fn current_user_id(&self) -> Option<String> {
        let identity = self.security()?.identity();
        identity.get("userId").and_then(Value::as_str).map(str::to_string)
    }

    fn layout_signature(&self) -> String {
        let pages = self.pages();
        pages
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|p| crate::js::stringify(p.get("ref").and_then(|r| r.get("objectNumber"))))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default()
    }

    fn compute_threads(&self) -> ThreadsIndex {
        let model = self.model();
        let dtos: Vec<Value> = model
            .order
            .iter()
            .filter_map(|id| model.by_id.get(id))
            .filter(|r| r.annotation.get("ref").is_some_and(|x| !x.is_null()))
            .map(|r| Value::Object((*r.annotation).clone()))
            .collect();
        let mut threads = build_comment_threads(&dtos, self.current_user_id().as_deref());
        let pages = self.pages();
        let display: HashMap<String, usize> = pages
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, p)| {
                (crate::js::stringify(p.get("ref").and_then(|r| r.get("objectNumber"))), i)
            })
            .collect();
        let max = 9_007_199_254_740_991_f64;
        let index_of = |t: &Value| {
            display
                .get(&crate::js::stringify(t.get("page").and_then(|p| p.get("objectNumber"))))
                .map_or(max, |i| *i as f64)
        };
        let y_of =
            |t: &Value| t.pointer("/root/rect/y").and_then(Value::as_f64).unwrap_or(f64::NAN);
        epdf_annotation_core::js::sort_by(&mut threads, |left, right| {
            let pa = index_of(left);
            let pb = index_of(right);
            if pa != pb {
                return pa - pb;
            }
            let (ya, yb) = (y_of(left), y_of(right));
            if ya != yb {
                return ya - yb;
            }
            let lc = left.pointer("/root/createdAt").and_then(Value::as_str);
            let rc = right.pointer("/root/createdAt").and_then(Value::as_str);
            match (lc, rc) {
                (Some(l), Some(r)) => compare_iso_date_time(l, r),
                (None, None) => 0.0,
                (None, Some(_)) => -1.0,
                (Some(_), None) => 1.0,
            }
        });
        let mut by_member = HashMap::new();
        for (i, thread) in threads.iter().enumerate() {
            by_member.insert(key_or_none(thread.pointer("/root/ref").unwrap_or(&Value::Null)), i);
            for list in ["replies", "groupedParts"] {
                for dto in thread.get(list).and_then(Value::as_array).into_iter().flatten() {
                    by_member.insert(key_or_none(dto.get("ref").unwrap_or(&Value::Null)), i);
                }
            }
            for reference in
                thread.pointer("/review/statusRefs").and_then(Value::as_array).into_iter().flatten()
            {
                by_member.insert(key_or_none(reference), i);
            }
        }
        ThreadsIndex { threads: Rc::new(threads), by_member }
    }

    /// `index()`: the threads, memoized. The same `Rc` until the records,
    /// the page layout or the user change (M19: one build per change, not
    /// per read).
    pub fn threads_index(&self) -> Rc<ThreadsIndex> {
        let model = self.model();
        let layout = self.layout_signature();
        let user = self.current_user_id();
        if let Some((by_id, order, l, u, index)) = &*self.threads.memo.borrow()
            && Rc::ptr_eq(by_id, &model.by_id)
            && Rc::ptr_eq(order, &model.order)
            && *l == layout
            && *u == user
        {
            return index.clone();
        }
        let index = Rc::new(self.compute_threads());
        *self.threads.memo.borrow_mut() =
            Some((model.by_id.clone(), model.order.clone(), layout, user, index.clone()));
        index
    }

    /// The thread a member is in.
    pub(crate) fn thread_of_member(&self, reference: &Value) -> Option<Value> {
        let index = self.threads_index();
        index.by_member.get(&key_or_none(reference)).map(|i| index.threads[*i].clone())
    }

    /// `threadOf(ref)`: throws for a ref in no thread.
    pub(crate) fn thread_of(&self, reference: &Value) -> Result<Value, Error> {
        self.thread_of_member(reference)
            .ok_or_else(|| Error::new("[annotation] no comment thread contains this ref"))
    }

    /// `rootRefOf(ref)`.
    pub(crate) fn root_ref_of(&self, reference: &Value) -> Value {
        self.thread_of_member(reference)
            .and_then(|t| t.pointer("/root/ref").cloned())
            .unwrap_or_else(|| reference.clone())
    }

    /// `memberRefsOf(thread)`: replies, grouped parts, status refs, the root last.
    pub(crate) fn member_refs_of(thread: &Value) -> Vec<Value> {
        let mut out = Vec::new();
        for list in ["replies", "groupedParts"] {
            for dto in thread.get(list).and_then(Value::as_array).into_iter().flatten() {
                out.push(dto.get("ref").cloned().unwrap_or(Value::Null));
            }
        }
        for reference in
            thread.pointer("/review/statusRefs").and_then(Value::as_array).into_iter().flatten()
        {
            out.push(reference.clone());
        }
        out.push(thread.pointer("/root/ref").cloned().unwrap_or(Value::Null));
        out
    }
}
