//! The form widget.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readWidgetAnnotation.ts`.
//!
//! A widget is the PAGE-SCOPED view of a form field. The value, the options
//! and the field semantics live on `doc.forms`; what is here is the widget
//! plane, which is geometry, appearance and a join key.
//!
//! Its colours come from `/MK`, not `/C`. That is the appearance-
//! characteristics dictionary §12.5.6.19 gives widgets, and it is a different
//! entry, so a reader that used the ordinary colour path would report nothing
//! for a widget that plainly has a border and a background.
//!
//! # The join is not ported and says so
//!
//! `field_object_number` and `field_family` are the key onto the forms side,
//! and both are reported as the unattached values here, always. Resolving
//! them means following `/Parent` to a FIELD dictionary, which no
//! annotation-plane accessor can do: the reconciled form model is the
//! authority, and that model is G8. The TypeScript has the same split, with
//! the same two placeholder values written by its own reader and filled in
//! afterwards by `joinWidgetFieldNumbers`.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::style::{BorderFields, Color, read_border_fields};
use crate::error::EngineError;
use crate::features::annotation::vocabulary::{AnnotationBorderStyle, StandardFont, TextAlignment};

/// `EPDF_MK_COLORTYPE`.
const MK_BORDER_COLOR: i32 = 0;
const MK_BACKGROUND_COLOR: i32 = 1;

/// What a widget reports for its owning field when it has none.
pub const UNATTACHED_FIELD_OBJECT_NUMBER: u32 = 0;

/// A widget annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetFields {
    /// `/MK /BC`, the border. `null` when the widget draws none.
    pub color: Option<Color>,
    /// `/MK /BG`, the background.
    pub interior_color: Option<Color>,
    pub stroke_width: f64,
    pub border_style: AnnotationBorderStyle,
    /// The `/DA` triple, present only when the widget has a `/DA` at all.
    /// A radio button has no text and so has no font, which is why these are
    /// three optional fields rather than three defaulted ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<StandardFont>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    pub text_align: TextAlignment,
    /// The owning field's object number, or zero when unattached. Always
    /// zero in this build: see the module docs.
    pub field_object_number: u32,
    /// The owning field's family. Always `unknown` in this build.
    pub field_family: FormFieldFamily,
}

/// What kind of control a widget renders, once its field is known.
///
/// Only [`FormFieldFamily::Unknown`] is produced here. The rest are the
/// vocabulary G8's form reader will fill in, and they are declared now so the
/// wire shape does not change when it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormFieldFamily {
    Unknown,
    PushButton,
    Checkbox,
    RadioButton,
    Text,
    Choice,
    Signature,
}

/// Read them.
pub fn read_widget(annotation: &Annotation<'_>) -> Result<WidgetFields, EngineError> {
    let border: BorderFields = read_border_fields(annotation)?;
    let appearance = annotation.default_appearance();
    Ok(WidgetFields {
        color: annotation.mk_color(MK_BORDER_COLOR).map(|(r, g, b)| Color { r, g, b }),
        interior_color: annotation.mk_color(MK_BACKGROUND_COLOR).map(|(r, g, b)| Color { r, g, b }),
        stroke_width: border.stroke_width,
        border_style: border.border_style,
        font_family: appearance.map(|da| StandardFont::from_code(da.font_code)),
        font_size: appearance.map(|da| f64::from(da.font_size)),
        font_color: appearance.map(|da| Color { r: da.color.0, g: da.color.1, b: da.color.2 }),
        text_align: TextAlignment::from_code(annotation.text_alignment()),
        field_object_number: UNATTACHED_FIELD_OBJECT_NUMBER,
        field_family: FormFieldFamily::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unported_join_reports_the_unattached_values() {
        // Stated as a test rather than only as a comment, so that when G8
        // lands the join, whoever changes these two lines finds the place
        // that says what they used to mean.
        assert_eq!(UNATTACHED_FIELD_OBJECT_NUMBER, 0);
        assert_eq!(
            serde_json::to_value(FormFieldFamily::Unknown).unwrap(),
            serde_json::json!("unknown"),
        );
    }

    #[test]
    fn the_default_standard_font_is_not_reported_when_there_is_no_da() {
        // A widget with no `/DA` has no font at all, which is different from
        // having the default one: a radio button never draws text, so the
        // three `/DA` fields are absent rather than defaulted.
        let fields = WidgetFields {
            color: None,
            interior_color: None,
            stroke_width: 1.0,
            border_style: AnnotationBorderStyle::Solid,
            font_family: None,
            font_size: None,
            font_color: None,
            text_align: TextAlignment::Left,
            field_object_number: UNATTACHED_FIELD_OBJECT_NUMBER,
            field_family: FormFieldFamily::Unknown,
        };
        let value = serde_json::to_value(&fields).unwrap();
        assert!(value.get("fontFamily").is_none());
        assert!(value.get("fontSize").is_none());
        assert!(value.get("fontColor").is_none());
    }
}
