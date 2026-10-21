//! The types the pre-catch-up patch and widget writers take, and who
//! creates an annotation.
//!
//! G11. A create's data is upstream's v3 draft, checked and resolved in
//! [`super::prepare`]. What is left here is the flags patch, the stamp fit
//! and link target and resource key the patch writer ([`super::patch`],
//! 8.15) still reads, the widget styling `forms.createField` places a widget
//! with (8.21), and [`AnnotationActor`].

use serde::{Deserialize, Serialize};

use crate::dto::Patch;
use crate::features::actions::destination::PdfDestination;
use crate::features::annotation::flags::AnnotationFlags;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::vocabulary::{AnnotationBorderStyle, StandardFont, TextAlignment};
use crate::geom::PdfRect;

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
