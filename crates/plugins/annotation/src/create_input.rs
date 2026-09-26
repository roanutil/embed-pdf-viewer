//! `create-input.ts`: page-space creation input, the data API's vocabulary.
//! Geometry is in the unrotated page's frame (origin at the crop box's top
//! left, y down), the frame every pointer sample and selection quad uses.
//! Tool defaults apply first (`tool`, or the subtype's own preset), then
//! `props`.

use epdf_annotation_core::{
    AnnotationFlagsPatch, AnnotationPropsPatch, Callout, Geom, PageRef, Point, Rect, TextQuad,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde::Deserialize;
use serde_json::Value;

/// `CreateAnnotationInput`. The geometry members are read per subtype by
/// [`geometry_from_input`], as the TypeScript's discriminated union is; the
/// input keeps them as JSON so a malformed one fails the way it does there.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CreateAnnotationInput {
    pub page: PageRef,
    pub subtype: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub props: Option<AnnotationPropsPatch>,
    #[serde(default)]
    pub flags: Option<AnnotationFlagsPatch>,
    #[serde(default)]
    pub select: Option<bool>,
    #[serde(default)]
    pub bounds: Option<Value>,
    #[serde(default)]
    pub rotation: Option<f64>,
    #[serde(default)]
    pub from: Option<Point>,
    #[serde(default)]
    pub to: Option<Point>,
    #[serde(default)]
    pub vertices: Option<Vec<Point>>,
    #[serde(default)]
    pub strokes: Option<Vec<Vec<Point>>>,
    #[serde(default)]
    pub quads: Option<Vec<TextQuad>>,
    #[serde(default)]
    pub callout: Option<Callout>,
}

fn invalid(message: impl Into<String>) -> Error {
    plugin_error(code::INVALID_INPUT, "annotation", message.into(), None, None)
}

/// A JavaScript `TypeError` for a member read off `undefined`.
fn read_of_undefined(member: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of undefined (reading '{member}')"))
}

fn finite_point(p: &Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// The bounds as a rectangle; a member that is not a number reads `NaN`.
fn rect_of(bounds: Option<&Value>) -> Result<Rect, Error> {
    let b = bounds.ok_or_else(|| read_of_undefined("x"))?;
    let n = |k: &str| b.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    Ok(Rect { x: n("x"), y: n("y"), width: n("width"), height: n("height") })
}

fn finite_rect(r: &Rect) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
}

/// `geometryFromInput(input)`: page-space input to the core's content-space
/// `Geom`, validating finiteness and arity.
pub fn geometry_from_input(input: &CreateAnnotationInput) -> Result<(String, Geom), Error> {
    match input.subtype.as_str() {
        "square" | "circle" => {
            let r = rect_of(input.bounds.as_ref())?;
            if !finite_rect(&r) || r.width < 0.0 || r.height < 0.0 {
                return Err(invalid("bounds must be a finite, non-negative rectangle"));
            }
            let rot = input.rotation.filter(|r| *r != 0.0 && !r.is_nan());
            Ok((
                input.subtype.clone(),
                Geom::Rect { rect: r, ellipse: input.subtype == "circle", rot },
            ))
        }
        "line" => {
            let (Some(a), Some(b)) = (input.from, input.to) else {
                return Err(read_of_undefined("x"));
            };
            if !finite_point(&a) || !finite_point(&b) {
                return Err(invalid("from and to must be finite points"));
            }
            Ok(("line".into(), Geom::Line { a, b, ends: None, rot: None }))
        }
        "polygon" | "polyline" => {
            let min = if input.subtype == "polygon" { 3 } else { 2 };
            let vertices = input.vertices.as_ref().ok_or_else(|| read_of_undefined("length"))?;
            if vertices.len() < min || !vertices.iter().all(finite_point) {
                return Err(invalid(format!(
                    "{} needs at least {min} finite vertices",
                    input.subtype
                )));
            }
            Ok((
                input.subtype.clone(),
                Geom::Poly {
                    points: vertices.clone(),
                    closed: input.subtype == "polygon",
                    ends: None,
                    rot: None,
                },
            ))
        }
        "ink" => {
            let strokes = input.strokes.as_ref().ok_or_else(|| read_of_undefined("filter"))?;
            let strokes: Vec<Vec<Point>> =
                strokes.iter().filter(|s| s.len() >= 2).cloned().collect();
            if strokes.is_empty() || !strokes.iter().all(|s| s.iter().all(finite_point)) {
                return Err(invalid("ink needs at least one stroke of two or more finite points"));
            }
            Ok(("ink".into(), Geom::Ink { strokes, rot: None }))
        }
        "highlight" | "underline" | "strikeout" | "squiggly" | "redact" => {
            let Some(quads) = &input.quads else {
                let r = rect_of(input.bounds.as_ref())?;
                if !finite_rect(&r) || r.width <= 0.0 || r.height <= 0.0 {
                    return Err(invalid("bounds must be a finite rectangle with positive size"));
                }
                return Ok(("redact".into(), Geom::Rect { rect: r, ellipse: false, rot: None }));
            };
            if quads.is_empty() {
                return Err(invalid(format!("{} needs at least one quad", input.subtype)));
            }
            Ok((input.subtype.clone(), Geom::Quads { quads: quads.clone() }))
        }
        "free-text" => {
            let r = rect_of(input.bounds.as_ref())?;
            if !finite_rect(&r) || r.width <= 0.0 || r.height <= 0.0 {
                return Err(invalid("bounds must be a finite rectangle with positive size"));
            }
            Ok(("free-text".into(), Geom::Text { rect: r, callout: input.callout, rot: None }))
        }
        other => Err(plugin_error(
            code::UNSUPPORTED,
            "annotation",
            format!(
                "create() does not build '{other}' annotations yet; use createRaw(), placeStamp() or createAttachment()"
            ),
            None,
            None,
        )),
    }
}
