//! `drawing/text.ts`: the families with text. A text field
//! (`GenerateTextFieldFormAP` and `GenerateTextFieldAP`), a combo box
//! (`GenerateComboBoxFormAP`, `GenerateComboBoxAP`), a list box
//! (`GenerateListBoxFormAP`, `GenerateListBoxAP`) and a push button's caption
//! (`GeneratePushButtonFormAP`). Each draws what its generator function
//! writes, in the same order.

use epdf_geometry::js;

use super::border::{BLACK, BorderStyleInfo, border_ops, box_ops, deflate, rect_path};
use super::{DrawOp, FieldOption, PathSegment, Rgb, TextFieldFacts, TextRun, WidgetBorderStyle};
use crate::layout::text_runs;
use crate::layout_variable_text;
use crate::{Alignment, LayoutFont, PdfRect, TextLayout, VariableTextOptions, fround};

fn is_float_zero(value: f64) -> bool {
    value < 0.0001 && value > -0.0001
}

fn is_float_bigger(left: f64, right: f64) -> bool {
    left > right && !is_float_zero(fround(left - right))
}

fn is_float_smaller(left: f64, right: f64) -> bool {
    left < right && !is_float_zero(fround(left - right))
}

/// What every text family reads from its widget.
#[derive(Clone, Debug)]
pub(crate) struct TextLook {
    pub bbox: PdfRect,
    pub border: BorderStyleInfo,
    pub background: Option<Rgb>,
    pub border_color: Option<Rgb>,
    pub font: LayoutFont,
    pub font_size: f64,
    pub text_color: Option<Rgb>,
    pub alignment: Alignment,
}

pub(crate) struct Drawn {
    pub ops: Vec<DrawOp>,
    /// A character the font can't write.
    pub approximate: bool,
}

fn content_height(layout: &TextLayout) -> f64 {
    fround(layout.content.top - layout.content.bottom)
}

fn plate_height(plate: PdfRect) -> f64 {
    fround(plate.top - plate.bottom)
}

fn text_op(look: &TextLook, layout: &TextLayout, runs: Vec<TextRun>, color: Rgb) -> DrawOp {
    DrawOp::Text { font: look.font.font, size: layout.font_size, color, runs }
}

/// One line of text the generator lays out in `plate`, aligned and sized as
/// it says, without a limit or cells.
fn one_line(
    text: &str,
    look: &TextLook,
    plate: PdfRect,
    size: f64,
    alignment: Alignment,
) -> TextLayout {
    layout_variable_text(
        text,
        &look.font,
        &VariableTextOptions {
            plate,
            font_size: size,
            alignment,
            multiline: false,
            substitute: None,
            limit: 0,
            cells: 0,
        },
    )
}

/// A text field's text as the generator lays it out, in its plate: the box
/// inside the border.
pub(crate) fn text_field_layout(
    look: &TextLook,
    field: &TextFieldFacts,
    value: &str,
) -> (PdfRect, TextLayout) {
    let width = look.border.width;
    let plate = deflate(look.bbox, fround(2.0 * width), width);
    let max_length = field.max_length.unwrap_or(0);
    let layout = layout_variable_text(
        value,
        &look.font,
        &VariableTextOptions {
            plate,
            font_size: look.font_size,
            alignment: look.alignment,
            multiline: field.multiline,
            substitute: field.password.then_some('*'),
            limit: if field.comb { 0 } else { max_length },
            cells: if field.comb { max_length } else { 0 },
        },
    );
    (plate, layout)
}

pub(crate) fn text_field_ops(look: &TextLook, field: &TextFieldFacts, value: &str) -> Drawn {
    let bbox = look.bbox;
    let width = look.border.width;
    let mut ops = Vec::new();
    if let Some(background) = look.background {
        ops.push(DrawOp::Fill { path: vec![rect_path(bbox)], color: background, even_odd: true });
    }
    if look.border.style == WidgetBorderStyle::Solid {
        if let Some(border_color) = look.border_color
            && width > 0.0
        {
            let half = fround(width / 2.0);
            ops.push(DrawOp::Stroke {
                path: vec![rect_path(deflate(bbox, half, half))],
                color: border_color,
                width,
                dash: None,
            });
        }
    } else {
        ops.extend(border_ops(bbox, &look.border, look.border_color));
    }
    let (body, layout) = text_field_layout(look, field, value);
    let offset = if field.multiline {
        (0.0, 0.0)
    } else {
        (0.0, fround(fround(content_height(&layout) - plate_height(body)) / 2.0))
    };
    let runs = text_runs(&layout, offset, !field.comb);
    let clipped = if runs.is_empty() {
        Vec::new()
    } else {
        vec![text_op(look, &layout, runs, look.text_color.unwrap_or(BLACK))]
    };
    ops.push(DrawOp::Clip { rect: deflate(bbox, width, width), ops: clipped });
    Drawn { ops, approximate: layout.approximate }
}

/// `{...rect}` normalized: each side the lesser or greater of its pair.
fn normalized(rect: PdfRect) -> PdfRect {
    PdfRect {
        left: js::min(rect.left, rect.right),
        right: js::max(rect.left, rect.right),
        bottom: js::min(rect.bottom, rect.top),
        top: js::max(rect.bottom, rect.top),
    }
}

fn is_empty(rect: PdfRect) -> bool {
    rect.left >= rect.right || rect.bottom >= rect.top
}

pub(crate) fn combo_box_ops(look: &TextLook, value: &str) -> Drawn {
    let bbox = look.bbox;
    let border = &look.border;
    let mut ops = Vec::new();
    if let Some(background) = look.background {
        ops.push(DrawOp::Fill { path: vec![rect_path(bbox)], color: background, even_odd: false });
    }
    ops.extend(border_ops(bbox, border, look.border_color));
    let body = deflate(bbox, border.width, border.width);
    let button = normalized(PdfRect { left: fround(body.right - 13.0), ..body });
    let edit = deflate(normalized(PdfRect { right: button.left, ..body }), 4.0, 0.0);
    let layout = one_line(value, look, edit, look.font_size, Alignment::Left);
    let offset = (0.0, fround(fround(content_height(&layout) - plate_height(edit)) / 2.0));
    let runs = text_runs(&layout, offset, true);
    if !runs.is_empty() {
        ops.push(DrawOp::Clip {
            rect: edit,
            ops: vec![text_op(look, &layout, runs, look.text_color.unwrap_or(BLACK))],
        });
    }
    if !is_empty(button) {
        let center_x = fround(fround(button.left + button.right) / 2.0);
        let center_y = fround(fround(button.top + button.bottom) / 2.0);
        let width = fround(button.right - button.left);
        let height = fround(button.top - button.bottom);
        if is_float_bigger(width, 6.0) && is_float_bigger(height, 6.0) {
            ops.push(DrawOp::Fill {
                path: vec![
                    PathSegment::Move { x: fround(center_x - 3.0), y: fround(center_y + 1.5) },
                    PathSegment::Line { x: fround(center_x + 3.0), y: fround(center_y + 1.5) },
                    PathSegment::Line { x: center_x, y: fround(center_y - 1.5) },
                    PathSegment::Line { x: fround(center_x - 3.0), y: fround(center_y + 1.5) },
                ],
                color: BLACK,
                even_odd: false,
            });
        }
    }
    Drawn { ops, approximate: layout.approximate }
}

/// The list box's selected rows: PDFium's form highlight.
const SELECTED_ROW: Rgb = [0.0, 51.0 / 255.0, 113.0 / 255.0];
const WHITE: Rgb = [1.0, 1.0, 1.0];

pub(crate) fn list_box_ops(look: &TextLook, options: &[FieldOption], top_index: i64) -> Drawn {
    let bbox = look.bbox;
    let border = &look.border;
    let mut ops = Vec::new();
    if let Some(background) = look.background {
        ops.push(DrawOp::Fill { path: vec![rect_path(bbox)], color: background, even_odd: false });
    }
    ops.extend(border_ops(bbox, border, look.border_color));
    let body = deflate(bbox, border.width, border.width);
    let mut rows = Vec::new();
    let mut approximate = false;
    let mut y = body.top;
    // Text without a colour of its own paints in the colour the last row left.
    let mut fill = BLACK;
    // PDFium counts rows from `/TI` as an unsigned number: a negative one draws none.
    let start = usize::try_from(top_index).unwrap_or(options.len());
    let size =
        if look.font_size == 0.0 || is_float_zero(look.font_size) { 12.0 } else { look.font_size };
    for option in options.iter().skip(start) {
        if is_float_smaller(y, body.bottom) {
            break;
        }
        let plate = PdfRect { left: body.left, bottom: 0.0, right: body.right, top: 0.0 };
        let layout = one_line(&option.label, look, plate, size, Alignment::Left);
        approximate |= layout.approximate;
        let height = content_height(&layout);
        let runs = text_runs(&layout, (0.0, y), true);
        if option.selected {
            rows.push(DrawOp::Fill {
                path: vec![rect_path(PdfRect {
                    left: body.left,
                    bottom: fround(y - height),
                    right: body.right,
                    top: y,
                })],
                color: SELECTED_ROW,
                even_odd: false,
            });
            rows.push(text_op(look, &layout, runs, WHITE));
            fill = WHITE;
        } else {
            fill = look.text_color.unwrap_or(fill);
            rows.push(text_op(look, &layout, runs, fill));
        }
        y = fround(y - height);
    }
    if !rows.is_empty() {
        ops.push(DrawOp::Clip { rect: body, ops: rows });
    }
    Drawn { ops, approximate }
}

pub(crate) fn push_button_ops(look: &TextLook, caption: Option<&str>) -> Drawn {
    let bbox = look.bbox;
    let border = &look.border;
    let mut ops = box_ops(bbox, border, look.background, look.border_color);
    let Some(caption) = caption.filter(|caption| !caption.is_empty()) else {
        return Drawn { ops, approximate: false };
    };
    let body = deflate(bbox, fround(2.0 * border.width), border.width);
    let layout = one_line(caption, look, body, look.font_size, Alignment::Center);
    let offset = (0.0, fround(fround(content_height(&layout) - plate_height(body)) / 2.0));
    let runs = text_runs(&layout, offset, true);
    if !runs.is_empty() {
        ops.push(DrawOp::Clip {
            rect: deflate(bbox, border.width, border.width),
            ops: vec![text_op(look, &layout, runs, look.text_color.unwrap_or(BLACK))],
        });
    }
    Drawn { ops, approximate: layout.approximate }
}
