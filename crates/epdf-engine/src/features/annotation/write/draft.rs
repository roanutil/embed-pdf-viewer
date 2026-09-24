//! What a caller may author.
//!
//! G11. Ports `core/src/annotation/draft-base.ts` and the `kinds/*/draft.ts`
//! files, one family at a time.
//!
//! A draft is NOT a DTO with holes in it. It carries only what a caller may
//! set at creation, which is why `/T`, `/M` and the `/EMBD_Metadata`
//! authorial fields are absent: those are stamped by the engine from the
//! caller's identity, and letting a draft carry them would let any caller
//! with create authority claim to be someone else.
//!
//! `nm` is the exception, and it is here rather than on the patch for the
//! opposite reason: `/NM` SEEDS an annotation's stable identity, so it can be
//! chosen once and never afterwards. A patch with an `nm` field would be a
//! stable id a caller could rename mid-session, which is not a stable id.

use serde::{Deserialize, Serialize};

use crate::dto::Patch;

use crate::features::annotation::flags::AnnotationFlags;
use crate::features::annotation::intent::InkIntent;
use crate::features::annotation::intent::StrikeoutIntent;
use crate::features::annotation::read::shape::PdfRectDifferences;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::vocabulary::LineEndings;
use crate::features::annotation::vocabulary::{AnnotationBorderStyle, BlendMode};
use crate::geom::{InkList, LinePoints, PdfPoint, PdfQuad, PdfRect};
use crate::identity::AnnotationRef;

/// The fields every draft carries.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotationDraftBase {
    /// `/Contents`. A clear is a no-op on a fresh annotation and is
    /// accepted rather than refused, which keeps a draft and a patch spelled
    /// the same.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub contents: Patch<String>,
    /// `/Subj`.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub subject: Patch<String>,
    /// The blend mode to bake into the appearance. Absent takes the
    /// subtype's default, which is NOT the same as asking for `normal`:
    /// the two reach different entry points in the fork.
    pub blend_mode: Option<BlendMode>,
    /// `/NM`, if the caller wants to choose the stable id.
    pub nm: Option<String>,
    /// `/F`. Only the keys supplied are set; a draft's dictionary starts at
    /// zero, so the rest are false.
    pub flags: Option<AnnotationFlagsPatch>,
    /// `/IRT`. The parent must be on the SAME page (§12.5.6.2).
    pub in_reply_to: Option<AnnotationRef>,
    /// `/RT`, when `in_reply_to` is set. Ignored otherwise.
    pub reply_type: Option<crate::features::annotation::vocabulary::AnnotationReplyType>,
}

/// A partial `/F`: every bit is tri-state.
///
/// `None` leaves a bit alone, which is what makes `{ hidden: true }` hide an
/// annotation without also un-printing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotationFlagsPatch {
    pub invisible: Option<bool>,
    pub hidden: Option<bool>,
    pub print: Option<bool>,
    pub no_zoom: Option<bool>,
    pub no_rotate: Option<bool>,
    pub no_view: Option<bool>,
    pub read_only: Option<bool>,
    pub locked: Option<bool>,
    pub toggle_no_view: Option<bool>,
    pub locked_contents: Option<bool>,
}

impl AnnotationFlagsPatch {
    /// Fold this patch onto a live `/F` word.
    ///
    /// Onto the CURRENT word rather than onto zero, because the flags this
    /// patch does not mention have to survive. That is also why
    /// [`AnnotationFlags::to_bits`] is not used here: it would rebuild the
    /// word from ten booleans and drop whatever reserved bit the file had.
    pub fn fold(self, current: u32) -> u32 {
        let mut bits = current;
        for (value, bit) in [
            (self.invisible, 1u32 << 0),
            (self.hidden, 1 << 1),
            (self.print, 1 << 2),
            (self.no_zoom, 1 << 3),
            (self.no_rotate, 1 << 4),
            (self.no_view, 1 << 5),
            (self.read_only, 1 << 6),
            (self.locked, 1 << 7),
            (self.toggle_no_view, 1 << 8),
            (self.locked_contents, 1 << 9),
        ] {
            match value {
                Some(true) => bits |= bit,
                Some(false) => bits &= !bit,
                None => {}
            }
        }
        bits
    }

    /// The flags this patch would produce from nothing, for a test or a
    /// caller that wants to see the word before writing it.
    pub fn to_flags(self) -> AnnotationFlags {
        AnnotationFlags::from_bits(self.fold(0))
    }
}

/// The family-specific half of a draft.
///
/// Two families so far. The rest land with their writers, and a subtype
/// without one is refused rather than created empty: an annotation of the
/// right subtype with none of its fields is worse than no annotation,
/// because it is in the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "kebab-case")]
#[serde(rename_all_fields = "camelCase")]
pub enum AnnotationDraftKind {
    Highlight {
        #[serde(flatten)]
        fields: TextMarkupDraftFields,
    },
    Underline {
        #[serde(flatten)]
        fields: TextMarkupDraftFields,
    },
    Squiggly {
        #[serde(flatten)]
        fields: TextMarkupDraftFields,
    },
    Strikeout {
        #[serde(flatten)]
        fields: TextMarkupDraftFields,
        /// `/IT`, for the subordinate half of a replace-text edit.
        intent: Option<StrikeoutIntent>,
    },
    Square {
        #[serde(flatten)]
        fields: ShapeDraftFields,
    },
    Circle {
        #[serde(flatten)]
        fields: ShapeDraftFields,
    },
    Polygon {
        #[serde(flatten)]
        fields: VertexDraftFields,
        /// `/BE`. Polygon has it and polyline does not: an open path has no
        /// interior for a cloudy border to enclose.
        #[serde(default, skip_serializing_if = "Patch::is_keep")]
        cloudy_intensity: Patch<f64>,
    },
    Polyline {
        #[serde(flatten)]
        fields: VertexDraftFields,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_endings: Option<LineEndings>,
    },
    Line {
        #[serde(flatten)]
        fields: GeometryDraftFields,
        /// `/L`. Required: it is what a line annotation IS.
        line_points: LinePoints,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_endings: Option<LineEndings>,
    },
    Ink {
        #[serde(flatten)]
        fields: StrokeDraftFields,
        /// `/InkList`, one point list per pen stroke.
        ink_list: InkList,
        /// `/IT`. `Some` only for the Acrobat freehand highlighter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<InkIntent>,
    },
}

impl AnnotationDraftKind {
    /// The wire spelling, which is also the subtype this draft creates.
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

/// What a text-markup draft carries.
///
/// No `rect`: §12.5.6.10 requires `/Rect` to be the smallest box enclosing
/// every quad, so it is DERIVED rather than accepted. A caller who could
/// supply both could supply two things that disagree.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextMarkupDraftFields {
    pub quad_points: Vec<PdfQuad>,
    pub color: Option<Color>,
    pub opacity: Option<f64>,
}

/// What a shape draft carries.
///
/// `rect` IS required here: a shape's geometry is its box, and nothing else
/// on the draft implies one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeDraftFields {
    pub rect: PdfRect,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub opacity: Option<f64>,
    #[serde(default)]
    pub stroke_width: Option<f64>,
    #[serde(default)]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(default)]
    pub dash_array: Option<Vec<f64>>,
    /// `/IC`. Absent and a clear are the same thing here: a shape whose
    /// draft says nothing about a fill has no fill.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    /// `/BE /I`.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub cloudy_intensity: Patch<f64>,
    /// `/RD`.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub rect_differences: Patch<PdfRectDifferences>,
    /// `/EMBD_Metadata /Rotation`, in degrees. A box kind's rotation is
    /// LOAD-BEARING rather than advisory: it drives the appearance bake, and
    /// it is unsatisfiable without the pre-rotation box below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// `/EMBD_Metadata /UnrotatedRect`, the logical box the rotation turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
}

/// The styling every geometric draft carries, with a fill.
///
/// The shape family adds `/BE` and `/RD` on top of this; the vertex and line
/// families do not, because their geometry is their own points rather than a
/// box with margins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryDraftFields {
    /// `/Rect`. Required on every geometric kind: the caller owns the
    /// bounding-box arithmetic, because it also owns the rotation that
    /// produced the points inside it.
    pub rect: PdfRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    /// `/EMBD_Metadata /Rotation`, ADVISORY for these families: the points
    /// are already rotated, so this records the angle that was applied and
    /// never drives the appearance generator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}

/// The same without a fill, for ink.
///
/// A pen stroke has no interior, so the field is absent rather than present
/// and always null: the type says what the family has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeDraftFields {
    pub rect: PdfRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}

/// What a polygon or polyline draft carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexDraftFields {
    #[serde(flatten)]
    pub geometry: GeometryDraftFields,
    /// `/Vertices`, in order.
    pub vertices: Vec<PdfPoint>,
}

/// One annotation to create.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationDraft {
    #[serde(flatten)]
    pub base: AnnotationDraftBase,
    #[serde(flatten)]
    pub kind: AnnotationDraftKind,
}

/// Who is creating it.
///
/// Supplied by whoever authenticated the caller, never by the caller. The
/// engine writes `/T` from the display name and `/EMBD_Metadata` from the
/// ids, which is why none of the three is on the draft.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotationActor {
    pub display_name: Option<String>,
    pub user_id: Option<String>,
    pub group_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_patch_leaves_the_bits_it_does_not_mention() {
        // The whole point of the tri-state. Hiding an annotation must not
        // also stop it printing.
        let current = AnnotationFlags { print: true, ..AnnotationFlags::default() }.to_bits();
        let patch = AnnotationFlagsPatch { hidden: Some(true), ..AnnotationFlagsPatch::default() };
        let folded = AnnotationFlags::from_bits(patch.fold(current));
        assert!(folded.print, "print survived");
        assert!(folded.hidden);
    }

    #[test]
    fn a_flag_patch_can_clear_a_bit() {
        let current =
            AnnotationFlags { print: true, hidden: true, ..AnnotationFlags::default() }.to_bits();
        let patch = AnnotationFlagsPatch { hidden: Some(false), ..AnnotationFlagsPatch::default() };
        let folded = AnnotationFlags::from_bits(patch.fold(current));
        assert!(folded.print);
        assert!(!folded.hidden);
    }

    #[test]
    fn folding_preserves_a_reserved_bit_the_file_had() {
        // Bit 11 is reserved and `AnnotationFlags::from_bits` drops it, so a
        // fold that rebuilt the word from the decoded booleans would erase
        // something the document said. This is why `fold` works on the raw
        // word.
        let current = 0x400 | (1 << 2);
        let patch = AnnotationFlagsPatch { hidden: Some(true), ..AnnotationFlagsPatch::default() };
        assert_eq!(patch.fold(current), 0x400 | (1 << 2) | (1 << 1));
    }
}
