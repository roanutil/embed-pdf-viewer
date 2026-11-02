//! The page/view transform helper over `epdf_geometry::page_transform`.
//!
//! The input is `PageTransformInput` restricted to the binding's contract: a
//! quarter-turn rotation and finite, positive size, scale, dpr and base scale.
//! A page's `/UserUnit` is folded into `scale` and `baseScale` by the caller,
//! as the viewer does. The answer carries the serialized metrics, including
//! the snapped device size, and each list mapped in its own direction.

use epdf_engine::host::HostResponse;
use epdf_geometry::{
    PageRotation, PageTransform, PageTransformInput, Point, Rect, Size, page_transform,
};
use serde::{Deserialize, Serialize};

use super::{answer, invalid, parse};
use crate::abi::Failure;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SizeArgs {
    width: f64,
    height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct InputArgs {
    page_size: SizeArgs,
    rotation: f64,
    scale: f64,
    #[serde(default)]
    base_scale: Option<f64>,
    dpr: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TransformArgs {
    input: InputArgs,
    #[serde(default)]
    page_points: Vec<Point>,
    #[serde(default)]
    page_rects: Vec<Rect>,
    #[serde(default)]
    view_points: Vec<Point>,
    #[serde(default)]
    view_rects: Vec<Rect>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TransformAnswer {
    transform: PageTransform,
    page_to_view: Vec<Point>,
    page_rects_to_view: Vec<Rect>,
    view_to_page: Vec<Point>,
    view_rects_to_page: Vec<Rect>,
}

fn finite_point(p: &Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn finite_rect(r: &Rect) -> bool {
    [r.x, r.y, r.width, r.height, r.x + r.width, r.y + r.height].iter().all(|n| n.is_finite())
}

fn checked_input(input: InputArgs) -> Result<PageTransformInput, Failure> {
    let positive = |name: &str, value: f64| {
        if value.is_finite() && value > 0.0 {
            Ok(value)
        } else {
            Err(invalid(format!("{name} must be finite and positive, not {value}")))
        }
    };
    if ![0.0, 90.0, 180.0, 270.0].contains(&input.rotation) {
        return Err(invalid(format!("rotation must be 0, 90, 180 or 270, not {}", input.rotation)));
    }
    Ok(PageTransformInput {
        page_size: Size {
            width: positive("pageSize.width", input.page_size.width)?,
            height: positive("pageSize.height", input.page_size.height)?,
        },
        rotation: PageRotation(input.rotation),
        scale: positive("scale", input.scale)?,
        base_scale: input.base_scale.map(|b| positive("baseScale", b)).transpose()?,
        dpr: positive("dpr", input.dpr)?,
    })
}

fn finite_transform(t: &PageTransform) -> bool {
    [
        t.view_width,
        t.view_height,
        t.content_width,
        t.content_height,
        t.device_width,
        t.device_height,
        t.render_scale,
        t.view_scale,
        t.base_scale,
        t.zoom,
    ]
    .iter()
    .all(|n| n.is_finite())
}

pub(super) fn apply(args: &[u8]) -> Result<HostResponse, Failure> {
    let args: TransformArgs = parse(args)?;
    let input = checked_input(args.input)?;
    let points = args.page_points.iter().chain(&args.view_points);
    let rects = args.page_rects.iter().chain(&args.view_rects);
    if !points.clone().all(finite_point) || !rects.clone().all(finite_rect) {
        return Err(invalid("a point or rect to transform is not finite"));
    }
    let transform = page_transform(input).map_err(|e| invalid(e.to_string()))?;
    if !finite_transform(&transform) {
        return Err(invalid("the transform's metrics are not finite"));
    }
    let answer_value = TransformAnswer {
        page_to_view: args.page_points.iter().map(|p| transform.page_to_view(*p)).collect(),
        page_rects_to_view: args
            .page_rects
            .iter()
            .map(|r| transform.page_to_view_rect(*r))
            .collect(),
        view_to_page: args.view_points.iter().map(|p| transform.view_to_page(*p)).collect(),
        view_rects_to_page: args
            .view_rects
            .iter()
            .map(|r| transform.view_to_page_rect(*r))
            .collect(),
        transform,
    };
    let mapped_points = answer_value.page_to_view.iter().chain(&answer_value.view_to_page);
    let mapped_rects =
        answer_value.page_rects_to_view.iter().chain(&answer_value.view_rects_to_page);
    if !mapped_points.clone().all(finite_point) || !mapped_rects.clone().all(finite_rect) {
        return Err(invalid("a transformed point or rect is not finite"));
    }
    answer(&answer_value)
}
