//! Annotations in page space (`core/src/pageSpace/annotations.ts`).
//!
//! Field by field, by what each field holds, as upstream converts them
//! (`ANNOTATION_FIELD_SPACES`, from `kind-facts.generated.ts`). A place on
//! the annotation's page is measured from that page's visible box; a
//! destination from the box of the page it goes to. A field that is `null`
//! or absent stays as it is.
//!
//! The conversion runs over the record's JSON, as upstream's runs over the
//! record's object: it is one table for every kind, and a field a kind
//! gains is converted by naming it here, not by a twin of its struct.

use serde_json::{Map, Value};

use super::destinations::{PageDestination, page_destination_of, pdf_destination_of};
use super::{VisibleBoxOf, page_point_of, page_rect_of, pdf_point_of, pdf_rect_of};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::PdfDestination;
use crate::features::actions::reader::ActionTree;
use crate::features::actions::write::ActionWrite;
use crate::geom::{PagePoint, PageRect, PdfPoint, PdfRect};
use crate::identity::PageRef;

/// What a measured field holds (`MeasuredFieldSpace`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldSpace {
    /// A size, not a place: it doesn't move (`length`).
    Length,
    Point,
    Points,
    Strokes,
    Box,
    Quads,
    LinePoints,
    CalloutLine,
    Measure,
    LinkTarget,
    Actions,
}

/// The measured fields of `subtype` (`ANNOTATION_FIELD_SPACES`); a kind the
/// engine doesn't know has the base fields.
pub fn field_spaces(subtype: &str) -> &'static [(&'static str, FieldSpace)] {
    use FieldSpace as S;
    match subtype {
        "highlight" | "underline" | "squiggly" | "strikeout" | "redact" => {
            &[("rect", S::Box), ("actions", S::Actions), ("quadPoints", S::Quads)]
        }
        "circle" | "square" | "caret" | "stamp" | "widget" => {
            &[("rect", S::Box), ("actions", S::Actions), ("box", S::Box)]
        }
        "polygon" | "polyline" => &[
            ("rect", S::Box),
            ("actions", S::Actions),
            ("vertices", S::Points),
            ("captionCenter", S::Point),
            ("measure", S::Measure),
        ],
        "line" => &[
            ("rect", S::Box),
            ("actions", S::Actions),
            ("linePoints", S::LinePoints),
            ("measure", S::Measure),
        ],
        "link" => &[("rect", S::Box), ("actions", S::Actions), ("target", S::LinkTarget)],
        "ink" => &[("rect", S::Box), ("actions", S::Actions), ("inkList", S::Strokes)],
        "free-text" => &[
            ("rect", S::Box),
            ("actions", S::Actions),
            ("box", S::Box),
            ("richText", S::Length),
            ("calloutLine", S::CalloutLine),
        ],
        _ => &[("rect", S::Box), ("actions", S::Actions)],
    }
}

impl FieldSpace {
    /// `MeasuredFieldSpace`'s spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Point => "point",
            Self::Points => "points",
            Self::Strokes => "strokes",
            Self::Box => "box",
            Self::Quads => "quads",
            Self::LinePoints => "linePoints",
            Self::CalloutLine => "calloutLine",
            Self::Measure => "measure",
            Self::LinkTarget => "linkTarget",
            Self::Actions => "actions",
        }
    }
}

fn malformed(name: &str, error: impl std::fmt::Display) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::WireFormat,
        format!("annotation field '{name}' is not what it should hold: {error}"),
    )
}

fn read<T: serde::de::DeserializeOwned>(name: &str, value: Value) -> Result<T, EngineError> {
    serde_json::from_value(value).map_err(|e| malformed(name, e))
}

fn write<T: serde::Serialize>(name: &str, value: &T) -> Result<Value, EngineError> {
    serde_json::to_value(value).map_err(|e| malformed(name, e))
}

/// A named quad's corners, each through `convert`.
fn convert_quad(
    name: &str,
    quad: Value,
    convert: &mut dyn FnMut(Value) -> Result<Value, EngineError>,
) -> Result<Value, EngineError> {
    let Value::Object(mut corners) = quad else {
        return Err(malformed(name, "a quad is an object"));
    };
    for corner in ["upperLeft", "upperRight", "lowerLeft", "lowerRight"] {
        let point = corners.remove(corner).ok_or_else(|| malformed(name, corner))?;
        corners.insert(corner.into(), convert(point)?);
    }
    Ok(Value::Object(corners))
}

fn each(
    name: &str,
    list: Value,
    convert: &mut dyn FnMut(Value) -> Result<Value, EngineError>,
) -> Result<Value, EngineError> {
    let Value::Array(items) = list else {
        return Err(malformed(name, "a list is an array"));
    };
    Ok(Value::Array(items.into_iter().map(convert).collect::<Result<_, _>>()?))
}

/// The two directions a conversion runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    ToPage,
    ToPdf,
}

fn convert_field(
    name: &str,
    space: FieldSpace,
    value: Value,
    visible: PdfRect,
    box_of: &mut VisibleBoxOf<'_>,
    direction: Direction,
) -> Result<Value, EngineError> {
    use FieldSpace as S;
    let mut point = |value: Value| -> Result<Value, EngineError> {
        match direction {
            Direction::ToPage => {
                write(name, &page_point_of(read::<PdfPoint>(name, value)?, visible))
            }
            Direction::ToPdf => {
                write(name, &pdf_point_of(read::<PagePoint>(name, value)?, visible))
            }
        }
    };
    match space {
        S::Length => Ok(value),
        S::Point => point(value),
        S::Points | S::CalloutLine => each(name, value, &mut point),
        S::Strokes => each(name, value, &mut |stroke| each(name, stroke, &mut point)),
        S::Quads => each(name, value, &mut |quad| convert_quad(name, quad, &mut point)),
        S::LinePoints => {
            let Value::Object(mut line) = value else {
                return Err(malformed(name, "a line is an object"));
            };
            for end in ["start", "end"] {
                let at = line.remove(end).ok_or_else(|| malformed(name, end))?;
                line.insert(end.into(), point(at)?);
            }
            Ok(Value::Object(line))
        }
        S::Box => match direction {
            Direction::ToPage => write(name, &page_rect_of(read::<PdfRect>(name, value)?, visible)),
            Direction::ToPdf => write(name, &pdf_rect_of(read::<PageRect>(name, value)?, visible)),
        },
        // A scale's origin (`/O`) is a place on the page (`pageMeasureOf`).
        S::Measure => {
            if !value.is_object() {
                return Ok(value);
            }
            Ok(match direction {
                Direction::ToPage => super::measure::page_measure_of(value, visible),
                Direction::ToPdf => super::measure::pdf_measure_of(value, visible),
            })
        }
        S::LinkTarget => {
            let Value::Object(mut target) = value else { return Ok(value) };
            if let Some(destination) = target.remove("destination") {
                let destination = match direction {
                    Direction::ToPage => write(
                        name,
                        &page_destination_of(read::<PdfDestination>(name, destination)?, box_of)?,
                    )?,
                    Direction::ToPdf => write(
                        name,
                        &pdf_destination_of(read::<PageDestination>(name, destination)?, box_of)?,
                    )?,
                };
                target.insert("destination".into(), destination);
            }
            Ok(Value::Object(target))
        }
        // `mapTriggerActions`: each trigger's read tree, or the action a
        // write sets it to (a widget's actions on an update); `null` stays.
        S::Actions => {
            let Value::Object(triggers) = value else {
                return Err(malformed(name, "actions are an object"));
            };
            let mut out = Map::with_capacity(triggers.len());
            for (trigger, action) in triggers {
                let tree = action.get("root").is_some();
                let converted = match (action.is_null(), tree, direction) {
                    (true, _, _) => action,
                    (false, true, Direction::ToPage) => {
                        let tree: ActionTree<PdfDestination> = read(name, action)?;
                        write(
                            name,
                            &tree.try_map_destinations(&mut |d| {
                                page_destination_of(d, &mut *box_of)
                            })?,
                        )?
                    }
                    (false, true, Direction::ToPdf) => {
                        let tree: ActionTree<PageDestination> = read(name, action)?;
                        write(
                            name,
                            &tree.try_map_destinations(&mut |d| {
                                pdf_destination_of(d, &mut *box_of)
                            })?,
                        )?
                    }
                    (false, false, Direction::ToPage) => {
                        let written: ActionWrite<PdfDestination> = read(name, action)?;
                        write(
                            name,
                            &written.try_map_destinations(&mut |d| {
                                page_destination_of(d, &mut *box_of)
                            })?,
                        )?
                    }
                    (false, false, Direction::ToPdf) => {
                        let written: ActionWrite<PageDestination> = read(name, action)?;
                        write(
                            name,
                            &written.try_map_destinations(&mut |d| {
                                pdf_destination_of(d, &mut *box_of)
                            })?,
                        )?
                    }
                };
                out.insert(trigger, converted);
            }
            Ok(Value::Object(out))
        }
    }
}

/// The measured fields of `subtype`, or of every kind by name when an
/// update leaves its kind out: a name holds the same thing in every kind
/// (`spacesOf`, `ANNOTATION_FIELD_SPACES_BY_NAME`).
fn space_of(subtype: Option<&str>, name: &str) -> Option<FieldSpace> {
    let found = |spaces: &[(&str, FieldSpace)]| {
        spaces.iter().find(|(n, _)| *n == name).map(|(_, space)| *space)
    };
    match subtype {
        Some(subtype) => found(field_spaces(subtype)),
        None => FIELD_SPACE_KINDS.iter().find_map(|kind| found(field_spaces(kind))),
    }
}

/// Every kind with measured fields of its own, for the by-name table.
const FIELD_SPACE_KINDS: &[&str] =
    &["highlight", "square", "polygon", "line", "link", "ink", "free-text"];

/// A place not given in page space (`notInPageSpace`).
fn not_in_page_space(name: &str, space: FieldSpace) -> EngineError {
    let shape =
        if space == FieldSpace::Box { "{ x, y, width, height }" } else { "points as { x, y }" };
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("field '{name}': expected page space, {shape} from the page's top-left"),
    )
    .with_details(serde_json::json!({ "field": name }))
}

fn convert_object(
    subtype: Option<&str>,
    fields: Map<String, Value>,
    visible: PdfRect,
    box_of: &mut VisibleBoxOf<'_>,
    direction: Direction,
) -> Result<Value, EngineError> {
    let mut out = Map::with_capacity(fields.len());
    for (name, field) in fields {
        let field = match space_of(subtype, &name) {
            Some(space) if !field.is_null() => {
                match convert_field(&name, space, field, visible, box_of, direction) {
                    // A value that isn't a place in page space reads, on
                    // the way in, as upstream's NaN check refuses it.
                    Err(e)
                        if direction == Direction::ToPdf
                            && e.code == EngineErrorCode::WireFormat =>
                    {
                        return Err(not_in_page_space(&name, space));
                    }
                    converted => converted?,
                }
            }
            _ => field,
        };
        out.insert(name, field);
    }
    Ok(Value::Object(out))
}

fn convert_fields(
    annotation: Value,
    box_of: &mut VisibleBoxOf<'_>,
    direction: Direction,
) -> Result<Value, EngineError> {
    let Value::Object(fields) = annotation else {
        return Err(malformed("annotation", "an annotation is an object"));
    };
    let page: PageRef = read("page", fields.get("page").cloned().unwrap_or(Value::Null))?;
    let visible = box_of(page.object_number())?;
    let subtype = fields.get("subtype").and_then(Value::as_str).unwrap_or_default().to_owned();
    convert_object(Some(&subtype), fields, visible, box_of, direction)
}

/// A page-space create in the file's coordinates (`pdfAnnotationDraftOf`),
/// on the page whose visible box is `visible`.
pub fn pdf_annotation_draft_of(
    draft: Map<String, Value>,
    visible: PdfRect,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Map<String, Value>, EngineError> {
    let subtype = draft.get("subtype").and_then(Value::as_str).unwrap_or_default().to_owned();
    match convert_object(Some(&subtype), draft, visible, box_of, Direction::ToPdf)? {
        Value::Object(converted) => Ok(converted),
        _ => unreachable!("an object converts to an object"),
    }
}

/// A page-space update in the file's coordinates (`pdfAnnotationPatchOf`).
/// An update may leave out its `subtype`; its fields' names say what they
/// hold.
pub fn pdf_annotation_patch_of(
    patch: Map<String, Value>,
    visible: PdfRect,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Map<String, Value>, EngineError> {
    let subtype = patch.get("subtype").and_then(Value::as_str).map(str::to_owned);
    match convert_object(subtype.as_deref(), patch, visible, box_of, Direction::ToPdf)? {
        Value::Object(converted) => Ok(converted),
        _ => unreachable!("an object converts to an object"),
    }
}

/// A read in page space (`pageAnnotationOf`), measured on the page it names.
pub fn page_annotation_of(
    annotation: Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Value, EngineError> {
    convert_fields(annotation, box_of, Direction::ToPage)
}

/// A page-space read in the file's coordinates (`pdfAnnotationOf`).
pub fn pdf_annotation_of(
    annotation: Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Value, EngineError> {
    convert_fields(annotation, box_of, Direction::ToPdf)
}

/// `annotations.list`'s answer in page space: each annotation on its page.
pub fn page_annotation_list_of(
    list: Value,
    box_of: &mut VisibleBoxOf<'_>,
) -> Result<Value, EngineError> {
    let Value::Object(mut list) = list else {
        return Err(malformed("list", "an annotation list is an object"));
    };
    let annotations = list.remove("annotations").unwrap_or(Value::Array(Vec::new()));
    let annotations = each("annotations", annotations, &mut |a| page_annotation_of(a, box_of))?;
    list.insert("annotations".into(), annotations);
    Ok(Value::Object(list))
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/test/pageSpace/annotations.test.ts`.

    use serde_json::json;

    use super::*;
    use crate::identity::PageObjectNumber;

    const VISIBLE: PdfRect = PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 };

    fn boxes() -> impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> {
        |_| Ok(VISIBLE)
    }

    fn base(subtype: &str) -> Value {
        json!({
            "ref": { "kind": "baseIndex", "page": { "kind": "objectNumber", "objectNumber": 3 },
                "baseIndex": 0 },
            "page": { "kind": "objectNumber", "objectNumber": 3 },
            "subtype": subtype,
            "rect": { "left": 100, "bottom": 600, "right": 150, "top": 650 },
            "contents": "rect",
            "actions": null,
        })
    }

    fn with(mut value: Value, fields: Value) -> Value {
        for (key, field) in fields.as_object().unwrap() {
            value[key] = field.clone();
        }
        value
    }

    #[test]
    fn a_rect_and_a_box_are_measured_from_the_top_left() {
        let square = with(
            base("square"),
            json!({ "box": { "left": 110, "bottom": 610, "right": 140, "top": 640 },
                "rotation": null }),
        );
        let page = page_annotation_of(square.clone(), &mut boxes()).unwrap();
        assert_eq!(page["rect"], json!({ "x": 50.0, "y": 82.0, "width": 50.0, "height": 50.0 }));
        assert_eq!(page["box"], json!({ "x": 60.0, "y": 92.0, "width": 30.0, "height": 30.0 }));
        // Fields that hold no place, and a `null`, go as they came.
        assert_eq!(page["contents"], "rect");
        assert_eq!(page["rotation"], Value::Null);
        assert_eq!(page["actions"], Value::Null);
        let back = pdf_annotation_of(page, &mut boxes()).unwrap();
        assert_eq!(back["box"]["left"], 110.0);
        assert_eq!(back["box"]["top"], 640.0);
    }

    #[test]
    fn a_reversed_rect_keeps_its_corner_order() {
        let text = with(
            base("text"),
            json!({ "rect": { "left": 200, "bottom": 92, "right": 180, "top": 72 } }),
        );
        let visible = PdfRect { left: 0.0, bottom: 0.0, right: 612.0, top: 792.0 };
        let page = page_annotation_of(text, &mut |_| Ok(visible)).unwrap();
        assert_eq!(
            page["rect"],
            json!({ "x": 200.0, "y": 720.0, "width": -20.0, "height": -20.0 })
        );
    }

    #[test]
    fn points_strokes_quads_and_lines_move_point_by_point() {
        let p = |x: f64, y: f64| json!({ "x": x, "y": y });
        let ink = with(base("ink"), json!({ "inkList": [[p(50.0, 732.0), p(150.0, 632.0)]] }));
        let page = page_annotation_of(ink, &mut boxes()).unwrap();
        assert_eq!(page["inkList"], json!([[p(0.0, 0.0), p(100.0, 100.0)]]));

        let line = with(
            base("line"),
            json!({ "linePoints": { "start": p(50.0, 732.0), "end": p(60.0, 722.0) },
                "measure": { "subtype": "rectilinear", "origin": p(50.0, 632.0) } }),
        );
        let page = page_annotation_of(line, &mut boxes()).unwrap();
        assert_eq!(page["linePoints"]["end"], p(10.0, 10.0));
        assert_eq!(page["measure"]["origin"], p(0.0, 100.0));

        let highlight = with(
            base("highlight"),
            json!({ "quadPoints": [{ "upperLeft": p(50.0, 732.0), "upperRight": p(60.0, 732.0),
                "lowerLeft": p(50.0, 722.0), "lowerRight": p(60.0, 722.0) }] }),
        );
        let page = page_annotation_of(highlight, &mut boxes()).unwrap();
        assert_eq!(page["quadPoints"][0]["lowerRight"], p(10.0, 10.0));

        let polygon = with(
            base("polygon"),
            json!({ "vertices": [p(50.0, 732.0)], "captionCenter": null,
                "measure": { "subtype": "geospatial" } }),
        );
        let page = page_annotation_of(polygon, &mut boxes()).unwrap();
        assert_eq!(page["vertices"], json!([p(0.0, 0.0)]));
        assert_eq!(page["captionCenter"], Value::Null);
        assert_eq!(page["measure"], json!({ "subtype": "geospatial" }));
    }

    #[test]
    fn a_link_goes_to_a_place_on_the_page_it_names() {
        let link = with(
            base("link"),
            json!({ "target": { "kind": "goto", "destination": { "kind": "xyz",
                "page": { "kind": "objectNumber", "objectNumber": 9 },
                "left": 150, "top": 632, "zoom": null } } }),
        );
        let mut asked = Vec::new();
        let page = page_annotation_of(link, &mut |page: PageObjectNumber| {
            asked.push(page.get());
            Ok(VISIBLE)
        })
        .unwrap();
        assert_eq!(page["target"]["destination"]["x"], 100.0);
        assert_eq!(page["target"]["destination"]["y"], 100.0);
        assert_eq!(asked, vec![3, 9], "the annotation's page, then the destination's");
        let uri = with(base("link"), json!({ "target": { "kind": "uri", "uri": "https://x" } }));
        assert_eq!(page_annotation_of(uri, &mut boxes()).unwrap()["target"]["uri"], "https://x");
    }
}
