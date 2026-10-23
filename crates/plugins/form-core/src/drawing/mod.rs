//! `drawing/index.ts`: what a widget looks like for its facts, as the engine's
//! appearance generator draws it. The widget row gives the look (`/MK`, `/BS`,
//! `/DA`) and the box, the field gives the family and its flags, and `shown`
//! what to show now: a value this session formatted, a toggle's state.

mod border;
mod text;
mod toggle;
mod types;

use epdf_fonts::StandardFont;
use epdf_geometry::Rect;
use serde::Deserialize;

pub(crate) use text::{TextLook, text_field_layout};
pub use types::{Approximation, DrawOp, PathSegment, Rgb, TextRun, WidgetDrawing};

use self::border::{border_style_info, rgb_of};
use self::text::{combo_box_ops, list_box_ops, push_button_ops, text_field_ops};
use self::toggle::{ToggleLook, checkbox_ops, radio_ops};
use crate::{Alignment, LayoutFont, PdfRect, layout_font_of};

/// A form widget's border (`/BS /S`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetBorderStyle {
    Solid,
    Dashed,
    Beveled,
    Inset,
    Underline,
}

/// The symbol a check box or radio button shows while it's on (`/MK /CA`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetCheckStyle {
    Check,
    Cross,
    Star,
    Circle,
    Square,
    Diamond,
}

/// `/Q`, as the widget row names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl From<TextAlign> for Alignment {
    fn from(align: TextAlign) -> Alignment {
        match align {
            TextAlign::Left => Alignment::Left,
            TextAlign::Center => Alignment::Center,
            TextAlign::Right => Alignment::Right,
        }
    }
}

/// The members of a widget annotation (`WidgetAnnotation`) the painter
/// reads. Deserializes from the DTO's JSON and ignores the rest of it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetRow {
    pub rect: Rect,
    /// The box the contents are laid out in, before the turn; `rect` when
    /// absent.
    #[serde(rename = "box")]
    pub box_: Option<Rect>,
    /// The state a toggle shows; `None` reads as off, as `null` does.
    pub appearance_state: Option<String>,
    /// `#rrggbb`.
    pub color: Option<String>,
    pub interior_color: Option<String>,
    pub stroke_width: Option<f64>,
    pub border_style: Option<WidgetBorderStyle>,
    pub dash_array: Option<Vec<f64>>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_color: Option<String>,
    pub text_align: Option<TextAlign>,
    pub caption: Option<String>,
    pub check_style: Option<WidgetCheckStyle>,
}

impl WidgetRow {
    /// `widget.box ?? widget.rect`.
    fn frame(&self) -> Rect {
        self.box_.unwrap_or(self.rect)
    }
}

/// A list option as the painter reads it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct FieldOption {
    pub label: String,
    pub selected: bool,
}

/// A text field's facts the painter reads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFieldFacts {
    pub value: String,
    /// `/MaxLen`; `None` when unlimited.
    pub max_length: Option<u32>,
    pub multiline: bool,
    pub password: bool,
    pub comb: bool,
}

/// What the painter reads of a form field (`FormFieldDTO`), by family.
/// Deserializes from the DTO's JSON and ignores the rest of it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "family")]
pub enum FieldFacts {
    #[serde(rename = "text")]
    Text(TextFieldFacts),
    #[serde(rename = "checkbox")]
    Checkbox,
    #[serde(rename = "radio")]
    Radio,
    #[serde(rename = "combobox")]
    ComboBox { value: String },
    #[serde(rename = "listbox")]
    ListBox {
        options: Vec<FieldOption>,
        /// `/TI`: the option in the box's top row.
        #[serde(rename = "topIndex")]
        top_index: i64,
    },
    #[serde(rename = "pushbutton")]
    PushButton,
    #[serde(rename = "signature")]
    Signature,
    #[serde(rename = "unknown")]
    Unknown,
}

/// A form field's family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldFamily {
    Text,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
    Pushbutton,
    Signature,
    Unknown,
}

impl FieldFacts {
    pub fn family(&self) -> FieldFamily {
        match self {
            FieldFacts::Text(_) => FieldFamily::Text,
            FieldFacts::Checkbox => FieldFamily::Checkbox,
            FieldFacts::Radio => FieldFamily::Radio,
            FieldFacts::ComboBox { .. } => FieldFamily::Combobox,
            FieldFacts::ListBox { .. } => FieldFamily::Listbox,
            FieldFacts::PushButton => FieldFamily::Pushbutton,
            FieldFacts::Signature => FieldFamily::Signature,
            FieldFacts::Unknown => FieldFamily::Unknown,
        }
    }
}

/// What a widget shows now, where it isn't what its field holds.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ShownValue {
    /// A text field's or combo box's text: the value as this session
    /// formatted it.
    pub text: Option<String>,
    /// Whether a check box or radio button is on.
    pub checked: Option<bool>,
}

/// A colour member as the TypeScript tests it: an empty string is falsy.
fn color_of(hex: &Option<String>) -> Option<Rgb> {
    hex.as_deref().filter(|hex| !hex.is_empty()).map(rgb_of)
}

/// The upright frame: the box's size, from the origin.
fn bbox_of(row: &WidgetRow) -> PdfRect {
    let frame = row.frame();
    PdfRect { left: 0.0, bottom: 0.0, right: frame.width, top: frame.height }
}

/// The look of a widget with text, and whether the painter draws its font
/// only approximately: a font that isn't one of the 14 lays out with
/// Helvetica's numbers (close, not exact).
pub(crate) fn text_look_of(row: &WidgetRow) -> (TextLook, Option<Approximation>) {
    let standard = layout_font_of(row.font_family.as_deref());
    let font: LayoutFont = standard
        .unwrap_or_else(|| layout_font_of(Some("helvetica")).expect("helvetica is standard"));
    let approximate = match standard {
        None => Some(Approximation::Font),
        Some(font) if matches!(font.font, StandardFont::Symbol | StandardFont::ZapfDingbats) => {
            Some(Approximation::SymbolicFont)
        }
        Some(_) => None,
    };
    let look = TextLook {
        bbox: bbox_of(row),
        border: border_style_info(row),
        background: color_of(&row.interior_color),
        border_color: color_of(&row.color),
        font,
        font_size: row.font_size.unwrap_or(0.0),
        text_color: color_of(&row.font_color),
        alignment: row.text_align.unwrap_or_default().into(),
    };
    (look, approximate)
}

/// Whether the painter draws this family's widgets: not a signature, nor a
/// family the engine doesn't know.
pub fn paints(family: FieldFamily) -> bool {
    !matches!(family, FieldFamily::Signature | FieldFamily::Unknown)
}

/// The drawing, or `None` for a widget the painter doesn't draw (a signature,
/// a family the engine doesn't know).
pub fn widget_drawing(
    row: &WidgetRow,
    field: &FieldFacts,
    shown: &ShownValue,
) -> Option<WidgetDrawing> {
    if !paints(field.family()) {
        return None;
    }
    let frame = row.frame();
    let drawing = |ops, approximate| WidgetDrawing {
        width: frame.width,
        height: frame.height,
        ops,
        approximate,
    };

    if let FieldFacts::Checkbox | FieldFacts::Radio = field {
        let look = ToggleLook {
            bbox: bbox_of(row),
            border: border_style_info(row),
            background: color_of(&row.interior_color),
            border_color: color_of(&row.color),
            symbol_color: color_of(&row.font_color),
            check_style: row.check_style,
        };
        let checked = shown
            .checked
            .unwrap_or_else(|| row.appearance_state.as_deref().is_some_and(|state| state != "Off"));
        let ops = if let FieldFacts::Checkbox = field {
            checkbox_ops(&look, checked)
        } else {
            radio_ops(&look, checked)
        };
        return Some(drawing(ops, None));
    }

    let (look, approximate) = text_look_of(row);
    let drawn = match field {
        FieldFacts::Text(facts) => {
            text_field_ops(&look, facts, shown.text.as_deref().unwrap_or(&facts.value))
        }
        FieldFacts::ComboBox { value } => {
            combo_box_ops(&look, shown.text.as_deref().unwrap_or(value))
        }
        FieldFacts::ListBox { options, top_index } => list_box_ops(&look, options, *top_index),
        _ => push_button_ops(&look, row.caption.as_deref()),
    };
    let approximate = match approximate {
        None if drawn.approximate => Some(Approximation::Glyph),
        other => other,
    };
    Some(drawing(drawn.ops, approximate))
}
