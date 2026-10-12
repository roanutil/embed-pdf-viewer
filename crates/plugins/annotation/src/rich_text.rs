//! `rich-text.ts`: what the plugin decides about a free-text annotation's
//! rich document, kept pure so every editor behaves the same.
//!
//! - the rich document of any annotation (the DTO's, or one synthesised from
//!   the plain text and the `/DA` text style for a draft with no DTO yet);
//! - a flat props patch as the run delta it means for a text range, and the
//!   keys left for the body;
//! - what a range reads back for those keys;
//! - faces: a DTO font (a standard kebab name or a registered key), the face
//!   a run names, and a CSS family list.
//!
//! A body is a JSON object here: the DTO's is carried verbatim (a spread
//! over it has to keep every member the engine sent), and the synthesised
//! one is built in the TypeScript's member order.

use epdf_annotation_core::richtext::{
    RichTextParagraph, RichTextRange, RichTextStyleDelta, locate_offset, paragraphs_from_plain_text,
};
use epdf_annotation_core::{Annot, AnnotationPropsPatch, PropKey, TextAlign, TextStyle};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::value::{get, str_of};

/// `TextFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextFormat {
    Bold,
    Italic,
    Underline,
}

impl TextFormat {
    pub fn key(self) -> PropKey {
        match self {
            TextFormat::Bold => PropKey::Bold,
            TextFormat::Italic => PropKey::Italic,
            TextFormat::Underline => PropKey::Underline,
        }
    }
}

/// `TextSelection`: the editor's range inside one free-text annotation,
/// flat offsets over its plain projection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSelection {
    pub id: String,
    pub start: f64,
    pub end: f64,
}

/// `Face`: the face a run or a body names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
}

/// `FontHandle`: one registered font, as the local engine's `fonts.list()`
/// answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontHandle {
    pub key: String,
    pub family_name: String,
    pub weight: f64,
    pub italic: bool,
}

/// `RANGE_KEYS`: the props keys a text range takes as run deltas.
pub const RANGE_KEYS: [PropKey; 6] = [
    PropKey::FontFamily,
    PropKey::FontSize,
    PropKey::FontColor,
    PropKey::Bold,
    PropKey::Italic,
    PropKey::Underline,
];

// ── faces ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StandardFamily {
    Helvetica,
    Times,
    Courier,
    Symbol,
    ZapfDingbats,
}

impl StandardFamily {
    fn name(self) -> &'static str {
        match self {
            StandardFamily::Helvetica => "Helvetica",
            StandardFamily::Times => "Times",
            StandardFamily::Courier => "Courier",
            StandardFamily::Symbol => "Symbol",
            StandardFamily::ZapfDingbats => "ZapfDingbats",
        }
    }

    /// `STANDARD_STACKS`.
    fn stack(self) -> &'static str {
        match self {
            StandardFamily::Helvetica => "Helvetica, Arial, sans-serif",
            StandardFamily::Times => "\"Times New Roman\", Times, serif",
            StandardFamily::Courier => "\"Courier New\", Courier, monospace",
            StandardFamily::Symbol | StandardFamily::ZapfDingbats => "serif",
        }
    }
}

/// `STANDARD_FACES`, in the object literal's order.
const STANDARD_FACES: [(&str, StandardFamily, f64, bool); 14] = [
    ("courier", StandardFamily::Courier, 400.0, false),
    ("courier-bold", StandardFamily::Courier, 700.0, false),
    ("courier-bold-oblique", StandardFamily::Courier, 700.0, true),
    ("courier-oblique", StandardFamily::Courier, 400.0, true),
    ("helvetica", StandardFamily::Helvetica, 400.0, false),
    ("helvetica-bold", StandardFamily::Helvetica, 700.0, false),
    ("helvetica-bold-oblique", StandardFamily::Helvetica, 700.0, true),
    ("helvetica-oblique", StandardFamily::Helvetica, 400.0, true),
    ("times-roman", StandardFamily::Times, 400.0, false),
    ("times-bold", StandardFamily::Times, 700.0, false),
    ("times-bold-italic", StandardFamily::Times, 700.0, true),
    ("times-italic", StandardFamily::Times, 400.0, true),
    ("symbol", StandardFamily::Symbol, 400.0, false),
    ("zapf-dingbats", StandardFamily::ZapfDingbats, 400.0, false),
];

fn standard_face(font: &str) -> Option<(StandardFamily, f64, bool)> {
    STANDARD_FACES.iter().find(|(name, ..)| *name == font).map(|(_, f, w, i)| (*f, *w, *i))
}

/// `STANDARD_FAMILY_KEYS`.
fn standard_family(key: &str) -> Option<StandardFamily> {
    Some(match key {
        "helvetica" | "arial" | "arialmt" | "sansserif" => StandardFamily::Helvetica,
        "times" | "timesroman" | "timesnewroman" | "timesnewromanpsmt" | "serif" => {
            StandardFamily::Times
        }
        "courier" | "couriernew" | "couriernewpsmt" | "monospace" => StandardFamily::Courier,
        "symbol" => StandardFamily::Symbol,
        "zapfdingbats" | "dingbats" => StandardFamily::ZapfDingbats,
        _ => return None,
    })
}

/// `FontLookup`: the registered fonts an engine knows.
pub type FontLookup<'a> = Option<&'a [FontHandle]>;

/// `familyKey(family)`: families compare without case, spaces, hyphens,
/// underscores and quotes.
pub fn family_key(family: &str) -> String {
    family
        .to_lowercase()
        .chars()
        .filter(|c| !(c.is_whitespace() || *c == '\u{feff}' || matches!(c, '-' | '_' | '\'' | '"')))
        .collect()
}

/// `faceForFont(font, fonts)`: a standard font's face, a registered key's
/// identity, else the string itself as a family.
pub fn face_for_font(font: &str, fonts: FontLookup) -> Face {
    if let Some((family, weight, italic)) = standard_face(font) {
        return Face { family: family.name().into(), weight: Some(weight), italic: Some(italic) };
    }
    if let Some(f) = fonts.unwrap_or_default().iter().find(|f| f.key == font) {
        return Face {
            family: f.family_name.clone(),
            weight: Some(f.weight),
            italic: Some(f.italic),
        };
    }
    Face { family: font.into(), weight: None, italic: None }
}

/// `fontForFace(face, fonts)`: the registered key whose identity matches
/// (closest weight, italic first), else the standard kebab name, else the
/// family itself.
pub fn font_for_face(face: &Face, fonts: FontLookup) -> String {
    let wanted = family_key(&face.family);
    let weight = face.weight.unwrap_or(400.0);
    let italic = face.italic.unwrap_or(false);
    let mut best: Option<(&str, f64)> = None;
    for f in fonts.unwrap_or_default() {
        if family_key(&f.family_name) != wanted {
            continue;
        }
        let score = (f.weight - weight).abs() + if f.italic == italic { 0.0 } else { 1000.0 };
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((&f.key, score));
        }
    }
    if let Some((key, _)) = best {
        return key.to_string();
    }
    if let Some(standard) = standard_family(&wanted) {
        let bold = weight >= 600.0;
        for (name, family, w, i) in STANDARD_FACES {
            if family == standard && (w >= 600.0) == bold && i == italic {
                return name.to_string();
            }
        }
        for (name, family, ..) in STANDARD_FACES {
            if family == standard {
                return name.to_string();
            }
        }
    }
    face.family.clone()
}

/// `cssFontFamilyForFace(family, fonts)`.
pub fn css_font_family_for_face(family: &str, fonts: FontLookup) -> String {
    let wanted = family_key(family);
    if let Some(standard) = standard_family(&wanted) {
        return standard.stack().into();
    }
    if let Some(f) = fonts.unwrap_or_default().iter().find(|f| family_key(&f.family_name) == wanted)
    {
        return format!("\"{}\"", f.key);
    }
    format!("\"{family}\", sans-serif")
}

/// `cssFontFamilyForFont(font)`: a standard kebab name's web stack, else
/// the key as a family.
pub fn css_font_family_for_font(font: &str) -> String {
    match standard_face(font) {
        Some((family, ..)) => family.stack().into(),
        None => format!("\"{font}\", sans-serif"),
    }
}

// ── documents ─────────────────────────────────────────────────────────────────

/// `RichTextDocument`: a complete body and the paragraphs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextDocument {
    pub body: Map<String, Value>,
    pub paragraphs: Vec<RichTextParagraph>,
}

fn hex(css: &str) -> String {
    css.trim().to_uppercase()
}

fn align_str(align: TextAlign) -> Value {
    serde_json::to_value(align).unwrap_or_default()
}

/// `bodyFromTextStyle(t, fonts)`: the rich body the `/DA` style describes.
pub fn body_from_text_style(t: &TextStyle, fonts: FontLookup) -> Map<String, Value> {
    let face = face_for_font(&t.font_family, fonts);
    let weight = if t.bold == Some(true) { 700.0 } else { face.weight.unwrap_or(400.0) };
    let italic = t.italic.or(face.italic).unwrap_or(false);
    let decoration: Vec<&str> = if t.underline == Some(true) { vec!["underline"] } else { vec![] };
    let mut body = Map::new();
    body.insert("family".into(), json!(face.family));
    body.insert("weight".into(), crate::value::number(weight));
    body.insert("italic".into(), json!(italic));
    body.insert("size".into(), crate::value::number(t.font_size));
    body.insert("color".into(), json!(hex(&t.font_color)));
    body.insert("decoration".into(), json!(decoration));
    body.insert("script".into(), json!("normal"));
    body.insert("letterSpacing".into(), json!(0));
    body.insert("horizontalScale".into(), json!(1));
    body.insert("align".into(), align_str(t.text_align));
    body.insert("dir".into(), json!("ltr"));
    body
}

/// `richDocOf(a, fonts)`: the DTO's rich document, else one synthesised
/// from the plain text and the text style.
pub fn rich_doc_of(a: &Annot, fonts: FontLookup) -> RichTextDocument {
    if let Some(d) = &a.data
        && str_of(d, "subtype") == Some("free-text")
        && let Some(rich) = get(d, "richText").filter(|r| crate::value::truthy(Some(r)))
        && let Ok(doc) = serde_json::from_value::<RichTextDocument>(rich.clone())
    {
        return doc;
    }
    let fallback = TextStyle {
        font_family: "helvetica".into(),
        font_size: 12.0,
        font_color: "#000000".into(),
        text_align: TextAlign::Left,
        bold: None,
        italic: None,
        underline: None,
    };
    let t = a.text.as_ref().unwrap_or(&fallback);
    let contents = a.data.as_ref().and_then(|d| str_of(d, "contents")).unwrap_or_default();
    RichTextDocument {
        body: body_from_text_style(t, fonts),
        paragraphs: paragraphs_from_plain_text(contents),
    }
}

/// `textCommitPatch(a, paragraphs, fonts)`: the rich paragraphs, their
/// body-equal paragraph properties stripped, and no body.
pub fn text_commit_patch(a: &Annot, paragraphs: &[RichTextParagraph], fonts: FontLookup) -> Value {
    let body = rich_doc_of(a, fonts).body;
    json!({ "richText": { "paragraphs": strip_body_defaults(paragraphs, &body) } })
}

/// `stripBodyDefaults(paragraphs, body)`: paragraph `align` and `dir` equal
/// to the body's are inherited, not overrides.
pub fn strip_body_defaults(
    paragraphs: &[RichTextParagraph],
    body: &Map<String, Value>,
) -> Vec<RichTextParagraph> {
    paragraphs
        .iter()
        .map(|p| {
            let differs = |key: &str| match p.props.get(key) {
                None => true,
                Some(v) => Some(v) != body.get(key),
            };
            if differs("align") && differs("dir") {
                return p.clone();
            }
            let mut out = p.clone();
            for key in ["align", "dir"] {
                if let Some(v) = p.props.get(key)
                    && Some(v) == body.get(key)
                {
                    out.props.remove(key);
                }
            }
            out
        })
        .collect()
}

// ── props and runs ────────────────────────────────────────────────────────────

/// `runDeltaForProps(patch, fonts)`: the run delta a range takes and the
/// keys left for the body.
pub fn run_delta_for_props(
    patch: &AnnotationPropsPatch,
    fonts: FontLookup,
) -> (RichTextStyleDelta, AnnotationPropsPatch) {
    let mut delta = RichTextStyleDelta::default();
    let mut rest = Map::new();
    let Value::Object(members) = serde_json::to_value(patch).unwrap_or_default() else {
        return (delta, AnnotationPropsPatch::default());
    };
    for (key, value) in members {
        match PropKey::parse(&key) {
            Some(PropKey::FontFamily) => {
                let face = face_for_font(value.as_str().unwrap_or_default(), fonts);
                delta.family = Some(face.family);
                if let Some(w) = face.weight {
                    delta.weight = Some(w);
                }
                if let Some(i) = face.italic {
                    delta.italic = Some(i);
                }
            }
            Some(PropKey::FontSize) => delta.size = value.as_f64(),
            Some(PropKey::FontColor) => delta.color = value.as_str().map(hex),
            Some(PropKey::Bold) => {
                delta.weight = Some(if value.as_bool() == Some(true) { 700.0 } else { 400.0 })
            }
            Some(PropKey::Italic) => delta.italic = value.as_bool(),
            Some(PropKey::Underline) => {
                delta.decoration = Some(if value.as_bool() == Some(true) {
                    vec!["underline".into()]
                } else {
                    vec![]
                })
            }
            _ => {
                rest.insert(key, value);
            }
        }
    }
    (delta, serde_json::from_value(Value::Object(rest)).unwrap_or_default())
}

fn para_len(p: &RichTextParagraph) -> f64 {
    p.runs.iter().fold(0.0, |n, r| n + epdf_annotation_core::js::utf16_len(&r.text) as f64)
}

/// `runsInRange(doc, range)`: the style deltas of every non-empty run a
/// range overlaps.
pub fn runs_in_range(
    paragraphs: &[RichTextParagraph],
    range: RichTextRange,
) -> Vec<RichTextStyleDelta> {
    let start = epdf_geometry::js::min(range.start, range.end);
    let end = epdf_geometry::js::max(range.start, range.end);
    if end <= start {
        return Vec::new();
    }
    let from = locate_offset(paragraphs, start);
    let to = locate_offset(paragraphs, end);
    let mut out = Vec::new();
    for (index, paragraph) in
        paragraphs.iter().enumerate().take(to.paragraph + 1).skip(from.paragraph)
    {
        let local_start = if index == from.paragraph { from.offset } else { 0.0 };
        let local_end = if index == to.paragraph { to.offset } else { para_len(paragraph) };
        let mut pos = 0.0;
        for run in &paragraph.runs {
            let len = epdf_annotation_core::js::utf16_len(&run.text) as f64;
            let run_end = pos + len;
            if run_end > local_start && pos < local_end && len > 0.0 {
                out.push(run.style.clone().unwrap_or_default());
            }
            pos = run_end;
        }
    }
    out
}

/// `rangeProps(doc, range, fonts)`: the values a range reads back for the
/// range keys, resolved against the body; `mixed` where runs disagree.
pub fn range_props(
    doc: &RichTextDocument,
    range: RichTextRange,
    fonts: FontLookup,
) -> Result<(Map<String, Value>, Vec<PropKey>), epdf_kernel::Error> {
    let body = &doc.body;
    let runs = runs_in_range(&doc.paragraphs, range);
    let runs = if runs.is_empty() { vec![RichTextStyleDelta::default()] } else { runs };
    let pick = |own: Option<Value>, key: &str| own.or_else(|| body.get(key).cloned());
    let styles: Vec<Map<String, Value>> = runs
        .iter()
        .map(|d| {
            let mut s = Map::new();
            let mut put = |key: &str, v: Option<Value>| {
                if let Some(v) = v {
                    s.insert(key.into(), v);
                }
            };
            put("family", pick(d.family.clone().map(Value::from), "family"));
            put("weight", pick(d.weight.map(crate::value::number), "weight"));
            put("italic", pick(d.italic.map(Value::from), "italic"));
            put("size", pick(d.size.map(crate::value::number), "size"));
            put("color", pick(d.color.clone().map(Value::from), "color"));
            put("decoration", pick(d.decoration.clone().map(|v| json!(v)), "decoration"));
            s
        })
        .collect();
    let body_weight = body.get("weight").and_then(Value::as_f64);
    let body_italic = body.get("italic").and_then(Value::as_bool);
    let mut values = Map::new();
    let mut mixed = Vec::new();
    // `of` answers `None` for `undefined`: the key stays out of `values`. A
    // style is read only until the first one that differs, as `some` stops.
    type Of<'a> = &'a dyn Fn(&Map<String, Value>) -> Result<Option<Value>, epdf_kernel::Error>;
    let mut read = |key: PropKey, of: Of| -> Result<(), epdf_kernel::Error> {
        let first = of(&styles[0])?;
        match &first {
            Some(v) => values.insert(key.as_str().into(), v.clone()),
            None => values.remove(key.as_str()),
        };
        for s in &styles {
            if of(s)? != first {
                mixed.push(key);
                break;
            }
        }
        Ok(())
    };
    read(PropKey::FontFamily, &|s| {
        let family = s.get("family").and_then(Value::as_str).unwrap_or_default().to_string();
        Ok(Some(json!(font_for_face(
            &Face { family, weight: body_weight, italic: body_italic },
            fonts
        ))))
    })?;
    read(PropKey::FontSize, &|s| Ok(s.get("size").cloned()))?;
    // `s.color!.toLowerCase()`: a body and run with no color throw.
    read(PropKey::FontColor, &|s| match s.get("color") {
        Some(Value::String(color)) => Ok(Some(json!(color.to_lowercase()))),
        other => Err(epdf_kernel::Error::named(
            "TypeError",
            format!(
                "Cannot read properties of {} (reading 'toLowerCase')",
                if other.is_some_and(Value::is_null) { "null" } else { "undefined" }
            ),
        )),
    })?;
    read(PropKey::Bold, &|s| {
        Ok(Some(json!(s.get("weight").and_then(Value::as_f64).unwrap_or(400.0) >= 600.0)))
    })?;
    read(PropKey::Italic, &|s| {
        Ok(Some(json!(s.get("italic").and_then(Value::as_bool).unwrap_or(false))))
    })?;
    read(PropKey::Underline, &|s| {
        Ok(Some(json!(
            s.get("decoration")
                .and_then(Value::as_array)
                .is_some_and(|d| d.iter().any(|x| x.as_str() == Some("underline")))
        )))
    })?;
    Ok((values, mixed))
}
