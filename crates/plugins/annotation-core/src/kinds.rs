//! `kinds/`: every annotation kind, one declaration each: its shape family,
//! how it is drawn (`styles.ts`), how its text is set (`texts.ts`), what a
//! user can do to it (`KindCaps`), and what a style panel edits
//! (`fields.ts`).

use serde_json::Value;

use crate::json::{self, Object};
use crate::object::Nullable;
use crate::shapes::Family;
use crate::types::{Style, TextStyle};
use crate::{js_object, json::num};

js_object! {
    /// Orthogonal capability flags (`KindCaps`).
    pub struct KindCaps {
        "selectable" => pub selectable: bool,
        "movable" => pub movable: bool,
        "resizable" => pub resizable: bool,
        "vertexEditable" => pub vertex_editable: bool,
        "rotatable" => pub rotatable: bool,
        "groupMovable" => pub group_movable: bool,
        "groupResizable" => pub group_resizable: bool,
        "groupRotatable" => pub group_rotatable: bool,
        "textEditable" => pub text_editable: bool,
        "commentable" => pub commentable: bool,
        "hasPopup" => pub has_popup: bool,
        "anchored" => pub anchored: bool,
        "paintsBeneath" => pub paints_beneath: bool,
        "hasFill" => pub has_fill: bool,
        "hasEndings" => pub has_endings: bool,
        "hasCloudy" => pub has_cloudy: bool,
        "opaqueBody" => pub opaque_body: bool,
        "rasterOnly" => pub raster_only: bool,
        "ignoresReadOnly" => pub ignores_read_only: bool,
        "noZoom" => pub no_zoom: bool,
        "noRotate" => pub no_rotate: bool,
    }
}

/// `NO_CAPS`: no capability at all.
pub const NO_CAPS: KindCaps = KindCaps {
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
    paints_beneath: false,
    has_fill: false,
    has_endings: false,
    has_cloudy: false,
    opaque_body: false,
    raster_only: false,
    ignores_read_only: false,
    no_zoom: false,
    no_rotate: false,
    order: crate::object::Order(Vec::new()),
};

/// How a kind reads its style off an annotation.
pub type StyleFn = fn(&Object) -> Style;
/// How a kind reads its text off an annotation.
pub type TextFn = fn(&Object) -> TextStyle;

/// One annotation kind (`AnnotationKind`).
#[derive(Clone, Debug)]
pub struct AnnotationKind {
    pub name: String,
    pub family: Family,
    pub style: StyleFn,
    pub text: Option<TextFn>,
    pub caps: KindCaps,
    /// What a style panel edits, in display order; flags are added by `propertiesOf`.
    pub properties: fn() -> Vec<Value>,
}

impl PartialEq for AnnotationKind {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl AnnotationKind {
    /// The kind's own properties.
    pub fn own_properties(&self) -> Vec<Value> {
        (self.properties)()
    }

    pub fn style_of(&self, annotation: &Object) -> Style {
        (self.style)(annotation)
    }

    pub fn text_of(&self, annotation: &Object) -> Option<TextStyle> {
        self.text.map(|t| t(annotation))
    }
}

// ── fields.ts ───────────────────────────────────────────────────────────────

fn s(text: &str) -> Value {
    Value::String(text.into())
}

fn property<const N: usize>(members: [(&str, Value); N]) -> Value {
    Value::Object(json::object(members))
}

fn option(value: &str, label: &str, key: &str) -> Value {
    property([("value", s(value)), ("label", s(label)), ("labelKey", s(key))])
}

fn color(key: &str, label: &str, label_key: &str) -> Value {
    property([
        ("key", s(key)),
        ("control", s("color")),
        ("label", s(label)),
        ("labelKey", s(label_key)),
    ])
}

fn number(key: &str, label: &str, label_key: &str, min: f64, max: f64, step: f64) -> Value {
    property([
        ("key", s(key)),
        ("control", s("number")),
        ("label", s(label)),
        ("labelKey", s(label_key)),
        ("min", num(min)),
        ("max", num(max)),
        ("step", num(step)),
    ])
}

fn opacity() -> Value {
    number("opacity", "Opacity", "annotation.style.opacity", 0.1, 1.0, 0.05)
}

fn stroke() -> Value {
    color("color", "Stroke", "annotation.style.stroke")
}

fn fill() -> Value {
    color("interiorColor", "Fill", "annotation.style.fill")
}

fn color_field() -> Value {
    color("color", "Color", "annotation.style.color")
}

fn stroke_width() -> Value {
    number("strokeWidth", "Stroke width", "annotation.style.strokeWidth", 0.5, 30.0, 0.5)
}

fn border(cloudy: bool, label: &str, label_key: &str) -> Value {
    let mut options = vec![
        option("solid", "Solid", "annotation.borderStyles.solid"),
        option("dashed", "Dashed", "annotation.borderStyles.dashed"),
    ];
    if cloudy {
        options.push(option("cloudy", "Cloudy", "annotation.borderStyles.cloudy"));
    }
    property([
        ("key", s("borderStyle")),
        ("control", s("choice")),
        ("label", s(label)),
        ("labelKey", s(label_key)),
        ("options", Value::Array(options)),
        ("cloudy", Value::Bool(cloudy)),
    ])
}

fn choice(key: &str, label: &str, label_key: &str, options: Vec<Value>) -> Value {
    property([
        ("key", s(key)),
        ("control", s("choice")),
        ("label", s(label)),
        ("labelKey", s(label_key)),
        ("options", Value::Array(options)),
    ])
}

/// Every line ending the engine draws (`LINE_ENDING_NAMES`).
pub const LINE_ENDING_NAMES: [&str; 10] = [
    "none",
    "square",
    "circle",
    "diamond",
    "open-arrow",
    "closed-arrow",
    "butt",
    "r-open-arrow",
    "r-closed-arrow",
    "slash",
];

fn line_endings() -> Value {
    let options = vec![
        option("none", "None", "annotation.lineEndings.none"),
        option("square", "Square", "annotation.lineEndings.square"),
        option("circle", "Circle", "annotation.lineEndings.circle"),
        option("diamond", "Diamond", "annotation.lineEndings.diamond"),
        option("open-arrow", "Open arrow", "annotation.lineEndings.openArrow"),
        option("closed-arrow", "Closed arrow", "annotation.lineEndings.closedArrow"),
        option("butt", "Butt", "annotation.lineEndings.butt"),
        option("r-open-arrow", "Reverse open arrow", "annotation.lineEndings.reverseOpenArrow"),
        option(
            "r-closed-arrow",
            "Reverse closed arrow",
            "annotation.lineEndings.reverseClosedArrow",
        ),
        option("slash", "Slash", "annotation.lineEndings.slash"),
    ];
    choice("lineEndings", "Line endings", "annotation.style.lineEndings", options)
}

fn blend_mode() -> Value {
    let modes = [
        ("normal", "Normal", "normal"),
        ("multiply", "Multiply", "multiply"),
        ("screen", "Screen", "screen"),
        ("overlay", "Overlay", "overlay"),
        ("darken", "Darken", "darken"),
        ("lighten", "Lighten", "lighten"),
        ("color-dodge", "Color dodge", "colorDodge"),
        ("color-burn", "Color burn", "colorBurn"),
        ("hard-light", "Hard light", "hardLight"),
        ("soft-light", "Soft light", "softLight"),
        ("difference", "Difference", "difference"),
        ("exclusion", "Exclusion", "exclusion"),
        ("hue", "Hue", "hue"),
        ("saturation", "Saturation", "saturation"),
        ("color", "Color", "color"),
        ("luminosity", "Luminosity", "luminosity"),
    ];
    let options =
        modes.iter().map(|(v, l, k)| option(v, l, &format!("annotation.blendModes.{k}"))).collect();
    choice("blendMode", "Blend mode", "annotation.style.blendMode", options)
}

fn font_family(label: &str, label_key: &str) -> Value {
    let fonts = [
        ("courier", "Courier", "courier"),
        ("courier-bold", "Courier Bold", "courierBold"),
        ("courier-bold-oblique", "Courier Bold Oblique", "courierBoldOblique"),
        ("courier-oblique", "Courier Oblique", "courierOblique"),
        ("helvetica", "Helvetica", "helvetica"),
        ("helvetica-bold", "Helvetica Bold", "helveticaBold"),
        ("helvetica-bold-oblique", "Helvetica Bold Oblique", "helveticaBoldOblique"),
        ("helvetica-oblique", "Helvetica Oblique", "helveticaOblique"),
        ("times-roman", "Times Roman", "timesRoman"),
        ("times-bold", "Times Bold", "timesBold"),
        ("times-bold-italic", "Times Bold Italic", "timesBoldItalic"),
        ("times-italic", "Times Italic", "timesItalic"),
        ("symbol", "Symbol", "symbol"),
        ("zapf-dingbats", "Zapf Dingbats", "zapfDingbats"),
    ];
    let options =
        fonts.iter().map(|(v, l, k)| option(v, l, &format!("annotation.fonts.{k}"))).collect();
    choice("fontFamily", label, label_key, options)
}

fn text_align(label: &str, label_key: &str) -> Value {
    choice(
        "textAlign",
        label,
        label_key,
        vec![
            option("left", "Left", "annotation.alignments.left"),
            option("center", "Center", "annotation.alignments.center"),
            option("right", "Right", "annotation.alignments.right"),
        ],
    )
}

fn linkable() -> Value {
    property([
        ("key", s("link")),
        ("control", s("link")),
        ("label", s("Link")),
        ("labelKey", s("annotation.style.link")),
    ])
}

fn shape_fields() -> Vec<Value> {
    vec![
        stroke(),
        fill(),
        opacity(),
        stroke_width(),
        border(true, "Border", "annotation.style.border"),
        linkable(),
    ]
}

fn line_fields() -> Vec<Value> {
    vec![
        stroke(),
        fill(),
        opacity(),
        stroke_width(),
        border(false, "Border", "annotation.style.border"),
        line_endings(),
        linkable(),
    ]
}

fn markup_fields() -> Vec<Value> {
    vec![color_field(), opacity(), blend_mode(), linkable()]
}

fn flag(key: &str, label: &str, label_key: &str) -> Value {
    property([
        ("key", s(key)),
        ("control", s("flag")),
        ("label", s(label)),
        ("labelKey", s(label_key)),
    ])
}

/// `FLAG_PROPERTIES`: the flags every kind has, in the order a panel lists them.
pub fn flag_properties() -> Vec<Value> {
    vec![
        flag("print", "Print", "annotation.flags.print"),
        flag("hidden", "Hidden", "annotation.flags.hidden"),
        flag("noView", "No view", "annotation.flags.noView"),
        flag("toggleNoView", "Toggle no view", "annotation.flags.toggleNoView"),
        flag("readOnly", "Read only", "annotation.flags.readOnly"),
        flag("locked", "Locked", "annotation.flags.locked"),
        flag("lockedContents", "Locked contents", "annotation.flags.lockedContents"),
        flag("noZoom", "No zoom", "annotation.flags.noZoom"),
        flag("noRotate", "No rotate", "annotation.flags.noRotate"),
        flag("invisible", "Invisible", "annotation.flags.invisible"),
    ]
}

fn text_format(key: &str, label: &str) -> Value {
    property([
        ("key", s(key)),
        ("control", s("textFormat")),
        ("label", s(label)),
        ("labelKey", s(&format!("annotation.style.{key}"))),
        ("format", s(key)),
    ])
}

fn text_box_fields() -> Vec<Value> {
    vec![
        font_family("Font", "annotation.style.font"),
        number("fontSize", "Font size", "annotation.style.fontSize", 4.0, 96.0, 1.0),
        color("fontColor", "Text color", "annotation.style.textColor"),
        text_format("bold", "Bold"),
        text_format("italic", "Italic"),
        text_format("underline", "Underline"),
        text_align("Align", "annotation.style.align"),
        opacity(),
        color("interiorColor", "Background", "annotation.style.background"),
        color("color", "Border", "annotation.style.border"),
        number("strokeWidth", "Border width", "annotation.style.borderWidth", 0.0, 12.0, 0.5),
        linkable(),
    ]
}

fn redact_fields() -> Vec<Value> {
    vec![
        color("color", "Outline", "annotation.style.outline"),
        fill(),
        opacity(),
        property([
            ("key", s("overlayText")),
            ("control", s("text")),
            ("label", s("Label")),
            ("labelKey", s("annotation.style.label")),
        ]),
        font_family("Label font", "annotation.style.labelFont"),
        number("fontSize", "Label size", "annotation.style.labelSize", 0.0, 96.0, 1.0),
        color("fontColor", "Label color", "annotation.style.labelColor"),
        text_align("Align", "annotation.style.align"),
    ]
}

fn note_fields() -> Vec<Value> {
    let icons = [
        ("comment", "Comment", "comment"),
        ("key", "Key", "key"),
        ("note", "Note", "note"),
        ("help", "Help", "help"),
        ("new-paragraph", "New paragraph", "newParagraph"),
        ("paragraph", "Paragraph", "paragraph"),
        ("insert", "Insert", "insert"),
    ];
    let options =
        icons.iter().map(|(v, l, k)| option(v, l, &format!("annotation.noteIcons.{k}"))).collect();
    vec![
        choice("icon", "Icon", "annotation.style.icon", options),
        color_field(),
        opacity(),
        linkable(),
    ]
}

fn attachment_fields() -> Vec<Value> {
    let icons = [
        ("push-pin", "Push pin", "pushPin"),
        ("paperclip", "Paperclip", "paperclip"),
        ("graph", "Graph", "graph"),
        ("tag", "Tag", "tag"),
    ];
    let options = icons
        .iter()
        .map(|(v, l, k)| option(v, l, &format!("annotation.attachmentIcons.{k}")))
        .collect();
    vec![choice("icon", "Icon", "annotation.style.icon", options), color_field(), opacity()]
}

fn widget_box_fields() -> Vec<Value> {
    vec![
        color("color", "Border color", "annotation.style.borderColor"),
        color("interiorColor", "Background", "annotation.style.background"),
        number("strokeWidth", "Border width", "annotation.style.borderWidth", 0.0, 12.0, 0.5),
        border(false, "Border style", "annotation.style.borderStyle"),
    ]
}

fn widget_text_fields() -> Vec<Value> {
    let mut out = widget_box_fields();
    out.push(font_family("Font", "annotation.style.font"));
    out.push(number("fontSize", "Font size", "annotation.style.fontSize", 0.0, 96.0, 1.0));
    out.push(color("fontColor", "Text color", "annotation.style.textColor"));
    out.push(text_align("Alignment", "annotation.style.alignment"));
    out
}

fn ink_fields() -> Vec<Value> {
    vec![color_field(), opacity(), stroke_width(), blend_mode(), linkable()]
}

fn caret_fields() -> Vec<Value> {
    vec![color_field(), opacity()]
}

fn stamp_fields() -> Vec<Value> {
    vec![opacity(), linkable()]
}

fn link_fields() -> Vec<Value> {
    vec![linkable()]
}

fn no_fields() -> Vec<Value> {
    Vec::new()
}

// ── styles.ts ───────────────────────────────────────────────────────────────

fn member(annotation: &Object, name: &str) -> Option<Value> {
    annotation.get(name).filter(|v| !json::undefined(Some(v))).cloned()
}

/// `('interiorColor' in a && a.interiorColor) || null`.
fn interior(annotation: &Object) -> Nullable<String> {
    match annotation.get("interiorColor") {
        Some(v) if json::truthy(Some(v)) => Nullable::from_option(v.as_str().map(str::to_string)),
        _ => Nullable::Null,
    }
}

fn plain_border(style: &mut Style) {
    style.border_style = Some(s("solid"));
    style.dash_array = Nullable::Null;
    style.cloudy_intensity = Nullable::Null;
}

fn stroke_number(annotation: &Object) -> f64 {
    json::get_num(annotation, "strokeWidth").unwrap_or(f64::NAN)
}

/// `strokedStyle(annotation)`: a kind drawn with a stroke.
pub fn stroked_style(annotation: &Object) -> Style {
    Style {
        color: member(annotation, "color"),
        interior_color: interior(annotation),
        stroke_width: stroke_number(annotation),
        opacity: member(annotation, "opacity"),
        blend_mode: member(annotation, "blendMode"),
        border_style: Some(json::get(annotation, "borderStyle").cloned().unwrap_or(s("solid"))),
        dash_array: match json::get(annotation, "dashArray") {
            Some(v) => Nullable::from_option(
                v.as_array().map(|a| a.iter().filter_map(json::number).collect()),
            ),
            None => Nullable::Null,
        },
        cloudy_intensity: match annotation.get("cloudyIntensity") {
            Some(v) if json::truthy(Some(v)) => Nullable::from_option(json::number(v)),
            _ => Nullable::Null,
        },
        order: Default::default(),
    }
}

fn coloured_style(annotation: &Object, stroke_width: f64) -> Style {
    let mut style = Style {
        color: member(annotation, "color"),
        interior_color: interior(annotation),
        stroke_width,
        opacity: member(annotation, "opacity"),
        blend_mode: member(annotation, "blendMode"),
        border_style: None,
        dash_array: Nullable::Null,
        cloudy_intensity: Nullable::Null,
        order: Default::default(),
    };
    plain_border(&mut style);
    style
}

/// `markupStyle`: text markup strokes nothing itself.
pub fn markup_style(annotation: &Object) -> Style {
    coloured_style(annotation, 0.0)
}

/// `caretStyle`: drawn 1 pt.
pub fn caret_style(annotation: &Object) -> Style {
    coloured_style(annotation, 1.0)
}

/// `redactStyle`: a 1.5 pt outline.
pub fn redact_style(annotation: &Object) -> Style {
    coloured_style(annotation, 1.5)
}

/// `iconStyle`: a note's or attachment's colour is the icon's.
pub fn icon_style(annotation: &Object) -> Style {
    coloured_style(annotation, 1.0)
}

/// `stampStyle`: its opacity is the only style it has.
pub fn stamp_style(annotation: &Object) -> Style {
    let mut style = Style {
        color: Some(s("#444444")),
        interior_color: Nullable::Null,
        stroke_width: 1.0,
        opacity: member(annotation, "opacity"),
        blend_mode: member(annotation, "blendMode"),
        border_style: None,
        dash_array: Nullable::Null,
        cloudy_intensity: Nullable::Null,
        order: Default::default(),
    };
    plain_border(&mut style);
    style
}

/// `widgetStyle`: a widget's border and background as the form draws them.
pub fn widget_style(annotation: &Object) -> Style {
    let border = match json::get(annotation, "borderStyle") {
        Some(Value::String(b)) if b == "underline" => Some(s("solid")),
        _ => member(annotation, "borderStyle"),
    };
    Style {
        color: Some(json::get(annotation, "color").cloned().unwrap_or(s("#1a1a1a"))),
        interior_color: Nullable::from_option(
            json::get(annotation, "interiorColor").and_then(Value::as_str).map(str::to_string),
        ),
        stroke_width: stroke_number(annotation),
        opacity: Some(num(1.0)),
        blend_mode: member(annotation, "blendMode"),
        border_style: border,
        dash_array: Nullable::Null,
        cloudy_intensity: Nullable::Null,
        order: Default::default(),
    }
}

/// `plainStyle`: a kind the viewer only shows.
pub fn plain_style(annotation: &Object) -> Style {
    let mut style = Style {
        color: Some(s("#444444")),
        interior_color: Nullable::Null,
        stroke_width: 1.0,
        opacity: Some(num(1.0)),
        blend_mode: member(annotation, "blendMode"),
        border_style: None,
        dash_array: Nullable::Null,
        cloudy_intensity: Nullable::Null,
        order: Default::default(),
    };
    plain_border(&mut style);
    style
}

/// `blendFor(style)`: the CSS blend a live drawing composites with; `None` for normal.
pub fn blend_for(style: &Style) -> Option<String> {
    match &style.blend_mode {
        Some(Value::String(mode)) if mode == "normal" => None,
        Some(Value::String(mode)) => Some(mode.clone()),
        _ => None,
    }
}

/// `dashOf(style)`: the dash a border draws with.
pub fn dash_of(style: &Style) -> Option<Vec<f64>> {
    if style.cloudy_intensity.as_option().is_some_and(|i| *i != 0.0 && !i.is_nan())
        || style.border_str() != Some("dashed")
    {
        return None;
    }
    match style.dash_array.as_option() {
        Some(dash) if !dash.is_empty() => Some(dash.clone()),
        _ => Some(vec![3.0, 3.0]),
    }
}

// ── texts.ts ────────────────────────────────────────────────────────────────

/// `bodyText(annotation)`: a free text's text is its rich body.
pub fn body_text(annotation: &Object) -> TextStyle {
    let body = annotation.get("richText").and_then(|r| r.get("body")).and_then(Value::as_object);
    let align = match annotation.get("textAlign") {
        Some(Value::String(a)) if a == "justify" => Some(s("left")),
        other => other.filter(|v| !json::undefined(Some(v))).cloned(),
    };
    let weight = body.and_then(|b| json::get_num(b, "weight"));
    let italic = body.is_some_and(|b| json::truthy(b.get("italic")));
    let underline = body
        .and_then(|b| b.get("decoration"))
        .and_then(Value::as_array)
        .is_some_and(|d| d.iter().any(|x| x.as_str() == Some("underline")));
    TextStyle {
        font_family: member(annotation, "fontFamily"),
        font_size: member(annotation, "fontSize"),
        font_color: member(annotation, "fontColor"),
        text_align: align,
        bold: weight.filter(|w| *w >= 600.0).map(|_| true),
        italic: italic.then_some(true),
        underline: underline.then_some(true),
        order: Default::default(),
    }
}

/// `fieldText(annotation)`: a form field's `/DA`.
pub fn field_text(annotation: &Object) -> TextStyle {
    TextStyle {
        font_family: Some(json::get(annotation, "fontFamily").cloned().unwrap_or(s("helvetica"))),
        font_size: Some(json::get(annotation, "fontSize").cloned().unwrap_or(num(0.0))),
        font_color: Some(match annotation.get("fontColor") {
            Some(v) if json::truthy(Some(v)) => v.clone(),
            _ => s("#000000"),
        }),
        text_align: member(annotation, "textAlign"),
        bold: None,
        italic: None,
        underline: None,
        order: Default::default(),
    }
}

/// `labelText(annotation)`: a redaction's label.
pub fn label_text(annotation: &Object) -> TextStyle {
    TextStyle {
        font_family: member(annotation, "fontFamily"),
        font_size: member(annotation, "fontSize"),
        font_color: member(annotation, "fontColor"),
        text_align: member(annotation, "textAlign"),
        bold: None,
        italic: None,
        underline: None,
        order: Default::default(),
    }
}

// ── the kinds ───────────────────────────────────────────────────────────────

fn caps(set: impl FnOnce(&mut KindCaps)) -> KindCaps {
    let mut caps = NO_CAPS.clone();
    set(&mut caps);
    caps
}

fn shape_caps(fill: bool, cloudy: bool) -> KindCaps {
    caps(|c| {
        c.selectable = true;
        c.movable = true;
        c.resizable = true;
        c.rotatable = true;
        c.group_movable = true;
        c.group_resizable = true;
        c.group_rotatable = true;
        c.commentable = true;
        c.has_fill = fill;
        c.has_cloudy = cloudy;
    })
}

fn vertex_caps(fill: bool, cloudy: bool, endings: bool) -> KindCaps {
    caps(|c| {
        c.selectable = true;
        c.movable = true;
        c.vertex_editable = true;
        c.rotatable = true;
        c.group_movable = true;
        c.group_resizable = true;
        c.group_rotatable = true;
        c.commentable = true;
        c.has_fill = fill;
        c.has_cloudy = cloudy;
        c.has_endings = endings;
    })
}

fn markup_caps() -> KindCaps {
    caps(|c| {
        c.paints_beneath = true;
        c.selectable = true;
        c.anchored = true;
        c.commentable = true;
    })
}

fn text_box_caps(callout: bool) -> KindCaps {
    caps(|c| {
        c.selectable = true;
        c.movable = true;
        c.resizable = true;
        c.rotatable = !callout;
        c.group_movable = true;
        c.group_resizable = !callout;
        c.group_rotatable = !callout;
        c.text_editable = true;
        c.commentable = true;
        c.has_fill = true;
    })
}

fn widget_caps() -> KindCaps {
    caps(|c| {
        c.selectable = true;
        c.movable = true;
        c.resizable = true;
        c.group_movable = true;
        c.has_fill = true;
        c.opaque_body = true;
        c.raster_only = true;
        c.ignores_read_only = true;
    })
}

fn kind(
    name: &str,
    family: Family,
    style: StyleFn,
    text: Option<TextFn>,
    caps: KindCaps,
    properties: fn() -> Vec<Value>,
) -> AnnotationKind {
    AnnotationKind { name: name.into(), family, style, text, caps, properties }
}

/// The names of the kinds in `KINDS`, in its order.
pub const KIND_NAMES: [&str; 24] = [
    "square",
    "circle",
    "line",
    "polygon",
    "polyline",
    "ink",
    "free-text",
    "free-text-callout",
    "highlight",
    "underline",
    "squiggly",
    "strikeout",
    "caret",
    "redact",
    "text",
    "file-attachment",
    "stamp",
    "link",
    "widget-text",
    "widget-choice",
    "widget-button",
    "widget-toggle",
    "widget-radio",
    "widget-box",
];

/// The kind `KINDS` declares under `name`, if any.
pub fn declared_kind(name: &str) -> Option<AnnotationKind> {
    Some(match name {
        "square" => {
            kind(name, Family::Box, stroked_style, None, shape_caps(true, true), shape_fields)
        }
        "circle" => {
            kind(name, Family::Box, stroked_style, None, shape_caps(true, true), shape_fields)
        }
        "line" => kind(
            name,
            Family::Points,
            stroked_style,
            None,
            vertex_caps(false, false, true),
            line_fields,
        ),
        "polygon" => kind(
            name,
            Family::Points,
            stroked_style,
            None,
            vertex_caps(true, true, false),
            shape_fields,
        ),
        "polyline" => kind(
            name,
            Family::Points,
            stroked_style,
            None,
            vertex_caps(false, false, true),
            line_fields,
        ),
        "ink" => kind(
            name,
            Family::Points,
            stroked_style,
            None,
            caps(|c| {
                c.selectable = true;
                c.movable = true;
                c.rotatable = true;
                c.group_movable = true;
                c.group_resizable = true;
                c.group_rotatable = true;
                c.commentable = true;
            }),
            ink_fields,
        ),
        "free-text" => kind(
            name,
            Family::TextBox,
            stroked_style,
            Some(body_text),
            text_box_caps(false),
            text_box_fields,
        ),
        "free-text-callout" => kind(
            name,
            Family::TextBox,
            stroked_style,
            Some(body_text),
            text_box_caps(true),
            text_box_fields,
        ),
        "highlight" | "underline" | "squiggly" | "strikeout" => {
            kind(name, Family::Quads, markup_style, None, markup_caps(), markup_fields)
        }
        "caret" => kind(
            name,
            Family::Caret,
            caret_style,
            None,
            caps(|c| {
                c.selectable = true;
                c.anchored = true;
                c.commentable = true;
            }),
            caret_fields,
        ),
        "redact" => kind(
            name,
            Family::RedactMark,
            redact_style,
            Some(label_text),
            caps(|c| {
                c.paints_beneath = true;
                c.selectable = true;
                c.movable = true;
                c.resizable = true;
                c.anchored = true;
                c.commentable = true;
                c.has_fill = true;
            }),
            redact_fields,
        ),
        "text" => kind(
            name,
            Family::Box,
            icon_style,
            None,
            caps(|c| {
                c.selectable = true;
                c.movable = true;
                c.group_movable = true;
                c.commentable = true;
                c.has_popup = true;
                c.opaque_body = true;
                c.no_zoom = true;
                c.no_rotate = true;
            }),
            note_fields,
        ),
        "file-attachment" => kind(
            name,
            Family::Box,
            icon_style,
            None,
            caps(|c| {
                c.selectable = true;
                c.movable = true;
                c.group_movable = true;
                c.commentable = true;
                c.opaque_body = true;
                c.no_zoom = true;
                c.no_rotate = true;
            }),
            attachment_fields,
        ),
        "stamp" => kind(
            name,
            Family::Box,
            stamp_style,
            None,
            caps(|c| {
                c.selectable = true;
                c.movable = true;
                c.resizable = true;
                c.rotatable = true;
                c.group_movable = true;
                c.group_resizable = true;
                c.group_rotatable = true;
                c.commentable = true;
                c.opaque_body = true;
                c.raster_only = true;
            }),
            stamp_fields,
        ),
        "link" => kind(
            name,
            Family::Box,
            plain_style,
            None,
            caps(|c| {
                c.selectable = true;
                c.movable = true;
                c.resizable = true;
                c.group_movable = true;
                c.opaque_body = true;
                c.raster_only = true;
            }),
            link_fields,
        ),
        "widget-text" | "widget-choice" | "widget-button" => kind(
            name,
            Family::Box,
            widget_style,
            Some(field_text),
            widget_caps(),
            widget_text_fields,
        ),
        "widget-toggle" | "widget-radio" | "widget-box" => {
            kind(name, Family::Box, widget_style, None, widget_caps(), widget_box_fields)
        }
        _ => return None,
    })
}

/// `kindNamed(name)`: the kind named `name`; a name no kind has reads as
/// one that is only shown (drawn from the engine's raster, with nothing to
/// do to it).
///
/// The TypeScript looks the name up in a plain object, so a name
/// `Object.prototype` has (`constructor`, `toString`…) finds the
/// prototype's member, not a kind, and later reads of it throw. That is
/// not a kind the core can work with: here such a name reads as an only
/// shown kind, like any other unknown name (a kept safe divergence; the
/// harness reproduces the TypeScript's answers for those names).
pub fn kind_named(name: &str) -> AnnotationKind {
    declared_kind(name).unwrap_or_else(|| AnnotationKind {
        name: name.into(),
        family: Family::Box,
        style: plain_style,
        text: None,
        caps: caps(|c| c.raster_only = true),
        properties: no_fields,
    })
}

/// `widgetKindOf(family)`: the widget kind for a field family.
pub fn widget_kind_of(family: &str) -> &'static str {
    match family {
        "text" => "widget-text",
        "combobox" | "listbox" => "widget-choice",
        "pushbutton" => "widget-button",
        "checkbox" => "widget-toggle",
        "radio" => "widget-radio",
        _ => "widget-box",
    }
}

/// `fieldFamilyOfKind(kind)`: the field family a widget kind stands for.
pub fn field_family_of_kind(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "widget-text" => "text",
        "widget-choice" => "combobox",
        "widget-button" => "pushbutton",
        "widget-toggle" => "checkbox",
        "widget-radio" => "radio",
        "widget-box" => "signature",
        _ => return None,
    })
}
