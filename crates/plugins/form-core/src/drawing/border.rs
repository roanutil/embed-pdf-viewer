//! `drawing/border.ts`: the pieces every widget's appearance shares, ported
//! from the generator. Rectangles as PDFium shrinks them, the border style's
//! numbers (`GetBorderStyleInfo`: a beveled or inset border counts double),
//! the border itself (`GenerateBorderAP`), and the box a check box or a push
//! button draws (`BuildCheckboxBoxStream`).

use epdf_geometry::js;

use super::{DrawOp, PathSegment, Rgb, WidgetBorderStyle, WidgetRow};
use crate::{PdfRect, fround};

/// ISO 32000-1 Table 166: a dashed border that names no dashes draws 3 on, 3
/// off.
const DEFAULT_DASH: [f64; 1] = [3.0];

/// `CFX_FloatRect::Deflate`: normalized, then shrunk; it may cross itself.
pub(crate) fn deflate(rect: PdfRect, x: f64, y: f64) -> PdfRect {
    let left = js::min(rect.left, rect.right);
    let right = js::max(rect.left, rect.right);
    let bottom = js::min(rect.bottom, rect.top);
    let top = js::max(rect.bottom, rect.top);
    PdfRect {
        left: fround(left + x),
        bottom: fround(bottom + y),
        right: fround(right - x),
        top: fround(top - y),
    }
}

pub(crate) fn rect_path(rect: PdfRect) -> PathSegment {
    PathSegment::Rect {
        x: rect.left,
        y: rect.bottom,
        width: fround(rect.right - rect.left),
        height: fround(rect.top - rect.bottom),
    }
}

/// `parseInt(pair, 16)`: the leading hex digits of `pair`, NaN for none.
fn hex_pair(pair: &str) -> f64 {
    let digits: String = pair.chars().take_while(char::is_ascii_hexdigit).collect();
    u32::from_str_radix(&digits, 16).map_or(f64::NAN, f64::from)
}

/// `rgbOf`: a `#rrggbb` colour's channels, each pair read as `parseInt(pair,
/// 16)` reads it. The DTO's schema allows only `#rrggbb`, so nothing else is
/// pinned.
pub(crate) fn rgb_of(hex: &str) -> Rgb {
    let units: Vec<char> = hex.chars().collect();
    let slice = |from: usize| -> String { units.iter().skip(from).take(2).collect() };
    [hex_pair(&slice(1)) / 255.0, hex_pair(&slice(3)) / 255.0, hex_pair(&slice(5)) / 255.0]
}

pub(crate) const BLACK: Rgb = [0.0, 0.0, 0.0];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BorderStyleInfo {
    pub width: f64,
    pub style: WidgetBorderStyle,
    pub dash: Vec<f64>,
}

/// The border as the generator reads `/BS`: width 1 unless it says, a
/// beveled or inset one doubled.
pub(crate) fn border_style_info(row: &WidgetRow) -> BorderStyleInfo {
    let style = row.border_style.unwrap_or(WidgetBorderStyle::Solid);
    let width = row.stroke_width.unwrap_or(1.0);
    BorderStyleInfo {
        width: if matches!(style, WidgetBorderStyle::Beveled | WidgetBorderStyle::Inset) {
            width * 2.0
        } else {
            width
        },
        style,
        dash: match &row.dash_array {
            Some(dash) if !dash.is_empty() => dash.iter().take(10).copied().collect(),
            _ => DEFAULT_DASH.to_vec(),
        },
    }
}

fn polyline(points: &[(f64, f64)]) -> Vec<PathSegment> {
    points
        .iter()
        .enumerate()
        .map(
            |(index, &(x, y))| {
                if index == 0 { PathSegment::Move { x, y } } else { PathSegment::Line { x, y } }
            },
        )
        .collect()
}

/// `GenerateBorderAP`: a combo box's, a list box's, and a text field's that
/// isn't solid.
pub(crate) fn border_ops(rect: PdfRect, info: &BorderStyleInfo, color: Option<Rgb>) -> Vec<DrawOp> {
    let width = info.width;
    if width <= 0.0 {
        return Vec::new();
    }
    let PdfRect { left, bottom, right, top } = rect;
    let half = fround(width / 2.0);
    match info.style {
        WidgetBorderStyle::Solid => color
            .map(|color| DrawOp::Fill {
                path: vec![rect_path(rect), rect_path(deflate(rect, width, width))],
                color,
                even_odd: true,
            })
            .into_iter()
            .collect(),
        WidgetBorderStyle::Dashed => color
            .map(|color| DrawOp::Stroke {
                path: polyline(&[
                    (fround(left + half), fround(bottom + half)),
                    (fround(left + half), fround(top - half)),
                    (fround(right - half), fround(top - half)),
                    (fround(right - half), fround(bottom + half)),
                    (fround(left + half), fround(bottom + half)),
                ]),
                color,
                width,
                dash: Some(info.dash.clone()),
            })
            .into_iter()
            .collect(),
        WidgetBorderStyle::Beveled | WidgetBorderStyle::Inset => {
            let beveled = info.style == WidgetBorderStyle::Beveled;
            let light_gray = if beveled { 1.0 } else { 0.5 };
            let dark_gray = if beveled { 0.5 } else { 0.75 };
            let mut ops = vec![
                DrawOp::Fill {
                    path: polyline(&[
                        (fround(left + half), fround(bottom + half)),
                        (fround(left + half), fround(top - half)),
                        (fround(right - half), fround(top - half)),
                        (fround(right - width), fround(top - width)),
                        (fround(left + width), fround(top - width)),
                        (fround(left + width), fround(bottom + width)),
                    ]),
                    color: [light_gray; 3],
                    even_odd: false,
                },
                DrawOp::Fill {
                    path: polyline(&[
                        (fround(right - half), fround(top - half)),
                        (fround(right - half), fround(bottom + half)),
                        (fround(left + half), fround(bottom + half)),
                        (fround(left + width), fround(bottom + width)),
                        (fround(right - width), fround(bottom + width)),
                        (fround(right - width), fround(top - width)),
                    ]),
                    color: [dark_gray; 3],
                    even_odd: false,
                },
            ];
            if let Some(color) = color {
                ops.push(DrawOp::Fill {
                    path: vec![rect_path(rect), rect_path(deflate(rect, half, half))],
                    color,
                    even_odd: true,
                });
            }
            ops
        }
        WidgetBorderStyle::Underline => color
            .map(|color| DrawOp::Stroke {
                path: polyline(&[(left, fround(bottom + half)), (right, fround(bottom + half))]),
                color,
                width,
                dash: None,
            })
            .into_iter()
            .collect(),
    }
}

/// `BuildCheckboxBoxStream`: the background inside the border's middle, and
/// the border, solid.
pub(crate) fn box_ops(
    bbox: PdfRect,
    info: &BorderStyleInfo,
    background: Option<Rgb>,
    border: Option<Rgb>,
) -> Vec<DrawOp> {
    let width = info.width;
    let half = fround(width / 2.0);
    let stroke = if width > 0.0 { deflate(bbox, half, half) } else { bbox };
    let mut ops = Vec::new();
    if let Some(background) = background {
        ops.push(DrawOp::Fill { path: vec![rect_path(stroke)], color: background, even_odd: true });
    }
    if let Some(border) = border
        && width > 0.0
    {
        ops.push(DrawOp::Stroke {
            path: vec![rect_path(stroke)],
            color: border,
            width,
            dash: None,
        });
    }
    ops
}
