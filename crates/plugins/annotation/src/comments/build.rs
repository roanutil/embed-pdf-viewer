//! `buildCommentThreads` from `engine-core` (`annotation/comments.ts`): the
//! conversation-plane view over the flat annotation list. The plugin links
//! no engine, so the composer is transcribed here; annotations stay JSON.
//!
//! Relation keys are `annotationKey` (`obj:<n>`, `base:<page>:<i>`), so two
//! inline annotations on one page key apart. Replies sort by
//! `createdAt ?? modifiedAt`, compared as `Date.parse` does, undated last,
//! z-order (place in the input) as the tiebreak.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::engine::compare_iso_date_time;
use crate::js::key_or_none;

const EXCLUDED_SUBTYPES: [&str; 4] = ["widget", "link", "popup", "unsupported"];
const REVIEW_STATES: [&str; 5] = ["accepted", "rejected", "cancelled", "completed", "none"];
const MARKED_STATES: [&str; 2] = ["marked", "unmarked"];

fn str_of<'a>(a: &'a Value, key: &str) -> Option<&'a str> {
    a.get(key).and_then(Value::as_str)
}

fn non_empty<'a>(a: &'a Value, key: &str) -> Option<&'a str> {
    str_of(a, key).filter(|s| !s.is_empty())
}

fn key(a: &Value) -> String {
    key_or_none(a.get("ref").unwrap_or(&Value::Null))
}

/// `standardStateModelOf(state)`.
pub fn standard_state_model_of(state: &str) -> Option<&'static str> {
    if REVIEW_STATES.contains(&state) {
        Some("review")
    } else if MARKED_STATES.contains(&state) {
        Some("marked")
    } else {
        None
    }
}

/// `isStateAnnotation(a)`.
pub fn is_state_annotation(a: &Value) -> bool {
    str_of(a, "subtype") == Some("text")
        && (non_empty(a, "state").is_some() || non_empty(a, "stateModel").is_some())
}

fn has_reply(a: &Value) -> bool {
    a.get("reply")
        .is_some_and(|r| !r.is_null() && !epdf_kernel::is_undefined(r) && r != &Value::Bool(false))
}

/// `classifyRelation(a) === 'grouped-subordinate'`.
fn is_grouped_subordinate(a: &Value) -> bool {
    has_reply(a) && a.pointer("/reply/type").and_then(Value::as_str) == Some("group")
}

/// A date member, `null` as absent (`a.createdAt ?? a.modifiedAt`).
fn date_of<'a>(a: &'a Value, key: &str) -> Option<&'a str> {
    a.get(key).and_then(Value::as_str)
}

fn chronological(z: &HashMap<String, usize>) -> impl Fn(&Value, &Value) -> Ordering + '_ {
    move |a, b| {
        let at = date_of(a, "createdAt").or_else(|| date_of(a, "modifiedAt"));
        let bt = date_of(b, "createdAt").or_else(|| date_of(b, "modifiedAt"));
        let order = match (at, bt) {
            (Some(x), Some(y)) => compare_iso_date_time(x, y),
            _ => 0.0,
        };
        if order != 0.0 {
            return if order < 0.0 { Ordering::Less } else { Ordering::Greater };
        }
        match (at, bt) {
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            _ => {}
        }
        z.get(&key(a)).copied().unwrap_or(0).cmp(&z.get(&key(b)).copied().unwrap_or(0))
    }
}

/// One reviewer's status.
#[derive(Clone, Debug)]
struct ReviewStatus {
    state: String,
    state_model: String,
    by: Option<String>,
    at: Option<String>,
    reference: Value,
}

impl ReviewStatus {
    fn to_json(&self) -> Value {
        json!({ "state": self.state, "stateModel": self.state_model, "by": self.by, "at": self.at, "ref": self.reference })
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
        state_model = Some(standard_state_model_of(s)?.to_string());
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
    let by = a
        .get("userId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| non_empty(a, "author").map(str::to_string));
    let at = date_of(a, "modifiedAt").or_else(|| date_of(a, "createdAt")).map(str::to_string);
    Some(ReviewStatus {
        state,
        state_model,
        by,
        at,
        reference: a.get("ref").cloned().unwrap_or(Value::Null),
    })
}

/// `isNewer(a, b)`: `at` decides; dated beats undated; otherwise the later entry wins.
fn is_newer(a: &ReviewStatus, b: Option<&ReviewStatus>) -> bool {
    let Some(b) = b else { return true };
    match (&a.at, &b.at) {
        (Some(x), Some(y)) => compare_iso_date_time(x, y) >= 0.0,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => true,
    }
}

fn compute_review(states: &[Value], current_user_id: Option<&str>) -> Value {
    let mut by_reviewer: Vec<(String, ReviewStatus)> = Vec::new();
    let mut marked_latest: Vec<(String, ReviewStatus)> = Vec::new();
    let mut last_change: Option<ReviewStatus> = None;
    let upsert = |list: &mut Vec<(String, ReviewStatus)>, by: &str, status: &ReviewStatus| {
        let current = list.iter().position(|(k, _)| k == by);
        if is_newer(status, current.map(|i| &list[i].1)) {
            match current {
                Some(i) => list[i].1 = status.clone(),
                None => list.push((by.to_string(), status.clone())),
            }
        }
    };
    for a in states {
        let Some(status) = to_review_status(a) else { continue };
        if status.state_model == "marked" {
            if let Some(by) = &status.by {
                upsert(&mut marked_latest, by, &status);
            }
            continue;
        }
        if let Some(by) = &status.by {
            upsert(&mut by_reviewer, by, &status);
        }
        if is_newer(&status, last_change.as_ref()) {
            last_change = Some(status.clone());
        }
    }
    let mut marked_by: Vec<String> =
        marked_latest.iter().filter(|(_, s)| s.state == "marked").map(|(k, _)| k.clone()).collect();
    marked_by.sort();
    let mut reviewers = Map::new();
    for (k, s) in record_order(&by_reviewer) {
        reviewers.insert(k, s.to_json());
    }
    let mut review = Map::new();
    review.insert("byReviewer".into(), Value::Object(reviewers));
    review.insert(
        "lastChange".into(),
        last_change.as_ref().map_or(Value::Null, ReviewStatus::to_json),
    );
    review.insert("markedBy".into(), json!(marked_by));
    review.insert(
        "statusRefs".into(),
        Value::Array(states.iter().map(|a| a.get("ref").cloned().unwrap_or(Value::Null)).collect()),
    );
    if let Some(user) = current_user_id {
        let mine =
            by_reviewer.iter().find(|(k, _)| k == user).map_or(Value::Null, |(_, s)| s.to_json());
        review.insert("mine".into(), mine);
    }
    Value::Object(review)
}

/// A JavaScript object's key order: integer-like keys first, ascending,
/// then the rest in insertion order.
fn record_order(list: &[(String, ReviewStatus)]) -> Vec<(String, ReviewStatus)> {
    let index = |k: &str| -> Option<u64> {
        if k == "0"
            || (!k.starts_with('0') && !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
        {
            k.parse::<u64>().ok().filter(|n| *n < 4_294_967_295)
        } else {
            None
        }
    };
    let mut ints: Vec<(u64, (String, ReviewStatus))> =
        list.iter().filter_map(|(k, s)| index(k).map(|n| (n, (k.clone(), s.clone())))).collect();
    ints.sort_by_key(|(n, _)| *n);
    let mut out: Vec<(String, ReviewStatus)> = ints.into_iter().map(|(_, e)| e).collect();
    out.extend(list.iter().filter(|(k, _)| index(k).is_none()).cloned());
    out
}

/// `buildCommentThreads(annotations, { currentUserId })`.
pub fn build_comment_threads(annotations: &[Value], current_user_id: Option<&str>) -> Vec<Value> {
    let eligible: Vec<&Value> = annotations
        .iter()
        .filter(|a| !EXCLUDED_SUBTYPES.contains(&str_of(a, "subtype").unwrap_or("undefined")))
        .collect();
    let by_key: HashMap<String, &Value> = eligible.iter().map(|a| (key(a), *a)).collect();
    let z: HashMap<String, usize> = eligible.iter().enumerate().map(|(i, a)| (key(a), i)).collect();
    let mut children: HashMap<String, Vec<&Value>> = HashMap::new();
    for a in &eligible {
        if !has_reply(a) {
            continue;
        }
        let Some(parent) = by_key.get(&key_or_none(a.pointer("/reply/to").unwrap_or(&Value::Null)))
        else {
            continue;
        };
        children.entry(key(parent)).or_default().push(a);
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut threads: Vec<Value> = Vec::new();
    let order = chronological(&z);
    let mut walk = |root: &Value, visited: &mut HashSet<String>| {
        let mut replies: Vec<Value> = Vec::new();
        let mut grouped: Vec<Value> = Vec::new();
        let mut states: Vec<Value> = Vec::new();
        visited.insert(key(root));
        let mut stack: Vec<&Value> = children.get(&key(root)).cloned().unwrap_or_default();
        while let Some(a) = stack.pop() {
            let k = key(a);
            if visited.contains(&k) {
                continue;
            }
            visited.insert(k.clone());
            if is_state_annotation(a) {
                states.push(a.clone());
            } else if is_grouped_subordinate(a) {
                grouped.push(a.clone());
            } else {
                replies.push(a.clone());
            }
            stack.extend(children.get(&k).cloned().unwrap_or_default());
        }
        epdf_annotation_core::js::sort_by_ordering(&mut replies, &order);
        epdf_annotation_core::js::sort_by_ordering(&mut states, &order);
        threads.push(json!({
            "root": root,
            "page": root.get("page").cloned().unwrap_or(Value::Null),
            "replies": replies,
            "groupedParts": grouped,
            "review": compute_review(&states, current_user_id),
        }));
    };
    for a in &eligible {
        if !has_reply(a) && !is_state_annotation(a) {
            walk(a, &mut visited);
        }
    }
    for a in &eligible {
        if visited.contains(&key(a)) || is_state_annotation(a) {
            continue;
        }
        walk(a, &mut visited);
    }
    threads
}
