//! What a caller may change.
//!
//! G11. Ports `core/src/annotation/patch-base.ts` and the `kinds/*/patch.ts`
//! files for the families whose writers exist.
//!
//! # Every field is tri-state and the three states are not two
//!
//! Absent leaves the entry alone. `null` removes it. A value sets it. That
//! is why every such field is a [`Patch<T>`] rather than an
//! `Option<Option<T>>`: serde decodes a present `null` into the OUTER `None`,
//! so the nested option collapses the two states back together and "I did not
//! mention the subject" becomes "remove the subject". The first draft of this
//! module used the nested option and the vector caught it.
//!
//! Every field is also omitted from the wire when it is absent, and that is
//! not tidiness. [`super::appearance::appearance_impact_of`] reads the
//! SERIALIZED patch by key, so a field emitted as `null` is a key it has to
//! consider, and an update that touched nothing would be classified as an
//! edit of everything. The vector caught that one too.
//!
//! # Authorial identity is not patchable
//!
//! `/T`, `/EMBD_Metadata /UserID` and `/CreatedBy` are bound at creation. The
//! reason is not tidiness: Acrobat and every other viewer display `/T` as the
//! author, so a patchable `/T` would let anyone with update authority rewrite
//! who appears to have written an annotation. `/GroupID` IS patchable,
//! because organizational ownership legitimately changes.
//!
//! There is no `nm`. A stable id a caller can rename mid-session is not a
//! stable id, so the type makes it unsayable rather than the writer refusing
//! it.

use serde::{Deserialize, Serialize};

use crate::dto::Patch;

use super::draft::{AnnotationFlagsPatch, LinkTargetDraft, ResourceRef, StampFitDraft};
use crate::features::annotation::intent::{
    CaretIntent, FreeTextIntent, InkIntent, StrikeoutIntent,
};
use crate::features::annotation::read::shape::PdfRectDifferences;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationReplyType, BlendMode, FileAttachmentIcon, LineEnding,
    LineEndings, NoteIcon, StandardFont, TextAlignment,
};
use crate::geom::{CalloutLine, InkList, LinePoints, PdfPoint, PdfQuad, PdfRect};
use crate::identity::AnnotationRef;

/// The fields every patch carries.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotationPatchBase {
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub contents: Patch<String>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub subject: Patch<String>,
    /// The blend mode to bake. Absent PRESERVES the current effective one,
    /// which is why the updater reads it before re-baking: it lives in the
    /// appearance stream's graphics state rather than in the dictionary, so
    /// a re-bake would otherwise reset it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend_mode: Option<BlendMode>,
    /// Reassign `/EMBD_Metadata /GroupID`. No clear form: group attribution
    /// is either present or absent, and the only mutation is a reassignment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<AnnotationFlagsPatch>,
    /// Re-target or clear `/IRT`.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub in_reply_to: Patch<AnnotationRef>,
    /// Change `/RT` without re-linking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_type: Option<AnnotationReplyType>,
}

/// The family-specific half.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "kebab-case")]
#[serde(rename_all_fields = "camelCase")]
pub enum AnnotationPatchKind {
    Highlight {
        #[serde(flatten)]
        fields: TextMarkupPatchFields,
    },
    Underline {
        #[serde(flatten)]
        fields: TextMarkupPatchFields,
    },
    Squiggly {
        #[serde(flatten)]
        fields: TextMarkupPatchFields,
    },
    Strikeout {
        #[serde(flatten)]
        fields: TextMarkupPatchFields,
        intent: Option<StrikeoutIntent>,
    },
    Square {
        #[serde(flatten)]
        fields: ShapePatchFields,
    },
    Circle {
        #[serde(flatten)]
        fields: ShapePatchFields,
    },
    Polygon {
        #[serde(flatten)]
        fields: VertexPatchFields,
        #[serde(default, skip_serializing_if = "Patch::is_keep")]
        cloudy_intensity: Patch<f64>,
    },
    Polyline {
        #[serde(flatten)]
        fields: VertexPatchFields,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_endings: Option<LineEndings>,
    },
    Line {
        #[serde(flatten)]
        fields: GeometryPatchFields,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_points: Option<LinePoints>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_endings: Option<LineEndings>,
    },
    Ink {
        #[serde(flatten)]
        fields: StrokePatchFields,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ink_list: Option<InkList>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<InkIntent>,
    },
    Text {
        #[serde(flatten)]
        fields: TextPatchFields,
    },
    Caret {
        #[serde(flatten)]
        fields: CaretPatchFields,
    },
    FreeText {
        #[serde(flatten)]
        fields: FreeTextPatchFields,
    },
    Redact {
        #[serde(flatten)]
        fields: RedactPatchFields,
    },
    Link {
        #[serde(flatten)]
        fields: LinkPatchFields,
    },
    FileAttachment {
        #[serde(flatten)]
        fields: FileAttachmentPatchFields,
    },
    Stamp {
        #[serde(flatten)]
        fields: StampPatchFields,
    },
    Widget {
        #[serde(flatten)]
        fields: WidgetPatchFields,
    },
}

/// What a sticky-note patch may change.
///
/// `state` and `state_model` are tri-state INDEPENDENTLY, and unlike a draft
/// there is no rule tying them. A draft is a whole annotation, so a state
/// without a model is a state nothing can interpret; a patch touches what it
/// states, and the other half may already be on the annotation.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<NoteIcon>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub state: Patch<String>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub state_model: Patch<String>,
}

/// What a caret patch may change.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CaretPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<CaretIntent>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rect_differences: Patch<PdfRectDifferences>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rotation: Patch<f64>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub unrotated_rect: Patch<PdfRect>,
}

/// What a free-text patch may change.
///
/// `/DA` packs the font, the size and the colour into ONE string, so a patch
/// naming any of the three has to read the other two rather than defaulting
/// them. There is one caveat the read cannot fix: a registered font reads
/// back as a font CODE, so preserving one across a size change means
/// restating `font_family`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FreeTextPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<FreeTextIntent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlignment>,
    /// The `/DA` colour: the border and the default text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// The `TextColor` override. Setting one is supported; clearing it back
    /// to "follow the `/DA` colour" is not, so there is no tri-state here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    /// `/C`, the box background.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rect_differences: Patch<PdfRectDifferences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout_line: Option<CalloutLine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_ending: Option<LineEnding>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rotation: Patch<f64>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub unrotated_rect: Patch<PdfRect>,
    /// A rich replacement of the text. With `contents` too, `contents` must
    /// be its plain projection. Boxed so it does not size every other
    /// `AnnotationPatchKind` variant to fit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rich_text: Option<Box<crate::features::annotation::rich_text::RichTextDocumentInput>>,
}

/// What a redaction patch may change.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RedactPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    /// New quads. On the same terms as text markup: PDFium can grow the list
    /// and not shrink it, so a shorter one is refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quad_points: Option<Vec<PdfQuad>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    /// `/OverlayText`. `Clear` removes the label, which also removes the
    /// reason the `/DA` beside it existed.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub overlay_text: Patch<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlignment>,
}

/// What a link patch may change.
///
/// `target` is tri-state and the three states are three different links.
/// Absent leaves it alone, a value REPLACES the `/A`, and `Clear` removes
/// both `/A` and `/Dest`, which is a dead link. The clear touches both
/// because the model treats the target as one concept with two spellings:
/// removing only `/A` would resurrect a stale direct `/Dest` as the live
/// target.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub target: Patch<LinkTargetDraft>,
}

/// What a file-attachment patch may change.
///
/// The FILE is not on this list, and that is deliberate. Bytes enter once at
/// creation; replacing them is a different attachment, which is a delete and
/// a create rather than an edit.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileAttachmentPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<FileAttachmentIcon>,
}

/// What a stamp patch may change.
///
/// Supplying a `source` replaces the picture, which means dropping the
/// existing appearance objects and authoring new ones. Without one, a patch
/// is geometry and the existing appearance is re-fitted into it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StampPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ResourceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit: Option<StampFitDraft>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rotation: Patch<f64>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub unrotated_rect: Patch<PdfRect>,
}

/// What a widget patch may change.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WidgetPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub color: Patch<Color>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<StandardFont>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlignment>,
}

impl WidgetPatchFields {
    /// Whether this patch says anything about the border.
    pub fn touches_border(&self) -> bool {
        self.stroke_width.is_some() || self.border_style.is_some()
    }

    /// Whether it says anything about the text.
    pub fn touches_text_style(&self) -> bool {
        self.font_family.is_some() || self.font_size.is_some() || self.font_color.is_some()
    }
}

/// The styling every geometric patch may change, with a fill.
///
/// `rect` is here rather than derived. A vertex kind's box is the caller's
/// arithmetic over its points, the same as on a draft: a patch that moved the
/// points and left the box would be describing two different annotations.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeometryPatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    /// The advisory scalar. `Clear`, and a zero, both remove the key.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rotation: Patch<f64>,
}

/// The same without a fill, for ink.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StrokePatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rotation: Patch<f64>,
}

/// What a polygon or polyline patch may change.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VertexPatchFields {
    #[serde(flatten)]
    pub geometry: GeometryPatchFields,
    /// A replacement `/Vertices`. Unlike `/QuadPoints`, this one CAN shrink:
    /// `EPDFAnnot_SetVertices` replaces the array rather than appending to
    /// it, so a shorter path is an ordinary edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertices: Option<Vec<PdfPoint>>,
}

impl AnnotationPatchKind {
    pub fn subtype(&self) -> crate::features::annotation::subtype::AnnotationSubtype {
        use crate::features::annotation::subtype::AnnotationSubtype as S;
        match self {
            Self::Highlight { .. } => S::Highlight,
            Self::Underline { .. } => S::Underline,
            Self::Squiggly { .. } => S::Squiggly,
            Self::Strikeout { .. } => S::Strikeout,
            Self::Square { .. } => S::Square,
            Self::Circle { .. } => S::Circle,
            Self::Polygon { .. } => S::Polygon,
            Self::Polyline { .. } => S::Polyline,
            Self::Line { .. } => S::Line,
            Self::Ink { .. } => S::Ink,
            Self::Text { .. } => S::Text,
            Self::Caret { .. } => S::Caret,
            Self::FreeText { .. } => S::FreeText,
            Self::Redact { .. } => S::Redact,
            Self::Link { .. } => S::Link,
            Self::FileAttachment { .. } => S::FileAttachment,
            Self::Stamp { .. } => S::Stamp,
            Self::Widget { .. } => S::Widget,
        }
    }
}

/// What a text-markup patch may change.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextMarkupPatchFields {
    /// New quads. `/Rect` is recomputed from them, so the two cannot drift.
    ///
    /// PDFium can GROW `/QuadPoints` and cannot shrink it, so a patch with
    /// fewer quads than the annotation has is refused rather than leaving
    /// stale ones behind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quad_points: Option<Vec<PdfQuad>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
}

/// What a shape patch may change.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShapePatchFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<PdfRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub cloudy_intensity: Patch<f64>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub rect_differences: Patch<PdfRectDifferences>,
}

/// One annotation to change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationPatch {
    #[serde(flatten)]
    pub base: AnnotationPatchBase,
    #[serde(flatten)]
    pub kind: AnnotationPatchKind,
}
