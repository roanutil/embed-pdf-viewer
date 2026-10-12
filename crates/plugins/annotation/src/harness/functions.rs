//! The function tables for `plugins/annotation-repository.json` and
//! `plugins/annotation-pure.json`, by the generator's names.

use std::rc::Rc;

use epdf_annotation_core::{Annot, PatchScope, PdfRect, Rect, Source};
use epdf_kernel::Error;
use plugin_conformance::FunctionTable;
use plugin_conformance::js::{arg, from_value, to_value};
use serde_json::{Value, json};

use crate::contract::*;
use crate::model::{
    AnnotationAction, AnnotationState, annotation_reducer, initial_annotation_state, merge_chrome,
};
use crate::repository::kinds::{free_text, misc, quads, shape};
use crate::repository::seam::{self, pdf_json};
use crate::repository::{self, projection::Wire};
use crate::rich_text::{self, Face, FontHandle, RichTextDocument};
use crate::tools::definitions::{AnnotationToolDef, build_tool_registry, is_touch_direct};
use crate::value::{opt_f64, opt_pdf_rect, pdf_rect_of};

type Out = Result<Value, Error>;

fn wire(w: Option<Wire>) -> Value {
    w.map_or(Value::Null, Value::Object)
}

fn annot(a: &[Value], i: usize) -> Result<Annot, Error> {
    arg(a, i)
}

fn crop(a: &[Value], i: usize) -> PdfRect {
    pdf_rect_of(a.get(i))
}

fn source(a: &[Value], i: usize) -> Source {
    match a.get(i).and_then(Value::as_str) {
        Some("vector") => Source::Vector,
        _ => Source::Baked,
    }
}

fn from_dto(a: &[Value]) -> Out {
    Ok(to_value(&repository::from_dto(&a[0], crop(a, 1), source(a, 2))))
}

fn to_patch(a: &[Value]) -> Out {
    Ok(wire(repository::to_patch(&annot(a, 0)?, crop(a, 1))))
}

fn to_scoped_patch(a: &[Value]) -> Out {
    let scope: PatchScope = arg(a, 1)?;
    Ok(wire(repository::to_scoped_patch(&annot(a, 0)?, &scope, crop(a, 2))))
}

fn to_create_draft(a: &[Value]) -> Out {
    let seq = std::cell::Cell::new(0u64);
    let uuid = || {
        seq.set(seq.get() + 1);
        crate::harness::uuid_at(seq.get())
    };
    Ok(wire(repository::to_create_draft(&annot(a, 0)?, crop(a, 1), &uuid)))
}

fn annotation_key(a: &[Value]) -> Out {
    Ok(json!(seam::annotation_key(&arg(a, 0)?)))
}

fn color_to_css(a: &[Value]) -> Out {
    Ok(json!(seam::color_to_css(&a[0])))
}

fn css_to_color(a: &[Value]) -> Out {
    Ok(seam::css_to_color(a[0].as_str().unwrap_or_default()))
}

fn box_geom_fields(a: &[Value]) -> Out {
    let rect: Rect = arg(a, 0)?;
    let rot: f64 = arg(a, 1)?;
    Ok(Value::Object(seam::box_geom_fields(rect, rot, crop(a, 2))))
}

fn box_geom_from_dto(a: &[Value]) -> Out {
    let dto = &a[0];
    let rotation = a.get(1).and_then(Value::as_f64);
    let unrotated = opt_pdf_rect(a.get(2));
    let ellipse = a.get(4).and_then(Value::as_bool).unwrap_or(false);
    Ok(to_value(&seam::box_geom_from_dto(
        pdf_rect_of(dto.get("rect")),
        rotation,
        unrotated,
        crop(a, 3),
        ellipse,
    )))
}

fn inset(a: &[Value]) -> Out {
    Ok(pdf_json(seam::inset_pdf_rect_by_rd(pdf_rect_of(a.first()), a.get(1))))
}

fn rot_from_dto(a: &[Value]) -> Out {
    let mut out = serde_json::Map::new();
    if let Some(rot) = seam::rot_from_dto(a.first().and_then(Value::as_f64)) {
        out.insert("rot".into(), json!(rot));
    }
    Ok(Value::Object(out))
}

fn border_from_dto(a: &[Value]) -> Out {
    Ok(to_value(&seam::border_from_dto(&a[0])))
}

fn writable_target(a: &[Value]) -> Out {
    let target: Option<epdf_annotation_core::PdfLinkTarget> =
        a.first().filter(|v| !epdf_kernel::is_undefined(v)).and_then(|v| from_value(v).ok());
    Ok(seam::writable_target(target.as_ref()).map_or(Value::Null, |t| to_value(&t)))
}

fn widget_kind_of(a: &[Value]) -> Out {
    Ok(json!(misc::widget_kind_of(a[0].as_str().unwrap_or_default())))
}

fn widget_appearance(a: &[Value]) -> Out {
    Ok(seam::widget_appearance_from_props(&arg(a, 0)?))
}

fn cloudy_extras(a: &[Value]) -> Out {
    Ok(Value::Object(shape::cloudy_extras(&annot(a, 0)?)))
}

fn callout_fields(a: &[Value]) -> Out {
    Ok(wire(free_text::callout_fields(&annot(a, 0)?, crop(a, 1))))
}

fn quad_points_for(a: &[Value]) -> Out {
    Ok(quads::quad_points_for(&annot(a, 0)?, crop(a, 1))
        .map_or(Value::Null, |q| quads::quad_points_json(&q)))
}

fn pdf_bounds_of_quads(a: &[Value]) -> Out {
    let list: Vec<[(f64, f64); 4]> = a[0]
        .as_array()
        .into_iter()
        .flatten()
        .map(|q| {
            let p = |k: &str| {
                let v = q.get(k).unwrap_or(&Value::Null);
                (opt_f64(v, "x").unwrap_or(f64::NAN), opt_f64(v, "y").unwrap_or(f64::NAN))
            };
            [p("p1"), p("p2"), p("p3"), p("p4")]
        })
        .collect();
    let b = quads::pdf_bounds_of_quads(&list);
    Ok(json!({
        "left": plugin_conformance::js::number(b.left),
        "bottom": plugin_conformance::js::number(b.bottom),
        "right": plugin_conformance::js::number(b.right),
        "top": plugin_conformance::js::number(b.top),
    }))
}

fn link_child_rects(a: &[Value]) -> Out {
    Ok(to_value(&repository::link_child_rects(&annot(a, 0)?)))
}

fn style_from_dto(a: &[Value]) -> Out {
    Ok(to_value(&seam::style_from_dto(&a[0])))
}

/// `plugins/annotation-repository.json`.
pub fn repository_functions() -> FunctionTable {
    vec![
        ("fromDTO", from_dto),
        ("toPatch", to_patch),
        ("toScopedPatch", to_scoped_patch),
        ("toCreateDraft", to_create_draft),
        ("annotationKey", annotation_key),
        ("colorToCss", color_to_css),
        ("cssToColor", css_to_color),
        ("boxGeomFields", box_geom_fields),
        ("boxGeomFromDTO", box_geom_from_dto),
        ("insetPdfRectByRD", inset),
        ("rotFromDTO", rot_from_dto),
        ("borderFromDTO", border_from_dto),
        ("writableTarget", writable_target),
        ("widgetKindOf", widget_kind_of),
        ("widgetAppearanceFromProps", widget_appearance),
        ("cloudyExtras", cloudy_extras),
        ("calloutFields", callout_fields),
        ("quadPointsFor", quad_points_for),
        ("pdfBoundsOfQuads", pdf_bounds_of_quads),
        ("linkChildRects", link_child_rects),
        ("styleFromDTO", style_from_dto),
    ]
}

// ── plugins/annotation-pure.json ─────────────────────────────────────────────

fn tool_registry(a: &[Value]) -> Out {
    let overrides: Vec<AnnotationToolDef> = match a.first() {
        Some(v) if !epdf_kernel::is_undefined(v) => arg(a, 0)?,
        _ => Vec::new(),
    };
    let registry = build_tool_registry(&overrides)?;
    let entries: Vec<Value> = registry.values().map(|t| json!([t.id, t.to_json()])).collect();
    Ok(json!({ "$map": entries }))
}

fn touch_direct(a: &[Value]) -> Out {
    let tags: Vec<String> = arg(a, 0)?;
    Ok(json!(is_touch_direct(&tags)))
}

fn text_items(a: &[Value]) -> Out {
    let m: epdf_annotation_core::Model = arg(a, 0)?;
    let page: epdf_annotation_core::PageRef = arg(a, 1)?;
    let view: Option<epdf_annotation_core::ViewEnv> = match a.get(2) {
        Some(v) if !epdf_kernel::is_undefined(v) => Some(from_value(v)?),
        _ => None,
    };
    Ok(to_value(&crate::text_item::build_text_items(&m, page, view)))
}

fn icon_draft(a: &[Value]) -> Out {
    let subtype = a[0].as_str().unwrap_or_default();
    let geom = a[1].as_object().cloned().unwrap_or_default();
    let defaults: epdf_annotation_core::AnnotationProps = arg(a, 2)?;
    let flags: Option<epdf_annotation_core::AnnotationFlagsPatch> = match a.get(3) {
        Some(v) if !epdf_kernel::is_undefined(v) => Some(from_value(v)?),
        _ => None,
    };
    let file = a.get(4).filter(|v| !v.is_null()).cloned();
    Ok(Value::Object(crate::write::placement::icon_placement_draft(
        subtype,
        geom,
        &defaults,
        flags.as_ref(),
        file,
    )?))
}

fn config_of(v: Option<&Value>) -> Result<AnnotationConfig, Error> {
    match v {
        Some(v) if !epdf_kernel::is_undefined(v) && !v.is_null() => from_value(v),
        _ => Ok(AnnotationConfig::default()),
    }
}

fn initial_state(a: &[Value]) -> Out {
    Ok(to_value(&initial_annotation_state(&config_of(a.first())?)))
}

fn merge_chrome_fn(a: &[Value]) -> Out {
    let base: ChromeSettings = arg(a, 0)?;
    let patch: ChromeSettingsPatch = arg(a, 1)?;
    Ok(to_value(&merge_chrome(&base, &patch)))
}

/// A `ToolGhost` from its JSON.
pub fn ghost_from_json(v: &Value) -> Result<Option<Rc<ToolGhost>>, Error> {
    if v.is_null() || epdf_kernel::is_undefined(v) {
        return Ok(None);
    }
    let kind = match v.get("kind").and_then(Value::as_str) {
        Some("vector") => ToolGhostKind::Vector {
            tool_id: v.get("toolId").and_then(Value::as_str).unwrap_or_default().into(),
            geom: from_value(v.get("geom").unwrap_or(&Value::Null))?,
        },
        _ => ToolGhostKind::Image,
    };
    Ok(Some(Rc::new(ToolGhost {
        page: from_value(v.get("page").unwrap_or(&Value::Null))?,
        box_: from_value(v.get("box").unwrap_or(&Value::Null))?,
        rot: v.get("rot").and_then(Value::as_f64).unwrap_or(0.0),
        kind,
    })))
}

fn hydration_from_json(v: &Value) -> AnnotationHydration {
    match v.get("status").and_then(Value::as_str) {
        Some("complete") => AnnotationHydration::Complete,
        Some("forbidden") => AnnotationHydration::Forbidden,
        Some("error") => {
            AnnotationHydration::Error(Error::from_spec(v.get("error").unwrap_or(&Value::Null)))
        }
        _ => AnnotationHydration::Loading,
    }
}

fn state_from_json(v: &Value) -> Result<AnnotationState, Error> {
    Ok(AnnotationState {
        model: Rc::new(from_value(v.get("model").unwrap_or(&Value::Null))?),
        hydration: hydration_from_json(v.get("hydration").unwrap_or(&Value::Null)),
        chrome: Rc::new(from_value(v.get("chrome").unwrap_or(&Value::Null))?),
        tool_ghost: ghost_from_json(v.get("toolGhost").unwrap_or(&Value::Null))?,
        stamp_arm_epoch: v.get("stampArmEpoch").and_then(Value::as_f64).unwrap_or(0.0),
        text_selection: match v.get("textSelection") {
            Some(t) if !t.is_null() && !epdf_kernel::is_undefined(t) => {
                Some(Rc::new(from_value(t)?))
            }
            _ => None,
        },
    })
}

/// `None` for an action outside the union, which the reducer's `default`
/// arm returns the state for.
fn action_from_json(v: &Value) -> Result<Option<AnnotationAction>, Error> {
    let member = |k: &str| v.get(k).unwrap_or(&Value::Null);
    Ok(Some(match v.get("type").and_then(Value::as_str) {
        Some("SET_MODEL") => {
            AnnotationAction::SetModel { model: Rc::new(from_value(member("model"))?) }
        }
        Some("SET_HYDRATION") => {
            AnnotationAction::SetHydration { hydration: hydration_from_json(member("hydration")) }
        }
        Some("SET_CHROME") => AnnotationAction::SetChrome { patch: from_value(member("patch"))? },
        Some("SET_TOOL_GHOST") => {
            AnnotationAction::SetToolGhost { ghost: ghost_from_json(member("ghost"))? }
        }
        Some("SET_TEXT_SELECTION") => AnnotationAction::SetTextSelection {
            selection: match member("selection") {
                Value::Null => None,
                t => Some(Rc::new(from_value(t)?)),
            },
        },
        Some("STAMP_ARM_CHANGED") => AnnotationAction::StampArmChanged,
        _ => return Ok(None),
    }))
}

fn reducer(a: &[Value]) -> Out {
    let state = state_from_json(&a[0])?;
    let next = match action_from_json(&a[1])? {
        Some(action) => annotation_reducer(&state, &action).unwrap_or(state),
        None => state,
    };
    Ok(to_value(&next))
}

fn comment_threads(a: &[Value]) -> Out {
    let dtos: Vec<Value> = a[0].as_array().cloned().unwrap_or_default();
    let user = a.get(1).and_then(|o| o.get("currentUserId")).and_then(Value::as_str);
    let threads = crate::comments::build::build_comment_threads(&dtos, user);
    Ok(Value::Array(threads.iter().map(|t| t.to_json()).collect()))
}

fn sniff(a: &[Value]) -> Out {
    let bytes = epdf_kernel::bytes::Bytes::from_json(&a[0]).map(|b| b.0).unwrap_or_default();
    Ok(match crate::binary::sniff_binary_metadata(&bytes) {
        None => Value::Null,
        Some(meta) => match meta.size {
            Some((width, height)) => {
                json!({ "mimeType": meta.mime_type, "width": width, "height": height })
            }
            None => json!({ "mimeType": meta.mime_type }),
        },
    })
}

// ── rich text ────────────────────────────────────────────────────────────────

fn fonts_arg(a: &[Value], i: usize) -> Result<Option<Vec<FontHandle>>, Error> {
    match a.get(i) {
        Some(v) if !epdf_kernel::is_undefined(v) && !v.is_null() => Ok(Some(from_value(v)?)),
        _ => Ok(None),
    }
}

fn text(a: &[Value], i: usize) -> String {
    a.get(i).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn face_for_font(a: &[Value]) -> Out {
    let fonts = fonts_arg(a, 1)?;
    Ok(to_value(&rich_text::face_for_font(&text(a, 0), fonts.as_deref())))
}

fn font_for_face(a: &[Value]) -> Out {
    let face: Face = arg(a, 0)?;
    let fonts = fonts_arg(a, 1)?;
    Ok(json!(rich_text::font_for_face(&face, fonts.as_deref())))
}

fn css_for_face(a: &[Value]) -> Out {
    let fonts = fonts_arg(a, 1)?;
    Ok(json!(rich_text::css_font_family_for_face(&text(a, 0), fonts.as_deref())))
}

fn css_for_font(a: &[Value]) -> Out {
    Ok(json!(rich_text::css_font_family_for_font(&text(a, 0))))
}

fn family_key(a: &[Value]) -> Out {
    Ok(json!(rich_text::family_key(&text(a, 0))))
}

fn body_from_text_style(a: &[Value]) -> Out {
    let t: epdf_annotation_core::TextStyle = arg(a, 0)?;
    let fonts = fonts_arg(a, 1)?;
    Ok(Value::Object(rich_text::body_from_text_style(&t, fonts.as_deref())))
}

fn rich_doc_of(a: &[Value]) -> Out {
    Ok(to_value(&rich_text::rich_doc_of(&annot(a, 0)?, None)))
}

fn text_commit_patch(a: &[Value]) -> Out {
    let paragraphs: Vec<epdf_annotation_core::richtext::RichTextParagraph> = arg(a, 1)?;
    Ok(rich_text::text_commit_patch(&annot(a, 0)?, &paragraphs, None))
}

fn strip_body_defaults(a: &[Value]) -> Out {
    let paragraphs: Vec<epdf_annotation_core::richtext::RichTextParagraph> = arg(a, 0)?;
    let body = a.get(1).and_then(Value::as_object).cloned().unwrap_or_default();
    Ok(to_value(&rich_text::strip_body_defaults(&paragraphs, &body)))
}

fn run_delta_for_props(a: &[Value]) -> Out {
    let patch: epdf_annotation_core::AnnotationPropsPatch = arg(a, 0)?;
    let fonts = fonts_arg(a, 1)?;
    let (delta, rest) = rich_text::run_delta_for_props(&patch, fonts.as_deref());
    Ok(json!({ "delta": to_value(&delta), "rest": to_value(&rest) }))
}

fn runs_in_range(a: &[Value]) -> Out {
    let paragraphs: Vec<epdf_annotation_core::richtext::RichTextParagraph> =
        from_value(a[0].get("paragraphs").unwrap_or(&Value::Null))?;
    let range: epdf_annotation_core::richtext::RichTextRange = arg(a, 1)?;
    Ok(to_value(&rich_text::runs_in_range(&paragraphs, range)))
}

fn range_props(a: &[Value]) -> Out {
    let doc: RichTextDocument = arg(a, 0)?;
    let range: epdf_annotation_core::richtext::RichTextRange = arg(a, 1)?;
    let fonts = fonts_arg(a, 2)?;
    let (values, mixed) = rich_text::range_props(&doc, range, fonts.as_deref())?;
    Ok(json!({ "values": values, "mixed": to_value(&mixed) }))
}

fn geometry_from_input(a: &[Value]) -> Out {
    let input: crate::create_input::CreateAnnotationInput = arg(a, 0)?;
    let (subtype, geom) = crate::create_input::geometry_from_input(&input)?;
    Ok(json!({ "subtype": subtype, "geom": to_value(&geom) }))
}

fn preview_bucket(a: &[Value]) -> Out {
    let px = a.first().and_then(Value::as_f64).unwrap_or(f64::NAN);
    let cap = a.get(1).and_then(Value::as_f64);
    Ok(plugin_conformance::js::number(crate::host_contract::preview_bucket(px, cap)))
}

/// `plugins/annotation-pure.json`.
pub fn pure_functions() -> FunctionTable {
    vec![
        ("buildToolRegistry", tool_registry),
        ("isTouchDirect", touch_direct),
        ("buildTextItems", text_items),
        ("iconPlacementDraft", icon_draft),
        ("initialAnnotationState", initial_state),
        ("mergeChrome", merge_chrome_fn),
        ("annotationReducer", reducer),
        ("buildCommentThreads", comment_threads),
        ("sniffBinaryMetadata", sniff),
        ("faceForFont", face_for_font),
        ("fontForFace", font_for_face),
        ("cssFontFamilyForFace", css_for_face),
        ("cssFontFamilyForFont", css_for_font),
        ("familyKey", family_key),
        ("bodyFromTextStyle", body_from_text_style),
        ("richDocOf", rich_doc_of),
        ("textCommitPatch", text_commit_patch),
        ("stripBodyDefaults", strip_body_defaults),
        ("runDeltaForProps", run_delta_for_props),
        ("runsInRange", runs_in_range),
        ("rangeProps", range_props),
        ("geometryFromInput", geometry_from_input),
        ("previewBucket", preview_bucket),
    ]
}
