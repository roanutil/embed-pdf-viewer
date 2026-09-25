//! `repository/props.ts`: the generic one-to-one lowerings from the flat
//! props vocabulary to wire fields. Kinds override only their exceptions.

use epdf_annotation_core::{Annot, Border, PropKey, Style, TextStyle, initial_text_style};
use serde_json::{Value, json};

use super::projection::Wire;
use super::seam::{css_to_color, num_json};

/// `borderSlice(style)`: a cloudy border keeps a solid underlying stroke.
pub fn border_slice(style: &Style) -> Wire {
    let mut out = Wire::new();
    match &style.border {
        Border::Dashed { dash } => {
            out.insert("borderStyle".into(), json!("dashed"));
            out.insert("dashArray".into(), Value::Array(dash.iter().map(|d| num_json(*d)).collect()));
        }
        _ => {
            out.insert("borderStyle".into(), json!("solid"));
        }
    }
    out
}

fn text_of(a: &Annot) -> TextStyle {
    a.text.clone().unwrap_or_else(initial_text_style)
}

fn one(key: &str, value: Value) -> Wire {
    let mut out = Wire::new();
    out.insert(key.into(), value);
    out
}

/// `GENERIC_PROPS[key]`, applied. Every key has a generic lowering; `link`
/// lowers to nothing (a non-link kind's link is attached children).
pub fn generic(key: PropKey, a: &Annot) -> Wire {
    match key {
        PropKey::Color => one("color", css_to_color(&a.style.color)),
        PropKey::Opacity => one("opacity", num_json(a.style.opacity)),
        PropKey::BlendMode => {
            one("blendMode", serde_json::to_value(a.style.blend_mode).unwrap_or_default())
        }
        PropKey::InteriorColor => one(
            "interiorColor",
            match a.style.interior_color.as_deref() {
                Some(c) if !c.is_empty() => css_to_color(c),
                _ => Value::Null,
            },
        ),
        PropKey::StrokeWidth => one("strokeWidth", num_json(a.style.stroke_width)),
        PropKey::Border => border_slice(&a.style),
        PropKey::FontFamily => one("fontFamily", json!(text_of(a).font_family)),
        PropKey::FontSize => one("fontSize", num_json(text_of(a).font_size)),
        PropKey::FontColor => one("fontColor", css_to_color(&text_of(a).font_color)),
        PropKey::TextAlign => {
            one("textAlign", serde_json::to_value(text_of(a).text_align).unwrap_or_default())
        }
        PropKey::Icon => match &a.icon {
            Some(icon) => one("icon", json!(icon)),
            None => Wire::new(),
        },
        PropKey::Link => Wire::new(),
        // `lineEndings` has no generic lowering; only the stroke kinds
        // declare one. The caller never asks for it here.
        PropKey::LineEndings => Wire::new(),
    }
}

/// Whether `GENERIC_PROPS` has the key.
pub fn has_generic(key: PropKey) -> bool {
    key != PropKey::LineEndings
}
