//! The annotation record in upstream's shape, and the widget's.
//!
//! G7. Ports `services/src/features/annotations/internal/read/`
//! `readAnnotationBase.ts` and `readWidgetAnnotation.ts`, read into
//! `core/src/annotation/kinds/shared-fields.ts` `annotationBaseFields` and
//! `kinds/widget/declaration.ts`.
//!
//! Every field is present, `null` when the file has none; the `/F` bits are
//! top-level booleans; colours are `"#rrggbb"`; dates are `createdAt` and
//! `modifiedAt`. The record is read in the file's coordinates and converted
//! to page space at the host boundary (`page_space::forms`).
//!
//! Only the widget reads into this shape so far: the form's rows
//! (`forms.list`) are its first reader. The other kinds still read into
//! [`super::dto`] until the annotation read moves over (Task 8.12), which
//! extends [`AnnotationRecordBase`] rather than writing a second one.

use pdfium::{Annotation, AppearanceMode};
use serde::{Deserialize, Serialize};

use super::base::{AnnotationActions, read_annotation_actions};
use super::dictionary_read;
use crate::clock::pdf_date_to_iso;
use crate::error::EngineError;
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::features::annotation::flags::AnnotationFlags;
use crate::features::annotation::rich_text::free_text_font_for_face;
use crate::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationReplyType, BlendMode, TextAlignment,
};
use crate::features::forms::snapshot::{FormFieldFamily, FormFieldRef};
use crate::geom::{PdfRect, PdfRotation, page_rotation_of};
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::page_space::{Coordinates, PdfCoordinates};

/// `"#rrggbb"`, the way the engine gives colours (`Color`).
pub type HexColor = String;

/// A colour from its 0..255 sRGB components (`colorOf`).
pub fn color_of((r, g, b): (u8, u8, u8)) -> HexColor {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `/IRT` and `/RT`: what the annotation replies to, and how
/// (`AnnotationReplySchema`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationReply {
    pub to: AnnotationRef,
    #[serde(rename = "type")]
    pub reply_type: AnnotationReplyType,
}

/// The fields every annotation kind has (`AnnotationBase<C>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound = "")]
pub struct AnnotationRecordBase<C: Coordinates = PdfCoordinates> {
    #[serde(rename = "ref")]
    pub annotation_ref: AnnotationRef,
    #[serde(with = "crate::identity::page_ref")]
    pub page: PageObjectNumber,
    /// Whether the file holds `/AP /N`.
    pub has_appearance: bool,
    /// `/AS`, or `None` when it names no state.
    pub appearance_state: Option<String>,
    pub nm: Option<String>,
    #[serde(flatten)]
    pub flags: AnnotationFlags,
    pub rect: C::Rect,
    pub contents: Option<String>,
    pub subject: Option<String>,
    /// `/T`.
    pub author: Option<String>,
    /// `/CreationDate`, as ISO 8601.
    pub created_at: Option<String>,
    /// `/M`, as ISO 8601.
    pub modified_at: Option<String>,
    pub blend_mode: BlendMode,
    pub reply: Option<AnnotationReply>,
    /// `/Popup`.
    pub popup: Option<AnnotationRef>,
    pub group_id: Option<String>,
    pub user_id: Option<String>,
    pub created_by: Option<String>,
    /// `/EMBD_Metadata /UpdatedBy`.
    pub modified_by: Option<String>,
    pub imported_by: Option<String>,
    /// `/A` and `/AA`.
    pub actions: Option<AnnotationActions<C::Destination>>,
}

/// A widget's border style: the shape styles and `underline`
/// (`WidgetBorderStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WidgetBorderStyle {
    Solid,
    Dashed,
    Beveled,
    Inset,
    Underline,
}

/// `FPDF_ANNOT_BORDER_STYLE_UNDERLINE`.
const BS_UNDERLINE: i32 = 5;

impl WidgetBorderStyle {
    /// `widgetBorderStyleFromCode`.
    pub fn from_code(code: i32) -> Self {
        if code == BS_UNDERLINE {
            return Self::Underline;
        }
        match AnnotationBorderStyle::from_code(code) {
            AnnotationBorderStyle::Solid => Self::Solid,
            AnnotationBorderStyle::Dashed => Self::Dashed,
            AnnotationBorderStyle::Beveled => Self::Beveled,
            AnnotationBorderStyle::Inset => Self::Inset,
        }
    }
}

/// The symbol a checkbox or radio button shows while on (`WidgetCheckStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WidgetCheckStyle {
    Check,
    Cross,
    Star,
    Circle,
    Square,
    Diamond,
}

impl WidgetCheckStyle {
    /// The ZapfDingbats character `/MK /CA` holds for each symbol, as
    /// Acrobat writes it (`widgetCaption.ts`).
    const CHARACTERS: [(Self, char); 6] = [
        (Self::Check, '4'),
        (Self::Cross, '8'),
        (Self::Star, 'H'),
        (Self::Circle, 'l'),
        (Self::Square, 'n'),
        (Self::Diamond, 'u'),
    ];

    /// The symbol a `/MK /CA` names, or `None` when it names none the
    /// appearance generator knows (`checkStyleOfCaption`).
    pub fn of_caption(caption: Option<&str>) -> Option<Self> {
        let first = caption?.chars().next()?;
        Self::CHARACTERS.iter().find(|(_, c)| *c == first).map(|(style, _)| *style)
    }
}

/// A form widget's row (`WidgetAnnotation<C>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound = "")]
pub struct WidgetAnnotation<C: Coordinates = PdfCoordinates> {
    #[serde(flatten)]
    pub base: AnnotationRecordBase<C>,
    /// The box the contents are laid out in, before the turn.
    #[serde(rename = "box")]
    pub content_box: C::Rect,
    /// `/MK /R` as a clockwise quarter turn; `None` upright.
    pub rotation: Option<PdfRotation>,
    /// Always `"widget"`.
    pub subtype: WidgetSubtype,
    /// `/MK /BC`.
    pub color: Option<HexColor>,
    /// `/MK /BG`.
    pub interior_color: Option<HexColor>,
    pub stroke_width: f64,
    pub border_style: WidgetBorderStyle,
    /// `/BS /D`; `None` for the default.
    pub dash_array: Option<Vec<f64>>,
    /// The font `/DA` names, as a free text names its font; `None` when it
    /// names none.
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_color: Option<HexColor>,
    pub text_align: TextAlignment,
    /// A push button's `/MK /CA`; `None` for every other widget.
    pub caption: Option<String>,
    /// A checkbox's or radio button's symbol; `None` for other widgets.
    pub check_style: Option<WidgetCheckStyle>,
    /// The field the widget belongs to, or `None` when it's in none.
    pub field: Option<FormFieldRef>,
    pub field_family: FormFieldFamily,
}

/// The one subtype a [`WidgetAnnotation`] has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WidgetSubtype {
    Widget,
}

/// `EPDF_MK_COLOR_BC`, `EPDF_MK_COLOR_BG` and `EPDF_MK_TEXT_CA`.
const MK_BORDER_COLOR: i32 = 0;
const MK_BACKGROUND_COLOR: i32 = 1;
const MK_CAPTION: i32 = 0;

/// The annotation at `index` of `page`'s `/Annots`, by its name for life
/// (`annotationRefOf`): its object number, or its position when it is
/// inline. A promoted inline annotation's birth name is not read yet: that
/// needs `EPDFLayer_GetBirthName`, which Task 8.12 binds.
pub fn annotation_ref_of(
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
    index: u32,
) -> AnnotationRef {
    match annotation.object_number() {
        0 => AnnotationRef::BaseIndex { page, base_index: index },
        annot => AnnotationRef::ObjectNumber { page, annot },
    }
}

/// The annotation `key` (`/IRT`, `/Popup`) points at, on the same page, or
/// `None` when the key is absent or its target has no object number
/// (`readLinkedAnnotationRef`).
fn linked_ref(
    annotation: &Annotation<'_>,
    key: &str,
    page: PageObjectNumber,
) -> Result<Option<AnnotationRef>, EngineError> {
    let Some(linked) = dictionary_read(key, annotation.linked_annotation(key))? else {
        return Ok(None);
    };
    Ok(match linked.object_number() {
        0 => None,
        annot => Some(AnnotationRef::ObjectNumber { page, annot }),
    })
}

/// An `/EMBD_Metadata` string: `None` when absent or empty
/// (`readEmbedMetadataString`).
fn embed_text(annotation: &Annotation<'_>, key: &str) -> Result<Option<String>, EngineError> {
    Ok(dictionary_read(key, annotation.embed_metadata_string(key))?.filter(|s| !s.is_empty()))
}

/// Read the fields every kind has (`readAnnotationBase`), in the file's
/// coordinates.
pub fn read_annotation_record_base(
    document: &pdfium::Document,
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
    index: u32,
    budget: &mut ActionReadBudgetTracker,
) -> Result<AnnotationRecordBase, EngineError> {
    let rect = annotation.rect();
    let date = |key: &str| -> Result<Option<String>, EngineError> {
        let raw = dictionary_read(key, annotation.string_value(key))?;
        // An empty date is no date; one the parser refuses reads as none.
        Ok(raw.filter(|raw| !raw.is_empty()).as_deref().and_then(pdf_date_to_iso))
    };
    let reply = linked_ref(annotation, "IRT", page)?.map(|to| AnnotationReply {
        to,
        reply_type: AnnotationReplyType::from_code(annotation.reply_type_code()),
    });
    let has_embed = annotation.has_embed_metadata();
    let embed = |key: &str| -> Result<Option<String>, EngineError> {
        if has_embed { embed_text(annotation, key) } else { Ok(None) }
    };

    Ok(AnnotationRecordBase {
        annotation_ref: annotation_ref_of(annotation, page, index),
        page,
        has_appearance: annotation.has_appearance_stream(AppearanceMode::Normal),
        appearance_state: dictionary_read("AS", annotation.appearance_state())?,
        nm: dictionary_read("NM", annotation.string_value("NM"))?,
        flags: AnnotationFlags::from_bits(annotation.flags()),
        rect: PdfRect {
            left: f64::from(rect.left),
            bottom: f64::from(rect.bottom),
            right: f64::from(rect.right),
            top: f64::from(rect.top),
        },
        contents: dictionary_read("Contents", annotation.string_value("Contents"))?,
        subject: dictionary_read("Subj", annotation.string_value("Subj"))?,
        author: dictionary_read("T", annotation.string_value("T"))?,
        created_at: date("CreationDate")?,
        modified_at: date("M")?,
        blend_mode: BlendMode::from_code(annotation.blend_mode_code()),
        reply,
        popup: linked_ref(annotation, "Popup", page)?,
        group_id: embed("GroupID")?,
        user_id: embed("UserID")?,
        created_by: embed("CreatedBy")?,
        modified_by: embed("UpdatedBy")?,
        imported_by: embed("ImportedBy")?,
        actions: read_annotation_actions(document, annotation, budget)?,
    })
}

/// A widget's turn: `/MK /R`, which the file holds counterclockwise, as the
/// clockwise quarter turn the API reads; `None` upright, and for a value
/// that isn't a quarter turn, as the appearance generator draws it
/// (`readWidgetTurn`).
pub fn read_widget_turn(annotation: &Annotation<'_>) -> Option<PdfRotation> {
    let degrees = annotation.mk_rotation();
    if degrees % 90 != 0 {
        return None;
    }
    Some(page_rotation_of(-degrees)).filter(|turn| *turn != PdfRotation::None)
}

/// The box that, turned `rotation` about its middle, stands upright in
/// `rect`: `rect` itself, its sides swapped under 90 and 270
/// (`pdfQuarterTurnBox`).
pub fn pdf_quarter_turn_box(rect: PdfRect, rotation: Option<PdfRotation>) -> PdfRect {
    match rotation {
        Some(PdfRotation::Clockwise90 | PdfRotation::Clockwise270) => {
            let x = (rect.left + rect.right) / 2.0;
            let y = (rect.bottom + rect.top) / 2.0;
            let half_width = (rect.top - rect.bottom) / 2.0;
            let half_height = (rect.right - rect.left) / 2.0;
            PdfRect {
                left: x - half_width,
                bottom: y - half_height,
                right: x + half_width,
                top: y + half_height,
            }
        }
        _ => rect,
    }
}

/// Read a widget's row (`readWidget`), the field join left unattached for
/// [`join_widget_field`]. Its attribution is its field's to say, so the row
/// reports none of its own.
pub fn read_widget_annotation(
    annotation: &Annotation<'_>,
    base: AnnotationRecordBase,
) -> Result<WidgetAnnotation, EngineError> {
    let border = annotation.border_style();
    let dash = dictionary_read("BS", annotation.border_dash_pattern())?;
    let appearance = annotation.default_appearance();
    let face = dictionary_read("DA", annotation.default_appearance_font_face())?;
    let key_for_face = crate::features::fonts::key_for_face;
    let rotation = read_widget_turn(annotation);
    let content_box = pdf_quarter_turn_box(base.rect, rotation);
    Ok(WidgetAnnotation {
        base: AnnotationRecordBase {
            group_id: None,
            user_id: None,
            created_by: None,
            modified_by: None,
            imported_by: None,
            ..base
        },
        content_box,
        rotation,
        subtype: WidgetSubtype::Widget,
        color: annotation.mk_color(MK_BORDER_COLOR).map(color_of),
        interior_color: annotation.mk_color(MK_BACKGROUND_COLOR).map(color_of),
        stroke_width: f64::from(border.width),
        border_style: WidgetBorderStyle::from_code(border.style_code),
        dash_array: (!dash.is_empty()).then(|| dash.into_iter().map(f64::from).collect()),
        font_family: face.map(|(family, weight, italic)| {
            free_text_font_for_face(&family, weight.max(0) as u32, italic, Some(&key_for_face))
        }),
        font_size: appearance.map(|da| f64::from(da.font_size)),
        font_color: appearance.map(|da| color_of(da.color)),
        text_align: TextAlignment::from_code(annotation.text_alignment()),
        caption: dictionary_read("MK", annotation.mk_text(MK_CAPTION))?,
        check_style: None,
        field: None,
        field_family: FormFieldFamily::Unknown,
    })
}

/// Stamp a widget row with its field and family (`joinWidgetFieldNumbers`).
/// `/MK /CA` is a push button's caption and a checkbox's or radio button's
/// symbol; another widget's reads as neither.
pub fn join_widget_field(model: &pdfium::FormModel<'_>, widget: &mut WidgetAnnotation) {
    let AnnotationRef::ObjectNumber { annot, .. } = widget.base.annotation_ref else {
        widget.caption = None;
        return;
    };
    let field = model.field_index_for_widget(annot).and_then(|index| model.field(index));
    let Some(field) = field.filter(|field| field.object_number() > 0) else {
        // In no field, or in one stored inline, which no ref can address.
        widget.field = None;
        widget.field_family = FormFieldFamily::Unknown;
        widget.caption = None;
        return;
    };
    widget.field = Some(FormFieldRef::ObjectNumber { field: field.object_number() });
    widget.field_family = FormFieldFamily::from_code(field.family_code());
    if matches!(widget.field_family, FormFieldFamily::Checkbox | FormFieldFamily::Radio) {
        widget.check_style = WidgetCheckStyle::of_caption(widget.caption.as_deref());
    }
    if widget.field_family != FormFieldFamily::Pushbutton {
        widget.caption = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_two_lowercase_hex_digits_a_component() {
        assert_eq!(color_of((0, 0, 0)), "#000000");
        assert_eq!(color_of((255, 255, 204)), "#ffffcc");
        assert_eq!(color_of((1, 171, 16)), "#01ab10");
    }

    #[test]
    fn a_check_style_is_named_by_the_first_character_of_the_caption() {
        assert_eq!(WidgetCheckStyle::of_caption(Some("4")), Some(WidgetCheckStyle::Check));
        assert_eq!(WidgetCheckStyle::of_caption(Some("u")), Some(WidgetCheckStyle::Diamond));
        assert_eq!(WidgetCheckStyle::of_caption(Some("8x")), Some(WidgetCheckStyle::Cross));
        assert_eq!(WidgetCheckStyle::of_caption(Some("Q")), None);
        assert_eq!(WidgetCheckStyle::of_caption(Some("")), None);
        assert_eq!(WidgetCheckStyle::of_caption(None), None);
    }

    #[test]
    fn a_quarter_turn_swaps_the_sides_about_the_middle() {
        let rect = PdfRect { left: 10.0, bottom: 20.0, right: 50.0, top: 30.0 };
        assert_eq!(pdf_quarter_turn_box(rect, None), rect);
        assert_eq!(pdf_quarter_turn_box(rect, Some(PdfRotation::Clockwise180)), rect);
        assert_eq!(
            pdf_quarter_turn_box(rect, Some(PdfRotation::Clockwise90)),
            PdfRect { left: 25.0, bottom: 5.0, right: 35.0, top: 45.0 }
        );
    }

    #[test]
    fn underline_is_a_widget_border_style_and_the_rest_are_the_shape_styles() {
        assert_eq!(WidgetBorderStyle::from_code(BS_UNDERLINE), WidgetBorderStyle::Underline);
        assert_eq!(WidgetBorderStyle::from_code(2), WidgetBorderStyle::Dashed);
        assert_eq!(WidgetBorderStyle::from_code(0), WidgetBorderStyle::Solid);
    }
}
