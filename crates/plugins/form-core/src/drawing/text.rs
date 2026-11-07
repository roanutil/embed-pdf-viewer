//! `drawing/text.ts`: the kinds with text. A text field
//! (`GenerateTextFieldFormAP` and `GenerateTextFieldAP`), a combo box
//! (`GenerateComboBoxFormAP`, `GenerateComboBoxAP`), a list box
//! (`GenerateListBoxFormAP`, `GenerateListBoxAP`) and a push button's caption
//! (`GeneratePushButtonFormAP`). Each draws what its generator function
//! writes, in the same order, its text laid out by [`crate::text`].

use epdf_geometry::js;

use super::border::{
    BLACK, BorderStyleInfo, border_ops, box_ops, deflate, field_box_ops, rect_path,
};
use super::{DrawOp, FieldOption, PathSegment, Rgb, TextFieldFacts, TextRun};
use crate::text::{Alignment, FieldText, FieldTextOptions, PaintFonts, layout_field};
use crate::{PdfRect, fround};

fn is_float_zero(value: f64) -> bool {
    value < 0.0001 && value > -0.0001
}

fn is_float_bigger(left: f64, right: f64) -> bool {
    left > right && !is_float_zero(fround(left - right))
}

fn is_float_smaller(left: f64, right: f64) -> bool {
    left < right && !is_float_zero(fround(left - right))
}

/// What every text kind reads from its widget.
#[derive(Clone, Debug)]
pub(crate) struct TextLook {
    pub bbox: PdfRect,
    pub border: BorderStyleInfo,
    pub background: Option<Rgb>,
    pub border_color: Option<Rgb>,
    pub fonts: PaintFonts,
    pub font_size: f64,
    pub text_color: Option<Rgb>,
    pub alignment: Alignment,
}

pub(crate) struct Drawn {
    pub ops: Vec<DrawOp>,
    /// Characters no font the painter has: drawn as boxes.
    pub missing: Vec<String>,
}

/// One line, as a combo box, a list box row and a caption draw it.
fn one_line(
    plate: PdfRect,
    font_size: f64,
    alignment: Alignment,
    centered: bool,
) -> FieldTextOptions {
    FieldTextOptions {
        plate,
        font_size,
        alignment,
        multiline: false,
        password: false,
        limit: 0,
        cells: 0,
        centered,
    }
}

/// Every run of the laid-out text, line by line, left to right.
fn runs_of(laid: &FieldText) -> Vec<TextRun> {
    laid.lines
        .iter()
        .flat_map(|line| {
            line.runs.iter().map(move |run| TextRun {
                x: run.x,
                y: line.baseline,
                text: run.text.clone(),
                font: run.font.clone(),
                rtl: run.rtl,
                plain: run.plain,
                width: run.width,
                joins_previous: run.joins_previous,
                joins_next: run.joins_next,
            })
        })
        .collect()
}

fn text_op(laid: &FieldText, color: Rgb) -> DrawOp {
    DrawOp::Text { size: laid.font_size, color, runs: runs_of(laid) }
}

fn has_runs(op: &DrawOp) -> bool {
    matches!(op, DrawOp::Text { runs, .. } if !runs.is_empty())
}

/// A text field's text as the generator lays it out, in its plate: the box
/// inside the border.
pub(crate) fn text_field_layout(
    look: &TextLook,
    field: &TextFieldFacts,
    value: &str,
) -> (PdfRect, FieldText) {
    let width = look.border.width;
    let plate = deflate(look.bbox, fround(2.0 * width), width);
    let max_length = field.max_length.unwrap_or(0);
    let laid = layout_field(
        value,
        &look.fonts,
        &FieldTextOptions {
            plate,
            font_size: look.font_size,
            alignment: look.alignment,
            multiline: field.multiline,
            password: field.password,
            limit: if field.comb { 0 } else { max_length },
            cells: if field.comb { max_length } else { 0 },
            // A one-line field's text has its middle at the plate's.
            centered: !field.multiline,
        },
    );
    (plate, laid)
}

pub(crate) fn text_field_ops(look: &TextLook, field: &TextFieldFacts, value: &str) -> Drawn {
    let bbox = look.bbox;
    let width = look.border.width;
    let mut ops = field_box_ops(bbox, &look.border, look.background, look.border_color);
    let (_, laid) = text_field_layout(look, field, value);
    let text = text_op(&laid, look.text_color.unwrap_or(BLACK));
    let clipped = if has_runs(&text) { vec![text] } else { Vec::new() };
    ops.push(DrawOp::Clip { rect: deflate(bbox, width, width), ops: clipped });
    Drawn { ops, missing: laid.missing }
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
    let laid =
        layout_field(value, &look.fonts, &one_line(edit, look.font_size, Alignment::Left, true));
    let text = text_op(&laid, look.text_color.unwrap_or(BLACK));
    if has_runs(&text) {
        ops.push(DrawOp::Clip { rect: edit, ops: vec![text] });
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
    Drawn { ops, missing: laid.missing }
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
    let mut missing: Vec<String> = Vec::new();
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
        // A row is one line, from the top of what is left.
        let plate = PdfRect { left: body.left, bottom: y, right: body.right, top: y };
        let laid = layout_field(
            &option.label,
            &look.fonts,
            &one_line(plate, size, Alignment::Left, false),
        );
        for character in &laid.missing {
            if !missing.contains(character) {
                missing.push(character.clone());
            }
        }
        let height = laid.content_height;
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
            rows.push(text_op(&laid, WHITE));
            fill = WHITE;
        } else {
            fill = look.text_color.unwrap_or(fill);
            rows.push(text_op(&laid, fill));
        }
        y = fround(y - height);
    }
    if !rows.is_empty() {
        ops.push(DrawOp::Clip { rect: body, ops: rows });
    }
    Drawn { ops, missing }
}

pub(crate) fn push_button_ops(look: &TextLook, caption: Option<&str>) -> Drawn {
    let bbox = look.bbox;
    let border = &look.border;
    let mut ops = box_ops(bbox, border, look.background, look.border_color);
    let Some(caption) = caption.filter(|caption| !caption.is_empty()) else {
        return Drawn { ops, missing: Vec::new() };
    };
    let body = deflate(bbox, fround(2.0 * border.width), border.width);
    let laid = layout_field(
        caption,
        &look.fonts,
        &one_line(body, look.font_size, Alignment::Center, true),
    );
    let text = text_op(&laid, look.text_color.unwrap_or(BLACK));
    if has_runs(&text) {
        ops.push(DrawOp::Clip { rect: deflate(bbox, border.width, border.width), ops: vec![text] });
    }
    Drawn { ops, missing: laid.missing }
}
