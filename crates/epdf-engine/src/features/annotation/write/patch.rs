//! What an update writes: the patch checked against the annotation it
//! changes and stated whole.
//!
//! G11. Ports `core/src/annotation/resolve/resolveAnnotationPatch.ts`
//! (`pdfPrepareAnnotationPatch`, `pdfDeriveAnnotationPatch`),
//! `resolve/sentBack.ts`, `resolve/text.ts` (`noteStateFollows`,
//! `assertRichTextAgreement`), `resolve/freeText.ts` (`freeTextFollows`,
//! `calloutEndFollows`), `calloutEnd.ts`, `resolve/measurement.ts`
//! (`measurementFollows`, as far as it is written before 8.25) and
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
) -> Result<(DraftFields, Option<AnnotationRef>), EngineError> {
    let kind = subtype.as_str();
    let mut fields = DraftFields::default();
    let mut parent = None;
    for name in read_back_fields(subtype) {
        let Some(value) = prepared.get(*name) else { continue };
        let writable = match *name {
            "parent" => read_parent(value).is_ok(),
            // A widget's actions are the form's (Task 8.21); a widget never
            // reaches an annotation update.
            "actions" => false,
            _ => read_field("update", subtype, name, value, &mut DraftFields::default()).is_ok(),
        };
        if !writable {
            return Err(invalid(format!(
                "{kind} update field '{name}': this value can be read but not written; send it back unchanged or replace it"
            ))
            .with_details(serde_json::json!({ "field": name })));
        }
    }
    for (name, value) in prepared {
        if name == "subtype" {
            continue;
        }
        match owner_of(subtype, name) {
            Some(FieldOwner::Data) if name == "parent" => parent = Some(read_parent(value)?),
            Some(FieldOwner::Data) => {
                // A foreign scale can't be written whatever 8.25 brings
                // (`measurementFollows`).
                if name == "measure"
                    && let Some(scale) = value.get("subtype").and_then(Value::as_str)
                    && !matches!(scale, "rectilinear" | "RL")
                {
                    let what = if scale == "geospatial" { "geospatial" } else { "foreign" };
                    return Err(invalid(format!(
                        "A {what} measure can't be written; leave measure out"
                    )));
                }
                read_field("update", subtype, name, value, &mut fields)?;
            }
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

/// A measurement field whose value needs 8.25's writing.
fn measurement_not_ported(kind: &str, name: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::NotImplemented,
        format!("{kind} update field '{name}': measurements are written by Task 8.25"),
    )
    .with_details(serde_json::json!({ "field": name }))
}

/// Whether a write names any of a caption's fields (`touchesCaption`).
pub fn touches_caption(value: &Map<String, Value>) -> bool {
    ["captionEnabled", "captionPosition", "captionOffset", "captionCenter"]
        .iter()
        .any(|name| value.contains_key(*name))
}

fn is_rectilinear(measure: Option<&Value>) -> bool {
    measure
        .and_then(|m| m.get("subtype"))
        .and_then(Value::as_str)
        .is_some_and(|s| matches!(s, "rectilinear" | "RL"))
}

/// `measurementFollows`, as far as this engine writes measurements before
/// Task 8.25: caption fields merge with the current caption into a complete
/// caption, which this engine writes when it is hidden, inline, with no
/// offset or centre, or removed; and nothing here derives a label.
///
/// re-port (Task 8): 8.25 writes a scale (its removal too), a leader, a
/// shown, placed or moved caption, and the label a dimension's points and
/// scale derive. Until then an update that needs one is refused
/// `NotImplemented`, before the first write.
fn measurement_follows(
    subtype: AnnotationSubtype,
    current: &Map<String, Value>,
    patch: &mut Map<String, Value>,
) -> Result<(), EngineError> {
    use AnnotationSubtype as S;
    if !matches!(subtype, S::Line | S::Polygon | S::Polyline) {
        return Ok(());
    }
    let kind = subtype.as_str();
    let current_has = |name: &str| current.get(name).is_some_and(|v| !v.is_null());
    // Removing a scale or a leader is a measurement write.
    for name in ["measure", "leader"] {
        if patch.get(name).is_some_and(Value::is_null) && current_has(name) {
            return Err(measurement_not_ported(kind, name));
        }
    }
    if subtype == S::Line {
        if touches_caption(patch) {
            let enabled = patch.get("captionEnabled").or_else(|| current.get("captionEnabled"));
            let position = patch
                .get("captionPosition")
                .filter(|p| !p.is_null())
                .or_else(|| current.get("captionPosition"));
            let offset = patch.get("captionOffset").or_else(|| current.get("captionOffset"));
            if enabled == Some(&Value::Bool(true)) {
                return Err(measurement_not_ported(kind, "captionEnabled"));
            }
            if position.and_then(Value::as_str) == Some("top") {
                return Err(measurement_not_ported(kind, "captionPosition"));
            }
            if offset.is_some_and(|o| !o.is_null()) {
                return Err(measurement_not_ported(kind, "captionOffset"));
            }
            patch.insert("captionEnabled".into(), enabled.cloned().unwrap_or(Value::Null));
            patch.insert("captionPosition".into(), Value::String("inline".into()));
            patch.insert("captionOffset".into(), Value::Null);
        }
    } else {
        // A whole-shape move carries a manual caption centre along.
        if patch.get("vertices").is_some_and(|v| !v.is_null())
            && patch.get("captionEnabled") != Some(&Value::Null)
            && !patch.contains_key("captionCenter")
            && current_has("captionCenter")
        {
            return Err(measurement_not_ported(kind, "captionCenter"));
        }
        if touches_caption(patch) {
            if patch.get("captionEnabled") == Some(&Value::Null) {
                // No caption flag: the caption and its placement go together.
                patch.insert("captionCenter".into(), Value::Null);
            } else {
                let enabled = patch
                    .get("captionEnabled")
                    .or_else(|| current.get("captionEnabled"))
                    .filter(|e| !e.is_null())
                    .cloned()
                    .unwrap_or(Value::Bool(false));
                if enabled == Value::Bool(true) {
                    return Err(measurement_not_ported(kind, "captionEnabled"));
                }
                let center = patch.get("captionCenter").or_else(|| current.get("captionCenter"));
                if center.is_some_and(|c| !c.is_null()) {
                    return Err(measurement_not_ported(kind, "captionCenter"));
                }
                patch.insert("captionEnabled".into(), enabled);
                patch.insert("captionCenter".into(), Value::Null);
            }
        }
    }
    // A dimension with a scale works its label out from its points and its
    // scale (`measurementReadout`).
    let label_inputs = ["linePoints", "vertices", "measure", "intent", "contents"];
    if label_inputs.iter().any(|name| patch.contains_key(*name)) {
        let intent = patch.get("intent").or_else(|| current.get("intent")).and_then(Value::as_str);
        let dimension = matches!(
            (subtype, intent),
            (S::Line, Some("line-dimension"))
                | (S::Polyline, Some("polyline-dimension"))
                | (S::Polygon, Some("polygon-dimension"))
        );
        let measure = match patch.get("measure") {
            Some(measure) => Some(measure),
            None => current.get("measure"),
        };
        if dimension && is_rectilinear(measure) {
            return Err(measurement_not_ported(kind, "measure"));
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
    // What follows was derived from checked values, so it reads as they do.
    let (fields, parent) = assert_annotation_patch(subtype, &data)?;
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
    fn a_shown_caption_or_a_scaled_dimension_waits_for_8_25() {
        let line = map(json!({ "subtype": "line", "intent": "line-dimension",
            "measure": { "subtype": "rectilinear" }, "captionEnabled": null,
            "captionPosition": "inline", "captionOffset": null,
            "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 1, "y": 1 } } }));
        for patch in [
            json!({ "captionEnabled": true }),
            json!({ "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 2, "y": 2 } } }),
        ] {
            let refused = pdf_resolve_annotation_patch(&line, map(patch), None).unwrap_err();
            assert_eq!(refused.code, EngineErrorCode::NotImplemented, "{refused}");
        }
        let hidden =
            pdf_resolve_annotation_patch(&line, map(json!({ "captionEnabled": false })), None)
                .unwrap();
        assert_eq!(hidden.data["captionPosition"], "inline");
    }
}
