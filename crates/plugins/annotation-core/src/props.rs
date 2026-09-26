//! `props.ts`: read and apply the flat property vocabulary, routing each key
//! to where it is stored (`style`, `geom.ends`, `text`, `icon`, `link`).

use serde::Serialize;
use serde_json::Value;

use crate::flags::annot_transformable;
use crate::js::Nullable;
use crate::kinds::{PropSpec, props_for};
use crate::types::{
    Annot, AnnotationProps, AnnotationPropsPatch, Border, Geom, LineEnding, LineEndings,
    PdfLinkTarget, PropKey, Style, TextAlign, TextStyle,
};

const NO_ENDINGS: LineEndings = LineEndings { start: LineEnding::None, end: LineEnding::None };

/// Base text styling for kinds and tools with no explicit defaults.
pub fn initial_text_style() -> TextStyle {
    TextStyle {
        font_family: "helvetica".into(),
        font_size: 14.0,
        font_color: "#000000".into(),
        text_align: TextAlign::Left,
        bold: None,
        italic: None,
        underline: None,
    }
}

/// The `Style` slice of a resolved props bag.
pub fn style_from_props(p: &AnnotationProps) -> Style {
    Style {
        color: p.color.clone(),
        interior_color: p.interior_color.clone(),
        stroke_width: p.stroke_width,
        opacity: p.opacity,
        blend_mode: p.blend_mode,
        border: p.border.clone(),
    }
}

/// The `TextStyle` slice of a resolved props bag.
pub fn text_style_from_props(p: &AnnotationProps) -> TextStyle {
    TextStyle {
        font_family: p.font_family.clone(),
        font_size: p.font_size,
        font_color: p.font_color.clone(),
        text_align: p.text_align,
        bold: p.bold,
        italic: p.italic,
        underline: p.underline,
    }
}

/// Does this kind's prop table declare `link`?
pub fn kind_takes_link(subtype: &str) -> bool {
    props_for(subtype).iter().any(|s| s.key == PropKey::Link)
}

/// A geom that carries `/LE` endings: a line, or an open poly.
fn endings_geom(g: &Geom) -> bool {
    matches!(g, Geom::Line { .. } | Geom::Poly { closed: false, .. })
}

fn geom_ends(g: &Geom) -> Option<LineEndings> {
    match g {
        Geom::Line { ends, .. } | Geom::Poly { ends, .. } => *ends,
        _ => None,
    }
}

/// The value of one property, where the annotation's kind carries it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PropValue {
    Text(String),
    Number(f64),
    Null,
    Blend(crate::types::BlendMode),
    Border(Border),
    Endings(LineEndings),
    Align(TextAlign),
    Link(PdfLinkTarget),
    Bool(bool),
}

/// Read one property off an annotation, or `None` (`undefined`) when its
/// kind doesn't carry it. `interiorColor` and `link` may read `Null`.
pub fn read_prop(a: &Annot, key: PropKey) -> Option<PropValue> {
    match key {
        PropKey::Color => Some(PropValue::Text(a.style.color.clone())),
        PropKey::InteriorColor => {
            Some(a.style.interior_color.clone().map_or(PropValue::Null, PropValue::Text))
        }
        PropKey::StrokeWidth => Some(PropValue::Number(a.style.stroke_width)),
        PropKey::Opacity => Some(PropValue::Number(a.style.opacity)),
        PropKey::BlendMode => Some(PropValue::Blend(a.style.blend_mode)),
        PropKey::Border => Some(PropValue::Border(a.style.border.clone())),
        PropKey::LineEndings => endings_geom(&a.geom)
            .then(|| PropValue::Endings(geom_ends(&a.geom).unwrap_or(NO_ENDINGS))),
        PropKey::FontFamily => a.text.as_ref().map(|t| PropValue::Text(t.font_family.clone())),
        PropKey::FontSize => a.text.as_ref().map(|t| PropValue::Number(t.font_size)),
        PropKey::FontColor => a.text.as_ref().map(|t| PropValue::Text(t.font_color.clone())),
        PropKey::TextAlign => a.text.as_ref().map(|t| PropValue::Align(t.text_align)),
        PropKey::Bold => a.text.as_ref().map(|t| PropValue::Bool(t.bold.unwrap_or(false))),
        PropKey::Italic => a.text.as_ref().map(|t| PropValue::Bool(t.italic.unwrap_or(false))),
        PropKey::Underline => {
            a.text.as_ref().map(|t| PropValue::Bool(t.underline.unwrap_or(false)))
        }
        PropKey::Icon => a.icon.clone().map(PropValue::Text),
        PropKey::Link => match &a.link {
            Nullable::Undefined => None,
            Nullable::Null => Some(PropValue::Null),
            Nullable::Value(t) => Some(PropValue::Link(t.clone())),
        },
    }
}

/// Which parts of an annotation [`apply_props_tracked`] replaced: the
/// TypeScript compares these by reference, so a write of an equal value
/// still counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PropsChanged {
    pub style: bool,
    pub geom: bool,
    pub text: bool,
    /// `icon` compares by value.
    pub icon: bool,
    /// The link kind stored a new `link` (`next.link !== a.link`).
    pub link: bool,
}

impl PropsChanged {
    pub fn any(&self) -> bool {
        self.style || self.geom || self.text || self.icon || self.link
    }
}

/// [`apply_props`], also saying what it replaced.
pub fn apply_props_tracked(
    a: &Annot,
    patch: &AnnotationPropsPatch,
) -> Option<(Annot, PropsChanged)> {
    if !annot_transformable(a) {
        return None;
    }
    let takes = |k: PropKey| props_for(&a.subtype).iter().any(|s| s.key == k);
    let mut next = a.clone();
    let mut changed = PropsChanged::default();

    let mut style = a.style.clone();
    let mut style_changed = false;
    if let Some(c) = &patch.color
        && takes(PropKey::Color)
    {
        style.color.clone_from(c);
        style_changed = true;
    }
    if !patch.interior_color.is_undefined() && takes(PropKey::InteriorColor) {
        style.interior_color = patch.interior_color.as_option().cloned();
        style_changed = true;
    }
    if let Some(w) = patch.stroke_width
        && takes(PropKey::StrokeWidth)
    {
        style.stroke_width = w;
        style_changed = true;
    }
    if let Some(o) = patch.opacity
        && takes(PropKey::Opacity)
    {
        style.opacity = o;
        style_changed = true;
    }
    if let Some(b) = patch.blend_mode
        && takes(PropKey::BlendMode)
    {
        style.blend_mode = b;
        style_changed = true;
    }
    if let Some(b) = &patch.border
        && takes(PropKey::Border)
    {
        style.border = b.clone();
        style_changed = true;
    }
    if style_changed {
        next.style = style;
        changed.style = true;
    }

    if let Some(le) = &patch.line_endings
        && takes(PropKey::LineEndings)
        && endings_geom(&next.geom)
    {
        let merged = le.over(geom_ends(&next.geom).unwrap_or(NO_ENDINGS));
        if let Geom::Line { ends, .. } | Geom::Poly { ends, .. } = &mut next.geom {
            *ends = Some(merged);
        }
        changed.geom = true;
    }

    if let Some(icon) = &patch.icon
        && takes(PropKey::Icon)
    {
        changed.icon = next.icon.as_deref() != Some(icon.as_str());
        next.icon = Some(icon.clone());
    }

    if !patch.link.is_undefined() && takes(PropKey::Link) && a.subtype == "link" {
        // A fresh target object is never the old one; `null` over `null` is.
        changed.link = !(patch.link.is_null() && a.link.is_null());
        next.link = patch.link.clone();
    }

    if let Some(text) = &mut next.text {
        let mut text_changed = false;
        if let Some(f) = &patch.font_family
            && takes(PropKey::FontFamily)
        {
            text.font_family.clone_from(f);
            text_changed = true;
        }
        if let Some(s) = patch.font_size
            && takes(PropKey::FontSize)
        {
            text.font_size = s;
            text_changed = true;
        }
        if let Some(c) = &patch.font_color
            && takes(PropKey::FontColor)
        {
            text.font_color.clone_from(c);
            text_changed = true;
        }
        if let Some(t) = patch.text_align
            && takes(PropKey::TextAlign)
        {
            text.text_align = t;
            text_changed = true;
        }
        for (key, value, slot) in [
            (PropKey::Bold, patch.bold, &mut text.bold),
            (PropKey::Italic, patch.italic, &mut text.italic),
            (PropKey::Underline, patch.underline, &mut text.underline),
        ] {
            if let Some(v) = value
                && takes(key)
                && slot.unwrap_or(false) != v
            {
                *slot = Some(v);
                text_changed = true;
            }
        }
        changed.text = text_changed;
    }

    // `next === a` in the TypeScript: nothing replaced, not even an icon or
    // link of the same value.
    let replaced = changed.style
        || changed.geom
        || changed.text
        || (patch.icon.is_some() && takes(PropKey::Icon))
        || (!patch.link.is_undefined() && takes(PropKey::Link) && a.subtype == "link");
    replaced.then_some((next, changed))
}

/// Apply a property patch to one annotation, honouring its kind's declared
/// keys. `None` when nothing applied (locked or read-only, or no declared
/// key in the patch).
pub fn apply_props(a: &Annot, patch: &AnnotationPropsPatch) -> Option<Annot> {
    apply_props_tracked(a, patch).map(|(next, _)| next)
}

/// The ordered specs every given kind declares, in the first kind's order.
pub fn shared_props(subtypes: &[String]) -> Vec<PropSpec> {
    let mut unique: Vec<&str> = Vec::new();
    for s in subtypes {
        if !unique.contains(&s.as_str()) {
            unique.push(s);
        }
    }
    let Some(first) = unique.first() else {
        return Vec::new();
    };
    let first = props_for(first);
    first
        .iter()
        .filter(|p| unique[1..].iter().all(|s| props_for(s).iter().any(|q| q.key == p.key)))
        .cloned()
        .collect()
}

/// `{ ...m.style, ...initialTextStyle, ...d, lineEndings }`: a resolved props
/// bag from a base style and an optional per-tool patch.
pub fn resolve_props(style: &Style, d: Option<&AnnotationPropsPatch>) -> AnnotationProps {
    let text = initial_text_style();
    let mut out = AnnotationProps {
        color: style.color.clone(),
        interior_color: style.interior_color.clone(),
        stroke_width: style.stroke_width,
        opacity: style.opacity,
        blend_mode: style.blend_mode,
        border: style.border.clone(),
        font_family: text.font_family,
        font_size: text.font_size,
        font_color: text.font_color,
        text_align: text.text_align,
        bold: None,
        italic: None,
        underline: None,
        line_endings: NO_ENDINGS,
        icon: None,
        link: Nullable::Undefined,
        extra: serde_json::Map::new(),
    };
    let Some(d) = d else {
        return out;
    };
    if let Some(c) = &d.color {
        out.color.clone_from(c);
    }
    if !d.interior_color.is_undefined() {
        out.interior_color = d.interior_color.as_option().cloned();
    }
    if let Some(v) = d.stroke_width {
        out.stroke_width = v;
    }
    if let Some(v) = d.opacity {
        out.opacity = v;
    }
    if let Some(v) = d.blend_mode {
        out.blend_mode = v;
    }
    if let Some(v) = &d.border {
        out.border = v.clone();
    }
    if let Some(v) = &d.font_family {
        out.font_family.clone_from(v);
    }
    if let Some(v) = d.font_size {
        out.font_size = v;
    }
    if let Some(v) = &d.font_color {
        out.font_color.clone_from(v);
    }
    if let Some(v) = d.text_align {
        out.text_align = v;
    }
    out.bold = d.bold;
    out.italic = d.italic;
    out.underline = d.underline;
    if let Some(v) = &d.icon {
        out.icon = Some(v.clone());
    }
    out.link = d.link.clone();
    out.line_endings = d.line_endings.unwrap_or_default().over(NO_ENDINGS);
    out.extra = d.extra.iter().map(|(k, v): (&String, &Value)| (k.clone(), v.clone())).collect();
    out
}
