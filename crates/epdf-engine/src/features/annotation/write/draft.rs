//! The widget styling `forms.createField` places a widget with, and who
//! writes an annotation.
//!
//! G11. A create's data is upstream's v3 draft, checked and resolved in
//! [`super::prepare`], and an update's its patch, in [`super::patch`]. What
//! is left here is the widget styling `forms.createField` places a widget
//! with (8.21), and [`AnnotationActor`].

use serde::{Deserialize, Serialize};

use crate::dto::Patch;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::vocabulary::{AnnotationBorderStyle, StandardFont, TextAlignment};
use crate::geom::PdfRect;

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
