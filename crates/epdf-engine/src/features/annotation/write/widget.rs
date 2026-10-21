//! A widget's place and look.
//!
//! Ports `services/src/features/annotations/internal/write/
//! writeWidgetAnnotation.ts` (`applyWidgetStyle`, `applyWidgetFont`,
//! `setWidgetTurn`, `writeWidgetBox`, `applyWidgetBoxPatch`,
//! `applyWidgetPatch`) and the widget kind's update fields
//! (`kinds/widget/declaration.ts`).
//!
//! One writer for both entry points: a field's widget placed by
//! `forms.create` or `forms.addWidget`, and a widget changed by
//! `forms.updateWidget`. Creation-time and edit-time styling cannot drift
//! apart.
//!
//! A widget's colours are not `/C` and `/IC`: they are its `/MK` border and
//! background, its border `/BS`, its text defaults `/DA` and its alignment
//! `/Q`.

use pdfium::Annotation;
use serde_json::Value;

use super::context::AnnotationWriteContext;
use super::prepare::{Nullable, Rgb};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::record::{
    WidgetBorderStyle, WidgetCheckStyle, pdf_quarter_turn_box, read_widget_turn,
};
use crate::features::annotation::rich_text::{face_for_free_text_font, family_key};
use crate::features::annotation::vocabulary::{AnnotationBorderStyle, StandardFont, TextAlignment};
use crate::geom::{PdfRect, PdfRotation, normalize_pdf_rect, pdf_rect_turned_bounds};

/// `EPDF_MK_COLOR_BC`, `EPDF_MK_COLOR_BG` and `EPDF_MK_TEXT_CA`.
const MK_BORDER_COLOR: i32 = 0;
const MK_BACKGROUND_COLOR: i32 = 1;
const MK_CAPTION: i32 = 0;

/// `KEEP_FONT_CODE`: `/DA` keeps the font it names.
const KEEP_FONT_CODE: i32 = -1;

/// `FPDF_ANNOT_BORDER_STYLE_UNDERLINE`.
const BS_UNDERLINE: i32 = 5;

/// The widget-plane style fields (`WidgetStyleDraftFields`), each as a
/// write takes it: absent keeps, `null` clears where the field is nullable.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WidgetStyle {
    pub color: Nullable<Rgb>,
    pub interior_color: Nullable<Rgb>,
    pub stroke_width: Option<f64>,
    pub border_style: Option<WidgetBorderStyle>,
    pub dash_array: Nullable<Vec<f64>>,
    pub font_family: Nullable<String>,
    pub font_size: Nullable<f64>,
    pub font_color: Nullable<Rgb>,
    pub text_align: Option<TextAlignment>,
    pub caption: Nullable<String>,
    pub check_style: Nullable<WidgetCheckStyle>,
}

/// The style fields, by name (`WIDGET_STYLE_NAMES`).
pub const WIDGET_STYLE_NAMES: [&str; 11] = [
    "color",
    "interiorColor",
    "strokeWidth",
    "borderStyle",
    "dashArray",
    "fontFamily",
    "fontSize",
    "fontColor",
    "textAlign",
    "caption",
    "checkStyle",
];

fn invalid(kind: &str, field: &str, message: impl std::fmt::Display) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("{kind} field '{field}': {message}"),
    )
    .with_details(serde_json::json!({ "field": field }))
}

fn nullable<T>(
    value: &Value,
    read: impl FnOnce(&Value) -> Result<T, EngineError>,
) -> Result<Option<T>, EngineError> {
    if value.is_null() { Ok(None) } else { read(value).map(Some) }
}

fn color(kind: &str, name: &str, value: &Value) -> Result<Rgb, EngineError> {
    value
        .as_str()
        .and_then(Rgb::parse_hex)
        .ok_or_else(|| invalid(kind, name, "expected a color as '#rrggbb'"))
}

fn nonnegative(kind: &str, name: &str, value: &Value) -> Result<f64, EngineError> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| invalid(kind, name, "expected a number of 0 or more"))
}

fn read_enum<T: serde::de::DeserializeOwned>(
    kind: &str,
    name: &str,
    value: &Value,
) -> Result<T, EngineError> {
    serde_json::from_value(value.clone()).map_err(|e| invalid(kind, name, e))
}

/// Read one widget style field into `style`; `false` when `name` is not one.
/// `kind` names the write in a refusal (`"widget update"`).
pub fn read_style_field(
    kind: &str,
    name: &str,
    value: &Value,
    style: &mut WidgetStyle,
) -> Result<bool, EngineError> {
    match name {
        "color" => style.color = Some(nullable(value, |v| color(kind, name, v))?),
        "interiorColor" => style.interior_color = Some(nullable(value, |v| color(kind, name, v))?),
        "fontColor" => style.font_color = Some(nullable(value, |v| color(kind, name, v))?),
        "strokeWidth" => style.stroke_width = Some(nonnegative(kind, name, value)?),
        "borderStyle" => style.border_style = Some(read_enum(kind, name, value)?),
        "dashArray" => {
            style.dash_array = Some(nullable(value, |v| {
                let dashes: Vec<f64> = read_enum(kind, name, v)?;
                if dashes.iter().any(|d| !d.is_finite() || *d < 0.0) {
                    return Err(invalid(kind, name, "expected numbers of 0 or more"));
                }
                Ok(dashes)
            })?);
        }
        "fontFamily" => {
            style.font_family = Some(nullable(value, |v| {
                v.as_str()
                    .filter(|family| !family.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(kind, name, "expected at least one character"))
            })?);
        }
        "fontSize" => style.font_size = Some(nullable(value, |v| nonnegative(kind, name, v))?),
        "textAlign" => style.text_align = Some(read_enum(kind, name, value)?),
        "caption" => {
            style.caption = Some(nullable(value, |v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(kind, name, "expected a string"))
            })?);
        }
        "checkStyle" => style.check_style = Some(nullable(value, |v| read_enum(kind, name, v))?),
        _ => return Ok(false),
    }
    Ok(true)
}

/// `applyWidgetStyle`: `/MK` colours and caption, `/BS`, `/DA`, `/Q`.
pub fn apply_widget_style(
    annotation: &Annotation<'_>,
    style: &WidgetStyle,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    write_mk_color(annotation, MK_BORDER_COLOR, style.color.as_ref())?;
    write_mk_color(annotation, MK_BACKGROUND_COLOR, style.interior_color.as_ref())?;

    // Style and width share one call: the one a change leaves out keeps what
    // the border has (a widget without `/BS` has a solid 1-point border).
    if style.stroke_width.is_some() || style.border_style.is_some() {
        let current = annotation.border_style();
        let code = match style.border_style {
            Some(chosen) => border_code(chosen),
            None => border_code(WidgetBorderStyle::from_code(current.style_code)),
        };
        let width = style.stroke_width.map_or(current.width, |w| w as f32);
        if !annotation.set_border_style(code, width) {
            return Err(unknown("EPDFAnnot_SetBorderStyle refused the widget's border"));
        }
    }
    match &style.dash_array {
        None => {}
        Some(None) => super::primitives::clear_border_dash_pattern(annotation)?,
        Some(Some(dashes)) => {
            let dashes: Vec<f32> = dashes.iter().map(|d| *d as f32).collect();
            if !annotation.set_border_dash_pattern(&dashes) {
                return Err(unknown("EPDFAnnot_SetBorderDashPattern refused the pattern"));
            }
        }
    }
    if style.font_family.is_some() || style.font_size.is_some() || style.font_color.is_some() {
        apply_widget_font(annotation, style, context)?;
    }
    // `/Q` is attempted and its refusal ignored, as upstream's: the fork
    // declines it on a widget with no `/FT` yet.
    if let Some(align) = style.text_align {
        let _ = annotation.set_text_alignment(align.code());
    }
    // `/MK /CA`: a push button's caption, or a toggle's symbol; a `null`
    // symbol removes it, and the family's own shows.
    let text: Option<Option<String>> = match &style.check_style {
        Some(check) => Some(check.map(|check| check.character().to_string())),
        None => style.caption.clone(),
    };
    if let Some(text) = text
        && !annotation.set_mk_text(MK_CAPTION, text.as_deref())
    {
        return Err(unknown("EPDFAnnot_SetMKText refused the widget's caption"));
    }
    Ok(())
}

fn border_code(style: WidgetBorderStyle) -> i32 {
    match style {
        WidgetBorderStyle::Solid => AnnotationBorderStyle::Solid.code(),
        WidgetBorderStyle::Dashed => AnnotationBorderStyle::Dashed.code(),
        WidgetBorderStyle::Beveled => AnnotationBorderStyle::Beveled.code(),
        WidgetBorderStyle::Inset => AnnotationBorderStyle::Inset.code(),
        WidgetBorderStyle::Underline => BS_UNDERLINE,
    }
}

fn write_mk_color(
    annotation: &Annotation<'_>,
    which: i32,
    color: Option<&Option<Rgb>>,
) -> Result<(), EngineError> {
    let ok = match color {
        None => return Ok(()),
        Some(None) => annotation.clear_mk_color(which),
        Some(Some(Rgb(r, g, b))) => annotation.set_mk_color(which, (*r, *g, *b)),
    };
    if !ok {
        return Err(unknown("PDFium refused a widget /MK colour"));
    }
    Ok(())
}

/// `applyWidgetFont`: `/DA` with what `style` changes. What it leaves out
/// keeps what `/DA` says now, so a colour change keeps the font and its
/// size. Naming the face `/DA` already has keeps that font.
fn apply_widget_font(
    annotation: &Annotation<'_>,
    style: &WidgetStyle,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    let current = annotation.default_appearance();
    let font_size = match style.font_size {
        None => current.map_or(12.0, |da| f64::from(da.font_size)),
        Some(size) => size.unwrap_or(12.0),
    };
    let color = match style.font_color {
        None => current.map_or(Rgb::BLACK, |da| Rgb(da.color.0, da.color.1, da.color.2)),
        Some(color) => color.unwrap_or(Rgb::BLACK),
    };
    let keeps = current.is_some()
        && match &style.font_family {
            None => true,
            Some(None) => false,
            Some(Some(family)) => names_current_face(annotation, family, context),
        };
    let Rgb(r, g, b) = color;
    if keeps {
        if !annotation.set_default_appearance(KEEP_FONT_CODE, font_size as f32, (r, g, b)) {
            return Err(unknown("EPDFAnnot_SetDefaultAppearance refused the widget's text"));
        }
        return Ok(());
    }
    let family = style.font_family.clone().flatten().unwrap_or_else(|| "helvetica".to_owned());
    let ok = match StandardFont::from_family(&family) {
        Some(standard) => {
            annotation.set_default_appearance(standard.code(), font_size as f32, (r, g, b))
        }
        None => annotation.set_default_appearance_registered_font(
            context.resolve_font(&family)?,
            font_size as f32,
            (r, g, b),
        ),
    };
    if !ok {
        return Err(unknown("EPDFAnnot_SetDefaultAppearance refused the widget's text"));
    }
    Ok(())
}

/// Whether `font` names the face `/DA` has now (`namesCurrentFace`).
fn names_current_face(
    annotation: &Annotation<'_>,
    font: &str,
    context: &AnnotationWriteContext<'_>,
) -> bool {
    let Ok(Some((family, weight, italic))) = annotation.default_appearance_font_face() else {
        return false;
    };
    let describe = |key: &str| context.describe_font(key);
    let named = face_for_free_text_font(font, Some(&describe));
    family_key(&named.family) == family_key(&family)
        && named.weight.is_none_or(|w| i64::from(w) == i64::from(weight))
        && named.italic.is_none_or(|i| i == italic)
}

/// `setWidgetTurn`: `/MK /R`, counterclockwise as the file keeps it, from
/// a clockwise quarter turn; upright removes it. The appearance is drawn
/// again by the caller.
pub fn set_widget_turn(
    annotation: &Annotation<'_>,
    rotation: Option<PdfRotation>,
) -> Result<(), EngineError> {
    let clockwise = i32::from(rotation.map_or(0, PdfRotation::degrees));
    if !annotation.set_mk_rotation((360 - clockwise) % 360) {
        return Err(unknown("the widget's turn (/MK /R) could not be written"));
    }
    Ok(())
}

/// `writeWidgetBox`: `/Rect` is where the widget stands, the box turned, and
/// `/MK /R` the turn. Each is written only when it changes from `current`.
pub fn write_widget_box(
    annotation: &Annotation<'_>,
    content_box: PdfRect,
    rotation: Option<PdfRotation>,
    current: Option<(PdfRect, Option<PdfRotation>)>,
) -> Result<(), EngineError> {
    let content_box = normalize_pdf_rect(content_box);
    let rect = match rotation {
        Some(turn) if turn != PdfRotation::None => {
            pdf_rect_turned_bounds(content_box, f64::from(turn.degrees()))
        }
        _ => content_box,
    };
    if current.is_none_or(|(_, turn)| turn != rotation) {
        set_widget_turn(annotation, rotation)?;
    }
    if current.is_none_or(|(now, _)| !same_rect(now, rect)) {
        super::primitives::set_rect(annotation, rect)?;
    }
    Ok(())
}

fn same_rect(a: PdfRect, b: PdfRect) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-3;
    close(a.left, b.left)
        && close(a.bottom, b.bottom)
        && close(a.right, b.right)
        && close(a.top, b.top)
}

/// `applyWidgetBoxPatch`: an update's `box` and `rotation`, each kept when
/// left out (`rotation: null` or `0` turns the widget upright about its
/// middle).
pub fn apply_widget_box_patch(
    annotation: &Annotation<'_>,
    content_box: Option<PdfRect>,
    rotation: Option<Option<PdfRotation>>,
) -> Result<(), EngineError> {
    if content_box.is_none() && rotation.is_none() {
        return Ok(());
    }
    let rect = normalize_pdf_rect(super::primitives::read_rect(annotation));
    let turn = read_widget_turn(annotation);
    let placed = content_box.unwrap_or_else(|| pdf_quarter_turn_box(rect, turn));
    let rotation = match rotation {
        None => turn,
        Some(turn) => turn.filter(|t| *t != PdfRotation::None),
    };
    write_widget_box(annotation, placed, rotation, Some((rect, turn)))
}

/// `pdfRotationOf`: a widget's turn as written, a quarter turn clockwise.
/// `None` for an angle that isn't one.
pub fn quarter_turn(degrees: f64) -> Option<Option<PdfRotation>> {
    let turn = degrees.rem_euclid(360.0);
    if turn.fract() != 0.0 || !(turn as u32).is_multiple_of(90) {
        return None;
    }
    Some(match turn as u32 {
        90 => Some(PdfRotation::Clockwise90),
        180 => Some(PdfRotation::Clockwise180),
        270 => Some(PdfRotation::Clockwise270),
        _ => None,
    })
}

fn unknown(message: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_field_reads_as_a_write_takes_it() {
        let mut style = WidgetStyle::default();
        assert!(read_style_field("widget update", "color", &Value::Null, &mut style).unwrap());
        assert_eq!(style.color, Some(None));
        assert!(
            read_style_field(
                "widget update",
                "interiorColor",
                &serde_json::json!("#ff8000"),
                &mut style
            )
            .unwrap()
        );
        assert_eq!(style.interior_color, Some(Some(Rgb(255, 128, 0))));
        assert!(
            read_style_field(
                "widget update",
                "borderStyle",
                &serde_json::json!("underline"),
                &mut style
            )
            .unwrap()
        );
        assert_eq!(style.border_style, Some(WidgetBorderStyle::Underline));
        assert!(!read_style_field("widget update", "rect", &Value::Null, &mut style).unwrap());
        let refused =
            read_style_field("widget update", "strokeWidth", &serde_json::json!(-1), &mut style)
                .unwrap_err();
        assert_eq!(refused.details, Some(serde_json::json!({ "field": "strokeWidth" })));
    }

    #[test]
    fn a_quarter_turn_is_one_of_four() {
        assert_eq!(quarter_turn(90.0), Some(Some(PdfRotation::Clockwise90)));
        assert_eq!(quarter_turn(0.0), Some(None));
        assert_eq!(quarter_turn(-90.0), Some(Some(PdfRotation::Clockwise270)));
        assert_eq!(quarter_turn(45.0), None);
    }
}
