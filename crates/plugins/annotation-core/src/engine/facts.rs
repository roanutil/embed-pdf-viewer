//! What each annotation kind declares about its fields (`kind-facts.ts`,
//! `field-names.ts`, `field-spaces.ts`), the engine's defaults
//! (`defaults.ts`) and the `/F` flags (`primitives.ts`).

use std::sync::OnceLock;

use serde_json::Value;

use super::kind_facts::{DEFAULTS_JSON, KIND_FACTS};
use crate::json::Object;

/// Who writes a field (`FieldOwner`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldOwner {
    Data,
    Attribution,
    Engine,
    Preserved,
}

/// What a field holds that is measured on the page (`MeasuredFieldSpace`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSpace {
    Length,
    Point,
    Points,
    Strokes,
    Box,
    Quads,
    LinePoints,
    CalloutLine,
    Measure,
    LinkTarget,
    Actions,
}

/// One kind's facts (`KindFacts`).
#[derive(Debug)]
pub struct KindFacts {
    /// Every field the kind declares, in declaration order, and who writes it.
    pub owners: &'static [(&'static str, FieldOwner)],
    /// The fields an update may send back as a read returned them.
    pub read_back: &'static [&'static str],
    /// The fields that hold a color.
    pub colors: &'static [&'static str],
    /// The fields that hold something measured on the page.
    pub spaces: &'static [(&'static str, FieldSpace)],
}

impl KindFacts {
    pub fn owner(&self, name: &str) -> Option<FieldOwner> {
        self.owners.iter().find(|(n, _)| *n == name).map(|(_, o)| *o)
    }

    pub fn declares(&self, name: &str) -> bool {
        self.owners.iter().any(|(n, _)| *n == name)
    }

    pub fn space(&self, name: &str) -> Option<FieldSpace> {
        self.spaces.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
    }
}

/// `kindFactsOf(subtype)`: the facts of the kind `subtype` names.
pub fn kind_facts_of(subtype: &str) -> Option<&'static KindFacts> {
    KIND_FACTS.iter().find(|(name, _)| *name == subtype).map(|(_, facts)| facts)
}

/// Every engine subtype, in the catalog order.
pub fn subtypes() -> impl Iterator<Item = &'static str> {
    KIND_FACTS.iter().map(|(name, _)| *name)
}

/// `ANNOTATION_FIELD_NAMES[subtype]`: the fields the kind declares. A name
/// no kind has declares none.
pub fn field_names(subtype: &str) -> Vec<&'static str> {
    kind_facts_of(subtype)
        .map(|facts| facts.owners.iter().map(|(n, _)| *n).collect())
        .unwrap_or_default()
}

/// `ANNOTATION_FIELD_SPACES[subtype] ?? ANNOTATION_FIELD_SPACES.unsupported`.
pub fn spaces_of_kind(subtype: &str) -> &'static [(&'static str, FieldSpace)] {
    kind_facts_of(subtype).or_else(|| kind_facts_of("unsupported")).map(|f| f.spaces).unwrap_or(&[])
}

/// `ANNOTATION_FIELD_SPACES_BY_NAME[name]`: a measured field by name, whatever its kind.
pub fn space_by_name(name: &str) -> Option<FieldSpace> {
    KIND_FACTS.iter().rev().find_map(|(_, facts)| facts.space(name))
}

/// The space of `name` on a write of `subtype` (or of any kind when the write
/// leaves its kind out), as `spacesOf` picks the table.
pub fn space_of(subtype: Option<&str>, name: &str) -> Option<FieldSpace> {
    match subtype {
        None => space_by_name(name),
        Some(subtype) => spaces_of_kind(subtype).iter().find(|(n, _)| *n == name).map(|(_, s)| *s),
    }
}

fn defaults_table() -> &'static Object {
    static TABLE: OnceLock<Object> = OnceLock::new();
    TABLE.get_or_init(|| match serde_json::from_str::<Value>(DEFAULTS_JSON) {
        Ok(Value::Object(map)) => map,
        _ => Object::new(),
    })
}

/// `annotationDefaultsOf(subtype)`: what a create of `subtype` reads back
/// for each data field it leaves out. An unmodelled kind has none.
pub fn annotation_defaults_of(subtype: &str) -> Object {
    match defaults_table().get(subtype) {
        Some(Value::Object(map)) => map.clone(),
        _ => Object::new(),
    }
}

/// The ten `/F` flags, in the primitives' order (`NO_ANNOTATION_FLAGS`).
pub const FLAG_NAMES: [&str; 10] = [
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
];

/// The fourteen standard PDF font names (`STANDARD_FONTS`).
pub const STANDARD_FONTS: [&str; 14] = [
    "courier",
    "courier-bold",
    "courier-bold-oblique",
    "courier-oblique",
    "helvetica",
    "helvetica-bold",
    "helvetica-bold-oblique",
    "helvetica-oblique",
    "times-roman",
    "times-bold",
    "times-bold-italic",
    "times-italic",
    "symbol",
    "zapf-dingbats",
];

/// The kinds whose `rect` the engine works out from their shape (`DRAWN_RECT_KINDS`).
pub const DRAWN_RECT_KINDS: [&str; 14] = [
    "highlight",
    "underline",
    "squiggly",
    "strikeout",
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

pub fn is_drawn_rect_kind(subtype: &str) -> bool {
    DRAWN_RECT_KINDS.contains(&subtype)
}

/// The kinds a write gives no appearance of the engine's (`UNBAKED_KINDS`).
pub fn is_unbaked_kind(subtype: &str) -> bool {
    subtype == "link" || subtype == "widget"
}
