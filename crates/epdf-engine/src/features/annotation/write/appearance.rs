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
//! The classifier compares a patch against a read BY KEY, and it knows almost
//! nothing about either schema: a few name lists and a per-kind list of
//! geometry fields. The TypeScript casts to `Record<string, unknown>` for the
//! same reason, and the vectors compare the verdicts rather than the types.
//! It reads positions by their axis names only (`x`, `left`, `right`; `y`,
//! `bottom`, `top`), so it classifies the file's values and page-space
//! values alike.

use serde_json::{Map, Value};

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

/// An update's appearance impact, with the distance a translation moves the
/// drawing by, in the coordinates of the values classified
/// (`AppearanceChange`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppearanceChange {
    Inert,
    Translation { x: f64, y: f64 },
    Regenerate,
}

impl AppearanceChange {
    pub fn impact(self) -> AppearanceImpact {
        match self {
            Self::Inert => AppearanceImpact::Inert,
            Self::Translation { .. } => AppearanceImpact::Translation,
            Self::Regenerate => AppearanceImpact::Regenerate,
        }
    }
}

/// Coordinate tolerance, in PDF points.
///
/// PDF numbers round-trip through `f32` and through a client's own space
/// conversions, so exact equality would classify a no-op write as an edit.
/// A thousandth of a point is far below anything visible and far above the
/// drift that accumulates at page magnitudes.
const EPSILON: f64 = 1e-3;

/// Keys that never affect `/AP` on any kind (`INERT_KEYS`): the
/// discriminator, the `/F` flags, the name, relationships and grouping, the
/// conversation plane (`/Subj`, `/State` and `/StateModel`, which §12.5.6.3
/// never paints), and the fields a write accepts but never applies.
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
    // Whether a comment window shows open: the window's state, never painted.
    "open",
    "groupId",
    "subject",
    "state",
    "stateModel",
    "ref",
    "page",
    "hasAppearance",
    // Which state shows: picked among the pictures the annotation has.
    "appearanceState",
    "author",
    "createdAt",
    "modifiedAt",
    "userId",
    "createdBy",
    "modifiedBy",
    "importedBy",
    "actions",
    // A file attachment's icon is drawn from `/Name` and `/C`, never from
    // its file.
    "file",
    // The field a widget belongs to, as the form reads it.
    "field",
    "fieldFamily",
];

/// The kinds whose `/AP` paints `/Contents`. Everywhere else `contents` is
/// the popup note's text, and editing a comment must not re-bake, let alone
/// destroy, the shape's appearance.
const CONTENTS_PAINTED: &[&str] = &["free-text", "redact"];

/// The kinds that turn by `rotation`: a box kind about the middle of its
/// `box`, a point kind about the middle of its points' box. Tri-state on
/// writes: an omitted rotation is kept, `null` (or `0`) clears it, so a
/// translation compares the after-state, not the patch key.
const TURNING_KINDS: &[&str] = &[
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

/// Per-kind absolute-geometry fields that one rigid translation shifts
/// together (`TRANSLATABLE_GEOMETRY`). A kind absent from this table never
/// takes the translation route. The first is where a move shows: the field
/// the caller gives the shape in (a `box`, the points, the quads), or `rect`
/// where that is the shape.
const TRANSLATABLE_GEOMETRY: &[(&str, &[&str])] = &[
    ("square", &["box"]),
    ("circle", &["box"]),
    ("free-text", &["box", "calloutLine"]),
    ("line", &["linePoints"]),
    ("polygon", &["vertices"]),
    ("polyline", &["vertices"]),
    ("ink", &["inkList"]),
    ("highlight", &["quadPoints"]),
    ("underline", &["quadPoints"]),
    ("squiggly", &["quadPoints"]),
    ("strikeout", &["quadPoints"]),
    ("redact", &["rect", "quadPoints"]),
    ("caret", &["box"]),
    ("text", &["rect"]),
    ("stamp", &["box"]),
    ("file-attachment", &["rect"]),
    ("link", &["rect"]),
    ("widget", &["box"]),
];

fn num_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPSILON
}

/// Degrees in `[0, 360)`. An absent rotation reads as none.
fn norm_deg(value: Option<&Value>) -> f64 {
    let degrees = value.and_then(Value::as_f64).unwrap_or(0.0);
    ((degrees % 360.0) + 360.0) % 360.0
}

/// Semantic equality between a patch value and the current one
/// (`semanticEqual`).
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

/// Is `next` exactly `cur` shifted by `(dx, dy)` (`shiftedBy`)?
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
            (Value::Array(_), _) | (_, Value::Array(_)) => false,
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

/// The first point a geometry value holds (`firstPoint`): a rect's
/// bottom-left, a point, a line's start, a quad's first corner, the first of
/// a list.
fn first_point(value: Option<&Value>) -> Option<(f64, f64)> {
    match value? {
        Value::Array(items) => first_point(items.first()),
        Value::Object(record) => {
            let number = |key: &str| record.get(key).and_then(Value::as_f64);
            if let (Some(x), Some(y)) = (number("x"), number("y")) {
                return Some((x, y));
            }
            if let (Some(left), Some(bottom)) = (number("left"), number("bottom")) {
                return Some((left, bottom));
            }
            first_point(record.get("start").or_else(|| record.get("upperLeft")))
        }
        _ => None,
    }
}

/// The distance the touched geometry moves by, when it is one rigid
/// translation of the current state; `None` when it isn't
/// (`rigidTranslationOf`). The kind's first geometry field must be in the
/// patch; it and every other geometry field present on the annotation must
/// ride along shifted by the same delta. For the kinds that turn, the turn
/// is checked as an after-state: an omitted rotation keeps the current one.
fn rigid_translation_of(
    cur: &Map<String, Value>,
    pat: &Map<String, Value>,
    subtype: &str,
    geometry_keys: &[&str],
) -> Option<(f64, f64)> {
    let anchor = geometry_keys.first()?;
    let (fx, fy) = first_point(cur.get(*anchor))?;
    let (tx, ty) = first_point(pat.get(*anchor))?;
    let (dx, dy) = (tx - fx, ty - fy);

    if TURNING_KINDS.contains(&subtype) {
        let after = match pat.get("rotation") {
            Some(value) => Some(value),
            None => cur.get("rotation"),
        };
        if !num_eq(norm_deg(cur.get("rotation")), norm_deg(after)) {
            return None;
        }
    }

    for key in geometry_keys {
        let c = cur.get(*key).filter(|v| !v.is_null());
        let p = pat.get(*key).filter(|v| !v.is_null());
        match (c, p) {
            // Absent on both: nothing to shift.
            (None, None) => continue,
            // Geometry appearing or vanishing is not a translation.
            (None, Some(_)) | (Some(_), None) => return None,
            (Some(_), Some(_)) => {
                if !shifted_by(c, p, dx, dy) {
                    return None;
                }
            }
        }
    }
    Some((dx, dy))
}

/// Classify an update patch against the annotation's current read, in
/// either space, with the distance a translation moves the drawing by
/// (`appearanceChangeOf`).
///
/// 1. Value-diff away the keys that did not change and the ones that never
///    matter (`contents` where it isn't painted). Nothing left: `Inert`.
/// 2. If every key left is translatable geometry for this kind and the
///    values are one rigid translation: `Translation`, by how far.
/// 3. Anything else, an unknown key or an unknown kind included:
///    `Regenerate`.
pub fn appearance_change_of(current: &Value, patch: &Value) -> AppearanceChange {
    let (Some(cur), Some(pat)) = (current.as_object(), patch.as_object()) else {
        return AppearanceChange::Regenerate;
    };
    let subtype = cur.get("subtype").and_then(Value::as_str).unwrap_or_default();
    if let Some(named) = pat.get("subtype").filter(|v| !v.is_null())
        && named.as_str() != Some(subtype)
    {
        return AppearanceChange::Regenerate;
    }
    let caption_enabled = match pat.get("captionEnabled") {
        Some(value) => Some(value),
        None => cur.get("captionEnabled"),
    };
    let contents_painted = CONTENTS_PAINTED.contains(&subtype)
        || (["line", "polygon", "polyline"].contains(&subtype)
            && caption_enabled == Some(&Value::Bool(true)));

    let mut touched: Vec<&str> = Vec::new();
    for (key, value) in pat {
        if INERT_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key == "contents" && !contents_painted {
            continue;
        }
        if !semantic_equal(Some(value), cur.get(key)) {
            touched.push(key);
        }
    }
    if touched.is_empty() {
        return AppearanceChange::Inert;
    }

    let Some((_, base_keys)) = TRANSLATABLE_GEOMETRY.iter().find(|(name, _)| *name == subtype)
    else {
        return AppearanceChange::Regenerate;
    };
    // A manual shape caption is page-space geometry and must ride with a
    // rigid move. Automatic centres follow the vertices without a patch.
    let mut geometry_keys: Vec<&str> = base_keys.to_vec();
    if (subtype == "polygon" || subtype == "polyline")
        && cur.get("captionCenter").is_some_and(|c| !c.is_null())
    {
        geometry_keys.push("captionCenter");
    }
    if !touched.iter().all(|key| geometry_keys.contains(key)) {
        return AppearanceChange::Regenerate;
    }
    match rigid_translation_of(cur, pat, subtype, &geometry_keys) {
        Some((x, y)) => AppearanceChange::Translation { x, y },
        None => AppearanceChange::Regenerate,
    }
}

/// [`appearance_change_of`]'s verdict alone, on the file's values: what the
/// engine's update decides (`appearanceImpactOf`).
pub fn appearance_impact_of(current: &Value, patch: &Value) -> AppearanceImpact {
    appearance_change_of(current, patch).impact()
}
