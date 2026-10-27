//! The function tables of `plugins/annotation-pure.json` and
//! `plugins/annotation-repository.json`: each name the TypeScript suite
//! calls, run on the Rust port with its arguments as canonical JSON.

use std::rc::Rc;

use epdf_annotation_core::font_css::css_font_family_for_font;
use epdf_annotation_core::kinds::widget_kind_of;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::record::{
    from_dto, link_child_rects, shape_of, style_of, widget_appearance_of, with_shape, with_values,
    writable_target,
};
use epdf_annotation_core::richtext::RichTextRange;
use epdf_annotation_core::types::{
    Model, ModelAnnotation, PageRef, Shape, Style, TextStyle, ViewEnv,
};
use epdf_annotation_core::update::changes::draft_of;
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use plugin_conformance::FunctionTable;
use serde_json::{Map, Value, json};

use crate::binary::sniff_binary_metadata;
use crate::comments::build::build_comment_threads;
use crate::engine::{color_of, rgb_of};
use crate::host_contract::preview_bucket;
use crate::js::{Object, num};
use crate::model::{
    AnnotationState, GhostPointer, initial_annotation_state, set_ghost_at, set_text_selection,
    with_session, with_snap,
};
use crate::rich_text::{
    Face, FontHandle, RichTextDocument, TextSelection, body_from_text_style,
    css_font_family_for_face, face_for_font, family_key, font_for_face, range_props, rich_doc_of,
    run_delta_for_fields, runs_in_range, strip_body_defaults, text_commit_patch,
};
use crate::sync::records::{core_error, dto_key};
use crate::text_item::build_text_items;
use crate::tools::definitions::{build_tool_registry, tool_touch_of};
use crate::write::placement::icon_placement;

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !epdf_kernel::is_undefined(v))
}

fn object(args: &[Value], i: usize) -> Object {
    arg(args, i).and_then(Value::as_object).cloned().unwrap_or_default()
}

fn text(args: &[Value], i: usize) -> String {
    arg(args, i).and_then(Value::as_str).unwrap_or("undefined").to_string()
}

fn malformed(what: &str, e: impl std::fmt::Display) -> Error {
    Error::new(format!("[harness] malformed {what}: {e}"))
}

fn fonts(args: &[Value], i: usize) -> Option<Vec<FontHandle>> {
    arg(args, i)
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(FontHandle::read).collect())
}

fn record(value: &Value) -> Result<ModelAnnotation, Error> {
    ModelAnnotation::read(value).map_err(|e| malformed("record", e.0))
}

fn doc_of(value: &Value) -> RichTextDocument {
    RichTextDocument {
        body: value.get("body").and_then(Value::as_object).cloned().unwrap_or_default(),
        paragraphs: value.get("paragraphs").and_then(Value::as_array).cloned().unwrap_or_default(),
    }
}

fn range_of(value: Option<&Value>) -> RichTextRange {
    let n = |k: &str| value.and_then(|v| v.get(k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
    RichTextRange { start: n("start"), end: n("end") }
}

/// A state read from its JSON form.
fn state_of(value: &Value) -> Result<Rc<AnnotationState>, Error> {
    let mut session = value.get("session").and_then(Value::as_object).cloned().unwrap_or_default();
    session.insert("byId".into(), json!({}));
    session.insert("order".into(), json!([]));
    let model = Model::read(&Value::Object(session)).map_err(|e| malformed("state", e.0))?;
    let mut state = initial_annotation_state();
    state.session = model.session();
    let vector = value
        .get("vector")
        .and_then(Value::as_object)
        .map(|o| o.keys().map(|k| (k.clone(), ())).collect());
    if let Some(vector) = vector {
        state.vector = Rc::new(vector);
    }
    Ok(Rc::new(state))
}

fn session_of(value: &Value) -> Result<epdf_annotation_core::types::Session, Error> {
    let mut session = value.as_object().cloned().unwrap_or_default();
    session.insert("byId".into(), json!({}));
    session.insert("order".into(), json!([]));
    Model::read(&Value::Object(session)).map(|m| m.session()).map_err(|e| malformed("session", e.0))
}

// ── pure ────────────────────────────────────────────────────────────────────

fn build_tool_registry_fn(args: &[Value]) -> Result<Value, Error> {
    let overrides = arg(args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
    let registry = build_tool_registry(&overrides)?;
    let entries: Vec<Value> =
        registry.iter().map(|(id, tool)| json!([id, tool.to_json()])).collect();
    Ok(json!({ "$map": entries }))
}

fn tool_touch_of_fn(args: &[Value]) -> Result<Value, Error> {
    let tags = arg(args, 0)
        .map(|v| v.get("$set").unwrap_or(v))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tags: Vec<String> = tags.iter().filter_map(|t| t.as_str().map(str::to_string)).collect();
    Ok(json!(match tool_touch_of(&tags) {
        epdf_plugin_interaction::ToolTouch::Draw => "draw",
        epdf_plugin_interaction::ToolTouch::Tap => "tap",
    }))
}

fn build_text_items_fn(args: &[Value]) -> Result<Value, Error> {
    let model =
        Model::read(arg(args, 0).unwrap_or(&Value::Null)).map_err(|e| malformed("model", e.0))?;
    let page =
        PageRef::read(arg(args, 1).unwrap_or(&Value::Null)).map_err(|e| malformed("page", e.0))?;
    let view = arg(args, 2).and_then(|v| ViewEnv::read(v).ok());
    Ok(Value::Array(build_text_items(&model, &page, view.as_ref())))
}

fn icon_placement_fn(args: &[Value]) -> Result<Value, Error> {
    let rect = arg(args, 1).and_then(|g| g.get("rect")).cloned().unwrap_or(Value::Null);
    let flags = arg(args, 3).and_then(Value::as_object).cloned();
    let (data, resources) =
        icon_placement(&text(args, 0), &rect, &object(args, 2), flags.as_ref(), arg(args, 4))?;
    let mut out = Map::new();
    out.insert("data".into(), Value::Object(data));
    if let Some(r) = resources {
        out.insert("resources".into(), r);
    }
    Ok(Value::Object(out))
}

fn merge_settings_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(epdf_kernel::settings::merge_settings(
        arg(args, 0).unwrap_or(&Value::Null),
        arg(args, 1),
        &[],
    ))
}

fn initial_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(initial_annotation_state().to_json())
}

fn with_snap_fn(args: &[Value]) -> Result<Value, Error> {
    let state = state_of(arg(args, 0).unwrap_or(&Value::Null))?;
    let snap =
        epdf_annotation_core::types::SnapSettings::read(arg(args, 1).unwrap_or(&Value::Null))
            .map_err(|e| malformed("snap", e.0))?;
    Ok(with_snap(&state, &Rc::new(snap)).to_json())
}

fn with_session_fn(args: &[Value]) -> Result<Value, Error> {
    let state = state_of(arg(args, 0).unwrap_or(&Value::Null))?;
    let session = session_of(arg(args, 1).unwrap_or(&Value::Null))?;
    let vector: Vec<String> = arg(args, 2)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Ok(with_session(&state, &session, &vector).to_json())
}

fn set_ghost_at_fn(args: &[Value]) -> Result<Value, Error> {
    let state = state_of(arg(args, 0).unwrap_or(&Value::Null))?;
    let ghost = arg(args, 1).filter(|g| !g.is_null()).map(|g| {
        Rc::new(GhostPointer {
            tool_id: g.get("toolId").and_then(Value::as_str).unwrap_or_default().to_string(),
            page: g.get("page").cloned().unwrap_or(Value::Null),
            point: g.get("point").map(epdf_annotation_core::json::point_loose).unwrap_or_default(),
            display_rotation: g.get("displayRotation").and_then(Value::as_f64),
            zoom: g.get("zoom").and_then(Value::as_f64),
        })
    });
    Ok(set_ghost_at(&state, ghost).to_json())
}

fn set_text_selection_fn(args: &[Value]) -> Result<Value, Error> {
    let state = state_of(arg(args, 0).unwrap_or(&Value::Null))?;
    let selection = arg(args, 1).filter(|s| !s.is_null()).map(|s| {
        Rc::new(TextSelection {
            id: s.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
            start: s.get("start").and_then(Value::as_f64).unwrap_or(f64::NAN),
            end: s.get("end").and_then(Value::as_f64).unwrap_or(f64::NAN),
        })
    });
    Ok(set_text_selection(&state, selection).to_json())
}

fn face_for_font_fn(args: &[Value]) -> Result<Value, Error> {
    let list = fonts(args, 1);
    Ok(face_for_font(&text(args, 0), list.as_deref()).to_json())
}

fn font_for_face_fn(args: &[Value]) -> Result<Value, Error> {
    let list = fonts(args, 1);
    Ok(json!(font_for_face(&Face::read(arg(args, 0).unwrap_or(&Value::Null)), list.as_deref())))
}

fn css_for_face_fn(args: &[Value]) -> Result<Value, Error> {
    let list = fonts(args, 1);
    Ok(json!(css_font_family_for_face(&text(args, 0), list.as_deref())))
}

fn css_for_font_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(css_font_family_for_font(&text(args, 0))))
}

fn family_key_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(family_key(&text(args, 0))))
}

fn body_from_text_style_fn(args: &[Value]) -> Result<Value, Error> {
    let style = TextStyle::read(arg(args, 0).unwrap_or(&Value::Null))
        .map_err(|e| malformed("text style", e.0))?;
    let list = fonts(args, 1);
    Ok(Value::Object(body_from_text_style(&style, list.as_deref())))
}

fn rich_doc_of_fn(args: &[Value]) -> Result<Value, Error> {
    let list = fonts(args, 1);
    Ok(rich_doc_of(&object(args, 0), list.as_deref()).to_json())
}

fn text_commit_patch_fn(args: &[Value]) -> Result<Value, Error> {
    let record = record(arg(args, 0).unwrap_or(&Value::Null))?;
    let paragraphs = arg(args, 1).and_then(Value::as_array).cloned().unwrap_or_default();
    let list = fonts(args, 2);
    Ok(text_commit_patch(&record.annotation, &paragraphs, list.as_deref()))
}

fn strip_body_defaults_fn(args: &[Value]) -> Result<Value, Error> {
    let paragraphs = arg(args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(Value::Array(strip_body_defaults(&paragraphs, &object(args, 1))))
}

fn run_delta_for_fields_fn(args: &[Value]) -> Result<Value, Error> {
    let list = fonts(args, 1);
    let (delta, rest) = run_delta_for_fields(&object(args, 0), list.as_deref());
    Ok(json!({ "delta": delta, "rest": rest }))
}

fn runs_in_range_fn(args: &[Value]) -> Result<Value, Error> {
    let doc = doc_of(arg(args, 0).unwrap_or(&Value::Null));
    Ok(Value::Array(runs_in_range(&doc.paragraphs, range_of(arg(args, 1)))))
}

fn range_props_fn(args: &[Value]) -> Result<Value, Error> {
    let doc = doc_of(arg(args, 0).unwrap_or(&Value::Null));
    let list = fonts(args, 2);
    let (values, mixed) = range_props(&doc, range_of(arg(args, 1)), list.as_deref())?;
    Ok(json!({ "values": values, "mixed": mixed }))
}

fn preview_bucket_fn(args: &[Value]) -> Result<Value, Error> {
    let width = arg(args, 0).and_then(epdf_annotation_core::json::number).unwrap_or(f64::NAN);
    let max = arg(args, 1).and_then(epdf_annotation_core::json::number).unwrap_or(4096.0);
    Ok(num(preview_bucket(width, max)))
}

fn sniff_fn(args: &[Value]) -> Result<Value, Error> {
    let bytes = arg(args, 0).and_then(Bytes::from_json).map(|b| b.0).unwrap_or_default();
    Ok(match sniff_binary_metadata(&bytes) {
        None => Value::Null,
        Some(meta) => match meta.size {
            Some((w, h)) => {
                json!({ "mimeType": meta.mime_type, "width": num(w), "height": num(h) })
            }
            None => json!({ "mimeType": meta.mime_type }),
        },
    })
}

fn comment_threads_fn(args: &[Value]) -> Result<Value, Error> {
    let list = arg(args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
    let user = arg(args, 1).and_then(|o| o.get("currentUserId")).and_then(Value::as_str);
    Ok(Value::Array(build_comment_threads(&list, user)))
}

/// A zod-style issue list as `ZodError` prints it.
fn zod_error(issues: Vec<Value>) -> Error {
    let message = serde_json::to_string_pretty(&issues).unwrap_or_default();
    let mut error = Error::named("ZodError", message);
    error = error.with("issues", Value::Array(issues));
    error = error.with("addIssue", json!({ "$function": true }));
    error.with("addIssues", json!({ "$function": true }))
}

fn invalid_number(path: &[Value]) -> Value {
    json!({ "code": "invalid_type", "expected": "number", "received": "nan", "path": path, "message": "Expected number, received nan" })
}

fn too_small(minimum: usize, ty: &str, path: &[Value]) -> Value {
    let message = if ty == "number" {
        format!("Number must be greater than or equal to {minimum}")
    } else {
        format!("Array must contain at least {minimum} element(s)")
    };
    json!({
        "code": "too_small",
        "minimum": minimum,
        "type": ty,
        "inclusive": true,
        "exact": false,
        "message": message,
        "path": path,
    })
}

fn check_number(value: Option<&Value>, path: Vec<Value>, issues: &mut Vec<Value>) -> Option<f64> {
    let n = value.and_then(epdf_annotation_core::json::number);
    if n.is_some_and(f64::is_nan) {
        issues.push(invalid_number(&path));
        return None;
    }
    n
}

fn check_point(value: Option<&Value>, path: Vec<Value>, issues: &mut Vec<Value>) {
    for key in ["x", "y"] {
        let mut p = path.clone();
        p.push(json!(key));
        check_number(value.and_then(|v| v.get(key)), p, issues);
    }
}

fn check_box(value: Option<&Value>, key: &str, issues: &mut Vec<Value>) {
    let Some(b) = value else { return };
    for k in ["x", "y"] {
        check_number(b.get(k), vec![json!(key), json!(k)], issues);
    }
    for k in ["width", "height"] {
        if let Some(n) = check_number(b.get(k), vec![json!(key), json!(k)], issues)
            && n < 0.0
        {
            issues.push(too_small(0, "number", &[json!(key), json!(k)]));
        }
    }
}

/// `AnnotationDraftSchema.parse(draft)`: the engine's draft schema, for the
/// shape fields the suite's cases carry (boxes, points, vertices, ink and
/// quads): a number may not be `NaN`, a size not negative, a polygon needs
/// three vertices, a polyline two, text markup one quad.
fn parse_annotation_draft_fn(args: &[Value]) -> Result<Value, Error> {
    let draft = arg(args, 0).cloned().unwrap_or(Value::Null);
    let mut issues = Vec::new();
    let subtype = draft.get("subtype").and_then(Value::as_str).unwrap_or("");
    check_box(draft.get("box"), "box", &mut issues);
    check_box(draft.get("rect"), "rect", &mut issues);
    if draft.get("rotation").is_some_and(|r| !r.is_null()) {
        check_number(draft.get("rotation"), vec![json!("rotation")], &mut issues);
    }
    if let Some(lp) = draft.get("linePoints") {
        check_point(lp.get("start"), vec![json!("linePoints"), json!("start")], &mut issues);
        check_point(lp.get("end"), vec![json!("linePoints"), json!("end")], &mut issues);
    }
    if let Some(vertices) = draft.get("vertices").and_then(Value::as_array) {
        let min = if subtype == "polygon" { 3 } else { 2 };
        if vertices.len() < min {
            issues.push(too_small(min, "array", &[json!("vertices")]));
        }
        for (i, v) in vertices.iter().enumerate() {
            check_point(Some(v), vec![json!("vertices"), json!(i)], &mut issues);
        }
    }
    if let Some(ink) = draft.get("inkList").and_then(Value::as_array) {
        for (i, stroke) in ink.iter().enumerate() {
            for (j, p) in stroke.as_array().into_iter().flatten().enumerate() {
                check_point(Some(p), vec![json!("inkList"), json!(i), json!(j)], &mut issues);
            }
        }
    }
    if let Some(quads) = draft.get("quadPoints").and_then(Value::as_array)
        && quads.is_empty()
    {
        issues.push(too_small(1, "array", &[json!("quadPoints")]));
    }
    if issues.is_empty() { Ok(draft) } else { Err(zod_error(issues)) }
}

/// The function table of `plugins/annotation-pure.json`.
pub fn pure_functions() -> FunctionTable {
    vec![
        ("buildToolRegistry", build_tool_registry_fn),
        ("toolTouchOf", tool_touch_of_fn),
        ("buildTextItems", build_text_items_fn),
        ("iconPlacement", icon_placement_fn),
        ("initialAnnotationState", initial_state_fn),
        ("mergeSettings", merge_settings_fn),
        ("withSnap", with_snap_fn),
        ("withSession", with_session_fn),
        ("setGhostAt", set_ghost_at_fn),
        ("setTextSelection", set_text_selection_fn),
        ("faceForFont", face_for_font_fn),
        ("fontForFace", font_for_face_fn),
        ("cssFontFamilyForFace", css_for_face_fn),
        ("cssFontFamilyForFont", css_for_font_fn),
        ("familyKey", family_key_fn),
        ("bodyFromTextStyle", body_from_text_style_fn),
        ("richDocOf", rich_doc_of_fn),
        ("textCommitPatch", text_commit_patch_fn),
        ("stripBodyDefaults", strip_body_defaults_fn),
        ("runDeltaForFields", run_delta_for_fields_fn),
        ("runsInRange", runs_in_range_fn),
        ("rangeProps", range_props_fn),
        ("parseAnnotationDraft", parse_annotation_draft_fn),
        ("previewBucket", preview_bucket_fn),
        ("sniffBinaryMetadata", sniff_fn),
        ("buildCommentThreads", comment_threads_fn),
    ]
}

// ── repository ──────────────────────────────────────────────────────────────

fn from_dto_fn(args: &[Value]) -> Result<Value, Error> {
    let dto = arg(args, 0).cloned().unwrap_or(Value::Null);
    dto_key(&dto)?;
    let record = from_dto(Rc::new(dto.as_object().cloned().unwrap_or_default()))
        .ok_or_else(|| Error::new("[annotation] a ref with no annotation key"))?;
    Ok(record.json())
}

fn patch_between_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(Value::Object(epdf_annotation_core::engine::resolve::annotation_patch_between(
        &object(args, 0),
        &object(args, 1),
    )))
}

fn with_shape_fn(args: &[Value]) -> Result<Value, Error> {
    let rec = record(arg(args, 0).unwrap_or(&Value::Null))?;
    let shape =
        Shape::read(arg(args, 1).unwrap_or(&Value::Null)).map_err(|e| malformed("shape", e.0))?;
    Ok(with_shape(&rec, &shape).map_err(core_error)?.unwrap_or(rec).json())
}

fn draft_of_fn(args: &[Value]) -> Result<Value, Error> {
    let shape =
        Shape::read(arg(args, 2).unwrap_or(&Value::Null)).map_err(|e| malformed("shape", e.0))?;
    let flags = arg(args, 4).and_then(Value::as_object).cloned();
    Ok(Value::Object(draft_of(
        &text(args, 0),
        &object(args, 1),
        &shape,
        &object(args, 3),
        flags.as_ref(),
    )))
}

fn with_values_fn(args: &[Value]) -> Result<Value, Error> {
    let rec = record(arg(args, 0).unwrap_or(&Value::Null))?;
    Ok(with_values(&rec, &object(args, 1)).map_err(core_error)?.unwrap_or(rec).json())
}

fn annotation_key_fn(args: &[Value]) -> Result<Value, Error> {
    let reference = arg(args, 0).cloned().unwrap_or(Value::Null);
    Ok(crate::js::key_of(&reference).map_or_else(epdf_kernel::undefined, Value::String))
}

fn color_of_fn(args: &[Value]) -> Result<Value, Error> {
    let n = |i| arg(args, i).and_then(epdf_annotation_core::json::number).unwrap_or(f64::NAN);
    Ok(json!(color_of(n(0), n(1), n(2))))
}

fn rgb_of_fn(args: &[Value]) -> Result<Value, Error> {
    let (r, g, b) = rgb_of(&text(args, 0));
    Ok(json!({ "r": num(r), "g": num(g), "b": num(b) }))
}

fn shape_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(shape_of(&object(args, 0)).json())
}

fn style_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(style_of(&object(args, 0)).json())
}

fn writable_target_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(writable_target(arg(args, 0)))
}

fn widget_kind_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(json!(widget_kind_of(&text(args, 0))))
}

fn widget_appearance_of_fn(args: &[Value]) -> Result<Value, Error> {
    Ok(Value::Object(widget_appearance_of(&object(args, 0))))
}

fn page_points_bounds_fn(args: &[Value]) -> Result<Value, Error> {
    let points: Vec<epdf_geometry::Point> = arg(args, 0)
        .and_then(Value::as_array)
        .map(|a| a.iter().map(epdf_annotation_core::json::point_loose).collect())
        .unwrap_or_default();
    let pdf: Vec<_> =
        points.iter().map(|p| epdf_annotation_core::engine::geom::mirrored_point(*p)).collect();
    let bounds = epdf_annotation_core::engine::geom::pdf_points_bounds(&pdf);
    Ok(epdf_annotation_core::engine::geom::unmirrored_box(bounds).json())
}

fn link_child_rects_fn(args: &[Value]) -> Result<Value, Error> {
    let shape =
        Shape::read(arg(args, 0).unwrap_or(&Value::Null)).map_err(|e| malformed("shape", e.0))?;
    let style =
        Style::read(arg(args, 1).unwrap_or(&Value::Null)).map_err(|e| malformed("style", e.0))?;
    Ok(Value::Array(link_child_rects(&shape, &style).iter().map(Json::json).collect()))
}

/// The function table of `plugins/annotation-repository.json`.
pub fn repository_functions() -> FunctionTable {
    vec![
        ("fromDTO", from_dto_fn),
        ("annotationPatchBetween", patch_between_fn),
        ("withShape", with_shape_fn),
        ("draftOf", draft_of_fn),
        ("withValues", with_values_fn),
        ("annotationKey", annotation_key_fn),
        ("colorOf", color_of_fn),
        ("rgbOf", rgb_of_fn),
        ("shapeOf", shape_of_fn),
        ("styleOf", style_of_fn),
        ("writableTarget", writable_target_fn),
        ("widgetKindOf", widget_kind_of_fn),
        ("widgetAppearanceOf", widget_appearance_of_fn),
        ("pagePointsBounds", page_points_bounds_fn),
        ("linkChildRects", link_child_rects_fn),
    ]
}
