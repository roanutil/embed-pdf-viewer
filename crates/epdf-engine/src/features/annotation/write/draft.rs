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
use crate::features::actions::destination::PdfDestination;

use crate::features::annotation::flags::AnnotationFlags;
use crate::features::annotation::intent::CaretIntent;
use crate::features::annotation::intent::FreeTextIntent;
use crate::features::annotation::intent::InkIntent;
use crate::features::annotation::intent::StrikeoutIntent;
use crate::features::annotation::read::shape::PdfRectDifferences;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::vocabulary::{AnnotationBorderStyle, BlendMode};
use crate::features::annotation::vocabulary::{
    FileAttachmentIcon, LineEnding, LineEndings, NoteIcon, StandardFont, TextAlignment,
};
use crate::geom::{CalloutLine, InkList, LinePoints, PdfPoint, PdfQuad, PdfRect};
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
    Text {
        #[serde(flatten)]
        fields: TextDraftFields,
    },
    Caret {
        #[serde(flatten)]
        fields: CaretDraftFields,
    },
    FreeText {
        #[serde(flatten)]
        fields: FreeTextDraftFields,
    },
    Redact {
        #[serde(flatten)]
        fields: RedactDraftFields,
    },
    FileAttachment {
        #[serde(flatten)]
        fields: FileAttachmentDraftFields,
    },
    Link {
        #[serde(flatten)]
        fields: LinkDraftFields,
    },
    Stamp {
        #[serde(flatten)]
        fields: StampDraftFields,
    },
    Widget {
        #[serde(flatten)]
        fields: WidgetDraftFields,
    },
}

/// What a widget draft carries.
///
/// Creates an INERT widget: placed and styled, and not yet a form control.
/// Binding it to a field is `forms`' job, and it is what bakes the
/// family-correct appearance, because the look of a checkbox comes from the
/// FIELD rather than from the widget.
///
/// The vocabulary is the sibling kinds' (`color` strokes, `interior_color`
/// fills) over the widget's own dictionaries: `/MK` for the colours, `/BS`
/// for the border, `/DA` for the text defaults, `/Q` for the alignment. A
/// widget's colours are not `/C` and `/IC`, so a writer reusing the ordinary
/// colour path produces a widget whose border no reader reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetDraftFields {
    pub rect: PdfRect,
    #[serde(flatten)]
    pub style: WidgetStyleFields,
}

/// The widget-plane styling, without the placement.
///
/// Split out because `forms.createField` places a widget and styles it in
/// one call, and the styling half is the same vocabulary. One type means a
/// widget born through `annotations.create` and one born beside a field
/// cannot drift, which is the whole reason the TypeScript shares it too.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WidgetStyleFields {
    /// `/MK /BC`. `Clear` writes no border colour, which is not the same as
    /// a black one.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub color: Patch<Color>,
    /// `/MK /BG`. `Clear` is a transparent background.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub interior_color: Patch<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    /// `/DA` font. One of the fourteen standard names only: a widget's text
    /// is drawn by the form machinery, which has no registered-font path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<StandardFont>,
    /// `/DA` size. Zero means auto-size, as it does on a redaction label and
    /// unlike on a free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlignment>,
}

impl WidgetStyleFields {
    /// Whether this draft says anything about the border.
    ///
    /// `/BS` is one call taking a style AND a width, so naming either means
    /// supplying both; saying nothing means leaving the entry off entirely.
    pub fn touches_border(&self) -> bool {
        self.stroke_width.is_some() || self.border_style.is_some()
    }

    /// Whether it says anything about the text.
    ///
    /// `/DA` packs the font, the size and the colour into one string, on the
    /// same terms.
    pub fn touches_text_style(&self) -> bool {
        self.font_family.is_some() || self.font_size.is_some() || self.font_color.is_some()
    }
}

/// What a stamp draft carries.
///
/// The only family whose CONTENT is a binary. Everything else describes a
/// shape the appearance generator draws; a stamp arrives with the picture
/// already made, and the writer's job is to put it in the box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampDraftFields {
    pub rect: PdfRect,
    /// The payload key for what the stamp displays: PNG, JPEG, or a
    /// single-page PDF. The format is SNIFFED, never taken from a declared
    /// mime type: handing JPEG bytes to the PNG decoder builds a corrupt
    /// image object rather than failing.
    pub source: ResourceRef,
    /// `/Name`, the advisory label. The PDF's own spelling: a stamp's label
    /// is already a name, so there is no wire vocabulary to translate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// How the content scales into the box. `contain` preserves the aspect
    /// ratio and stays whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<StampFitDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
}

/// A reference to a binary that travelled beside the draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    pub resource: String,
}

/// The CSS `object-fit` vocabulary the wire uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StampFitDraft {
    /// Whole content, letterboxed.
    #[default]
    Contain,
    /// Whole box, cropped.
    Cover,
    /// Whole box, distorted. Spelled `fill` on the wire and `Stretch` in the
    /// fork's enum, which are the same policy under two names.
    Fill,
}

impl StampFitDraft {
    pub fn native(self) -> pdfium::StampFit {
        match self {
            Self::Contain => pdfium::StampFit::Contain,
            Self::Cover => pdfium::StampFit::Cover,
            Self::Fill => pdfium::StampFit::Stretch,
        }
    }
}

/// Where a link a writer creates may point.
///
/// A STRICT subset of what the reader reports, and the gap is the point.
/// `goto-remote` and `launch` are executable-shaped actions a viewer never
/// needs to author, and putting them out of reach of the write surface is
/// cheaper than auditing every path that could reach one. `javascript`
/// carries no payload to write, `named` is a viewer verb this engine does not
/// author, and `unsupported` is by definition nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[serde(rename_all_fields = "camelCase")]
pub enum LinkTargetDraft {
    Goto { destination: PdfDestination },
    Uri { uri: String },
}

/// What a link draft carries.
///
/// A link IS its hit rectangle, so `rect` is required and the target is not.
/// `target: null` is the create-then-edit flow: the rectangle is dragged
/// first and the destination chosen afterwards, so a link with nowhere to go
/// is a legitimate intermediate state rather than a mistake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkDraftFields {
    pub rect: PdfRect,
    pub target: Option<LinkTargetDraft>,
}

/// The embedded file a file-attachment draft names.
///
/// The bytes are NOT here. A draft is JSON and a file is not, so the draft
/// carries a key and the bytes travel beside it as a payload on the write
/// context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFileDraft {
    /// The payload key.
    pub resource: String,
    /// `/UF`, the display name. Required: a filespec without one names
    /// nothing.
    pub name: String,
    /// The stream's `/Subtype`. Defaults to `application/octet-stream`,
    /// which is what a reader reports for a file whose producer said
    /// nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// `/Desc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// What a file-attachment draft carries.
///
/// The icon is generator-owned in exactly the way a sticky note's is: the
/// closing bake draws it from `/C` and `/Name`. Unlike a stamp, the bytes are
/// never sniffed, because ANY bytes are a valid attachment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachmentDraftFields {
    pub rect: PdfRect,
    pub file: AttachmentFileDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    /// `/Name`. The writer's default is the paperclip, which is NOT the
    /// reader's default of a push pin. That asymmetry is deliberate and is
    /// the TypeScript's: §12.5.6.15 says an absent `/Name` reads as
    /// `PushPin`, and a tool creating an attachment marker draws a
    /// paperclip. Since the writer always writes the key, the two never meet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<FileAttachmentIcon>,
}

/// What a redaction draft carries.
///
/// A redaction marking is the NON-DESTRUCTIVE half of a redaction: it says
/// what should be removed and what the region must look like afterwards.
/// Creating one removes nothing, which is why nothing here is irreversible.
///
/// Two colours, and neither is a stroke. `color` is `/C`, the outline a
/// reviewer sees BEFORE the redaction is applied. `interior_color` is `/IC`,
/// the fill painted where the content was, AFTER.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactDraftFields {
    /// `/Rect`. Required even for a text redaction: the caller owns the
    /// arithmetic that turns the quads into a box.
    pub rect: PdfRect,
    /// `/QuadPoints`, for a text redaction. Absent or empty makes it an area
    /// redaction, where `/Rect` is the region.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quad_points: Vec<PdfQuad>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// `/CA`, the MARKING-stage opacity. The applied overlay always paints
    /// opaque, so this never survives an apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    /// `/IC`. Absent leaves the region transparent, which is the ISO default
    /// and is NOT the same as a white fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interior_color: Option<Color>,
    /// `/OverlayText`, the label drawn over the region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_text: Option<String>,
    /// `/Repeat`, which tiles that label to fill the region.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub repeat: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// `/DA` size. Zero is MEANINGFUL here and means auto-fit to the region,
    /// which is why this family does not refuse it the way free text does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    /// The `/DA` colour, which for a redaction is the label text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlignment>,
}

impl RedactDraftFields {
    /// Whether this draft says anything about the label's styling.
    ///
    /// ISO 32000-2 requires a `/DA` alongside an `/OverlayText`, so a draft
    /// that sets a label always emits one and fills any member it did not
    /// name. A draft with no label and no styling emits none, because a `/DA`
    /// on a redaction with nothing to draw is an entry describing nothing.
    pub fn touches_label_style(&self) -> bool {
        self.overlay_text.as_ref().is_some_and(|text| !text.is_empty())
            || self.font_family.is_some()
            || self.font_size.is_some()
            || self.font_color.is_some()
    }
}

/// What a free-text draft carries.
///
/// Three colours, and they are three different entries. `color` is the `/DA`
/// colour, which draws the border AND the text; `font_color` is a `TextColor`
/// override for the text alone; `interior_color` is `/C`, the box background.
/// A reader that conflated any two of them would report a black-bordered box
/// as a black-filled one.
///
/// The intent, the font, the size and the alignment are all REQUIRED. A free
/// text has no meaningful default font, and a draft that omitted one would be
/// asking the engine to decide how somebody's text looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeTextDraftFields {
    pub rect: PdfRect,
    /// `/IT`. Selects a plain box or a callout.
    pub intent: FreeTextIntent,
    /// One of the fourteen standard names, or the key of a font registered
    /// through `fonts`. The standard names are reserved: a registered font
    /// keyed `helvetica` resolves as the standard one and never reaches the
    /// registry.
    pub font_family: String,
    pub font_size: f64,
    pub text_align: TextAlignment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// The `TextColor` override. Absent leaves the text following `color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    /// `/C`, the box background. Absent is a transparent box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interior_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_style: Option<AnnotationBorderStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub rect_differences: Patch<PdfRectDifferences>,
    /// `/CL`, for a callout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callout_line: Option<CalloutLine>,
    /// `/LE`, the ending at the called-out end of the leader. The other end
    /// touches the text box and never draws one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_ending: Option<LineEnding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
}

/// What a sticky-note draft carries.
///
/// The visual is entirely the generator's: the closing bake draws the 20x20
/// note icon from `/C` and `/Name`, so nothing here describes a shape. What
/// it records is which icon and what the review thread has decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDraftFields {
    pub rect: PdfRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    /// `/Name`. Defaults to the plain note, which is what a reader reports
    /// for an annotation with no `/Name` at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<NoteIcon>,
    /// `/StateModel`. A `String` rather than an enum on purpose: Acrobat
    /// lets a workflow register its own model, and the reader passes an
    /// unknown one through, so the writer has to be able to write one back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_model: Option<String>,
    /// `/State`, on the same terms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// What a caret draft carries.
///
/// A caret has no geometry beyond its `/Rect` and the `/RD` inset of the
/// drawn symbol from it. It is a BOX kind, so its rotation is the pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaretDraftFields {
    pub rect: PdfRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    /// `/IT`, for the caret half of a replace-text edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<CaretIntent>,
    /// `/RD`.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub rect_differences: Patch<PdfRectDifferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
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
            Self::Text { .. } => S::Text,
            Self::Caret { .. } => S::Caret,
            Self::FreeText { .. } => S::FreeText,
            Self::Redact { .. } => S::Redact,
            Self::FileAttachment { .. } => S::FileAttachment,
            Self::Link { .. } => S::Link,
            Self::Stamp { .. } => S::Stamp,
            Self::Widget { .. } => S::Widget,
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
