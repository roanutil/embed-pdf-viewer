//! `buildCommentThreads` from `engine-core` (`annotation/comments.ts`), the
//! conversation-plane view the `comments` lens memoizes. The plugin layer
//! links no engine, so the composer is transcribed here; DTOs stay JSON.

use std::cmp::Ordering;

use epdf_annotation_core::js::Record;
use serde_json::{Map, Value, json};

use crate::value::{get, str_of};

/// `refKey` from `annotation/relationships.ts` (page-qualified, unlike the
/// plugin's model ids).
fn rel_key(r: &Value) -> String {
    let pon = get(r, "pageObjectNumber").map(num_text).unwrap_or_default();
    match str_of(r, "kind") {
        Some("objectNumber") => {
            format!("obj:{pon}:{}", get(r, "annotObjectNumber").map(num_text).unwrap_or_default())
        }
        Some("nm") => format!("nm:{pon}:{}", str_of(r, "nm").unwrap_or_default()),
        _ => format!("idx:{pon}:{}", get(r, "index").map(num_text).unwrap_or_default()),
    }
}

fn num_text(v: &Value) -> String {
    match v {
        Value::Number(n) => n.as_f64().map(epdf_geometry::js::number_to_string).unwrap_or_default(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

const REVIEW_STATES: [&str; 5] = ["accepted", "rejected", "cancelled", "completed", "none"];
const MARKED_STATES: [&str; 2] = ["marked", "unmarked"];

fn non_empty<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    str_of(v, key).filter(|s| !s.is_empty())
}

/// `isStateAnnotation(a)`.
pub fn is_state_annotation(a: &Value) -> bool {
    str_of(a, "subtype") == Some("text")
        && (non_empty(a, "state").is_some() || non_empty(a, "stateModel").is_some())
}

fn ref_of(a: &Value) -> Value {
    get(a, "ref").cloned().unwrap_or(Value::Null)
}

/// One reviewer's status.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewStatus {
    pub state: String,
    pub state_model: String,
    pub by: Option<String>,
    pub at: Option<String>,
    pub ref_: Value,
}

impl ReviewStatus {
    fn to_json(&self) -> Value {
        json!({
            "state": self.state,
            "stateModel": self.state_model,
            "by": self.by,
            "at": self.at,
            "ref": self.ref_,
        })
    }
}

/// `CommentThreadReview`.
#[derive(Clone, Debug, PartialEq)]
pub struct CommentThreadReview {
    pub by_reviewer: Record<ReviewStatus>,
    pub last_change: Option<ReviewStatus>,
    pub marked_by: Vec<String>,
    pub status_refs: Vec<Value>,
    /// `Some` only when a current user id was given.
    pub mine: Option<Option<ReviewStatus>>,
}

/// `CommentThread`.
#[derive(Clone, Debug, PartialEq)]
pub struct CommentThread {
    pub root: Value,
    pub page_object_number: Value,
    pub replies: Vec<Value>,
    pub grouped_parts: Vec<Value>,
    pub review: CommentThreadReview,
}

impl CommentThread {
    pub fn to_json(&self) -> Value {
        let mut by = Map::new();
        for (k, v) in self.review.by_reviewer.iter() {
            by.insert(k.to_string(), v.to_json());
        }
        let mut review = Map::new();
        review.insert("byReviewer".into(), Value::Object(by));
        review.insert(
            "lastChange".into(),
            self.review.last_change.as_ref().map_or(Value::Null, ReviewStatus::to_json),
        );
        review.insert("markedBy".into(), json!(self.review.marked_by));
        review.insert("statusRefs".into(), Value::Array(self.review.status_refs.clone()));
        if let Some(mine) = &self.review.mine {
            review.insert("mine".into(), mine.as_ref().map_or(Value::Null, ReviewStatus::to_json));
        }
        json!({
            "root": self.root,
            "pageObjectNumber": self.page_object_number,
            "replies": self.replies,
            "groupedParts": self.grouped_parts,
            "review": Value::Object(review),
        })
    }
}

fn date(a: &Value, key: &str) -> Option<String> {
    str_of(a, key).map(str::to_string)
}

/// `created ?? modified` ascending; undated last; z-order tiebreak.
fn chronological(a: &Value, b: &Value) -> Ordering {
    let at = date(a, "created").or_else(|| date(a, "modified"));
    let bt = date(b, "created").or_else(|| date(b, "modified"));
    match (&at, &bt) {
        (Some(x), Some(y)) if x != y => return if x < y { Ordering::Less } else { Ordering::Greater },
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        _ => {}
    }
    let ia = get(a, "index").and_then(Value::as_f64).unwrap_or(f64::NAN);
    let ib = get(b, "index").and_then(Value::as_f64).unwrap_or(f64::NAN);
    let d = ia - ib;
    if d < 0.0 {
        Ordering::Less
    } else if d > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

fn to_review_status(a: &Value) -> Option<ReviewStatus> {
    if str_of(a, "subtype") != Some("text") {
        return None;
    }
    let raw_state = non_empty(a, "state");
    let raw_model = non_empty(a, "stateModel");
    let mut state_model = raw_model.map(str::to_string);
    if state_model.is_none()
        && let Some(s) = raw_state
    {
        if REVIEW_STATES.contains(&s) {
            state_model = Some("review".into());
        } else if MARKED_STATES.contains(&s) {
            state_model = Some("marked".into());
        } else {
            return None;
        }
    }
    let state = match raw_state {
        Some(s) => s.to_string(),
        None => match state_model.as_deref() {
            Some("review") => "none".into(),
            Some("marked") => "unmarked".into(),
            _ => return None,
        },
    };
    let state_model = state_model?;
    let by = str_of(a, "userId")
        .map(str::to_string)
        .or_else(|| non_empty(a, "author").map(str::to_string));
    Some(ReviewStatus {
        state,
        state_model,
        by,
        at: date(a, "modified").or_else(|| date(a, "created")),
        ref_: ref_of(a),
    })
}

fn is_newer(a: &ReviewStatus, b: Option<&ReviewStatus>) -> bool {
    let Some(b) = b else { return true };
    match (&a.at, &b.at) {
        (Some(x), Some(y)) => x >= y,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => true,
    }
}

fn compute_review(states: &[Value], current_user_id: Option<&str>) -> CommentThreadReview {
    let mut by_reviewer: Record<ReviewStatus> = Record::new();
    let mut marked_latest: Record<ReviewStatus> = Record::new();
    let mut last_change: Option<ReviewStatus> = None;
    for a in states {
        let Some(status) = to_review_status(a) else { continue };
        if status.state_model == "marked" {
            if let Some(by) = status.by.clone()
                && is_newer(&status, marked_latest.get(&by))
            {
                marked_latest.insert(by, status);
            }
            continue;
        }
        if let Some(by) = status.by.clone()
            && is_newer(&status, by_reviewer.get(&by))
        {
            by_reviewer.insert(by, status.clone());
        }
        if is_newer(&status, last_change.as_ref()) {
            last_change = Some(status);
        }
    }
    let mut marked_by: Vec<String> = marked_latest
        .iter()
        .filter(|(_, s)| s.state == "marked")
        .map(|(k, _)| k.to_string())
        .collect();
    marked_by.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let mine = current_user_id.map(|id| by_reviewer.get(id).cloned());
    CommentThreadReview {
        by_reviewer,
        last_change,
        marked_by,
        status_refs: states.iter().map(ref_of).collect(),
        mine,
    }
}

fn is_grouped_subordinate(a: &Value) -> bool {
    crate::value::truthy(get(a, "inReplyTo")) && str_of(a, "replyType") == Some("group")
}

/// `buildCommentThreads(annotations, { currentUserId })`.
pub fn build_comment_threads(annotations: &[Value], current_user_id: Option<&str>) -> Vec<CommentThread> {
    let eligible: Vec<&Value> = annotations
        .iter()
        .filter(|a| !matches!(str_of(a, "subtype"), Some("widget" | "link" | "unsupported")))
        .collect();

    let mut by_key: Vec<(String, &Value)> = Vec::new();
    let find = |by_key: &Vec<(String, &Value)>, k: &str| -> Option<usize> {
        by_key.iter().position(|(key, _)| key == k)
    };
    for a in &eligible {
        let key = rel_key(&ref_of(a));
        if find(&by_key, &key).is_none() {
            by_key.push((key, a));
        }
        if let Some(nm) = non_empty(a, "nm") {
            let alias = rel_key(&json!({
                "kind": "nm",
                "pageObjectNumber": get(a, "pageObjectNumber").cloned().unwrap_or(Value::Null),
                "nm": nm,
            }));
            if find(&by_key, &alias).is_none() {
                by_key.push((alias, a));
            }
        }
    }

    let mut children: Vec<(String, Vec<&Value>)> = Vec::new();
    for a in &eligible {
        let Some(irt) = get(a, "inReplyTo").filter(|v| crate::value::truthy(Some(v))) else {
            continue;
        };
        let Some(i) = find(&by_key, &rel_key(irt)) else { continue };
        let parent_key = rel_key(&ref_of(by_key[i].1));
        match children.iter_mut().find(|(k, _)| *k == parent_key) {
            Some((_, list)) => list.push(a),
            None => children.push((parent_key, vec![a])),
        }
    }
    let children_of = |key: &str| -> Vec<&Value> {
        children.iter().find(|(k, _)| k == key).map(|(_, l)| l.clone()).unwrap_or_default()
    };

    let mut visited: Vec<String> = Vec::new();
    let mut threads: Vec<CommentThread> = Vec::new();
    let walk = |root: &Value, visited: &mut Vec<String>, threads: &mut Vec<CommentThread>| {
        let mut replies: Vec<Value> = Vec::new();
        let mut grouped: Vec<Value> = Vec::new();
        let mut states: Vec<Value> = Vec::new();
        let root_key = rel_key(&ref_of(root));
        visited.push(root_key.clone());
        let mut stack: Vec<&Value> = children_of(&root_key);
        while let Some(a) = stack.pop() {
            let key = rel_key(&ref_of(a));
            if visited.contains(&key) {
                continue;
            }
            visited.push(key.clone());
            if is_state_annotation(a) {
                states.push(a.clone());
            } else if is_grouped_subordinate(a) {
                grouped.push(a.clone());
            } else {
                replies.push(a.clone());
            }
            stack.extend(children_of(&key));
        }
        replies.sort_by(chronological);
        states.sort_by(chronological);
        threads.push(CommentThread {
            root: root.clone(),
            page_object_number: get(root, "pageObjectNumber").cloned().unwrap_or(Value::Null),
            replies,
            grouped_parts: grouped,
            review: compute_review(&states, current_user_id),
        });
    };

    for a in &eligible {
        if !crate::value::truthy(get(a, "inReplyTo")) && !is_state_annotation(a) {
            walk(a, &mut visited, &mut threads);
        }
    }
    for a in &eligible {
        if visited.contains(&rel_key(&ref_of(a))) || is_state_annotation(a) {
            continue;
        }
        walk(a, &mut visited, &mut threads);
    }
    threads
}
