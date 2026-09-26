//! `repository/index.ts`: the boundary between the engine's PDF-space
//! annotation DTOs and the core's content-space `Annot`, kind-major. Each
//! family declares one [`projection::Kind`], and every wire statement
//! derives from it:
//!
//! ```text
//! full patch    =  geometry(a)  ∪  props(a, every key the kind declares)
//! create draft  =  full patch   ∪  draftExtras(a)   (+ /F verbatim)
//! scoped patch  =  geometry(a)  |  props(a, the touched keys)
//! ```
//!
//! DTOs stay JSON (`Annot.data` carries one verbatim), so the kind modules
//! read them through [`crate::value`].

pub mod kinds;
pub mod links;
pub mod projection;
pub mod props;
pub mod seam;

use epdf_annotation_core::measurement::MeasurementAppearance;
use epdf_annotation_core::{
    Annot, AnnotationFlags, AnnotationRef, PageRef, PatchScope, PdfRect, PropKey, Source,
    geom_rotation, pdf_to_content_rect, props_for,
};
use serde_json::{Value, json};

pub use links::link_child_rects;
pub use projection::{Kind, Wire, box_emit};
pub use seam::{
    annotation_key, annotation_key_of, box_geom_fields, color_to_css, css_to_color, style_from_dto,
    widget_appearance_from_props, writable_target,
};

use crate::value::{get, opt_pdf_rect, pdf_rect_of, str_of, truthy};

fn wire_subtype_of(a: &Annot) -> String {
    if a.subtype.starts_with("widget") { "widget".into() } else { a.subtype.clone() }
}

/// `fromDTO(dto, crop, source)`: engine DTO to content-space `Annot`.
/// `Baked` shows the engine's appearance raster, `Vector` renders live.
pub fn from_dto(dto: &Value, crop: PdfRect, source: Source) -> Annot {
    let subtype = str_of(dto, "subtype").unwrap_or_default();
    let slice = Kind::of(subtype).ingest(dto, crop);
    let is_callout = subtype == "free-text" && str_of(dto, "intent") == Some("free-text-callout");
    let stripped = if !is_callout
        && dto.get("unrotatedRect").is_some()
        && dto.get("rotation").is_some()
        && truthy(get(dto, "rotation"))
    {
        opt_pdf_rect(get(dto, "unrotatedRect"))
    } else {
        None
    };
    let ref_: Option<AnnotationRef> =
        get(dto, "ref").and_then(|r| serde_json::from_value(r.clone()).ok());
    let irt = get(dto, "inReplyTo").filter(|v| truthy(Some(v))).and_then(seam::annotation_key_of);
    let group = if str_of(dto, "replyType") == Some("group") { irt.clone() } else { None };
    let flags: AnnotationFlags =
        get(dto, "flags").and_then(|f| serde_json::from_value(f.clone()).ok()).unwrap_or_default();
    let ap_box =
        pdf_to_content_rect(stripped.unwrap_or_else(|| pdf_rect_of(get(dto, "rect"))), crop);
    Annot {
        id: ref_.as_ref().map(annotation_key).unwrap_or_default(),
        ref_,
        page: get(dto, "page")
            .and_then(|p| serde_json::from_value::<PageRef>(p.clone()).ok())
            .unwrap_or_default(),
        subtype: if subtype == "widget" {
            kinds::misc::widget_kind_of(str_of(dto, "fieldFamily").unwrap_or_default())
        } else {
            subtype.to_string()
        },
        style: style_from_dto(dto),
        text: slice.text,
        label: slice.label,
        icon: slice.icon,
        flags,
        source,
        ap_box: Some(ap_box),
        ap_rot: stripped.map(|_| geom_rotation(&slice.geom)),
        ap_version: None,
        authority: None,
        data: Some(dto.clone()),
        measure: slice.measure,
        intent: slice.intent,
        link: slice.link,
        irt,
        group,
        geom: slice.geom,
    }
}

/// `emitProps(a, crop, keys)`: lower `keys` through the kind's overrides and
/// the generic table; `None` when some key has no lowering.
fn emit_props<S: AsRef<str>>(a: &Annot, crop: PdfRect, keys: &[S]) -> Option<Wire> {
    let kind = Kind::of(&a.subtype);
    let mut out = Wire::new();
    for key in keys {
        let key = PropKey::parse(key.as_ref())?;
        let lowered = match kind.prop(key, a, crop) {
            Some(w) => w,
            None if props::has_generic(key) => props::generic(key, a),
            None => return None,
        };
        out.extend(lowered);
    }
    Some(out)
}

fn editable_keys(subtype: &str) -> Vec<&'static str> {
    props_for(subtype).iter().map(|s| s.key.as_str()).collect()
}

fn with_subtype(a: &Annot, rest: Wire) -> Wire {
    let mut out = Wire::new();
    out.insert("subtype".into(), json!(wire_subtype_of(a)));
    out.extend(rest);
    out
}

/// `toPatch(a, crop)`: the full engine patch, geometry plus every editable
/// prop. `None` when there is nothing to state.
pub fn to_patch(a: &Annot, crop: PdfRect) -> Option<Wire> {
    let kind = Kind::of(&a.subtype);
    let geo = kind.geometry(a, crop);
    let props = emit_props(a, crop, &editable_keys(&a.subtype)).unwrap_or_default();
    if geo.is_none() && props.is_empty() {
        return None;
    }
    let mut rest = geo.unwrap_or_default();
    rest.extend(props);
    Some(with_subtype(a, rest))
}

/// `toScopedPatch(a, scope, crop)`: the sparse patch for exactly that
/// intent; kinds without editable geometry and unlowerable keys degrade to
/// the full patch.
pub fn to_scoped_patch(a: &Annot, scope: &PatchScope, crop: PdfRect) -> Option<Wire> {
    let kind = Kind::of(&a.subtype);
    match scope {
        PatchScope::Caption => {
            let caption = match a.measure.as_ref()? {
                MeasurementAppearance::Distance(d) => serde_json::to_value(d.caption),
                MeasurementAppearance::Shape(s) => serde_json::to_value(s.caption),
            };
            let mut rest = Wire::new();
            rest.insert("caption".into(), caption.unwrap_or_default());
            Some(with_subtype(a, rest))
        }
        PatchScope::Leader => {
            let Some(MeasurementAppearance::Distance(d)) = &a.measure else { return None };
            let mut out = Wire::new();
            out.insert("subtype".into(), json!("line"));
            if let Some(leader) = d.leader {
                out.insert("leader".into(), serde_json::to_value(leader).unwrap_or_default());
            }
            Some(out)
        }
        PatchScope::Geometry => match kind.geometry(a, crop) {
            Some(geo) => Some(with_subtype(a, geo)),
            None => to_patch(a, crop),
        },
        PatchScope::Props { keys } => match emit_props(a, crop, keys) {
            None => to_patch(a, crop),
            Some(props) if props.is_empty() => None,
            Some(props) => Some(with_subtype(a, props)),
        },
    }
}

/// `toCreateDraft(a, crop)`: the full statement plus the kind's create-only
/// extras, a fresh `/NM` from `uuid` (the TypeScript's
/// `crypto.randomUUID()`), and the model's `/F` verbatim. `None` for the
/// kinds whose creates travel their own path.
pub fn to_create_draft(a: &Annot, crop: PdfRect, uuid: &dyn Fn() -> String) -> Option<Wire> {
    let kind = Kind::of(&a.subtype);
    if !kind.createable() {
        return None;
    }
    let mut base = to_patch(a, crop)?;
    let extras = kind.draft_extras(a, crop);
    if let Some(extras) = extras {
        base.extend(extras?);
    }
    base.insert("nm".into(), json!(uuid()));
    base.insert("flags".into(), serde_json::to_value(a.flags).unwrap_or_default());
    Some(base)
}
