//! `drawing/index.ts`: what a widget looks like for its facts, as the engine's
//! appearance generator draws it. The widget row gives the look (`/MK`, `/BS`,
//! `/DA`) and the box, the field gives the kind and its flags, `shown` what
//! to show now (a value this session formatted, a toggle's state), and
//! `fonts` what its text is drawn in: the widget's font, the fallbacks and the
//! last resort, as the viewer knows them.

mod border;
mod text;
mod toggle;
mod types;

use epdf_fonts::StandardFont;
use epdf_geometry::Rect;
use serde::Deserialize;

pub(crate) use text::{TextLook, text_field_layout};
pub use types::{Approximation, DrawOp, PathSegment, Rgb, TextRun, WidgetDrawing};

use self::border::{border_style_info, field_box_ops, rgb_of};
use self::text::{combo_box_ops, list_box_ops, push_button_ops, text_field_ops};
use self::toggle::{ToggleLook, checkbox_ops, radio_ops};
use crate::PdfRect;
use crate::text::{Alignment, PaintFonts, default_paint_fonts};

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
    /// The state a toggle shows: `Some(None)` for `null`, `None` when the
    /// member is absent. As in the TypeScript (`undefined !== null`), an
    /// absent state reads as on and `null` as off.
    #[serde(default, deserialize_with = "present")]
    pub appearance_state: Option<Option<String>>,
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

/// A member that is present, `null` included: `Some` of what it holds.
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
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

/// What the painter reads of a form field (`FormField`), by kind.
/// Deserializes from the field's JSON and ignores the rest of it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind")]
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

/// A form field's kind (`FormField['kind']`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
    Pushbutton,
    Signature,
    Unknown,
}

/// The kind's name before c4b286a69 renamed `family` to `kind`.
pub type FieldFamily = FieldKind;

impl FieldFacts {
    pub fn kind(&self) -> FieldKind {
        match self {
            FieldFacts::Text(_) => FieldKind::Text,
            FieldFacts::Checkbox => FieldKind::Checkbox,
            FieldFacts::Radio => FieldKind::Radio,
            FieldFacts::ComboBox { .. } => FieldKind::Combobox,
            FieldFacts::ListBox { .. } => FieldKind::Listbox,
            FieldFacts::PushButton => FieldKind::Pushbutton,
            FieldFacts::Signature => FieldKind::Signature,
            FieldFacts::Unknown => FieldKind::Unknown,
        }
    }

    /// [`FieldFacts::kind`], by its name before c4b286a69.
    pub fn family(&self) -> FieldKind {
        self.kind()
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

/// The look of a widget with text, in `fonts` (the widget's standard font
/// when none are given), and whether the painter draws its font only
/// approximately: without fonts, a font that isn't one of the 14 lays out
/// with Helvetica's numbers (close, not exact).
pub(crate) fn text_look_of(
    row: &WidgetRow,
    fonts: Option<&PaintFonts>,
) -> (TextLook, Option<Approximation>) {
    let family = row.font_family.as_deref().unwrap_or("");
    let standard = StandardFont::from_name(family);
    let approximate = match standard {
        Some(StandardFont::Symbol | StandardFont::ZapfDingbats) => {
            Some(Approximation::SymbolicFont)
        }
        Some(_) => None,
        None if fonts.is_some() => None,
        None => Some(Approximation::Font),
    };
    let look = TextLook {
        bbox: bbox_of(row),
        border: border_style_info(row),
        background: color_of(&row.interior_color),
        border_color: color_of(&row.color),
        fonts: fonts.cloned().unwrap_or_else(|| default_paint_fonts(row.font_family.as_deref())),
        font_size: row.font_size.unwrap_or(0.0),
        text_color: color_of(&row.font_color),
        alignment: row.text_align.unwrap_or_default().into(),
    };
    (look, approximate)
}

/// Whether the painter draws this field's widgets: not a kind the engine
/// doesn't know.
pub fn paints(kind: FieldKind) -> bool {
    kind != FieldKind::Unknown
}

/// Every character the painter draws for a widget, as one string: what a
/// viewer finds the widget's fonts for. A password's characters are drawn as
/// `*`; a check box or a radio button draws none.
pub fn widget_text(row: &WidgetRow, field: &FieldFacts, shown: &ShownValue) -> String {
    match field {
        FieldFacts::Text(facts) => {
            let value = shown.text.as_deref().unwrap_or(&facts.value);
            if facts.password && !value.is_empty() { "*".into() } else { value.into() }
        }
        FieldFacts::ComboBox { value } => shown.text.as_deref().unwrap_or(value).into(),
        FieldFacts::ListBox { options, .. } => {
            options.iter().map(|option| option.label.as_str()).collect::<Vec<_>>().join("\n")
        }
        FieldFacts::PushButton => row.caption.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// The drawing, or `None` for a widget the painter doesn't draw: a kind the
/// engine doesn't know. A signature field's is its box, with no mark in it.
pub fn widget_drawing(
    row: &WidgetRow,
    field: &FieldFacts,
    shown: &ShownValue,
) -> Option<WidgetDrawing> {
    widget_drawing_in(row, field, shown, None)
}

/// [`widget_drawing`] in `fonts`, as the form plugin draws a widget when it
/// knows the document's fonts.
pub fn widget_drawing_in(
    row: &WidgetRow,
    field: &FieldFacts,
    shown: &ShownValue,
    fonts: Option<&PaintFonts>,
) -> Option<WidgetDrawing> {
    if !paints(field.kind()) {
        return None;
    }
    let frame = row.frame();
    let bbox = bbox_of(row);
    let border = border_style_info(row);
    let background = color_of(&row.interior_color);
    let border_color = color_of(&row.color);
    let drawing = |ops, approximate| WidgetDrawing {
        width: frame.width,
        height: frame.height,
        ops,
        approximate,
    };

    if let FieldFacts::Signature = field {
        return Some(drawing(field_box_ops(bbox, &border, background, border_color), None));
    }

    if let FieldFacts::Checkbox | FieldFacts::Radio = field {
        let look = ToggleLook {
            bbox,
            border,
            background,
            border_color,
            symbol_color: color_of(&row.font_color),
            check_style: row.check_style,
        };
        let checked = shown.checked.unwrap_or_else(|| match &row.appearance_state {
            None => true,
            Some(state) => state.as_deref().is_some_and(|state| state != "Off"),
        });
        let ops = if let FieldFacts::Checkbox = field {
            checkbox_ops(&look, checked)
        } else {
            radio_ops(&look, checked)
        };
        return Some(drawing(ops, None));
    }

    let (look, approximate) = text_look_of(row, fonts);
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
    // A box the painter drew may be a character a font it wasn't given has.
    let approximate = match approximate {
        None if !drawn.missing.is_empty() && fonts.is_none() => Some(Approximation::Glyph),
        other => other,
    };
    Some(drawing(drawn.ops, approximate))
}
