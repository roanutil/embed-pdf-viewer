//! Whether an update may keep the appearance stream that is already there.
//!
//! G11. Ports `core/src/annotation/appearance.ts`.
//!
//! # An `/AP` stream is content, not cache
//!
//! A foreign producer's appearance generally cannot be recomputed: Acrobat's
//! rich free-text rendering, an image-backed stamp, a shape drawn by
//! something with its own idea of what a cloud border looks like. Destroying
//! one is acceptable only as the explicit consequence of a semantic edit.
//!
//! §12.5.5 fits a form XObject's `/BBox` into `/Rect` algorithmically, so a
//! same-size `/Rect` change translates the painted pixels verbatim.
//! Preservation on a verified rigid translation is therefore a theorem rather
//! than a policy, and the engine checks the geometry rather than believing a
//! caller who says "this is just a move".
//!
//! # Why this works over untyped values
//!
//! The classifier compares a patch against a DTO BY KEY, and it knows almost
//! nothing about either schema: five name lists and a per-kind list of
//! geometry fields. Typing it would mean enumerating every field of every
//! subtype twice and revisiting it whenever one gains a field, which is the
//! opposite of what makes it safe. The TypeScript casts to
//! `Record<string, unknown>` for the same reason, and the vectors compare the
//! verdicts rather than the types.

use serde_json::Value;

/// What an update does to `/AP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppearanceImpact {
    /// Nothing appearance-affecting changed value. Never touch `/AP`.
    Inert,
    /// A verified rigid translation: the pixels are identical up to a shift.
    Translation,
    /// A semantic edit. The owner re-bakes `/AP`.
    Regenerate,
}

/// Coordinate tolerance, in PDF points.
///
/// PDF numbers round-trip through `f32` and through a client's own space
/// conversions, so exact equality would classify a no-op write as an edit.
/// A thousandth of a point is far below anything visible and far above the
/// drift that accumulates at page magnitudes.
const EPSILON: f64 = 1e-3;

/// Keys that never affect `/AP` on any subtype.
///
/// The discriminator, the behavioural flags, the reply relationship, the
/// group attribution, and the conversation plane: `/Subj`, `/State` and
/// `/StateModel` are dictionary entries §12.5.6.3 never paints.
const INERT_KEYS: [&str; 8] =
    ["subtype", "flags", "inReplyTo", "replyType", "groupId", "subject", "state", "stateModel"];

/// The subtypes whose `/AP` paints `/Contents`.
///
/// Everywhere else `contents` is the popup note's text, and editing a comment
/// must not re-bake, let alone destroy, the shape's appearance.
const CONTENTS_PAINTED: [&str; 2] = ["free-text", "redact"];

/// The subtypes whose `rotation` is advisory.
///
/// A vertex kind's points are already rotated, so the scalar records what was
/// applied and never drives the generator. A patch touching it changes
/// nothing about the picture.
const ADVISORY_ROTATION: [&str; 4] = ["line", "polyline", "polygon", "ink"];

/// The subtypes carrying the `/EMBD_Metadata` transform pair.
const BOX_TRANSFORM: [&str; 4] = ["square", "circle", "free-text", "stamp"];

/// Per-subtype absolute-geometry fields that one rigid translation shifts
/// together.
///
/// A subtype absent from this table never takes the translation route. `rect`
/// is in every entry because a translation always moves `/Rect`.
const TRANSLATABLE_GEOMETRY: [(&str, &[&str]); 17] = [
    ("square", &["rect", "unrotatedRect"]),
    ("circle", &["rect", "unrotatedRect"]),
    ("free-text", &["rect", "unrotatedRect", "calloutLine"]),
    ("line", &["rect", "linePoints"]),
    ("polygon", &["rect", "vertices"]),
    ("polyline", &["rect", "vertices"]),
    ("ink", &["rect", "inkList"]),
    ("highlight", &["rect", "quadPoints"]),
    ("underline", &["rect", "quadPoints"]),
    ("squiggly", &["rect", "quadPoints"]),
    ("strikeout", &["rect", "quadPoints"]),
    ("redact", &["rect", "quadPoints"]),
    ("caret", &["rect"]),
    ("text", &["rect"]),
    ("stamp", &["rect", "unrotatedRect"]),
    ("file-attachment", &["rect"]),
    // A link paints nothing of its own, and it is still here: a moved link
    // rect classifies as a translation rather than as a regeneration, which
    // is what stops a move from asking for an `/AP` that does not exist.
    ("link", &["rect"]),
];

fn num_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPSILON
}

/// Degrees in `[0, 360)`. An absent rotation reads as none.
fn norm_deg(value: Option<&Value>) -> f64 {
    let degrees = value.and_then(Value::as_f64).unwrap_or(0.0);
    degrees.rem_euclid(360.0)
}

/// Semantic equality between a patch value and the current one.
///
/// `null` and absent both mean the entry is not there, so clearing something
/// that was already absent is a no-op rather than an edit. Numbers compare
/// within [`EPSILON`]; arrays and objects compare structurally.
pub fn semantic_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    let a = a.filter(|v| !v.is_null());
    let b = b.filter(|v| !v.is_null());
    match (a, b) {
        (None, None) => true,
        (None, Some(_)) | (Some(_), None) => false,
        (Some(a), Some(b)) => match (a, b) {
            (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) => num_eq(x, y),
                _ => a == b,
            },
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(x, y)| semantic_equal(Some(x), Some(y)))
            }
            (Value::Object(x), Value::Object(y)) => {
                // The union of both key sets, so a key present on one side
                // and absent on the other is compared rather than skipped.
                let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
                keys.sort_unstable();
                keys.dedup();
                keys.into_iter().all(|k| semantic_equal(x.get(k), y.get(k)))
            }
            _ => a == b,
        },
    }
}

/// Is `next` exactly `cur` shifted by `(dx, dy)`?
///
/// Walks the value structurally. A numeric leaf under an x-axis key shifts by
/// `dx`, one under a y-axis key by `dy`, and any other numeric leaf must not
/// have moved at all: a "translation" that also changed a stroke width is not
/// a translation.
fn shifted_by(cur: Option<&Value>, next: Option<&Value>, dx: f64, dy: f64) -> bool {
    let cur = cur.filter(|v| !v.is_null());
    let next = next.filter(|v| !v.is_null());
    match (cur, next) {
        (None, None) => true,
        (None, Some(_)) | (Some(_), None) => false,
        (Some(cur), Some(next)) => match (cur, next) {
            (Value::Array(c), Value::Array(n)) => {
                c.len() == n.len()
                    && c.iter().zip(n).all(|(c, n)| shifted_by(Some(c), Some(n), dx, dy))
            }
            (Value::Object(c), Value::Object(n)) => {
                let mut keys: Vec<&String> = c.keys().chain(n.keys()).collect();
                keys.sort_unstable();
                keys.dedup();
                keys.into_iter().all(|key| {
                    match (c.get(key).and_then(Value::as_f64), n.get(key).and_then(Value::as_f64)) {
                        (Some(c), Some(n)) => {
                            let delta = match key.as_str() {
                                "left" | "right" | "x" => dx,
                                "bottom" | "top" | "y" => dy,
                                _ => 0.0,
                            };
                            num_eq(c + delta, n)
                        }
                        _ => shifted_by(c.get(key), n.get(key), dx, dy),
                    }
                })
            }
            _ => cur == next,
        },
    }
}

/// Is the touched geometry one rigid translation of the current state?
///
/// Requires a `rect` in the patch, with width and height preserved, and every
/// other geometry field the annotation has must ride along by the same delta.
/// A rect move that leaves `vertices` behind is NOT a translation: the
/// dictionary would desync from the pixels.
fn is_rigid_translation(
    cur: &serde_json::Map<String, Value>,
    pat: &serde_json::Map<String, Value>,
    subtype: &str,
    geometry_keys: &[&str],
) -> bool {
    let (Some(cur_rect), Some(pat_rect)) = (cur.get("rect"), pat.get("rect")) else {
        return false;
    };
    let edge = |rect: &Value, key: &str| rect.get(key).and_then(Value::as_f64);
    let (Some(cl), Some(cb), Some(cr), Some(ct)) = (
        edge(cur_rect, "left"),
        edge(cur_rect, "bottom"),
        edge(cur_rect, "right"),
        edge(cur_rect, "top"),
    ) else {
        return false;
    };
    let (Some(pl), Some(pb), Some(pr), Some(pt)) = (
        edge(pat_rect, "left"),
        edge(pat_rect, "bottom"),
        edge(pat_rect, "right"),
        edge(pat_rect, "top"),
    ) else {
        return false;
    };

    let dx = pl - cl;
    let dy = pb - cb;
    if !num_eq(pr - pl, cr - cl) || !num_eq(pt - pb, ct - cb) {
        return false;
    }

    if BOX_TRANSFORM.contains(&subtype) {
        // Tri-state: an omitted rotation PRESERVES the current one and a
        // null clears it, so what is compared is the after-state rather than
        // the patch key.
        let rotation_after = match pat.get("rotation") {
            Some(value) => Some(value),
            None => cur.get("rotation"),
        };
        if !num_eq(norm_deg(cur.get("rotation")), norm_deg(rotation_after)) {
            return false;
        }
    }

    for key in geometry_keys {
        if *key == "rect" {
            continue;
        }
        let c = cur.get(*key).filter(|v| !v.is_null());
        let p = pat.get(*key).filter(|v| !v.is_null());
        match (c, p) {
            // Absent on both: nothing to shift.
            (None, None) => continue,
            // Geometry appearing or vanishing is not a translation.
            (None, Some(_)) | (Some(_), None) => return false,
            (Some(_), Some(_)) => {
                if !shifted_by(c, p, dx, dy) {
                    return false;
                }
            }
        }
    }
    true
}

/// Classify an update patch against the annotation's current DTO.
///
/// Three steps, in order. Value-diff away the keys that did not change and
/// the ones that never matter. If nothing is left, `Inert`. If everything
/// left is translatable geometry for this subtype AND the values really are
/// one rigid translation, `Translation`. Anything else, including an unknown
/// key or an unknown subtype, `Regenerate`: conservative by default, because
/// the cost of a wrong `Regenerate` is a re-bake and the cost of a wrong
/// `Translation` is a picture that no longer matches its dictionary.
pub fn appearance_impact_of(current: &Value, patch: &Value) -> AppearanceImpact {
    let (Some(cur), Some(pat)) = (current.as_object(), patch.as_object()) else {
        return AppearanceImpact::Regenerate;
    };
    let subtype = match (
        pat.get("subtype").and_then(Value::as_str),
        cur.get("subtype").and_then(Value::as_str),
    ) {
        (Some(p), Some(c)) if p == c => p,
        // A patch for a different subtype, or for none, is not something to
        // reason about geometrically.
        _ => return AppearanceImpact::Regenerate,
    };

    let mut touched: Vec<&str> = Vec::new();
    for (key, value) in pat {
        // A `null` here is a tri-state clear, and clearing an entry that is
        // already absent is a no-op. `semantic_equal` treats absent and null
        // alike, so that case falls out rather than needing an arm.
        if INERT_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key == "contents" && !CONTENTS_PAINTED.contains(&subtype) {
            continue;
        }
        if key == "rotation" && ADVISORY_ROTATION.contains(&subtype) {
            continue;
        }
        if !semantic_equal(Some(value), cur.get(key)) {
            touched.push(key);
        }
    }
    if touched.is_empty() {
        return AppearanceImpact::Inert;
    }

    let Some((_, geometry_keys)) = TRANSLATABLE_GEOMETRY.iter().find(|(name, _)| *name == subtype)
    else {
        return AppearanceImpact::Regenerate;
    };
    if !touched.iter().all(|key| geometry_keys.contains(key)) {
        return AppearanceImpact::Regenerate;
    }
    if is_rigid_translation(cur, pat, subtype, geometry_keys) {
        AppearanceImpact::Translation
    } else {
        AppearanceImpact::Regenerate
    }
}
