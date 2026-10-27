//! Records several plugins' exports carry: how the engine addresses an
//! annotation, an explicit destination, and where a link points.
//!
//! The plugins hold these as the engine's JSON records
//! (`packages/engine/core/src/identity/AnnotationRef.ts`,
//! `dto/PdfDestination.ts`, `dto/PdfLinkTarget.ts`), so each record here
//! reads and writes that JSON.
//!
//! A destination's coordinates may be absent, `null` or a number. Absent and
//! `null` both mean "keep the current value" to every consumer, so both
//! arrive as `None`, and `None` goes back as absent.

use epdf_annotation_core as core;
use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::PageRef;

fn invalid_state(message: impl Into<String>) -> Error {
    Error::named("InvalidState", message.into()).with("code", "invalid-state")
}

/// A whole number from JSON that fits a `u32`, or `invalid-state`.
fn whole(value: Option<&Value>, what: &str) -> Result<u32, Error> {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX))
        .map(|n| n as u32)
        .ok_or_else(|| invalid_state(format!("{what} is not a whole number")))
}

impl PageRef {
    /// `{ kind: 'objectNumber', objectNumber }`.
    pub fn to_json(self) -> Value {
        json!({ "kind": "objectNumber", "objectNumber": self.page_object_number })
    }

    pub fn from_json(value: &Value) -> Result<PageRef, Error> {
        if value.get("kind").and_then(Value::as_str) != Some("objectNumber") {
            return Err(invalid_state("a page ref of a kind the engine does not write"));
        }
        Ok(PageRef {
            page_object_number: whole(value.get("objectNumber"), "a page's object number")?,
        })
    }
}

/// The core's `PageRef` holds a JavaScript number: one that is not a
/// whole `u32` is `invalid-state`, as [`PageRef::from_json`] refuses it.
impl TryFrom<core::PageRef> for PageRef {
    type Error = Error;

    fn try_from(p: core::PageRef) -> Result<Self, Error> {
        let n = Value::from(p.object_number);
        Ok(PageRef { page_object_number: whole(Some(&n), "a page's object number")? })
    }
}

/// `AnnotationRef`: an annotation by its object number, or by its index
/// among the uploaded base PDF's annotations on its page (`baseIndex`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AnnotationRef {
    ObjectNumber { page: PageRef, annot_object_number: u32 },
    Index { page: PageRef, index: u32 },
}

impl AnnotationRef {
    pub fn page(&self) -> PageRef {
        match self {
            AnnotationRef::ObjectNumber { page, .. } | AnnotationRef::Index { page, .. } => *page,
        }
    }

    /// The engine's record: `{ kind: 'objectNumber', page, objectNumber }`
    /// or `{ kind: 'baseIndex', page, baseIndex }`.
    pub fn to_json(&self) -> Value {
        match self {
            AnnotationRef::ObjectNumber { page, annot_object_number } => json!({
                "kind": "objectNumber", "page": page.to_json(), "objectNumber": annot_object_number
            }),
            AnnotationRef::Index { page, index } => json!({
                "kind": "baseIndex", "page": page.to_json(), "baseIndex": index
            }),
        }
    }

    /// Read the engine's record. A number that isn't whole, or a kind the
    /// engine does not write, is `invalid-state`.
    pub fn from_json(value: &Value) -> Result<AnnotationRef, Error> {
        let page = PageRef::from_json(value.get("page").unwrap_or(&Value::Null))?;
        match value.get("kind").and_then(Value::as_str) {
            Some("objectNumber") => Ok(AnnotationRef::ObjectNumber {
                page,
                annot_object_number: whole(
                    value.get("objectNumber"),
                    "an annotation's object number",
                )?,
            }),
            Some("baseIndex") => Ok(AnnotationRef::Index {
                page,
                index: whole(value.get("baseIndex"), "an annotation's base index")?,
            }),
            _ => Err(invalid_state("an annotation ref of a kind the engine does not write")),
        }
    }
}

/// `PdfDestination`: an explicit destination as the file writes it (the
/// engine's readers' shape): a page, a location in the file's coordinates
/// and a magnification.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PdfDestination {
    Xyz { page: PageRef, left: Option<f64>, top: Option<f64>, zoom: Option<f64> },
    Fit { page: PageRef },
    FitH { page: PageRef, top: Option<f64> },
    FitV { page: PageRef, left: Option<f64> },
    FitR { page: PageRef, left: f64, bottom: f64, right: f64, top: f64 },
    FitB { page: PageRef },
    FitBH { page: PageRef, top: Option<f64> },
    FitBV { page: PageRef, left: Option<f64> },
}

/// A coordinate: absent and `null` both read as `None`.
fn coordinate(value: &Value, key: &str) -> Result<Option<f64>, Error> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_f64()),
        Some(_) => Err(invalid_state(format!("a destination's {key} is not a number"))),
    }
}

fn required(value: &Value, key: &str) -> Result<f64, Error> {
    coordinate(value, key)?.ok_or_else(|| invalid_state(format!("a destination has no {key}")))
}

impl PdfDestination {
    pub fn page(&self) -> PageRef {
        match self {
            PdfDestination::Xyz { page, .. }
            | PdfDestination::Fit { page }
            | PdfDestination::FitH { page, .. }
            | PdfDestination::FitV { page, .. }
            | PdfDestination::FitR { page, .. }
            | PdfDestination::FitB { page }
            | PdfDestination::FitBH { page, .. }
            | PdfDestination::FitBV { page, .. } => *page,
        }
    }

    /// The engine's record; a `None` coordinate is left out.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        let mut put = |key: &str, value: Option<f64>| {
            if let Some(value) = value {
                out.insert(key.into(), json!(value));
            }
        };
        let kind = match self {
            PdfDestination::Xyz { left, top, zoom, .. } => {
                put("left", *left);
                put("top", *top);
                put("zoom", *zoom);
                "xyz"
            }
            PdfDestination::Fit { .. } => "fit",
            PdfDestination::FitH { top, .. } => {
                put("top", *top);
                "fitH"
            }
            PdfDestination::FitV { left, .. } => {
                put("left", *left);
                "fitV"
            }
            PdfDestination::FitR { left, bottom, right, top, .. } => {
                put("left", Some(*left));
                put("bottom", Some(*bottom));
                put("right", Some(*right));
                put("top", Some(*top));
                "fitR"
            }
            PdfDestination::FitB { .. } => "fitB",
            PdfDestination::FitBH { top, .. } => {
                put("top", *top);
                "fitBH"
            }
            PdfDestination::FitBV { left, .. } => {
                put("left", *left);
                "fitBV"
            }
        };
        let mut record = Map::new();
        record.insert("kind".into(), json!(kind));
        record.insert("page".into(), self.page().to_json());
        record.extend(out);
        Value::Object(record)
    }

    pub fn from_json(value: &Value) -> Result<PdfDestination, Error> {
        let page = PageRef::from_json(value.get("page").unwrap_or(&Value::Null))?;
        Ok(match value.get("kind").and_then(Value::as_str) {
            Some("xyz") => PdfDestination::Xyz {
                page,
                left: coordinate(value, "left")?,
                top: coordinate(value, "top")?,
                zoom: coordinate(value, "zoom")?,
            },
            Some("fit") => PdfDestination::Fit { page },
            Some("fitH") => PdfDestination::FitH { page, top: coordinate(value, "top")? },
            Some("fitV") => PdfDestination::FitV { page, left: coordinate(value, "left")? },
            Some("fitR") => PdfDestination::FitR {
                page,
                left: required(value, "left")?,
                bottom: required(value, "bottom")?,
                right: required(value, "right")?,
                top: required(value, "top")?,
            },
            Some("fitB") => PdfDestination::FitB { page },
            Some("fitBH") => PdfDestination::FitBH { page, top: coordinate(value, "top")? },
            Some("fitBV") => PdfDestination::FitBV { page, left: coordinate(value, "left")? },
            _ => return Err(invalid_state("a destination of a kind the engine does not write")),
        })
    }
}

/// `PdfLinkTarget<PdfDestination>`: where a link points, normalized, as the
/// engine's readers write it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PdfLinkTarget {
    Goto { destination: PdfDestination },
    Uri { uri: String },
    GotoRemote { file: String },
    Launch { path: String },
    Javascript,
    Named { name: String },
    Unsupported,
}

fn text(value: &Value, key: &str) -> Result<String, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| invalid_state(format!("a link target has no {key}")))
}

impl PdfLinkTarget {
    pub fn to_json(&self) -> Value {
        match self {
            PdfLinkTarget::Goto { destination } => {
                json!({ "kind": "goto", "destination": destination.to_json() })
            }
            PdfLinkTarget::Uri { uri } => json!({ "kind": "uri", "uri": uri }),
            PdfLinkTarget::GotoRemote { file } => json!({ "kind": "goto-remote", "file": file }),
            PdfLinkTarget::Launch { path } => json!({ "kind": "launch", "path": path }),
            PdfLinkTarget::Javascript => json!({ "kind": "javascript" }),
            PdfLinkTarget::Named { name } => json!({ "kind": "named", "name": name }),
            PdfLinkTarget::Unsupported => json!({ "kind": "unsupported" }),
        }
    }

    pub fn from_json(value: &Value) -> Result<PdfLinkTarget, Error> {
        Ok(match value.get("kind").and_then(Value::as_str) {
            Some("goto") => PdfLinkTarget::Goto {
                destination: PdfDestination::from_json(
                    value.get("destination").unwrap_or(&Value::Null),
                )?,
            },
            Some("uri") => PdfLinkTarget::Uri { uri: text(value, "uri")? },
            Some("goto-remote") => PdfLinkTarget::GotoRemote { file: text(value, "file")? },
            Some("launch") => PdfLinkTarget::Launch { path: text(value, "path")? },
            Some("javascript") => PdfLinkTarget::Javascript,
            Some("named") => PdfLinkTarget::Named { name: text(value, "name")? },
            Some("unsupported") => PdfLinkTarget::Unsupported,
            _ => return Err(invalid_state("a link target of a kind the engine does not write")),
        })
    }
}

/// `PageDestination` (`dto/PdfDestination.ts:49-57`): a destination in page
/// space, the page it goes to and where on it, from the top-left of the
/// page's visible box, y down. The engine hands these out: a link's or an
/// outline item's `goto` goes to one. `None` keeps the viewer's current
/// value, as absent and `null` do.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PageDestination {
    Xyz { page: PageRef, x: Option<f64>, y: Option<f64>, zoom: Option<f64> },
    Fit { page: PageRef },
    FitH { page: PageRef, y: Option<f64> },
    FitV { page: PageRef, x: Option<f64> },
    FitR { page: PageRef, x: f64, y: f64, width: f64, height: f64 },
    FitB { page: PageRef },
    FitBH { page: PageRef, y: Option<f64> },
    FitBV { page: PageRef, x: Option<f64> },
}

impl PageDestination {
    pub fn page(&self) -> PageRef {
        match self {
            PageDestination::Xyz { page, .. }
            | PageDestination::Fit { page }
            | PageDestination::FitH { page, .. }
            | PageDestination::FitV { page, .. }
            | PageDestination::FitR { page, .. }
            | PageDestination::FitB { page }
            | PageDestination::FitBH { page, .. }
            | PageDestination::FitBV { page, .. } => *page,
        }
    }

    /// The engine's record; a `None` coordinate is left out.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        let mut members: Vec<(&str, Option<f64>)> = Vec::new();
        let kind = match self {
            PageDestination::Xyz { x, y, zoom, .. } => {
                members.extend([("x", *x), ("y", *y), ("zoom", *zoom)]);
                "xyz"
            }
            PageDestination::Fit { .. } => "fit",
            PageDestination::FitH { y, .. } => {
                members.push(("y", *y));
                "fitH"
            }
            PageDestination::FitV { x, .. } => {
                members.push(("x", *x));
                "fitV"
            }
            PageDestination::FitR { x, y, width, height, .. } => {
                members.extend([
                    ("x", Some(*x)),
                    ("y", Some(*y)),
                    ("width", Some(*width)),
                    ("height", Some(*height)),
                ]);
                "fitR"
            }
            PageDestination::FitB { .. } => "fitB",
            PageDestination::FitBH { y, .. } => {
                members.push(("y", *y));
                "fitBH"
            }
            PageDestination::FitBV { x, .. } => {
                members.push(("x", *x));
                "fitBV"
            }
        };
        record.insert("kind".into(), json!(kind));
        record.insert("page".into(), self.page().to_json());
        for (key, value) in members {
            if let Some(value) = value {
                record.insert(key.into(), json!(value));
            }
        }
        Value::Object(record)
    }

    pub fn from_json(value: &Value) -> Result<PageDestination, Error> {
        let page = PageRef::from_json(value.get("page").unwrap_or(&Value::Null))?;
        Ok(match value.get("kind").and_then(Value::as_str) {
            Some("xyz") => PageDestination::Xyz {
                page,
                x: coordinate(value, "x")?,
                y: coordinate(value, "y")?,
                zoom: coordinate(value, "zoom")?,
            },
            Some("fit") => PageDestination::Fit { page },
            Some("fitH") => PageDestination::FitH { page, y: coordinate(value, "y")? },
            Some("fitV") => PageDestination::FitV { page, x: coordinate(value, "x")? },
            Some("fitR") => PageDestination::FitR {
                page,
                x: required(value, "x")?,
                y: required(value, "y")?,
                width: required(value, "width")?,
                height: required(value, "height")?,
            },
            Some("fitB") => PageDestination::FitB { page },
            Some("fitBH") => PageDestination::FitBH { page, y: coordinate(value, "y")? },
            Some("fitBV") => PageDestination::FitBV { page, x: coordinate(value, "x")? },
            _ => return Err(invalid_state("a destination of a kind the engine does not write")),
        })
    }
}

/// `PdfLinkTarget` (`dto/PdfLinkTarget.ts:28`, its default
/// `PageDestination`): where a link or an outline item points, as the
/// plugins carry it. [`PdfLinkTarget`] is the readers' file-space form.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PageLinkTarget {
    Goto { destination: PageDestination },
    Uri { uri: String },
    GotoRemote { file: String },
    Launch { path: String },
    Javascript,
    Named { name: String },
    Unsupported,
}

impl PageLinkTarget {
    pub fn to_json(&self) -> Value {
        match self {
            PageLinkTarget::Goto { destination } => {
                json!({ "kind": "goto", "destination": destination.to_json() })
            }
            PageLinkTarget::Uri { uri } => json!({ "kind": "uri", "uri": uri }),
            PageLinkTarget::GotoRemote { file } => json!({ "kind": "goto-remote", "file": file }),
            PageLinkTarget::Launch { path } => json!({ "kind": "launch", "path": path }),
            PageLinkTarget::Javascript => json!({ "kind": "javascript" }),
            PageLinkTarget::Named { name } => json!({ "kind": "named", "name": name }),
            PageLinkTarget::Unsupported => json!({ "kind": "unsupported" }),
        }
    }

    pub fn from_json(value: &Value) -> Result<PageLinkTarget, Error> {
        Ok(match value.get("kind").and_then(Value::as_str) {
            Some("goto") => PageLinkTarget::Goto {
                destination: PageDestination::from_json(
                    value.get("destination").unwrap_or(&Value::Null),
                )?,
            },
            Some("uri") => PageLinkTarget::Uri { uri: text(value, "uri")? },
            Some("goto-remote") => PageLinkTarget::GotoRemote { file: text(value, "file")? },
            Some("launch") => PageLinkTarget::Launch { path: text(value, "path")? },
            Some("javascript") => PageLinkTarget::Javascript,
            Some("named") => PageLinkTarget::Named { name: text(value, "name")? },
            Some("unsupported") => PageLinkTarget::Unsupported,
            _ => return Err(invalid_state("a link target of a kind the engine does not write")),
        })
    }
}

/// An in-progress polygon or polyline: what a "Finish" button needs
/// (`CreationDraftAnchor`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CreationDraftAnchor {
    /// `true` for a polygon, `false` for a polyline.
    pub closed: bool,
    pub page: PageRef,
    pub bounds: crate::geometry::Rect,
    pub point_count: u32,
    pub min_points: u32,
    pub can_finish: bool,
}

impl TryFrom<core::CreationDraftAnchor> for CreationDraftAnchor {
    type Error = Error;

    /// A count that is not a whole `u32` (negative, fractional, past
    /// `u32::MAX`, `NaN`) is refused as `invalid-state`, as a page number
    /// is: the TypeScript hands the number on as it is, and a `u32` can't
    /// hold it.
    fn try_from(a: core::CreationDraftAnchor) -> Result<Self, Error> {
        let count = |n: f64, what: &str| whole(Some(&Value::from(n)), what);
        Ok(CreationDraftAnchor {
            closed: a.subtype == "polygon",
            page: a.page.try_into()?,
            bounds: a.bounds.into(),
            point_count: count(a.point_count, "a draft's point count")?,
            min_points: count(a.min_points, "a draft's minimum point count")?,
            can_finish: a.can_finish,
        })
    }
}
