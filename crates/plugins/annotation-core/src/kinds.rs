//! `kinds.ts`: the declarative kind table. One entry per subtype: which
//! geometry variant it renders as, its capability flags, and its editable
//! properties in display order.

use std::sync::LazyLock;

use serde::Serialize;
use serde::ser::SerializeMap;

use crate::types::{GeomVariant, PropKey};

/// One editable property of a kind, as a UI contract. The members beyond
/// `key` and `label` depend on the key: a range for `opacity`, `strokeWidth`
/// and `fontSize`, `cloudy` for `border`, `options` for `icon`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropSpec {
    pub key: PropKey,
    pub label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<&'static [&'static str]>,
}

const fn spec(key: PropKey, label: &'static str) -> PropSpec {
    PropSpec { key, label, min: None, max: None, step: None, cloudy: None, options: None }
}

const fn ranged(key: PropKey, label: &'static str, min: f64, max: f64, step: f64) -> PropSpec {
    PropSpec {
        key,
        label,
        min: Some(min),
        max: Some(max),
        step: Some(step),
        cloudy: None,
        options: None,
    }
}

const fn border(label: &'static str, cloudy: bool) -> PropSpec {
    PropSpec {
        key: PropKey::Border,
        label,
        min: None,
        max: None,
        step: None,
        cloudy: Some(cloudy),
        options: None,
    }
}

const fn icon(options: &'static [&'static str]) -> PropSpec {
    PropSpec {
        key: PropKey::Icon,
        label: "Icon",
        min: None,
        max: None,
        step: None,
        cloudy: None,
        options: Some(options),
    }
}

/// Orthogonal capability flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindCaps {
    pub selectable: bool,
    pub movable: bool,
    pub resizable: bool,
    pub vertex_editable: bool,
    pub rotatable: bool,
    pub group_movable: bool,
    pub group_resizable: bool,
    pub group_rotatable: bool,
    pub text_editable: bool,
    pub commentable: bool,
    pub has_popup: bool,
    pub anchored: bool,
    pub has_fill: bool,
    pub has_endings: bool,
    pub has_cloudy: bool,
    pub opaque_body: bool,
    pub ignores_read_only: bool,
    pub no_zoom: bool,
    pub no_rotate: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationKind {
    pub subtype: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<&'static str>,
    pub variant: GeomVariant,
    pub caps: KindCaps,
    pub props: Vec<PropSpec>,
}

const OPACITY: PropSpec = ranged(PropKey::Opacity, "Opacity", 0.1, 1.0, 0.05);
const ICON_COLOR: PropSpec = spec(PropKey::Color, "Color");
const NOTE_ICONS: &[&str] =
    &["comment", "key", "note", "help", "new-paragraph", "paragraph", "insert"];
const FILE_ATTACHMENT_ICONS: &[&str] = &["push-pin", "paperclip", "graph", "tag"];
const STROKE: PropSpec = spec(PropKey::Color, "Stroke");
const FILL: PropSpec = spec(PropKey::InteriorColor, "Fill");
const STROKE_WIDTH: PropSpec = ranged(PropKey::StrokeWidth, "Stroke width", 0.5, 30.0, 0.5);
const BORDER_CLOUDY: PropSpec = border("Border", true);
const BORDER_PLAIN: PropSpec = border("Border", false);
const LINE_ENDINGS: PropSpec = spec(PropKey::LineEndings, "Line endings");
const BLEND_MODE: PropSpec = spec(PropKey::BlendMode, "Blend mode");
const LINKABLE: PropSpec = spec(PropKey::Link, "Link");

fn shape_props() -> Vec<PropSpec> {
    vec![STROKE, FILL, OPACITY, STROKE_WIDTH, BORDER_CLOUDY, LINKABLE]
}

fn widget_box_props() -> Vec<PropSpec> {
    vec![
        spec(PropKey::Color, "Border color"),
        spec(PropKey::InteriorColor, "Background"),
        ranged(PropKey::StrokeWidth, "Border width", 0.0, 12.0, 0.5),
        border("Border style", false),
    ]
}

fn widget_text_props() -> Vec<PropSpec> {
    let mut props = widget_box_props();
    props.extend([
        spec(PropKey::FontFamily, "Font"),
        ranged(PropKey::FontSize, "Font size", 0.0, 96.0, 1.0),
        spec(PropKey::FontColor, "Text color"),
        spec(PropKey::TextAlign, "Alignment"),
    ]);
    props
}

fn line_props() -> Vec<PropSpec> {
    vec![STROKE, FILL, OPACITY, STROKE_WIDTH, BORDER_PLAIN, LINE_ENDINGS, LINKABLE]
}

fn mark_props() -> Vec<PropSpec> {
    vec![spec(PropKey::Color, "Color"), OPACITY, BLEND_MODE, LINKABLE]
}

fn caret_props() -> Vec<PropSpec> {
    vec![spec(PropKey::Color, "Color"), OPACITY]
}

fn ink_props() -> Vec<PropSpec> {
    vec![spec(PropKey::Color, "Color"), OPACITY, STROKE_WIDTH, BLEND_MODE, LINKABLE]
}

fn redact_props() -> Vec<PropSpec> {
    vec![
        spec(PropKey::Color, "Outline"),
        FILL,
        OPACITY,
        spec(PropKey::FontFamily, "Label font"),
        ranged(PropKey::FontSize, "Label size", 0.0, 96.0, 1.0),
        spec(PropKey::FontColor, "Label color"),
        spec(PropKey::TextAlign, "Align"),
    ]
}

fn text_props() -> Vec<PropSpec> {
    vec![
        spec(PropKey::FontFamily, "Font"),
        ranged(PropKey::FontSize, "Font size", 4.0, 96.0, 1.0),
        spec(PropKey::FontColor, "Text color"),
        spec(PropKey::TextAlign, "Align"),
        OPACITY,
        spec(PropKey::InteriorColor, "Background"),
        spec(PropKey::Color, "Border"),
        ranged(PropKey::StrokeWidth, "Border width", 0.0, 12.0, 0.5),
        LINKABLE,
    ]
}

/// Read-only fallback for unknown subtypes: every capability off.
pub const READONLY: KindCaps = KindCaps {
    selectable: false,
    movable: false,
    resizable: false,
    vertex_editable: false,
    rotatable: false,
    group_movable: false,
    group_resizable: false,
    group_rotatable: false,
    text_editable: false,
    commentable: false,
    has_popup: false,
    anchored: false,
    has_fill: false,
    has_endings: false,
    has_cloudy: false,
    opaque_body: false,
    ignores_read_only: false,
    no_zoom: false,
    no_rotate: false,
};

const WIDGET_CAPS: KindCaps = KindCaps {
    selectable: true,
    movable: true,
    resizable: true,
    group_movable: true,
    has_fill: true,
    opaque_body: true,
    ignores_read_only: true,
    ..READONLY
};

const MARKUP_CAPS: KindCaps =
    KindCaps { selectable: true, anchored: true, commentable: true, ..READONLY };

/// Shapes, lines and polys all move, rotate and take part in every group op.
const TRANSFORMABLE: KindCaps = KindCaps {
    selectable: true,
    movable: true,
    rotatable: true,
    group_movable: true,
    group_resizable: true,
    group_rotatable: true,
    commentable: true,
    ..READONLY
};

fn kind(
    subtype: &'static str,
    variant: GeomVariant,
    caps: KindCaps,
    props: Vec<PropSpec>,
) -> AnnotationKind {
    AnnotationKind { subtype, intent: None, variant, caps, props }
}

/// The built-in kinds, in the TypeScript table's order.
pub static KINDS: LazyLock<Vec<AnnotationKind>> = LazyLock::new(|| {
    use GeomVariant as V;
    vec![
        kind("widget-text", V::Rect, WIDGET_CAPS, widget_text_props()),
        kind("widget-choice", V::Rect, WIDGET_CAPS, widget_text_props()),
        kind("widget-button", V::Rect, WIDGET_CAPS, widget_text_props()),
        kind("widget-toggle", V::Rect, WIDGET_CAPS, widget_box_props()),
        kind("widget-box", V::Rect, WIDGET_CAPS, widget_box_props()),
        kind(
            "free-text",
            V::Text,
            KindCaps { resizable: true, text_editable: true, has_fill: true, ..TRANSFORMABLE },
            text_props(),
        ),
        kind(
            "square",
            V::Rect,
            KindCaps { resizable: true, has_fill: true, has_cloudy: true, ..TRANSFORMABLE },
            shape_props(),
        ),
        kind(
            "circle",
            V::Rect,
            KindCaps { resizable: true, has_fill: true, has_cloudy: true, ..TRANSFORMABLE },
            shape_props(),
        ),
        kind(
            "line",
            V::Line,
            KindCaps { vertex_editable: true, has_endings: true, ..TRANSFORMABLE },
            line_props(),
        ),
        kind(
            "polygon",
            V::Poly,
            KindCaps { vertex_editable: true, has_fill: true, has_cloudy: true, ..TRANSFORMABLE },
            shape_props(),
        ),
        kind(
            "polyline",
            V::Poly,
            KindCaps { vertex_editable: true, has_endings: true, ..TRANSFORMABLE },
            line_props(),
        ),
        kind("ink", V::Ink, TRANSFORMABLE, ink_props()),
        kind("highlight", V::Quads, MARKUP_CAPS, mark_props()),
        kind("underline", V::Quads, MARKUP_CAPS, mark_props()),
        kind("squiggly", V::Quads, MARKUP_CAPS, mark_props()),
        kind("strikeout", V::Quads, MARKUP_CAPS, mark_props()),
        kind(
            "redact",
            V::Quads,
            KindCaps { movable: true, resizable: true, has_fill: true, ..MARKUP_CAPS },
            redact_props(),
        ),
        kind("caret", V::Caret, MARKUP_CAPS, caret_props()),
        kind(
            "text",
            V::Rect,
            KindCaps {
                selectable: true,
                movable: true,
                group_movable: true,
                commentable: true,
                has_popup: true,
                opaque_body: true,
                no_zoom: true,
                no_rotate: true,
                ..READONLY
            },
            vec![icon(NOTE_ICONS), ICON_COLOR, OPACITY, LINKABLE],
        ),
        kind(
            "file-attachment",
            V::Rect,
            KindCaps {
                selectable: true,
                movable: true,
                group_movable: true,
                commentable: true,
                opaque_body: true,
                no_zoom: true,
                no_rotate: true,
                ..READONLY
            },
            vec![icon(FILE_ATTACHMENT_ICONS), ICON_COLOR, OPACITY],
        ),
        kind(
            "stamp",
            V::Rect,
            KindCaps { resizable: true, opaque_body: true, ..TRANSFORMABLE },
            vec![LINKABLE],
        ),
        kind(
            "link",
            V::Rect,
            KindCaps {
                selectable: true,
                movable: true,
                resizable: true,
                group_movable: true,
                opaque_body: true,
                ..READONLY
            },
            vec![LINKABLE],
        ),
    ]
});

/// `KINDS` as the TypeScript object prints: keyed by subtype.
pub struct KindTable;

impl Serialize for KindTable {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(KINDS.len()))?;
        for k in KINDS.iter() {
            map.serialize_entry(k.subtype, k)?;
        }
        map.end()
    }
}

/// `KINDS[subtype]`. A prototype member (`constructor`, ...) is found in
/// the TypeScript, but it carries no `caps`, `props` or `variant`, so every
/// caller treats it as missing too.
pub fn kind_of(subtype: &str) -> Option<&'static AnnotationKind> {
    KINDS.iter().find(|k| k.subtype == subtype)
}

/// The capabilities of a subtype, or [`READONLY`] for unknown kinds.
pub fn caps_for(subtype: &str) -> KindCaps {
    kind_of(subtype).map_or(READONLY, |k| k.caps)
}

/// A kind's editable properties in display order; empty for unknown kinds.
pub fn props_for(subtype: &str) -> &'static [PropSpec] {
    kind_of(subtype).map_or(&[], |k| k.props.as_slice())
}

/// A text-markup kind (quads variant): drawn beneath every other kind.
pub fn is_markup(subtype: &str) -> bool {
    kind_of(subtype).is_some_and(|k| k.variant == GeomVariant::Quads)
}
