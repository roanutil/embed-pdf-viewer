//! `types.ts`: the model, messages, effects and render vocabulary, plus the
//! `@embedpdf/engine-core/runtime` DTO types the model carries.
//!
//! Every type serializes to the JSON the TypeScript object prints: optional
//! members are skipped when absent, `null` members (`ref`, `interiorColor`,
//! `hovered`) print as `null`. The TypeScript `Vec` is [`Point`] here (a
//! `Vec` would shadow Rust's), and `Rect` / `TextQuad` are the
//! `epdf_geometry` types.

use std::collections::BTreeMap;
use std::rc::Rc;

use indexmap::IndexMap;
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

pub use epdf_geometry::{Point, Rect, Size, TextQuad};

use crate::js::{IdSet, Nullable, Record};
pub use crate::measurement::{DistanceAppearance, MeasurementAppearance};
pub use crate::measurement_shape::ShapeMeasurementAppearance;
pub use crate::richtext::RichTextDocumentInput;

/// `Vec` in the TypeScript: a content-space point.
pub type Vec2 = Point;
/// Four positional content-space points (selection chrome, OBB corners).
pub type Quad = [Point; 4];
pub type Id = String;
pub type Cursor = String;
/// `PageObjectNumber` (`@embedpdf/core`): a page's PDF object number.
pub type PageObjectNumber = u32;

/// `PageRef['kind']`: `objectNumber` is the only kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageRefKind {
    #[default]
    ObjectNumber,
}

/// `PageRef` (`@embedpdf/engine-core/runtime`): a page's durable address.
/// The model keys pages by `page_object_number` and passes the address
/// itself around.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRef {
    pub kind: PageRefKind,
    pub page_object_number: PageObjectNumber,
}

impl PageRef {
    /// `toPageRef(pon)`.
    pub fn new(page_object_number: PageObjectNumber) -> Self {
        PageRef { kind: PageRefKind::ObjectNumber, page_object_number }
    }

    /// `page.pageObjectNumber`.
    pub fn pon(&self) -> PageObjectNumber {
        self.page_object_number
    }

    /// Two addresses name the same page.
    pub fn same_page(&self, other: &PageRef) -> bool {
        self.page_object_number == other.page_object_number
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

// ── engine-core runtime primitives ──────────────────────────────────────────

/// `/LE` line ending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineEnding {
    None,
    Square,
    Circle,
    Diamond,
    OpenArrow,
    ClosedArrow,
    Butt,
    ROpenArrow,
    RClosedArrow,
    Slash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineEndings {
    pub start: LineEnding,
    pub end: LineEnding,
}

/// `Partial<LineEndings>`: a per-side endings write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineEndingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<LineEnding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<LineEnding>,
}

impl LineEndingsPatch {
    /// `{ ...base, ...self }`.
    pub fn over(&self, base: LineEndings) -> LineEndings {
        LineEndings { start: self.start.unwrap_or(base.start), end: self.end.unwrap_or(base.end) }
    }

    /// `{ ...self, ...next }`.
    pub fn merge(&self, next: &LineEndingsPatch) -> LineEndingsPatch {
        LineEndingsPatch { start: next.start.or(self.start), end: next.end.or(self.end) }
    }
}

/// Effective blend mode of an annotation's normal appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// The `/F` flags, one boolean per bit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationFlags {
    pub invisible: bool,
    pub hidden: bool,
    pub print: bool,
    pub no_zoom: bool,
    pub no_rotate: bool,
    pub no_view: bool,
    pub read_only: bool,
    pub locked: bool,
    pub toggle_no_view: bool,
    pub locked_contents: bool,
}

/// `Partial<AnnotationFlags>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationFlagsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invisible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_zoom: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_rotate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_view: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle_no_view: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked_contents: Option<bool>,
}

/// `RevisionToken`: the page generation an index ref was read at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionToken {
    pub doc_session_id: String,
    pub page: PageRef,
    pub generation: f64,
}

/// `AnnotationRef`: how the engine addresses an annotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AnnotationRef {
    ObjectNumber { page: PageRef, annot_object_number: f64 },
    Nm { page: PageRef, nm: String },
    Index { page: PageRef, index: f64, revision: RevisionToken },
}

/// `PdfDestination`: an explicit in-document destination.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum PdfDestination {
    #[serde(rename = "xyz")]
    Xyz {
        page: PageRef,
        #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
        left: Nullable<f64>,
        #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
        top: Nullable<f64>,
        #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
        zoom: Nullable<f64>,
    },
    #[serde(rename = "fit")]
    Fit { page: PageRef },
    #[serde(rename = "fitH")]
    FitH {
        page: PageRef,
        #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
        top: Nullable<f64>,
    },
    #[serde(rename = "fitV")]
    FitV {
        page: PageRef,
        #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
        left: Nullable<f64>,
    },
    #[serde(rename = "fitR")]
    FitR { page: PageRef, left: f64, bottom: f64, right: f64, top: f64 },
    #[serde(rename = "fitB")]
    FitB { page: PageRef },
    #[serde(rename = "fitBH")]
    FitBH {
        page: PageRef,
        #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
        top: Nullable<f64>,
    },
    #[serde(rename = "fitBV")]
    FitBV {
        page: PageRef,
        #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
        left: Nullable<f64>,
    },
}

/// `PdfLinkTarget`: where a link points, normalized.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PdfLinkTarget {
    Goto { destination: PdfDestination },
    Uri { uri: String },
    GotoRemote { file: String },
    Launch { path: String },
    Javascript,
    Named { name: String },
    Unsupported,
}

/// `PdfPoint`: PDF user space, y-up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfPoint {
    pub x: f64,
    pub y: f64,
}

/// `PdfRect`: PDF user space as edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// `CaretIntent | StrikeoutIntent | InkIntent`: the normalized `/IT` of an
/// annotation authored before a DTO exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Intent {
    Replace,
    StrikeoutTextEdit,
    InkHighlight,
}

// ── geometry ────────────────────────────────────────────────────────────────

/// A text-edit anchor: the boundary glyph's cell plus the reading direction
/// along its baseline (`1` toward `end`, `-1` toward `start`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEndAnchor {
    pub glyph_quad: TextQuad,
    pub advance: f64,
}

/// The per-page view environment the screen-anchor projection consumes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewEnv {
    pub zoom: f64,
    pub rotation: f64,
}

/// A free-text callout's leader.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Callout {
    pub tip: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knee: Option<Point>,
    pub ending: LineEnding,
}

/// Content-space geometry, the one thing hit-testing, editing and rendering
/// work on. `rot` is kept as given: absent and `0` print differently.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Geom {
    Rect {
        rect: Rect,
        ellipse: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rot: Option<f64>,
    },
    Line {
        a: Point,
        b: Point,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends: Option<LineEndings>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rot: Option<f64>,
    },
    Poly {
        points: Vec<Point>,
        closed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends: Option<LineEndings>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rot: Option<f64>,
    },
    Quads {
        quads: Vec<TextQuad>,
    },
    Caret {
        rect: Rect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rot: Option<f64>,
    },
    Ink {
        strokes: Vec<Vec<Point>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rot: Option<f64>,
    },
    Text {
        rect: Rect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        callout: Option<Callout>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rot: Option<f64>,
    },
}

/// `Geom['t']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeomVariant {
    Rect,
    Line,
    Poly,
    Quads,
    Caret,
    Ink,
    Text,
}

impl Geom {
    pub fn variant(&self) -> GeomVariant {
        match self {
            Geom::Rect { .. } => GeomVariant::Rect,
            Geom::Line { .. } => GeomVariant::Line,
            Geom::Poly { .. } => GeomVariant::Poly,
            Geom::Quads { .. } => GeomVariant::Quads,
            Geom::Caret { .. } => GeomVariant::Caret,
            Geom::Ink { .. } => GeomVariant::Ink,
            Geom::Text { .. } => GeomVariant::Text,
        }
    }

    /// `'rect' in g`: the box of a rect, caret or text geom.
    pub fn rect(&self) -> Option<&Rect> {
        match self {
            Geom::Rect { rect, .. } | Geom::Caret { rect, .. } | Geom::Text { rect, .. } => {
                Some(rect)
            }
            _ => None,
        }
    }

    /// The `rot` member where the variant has one.
    pub fn rot(&self) -> Option<f64> {
        match self {
            Geom::Rect { rot, .. }
            | Geom::Line { rot, .. }
            | Geom::Poly { rot, .. }
            | Geom::Caret { rot, .. }
            | Geom::Ink { rot, .. }
            | Geom::Text { rot, .. } => *rot,
            Geom::Quads { .. } => None,
        }
    }

    /// A text geom with a callout.
    pub fn is_callout(&self) -> bool {
        matches!(self, Geom::Text { callout: Some(_), .. })
    }
}

/// How a shape's outline is stroked.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Border {
    #[default]
    Solid,
    Dashed {
        dash: Vec<f64>,
    },
    Cloudy {
        intensity: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    pub color: String,
    pub interior_color: Option<String>,
    pub stroke_width: f64,
    pub opacity: f64,
    pub blend_mode: BlendMode,
    pub border: Border,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_family: String,
    pub font_size: f64,
    pub font_color: String,
    pub text_align: TextAlign,
    /// The rich body's formatting (free text only). Absent is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
}

/// The one flat vocabulary for editable appearance properties. `extra`
/// holds members a patch brought that no key names: `{ ...base, ...patch }`
/// keeps them, so a resolved defaults bag prints them too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationProps {
    pub color: String,
    pub interior_color: Option<String>,
    pub stroke_width: f64,
    pub opacity: f64,
    pub blend_mode: BlendMode,
    pub border: Border,
    pub font_family: String,
    pub font_size: f64,
    pub font_color: String,
    pub text_align: TextAlign,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    pub line_endings: LineEndings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
    pub link: Nullable<PdfLinkTarget>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// `PropKey`: a key of [`AnnotationProps`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropKey {
    Color,
    InteriorColor,
    StrokeWidth,
    Opacity,
    BlendMode,
    Border,
    LineEndings,
    FontFamily,
    FontSize,
    FontColor,
    TextAlign,
    Bold,
    Italic,
    Underline,
    Icon,
    Link,
}

impl PropKey {
    pub const ALL: [PropKey; 16] = [
        PropKey::Color,
        PropKey::InteriorColor,
        PropKey::StrokeWidth,
        PropKey::Opacity,
        PropKey::BlendMode,
        PropKey::Border,
        PropKey::LineEndings,
        PropKey::FontFamily,
        PropKey::FontSize,
        PropKey::FontColor,
        PropKey::TextAlign,
        PropKey::Bold,
        PropKey::Italic,
        PropKey::Underline,
        PropKey::Icon,
        PropKey::Link,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PropKey::Color => "color",
            PropKey::InteriorColor => "interiorColor",
            PropKey::StrokeWidth => "strokeWidth",
            PropKey::Opacity => "opacity",
            PropKey::BlendMode => "blendMode",
            PropKey::Border => "border",
            PropKey::LineEndings => "lineEndings",
            PropKey::FontFamily => "fontFamily",
            PropKey::FontSize => "fontSize",
            PropKey::FontColor => "fontColor",
            PropKey::TextAlign => "textAlign",
            PropKey::Bold => "bold",
            PropKey::Italic => "italic",
            PropKey::Underline => "underline",
            PropKey::Icon => "icon",
            PropKey::Link => "link",
        }
    }

    pub fn parse(key: &str) -> Option<PropKey> {
        PropKey::ALL.into_iter().find(|k| k.as_str() == key)
    }
}

/// `AnnotationPropsPatch`: a partial property write. `interiorColor` and
/// `link` have three states (absent, `null` to clear, a value). `order` is
/// every key the patch carried, in the order it arrived (members no key
/// names included, which `extra` holds): the `props` patch effect reports
/// them in that order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationPropsPatch {
    pub color: Option<String>,
    pub interior_color: Nullable<String>,
    pub stroke_width: Option<f64>,
    pub opacity: Option<f64>,
    pub blend_mode: Option<BlendMode>,
    pub border: Option<Border>,
    pub line_endings: Option<LineEndingsPatch>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_color: Option<String>,
    pub text_align: Option<TextAlign>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub icon: Option<String>,
    pub link: Nullable<PdfLinkTarget>,
    pub extra: serde_json::Map<String, Value>,
    pub order: Vec<String>,
}

impl AnnotationPropsPatch {
    /// Whether the member `key` is present (and not undefined).
    pub fn has(&self, key: &str) -> bool {
        match PropKey::parse(key) {
            Some(PropKey::Color) => self.color.is_some(),
            Some(PropKey::InteriorColor) => !self.interior_color.is_undefined(),
            Some(PropKey::StrokeWidth) => self.stroke_width.is_some(),
            Some(PropKey::Opacity) => self.opacity.is_some(),
            Some(PropKey::BlendMode) => self.blend_mode.is_some(),
            Some(PropKey::Border) => self.border.is_some(),
            Some(PropKey::LineEndings) => self.line_endings.is_some(),
            Some(PropKey::FontFamily) => self.font_family.is_some(),
            Some(PropKey::FontSize) => self.font_size.is_some(),
            Some(PropKey::FontColor) => self.font_color.is_some(),
            Some(PropKey::TextAlign) => self.text_align.is_some(),
            Some(PropKey::Bold) => self.bold.is_some(),
            Some(PropKey::Italic) => self.italic.is_some(),
            Some(PropKey::Underline) => self.underline.is_some(),
            Some(PropKey::Icon) => self.icon.is_some(),
            Some(PropKey::Link) => !self.link.is_undefined(),
            None => self.extra.contains_key(key),
        }
    }

    /// `Object.keys(patch)`: the recorded order, then any member set in Rust
    /// without being recorded, in declaration order.
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.order.iter().filter(|k| self.has(k)).cloned().collect();
        for key in PropKey::ALL {
            if self.has(key.as_str()) && !keys.iter().any(|k| k == key.as_str()) {
                keys.push(key.as_str().to_string());
            }
        }
        for key in self.extra.keys() {
            if !keys.contains(key) {
                keys.push(key.clone());
            }
        }
        keys
    }

    /// `{ ...self, ...next }`: `next`'s members win; keys keep their first
    /// position and new ones append.
    pub fn spread(&self, next: &AnnotationPropsPatch) -> AnnotationPropsPatch {
        let mut out = self.clone();
        if next.color.is_some() {
            out.color.clone_from(&next.color);
        }
        if !next.interior_color.is_undefined() {
            out.interior_color = next.interior_color.clone();
        }
        if next.stroke_width.is_some() {
            out.stroke_width = next.stroke_width;
        }
        if next.opacity.is_some() {
            out.opacity = next.opacity;
        }
        if next.blend_mode.is_some() {
            out.blend_mode = next.blend_mode;
        }
        if next.border.is_some() {
            out.border.clone_from(&next.border);
        }
        if next.line_endings.is_some() {
            out.line_endings = next.line_endings;
        }
        if next.font_family.is_some() {
            out.font_family.clone_from(&next.font_family);
        }
        if next.font_size.is_some() {
            out.font_size = next.font_size;
        }
        if next.font_color.is_some() {
            out.font_color.clone_from(&next.font_color);
        }
        if next.text_align.is_some() {
            out.text_align = next.text_align;
        }
        if next.bold.is_some() {
            out.bold = next.bold;
        }
        if next.italic.is_some() {
            out.italic = next.italic;
        }
        if next.underline.is_some() {
            out.underline = next.underline;
        }
        if next.icon.is_some() {
            out.icon.clone_from(&next.icon);
        }
        if !next.link.is_undefined() {
            out.link = next.link.clone();
        }
        for (k, v) in &next.extra {
            out.extra.insert(k.clone(), v.clone());
        }
        let mut order = self.keys();
        for key in next.keys() {
            if !order.contains(&key) {
                order.push(key);
            }
        }
        out.order = order;
        out
    }
}

impl Serialize for AnnotationPropsPatch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for key in self.keys() {
            match PropKey::parse(&key) {
                Some(PropKey::Color) => map.serialize_entry(&key, &self.color)?,
                Some(PropKey::InteriorColor) => map.serialize_entry(&key, &self.interior_color)?,
                Some(PropKey::StrokeWidth) => map.serialize_entry(&key, &self.stroke_width)?,
                Some(PropKey::Opacity) => map.serialize_entry(&key, &self.opacity)?,
                Some(PropKey::BlendMode) => map.serialize_entry(&key, &self.blend_mode)?,
                Some(PropKey::Border) => map.serialize_entry(&key, &self.border)?,
                Some(PropKey::LineEndings) => map.serialize_entry(&key, &self.line_endings)?,
                Some(PropKey::FontFamily) => map.serialize_entry(&key, &self.font_family)?,
                Some(PropKey::FontSize) => map.serialize_entry(&key, &self.font_size)?,
                Some(PropKey::FontColor) => map.serialize_entry(&key, &self.font_color)?,
                Some(PropKey::TextAlign) => map.serialize_entry(&key, &self.text_align)?,
                Some(PropKey::Bold) => map.serialize_entry(&key, &self.bold)?,
                Some(PropKey::Italic) => map.serialize_entry(&key, &self.italic)?,
                Some(PropKey::Underline) => map.serialize_entry(&key, &self.underline)?,
                Some(PropKey::Icon) => map.serialize_entry(&key, &self.icon)?,
                Some(PropKey::Link) => map.serialize_entry(&key, &self.link)?,
                None => map.serialize_entry(&key, &self.extra[&key])?,
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for AnnotationPropsPatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PatchVisitor;
        impl<'de> Visitor<'de> for PatchVisitor {
            type Value = AnnotationPropsPatch;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an annotation props patch")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut p = AnnotationPropsPatch::default();
                while let Some(key) = map.next_key::<String>()? {
                    match PropKey::parse(&key) {
                        Some(PropKey::Color) => p.color = Some(map.next_value()?),
                        Some(PropKey::InteriorColor) => p.interior_color = map.next_value()?,
                        Some(PropKey::StrokeWidth) => p.stroke_width = Some(map.next_value()?),
                        Some(PropKey::Opacity) => p.opacity = Some(map.next_value()?),
                        Some(PropKey::BlendMode) => p.blend_mode = Some(map.next_value()?),
                        Some(PropKey::Border) => p.border = Some(map.next_value()?),
                        Some(PropKey::LineEndings) => p.line_endings = Some(map.next_value()?),
                        Some(PropKey::FontFamily) => p.font_family = Some(map.next_value()?),
                        Some(PropKey::FontSize) => p.font_size = Some(map.next_value()?),
                        Some(PropKey::FontColor) => p.font_color = Some(map.next_value()?),
                        Some(PropKey::TextAlign) => p.text_align = Some(map.next_value()?),
                        Some(PropKey::Bold) => p.bold = Some(map.next_value()?),
                        Some(PropKey::Italic) => p.italic = Some(map.next_value()?),
                        Some(PropKey::Underline) => p.underline = Some(map.next_value()?),
                        Some(PropKey::Icon) => p.icon = Some(map.next_value()?),
                        Some(PropKey::Link) => p.link = map.next_value()?,
                        None => {
                            let v: Value = map.next_value()?;
                            p.extra.insert(key.clone(), v);
                        }
                    }
                    if !p.order.contains(&key) {
                        p.order.push(key);
                    }
                }
                Ok(p)
            }
        }
        deserializer.deserialize_map(PatchVisitor)
    }
}

// ── the model ───────────────────────────────────────────────────────────────

/// Render source of a stored annotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Baked,
    Vector,
}

/// Render source of a render item: a draft or preview is a `ghost`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderSource {
    Baked,
    Vector,
    Ghost,
}

impl From<Source> for RenderSource {
    fn from(s: Source) -> Self {
        match s {
            Source::Baked => RenderSource::Baked,
            Source::Vector => RenderSource::Vector,
        }
    }
}

/// A redaction label (`/OverlayText` + `/Repeat`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub text: String,
    pub repeat: bool,
}

/// The session's authority over a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authority {
    pub update: bool,
    pub delete: bool,
}

/// A stored annotation. It serializes through the impl below, which writes
/// the members in the order the TypeScript object that built it has.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annot {
    pub id: Id,
    #[serde(rename = "ref", default)]
    pub ref_: Option<AnnotationRef>,
    /// The page this annotation lives on.
    pub page: PageRef,
    pub subtype: String,
    pub geom: Geom,
    pub style: Style,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<MeasurementAppearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub flags: AnnotationFlags,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ap_box: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ap_rot: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ap_version: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<Authority>,
    /// The engine `AnnotationDTO`, carried verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    #[serde(default, skip_serializing_if = "Nullable::is_undefined")]
    pub link: Nullable<PdfLinkTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irt: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// A member of [`Annot`], for the member orders below.
#[derive(Clone, Copy)]
enum AnnotKey {
    Id,
    Ref,
    Page,
    Subtype,
    Geom,
    Style,
    Text,
    Label,
    Measure,
    Icon,
    Flags,
    Source,
    ApBox,
    ApRot,
    ApVersion,
    Authority,
    Data,
    Intent,
    Link,
    Irt,
    Group,
}

/// `fromDTO` (`repository/index.ts`): identity, `flags`, `source` and the
/// DTO, then `irt`/`group`, `style`, the kind's slice (a dimension's
/// `measure` before its `geom`, the rest after), and `apBox`/`apRot`. The
/// session's `authority` and `apVersion` are spread on later.
const FROM_DTO_ORDER: [AnnotKey; 21] = {
    use AnnotKey::*;
    [
        Id, Ref, Page, Subtype, Flags, Source, Data, Irt, Group, Style, Measure, Geom, Text, Label,
        Icon, Link, Intent, ApBox, ApRot, Authority, ApVersion,
    ]
};

/// The reducer's create literals (`update.ts`): `geom`, `style` and the
/// kind's extras, then `flags` and `source`. A caret replacement writes
/// `intent` ahead of `geom`; ink writes it after `style` (handled in
/// [`Annot::member_order`]).
const DRAWN_ORDER: [AnnotKey; 21] = {
    use AnnotKey::*;
    [
        Id, Ref, Page, Subtype, Intent, Geom, Measure, Style, Text, Label, Icon, Link, Flags,
        Source, Irt, Group, Data, ApBox, ApRot, Authority, ApVersion,
    ]
};

/// A stored record that is neither: the base members, then what spreads
/// appended after `source`.
const STORED_ORDER: [AnnotKey; 21] = {
    use AnnotKey::*;
    [
        Id, Ref, Page, Subtype, Intent, Geom, Style, Flags, Source, Text, Label, Measure, Icon,
        Link, Irt, Group, Data, ApBox, ApRot, Authority, ApVersion,
    ]
};

impl Annot {
    /// The member order of the TypeScript object, read off what the record
    /// carries since the Rust struct keeps no order of its own: `data` with
    /// `apBox` is `fromDTO`'s, no `ref` yet is a reducer create, anything
    /// else a stored record grown by spreads.
    fn member_order(&self) -> impl Iterator<Item = AnnotKey> {
        let (order, ink) = if self.data.is_some() && self.ap_box.is_some() {
            (&FROM_DTO_ORDER, false)
        } else if self.ref_.is_none() {
            (&DRAWN_ORDER, self.subtype == "ink")
        } else {
            (&STORED_ORDER, false)
        };
        order
            .iter()
            .copied()
            .flat_map(move |key| match (ink, key) {
                (true, AnnotKey::Intent) => [None, None],
                (true, AnnotKey::Style) => [Some(AnnotKey::Style), Some(AnnotKey::Intent)],
                _ => [Some(key), None],
            })
            .flatten()
    }
}

impl Serialize for Annot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        fn put<M: SerializeMap, T: Serialize>(
            map: &mut M,
            key: &str,
            value: Option<&T>,
        ) -> Result<(), M::Error> {
            match value {
                Some(v) => map.serialize_entry(key, v),
                None => Ok(()),
            }
        }
        let mut map = serializer.serialize_map(None)?;
        for key in self.member_order() {
            let m = &mut map;
            match key {
                AnnotKey::Id => put(m, "id", Some(&self.id)),
                AnnotKey::Ref => put(m, "ref", Some(&self.ref_)),
                AnnotKey::Page => put(m, "page", Some(&self.page)),
                AnnotKey::Subtype => put(m, "subtype", Some(&self.subtype)),
                AnnotKey::Geom => put(m, "geom", Some(&self.geom)),
                AnnotKey::Style => put(m, "style", Some(&self.style)),
                AnnotKey::Text => put(m, "text", self.text.as_ref()),
                AnnotKey::Label => put(m, "label", self.label.as_ref()),
                AnnotKey::Measure => put(m, "measure", self.measure.as_ref()),
                AnnotKey::Icon => put(m, "icon", self.icon.as_ref()),
                AnnotKey::Flags => put(m, "flags", Some(&self.flags)),
                AnnotKey::Source => put(m, "source", Some(&self.source)),
                AnnotKey::ApBox => put(m, "apBox", self.ap_box.as_ref()),
                AnnotKey::ApRot => put(m, "apRot", self.ap_rot.as_ref()),
                AnnotKey::ApVersion => put(m, "apVersion", self.ap_version.as_ref()),
                AnnotKey::Authority => put(m, "authority", self.authority.as_ref()),
                AnnotKey::Data => put(m, "data", self.data.as_ref()),
                AnnotKey::Intent => put(m, "intent", self.intent.as_ref()),
                AnnotKey::Link => put(m, "link", (!self.link.is_undefined()).then_some(&self.link)),
                AnnotKey::Irt => put(m, "irt", self.irt.as_ref()),
                AnnotKey::Group => put(m, "group", self.group.as_ref()),
            }?;
        }
        map.end()
    }
}

/// A draggable handle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Handle {
    pub id: String,
    pub at: Point,
    pub cursor: Cursor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
}

/// An alignment guide.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub axis: Axis,
    pub at: f64,
    pub lo: f64,
    pub hi: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapSettings {
    pub guides: bool,
    pub guide_threshold: f64,
    pub rotation: bool,
    pub rotation_angles: Vec<f64>,
    pub rotation_threshold: f64,
}

/// `Partial<SnapSettings>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guides: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide_threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_angles: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_threshold: Option<f64>,
}

/// A click-create box anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoxAnchor {
    Center,
    TopLeft,
}

/// `ClickCreate`: a default-size box (`width`/`height`/`anchor`) or a
/// default-length segment (`length`/`angleDeg`). The TypeScript tells the arms
/// apart by `'length' in policy` in one place and `'width' in policy` in
/// others, so an object carrying both answers differently to each; the
/// members stay optional here to keep that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickCreate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<BoxAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f64>,
}

impl ClickCreate {
    pub fn boxed(width: f64, height: f64, anchor: Option<BoxAnchor>) -> Self {
        ClickCreate { width: Some(width), height: Some(height), anchor, ..Default::default() }
    }

    pub fn segment(length: f64, angle_deg: Option<f64>) -> Self {
        ClickCreate { length: Some(length), angle_deg, ..Default::default() }
    }

    /// `'length' in policy`.
    pub fn is_segment(&self) -> bool {
        self.length.is_some()
    }

    /// `'width' in policy`.
    pub fn has_width(&self) -> bool {
        self.width.is_some()
    }
}

/// `ClickCreate | false`: a tool's click policy, or `false` to suppress a
/// kind's own click fallback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClickCreateOption {
    Policy(ClickCreate),
    Off,
}

impl ClickCreateOption {
    /// `d.clickCreate && 'width' in d.clickCreate`.
    pub fn box_policy(this: Option<&Self>) -> Option<ClickCreate> {
        match this {
            Some(ClickCreateOption::Policy(p)) if p.has_width() => Some(*p),
            _ => None,
        }
    }

    /// `d.clickCreate && 'length' in d.clickCreate`.
    pub fn segment_policy(this: Option<&Self>) -> Option<ClickCreate> {
        match this {
            Some(ClickCreateOption::Policy(p)) if p.is_segment() => Some(*p),
            _ => None,
        }
    }
}

impl Serialize for ClickCreateOption {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ClickCreateOption::Policy(p) => p.serialize(serializer),
            ClickCreateOption::Off => serializer.serialize_bool(false),
        }
    }
}

impl<'de> Deserialize<'de> for ClickCreateOption {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Bool(false) => Ok(ClickCreateOption::Off),
            other => ClickCreate::deserialize(other)
                .map(ClickCreateOption::Policy)
                .map_err(de::Error::custom),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CalloutStep {
    Knee,
    Box,
}

/// The two stages of a distance draft: drag the endpoints, then place the
/// dimension line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DistanceStep {
    Endpoints,
    Offset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupOp {
    Resize,
}

/// The in-progress gesture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "g", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Draft {
    CreateRect {
        subtype: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        page: PageRef,
        from: Point,
        to: Point,
        ellipse: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_rotation: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upright: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        click_create: Option<ClickCreateOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    CreateDistance {
        step: DistanceStep,
        subtype: String,
        preset: String,
        page: PageRef,
        from: Point,
        to: Point,
        measure: DistanceAppearance,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    CreateLine {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        measure: Option<MeasurementAppearance>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capture: Option<String>,
        subtype: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        page: PageRef,
        from: Point,
        to: Point,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        click_create: Option<ClickCreateOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    CreatePoly {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        measure: Option<ShapeMeasurementAppearance>,
        subtype: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        page: PageRef,
        points: Vec<Point>,
        cur: Point,
        closed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    CreateInk {
        subtype: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        page: PageRef,
        strokes: Vec<Vec<Point>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<Intent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    CreateCallout {
        subtype: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        page: PageRef,
        step: CalloutStep,
        tip: Point,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knee: Option<Point>,
        cur: Point,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        box_from: Option<Point>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        box_to: Option<Point>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page_box: Option<Rect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_rotation: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upright: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    Move {
        ids: Vec<Id>,
        start: Point,
        delta: Point,
        guides: Vec<Guide>,
    },
    Handle {
        id: Id,
        handle: String,
        base: Geom,
        cur: Geom,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<ViewEnv>,
    },
    Rotate {
        ids: Vec<Id>,
        pivot: Point,
        start: Point,
        cur: Point,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        free: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<ViewEnv>,
    },
    Group {
        op: GroupOp,
        ids: Vec<Id>,
        handle: String,
        anchor: Point,
        base: Rect,
        cur: Rect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<ViewEnv>,
    },
    Caption {
        id: Id,
        start: Point,
        delta: Point,
    },
    Leader {
        id: Id,
        start: Point,
        delta: f64,
    },
    Marquee {
        page: PageRef,
        from: Point,
        to: Point,
    },
}

/// The fields of a rotate draft, for [`crate::update::rotate_draft_delta`].
#[derive(Clone, Debug, PartialEq)]
pub struct RotateDraft<'a> {
    pub ids: &'a [Id],
    pub pivot: Point,
    pub start: Point,
    pub cur: Point,
    pub free: Option<bool>,
    pub view: Option<ViewEnv>,
}

impl Draft {
    /// The rotate draft's fields, when this is one.
    pub fn as_rotate(&self) -> Option<RotateDraft<'_>> {
        match self {
            Draft::Rotate { ids, pivot, start, cur, free, view } => Some(RotateDraft {
                ids,
                pivot: *pivot,
                start: *start,
                cur: *cur,
                free: *free,
                view: *view,
            }),
            _ => None,
        }
    }

    /// `draft.g.startsWith('create-')`: a creation draft, not an edit
    /// gesture. No wildcard arm, so a new variant has to pick a side.
    pub fn is_create(&self) -> bool {
        match self {
            Draft::CreateRect { .. }
            | Draft::CreateDistance { .. }
            | Draft::CreateLine { .. }
            | Draft::CreatePoly { .. }
            | Draft::CreateInk { .. }
            | Draft::CreateCallout { .. } => true,
            Draft::Move { .. }
            | Draft::Handle { .. }
            | Draft::Rotate { .. }
            | Draft::Group { .. }
            | Draft::Caption { .. }
            | Draft::Leader { .. }
            | Draft::Marquee { .. } => false,
        }
    }

    /// `'page' in draft`: the creation drafts and the marquee.
    pub fn page(&self) -> Option<PageRef> {
        match self {
            Draft::CreateRect { page, .. }
            | Draft::CreateDistance { page, .. }
            | Draft::CreateLine { page, .. }
            | Draft::CreatePoly { page, .. }
            | Draft::CreateInk { page, .. }
            | Draft::CreateCallout { page, .. }
            | Draft::Marquee { page, .. } => Some(*page),
            _ => None,
        }
    }

    /// `'id' in draft ? draft.id : undefined`: the single-annotation edits.
    pub fn id(&self) -> Option<&Id> {
        match self {
            Draft::Handle { id, .. } | Draft::Caption { id, .. } | Draft::Leader { id, .. } => {
                Some(id)
            }
            _ => None,
        }
    }

    /// `'ids' in draft ? draft.ids : undefined`.
    pub fn ids(&self) -> Option<&[Id]> {
        match self {
            Draft::Move { ids, .. } | Draft::Rotate { ids, .. } | Draft::Group { ids, .. } => {
                Some(ids)
            }
            _ => None,
        }
    }
}

/// A live text-markup preview, per page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkupPreview {
    pub subtype: String,
    pub preset: String,
    pub by_page: BTreeMap<String, Vec<TextQuad>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolySubtype {
    Polygon,
    Polyline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CreationDraftKind {
    Poly,
}

/// Anchor and affordances for UI that controls an in-progress creation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationDraftAnchor {
    pub kind: CreationDraftKind,
    pub subtype: PolySubtype,
    pub page: PageRef,
    pub bounds: Rect,
    pub point_count: usize,
    pub min_points: usize,
    pub can_finish: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    /// Shared so a step that leaves the annotations alone (every draft
    /// pointermove) clones a pointer rather than every annotation's JSON,
    /// as the TypeScript's spread shares the object.
    pub by_id: Rc<BTreeMap<Id, Annot>>,
    pub order: Rc<Vec<Id>>,
    pub selected: Vec<Id>,
    pub hovered: Option<Id>,
    /// Shared, so a step that keeps the draft keeps the same one: a new
    /// `Rc` is the TypeScript's new draft object, which is what
    /// `draftChanged` announces.
    pub draft: Option<Rc<Draft>>,
    pub preview: Option<MarkupPreview>,
    pub seq: f64,
    pub style: Style,
    /// Insertion-ordered, as the TypeScript `{ ...m.defaults, [subtype]: next }`
    /// keeps a configured tool where it was first set.
    pub defaults: IndexMap<String, AnnotationPropsPatch>,
    pub hit_margin: f64,
    pub editing: Option<Id>,
    pub snap: SnapSettings,
}

/// Selection-chrome geometry in content units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeGeom {
    pub handle_tol: f64,
    pub knob_tol: f64,
    pub knob_offset: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerInput {
    /// The page the sample resolved against.
    pub page: PageRef,
    pub point: Point,
    pub shift: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_box: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrome: Option<ChromeGeom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upright: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inert: Option<IdSet>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Down,
    Move,
    Up,
}

/// Pure straight-line recognition settings for a freehand stroke.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkStraightenOptions {
    pub deviation_threshold: f64,
    pub axis_snap_degrees: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Msg {
    EditPointer {
        phase: Phase,
        #[serde(rename = "in")]
        input: PointerInput,
    },
    MarqueePointer {
        phase: Phase,
        #[serde(rename = "in")]
        input: PointerInput,
    },
    CreatePointer {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        measure: Option<MeasurementAppearance>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capture: Option<String>,
        phase: Phase,
        subtype: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<Intent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        click_create: Option<ClickCreateOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defer_ink_commit: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        straighten_ink: Option<InkStraightenOptions>,
        #[serde(rename = "in")]
        input: PointerInput,
    },
    FinishInkDraft,
    FinishCreationDraft,
    /// Programmatic creation from page-space geometry: the data API's
    /// `create`.
    CreateAnnot {
        page: PageRef,
        subtype: String,
        geom: Geom,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Option<AnnotationPropsPatch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        select: Option<bool>,
    },
    CreateCaret {
        page: PageRef,
        anchor: TextEndAnchor,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    CreateReplaceText {
        page: PageRef,
        quads: Vec<TextQuad>,
        anchor: TextEndAnchor,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
    },
    CreateMarkup {
        subtype: String,
        page: PageRef,
        quads: Vec<TextQuad>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<AnnotationFlagsPatch>,
    },
    SetMarkupPreview {
        subtype: String,
        quads_by_page: Record<Vec<TextQuad>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
    },
    ClearMarkupPreview,
    Deselect {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ids: Option<Vec<Id>>,
    },
    Hover {
        id: Option<Id>,
    },
    Select {
        ids: Vec<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        add: Option<bool>,
    },
    SetProps {
        patch: AnnotationPropsPatch,
    },
    SetFlags {
        patch: AnnotationFlagsPatch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ids: Option<Vec<Id>>,
    },
    SetDefaults {
        subtype: String,
        patch: AnnotationPropsPatch,
    },
    SetSnap {
        patch: SnapSettingsPatch,
    },
    #[serde(rename = "rotate90")]
    Rotate90,
    ResetRotation,
    Delete,
    Cancel,
    Loaded {
        annots: Vec<Annot>,
    },
    Hydrated {
        annots: Vec<Annot>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bump_ap: Option<bool>,
    },
    Created {
        temp_id: Id,
        id: Id,
        #[serde(rename = "ref")]
        ref_: AnnotationRef,
    },
    CreateFailed {
        temp_id: Id,
    },
    Upsert {
        annots: Vec<Annot>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bump_ap: Option<bool>,
    },
    BumpAp {
        ids: Vec<Id>,
    },
    Remove {
        ids: Vec<Id>,
    },
    BeginTextEdit {
        id: Id,
    },
    SetText {
        id: Id,
        text: String,
    },
    SetRichText {
        id: Id,
        doc: RichTextDocumentInput,
    },
    EndTextEdit,
}

/// What a committed edit changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PatchScope {
    Geometry,
    Caption,
    Leader,
    /// The user's patch keys, verbatim (a key no [`PropKey`] names included).
    Props {
        keys: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "fx", rename_all = "camelCase")]
pub enum Effect {
    Captured {
        tool: String,
        page: PageRef,
        geom: Geom,
    },
    Create {
        id: Id,
    },
    CreateGroup {
        primary: Id,
        members: Vec<Id>,
    },
    Patch {
        id: Id,
        scope: PatchScope,
        #[serde(rename = "apChanged", default, skip_serializing_if = "is_false")]
        ap_changed: bool,
    },
    Flags {
        id: Id,
    },
    SyncLink {
        id: Id,
        target: Option<PdfLinkTarget>,
    },
    Delete {
        #[serde(rename = "ref")]
        ref_: AnnotationRef,
    },
}

/// Per-annotation render data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderItem {
    pub id: Id,
    #[serde(rename = "ref", default)]
    pub ref_: Option<AnnotationRef>,
    pub subtype: String,
    pub geom: Geom,
    #[serde(rename = "box")]
    pub box_: Rect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ap_box: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ap_rot: Option<f64>,
    pub style: Style,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextStyle>,
    pub source: RenderSource,
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hovered: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<MeasurementAppearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rot: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<BlendMode>,
}

/// The geometry-only draw vocabulary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RenderNode {
    Rect { rect: Rect },
    Ellipse { rect: Rect },
    Line { a: Point, b: Point },
    Poly { points: Vec<Point>, closed: bool },
    Path { d: String },
}

/// `'round'`: the one stroke cap and join `Paint` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Round {
    Round,
}

/// How to paint one node; absent members mean none.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<BlendMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<Round>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<Round>,
}

/// A fully painted draw node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum SceneNode {
    Rect {
        rect: Rect,
        paint: Paint,
    },
    Ellipse {
        rect: Rect,
        paint: Paint,
    },
    Line {
        a: Point,
        b: Point,
        paint: Paint,
    },
    Poly {
        points: Vec<Point>,
        closed: bool,
        paint: Paint,
    },
    Path {
        d: String,
        paint: Paint,
    },
    Text {
        at: Point,
        text: String,
        font_size: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_family: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rotation: Option<f64>,
        paint: Paint,
    },
}

impl SceneNode {
    /// A geometry node with its paint.
    pub fn painted(node: RenderNode, paint: Paint) -> SceneNode {
        match node {
            RenderNode::Rect { rect } => SceneNode::Rect { rect, paint },
            RenderNode::Ellipse { rect } => SceneNode::Ellipse { rect, paint },
            RenderNode::Line { a, b } => SceneNode::Line { a, b, paint },
            RenderNode::Poly { points, closed } => SceneNode::Poly { points, closed, paint },
            RenderNode::Path { d } => SceneNode::Path { d, paint },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuideRole {
    Axis,
    Indicator,
}

/// One finished rotation-guide segment.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuideLine {
    pub a: Point,
    pub b: Point,
    pub role: GuideRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ChromeNode {
    Outline {
        rect: Rect,
    },
    Obb {
        corners: Quad,
        angle: f64,
    },
    Handle {
        at: Point,
        cursor: Cursor,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rot: Option<f64>,
    },
    RotateKnob {
        at: Point,
        from: Point,
    },
    Guide {
        axis: Axis,
        at: f64,
        lo: f64,
        hi: f64,
    },
    AngleChip {
        at: Point,
        angle: f64,
    },
    RotateGuides {
        center: Point,
        angle: f64,
        lines: Vec<GuideLine>,
    },
    Marquee {
        rect: Rect,
    },
}
