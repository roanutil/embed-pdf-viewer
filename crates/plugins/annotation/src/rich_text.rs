//! `rich-text.ts`: what the plugin decides about a free-text annotation's
//! rich document, kept pure so every editor behaves the same: a flat patch as
//! the run delta it means for a text range, what a range reads back, faces,
//! and the commit rule.
//!
//! Documents stay the engine's JSON (a DTO's body and paragraphs carried
//! verbatim, in the order the engine wrote their members); the run algebra
//! reads them into `epdf_annotation_core::richtext` types where it works on
//! them.

use epdf_annotation_core::engine::text::{FontIdentity, face_for_free_text_font, standard_face};
use epdf_annotation_core::font_css::{mounted_font_family, standard_font_stack};
use epdf_annotation_core::record::text_of;
use epdf_annotation_core::richtext::{
    OffsetLocation, RichTextParagraph, RichTextRange, locate_offset,
};
use epdf_annotation_core::types::TextStyle;
use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::js::{Object, num, number};

/// `TextFormat`: `'bold' | 'italic' | 'underline'`.
pub const TEXT_FORMATS: [&str; 3] = ["bold", "italic", "underline"];

/// `TextSelection`: the editor's range inside one free-text annotation, flat
/// offsets over its plain projection.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSelection {
    pub id: String,
    pub start: f64,
    pub end: f64,
}

impl TextSelection {
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "start": num(self.start), "end": num(self.end) })
    }
}

/// `Face`: the face a run or a body names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Face {
    pub family: String,
    pub weight: Option<f64>,
    pub italic: Option<bool>,
}

impl Face {
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("family".into(), Value::String(self.family.clone()));
        if let Some(w) = self.weight {
            out.insert("weight".into(), num(w));
        }
        if let Some(i) = self.italic {
            out.insert("italic".into(), Value::Bool(i));
        }
        Value::Object(out)
    }

    pub fn read(value: &Value) -> Face {
        Face {
            family: value.get("family").and_then(Value::as_str).unwrap_or_default().to_string(),
            weight: value.get("weight").and_then(number),
            italic: value.get("italic").and_then(Value::as_bool),
        }
    }
}

/// `FontHandle`: one registered font, as the local engine's `fonts.list()`
/// answers it.
#[derive(Clone, Debug, PartialEq)]
pub struct FontHandle {
    pub key: String,
    pub family_name: String,
    pub weight: f64,
    pub italic: bool,
}

impl FontHandle {
    pub fn read(value: &Value) -> Option<FontHandle> {
        Some(FontHandle {
            key: value.get("key")?.as_str()?.to_string(),
            family_name: value.get("familyName")?.as_str()?.to_string(),
            weight: value.get("weight").and_then(number)?,
            italic: value.get("italic")?.as_bool()?,
        })
    }
}

/// `FontLookup`: the registered fonts an engine knows (the local engine's
/// list; none on the cloud engine). `None` is no lookup at all.
pub type FontLookup<'a> = Option<&'a [FontHandle]>;

/// `RANGE_KEYS`: the fields a text range takes as run deltas.
pub const RANGE_KEYS: [&str; 6] =
    ["fontFamily", "fontSize", "fontColor", "bold", "italic", "underline"];

// ── faces ───────────────────────────────────────────────────────────────────

/// `STANDARD_FACES`, in the object literal's order.
const STANDARD_FACE_NAMES: [&str; 14] = [
    "courier",
    "courier-bold",
    "courier-bold-oblique",
    "courier-oblique",
    "helvetica",
    "helvetica-bold",
    "helvetica-bold-oblique",
    "helvetica-oblique",
    "times-roman",
    "times-bold",
    "times-bold-italic",
    "times-italic",
    "symbol",
    "zapf-dingbats",
];

/// `STANDARD_FAMILY_KEYS`.
fn standard_family(key: &str) -> Option<&'static str> {
    Some(match key {
        "helvetica" | "arial" | "arialmt" | "sansserif" => "Helvetica",
        "times" | "timesroman" | "timesnewroman" | "timesnewromanpsmt" | "serif" => "Times",
        "courier" | "couriernew" | "couriernewpsmt" | "monospace" => "Courier",
        "symbol" => "Symbol",
        "zapfdingbats" | "dingbats" => "ZapfDingbats",
        _ => return None,
    })
}

/// `familyKey(family)`: families compare without case, spaces, hyphens,
/// underscores and quotes (`/[\s\-_'"]/g`).
pub fn family_key(family: &str) -> String {
    family
        .to_lowercase()
        .chars()
        .filter(|c| !(js_space(*c) || matches!(c, '-' | '_' | '\'' | '"')))
        .collect()
}

/// JavaScript's `\s`.
fn js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// `faceForFont(font, fonts)`: a standard font's face, a registered key's
/// identity, else the string itself as a family.
pub fn face_for_font(font: &str, fonts: FontLookup) -> Face {
    let describe = |key: &str| -> Option<FontIdentity> {
        fonts?.iter().find(|h| h.key == key).map(|h| FontIdentity {
            family_name: h.family_name.clone(),
            weight: h.weight,
            italic: h.italic,
        })
    };
    let face = if fonts.is_some() {
        face_for_free_text_font(font, Some(&describe))
    } else {
        face_for_free_text_font(font, None)
    };
    Face { family: face.family, weight: face.weight, italic: face.italic }
}

/// `fontForFace(face, fonts)`: the registered key whose identity matches (the
/// closest weight, italic first), else the standard kebab name, else the
/// family itself.
pub fn font_for_face(face: &Face, fonts: FontLookup) -> String {
    let wanted = family_key(&face.family);
    let weight = face.weight.unwrap_or(400.0);
    let italic = face.italic.unwrap_or(false);
    let mut best: Option<(&str, f64)> = None;
    for handle in fonts.unwrap_or_default() {
        if family_key(&handle.family_name) != wanted {
            continue;
        }
        let score =
            (handle.weight - weight).abs() + if handle.italic == italic { 0.0 } else { 1000.0 };
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((&handle.key, score));
        }
    }
    if let Some((key, _)) = best {
        return key.to_string();
    }
    if let Some(standard) = standard_family(&wanted) {
        let bold = weight >= 600.0;
        for name in STANDARD_FACE_NAMES {
            let spec = standard_face(name).expect("a standard face");
            if spec.family == standard && (spec.weight >= 600.0) == bold && spec.italic == italic {
                return name.to_string();
            }
        }
        for name in STANDARD_FACE_NAMES {
            if standard_face(name).expect("a standard face").family == standard {
                return name.to_string();
            }
        }
    }
    face.family.clone()
}

/// `cssFontFamilyForFace(family, fonts)`: a standard family's web stack, a
/// registered family's mounted `@font-face`, else the family with a generic
/// fallback.
pub fn css_font_family_for_face(family: &str, fonts: FontLookup) -> String {
    let wanted = family_key(family);
    if let Some(standard) = standard_family(&wanted) {
        return standard_font_stack(standard).unwrap_or_default().to_string();
    }
    if let Some(handle) =
        fonts.unwrap_or_default().iter().find(|h| family_key(&h.family_name) == wanted)
    {
        return mounted_font_family(&handle.key);
    }
    format!("\"{family}\", sans-serif")
}

// ── documents ───────────────────────────────────────────────────────────────

/// `RichTextDocument`: a whole body and the paragraphs, as JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct RichTextDocument {
    pub body: Object,
    pub paragraphs: Vec<Value>,
}

impl RichTextDocument {
    pub fn to_json(&self) -> Value {
        json!({ "body": self.body, "paragraphs": self.paragraphs })
    }

    /// The paragraphs as the run algebra reads them.
    pub fn typed(&self) -> Vec<RichTextParagraph> {
        typed_paragraphs(&self.paragraphs)
    }
}

/// Paragraphs read into the run algebra's types.
pub fn typed_paragraphs(paragraphs: &[Value]) -> Vec<RichTextParagraph> {
    paragraphs
        .iter()
        .map(|p| {
            serde_json::from_value(p.clone())
                .unwrap_or(RichTextParagraph { runs: Vec::new(), props: Map::new() })
        })
        .collect()
}

fn hex(css: &str) -> String {
    css.trim().to_uppercase()
}

fn str_of(value: &Option<Value>) -> Option<&str> {
    value.as_ref().and_then(Value::as_str)
}

/// `bodyFromTextStyle(style, fonts)`: the rich body the `/DA` text style
/// describes.
pub fn body_from_text_style(style: &TextStyle, fonts: FontLookup) -> Object {
    let face = face_for_font(str_of(&style.font_family).unwrap_or("undefined"), fonts);
    let weight = if style.bold == Some(true) { 700.0 } else { face.weight.unwrap_or(400.0) };
    let italic = style.italic.or(face.italic).unwrap_or(false);
    let decoration: Vec<Value> =
        if style.underline == Some(true) { vec![json!("underline")] } else { vec![] };
    let mut body = Map::new();
    body.insert("family".into(), Value::String(face.family));
    body.insert("weight".into(), num(weight));
    body.insert("italic".into(), Value::Bool(italic));
    body.insert("size".into(), style.font_size.clone().unwrap_or(Value::Null));
    body.insert(
        "color".into(),
        str_of(&style.font_color).map_or(Value::Null, |c| Value::String(hex(c))),
    );
    body.insert("decoration".into(), Value::Array(decoration));
    body.insert("script".into(), json!("normal"));
    body.insert("letterSpacing".into(), json!(0));
    body.insert("horizontalScale".into(), json!(1));
    body.insert("align".into(), style.text_align.clone().unwrap_or(Value::Null));
    body.insert("dir".into(), json!("ltr"));
    body
}

/// `paragraphsFromPlainText(text)`: one paragraph per line, one unstyled run each.
pub fn paragraphs_from_plain_text(text: &str) -> Vec<Value> {
    split_lines(text).into_iter().map(|line| json!({ "runs": [{ "text": line }] })).collect()
}

/// `text.split(/\r\n|\r|\n/)`.
fn split_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(std::mem::take(&mut current));
            }
            '\n' => out.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    out.push(current);
    out
}

/// `richDocOf(annotation, fonts)`: a free text's own rich document, else one
/// made from its plain text and how its text is set.
pub fn rich_doc_of(annotation: &Object, fonts: FontLookup) -> RichTextDocument {
    if annotation.get("subtype").and_then(Value::as_str) == Some("free-text")
        && let Some(rich) = annotation.get("richText").filter(|r| truthy(r))
    {
        return RichTextDocument {
            body: rich.get("body").and_then(Value::as_object).cloned().unwrap_or_default(),
            paragraphs: rich
                .get("paragraphs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        };
    }
    let style = text_of(annotation).unwrap_or_else(|| TextStyle {
        font_family: Some(json!("helvetica")),
        font_size: Some(json!(12)),
        font_color: Some(json!("#000000")),
        text_align: Some(json!("left")),
        bold: None,
        italic: None,
        underline: None,
        order: Default::default(),
    });
    let contents = annotation.get("contents").and_then(Value::as_str).unwrap_or("");
    RichTextDocument {
        body: body_from_text_style(&style, fonts),
        paragraphs: paragraphs_from_plain_text(contents),
    }
}

fn truthy(value: &Value) -> bool {
    epdf_annotation_core::json::truthy(Some(value))
}

/// `textCommitPatch(record, paragraphs, fonts)`: the rich paragraphs, their
/// body-equal paragraph properties stripped, and no body.
pub fn text_commit_patch(annotation: &Object, paragraphs: &[Value], fonts: FontLookup) -> Value {
    let body = rich_doc_of(annotation, fonts).body;
    json!({ "richText": { "paragraphs": strip_body_defaults(paragraphs, &body) } })
}

/// `stripBodyDefaults(paragraphs, body)`: paragraph `align` and `dir` equal
/// to the body's are inherited, not overrides.
pub fn strip_body_defaults(paragraphs: &[Value], body: &Object) -> Vec<Value> {
    paragraphs
        .iter()
        .map(|paragraph| {
            let Some(p) = paragraph.as_object() else { return paragraph.clone() };
            let own = |k: &str| p.get(k).filter(|v| !epdf_kernel::is_undefined(v));
            let differs = |k: &str| own(k).is_none_or(|v| Some(v) != body.get(k));
            if differs("align") && differs("dir") {
                return paragraph.clone();
            }
            let mut rest = Map::new();
            for (k, v) in p {
                if k != "align" && k != "dir" {
                    rest.insert(k.clone(), v.clone());
                }
            }
            for key in ["align", "dir"] {
                if let Some(v) = own(key)
                    && Some(v) != body.get(key)
                {
                    rest.insert(key.into(), v.clone());
                }
            }
            Value::Object(rest)
        })
        .collect()
}

// ── fields and runs ─────────────────────────────────────────────────────────

/// `runDeltaForFields(patch, fonts)`: the run delta a range takes (a JSON
/// object in the order the patch named its keys) and the fields left for
/// the annotation.
pub fn run_delta_for_fields(patch: &Object, fonts: FontLookup) -> (Object, Object) {
    let mut delta = Map::new();
    let mut rest = Map::new();
    for (key, value) in patch {
        if epdf_kernel::is_undefined(value) {
            continue;
        }
        match key.as_str() {
            "fontFamily" => {
                let face = face_for_font(value.as_str().unwrap_or("undefined"), fonts);
                delta.insert("family".into(), Value::String(face.family));
                if let Some(w) = face.weight {
                    delta.insert("weight".into(), num(w));
                }
                if let Some(i) = face.italic {
                    delta.insert("italic".into(), Value::Bool(i));
                }
            }
            "fontSize" => {
                delta.insert("size".into(), value.clone());
            }
            "fontColor" => {
                delta.insert(
                    "color".into(),
                    value.as_str().map_or(value.clone(), |c| Value::String(hex(c))),
                );
            }
            "bold" => {
                delta.insert("weight".into(), json!(if truthy(value) { 700 } else { 400 }));
            }
            "italic" => {
                delta.insert("italic".into(), value.clone());
            }
            "underline" => {
                delta.insert(
                    "decoration".into(),
                    if truthy(value) { json!(["underline"]) } else { json!([]) },
                );
            }
            _ => {
                rest.insert(key.clone(), value.clone());
            }
        }
    }
    (delta, rest)
}

fn utf16_len(s: &str) -> f64 {
    s.encode_utf16().count() as f64
}

fn paragraph_len(p: &RichTextParagraph) -> f64 {
    p.runs.iter().map(|r| utf16_len(&r.text)).sum()
}

/// `runsInRange(doc, range)`: the style deltas (as JSON) of every non-empty
/// run a range overlaps.
pub fn runs_in_range(paragraphs: &[Value], range: RichTextRange) -> Vec<Value> {
    let typed = typed_paragraphs(paragraphs);
    let start = epdf_geometry::js::min(range.start, range.end);
    let end = epdf_geometry::js::max(range.start, range.end);
    if end <= start {
        return Vec::new();
    }
    let from: OffsetLocation = locate_offset(&typed, start);
    let to: OffsetLocation = locate_offset(&typed, end);
    let mut out = Vec::new();
    for index in from.paragraph..=to.paragraph {
        let Some(paragraph) = typed.get(index) else { break };
        let local_start = if index == from.paragraph { from.offset } else { 0.0 };
        let local_end = if index == to.paragraph { to.offset } else { paragraph_len(paragraph) };
        let raw_runs = paragraphs.get(index).and_then(|p| p.get("runs")).and_then(Value::as_array);
        let mut pos = 0.0;
        for (i, run) in paragraph.runs.iter().enumerate() {
            let len = utf16_len(&run.text);
            let run_end = pos + len;
            if run_end > local_start && pos < local_end && len > 0.0 {
                let style = raw_runs
                    .and_then(|r| r.get(i))
                    .and_then(|r| r.get("style"))
                    .filter(|s| !s.is_null() && !epdf_kernel::is_undefined(s))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                out.push(style);
            }
            pos = run_end;
        }
    }
    out
}

/// `rangeProps(doc, range, fonts)`: the values a range reads back for the
/// range keys, resolved against the body, `mixed` where the runs disagree.
pub fn range_props(
    doc: &RichTextDocument,
    range: RichTextRange,
    fonts: FontLookup,
) -> Result<(Object, Vec<String>), Error> {
    let body = &doc.body;
    let runs = runs_in_range(&doc.paragraphs, range);
    let runs = if runs.is_empty() { vec![json!({})] } else { runs };
    let pick = |delta: &Value, key: &str| -> Option<Value> {
        delta
            .get(key)
            .filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v))
            .or_else(|| body.get(key).filter(|v| !epdf_kernel::is_undefined(v)))
            .cloned()
    };
    let styles: Vec<Object> = runs
        .iter()
        .map(|delta| {
            let mut style = Map::new();
            for key in ["family", "weight", "italic", "size", "color", "decoration"] {
                if let Some(v) = pick(delta, key) {
                    style.insert(key.into(), v);
                }
            }
            style
        })
        .collect();
    let body_weight = body.get("weight").and_then(number);
    let body_italic = body.get("italic").and_then(Value::as_bool);
    let mut values = Map::new();
    let mut mixed = Vec::new();
    type Of<'a> = &'a dyn Fn(&Object) -> Result<Option<Value>, Error>;
    let mut read = |key: &str, of: Of| -> Result<(), Error> {
        let first = of(&styles[0])?;
        match &first {
            Some(v) => values.insert(key.into(), v.clone()),
            None => values.insert(key.into(), epdf_kernel::undefined()),
        };
        let first_text = crate::js::stringify(first.as_ref());
        for style in &styles {
            if crate::js::stringify(of(style)?.as_ref()) != first_text {
                mixed.push(key.to_string());
                break;
            }
        }
        Ok(())
    };
    read("fontFamily", &|s| {
        let family = s.get("family").and_then(Value::as_str).unwrap_or("undefined").to_string();
        Ok(Some(json!(font_for_face(
            &Face { family, weight: body_weight, italic: body_italic },
            fonts
        ))))
    })?;
    read("fontSize", &|s| Ok(s.get("size").cloned()))?;
    read("fontColor", &|s| match s.get("color") {
        Some(Value::String(color)) => Ok(Some(json!(color.to_lowercase()))),
        other => Err(Error::named(
            "TypeError",
            format!(
                "Cannot read properties of {} (reading 'toLowerCase')",
                if other.is_some_and(Value::is_null) { "null" } else { "undefined" }
            ),
        )),
    })?;
    read("bold", &|s| Ok(Some(json!(s.get("weight").and_then(number).unwrap_or(400.0) >= 600.0))))?;
    read("italic", &|s| Ok(Some(json!(s.get("italic").is_some_and(truthy)))))?;
    read("underline", &|s| {
        Ok(Some(json!(
            s.get("decoration")
                .and_then(Value::as_array)
                .is_some_and(|d| d.iter().any(|x| x.as_str() == Some("underline")))
        )))
    })?;
    Ok((values, mixed))
}

/// `plainTextOf(doc)`: paragraphs joined by `\r`, runs concatenated.
pub fn plain_text_of(paragraphs: &[Value]) -> String {
    paragraphs
        .iter()
        .map(|p| {
            p.get("runs")
                .and_then(Value::as_array)
                .map(|runs| runs.iter().filter_map(|r| r.get("text")?.as_str()).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\r")
}
