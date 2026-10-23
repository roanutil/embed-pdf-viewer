//! What a create or an update writes, as the engine resolves it
//! (`annotation/resolve/`), and the page-space wrappers a viewer predicts
//! with (`pageSpace/helpers.ts`: `resolveRectCommand`,
//! `resolveAnnotationPatch`, `applyAnnotationPatch`, `annotationOfDraft`).
//!
//! The rules run in the file's coordinates on values flipped out of page
//! space (`space.rs`), as the TypeScript runs them; every value a caller
//! gave that no rule changed comes back as given.

use epdf_geometry::{PdfRect, Point};
use serde_json::Value;

use super::appearance::semantic_equal;
use super::facts::{
    FieldOwner, FieldSpace, annotation_defaults_of, field_names, is_drawn_rect_kind,
    is_unbaked_kind, kind_facts_of, space_of,
};
use super::geom::{
    normalize_pdf_rect, pdf_callout_end, pdf_points_bounds, pdf_quarter_turn_box, quarter_turn_of,
};
use super::measure::{
    DimensionInput, assert_pdf_float, assert_writable_measure, measurement_readout,
};
use super::shape_for_rect::{pdf_shape_for_rect, shape_fields_of};
use super::space::{pdf_rect_of_value, pdf_rect_value, to_page, to_pdf};
use super::text::{
    DescribeFont, action_tree_of, default_rich_text_body, face_for_free_text_font,
    rich_text_paragraphs_from_plain_text, rich_text_plain_text, standard_state_model_of,
};
use crate::error::{CoreError, CoreResult};
use crate::json::{self, Object, num};

fn defined(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !json::undefined(Some(v)))
}

fn number_text(x: f64) -> String {
    epdf_geometry::js::number_to_string(x)
}

// ── checks ──────────────────────────────────────────────────────────────────

/// `assertDeclaredFields(subtype, write)`.
pub fn assert_declared_fields(subtype: &str, write: &Object) -> CoreResult<()> {
    let known = field_names(subtype);
    let unknown: Vec<String> = write
        .keys()
        .filter(|name| *name != "subtype" && !known.contains(&name.as_str()))
        .map(|name| format!("'{name}'"))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(CoreError::invalid(format!("{subtype} has no field {}", unknown.join(", "))))
    }
}

/// `assertRichTextAgreement(input)`.
pub fn assert_rich_text_agreement(input: &Object) -> CoreResult<()> {
    if json::get_str(input, "subtype") != Some("free-text") {
        return Ok(());
    }
    let Some(rich) = defined(input.get("richText")) else { return Ok(()) };
    let Some(contents) = json::get(input, "contents").and_then(Value::as_str) else {
        return Ok(());
    };
    let paragraphs = rich.get("paragraphs").and_then(Value::as_array).cloned().unwrap_or_default();
    if contents != rich_text_plain_text(&paragraphs) {
        return Err(CoreError::invalid(
            "free-text: `contents` must equal the plain projection of `richText` when both are given",
        ));
    }
    Ok(())
}

fn state_needs_model(state: &str) -> CoreError {
    CoreError::invalid_field(
        format!("text: the custom state '{state}' needs a stateModel"),
        "stateModel",
    )
}

// ── reads ───────────────────────────────────────────────────────────────────

fn lower_color(object: &mut Object) {
    if let Some(color) = object.get("color").and_then(Value::as_str) {
        let lower = color.to_lowercase();
        object.insert("color".into(), Value::String(lower));
    }
}

/// `readValueOf(name, value, current)`: a written field as a read spells it.
pub fn read_value_of(name: &str, value: &Value, current: Option<&Value>) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    match name {
        "reply" => {
            let ty = defined(value.get("type")).cloned().unwrap_or(Value::String("reply".into()));
            Value::Object(json::object([
                ("to", value.get("to").cloned().unwrap_or(Value::Null)),
                ("type", ty),
            ]))
        }
        "richText" => {
            let mut body = match value
                .get("body")
                .filter(|b| json::truthy(Some(b)))
                .and_then(Value::as_object)
            {
                Some(given) => json::spread(&default_rich_text_body(), given),
                None => current
                    .and_then(|c| c.get("body"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_else(default_rich_text_body),
            };
            lower_color(&mut body);
            let paragraphs = value
                .get("paragraphs")
                .and_then(Value::as_array)
                .map(|ps| {
                    ps.iter()
                        .map(|p| {
                            let mut paragraph = p.as_object().cloned().unwrap_or_default();
                            let runs = p
                                .get("runs")
                                .and_then(Value::as_array)
                                .map(|runs| {
                                    runs.iter()
                                        .filter(|r| {
                                            r.get("text").and_then(Value::as_str) != Some("")
                                        })
                                        .map(|run| {
                                            let colored = run
                                                .get("style")
                                                .and_then(|s| s.get("color"))
                                                .is_some_and(|c| json::truthy(Some(c)));
                                            if !colored {
                                                return run.clone();
                                            }
                                            let mut run =
                                                run.as_object().cloned().unwrap_or_default();
                                            let mut style = run
                                                .get("style")
                                                .and_then(Value::as_object)
                                                .cloned()
                                                .unwrap_or_default();
                                            lower_color(&mut style);
                                            run.insert("style".into(), Value::Object(style));
                                            Value::Object(run)
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            paragraph.insert("runs".into(), Value::Array(runs));
                            Value::Object(paragraph)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Value::Object(json::object([
                ("body", Value::Object(body)),
                ("paragraphs", Value::Array(paragraphs)),
            ]))
        }
        "file" => {
            let mut out = match current.and_then(Value::as_object) {
                Some(read) => read.clone(),
                None => json::object([
                    ("size", Value::Null),
                    ("checksum", Value::Null),
                    ("createdAt", Value::Null),
                    ("modifiedAt", Value::Null),
                ]),
            };
            out.insert("name".into(), value.get("name").cloned().unwrap_or(Value::Null));
            for key in ["mimeType", "description"] {
                let v = value
                    .get(key)
                    .filter(|v| !json::nullish(Some(v)))
                    .cloned()
                    .unwrap_or(Value::Null);
                out.insert(key.into(), v);
            }
            Value::Object(out)
        }
        _ => value.clone(),
    }
}

/// `followingReads(current, resolved)`: a link's `actions` follow its `target`.
fn following_reads(current: &Object, resolved: &Object) -> Object {
    let mut out = Object::new();
    if json::get_str(current, "subtype") != Some("link")
        || json::get_str(resolved, "subtype") != Some("link")
    {
        return out;
    }
    let Some(target) = defined(resolved.get("target")) else { return out };
    let writable = target.is_null()
        || matches!(target.get("kind").and_then(Value::as_str), Some("uri" | "goto"));
    if !writable {
        return out;
    }
    let mut actions =
        json::get(current, "actions").and_then(Value::as_object).cloned().unwrap_or_default();
    actions.shift_remove("activate");
    if let Some(target) = target.as_object() {
        actions.insert("activate".into(), action_tree_of(target));
    }
    out.insert(
        "actions".into(),
        if actions.is_empty() { Value::Null } else { Value::Object(actions) },
    );
    out
}

/// `applyResolvedPatch(current, resolved)`: the patch's data fields in place,
/// each as a read spells it; the rest keep `current`'s values.
pub fn apply_resolved_patch(current: &Object, resolved: &Object) -> Object {
    let subtype = json::get_str(current, "subtype").unwrap_or("");
    let facts = kind_facts_of(subtype);
    let mut next = current.clone();
    for (name, value) in resolved {
        if name == "subtype" || json::undefined(Some(value)) {
            continue;
        }
        if facts.and_then(|f| f.owner(name)) != Some(FieldOwner::Data) {
            continue;
        }
        let read = read_value_of(name, value, current.get(name));
        next.insert(name.clone(), read);
    }
    for (name, value) in following_reads(current, resolved) {
        next.insert(name, value);
    }
    next
}

/// `mergeAnnotationPatch(current, patch)`: the patch as given, no rule run.
pub fn merge_annotation_patch(current: &Object, patch: &Object) -> Object {
    apply_resolved_patch(current, patch)
}

/// `annotationPatchBetween(before, after)`: each data field whose value differs, whole.
pub fn annotation_patch_between(before: &Object, after: &Object) -> Object {
    let mut patch = Object::new();
    let subtype = json::get_str(after, "subtype").unwrap_or("");
    let Some(facts) = kind_facts_of(subtype) else { return patch };
    for (name, owner) in facts.owners {
        if *owner != FieldOwner::Data || json::same_value(before.get(*name), after.get(*name)) {
            continue;
        }
        let now = defined(after.get(*name)).cloned().unwrap_or(Value::Null);
        patch.insert((*name).into(), now);
    }
    patch
}

// ── preparing an update ─────────────────────────────────────────────────────

fn annotation_patch_for_target(current: &Object, patch: &Object) -> CoreResult<Object> {
    let subtype = json::get_str(current, "subtype").unwrap_or("");
    if let Some(given) = defined(patch.get("subtype"))
        && given.as_str() != Some(subtype)
    {
        return Err(CoreError::invalid("Annotation subtype cannot change"));
    }
    if subtype == "unsupported" {
        return Err(CoreError::invalid(
            "an annotation of a type the engine doesn't model can't be updated",
        ));
    }
    if let Some(nm) = defined(patch.get("nm"))
        && Some(nm) != current.get("nm")
    {
        return Err(CoreError::invalid("an annotation's nm can't change after create"));
    }
    assert_declared_fields(subtype, patch)?;
    let mut out = patch.clone();
    out.insert("subtype".into(), Value::String(subtype.into()));
    Ok(out)
}

/// `pdfResolveRectCommand(current, patch)`.
pub fn pdf_resolve_rect_command(current: &Object, patch: &Object) -> CoreResult<Object> {
    let subtype = json::get_str(current, "subtype").unwrap_or("");
    let Some(rect) = defined(patch.get("rect")) else { return Ok(patch.clone()) };
    if !is_drawn_rect_kind(subtype) {
        return Ok(patch.clone());
    }
    let rest = json::without(patch, "rect");
    let Some(rect) = pdf_rect_of_value(rect)
        .filter(|r| [r.left, r.bottom, r.right, r.top].iter().all(|e| e.is_finite()))
    else {
        return Err(CoreError::invalid_field(
            format!("{subtype} update field 'rect': not a rect of finite numbers"),
            "rect",
        ));
    };
    if semantic_equal(Some(&pdf_rect_value(normalize_pdf_rect(rect))), current.get("rect")) {
        return Ok(rest);
    }
    let changed = shape_fields_of(subtype).into_iter().find(|name| {
        defined(patch.get(*name))
            .is_some_and(|value| !semantic_equal(Some(value), current.get(*name)))
    });
    if let Some(changed) = changed {
        return Err(CoreError::invalid_field(
            format!(
                "{subtype} update field 'rect': a new rect moves the shape, so it can't come with a new '{changed}'"
            ),
            "rect",
        ));
    }
    let placed = pdf_shape_for_rect(
        current,
        PdfRect { left: rect.left, right: rect.right, bottom: rect.bottom, top: rect.top },
    )?;
    Ok(json::spread(&rest, &placed))
}

fn without_unchanged_colors(current: &Object, patch: &Object) -> Object {
    let subtype = json::get_str(current, "subtype").unwrap_or("");
    let colors = kind_facts_of(subtype).map(|f| f.colors).unwrap_or(&[]);
    patch
        .iter()
        .filter(|(name, value)| {
            if name.as_str() == "subtype" {
                return true;
            }
            let was = current.get(name.as_str()).and_then(Value::as_str);
            let drop = colors.contains(&name.as_str())
                && value
                    .as_str()
                    .is_some_and(|v| was.is_some_and(|w| v.to_lowercase() == w.to_lowercase()));
            !drop
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn without_unchanged_fields(current: &Object, patch: &Object) -> Object {
    patch
        .iter()
        .filter(|(name, value)| {
            name.as_str() == "subtype"
                || !(!json::undefined(Some(value))
                    && semantic_equal(Some(value), current.get(name.as_str())))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// `pdfPrepareAnnotationPatch(current, patch)`.
pub fn pdf_prepare_annotation_patch(current: &Object, patch: &Object) -> CoreResult<Object> {
    let targeted = annotation_patch_for_target(current, patch)?;
    let commanded = pdf_resolve_rect_command(current, &targeted)?;
    Ok(without_unchanged_fields(current, &without_unchanged_colors(current, &commanded)))
}

// ── what follows ────────────────────────────────────────────────────────────

fn note_state_follows(current: &Object, patch: &Object) -> CoreResult<Object> {
    if json::get_str(current, "subtype") != Some("text")
        || json::get_str(patch, "subtype") != Some("text")
    {
        return Ok(patch.clone());
    }
    let Some(state) = json::get(patch, "state").and_then(Value::as_str) else {
        return Ok(patch.clone());
    };
    if defined(patch.get("stateModel")).is_some() {
        return Ok(patch.clone());
    }
    if let Some(model) = standard_state_model_of(state) {
        let mut out = patch.clone();
        out.insert("stateModel".into(), Value::String(model.into()));
        return Ok(out);
    }
    if !json::truthy(current.get("stateModel")) {
        return Err(state_needs_model(state));
    }
    Ok(patch.clone())
}

fn note_draft_state_follows(draft: &Object) -> CoreResult<Object> {
    if json::get_str(draft, "subtype") != Some("text") || !json::nullish(draft.get("stateModel")) {
        return Ok(draft.clone());
    }
    let Some(state) = json::get(draft, "state").and_then(Value::as_str) else {
        return Ok(draft.clone());
    };
    let Some(model) = standard_state_model_of(state) else { return Err(state_needs_model(state)) };
    let mut out = draft.clone();
    out.insert("stateModel".into(), Value::String(model.into()));
    Ok(out)
}

/// `bodyWithStyle(base, style, describe)`.
fn body_with_style(base: &Object, style: &Object, describe: Option<DescribeFont>) -> Object {
    let mut body = base.clone();
    if let Some(font) = defined(style.get("fontFamily")) {
        let face = face_for_free_text_font(font.as_str().unwrap_or(""), describe);
        body.insert("family".into(), Value::String(face.family));
        if let Some(weight) = face.weight {
            body.insert("weight".into(), num(weight));
        }
        if let Some(italic) = face.italic {
            body.insert("italic".into(), Value::Bool(italic));
        }
    }
    if let Some(size) = defined(style.get("fontSize")) {
        body.insert("size".into(), size.clone());
    }
    if let Some(color) = json::get(style, "fontColor") {
        body.insert("color".into(), color.clone());
    }
    if let Some(align) = defined(style.get("textAlign")) {
        body.insert("align".into(), align.clone());
    }
    body
}

fn assert_callout_intent(intent: Option<&str>, callout_line: Option<&Value>) -> CoreResult<()> {
    if !json::nullish(callout_line) && intent != Some("free-text-callout") {
        let intent = intent.unwrap_or("undefined");
        return Err(CoreError::invalid_field(
            format!("free-text: a callout line needs intent 'free-text-callout', not '{intent}'"),
            "intent",
        ));
    }
    Ok(())
}

fn style_of_body(body: &Object, current: &Object, text: &Object) -> Object {
    let mut style = Object::new();
    if json::undefined(text.get("fontSize"))
        && let Some(size) = defined(body.get("size"))
        && !json::same_value(Some(size), current.get("fontSize"))
    {
        style.insert("fontSize".into(), size.clone());
    }
    if json::undefined(text.get("fontColor"))
        && let Some(color) = defined(body.get("color"))
    {
        let lower = color.as_str().unwrap_or("").to_lowercase();
        let was = json::get(current, "fontColor").and_then(Value::as_str).map(str::to_lowercase);
        if Some(&lower) != was.as_ref() {
            style.insert("fontColor".into(), Value::String(lower));
        }
    }
    if json::undefined(text.get("textAlign"))
        && let Some(align) = defined(body.get("align"))
        && Some(align) != current.get("textAlign")
    {
        style.insert("textAlign".into(), align.clone());
    }
    style
}

fn free_text_follows(
    current: &Object,
    patch: &Object,
    describe: Option<DescribeFont>,
) -> CoreResult<Object> {
    if json::get_str(current, "subtype") != Some("free-text")
        || json::get_str(patch, "subtype") != Some("free-text")
    {
        return Ok(patch.clone());
    }
    if defined(patch.get("intent")).is_some() || defined(patch.get("calloutLine")).is_some() {
        let intent =
            defined(patch.get("intent")).or_else(|| current.get("intent")).and_then(Value::as_str);
        let line = if defined(patch.get("calloutLine")).is_some() {
            patch.get("calloutLine")
        } else {
            current.get("calloutLine")
        };
        assert_callout_intent(intent, line)?;
    }
    let changes = body_with_style(&Object::new(), patch, describe);
    let text_changed =
        defined(patch.get("richText")).is_some() || defined(patch.get("contents")).is_some();
    if !text_changed && changes.is_empty() {
        return Ok(patch.clone());
    }
    let current_rich =
        current.get("richText").and_then(Value::as_object).cloned().unwrap_or_default();
    let current_body =
        current_rich.get("body").and_then(Value::as_object).cloned().unwrap_or_default();
    let given_body = patch
        .get("richText")
        .and_then(|r| r.get("body"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let body = json::spread(&json::spread(&current_body, &given_body), &changes);
    let paragraphs = match patch
        .get("richText")
        .and_then(|r| defined(r.get("paragraphs")))
        .filter(|p| !p.is_null())
    {
        Some(p) => p.clone(),
        None => match defined(patch.get("contents")) {
            Some(contents) => {
                Value::Array(rich_text_paragraphs_from_plain_text(contents.as_str().unwrap_or("")))
            }
            None => current_rich.get("paragraphs").cloned().unwrap_or(Value::Array(Vec::new())),
        },
    };
    let rich_text =
        json::object([("body", Value::Object(body.clone())), ("paragraphs", paragraphs.clone())]);
    let mut out = patch.clone();
    for (k, v) in style_of_body(&body, current, patch) {
        out.insert(k, v);
    }
    out.insert("richText".into(), Value::Object(rich_text));
    if text_changed {
        let plain = rich_text_plain_text(paragraphs.as_array().map(Vec::as_slice).unwrap_or(&[]));
        out.insert("contents".into(), Value::String(plain));
    }
    Ok(out)
}

fn callout_end_follows(current: &Object, patch: &Object) -> Object {
    if json::get_str(current, "subtype") != Some("free-text")
        || json::get_str(patch, "subtype") != Some("free-text")
    {
        return patch.clone();
    }
    let Some(line) = json::get(current, "calloutLine").and_then(Value::as_array) else {
        return patch.clone();
    };
    if defined(patch.get("calloutLine")).is_some() {
        return patch.clone();
    }
    if defined(patch.get("box")).is_none() && defined(patch.get("rotation")).is_none() {
        return patch.clone();
    }
    let rect_value =
        json::get(patch, "box").or_else(|| current.get("box")).cloned().unwrap_or(Value::Null);
    let rect = super::space::pdf_rect_loose(&rect_value);
    let rotation_value = if defined(patch.get("rotation")).is_none() {
        current.get("rotation")
    } else {
        patch.get("rotation")
    };
    let rotation = rotation_value.and_then(json::number).unwrap_or(0.0);
    let tip = line.first().cloned().unwrap_or(Value::Null);
    let knee = if line.len() == 3 { line.get(1).cloned() } else { None };
    let toward = json::point_loose(knee.as_ref().unwrap_or(&tip));
    let end = json::point_value(pdf_callout_end(rect, toward, rotation));
    let mut out = patch.clone();
    let new_line = match knee {
        Some(knee) => vec![tip, knee, end],
        None => vec![tip, end],
    };
    out.insert("calloutLine".into(), Value::Array(new_line));
    out
}

fn free_text_draft_follows(draft: &Object, describe: Option<DescribeFont>) -> CoreResult<Object> {
    if json::get_str(draft, "subtype") != Some("free-text") {
        return Ok(draft.clone());
    }
    let intent = match json::get(draft, "intent").and_then(Value::as_str) {
        Some(intent) => intent.to_string(),
        None if !json::nullish(draft.get("calloutLine")) => "free-text-callout".into(),
        None => "free-text".into(),
    };
    assert_callout_intent(Some(&intent), draft.get("calloutLine"))?;
    let base = draft
        .get("richText")
        .and_then(|r| r.get("body"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let body = body_with_style(&base, draft, describe);
    let paragraphs = match draft
        .get("richText")
        .and_then(|r| json::get(r.as_object()?, "paragraphs").cloned())
    {
        Some(p) => p,
        None => Value::Array(rich_text_paragraphs_from_plain_text(
            json::get(draft, "contents").and_then(Value::as_str).unwrap_or(""),
        )),
    };
    let plain = rich_text_plain_text(paragraphs.as_array().map(Vec::as_slice).unwrap_or(&[]));
    let mut out = draft.clone();
    out.insert("intent".into(), Value::String(intent));
    out.insert(
        "richText".into(),
        Value::Object(json::object([("body", Value::Object(body)), ("paragraphs", paragraphs)])),
    );
    out.insert("contents".into(), Value::String(plain));
    Ok(out)
}

fn redact_draft_follows(draft: &Object) -> Object {
    if json::get_str(draft, "subtype") != Some("redact") || !json::nullish(draft.get("rect")) {
        return draft.clone();
    }
    let quads =
        json::get(draft, "quadPoints").and_then(Value::as_array).cloned().unwrap_or_default();
    if quads.is_empty() {
        return draft.clone();
    }
    let points: Vec<Point> = quads
        .iter()
        .flat_map(|q| {
            ["upperLeft", "upperRight", "lowerLeft", "lowerRight"].map(|n| {
                q.get(n).map(json::point_loose).unwrap_or(Point { x: f64::NAN, y: f64::NAN })
            })
        })
        .collect();
    let rect = pdf_points_bounds(&points);
    let mut out = draft.clone();
    out.insert(
        "rect".into(),
        Value::Object(json::object([
            ("left", num(rect.left)),
            ("bottom", num(rect.bottom)),
            ("right", num(rect.right)),
            ("top", num(rect.top)),
        ])),
    );
    out
}

fn box_draft_follows(draft: &Object) -> CoreResult<Object> {
    let subtype = json::get_str(draft, "subtype").unwrap_or("");
    if space_of(Some(subtype), "box") != Some(FieldSpace::Box) || kind_facts_of(subtype).is_none() {
        return Ok(draft.clone());
    }
    if defined(draft.get("box")).is_some() {
        return Ok(draft.clone());
    }
    let Some(rect) = defined(draft.get("rect")) else {
        return Err(CoreError::invalid_field(
            format!("{subtype} create: give its `box`, or its `rect` with a quarter turn"),
            "box",
        ));
    };
    let Some(rect) = pdf_rect_of_value(rect)
        .filter(|r| [r.left, r.bottom, r.right, r.top].iter().all(|e| e.is_finite()))
    else {
        return Err(CoreError::invalid_field(
            format!("{subtype} create field 'rect': not a rect of finite numbers"),
            "rect",
        ));
    };
    let rotation = json::get_num(draft, "rotation").unwrap_or(0.0);
    let Some(turn) = quarter_turn_of(rotation) else {
        return Err(CoreError::invalid_field(
            format!(
                "{subtype} create field 'rect': a rect places a box at a quarter turn; at {}° give its `box`",
                number_text(rotation)
            ),
            "rect",
        ));
    };
    let mut out = json::without(draft, "rect");
    let placed = pdf_quarter_turn_box(normalize_pdf_rect(rect), turn);
    out.insert(
        "box".into(),
        Value::Object(json::object([
            ("left", num(placed.left)),
            ("right", num(placed.right)),
            ("bottom", num(placed.bottom)),
            ("top", num(placed.top)),
        ])),
    );
    Ok(out)
}

// ── measurements ────────────────────────────────────────────────────────────

const LABEL_INPUTS: [&str; 5] = ["linePoints", "vertices", "measure", "intent", "contents"];
const CAPTION_FIELDS: [&str; 4] =
    ["captionEnabled", "captionPosition", "captionOffset", "captionCenter"];

/// `touchesCaption(value)`.
pub fn touches_caption(value: &Object) -> bool {
    CAPTION_FIELDS.iter().any(|n| defined(value.get(*n)).is_some())
}

fn validate_dimension(subtype: &str, write: &Object) -> CoreResult<()> {
    let run = || -> CoreResult<()> {
        if let Some(measure) = json::get(write, "measure")
            && measure.get("subtype").and_then(Value::as_str) == Some("rectilinear")
        {
            assert_writable_measure(measure)?;
        }
        if subtype == "line"
            && let Some(leader) = json::get(write, "leader")
            && !json::is_plain_object(leader)
        {
            return Err(CoreError::invalid("Invalid line leader"));
        }
        let intents: &[&str] = match subtype {
            "line" => &["line-dimension", "line-arrow"],
            "polygon" => &["polygon-dimension", "polygon-cloud"],
            _ => &["polyline-dimension"],
        };
        if let Some(intent) = json::get(write, "intent")
            && !intent.as_str().is_some_and(|i| intents.contains(&i))
        {
            return Err(CoreError::invalid("Invalid measurement intent"));
        }
        if let Some(enabled) = json::get(write, "captionEnabled")
            && !enabled.is_boolean()
        {
            return Err(CoreError::invalid("Invalid caption visibility"));
        }
        if let Some(position) = json::get(write, "captionPosition")
            && !matches!(position.as_str(), Some("inline" | "top"))
        {
            return Err(CoreError::invalid("Invalid caption position"));
        }
        if let Some(offset) = write.get("captionOffset").filter(|v| json::truthy(Some(v))) {
            assert_pdf_float(offset.get("along"))?;
            assert_pdf_float(offset.get("perpendicular"))?;
        }
        if let Some(center) = write.get("captionCenter").filter(|v| json::truthy(Some(v))) {
            assert_pdf_float(center.get("x"))?;
            assert_pdf_float(center.get("y"))?;
        }
        if let Some(leader) = write.get("leader").filter(|v| json::truthy(Some(v))) {
            let extension = defined(leader.get("extension"))
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or(num(0.0));
            let offset =
                defined(leader.get("offset")).filter(|v| !v.is_null()).cloned().unwrap_or(num(0.0));
            assert_pdf_float(leader.get("length"))?;
            assert_pdf_float(Some(&extension))?;
            assert_pdf_float(Some(&offset))?;
            if json::number(&extension).is_some_and(|e| e < 0.0)
                || json::number(&offset).is_some_and(|o| o < 0.0)
            {
                return Err(CoreError::invalid("Leader extension and offset must be nonnegative"));
            }
        }
        let points: Vec<Value> = if write.contains_key("linePoints") {
            match json::get(write, "linePoints") {
                Some(lp) => vec![
                    lp.get("start").cloned().unwrap_or(Value::Null),
                    lp.get("end").cloned().unwrap_or(Value::Null),
                ],
                None => Vec::new(),
            }
        } else if write.contains_key("vertices") {
            json::get(write, "vertices").and_then(Value::as_array).cloned().unwrap_or_default()
        } else {
            Vec::new()
        };
        for point in &points {
            assert_pdf_float(point.get("x"))?;
            assert_pdf_float(point.get("y"))?;
        }
        Ok(())
    };
    run().map_err(|error| CoreError::invalid(error.message))
}

fn move_center(before: &[Point], after: &[Point], center: Point) -> Option<Point> {
    if before.len() != after.len() || before.len() < 2 {
        return None;
    }
    let origin = before[0];
    let moved = after[0];
    let index = before
        .iter()
        .position(|p| epdf_geometry::js::hypot(p.x - origin.x, p.y - origin.y) > 1e-6)?;
    let from = Point { x: before[index].x - origin.x, y: before[index].y - origin.y };
    let to = Point { x: after[index].x - moved.x, y: after[index].y - moved.y };
    let length = from.x * from.x + from.y * from.y;
    let cos = (from.x * to.x + from.y * to.y) / length;
    let sin = (from.x * to.y - from.y * to.x) / length;
    if (cos * cos + sin * sin - 1.0).abs() > 1e-5 {
        return None;
    }
    let transform = |p: Point| Point {
        x: moved.x + cos * (p.x - origin.x) - sin * (p.y - origin.y),
        y: moved.y + sin * (p.x - origin.x) + cos * (p.y - origin.y),
    };
    let fits = before.iter().zip(after).all(|(p, q)| {
        let placed = transform(*p);
        epdf_geometry::js::hypot(placed.x - q.x, placed.y - q.y) <= 1e-3
    });
    fits.then(|| transform(center))
}

fn foreign_measure(subtype: &Value) -> CoreError {
    let which = if subtype.as_str() == Some("geospatial") { "geospatial" } else { "foreign" };
    CoreError::invalid(format!("A {which} measure can't be written; leave measure out"))
}

/// `deriveMeasurementLabel(dto)`.
fn derive_measurement_label(dto: &Object) -> Object {
    let readout = measurement_readout(&DimensionInput::of(dto));
    match readout.label() {
        Some(label) if json::get_str(dto, "contents") != Some(label) => {
            let mut out = dto.clone();
            out.insert("contents".into(), Value::String(label.into()));
            out
        }
        _ => dto.clone(),
    }
}

fn is_dimension_kind(subtype: &str) -> bool {
    matches!(subtype, "line" | "polygon" | "polyline")
}

/// `resolveMeasurementDraft(draft)`.
pub fn resolve_measurement_draft(draft: &Object) -> CoreResult<Object> {
    let subtype = json::get_str(draft, "subtype").unwrap_or("");
    if !is_dimension_kind(subtype) {
        return Ok(draft.clone());
    }
    validate_dimension(subtype, draft)?;
    if let Some(measure) = json::get(draft, "measure") {
        let ms = measure.get("subtype").cloned().unwrap_or(Value::Null);
        if ms.as_str() != Some("rectilinear") {
            return Err(foreign_measure(&ms));
        }
    }
    let mut resolved = draft.clone();
    if touches_caption(draft) {
        if subtype == "line" {
            let enabled =
                defined(draft.get("captionEnabled")).cloned().unwrap_or(Value::Bool(false));
            let position = json::get(draft, "captionPosition")
                .cloned()
                .unwrap_or(Value::String("inline".into()));
            let offset = json::get(draft, "captionOffset").cloned().unwrap_or(Value::Null);
            resolved.insert("captionEnabled".into(), enabled);
            resolved.insert("captionPosition".into(), position);
            resolved.insert("captionOffset".into(), offset);
        } else {
            let enabled = match json::get(draft, "captionEnabled") {
                Some(v) => v.clone(),
                None if !json::nullish(draft.get("captionCenter")) => Value::Bool(false),
                None => Value::Null,
            };
            let center = json::get(draft, "captionCenter").cloned().unwrap_or(Value::Null);
            resolved.insert("captionEnabled".into(), enabled);
            resolved.insert("captionCenter".into(), center);
        }
    }
    Ok(derive_measurement_label(&resolved))
}

/// Set `name` to `value` when it is defined; drop it when it is `undefined`.
fn set_or_drop(object: &mut Object, name: &str, value: Option<Value>) {
    match value.filter(|v| !json::undefined(Some(v))) {
        Some(v) => {
            object.insert(name.into(), v);
        }
        None => {
            object.shift_remove(name);
        }
    }
}

/// A measure's `subtype`, `None` when absent or `undefined`.
fn defined_subtype(measure: &Value) -> Option<&Value> {
    measure.get("subtype").filter(|v| !json::undefined(Some(v)))
}

/// `measurementFollows(current, patch)`.
pub fn measurement_follows(current: &Object, patch: &Object) -> CoreResult<Object> {
    let subtype = json::get_str(current, "subtype").unwrap_or("");
    if !is_dimension_kind(subtype) {
        return Ok(patch.clone());
    }
    let mut next = patch.clone();
    next.insert("subtype".into(), Value::String(subtype.into()));
    validate_dimension(subtype, &next)?;
    if let Some(measure) = json::get(&next, "measure").cloned() {
        // `measure.subtype` and `current.measure?.subtype`, an absent or
        // `undefined` member as `None`, so `null` and `undefined` differ as
        // `!==` tells them apart.
        let ms = defined_subtype(&measure);
        if ms.and_then(Value::as_str) != Some("rectilinear") {
            let current_ms = json::get(current, "measure").and_then(defined_subtype);
            if current_ms != ms {
                return Err(foreign_measure(ms.unwrap_or(&Value::Null)));
            }
            next.shift_remove("measure");
        }
    }
    if subtype == "line" {
        if touches_caption(&next) {
            let enabled = defined(next.get("captionEnabled"))
                .cloned()
                .or_else(|| current.get("captionEnabled").cloned());
            let position = json::get(&next, "captionPosition")
                .or_else(|| current.get("captionPosition"))
                .cloned();
            let offset = defined(next.get("captionOffset"))
                .cloned()
                .or_else(|| current.get("captionOffset").cloned());
            set_or_drop(&mut next, "captionEnabled", enabled);
            set_or_drop(&mut next, "captionPosition", position);
            set_or_drop(&mut next, "captionOffset", offset);
        }
    } else {
        let vertices_given = next.get("vertices").is_some_and(|v| json::truthy(Some(v)));
        if vertices_given
            && next.get("captionEnabled") != Some(&Value::Null)
            && json::undefined(next.get("captionCenter"))
            && json::truthy(current.get("captionCenter"))
        {
            let before = current.get("vertices").and_then(json::points).unwrap_or_default();
            let after = next.get("vertices").and_then(json::points).unwrap_or_default();
            let center = current.get("captionCenter").map(json::point_loose).unwrap_or_default();
            if let Some(center) = move_center(&before, &after, center) {
                next.insert("captionCenter".into(), json::point_value(center));
            }
        }
        if touches_caption(&next) {
            if next.get("captionEnabled") == Some(&Value::Null) {
                next.insert("captionEnabled".into(), Value::Null);
                next.insert("captionCenter".into(), Value::Null);
            } else {
                let enabled = json::get(&next, "captionEnabled")
                    .or_else(|| json::get(current, "captionEnabled"))
                    .cloned()
                    .unwrap_or(Value::Bool(false));
                let center = if json::undefined(next.get("captionCenter")) {
                    current.get("captionCenter").cloned()
                } else {
                    next.get("captionCenter").cloned()
                };
                next.insert("captionEnabled".into(), enabled);
                set_or_drop(&mut next, "captionCenter", center);
            }
        }
    }
    if LABEL_INPUTS.iter().any(|k| defined(next.get(*k)).is_some()) {
        let merged = json::spread(current, &next);
        let readout = measurement_readout(&DimensionInput::of(&merged));
        if let Some(label) = readout.label() {
            next.insert("contents".into(), Value::String(label.into()));
        }
    }
    validate_dimension(subtype, &next)?;
    Ok(next)
}

// ── the predictor ───────────────────────────────────────────────────────────

/// `pdfDeriveAnnotationPatch(current, prepared)`.
fn pdf_derive_annotation_patch(
    current: &Object,
    prepared: &Object,
    describe: Option<DescribeFont>,
) -> CoreResult<Object> {
    let resolved = note_state_follows(current, prepared)?;
    assert_rich_text_agreement(&resolved)?;
    let text = callout_end_follows(current, &free_text_follows(current, &resolved, describe)?);
    measurement_follows(current, &text)
}

/// `pdfPredictAnnotationPatch(current, patch)`.
pub fn pdf_predict_annotation_patch(
    current: &Object,
    patch: &Object,
    describe: Option<DescribeFont>,
) -> CoreResult<Object> {
    pdf_derive_annotation_patch(current, &pdf_prepare_annotation_patch(current, patch)?, describe)
}

/// `pdfPredictAnnotationDraft(draft)`.
pub fn pdf_predict_annotation_draft(
    draft: &Object,
    describe: Option<DescribeFont>,
) -> CoreResult<Object> {
    let subtype = json::get_str(draft, "subtype").unwrap_or("");
    assert_declared_fields(subtype, draft)?;
    let prepared = box_draft_follows(draft)?;
    assert_rich_text_agreement(&prepared)?;
    let followed = redact_draft_follows(&free_text_draft_follows(
        &note_draft_state_follows(&prepared)?,
        describe,
    )?);
    resolve_measurement_draft(&followed)
}

/// Put back each value `given` stated that the round trip left semantically equal.
fn restore_given(back: &mut Object, given: &Object) {
    let names: Vec<String> = back.keys().cloned().collect();
    for name in names {
        if let Some(value) = defined(given.get(&name))
            && semantic_equal(back.get(&name), Some(value))
        {
            back.insert(name, value.clone());
        }
    }
}

// ── page-space wrappers ─────────────────────────────────────────────────────

/// `resolveRectCommand(annotation, patch)`: a page-space patch with its
/// `rect` resolved as the engine resolves it on update.
pub fn resolve_rect_command(annotation: &Object, patch: &Object) -> CoreResult<Object> {
    let subtype = json::get_str(annotation, "subtype").unwrap_or("");
    if defined(patch.get("rect")).is_none() || !is_drawn_rect_kind(subtype) {
        return Ok(patch.clone());
    }
    let mut names = vec!["rect"];
    names.extend(shape_fields_of(subtype));
    let pick = |from: &Object| {
        let mut out = json::object([("subtype", Value::String(subtype.into()))]);
        for name in &names {
            if let Some(value) = defined(from.get(*name)) {
                out.insert((*name).into(), value.clone());
            }
        }
        out
    };
    let current = to_pdf(&pick(annotation))?;
    let command = to_pdf(&pick(patch))?;
    let placed = json::without(&to_page(&pdf_resolve_rect_command(&current, &command)?), "subtype");
    let mut out = json::without(patch, "rect");
    for (name, value) in placed {
        let given = defined(patch.get(&name)).filter(|g| semantic_equal(Some(&value), Some(g)));
        out.insert(name.clone(), given.cloned().unwrap_or(value));
    }
    Ok(out)
}

/// `resolveAnnotationPatch(current, patch)`.
pub fn resolve_annotation_patch(
    current: &Object,
    patch: &Object,
    describe: Option<DescribeFont>,
) -> CoreResult<Object> {
    let mut full = patch.clone();
    if defined(patch.get("subtype")).is_none() {
        full.insert("subtype".into(), current.get("subtype").cloned().unwrap_or(Value::Null));
    }
    let resolved = pdf_predict_annotation_patch(&to_pdf(current)?, &to_pdf(&full)?, describe)?;
    let mut back = to_page(&resolved);
    restore_given(&mut back, patch);
    Ok(back)
}

/// `applyAnnotationPatch(current, patch)`.
pub fn apply_annotation_patch(
    current: &Object,
    patch: &Object,
    describe: Option<DescribeFont>,
) -> CoreResult<Object> {
    Ok(apply_resolved_patch(current, &resolve_annotation_patch(current, patch, describe)?))
}

/// `resolveAnnotationDraft(draft)`.
pub fn resolve_annotation_draft(
    draft: &Object,
    describe: Option<DescribeFont>,
) -> CoreResult<Object> {
    let resolved = pdf_predict_annotation_draft(&to_pdf(draft)?, describe)?;
    let mut back = to_page(&resolved);
    restore_given(&mut back, draft);
    Ok(back)
}

/// What a draft alone doesn't say about the annotation it creates (`DraftContext`).
#[derive(Clone, Debug, Default)]
pub struct DraftContext {
    /// The ref the read names it by.
    pub reference: Value,
    pub nm: Option<String>,
    /// Who creates it and when, by field name.
    pub attribution: Option<Object>,
    /// The box around what it draws, for a kind whose `rect` the engine works out.
    pub rect: Option<Value>,
}

/// `annotationOfResolvedDraft(resolved, context)`.
pub fn annotation_of_resolved_draft(resolved: &Object, context: &DraftContext) -> Object {
    let subtype = json::get_str(resolved, "subtype").unwrap_or("").to_string();
    let defaults = annotation_defaults_of(&subtype);
    let mut read = json::object([("subtype", Value::String(subtype.clone()))]);
    if let Some(facts) = kind_facts_of(&subtype) {
        for (name, owner) in facts.owners {
            let value = match owner {
                FieldOwner::Data => match defined(resolved.get(*name)) {
                    Some(given) => read_value_of(name, given, None),
                    None => {
                        defaults.get(*name).filter(|v| !v.is_null()).cloned().unwrap_or(Value::Null)
                    }
                },
                FieldOwner::Attribution => context
                    .attribution
                    .as_ref()
                    .and_then(|a| a.get(*name))
                    .filter(|v| !json::nullish(Some(v)))
                    .cloned()
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            };
            read.insert((*name).into(), value);
        }
    }
    if read.get("nm") == Some(&Value::Null)
        && let Some(nm) = &context.nm
    {
        read.insert("nm".into(), Value::String(nm.clone()));
    }
    read.insert("ref".into(), context.reference.clone());
    read.insert("page".into(), context.reference.get("page").cloned().unwrap_or(Value::Null));
    read.insert("hasAppearance".into(), Value::Bool(!is_unbaked_kind(&subtype)));
    if read.get("rect").is_none_or(Value::is_null) {
        let rect = context
            .rect
            .clone()
            .or_else(|| json::get(resolved, "rect").cloned())
            .or_else(|| json::get(resolved, "box").cloned())
            .unwrap_or_else(|| json::rect_value(epdf_geometry::Rect::default()));
        read.insert("rect".into(), rect);
    }
    if subtype == "free-text" {
        let given_body = resolved
            .get("richText")
            .and_then(|r| r.get("body"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut body = json::spread(&default_rich_text_body(), &given_body);
        lower_color(&mut body);
        let mut rich = read.get("richText").and_then(Value::as_object).cloned().unwrap_or_default();
        rich.insert("body".into(), Value::Object(body.clone()));
        read.insert("richText".into(), Value::Object(rich));
        read.insert("fontSize".into(), body.get("size").cloned().unwrap_or(Value::Null));
        read.insert("fontColor".into(), body.get("color").cloned().unwrap_or(Value::Null));
        read.insert("textAlign".into(), body.get("align").cloned().unwrap_or(Value::Null));
    }
    if subtype == "link" {
        let target = resolved.get("target").and_then(Value::as_object);
        let actions = match target {
            Some(t) if matches!(json::get_str(t, "kind"), Some("uri" | "goto")) => {
                Value::Object(json::object([("activate", action_tree_of(t))]))
            }
            _ => Value::Null,
        };
        read.insert("actions".into(), actions);
    }
    if subtype == "widget" {
        read.insert("fieldFamily".into(), Value::String("unknown".into()));
    }
    read
}

/// `annotationOfDraft(draft, context)`: the annotation the engine reads back after a create.
pub fn annotation_of_draft(draft: &Object, context: &DraftContext) -> CoreResult<Object> {
    Ok(annotation_of_resolved_draft(&resolve_annotation_draft(draft, None)?, context))
}
