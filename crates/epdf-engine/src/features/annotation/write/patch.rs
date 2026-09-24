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

use super::draft::AnnotationFlagsPatch;
use crate::features::annotation::intent::{InkIntent, StrikeoutIntent};
use crate::features::annotation::read::shape::PdfRectDifferences;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationReplyType, BlendMode, LineEndings,
};
use crate::geom::{InkList, LinePoints, PdfPoint, PdfQuad, PdfRect};
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
