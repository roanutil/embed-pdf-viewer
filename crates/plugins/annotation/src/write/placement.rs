//! `write/placement.ts`: per-kind code for the click-to-place icon kinds
//! (note, file attachment). All tool config stays in the tool table; this
//! module only interprets it.

use epdf_annotation_core::anchor::{anchor_mode_of, anchored_geom, unanchored_geom};
use epdf_annotation_core::flags::FlagBearer;
use epdf_annotation_core::geometry::fit_stamp_box;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::{Authority, BoxShape, Model, Point, Rect, Shape, ViewEnv};
use epdf_annotation_core::update::tool_annotation;
use epdf_geometry::Size;
use epdf_kernel::Error;
use serde_json::{Value, json};

use crate::js::Object;
use crate::tools::definitions::ResolvedTool;

/// Icon kinds place at their usual size.
pub const ICON_PLACE_SIZE: (f64, f64) = (20.0, 20.0);

/// `isIconPlaceKind(subtype)`.
pub fn is_icon_place_kind(subtype: &str) -> bool {
    subtype == "text" || subtype == "file-attachment"
}

/// `annotationOfTool(model, tool)`: what a tool creates, read as an
/// annotation before it has a shape: its defaults and its `/F` seed.
pub fn annotation_of_tool(model: &Model, tool: &ResolvedTool) -> Object {
    let mut annotation = tool_annotation(model, &tool.subtype, Some(&tool.preset));
    for (k, v) in tool.flags.iter().flatten() {
        annotation.insert(k.clone(), v.clone());
    }
    annotation
}

/// A bare annotation, as the anchor rules read it (`{ annotation }`).
pub struct Bare<'a>(pub &'a Object);

impl FlagBearer for Bare<'_> {
    fn bearer_annotation(&self) -> &Object {
        self.0
    }
    fn bearer_authority(&self) -> Option<&Authority> {
        None
    }
}

fn box_shape(rect: Rect, rotation: f64) -> Shape {
    Shape::Box(BoxShape { box_: rect, rotation, ellipse: false, order: Default::default() })
}

/// `iconPlaceAt(annotation, point, page, view)`: where an icon placed at
/// `point` goes, as it shows (`shown`) and as it is stored (`rect`).
pub fn icon_place_at(
    annotation: &Object,
    point: Point,
    page: (f64, f64),
    view: Option<&ViewEnv>,
) -> (Shape, Rect) {
    let mode = anchor_mode_of(&Bare(annotation));
    let usual = box_shape(
        Rect { x: 0.0, y: 0.0, width: ICON_PLACE_SIZE.0, height: ICON_PLACE_SIZE.1 },
        0.0,
    );
    let Shape::Box(sized) = anchored_geom(&usual, mode.as_ref(), view) else {
        unreachable!("a box stays a box")
    };
    let shown = box_shape(
        fit_stamp_box(
            point,
            Size { width: sized.box_.width, height: sized.box_.height },
            Size { width: page.0, height: page.1 },
            sized.rotation,
        ),
        sized.rotation,
    );
    let rect = match unanchored_geom(&shown, mode.as_ref(), view) {
        Shape::Box(b) => b.box_,
        _ => unreachable!("a box stays a box"),
    };
    (shown, rect)
}

/// `iconPlacement(subtype, geometry, defaults, flags, file)`: the engine
/// create for a placed icon annotation, and for a file attachment the
/// file's bytes as its `file` resource.
pub fn icon_placement(
    subtype: &str,
    rect: &Value,
    defaults: &Object,
    flags: Option<&Object>,
    file: Option<&Value>,
) -> Result<(Object, Option<Value>), Error> {
    let mut shared = defaults.clone();
    shared.insert("rect".into(), rect.clone());
    for (k, v) in flags.into_iter().flatten() {
        shared.insert(k.clone(), v.clone());
    }
    if subtype == "text" {
        shared.insert("subtype".into(), json!("text"));
        return Ok((shared, None));
    }
    let Some(file) = file.filter(|f| !f.is_null() && !epdf_kernel::is_undefined(f)) else {
        return Err(Error::new("[annotation] a file-attachment placement requires a file payload"));
    };
    shared.insert("subtype".into(), json!("file-attachment"));
    shared.insert("file".into(), attachment_metadata_of(file)?);
    Ok((shared, Some(json!({ "file": file.get("data").cloned().unwrap_or(Value::Null) }))))
}

/// A picked file's name, MIME type and description.
fn attachment_metadata_of(file: &Value) -> Result<Value, Error> {
    let present = |k: &str| file.get(k).filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v));
    let Some(name) = present("name").and_then(Value::as_str) else {
        return Err(Error::new(
            "[annotation] an attached file needs a name: pass { data, name } or a File",
        ));
    };
    let mut out = serde_json::Map::new();
    out.insert("name".into(), json!(name));
    if let Some(mime) = present("mimeType") {
        out.insert("mimeType".into(), mime.clone());
    }
    if let Some(description) = file.get("description").filter(|d| !epdf_kernel::is_undefined(d)) {
        out.insert("description".into(), description.clone());
    }
    Ok(Value::Object(out))
}

/// The icon's stored rect as JSON.
pub fn rect_json(rect: Rect) -> Value {
    rect.json()
}
