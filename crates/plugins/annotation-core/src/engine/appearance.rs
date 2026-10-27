//! Whether an update re-bakes an annotation's appearance
//! (`annotation/appearance.ts`): `semanticEqual` and `appearanceChangeOf`,
//! on values in either space.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::json::{self, Object};

/// An update's appearance impact (`AppearanceChange`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppearanceChange {
    Inert,
    /// A verified rigid translation, by how far.
    Translation {
        x: f64,
        y: f64,
    },
    Regenerate,
}

const EPSILON: f64 = 1e-3;

const INERT_KEYS: &[&str] = &[
    "subtype",
    "invisible",
    "hidden",
    "print",
    "noZoom",
    "noRotate",
    "noView",
    "readOnly",
    "locked",
    "toggleNoView",
    "lockedContents",
    "nm",
    "reply",
    "popup",
    "parent",
    "open",
    "groupId",
    "subject",
    "state",
    "stateModel",
    "ref",
    "page",
    "hasAppearance",
    "appearanceState",
    "author",
    "createdAt",
    "modifiedAt",
    "userId",
    "createdBy",
    "modifiedBy",
    "importedBy",
    "actions",
    "file",
    "field",
    "fieldFamily",
];

const CONTENTS_PAINTED: [&str; 2] = ["free-text", "redact"];

const TURNING_KINDS: [&str; 10] = [
    "square",
    "circle",
    "free-text",
    "stamp",
    "caret",
    "widget",
    "line",
    "polyline",
    "polygon",
    "ink",
];

fn translatable_geometry(subtype: &str) -> Option<&'static [&'static str]> {
    Some(match subtype {
        "square" | "circle" | "caret" | "stamp" | "widget" => &["box"],
        "free-text" => &["box", "calloutLine"],
        "line" => &["linePoints"],
        "polygon" | "polyline" => &["vertices"],
        "ink" => &["inkList"],
        "highlight" | "underline" | "squiggly" | "strikeout" => &["quadPoints"],
        "redact" => &["rect", "quadPoints"],
        "text" | "file-attachment" | "link" => &["rect"],
        _ => return None,
    })
}

fn num_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPSILON
}

/// Degrees in `[0, 360)`; anything not a number reads as 0.
fn norm_deg(value: Option<&Value>) -> f64 {
    match value.and_then(json::number) {
        Some(n) => ((n % 360.0) + 360.0) % 360.0,
        None => 0.0,
    }
}

/// Object keys of `a` then the new ones of `b`, as a `Set` of both collects them.
fn union_keys<'a>(a: &'a Object, b: &'a Object) -> Vec<&'a String> {
    let mut keys: Vec<&String> = a.keys().collect();
    for k in b.keys() {
        if !a.contains_key(k) {
            keys.push(k);
        }
    }
    keys
}

/// `semanticEqual`: `null` and absent are the same, numbers within 1e-3,
/// arrays and objects structurally.
pub fn semantic_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    if json::nullish(a) || json::nullish(b) {
        return json::nullish(a) && json::nullish(b);
    }
    let (a, b) = (a.unwrap_or(&Value::Null), b.unwrap_or(&Value::Null));
    if let (Some(x), Some(y)) = (json::number(a), json::number(b)) {
        return num_eq(x, y);
    }
    if a.is_array() || b.is_array() {
        let (Some(x), Some(y)) = (a.as_array(), b.as_array()) else { return false };
        return x.len() == y.len()
            && x.iter().zip(y).all(|(p, q)| semantic_equal(Some(p), Some(q)));
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y))
            if json::number(a).is_none() && json::number(b).is_none() =>
        {
            union_keys(x, y).into_iter().all(|k| semantic_equal(x.get(k), y.get(k)))
        }
        _ => {
            // A canonical number against anything else, or two leaves.
            if json::number(a).is_some() || json::number(b).is_some() {
                return false;
            }
            a == b
        }
    }
}

fn shifted_by(cur: Option<&Value>, next: Option<&Value>, dx: f64, dy: f64) -> bool {
    if json::nullish(cur) || json::nullish(next) {
        return json::nullish(cur) && json::nullish(next);
    }
    let (cur, next) = (cur.unwrap_or(&Value::Null), next.unwrap_or(&Value::Null));
    if cur.is_array() || next.is_array() {
        let (Some(c), Some(n)) = (cur.as_array(), next.as_array()) else { return false };
        return c.len() == n.len()
            && c.iter().zip(n).all(|(p, q)| shifted_by(Some(p), Some(q), dx, dy));
    }
    match (cur, next) {
        (Value::Object(c), Value::Object(n))
            if json::number(cur).is_none() && json::number(next).is_none() =>
        {
            for k in union_keys(c, n) {
                let (cv, nv) = (c.get(k), n.get(k));
                match (cv.and_then(json::number), nv.and_then(json::number)) {
                    (Some(a), Some(b)) => {
                        let delta = match k.as_str() {
                            "left" | "right" | "x" => dx,
                            "bottom" | "top" | "y" => dy,
                            _ => 0.0,
                        };
                        if !num_eq(a + delta, b) {
                            return false;
                        }
                    }
                    _ => {
                        if !shifted_by(cv, nv, dx, dy) {
                            return false;
                        }
                    }
                }
            }
            true
        }
        _ => cur == next,
    }
}

fn first_point(value: Option<&Value>) -> Option<(f64, f64)> {
    let value = value?;
    if json::nullish(Some(value)) {
        return None;
    }
    if let Some(items) = value.as_array() {
        return items.first().and_then(|v| first_point(Some(v)));
    }
    let record = value.as_object()?;
    if json::number(value).is_some() {
        return None;
    }
    if let (Some(x), Some(y)) =
        (record.get("x").and_then(json::number), record.get("y").and_then(json::number))
    {
        return Some((x, y));
    }
    if let (Some(l), Some(b)) =
        (record.get("left").and_then(json::number), record.get("bottom").and_then(json::number))
    {
        return Some((l, b));
    }
    let start = record.get("start").filter(|v| !json::nullish(Some(v)));
    first_point(start.or_else(|| record.get("upperLeft")))
}

fn rigid_translation_of(
    cur: &Object,
    pat: &Object,
    subtype: &str,
    keys: &[&str],
) -> Option<(f64, f64)> {
    let anchor = keys.first()?;
    let from = first_point(cur.get(*anchor))?;
    let to = first_point(pat.get(*anchor))?;
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    if TURNING_KINDS.contains(&subtype) {
        let after = if json::undefined(pat.get("rotation")) {
            cur.get("rotation")
        } else {
            pat.get("rotation")
        };
        if !num_eq(norm_deg(cur.get("rotation")), norm_deg(after)) {
            return None;
        }
    }
    for key in keys {
        let (c, p) = (cur.get(*key), pat.get(*key));
        if json::nullish(c) && json::nullish(p) {
            continue;
        }
        if json::nullish(c) || json::nullish(p) {
            return None;
        }
        if !shifted_by(c, p, dx, dy) {
            return None;
        }
    }
    Some((dx, dy))
}

/// `appearanceChangeOf(current, patch)`.
pub fn appearance_change_of(current: &Object, patch: &Object) -> AppearanceChange {
    let subtype = json::get_str(current, "subtype").unwrap_or("");
    if let Some(given) = patch.get("subtype").filter(|v| !json::undefined(Some(v)))
        && given.as_str() != Some(subtype)
    {
        return AppearanceChange::Regenerate;
    }
    let caption_enabled = if json::undefined(patch.get("captionEnabled")) {
        current.get("captionEnabled")
    } else {
        patch.get("captionEnabled")
    };
    let contents_painted = CONTENTS_PAINTED.contains(&subtype)
        || (["line", "polygon", "polyline"].contains(&subtype)
            && caption_enabled == Some(&Value::Bool(true)));
    let inert: BTreeSet<&str> = INERT_KEYS.iter().copied().collect();
    let mut touched: Vec<&str> = Vec::new();
    for (key, value) in patch {
        if json::undefined(Some(value)) || inert.contains(key.as_str()) {
            continue;
        }
        if key == "contents" && !contents_painted {
            continue;
        }
        if !semantic_equal(Some(value), current.get(key)) {
            touched.push(key);
        }
    }
    if touched.is_empty() {
        return AppearanceChange::Inert;
    }
    let Some(base) = translatable_geometry(subtype) else { return AppearanceChange::Regenerate };
    let mut keys: Vec<&str> = base.to_vec();
    if (subtype == "polygon" || subtype == "polyline") && json::truthy(current.get("captionCenter"))
    {
        keys.push("captionCenter");
    }
    if !touched.iter().all(|k| keys.contains(k)) {
        return AppearanceChange::Regenerate;
    }
    match rigid_translation_of(current, patch, subtype, &keys) {
        Some((x, y)) => AppearanceChange::Translation { x, y },
        None => AppearanceChange::Regenerate,
    }
}
