//! `repository/projection.ts`: one declaration per kind family, from which
//! every wire statement shape derives (see [`super`]).
//!
//! The TypeScript `KindProjection` is an object with optional members; here
//! it is [`Kind`], whose methods answer what each family's object does, and
//! `None` where the member is absent.

use epdf_annotation_core::js::Nullable;
use epdf_annotation_core::{
    Annot, Geom, Intent, Label, PdfLinkTarget, PdfRect, PropKey, TextStyle, geom_rotation,
};
use serde_json::{Map, Value};

use super::kinds;
use super::seam::box_geom_fields;

/// An untyped partial wire statement.
pub type Wire = Map<String, Value>;

/// The kind-specific slice a DTO ingest contributes on top of the generic
/// base `fromDTO` builds for every kind.
#[derive(Clone, Debug)]
pub struct IngestSlice {
    pub geom: Geom,
    pub text: Option<TextStyle>,
    pub icon: Option<String>,
    pub label: Option<Label>,
    pub link: Nullable<PdfLinkTarget>,
    pub intent: Option<Intent>,
}

impl IngestSlice {
    pub fn geom(geom: Geom) -> Self {
        IngestSlice {
            geom,
            text: None,
            icon: None,
            label: None,
            link: Nullable::Undefined,
            intent: None,
        }
    }
}

/// The wire kind families (`KINDS` in `repository/index.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Square,
    Circle,
    Line,
    Polygon,
    Polyline,
    Ink,
    FreeText,
    Highlight,
    Underline,
    Squiggly,
    Strikeout,
    Caret,
    Redact,
    Text,
    FileAttachment,
    Stamp,
    Link,
    Widget,
    Unsupported,
}

impl Kind {
    /// `projectionOf(subtype)`: the widget client kinds project through the
    /// widget family; anything unknown is unsupported.
    pub fn of(subtype: &str) -> Kind {
        if subtype.starts_with("widget") {
            return Kind::Widget;
        }
        match subtype {
            "square" => Kind::Square,
            "circle" => Kind::Circle,
            "line" => Kind::Line,
            "polygon" => Kind::Polygon,
            "polyline" => Kind::Polyline,
            "ink" => Kind::Ink,
            "free-text" => Kind::FreeText,
            "highlight" => Kind::Highlight,
            "underline" => Kind::Underline,
            "squiggly" => Kind::Squiggly,
            "strikeout" => Kind::Strikeout,
            "caret" => Kind::Caret,
            "redact" => Kind::Redact,
            "text" => Kind::Text,
            "file-attachment" => Kind::FileAttachment,
            "stamp" => Kind::Stamp,
            "link" => Kind::Link,
            _ => Kind::Unsupported,
        }
    }

    /// `ingest(dto, crop)`.
    pub fn ingest(self, dto: &Value, crop: PdfRect) -> IngestSlice {
        match self {
            Kind::Square => kinds::shape::ingest(dto, crop, false),
            Kind::Circle => kinds::shape::ingest(dto, crop, true),
            Kind::Line => kinds::stroke::line_ingest(dto, crop),
            Kind::Polygon => kinds::stroke::poly_ingest(dto, crop, true),
            Kind::Polyline => kinds::stroke::poly_ingest(dto, crop, false),
            Kind::Ink => kinds::stroke::ink_ingest(dto, crop),
            Kind::FreeText => kinds::free_text::ingest(dto, crop),
            Kind::Highlight | Kind::Underline | Kind::Squiggly | Kind::Strikeout => {
                kinds::quads::markup_ingest(dto, crop, self == Kind::Strikeout)
            }
            Kind::Caret => kinds::quads::caret_ingest(dto, crop),
            Kind::Redact => kinds::quads::redact_ingest(dto, crop),
            Kind::Text | Kind::FileAttachment => kinds::misc::icon_ingest(dto, crop),
            Kind::Stamp => kinds::misc::stamp_ingest(dto, crop),
            Kind::Link => kinds::misc::link_ingest(dto, crop),
            Kind::Widget => kinds::misc::widget_ingest(dto, crop),
            Kind::Unsupported => kinds::misc::unsupported_ingest(dto, crop),
        }
    }

    /// `geometry(a, crop)`: the committed-geometry wire group, or `None`
    /// for a kind without editable geometry.
    pub fn geometry(self, a: &Annot, crop: PdfRect) -> Option<Wire> {
        match self {
            Kind::Square | Kind::Circle | Kind::Stamp => Some(box_emit(a, crop)),
            Kind::Line => kinds::stroke::line_geometry(a, crop),
            Kind::Polygon | Kind::Polyline => kinds::stroke::poly_geometry(a, crop),
            Kind::Ink => kinds::stroke::ink_geometry(a, crop),
            Kind::FreeText => kinds::free_text::geometry(a, crop),
            Kind::Highlight | Kind::Underline | Kind::Squiggly | Kind::Strikeout => None,
            Kind::Caret => kinds::quads::caret_geometry(a, crop),
            Kind::Redact => kinds::quads::redact_geometry(a, crop),
            Kind::Text | Kind::FileAttachment | Kind::Link | Kind::Widget => {
                kinds::misc::rect_geometry(a, crop)
            }
            Kind::Unsupported => None,
        }
    }

    /// `prop?.[key]`: the kind's own lowering for a key, when it has one.
    pub fn prop(self, key: PropKey, a: &Annot, crop: PdfRect) -> Option<Wire> {
        match self {
            Kind::Square | Kind::Circle => kinds::shape::prop(key, a),
            Kind::Line | Kind::Polygon | Kind::Polyline | Kind::Ink => {
                kinds::stroke::prop(key, a, crop)
            }
            Kind::Link => kinds::misc::link_prop(key, a),
            _ => None,
        }
    }

    /// `draftExtras?.(a, crop)`: `None` when the kind has no such member,
    /// `Some(None)` when it answers `null`.
    pub fn draft_extras(self, a: &Annot, crop: PdfRect) -> Option<Option<Wire>> {
        match self {
            Kind::Ink => Some(Some(kinds::stroke::ink_draft_extras(a))),
            Kind::FreeText => Some(Some(kinds::free_text::draft_extras(a, crop))),
            Kind::Highlight | Kind::Underline | Kind::Squiggly | Kind::Strikeout => {
                Some(kinds::quads::markup_draft_extras(a, crop, self == Kind::Strikeout))
            }
            Kind::Caret => Some(Some(kinds::quads::caret_draft_extras(a))),
            Kind::Redact => Some(Some(kinds::quads::redact_draft_extras(a, crop))),
            Kind::Link => Some(Some(kinds::misc::link_draft_extras(a))),
            _ => None,
        }
    }

    /// `createable !== false`.
    pub fn createable(self) -> bool {
        !matches!(
            self,
            Kind::Text
                | Kind::FileAttachment
                | Kind::Stamp
                | Kind::Widget
                | Kind::Unsupported
        )
    }
}

/// `boxEmit(a, crop)`: a box kind's unrotated `rect` plus its applied tilt.
pub fn box_emit(a: &Annot, crop: PdfRect) -> Wire {
    let rect = a.geom.rect().copied().unwrap_or(epdf_annotation_core::Rect {
        x: f64::NAN,
        y: f64::NAN,
        width: f64::NAN,
        height: f64::NAN,
    });
    box_geom_fields(rect, geom_rotation(&a.geom), crop)
}
