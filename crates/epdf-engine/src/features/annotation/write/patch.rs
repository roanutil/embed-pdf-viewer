//! What an update writes: the patch checked against the annotation it
//! changes and stated whole.
//!
//! G11. Ports `core/src/annotation/resolve/resolveAnnotationPatch.ts`
//! (`pdfPrepareAnnotationPatch`, `pdfDeriveAnnotationPatch`),
//! `resolve/sentBack.ts`, `resolve/text.ts` (`noteStateFollows`,
//! `assertRichTextAgreement`), `resolve/freeText.ts` (`freeTextFollows`,
//! `calloutEndFollows`), `calloutEnd.ts`, `resolve/measurement.ts`
//! (`measurementFollows`) and
//! `checkWrite.ts` (`assertAnnotationPatch`, `pdfResolveAnnotationPatch`).
//!
//! A patch arrives in the file's coordinates as the JSON object the caller
//! sent, and the annotation it changes as its read, the same way. It is
//! prepared (its subtype filled in, a drawn kind's `rect` resolved into the
//! shape, what it sends back as it reads dropped), checked field by field
//! against its kind's update schema, then what follows from it is stated.
//! The writers write what this resolves and nothing more.

use serde_json::{Map, Value};

use super::appearance::semantic_equal;
use super::prepare::{DraftFields, FieldOwner, owner_of, read_field, standard_state_model_of};
use super::shape_for_rect::pdf_resolve_rect_command;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::rich_text::{
    RichTextParagraph, face_for_free_text_font, paragraphs_from_plain_text, plain_text,
};
use crate::features::annotation::subtype::AnnotationSubtype;
use crate::geom::{PdfPoint, PdfPointTurn, PdfRect, pdf_point_turned, pdf_point_unturned};
use crate::identity::AnnotationRef;

/// A patch the writers take: its kind, the resolved patch as JSON (what the
/// appearance verdict and the undo read), and its data fields read into
/// their types.
#[derive(Debug, Clone)]
pub struct ResolvedPatch {
    pub subtype: AnnotationSubtype,
    pub data: Map<String, Value>,
    pub fields: DraftFields,
    /// A popup's new `parent`, when the patch names one.
    pub parent: Option<AnnotationRef>,
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
}

fn subtype_of(current: &Map<String, Value>) -> Result<AnnotationSubtype, EngineError> {
    let named = current.get("subtype").and_then(Value::as_str).unwrap_or_default();
    serde_json::from_value(Value::String(named.to_owned()))
        .map_err(|_| invalid(format!("an annotation of kind '{named}' reads as no kind")))
}

/// `assertDeclaredFields`: a write names only fields its kind declares.
fn assert_declared_fields(
    subtype: AnnotationSubtype,
    patch: &Map<String, Value>,
) -> Result<(), EngineError> {
    let unknown: Vec<String> = patch
        .keys()
        .filter(|name| *name != "subtype" && owner_of(subtype, name).is_none())
        .map(|name| format!("'{name}'"))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(invalid(format!("{} has no field {}", subtype.as_str(), unknown.join(", "))))
}

/// `annotationPatchForTarget`: the patch as the target's kind. A different
/// subtype, a changed name, a field the kind doesn't declare, or any change
/// to an annotation of a type the engine doesn't model is refused.
fn patch_for_target(
    current: &Map<String, Value>,
    mut patch: Map<String, Value>,
) -> Result<(AnnotationSubtype, Map<String, Value>), EngineError> {
    let subtype = subtype_of(current)?;
    if let Some(named) = patch.get("subtype")
        && named.as_str() != Some(subtype.as_str())
    {
        return Err(invalid("Annotation subtype cannot change"));
    }
    if subtype == AnnotationSubtype::Unsupported {
        return Err(invalid("an annotation of a type the engine doesn't model can't be updated"));
    }
    if let Some(nm) = patch.get("nm")
        && nm != current.get("nm").unwrap_or(&Value::Null)
    {
        return Err(invalid("an annotation's nm can't change after create"));
    }
    assert_declared_fields(subtype, &patch)?;
    patch.insert("subtype".into(), Value::String(subtype.as_str().to_owned()));
    Ok((subtype, patch))
}

/// The colour fields a kind declares (`kindFacts.colors`).
const COLORS: [&str; 3] = ["color", "interiorColor", "fontColor"];

/// `withoutUnchangedColors`: a colour sent back as it reads is left out, so
/// a gray or CMYK colour the file holds stays as it is rather than turning
/// into the sRGB value it reads as. Compared without case.
fn without_unchanged_colors(current: &Map<String, Value>, patch: &mut Map<String, Value>) {
    patch.retain(|name, value| {
        if name == "subtype" || !COLORS.contains(&name.as_str()) {
            return true;
        }
        match (value.as_str(), current.get(name).and_then(Value::as_str)) {
            (Some(sent), Some(read)) => !sent.eq_ignore_ascii_case(read),
            _ => true,
        }
    });
}

/// `withoutUnchangedFields`: every field sent back as it reads is left out,
/// as `semanticEqual` judges it (`null` and absent the same, numbers within
/// what a round trip through page space keeps).
fn without_unchanged_fields(current: &Map<String, Value>, patch: &mut Map<String, Value>) {
    patch
        .retain(|name, value| name == "subtype" || !semantic_equal(Some(value), current.get(name)));
}

/// `pdfPrepareAnnotationPatch`: the patch an update writes before anything
/// follows from it, in the file's coordinates.
pub fn pdf_prepare_annotation_patch(
    current: &Map<String, Value>,
    patch: Map<String, Value>,
) -> Result<(AnnotationSubtype, Map<String, Value>), EngineError> {
    let (subtype, targeted) = patch_for_target(current, patch)?;
    let mut prepared = pdf_resolve_rect_command(current, targeted)?;
    without_unchanged_colors(current, &mut prepared);
    without_unchanged_fields(current, &mut prepared);
    Ok((subtype, prepared))
}

/// The data fields whose read holds values a write can't make
/// (`readBack()`): sent back unchanged they were left out by preparation,
/// any other such value is refused.
fn read_back_fields(subtype: AnnotationSubtype) -> &'static [&'static str] {
    use AnnotationSubtype as S;
    match subtype {
        S::Square | S::Circle | S::Line => &["borderStyle"],
        S::Polygon | S::Polyline => &["borderStyle", "vertices"],
        S::Ink => &["borderStyle", "strokeWidth", "inkList"],
        S::FreeText => &["borderStyle", "intent"],
        S::Link => &["target"],
        S::Popup => &["parent"],
        S::Widget => &["actions"],
        _ => &[],
    }
}

/// A popup's `parent` as a write takes it: an annotation, never `null`.
fn read_parent(value: &Value) -> Result<AnnotationRef, EngineError> {
    serde_json::from_value(value.clone()).map_err(|e| {
        invalid(format!("popup update field 'parent': {e}"))
            .with_details(serde_json::json!({ "field": "parent" }))
    })
}

/// `assertAnnotationPatch`: a prepared update's data checked against its
/// kind. A read-only value still in it is refused, naming the field; the
/// rest is checked against the kind's update schema, each field read into
/// its type. Fields the engine works out, stamps or keeps as found are
/// accepted and ignored.
pub fn assert_annotation_patch(
    subtype: AnnotationSubtype,
    prepared: &Map<String, Value>,
) -> Result<(), EngineError> {
    let kind = subtype.as_str();
    for name in read_back_fields(subtype) {
        let Some(value) = prepared.get(*name) else { continue };
        let writable = match *name {
            "parent" => read_parent(value).is_ok(),
            // A widget's actions write as a patch by event; a read's trees
            // sent back unchanged were left out by preparation.
            "actions" => value.is_null()
                || serde_json::from_value::<crate::features::actions::write::WidgetActionsPatch>(
                    value.clone(),
                )
                .is_ok(),
            _ => read_field("update", subtype, name, value, &mut DraftFields::default()).is_ok(),
        };
        if !writable {
            return Err(invalid(format!(
                "{kind} update field '{name}': this value can be read but not written; send it back unchanged or replace it"
            ))
            .with_details(serde_json::json!({ "field": name })));
        }
    }
    read_patch_fields(subtype, prepared).map(|_| ())
}

/// A patch's data fields read into their types, as the writers take them.
/// Fields the engine works out, stamps or keeps as found are left out.
fn read_patch_fields(
    subtype: AnnotationSubtype,
    patch: &Map<String, Value>,
) -> Result<(DraftFields, Option<AnnotationRef>), EngineError> {
    let mut fields = DraftFields::default();
    let mut parent = None;
    for (name, value) in patch {
        if name == "subtype" {
            continue;
        }
        match owner_of(subtype, name) {
            Some(FieldOwner::Data) if name == "parent" => parent = Some(read_parent(value)?),
            Some(FieldOwner::Data) => read_field("update", subtype, name, value, &mut fields)?,
            // Accepted so a read can be sent back; the engine decides what
            // happens to them.
            _ => {}
        }
    }
    Ok((fields, parent))
}

/// `noteStateFollows`: a note's new standard state sets its model; a custom
/// one keeps the annotation's, and needs one.
fn note_state_follows(
    current: &Map<String, Value>,
    patch: &mut Map<String, Value>,
) -> Result<(), EngineError> {
    let Some(state) = patch.get("state").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(());
    };
    if patch.contains_key("stateModel") {
        return Ok(());
    }
    if let Some(model) = standard_state_model_of(&state) {
        patch.insert("stateModel".into(), Value::String(model.to_owned()));
        return Ok(());
    }
    if current.get("stateModel").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err(invalid(format!("text: the custom state '{state}' needs a stateModel"))
            .with_details(serde_json::json!({ "field": "stateModel" })));
    }
    Ok(())
}

fn paragraphs_of(value: Option<&Value>) -> Option<Vec<RichTextParagraph>> {
    serde_json::from_value(value?.clone()).ok()
}

/// `assertRichTextAgreement`: `contents` beside `richText` is its plain
/// projection.
fn assert_rich_text_agreement(patch: &Map<String, Value>) -> Result<(), EngineError> {
    let (Some(contents), Some(rich)) =
        (patch.get("contents").and_then(Value::as_str), patch.get("richText"))
    else {
        return Ok(());
    };
    let paragraphs = paragraphs_of(rich.get("paragraphs")).unwrap_or_default();
    if contents != plain_text(&paragraphs) {
        return Err(invalid(
            "free-text: `contents` must equal the plain projection of `richText` when both are given",
        ));
    }
    Ok(())
}

/// `freeTextFollows`: a free text's text lives in its rich text. New
/// `richText` brings its `contents`; new `contents` becomes body-style
/// paragraphs; a new font, size, text colour or alignment moves the body;
/// whatever moves the body states the complete rich text, and the size,
/// text colour and alignment it reads back with. A callout line needs the
/// callout intent.
fn free_text_follows(
    current: &Map<String, Value>,
    patch: &mut Map<String, Value>,
    describe: Option<crate::features::annotation::rich_text::DescribeFont<'_>>,
) -> Result<(), EngineError> {
    if patch.contains_key("intent") || patch.contains_key("calloutLine") {
        let intent = patch
            .get("intent")
            .or_else(|| current.get("intent"))
            .and_then(Value::as_str)
            .unwrap_or("free-text")
            .to_owned();
        let callout = patch.get("calloutLine").or_else(|| current.get("calloutLine"));
        if callout.is_some_and(|line| !line.is_null()) && intent != "free-text-callout" {
            return Err(invalid(format!(
                "free-text: a callout line needs intent 'free-text-callout', not '{intent}'"
            ))
            .with_details(serde_json::json!({ "field": "intent" })));
        }
    }
    // `bodyWithStyle({}, text)`: what the patch's style asks of the body.
    let mut changes = Map::new();
    if let Some(family) = patch.get("fontFamily").and_then(Value::as_str) {
        let face = face_for_free_text_font(family, describe);
        changes.insert("family".into(), Value::String(face.family));
        if let Some(weight) = face.weight {
            changes.insert("weight".into(), Value::from(weight));
        }
        if let Some(italic) = face.italic {
            changes.insert("italic".into(), Value::Bool(italic));
        }
    }
    if let Some(size) = patch.get("fontSize") {
        changes.insert("size".into(), size.clone());
    }
    if let Some(color) = patch.get("fontColor").filter(|c| !c.is_null()) {
        changes.insert("color".into(), color.clone());
    }
    if let Some(align) = patch.get("textAlign") {
        changes.insert("align".into(), align.clone());
    }
    let text_changed = patch.contains_key("richText") || patch.contains_key("contents");
    if !text_changed && changes.is_empty() {
        return Ok(());
    }
    let read_rich = current.get("richText");
    let mut body = read_rich
        .and_then(|rich| rich.get("body"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(given) =
        patch.get("richText").and_then(|rich| rich.get("body")).and_then(Value::as_object)
    {
        body.extend(given.clone());
    }
    body.extend(changes);
    let paragraphs = match patch.get("richText").and_then(|rich| rich.get("paragraphs")) {
        Some(paragraphs) => paragraphs.clone(),
        None => match patch.get("contents") {
            Some(contents) => serde_json::to_value(paragraphs_from_plain_text(
                contents.as_str().unwrap_or_default(),
            ))
            .unwrap_or_default(),
            None => read_rich.and_then(|rich| rich.get("paragraphs")).cloned().unwrap_or_default(),
        },
    };
    // `styleOfBody`: the style fields a new body brings, beside it, where
    // they differ from the read and the patch doesn't state its own.
    if !patch.contains_key("fontSize")
        && let Some(size) = body.get("size")
        && !semantic_equal(Some(size), current.get("fontSize"))
    {
        patch.insert("fontSize".into(), size.clone());
    }
    if !patch.contains_key("fontColor")
        && let Some(color) = body.get("color").and_then(Value::as_str)
        && Some(color.to_ascii_lowercase())
            != current.get("fontColor").and_then(Value::as_str).map(str::to_ascii_lowercase)
    {
        patch.insert("fontColor".into(), Value::String(color.to_ascii_lowercase()));
    }
    if !patch.contains_key("textAlign")
        && let Some(align) = body.get("align")
        && Some(align) != current.get("textAlign")
    {
        patch.insert("textAlign".into(), align.clone());
    }
    if text_changed {
        let projected = paragraphs_of(Some(&paragraphs)).unwrap_or_default();
        // The engine writes `/Contents` from the rich text: a line break
        // reads back as `\r`.
        patch.insert("contents".into(), Value::String(plain_text(&projected)));
    }
    patch.insert(
        "richText".into(),
        serde_json::json!({ "body": Value::Object(body), "paragraphs": paragraphs }),
    );
    Ok(())
}

/// `pdfCalloutEnd`: where a callout's line meets its text box, the middle of
/// the side `toward` lies beyond, decided in the box's own frame when it is
/// turned.
pub fn pdf_callout_end(content_box: PdfRect, toward: PdfPoint, rotation: f64) -> PdfPoint {
    let center = PdfPoint {
        x: (content_box.left + content_box.right) / 2.0,
        y: (content_box.bottom + content_box.top) / 2.0,
    };
    let turn = PdfPointTurn { degrees: rotation, center };
    let local = if rotation != 0.0 { pdf_point_unturned(toward, turn) } else { toward };
    let dx = local.x - center.x;
    let dy = local.y - center.y;
    let end = if dx.abs() >= dy.abs() {
        PdfPoint { x: if dx >= 0.0 { content_box.right } else { content_box.left }, y: center.y }
    } else {
        PdfPoint { x: center.x, y: if dy > 0.0 { content_box.top } else { content_box.bottom } }
    };
    if rotation != 0.0 { pdf_point_turned(end, turn) } else { end }
}

/// `calloutEndFollows`: when a callout's box moves or turns and the patch
/// doesn't state its own line, the line's end moves to where it now meets
/// the box.
fn callout_end_follows(current: &Map<String, Value>, patch: &mut Map<String, Value>) {
    let Some(line) = current.get("calloutLine").and_then(|line| {
        serde_json::from_value::<Vec<PdfPoint>>(line.clone()).ok().filter(|l| l.len() >= 2)
    }) else {
        return;
    };
    if patch.contains_key("calloutLine")
        || (!patch.contains_key("box") && !patch.contains_key("rotation"))
    {
        return;
    }
    let Some(content_box) = patch
        .get("box")
        .or_else(|| current.get("box"))
        .and_then(|b| serde_json::from_value::<PdfRect>(b.clone()).ok())
    else {
        return;
    };
    let rotation = match patch.get("rotation") {
        Some(value) => value.as_f64(),
        None => current.get("rotation").and_then(Value::as_f64),
    }
    .unwrap_or(0.0);
    let knee = (line.len() == 3).then(|| line[1]);
    let end = pdf_callout_end(content_box, knee.unwrap_or(line[0]), rotation);
    let points = match knee {
        Some(knee) => vec![line[0], knee, end],
        None => vec![line[0], end],
    };
    patch.insert("calloutLine".into(), serde_json::to_value(points).unwrap_or_default());
}

/// Whether a write names any of a caption's fields (`touchesCaption`).
pub fn touches_caption(value: &Map<String, Value>) -> bool {
    ["captionEnabled", "captionPosition", "captionOffset", "captionCenter"]
        .iter()
        .any(|name| value.contains_key(*name))
}

fn point_of(value: Option<&Value>) -> Option<PdfPoint> {
    value.filter(|v| !v.is_null()).and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn points_of(value: Option<&Value>) -> Option<Vec<PdfPoint>> {
    value.filter(|v| !v.is_null()).and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// `moveCenter`: a manual caption centre carried along by a rigid move of
/// the vertices, inferred only when every vertex agrees. A vertex edit
/// leaves the centre where it is (`None`).
fn move_center(before: &[PdfPoint], after: &[PdfPoint], center: PdfPoint) -> Option<PdfPoint> {
    use crate::features::measure::derive::js_hypot as hypot;
    if before.len() != after.len() || before.len() < 2 {
        return None;
    }
    let origin = before[0];
    let moved = after[0];
    let index =
        before.iter().position(|point| hypot(point.x - origin.x, point.y - origin.y) > 1e-6)?;
    let from = PdfPoint { x: before[index].x - origin.x, y: before[index].y - origin.y };
    let to = PdfPoint { x: after[index].x - moved.x, y: after[index].y - moved.y };
    let length = from.x * from.x + from.y * from.y;
    let cos = (from.x * to.x + from.y * to.y) / length;
    let sin = (from.x * to.y - from.y * to.x) / length;
    if (cos * cos + sin * sin - 1.0).abs() > 1e-5 {
        return None;
    }
    let transform = |point: PdfPoint| PdfPoint {
        x: moved.x + cos * (point.x - origin.x) - sin * (point.y - origin.y),
        y: moved.y + sin * (point.x - origin.x) + cos * (point.y - origin.y),
    };
    let rigid = before.iter().zip(after).all(|(point, at)| {
        let placed = transform(*point);
        hypot(placed.x - at.x, placed.y - at.y) <= 1e-3
    });
    rigid.then(|| transform(center))
}

/// `measurementFollows`: a dimension patch with what follows from it. A
/// foreign measure's marker sent back keeps the measure as it is (another
/// foreign one is refused); caption fields merge with the current caption
/// into a complete caption; a whole-shape move carries a manual caption
/// centre along; and the label is worked out from the resulting points and
/// scale.
fn measurement_follows(
    subtype: AnnotationSubtype,
    current: &Map<String, Value>,
    patch: &mut Map<String, Value>,
) -> Result<(), EngineError> {
    use AnnotationSubtype as S;
    if !matches!(subtype, S::Line | S::Polygon | S::Polyline) {
        return Ok(());
    }
    let subtype_of = |measure: Option<&Value>| {
        measure.and_then(|m| m.get("subtype")).and_then(Value::as_str).map(str::to_owned)
    };
    if let Some(foreign) = subtype_of(patch.get("measure")).filter(|s| s != "rectilinear") {
        if subtype_of(current.get("measure")).as_deref() != Some(foreign.as_str()) {
            let what = if foreign == "geospatial" { "geospatial" } else { "foreign" };
            return Err(invalid(format!("A {what} measure can't be written; leave measure out")));
        }
        // The marker a read returned, sent back: the measure stays as it is.
        patch.remove("measure");
    }
    let current_of = |name: &str| current.get(name).cloned().unwrap_or(Value::Null);
    if subtype == S::Line {
        if touches_caption(patch) {
            let enabled =
                patch.get("captionEnabled").cloned().unwrap_or(current_of("captionEnabled"));
            let position = patch
                .get("captionPosition")
                .filter(|p| !p.is_null())
                .cloned()
                .unwrap_or(current_of("captionPosition"));
            let offset = patch.get("captionOffset").cloned().unwrap_or(current_of("captionOffset"));
            patch.insert("captionEnabled".into(), enabled);
            patch.insert("captionPosition".into(), position);
            patch.insert("captionOffset".into(), offset);
        }
    } else {
        if let (Some(after), Some(before), Some(center)) = (
            points_of(patch.get("vertices")),
            points_of(current.get("vertices")),
            point_of(current.get("captionCenter")),
        ) && patch.get("captionEnabled") != Some(&Value::Null)
            && !patch.contains_key("captionCenter")
            && let Some(moved) = move_center(&before, &after, center)
        {
            patch.insert("captionCenter".into(), serde_json::to_value(moved).unwrap_or_default());
        }
        if touches_caption(patch) {
            if patch.get("captionEnabled") == Some(&Value::Null) {
                // No caption flag: the caption and its placement go together.
                patch.insert("captionCenter".into(), Value::Null);
            } else {
                let enabled = patch
                    .get("captionEnabled")
                    .filter(|e| !e.is_null())
                    .or_else(|| current.get("captionEnabled").filter(|e| !e.is_null()))
                    .cloned()
                    .unwrap_or(Value::Bool(false));
                let center =
                    patch.get("captionCenter").cloned().unwrap_or(current_of("captionCenter"));
                patch.insert("captionEnabled".into(), enabled);
                patch.insert("captionCenter".into(), center);
            }
        }
    }
    // The label, from the resulting points and scale (`measurementReadout`
    // over the annotation as the patch leaves it).
    let label_inputs = ["linePoints", "vertices", "measure", "intent", "contents"];
    if label_inputs.iter().any(|name| patch.contains_key(*name)) {
        let after = |name: &str| patch.get(name).or_else(|| current.get(name));
        let line_points = after("linePoints")
            .and_then(|line| Some((point_of(line.get("start"))?, point_of(line.get("end"))?)));
        let readout = crate::features::measure::derive::measurement_readout(
            &crate::features::measure::derive::DimensionInput {
                subtype: subtype.as_str(),
                intent: after("intent").and_then(Value::as_str),
                measure: after("measure"),
                line_points,
                vertices: points_of(after("vertices")),
            },
        );
        if let Ok(readout) = readout {
            patch.insert("contents".into(), Value::String(readout.label));
        }
    }
    Ok(())
}

/// `pdfDeriveAnnotationPatch`: a prepared patch with every field that
/// follows from it stated. Pure: it reads nothing but its arguments.
pub fn pdf_derive_annotation_patch(
    subtype: AnnotationSubtype,
    current: &Map<String, Value>,
    mut patch: Map<String, Value>,
    describe: Option<crate::features::annotation::rich_text::DescribeFont<'_>>,
) -> Result<Map<String, Value>, EngineError> {
    if subtype == AnnotationSubtype::Text {
        note_state_follows(current, &mut patch)?;
    }
    if subtype == AnnotationSubtype::FreeText {
        assert_rich_text_agreement(&patch)?;
        free_text_follows(current, &mut patch, describe)?;
        callout_end_follows(current, &mut patch);
    }
    measurement_follows(subtype, current, &mut patch)?;
    Ok(patch)
}

/// `pdfResolveAnnotationPatch`: the patch the engine writes for `patch` on
/// `current`, in the file's coordinates: prepared, checked against its kind,
/// and every field that follows from it stated. Throws `InvalidArg` for a
/// patch the engine refuses.
pub fn pdf_resolve_annotation_patch(
    current: &Map<String, Value>,
    patch: Map<String, Value>,
    describe: Option<crate::features::annotation::rich_text::DescribeFont<'_>>,
) -> Result<ResolvedPatch, EngineError> {
    let (subtype, prepared) = pdf_prepare_annotation_patch(current, patch)?;
    assert_annotation_patch(subtype, &prepared)?;
    let data = pdf_derive_annotation_patch(subtype, current, prepared, describe)?;
    // The writers take the derived patch typed: read, not checked again
    // (the check ran on the prepared patch, as upstream's does).
    let (fields, parent) = read_patch_fields(subtype, &data)?;
    Ok(ResolvedPatch { subtype, data, fields, parent })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn square() -> Map<String, Value> {
        map(json!({ "subtype": "square", "nm": "s",
            "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 },
            "box": { "left": 0, "bottom": 0, "right": 10, "top": 10 },
            "color": "#808080", "interiorColor": null, "borderStyle": "beveled",
            "strokeWidth": 1, "rotation": null }))
    }

    #[test]
    fn a_read_sent_back_is_no_change() {
        let mut sent = square();
        sent.insert("color".into(), json!("#808080".to_uppercase()));
        let (_, prepared) = pdf_prepare_annotation_patch(&square(), sent).unwrap();
        assert_eq!(prepared, map(json!({ "subtype": "square" })));
    }

    #[test]
    fn a_subtype_or_name_change_is_refused() {
        for patch in [json!({ "subtype": "circle" }), json!({ "nm": "other" })] {
            let refused = pdf_prepare_annotation_patch(&square(), map(patch)).unwrap_err();
            assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        }
        let unknown =
            pdf_prepare_annotation_patch(&square(), map(json!({ "vertices": [] }))).unwrap_err();
        assert!(unknown.message.contains("square has no field 'vertices'"), "{unknown}");
    }

    #[test]
    fn a_read_only_value_changed_is_refused_and_sent_back_kept() {
        let refused =
            pdf_resolve_annotation_patch(&square(), map(json!({ "borderStyle": "inset" })), None)
                .unwrap_err();
        assert!(refused.message.contains("can be read but not written"), "{refused}");
        assert_eq!(refused.details.unwrap()["field"], "borderStyle");
        let kept =
            pdf_resolve_annotation_patch(&square(), map(json!({ "borderStyle": "beveled" })), None)
                .unwrap();
        assert!(!kept.data.contains_key("borderStyle"));
    }

    #[test]
    fn a_custom_state_keeps_the_notes_model_and_needs_one() {
        let note = map(json!({ "subtype": "text", "stateModel": null }));
        let refused = pdf_resolve_annotation_patch(&note, map(json!({ "state": "Pending" })), None)
            .unwrap_err();
        assert_eq!(refused.details.unwrap()["field"], "stateModel");
        let standard =
            pdf_resolve_annotation_patch(&note, map(json!({ "state": "accepted" })), None).unwrap();
        assert_eq!(standard.data["stateModel"], "review");
    }

    #[test]
    fn a_free_text_style_change_moves_its_body_and_keeps_its_text() {
        let current = map(json!({ "subtype": "free-text", "intent": "free-text",
            "fontSize": 12, "fontColor": "#000000", "textAlign": "left", "contents": "Hi",
            "richText": { "body": { "family": "Helvetica", "size": 12, "color": "#000000",
                "align": "left" }, "paragraphs": [{ "runs": [{ "text": "Hi" }] }] } }));
        let resolved =
            pdf_resolve_annotation_patch(&current, map(json!({ "fontSize": 20 })), None).unwrap();
        assert_eq!(resolved.data["richText"]["body"]["size"], 20);
        assert_eq!(resolved.data["richText"]["paragraphs"][0]["runs"][0]["text"], "Hi");
        assert!(!resolved.data.contains_key("contents"), "the text isn't rewritten");
        let text = pdf_resolve_annotation_patch(&current, map(json!({ "contents": "a\nb" })), None)
            .unwrap();
        assert_eq!(text.data["contents"], "a\rb");
    }

    #[test]
    fn a_callout_line_stays_attached_when_its_box_moves() {
        let current = map(json!({ "subtype": "free-text", "intent": "free-text-callout",
            "box": { "left": 100, "bottom": 100, "right": 200, "top": 150 }, "rotation": null,
            "calloutLine": [{ "x": 10, "y": 10 }, { "x": 100, "y": 125 }] }));
        let resolved = pdf_resolve_annotation_patch(
            &current,
            map(json!({ "box": { "left": 120, "bottom": 100, "right": 220, "top": 150 } })),
            None,
        )
        .unwrap();
        assert_eq!(resolved.data["calloutLine"][1], json!({ "x": 120.0, "y": 125.0 }));
    }

    #[test]
    fn a_caption_merges_with_the_lines_and_a_scaled_dimension_relabels_itself() {
        let line = map(json!({ "subtype": "line", "intent": "line-dimension",
            "measure": { "subtype": "rectilinear",
                "x": [{ "unit": "m", "conversion": 1 }],
                "distance": [{ "unit": "m", "conversion": 1, "precision": 100 }], "area": [] },
            "captionEnabled": null, "captionPosition": "top",
            "captionOffset": { "along": 1, "perpendicular": 2 },
            "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 3, "y": 4 } } }));
        let shown =
            pdf_resolve_annotation_patch(&line, map(json!({ "captionEnabled": true })), None)
                .unwrap();
        assert_eq!(shown.data["captionEnabled"], true);
        assert_eq!(shown.data["captionPosition"], "top", "kept from the line");
        assert_eq!(shown.data["captionOffset"], json!({ "along": 1, "perpendicular": 2 }));
        assert!(!shown.data.contains_key("contents"), "no label input: {:?}", shown.data);
        let moved = pdf_resolve_annotation_patch(
            &line,
            map(
                json!({ "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 6, "y": 8 } } }),
            ),
            None,
        )
        .unwrap();
        assert_eq!(moved.data["contents"], "10.00 m");
        let foreign = pdf_resolve_annotation_patch(
            &line,
            map(json!({ "measure": { "subtype": "geospatial" } })),
            None,
        )
        .unwrap_err();
        assert_eq!(foreign.code, EngineErrorCode::InvalidArg, "{foreign}");
    }
}
