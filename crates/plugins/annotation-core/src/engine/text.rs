//! Text the predictor reads and writes: font faces (`annotation/fontFaces.ts`),
//! the rich text DTO helpers (`dto/RichText.ts`), a note's standard review
//! states (`annotation/comments.ts`) and a link's action tree
//! (`dto/PdfLinkTarget.ts`).

use serde_json::Value;

use super::facts::STANDARD_FONTS;
use crate::json::{self, Object, num};

/// A standard font's face (`STANDARD_FACES`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StandardFace {
    pub family: &'static str,
    pub weight: f64,
    pub italic: bool,
}

/// `STANDARD_FACES[name]`.
pub fn standard_face(name: &str) -> Option<StandardFace> {
    let (family, weight, italic) = match name {
        "courier" => ("Courier", 400.0, false),
        "courier-bold" => ("Courier", 700.0, false),
        "courier-bold-oblique" => ("Courier", 700.0, true),
        "courier-oblique" => ("Courier", 400.0, true),
        "helvetica" => ("Helvetica", 400.0, false),
        "helvetica-bold" => ("Helvetica", 700.0, false),
        "helvetica-bold-oblique" => ("Helvetica", 700.0, true),
        "helvetica-oblique" => ("Helvetica", 400.0, true),
        "times-roman" => ("Times", 400.0, false),
        "times-bold" => ("Times", 700.0, false),
        "times-bold-italic" => ("Times", 700.0, true),
        "times-italic" => ("Times", 400.0, true),
        "symbol" => ("Symbol", 400.0, false),
        "zapf-dingbats" => ("ZapfDingbats", 400.0, false),
        _ => return None,
    };
    Some(StandardFace { family, weight, italic })
}

/// `isStandardFontName(font)`.
pub fn is_standard_font_name(font: &str) -> bool {
    STANDARD_FONTS.contains(&font)
}

/// The face a font name asks for (`FaceRequest`).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceRequest {
    pub family: String,
    pub weight: Option<f64>,
    pub italic: Option<bool>,
}

/// A registered font's identity, as `engine.fonts` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct FontIdentity {
    pub family_name: String,
    pub weight: f64,
    pub italic: bool,
}

/// Looks up a registered font by its key (`DescribeFont`).
pub type DescribeFont<'a> = &'a dyn Fn(&str) -> Option<FontIdentity>;

/// `faceForFreeTextFont(font, describe)`.
pub fn face_for_free_text_font(font: &str, describe: Option<DescribeFont>) -> FaceRequest {
    if let Some(face) = standard_face(font) {
        return FaceRequest {
            family: face.family.into(),
            weight: Some(face.weight),
            italic: Some(face.italic),
        };
    }
    if let Some(registered) = describe.and_then(|d| d(font)) {
        return FaceRequest {
            family: registered.family_name,
            weight: Some(registered.weight),
            italic: Some(registered.italic),
        };
    }
    FaceRequest { family: font.into(), weight: None, italic: None }
}

/// `DEFAULT_RICH_TEXT_BODY`.
pub fn default_rich_text_body() -> Object {
    json::object([
        ("family", Value::String("Helvetica".into())),
        ("weight", num(400.0)),
        ("italic", Value::Bool(false)),
        ("size", num(12.0)),
        ("color", Value::String("#000000".into())),
        ("decoration", Value::Array(Vec::new())),
        ("script", Value::String("normal".into())),
        ("letterSpacing", num(0.0)),
        ("horizontalScale", num(1.0)),
        ("align", Value::String("left".into())),
        ("dir", Value::String("ltr".into())),
    ])
}

/// `richTextPlainText(doc)`: runs joined, paragraphs joined by `\r`.
pub fn rich_text_plain_text(paragraphs: &[Value]) -> String {
    paragraphs
        .iter()
        .map(|p| {
            p.get("runs")
                .and_then(Value::as_array)
                .map(|runs| {
                    runs.iter()
                        .map(|r| r.get("text").and_then(Value::as_str).unwrap_or(""))
                        .collect::<String>()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\r")
}

/// `text.split(/\r\n|\r|\n/)`.
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                out.push(&text[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            b'\n' => {
                out.push(&text[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push(&text[start..]);
    out
}

/// `richTextParagraphsFromPlainText(text)`: one paragraph per line, one run each.
pub fn rich_text_paragraphs_from_plain_text(text: &str) -> Vec<Value> {
    split_lines(text)
        .into_iter()
        .map(|line| {
            let run = json::object([("text", Value::String(line.into()))]);
            Value::Object(json::object([("runs", Value::Array(vec![Value::Object(run)]))]))
        })
        .collect()
}

/// `standardStateModelOf(state)`.
pub fn standard_state_model_of(state: &str) -> Option<&'static str> {
    match state {
        "accepted" | "rejected" | "cancelled" | "completed" | "none" => Some("review"),
        "marked" | "unmarked" => Some("marked"),
        _ => None,
    }
}

/// `actionTreeOf(target)`: a writable link target as the `/A` tree a read gives.
pub fn action_tree_of(target: &Object) -> Value {
    let kind = json::get_str(target, "kind").unwrap_or("");
    let root = match kind {
        "uri" => json::object([
            ("type", Value::String("uri".into())),
            ("subtype", Value::String("URI".into())),
            ("uri", target.get("uri").cloned().unwrap_or(Value::Null)),
            ("isMap", Value::Bool(false)),
            ("next", Value::Array(Vec::new())),
        ]),
        "goto" => json::object([
            ("type", Value::String("goto".into())),
            ("subtype", Value::String("GoTo".into())),
            ("destination", target.get("destination").cloned().unwrap_or(Value::Null)),
            ("next", Value::Array(Vec::new())),
        ]),
        _ => json::object([
            ("type", Value::String("named".into())),
            ("subtype", Value::String("Named".into())),
            ("name", target.get("name").cloned().unwrap_or(Value::Null)),
            ("next", Value::Array(Vec::new())),
        ]),
    };
    Value::Object(json::object([
        ("root", Value::Object(root)),
        ("incomplete", Value::Bool(false)),
        ("warningFlags", num(0.0)),
        ("warnings", Value::Array(Vec::new())),
    ]))
}
