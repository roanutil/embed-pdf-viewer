//! `drawing/toggle.ts`: check boxes and radio buttons
//! (`GenerateCheckboxFormAP`, `GenerateRadioButtonFormAP`). The box or the
//! circle, and, when on, the symbol `/MK /CA` names (`WriteCheckSymbol`), in
//! the `/DA` colour.

use std::f64::consts::PI;

use super::border::{BLACK, BorderStyleInfo, box_ops, deflate};
use super::{DrawOp, PathSegment, Rgb, WidgetCheckStyle};
use crate::{PdfRect, fround};

/// PDFium's `FXSYS_BEZIER`: a quarter circle's control point distance.
fn bezier() -> f64 {
    fround(0.5522847498308)
}

/// The radio button generator's own kappa.
fn kappa() -> f64 {
    fround(0.5523)
}

pub(crate) struct ToggleLook {
    pub bbox: PdfRect,
    pub border: BorderStyleInfo,
    pub background: Option<Rgb>,
    pub border_color: Option<Rgb>,
    /// The `/DA` colour; the symbol is black without one.
    pub symbol_color: Option<Rgb>,
    pub check_style: Option<WidgetCheckStyle>,
}

pub(crate) fn checkbox_ops(look: &ToggleLook, checked: bool) -> Vec<DrawOp> {
    let mut ops = box_ops(look.bbox, &look.border, look.background, look.border_color);
    if !checked {
        return ops;
    }
    let style = look.check_style.unwrap_or(WidgetCheckStyle::Check);
    let body = deflate(look.bbox, look.border.width, look.border.width);
    ops.push(symbol_op(look, style, symbol_rect(body, style, false)));
    ops
}

pub(crate) fn radio_ops(look: &ToggleLook, checked: bool) -> Vec<DrawOp> {
    let style = look.check_style.unwrap_or(WidgetCheckStyle::Circle);
    if style != WidgetCheckStyle::Circle {
        // Any other symbol sits in a box, as PDFium's form filler draws it.
        let mut ops = box_ops(look.bbox, &look.border, look.background, look.border_color);
        if checked {
            let width = look.border.width;
            ops.push(symbol_op(
                look,
                style,
                symbol_rect(deflate(look.bbox, width, width), style, true),
            ));
        }
        return ops;
    }
    let bbox = look.bbox;
    let width = look.border.width;
    let half = fround(width / 2.0);
    let center_x = fround(fround(bbox.left + bbox.right) / 2.0);
    let center_y = fround(fround(bbox.bottom + bbox.top) / 2.0);
    let radius_x = fround(fround(bbox.right - bbox.left) / 2.0);
    let radius_y = fround(fround(bbox.top - bbox.bottom) / 2.0);
    let mut ops = Vec::new();
    let ring = ellipse(center_x, center_y, fround(radius_x - half), fround(radius_y - half));
    if let Some(background) = look.background {
        ops.push(DrawOp::Fill { path: ring.clone(), color: background, even_odd: true });
    }
    if let Some(border_color) = look.border_color
        && width > 0.0
    {
        ops.push(DrawOp::Stroke { path: ring, color: border_color, width, dash: None });
    }
    if checked {
        ops.push(DrawOp::Fill {
            path: ellipse(
                center_x,
                center_y,
                fround(fround(radius_x - width) * 0.5),
                fround(fround(radius_y - width) * 0.5),
            ),
            color: look.symbol_color.unwrap_or(BLACK),
            even_odd: true,
        });
    }
    ops
}

/// The radio generator's ellipse: from the top, clockwise, four curves,
/// closed.
fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathSegment> {
    let dx = fround(kappa() * rx);
    let dy = fround(kappa() * ry);
    vec![
        PathSegment::Move { x: cx, y: fround(cy + ry) },
        PathSegment::Curve {
            x1: fround(cx + dx),
            y1: fround(cy + ry),
            x2: fround(cx + rx),
            y2: fround(cy + dy),
            x: fround(cx + rx),
            y: cy,
        },
        PathSegment::Curve {
            x1: fround(cx + rx),
            y1: fround(cy - dy),
            x2: fround(cx + dx),
            y2: fround(cy - ry),
            x: cx,
            y: fround(cy - ry),
        },
        PathSegment::Curve {
            x1: fround(cx - dx),
            y1: fround(cy - ry),
            x2: fround(cx - rx),
            y2: fround(cy - dy),
            x: fround(cx - rx),
            y: cy,
        },
        PathSegment::Curve {
            x1: fround(cx - rx),
            y1: fround(cy + dy),
            x2: fround(cx - dx),
            y2: fround(cy + ry),
            x: cx,
            y: fround(cy + ry),
        },
        PathSegment::Close,
    ]
}

/// `CheckSymbolRect`: the body's middle square, shrunk for the filled shapes.
fn symbol_rect(body: PdfRect, style: WidgetCheckStyle, radio: bool) -> PdfRect {
    let width = fround(body.right - body.left);
    let height = fround(body.top - body.bottom);
    let half = if width > height { fround(height / 2.0) } else { fround(width / 2.0) };
    let center_x = fround(fround(body.left + body.right) / 2.0);
    let center_y = fround(fround(body.top + body.bottom) / 2.0);
    let square = PdfRect {
        left: fround(center_x - half),
        bottom: fround(center_y - half),
        right: fround(center_x + half),
        top: fround(center_y + half),
    };
    let scale = match style {
        WidgetCheckStyle::Circle if radio => fround(1.0 / 2.0),
        WidgetCheckStyle::Circle => fround(2.0 / 3.0),
        WidgetCheckStyle::Check | WidgetCheckStyle::Cross => 1.0,
        _ => fround(2.0 / 3.0),
    };
    if scale == 1.0 {
        return square;
    }
    let half_width = fround(fround(square.right - square.left) / 2.0);
    let half_height = fround(fround(square.top - square.bottom) / 2.0);
    let middle_x = fround(fround(square.left + square.right) / 2.0);
    let middle_y = fround(fround(square.top + square.bottom) / 2.0);
    PdfRect {
        left: fround(middle_x - fround(half_width * scale)),
        bottom: fround(middle_y - fround(half_height * scale)),
        right: fround(middle_x + fround(half_width * scale)),
        top: fround(middle_y + fround(half_height * scale)),
    }
}

fn symbol_op(look: &ToggleLook, style: WidgetCheckStyle, rect: PdfRect) -> DrawOp {
    let color = look.symbol_color.unwrap_or(BLACK);
    let mid_x = fround(fround(rect.left + rect.right) / 2.0);
    let mid_y = fround(fround(rect.bottom + rect.top) / 2.0);
    let fill = |path| DrawOp::Fill { path, color, even_odd: false };
    match style {
        WidgetCheckStyle::Check => fill(checkmark(rect)),
        WidgetCheckStyle::Circle => {
            let rx = fround(fround(rect.right - rect.left) / 2.0);
            let ry = fround(fround(rect.top - rect.bottom) / 2.0);
            let b = bezier();
            let (left_x, left_y) = (rect.left, mid_y);
            let (top_x, top_y) = (mid_x, rect.top);
            let (right_x, right_y) = (rect.right, mid_y);
            let (bottom_x, bottom_y) = (mid_x, rect.bottom);
            fill(vec![
                PathSegment::Move { x: left_x, y: left_y },
                PathSegment::Curve {
                    x1: left_x,
                    y1: fround(left_y + fround(ry * b)),
                    x2: fround(top_x - fround(rx * b)),
                    y2: top_y,
                    x: top_x,
                    y: top_y,
                },
                PathSegment::Curve {
                    x1: fround(top_x + fround(rx * b)),
                    y1: top_y,
                    x2: right_x,
                    y2: fround(right_y + fround(ry * b)),
                    x: right_x,
                    y: right_y,
                },
                PathSegment::Curve {
                    x1: right_x,
                    y1: fround(right_y - fround(ry * b)),
                    x2: fround(bottom_x + fround(rx * b)),
                    y2: bottom_y,
                    x: bottom_x,
                    y: bottom_y,
                },
                PathSegment::Curve {
                    x1: fround(bottom_x - fround(rx * b)),
                    y1: bottom_y,
                    x2: left_x,
                    y2: fround(left_y - fround(ry * b)),
                    x: left_x,
                    y: left_y,
                },
            ])
        }
        // Stroked with the line width the box's border left set (1 without a border).
        WidgetCheckStyle::Cross => DrawOp::Stroke {
            path: vec![
                PathSegment::Move { x: rect.left, y: rect.top },
                PathSegment::Line { x: rect.right, y: rect.bottom },
                PathSegment::Move { x: rect.left, y: rect.bottom },
                PathSegment::Line { x: rect.right, y: rect.top },
            ],
            color,
            width: if look.border_color.is_some() && look.border.width > 0.0 {
                look.border.width
            } else {
                1.0
            },
            dash: None,
        },
        WidgetCheckStyle::Diamond => fill(polygon(&[
            (rect.left, mid_y),
            (mid_x, rect.top),
            (rect.right, mid_y),
            (mid_x, rect.bottom),
        ])),
        WidgetCheckStyle::Square => fill(polygon(&[
            (rect.left, rect.top),
            (rect.right, rect.top),
            (rect.right, rect.bottom),
            (rect.left, rect.bottom),
        ])),
        WidgetCheckStyle::Star => {
            // Five points joined every second one, filled by the nonzero rule.
            let radius =
                fround(fround(rect.top - rect.bottom) / fround(1.0 + fround((PI / 5.0).cos())));
            let mut tips = Vec::with_capacity(5);
            let mut angle = fround(PI / 10.0);
            for _ in 0..5 {
                tips.push((
                    fround(mid_x + fround(radius * fround(angle.cos()))),
                    fround(mid_y + fround(radius * fround(angle.sin()))),
                ));
                angle = fround(angle + fround((PI * 2.0) / 5.0));
            }
            fill(polygon(&[tips[0], tips[2], tips[4], tips[1], tips[3]]))
        }
    }
}

fn polygon(points: &[(f64, f64)]) -> Vec<PathSegment> {
    let mut path: Vec<PathSegment> = points
        .iter()
        .enumerate()
        .map(
            |(index, &(x, y))| {
                if index == 0 { PathSegment::Move { x, y } } else { PathSegment::Line { x, y } }
            },
        )
        .collect();
    path.push(PathSegment::Close);
    path
}

/// `GenerateCheckmarkPath`: PDFium's check, eight curves through the rect.
const CHECK: [[(f64, f64); 3]; 8] = [
    [(0.28, 0.52), (0.27, 0.48), (0.29, 0.4)],
    [(0.3, 0.33), (0.31, 0.29), (0.31, 0.28)],
    [(0.39, 0.28), (0.49, 0.29), (0.77, 0.67)],
    [(0.76, 0.68), (0.78, 0.69), (0.76, 0.75)],
    [(0.76, 0.75), (0.73, 0.8), (0.68, 0.75)],
    [(0.68, 0.74), (0.68, 0.74), (0.44, 0.47)],
    [(0.43, 0.47), (0.4, 0.47), (0.41, 0.58)],
    [(0.4, 0.6), (0.28, 0.66), (0.3, 0.56)],
];

fn checkmark(rect: PdfRect) -> Vec<PathSegment> {
    let width = fround(rect.right - rect.left);
    let height = fround(rect.top - rect.bottom);
    let points: Vec<[(f64, f64); 3]> = CHECK
        .iter()
        .map(|row| {
            row.map(|(x, y)| {
                (
                    fround(fround(fround(x) * width) + rect.left),
                    fround(fround(fround(y) * height) + rect.bottom),
                )
            })
        })
        .collect();
    let b = bezier();
    let (start_x, start_y) = points[0][0];
    let mut path = vec![PathSegment::Move { x: start_x, y: start_y }];
    for (index, row) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()][0];
        let (x0, y0) = row[0];
        let px1 = fround(row[1].0 - x0);
        let py1 = fround(row[1].1 - y0);
        let px2 = fround(row[2].0 - next.0);
        let py2 = fround(row[2].1 - next.1);
        path.push(PathSegment::Curve {
            x1: fround(x0 + fround(px1 * b)),
            y1: fround(y0 + fround(py1 * b)),
            x2: fround(next.0 + fround(px2 * b)),
            y2: fround(next.1 + fround(py2 * b)),
            x: next.0,
            y: next.1,
        });
    }
    path
}
