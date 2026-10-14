//! The rich text of a free text, and the wire between it and the fork.
//!
//! Ports `core/src/dto/RichText.ts` (the DTO) and
//! `services/src/features/annotations/internal/richTextWire.ts` (the wire).
//!
//! The fork's `EPDFAnnot_GetRichTextJSON` / `EPDFAnnot_SetRichTextJSON` JSON
//! is the DTO shape plus `source` and `diagnostics`, so reading is a parse and
//! writing is a stringify, except for FACES. The DTO names a face the way the
//! host does (a registered font's key, a standard font's kebab name); the fork
//! names it the way a PDF does (family, weight, italic). Both directions map
//! here.
//!
//! Only what the engine itself reasons about is typed: the body's face, size,
//! colour and alignment. Paragraph properties and run deltas are carried as
//! JSON objects, verbatim, because the engine only ever passes them through
//! and a typed copy would drop whatever the fork adds next.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::vocabulary::{StandardFont, TextAlignment};

/// `RichTextBody`: the complete body style and paragraph defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichTextBody {
    pub family: String,
    /// 100..900.
    pub weight: u32,
    pub italic: bool,
    /// Points.
    pub size: f64,
    /// `#RRGGBB`.
    pub color: String,
    pub decoration: Vec<String>,
    pub script: String,
    pub letter_spacing: f64,
    pub horizontal_scale: f64,
    /// `left`, `center`, `right` or `justify`.
    pub align: String,
    /// `ltr` or `rtl`.
    pub dir: String,
    /// `lineHeight`, `margins`, `textIndent`, `unknown`: whatever else the
    /// fork reports, kept as it came.
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

/// `RichTextRun`. The style holds only what the run overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichTextRun {
    /// May contain `\r`: a hard line break inside the paragraph.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Map<String, Value>>,
}

/// `RichTextParagraph`: its runs, and the paragraph properties it overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichTextParagraph {
    pub runs: Vec<RichTextRun>,
    #[serde(flatten)]
    pub props: Map<String, Value>,
}

/// `RichTextDocument`: what an annotation reads back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichTextDocument {
    pub body: RichTextBody,
    pub paragraphs: Vec<RichTextParagraph>,
}

/// `RichTextDocumentInput`: what a draft or patch writes. An absent body
/// keeps the annotation's current body style; a partial one takes the
/// fork's defaults for what it leaves out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichTextDocumentInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Map<String, Value>>,
    pub paragraphs: Vec<RichTextParagraph>,
}

impl RichTextBody {
    /// The body alignment as a `/Q` value. `None` for `justify`, which `/Q`
    /// cannot say, and for anything unrecognized.
    pub fn text_alignment(&self) -> Option<TextAlignment> {
        match self.align.as_str() {
            "left" => Some(TextAlignment::Left),
            "center" => Some(TextAlignment::Center),
            "right" => Some(TextAlignment::Right),
            _ => None,
        }
    }
}

/// `richTextPlainText`: paragraphs joined by `\r`, runs concatenated. What the
/// fork writes to `/Contents`.
pub fn plain_text(paragraphs: &[RichTextParagraph]) -> String {
    paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\r")
}

/// `richTextParagraphsFromPlainText`: one paragraph per line break, one run
/// each, no overrides.
pub fn paragraphs_from_plain_text(text: &str) -> Vec<RichTextParagraph> {
    let mut lines = Vec::new();
    let mut rest = text;
    loop {
        match rest.find(['\r', '\n']) {
            Some(at) => {
                lines.push(&rest[..at]);
                let skip = if rest[at..].starts_with("\r\n") { 2 } else { 1 };
                rest = &rest[at + skip..];
            }
            None => {
                lines.push(rest);
                break;
            }
        }
    }
    lines
        .into_iter()
        .map(|line| RichTextParagraph {
            runs: vec![RichTextRun { text: line.to_owned(), style: None }],
            props: Map::new(),
        })
        .collect()
}

/// A face as the fork names one. Weight and italic are absent when the name
/// alone does not imply them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceRequest {
    pub family: String,
    pub weight: Option<u32>,
    pub italic: Option<bool>,
}

/// What a registered font key resolves to.
pub type DescribeFont<'a> = &'a dyn Fn(&str) -> Option<pdfium::FontIdentity>;
/// The registered key whose identity matches a face.
pub type KeyForFace<'a> = &'a dyn Fn(&str, u32, bool) -> Option<String>;

/// `STANDARD_FACES`: the family, weight and italic of each standard name.
fn standard_face(font: StandardFont) -> (&'static str, u32, bool) {
    use StandardFont as S;
    match font {
        S::Courier => ("Courier", 400, false),
        S::CourierBold => ("Courier", 700, false),
        S::CourierBoldOblique => ("Courier", 700, true),
        S::CourierOblique => ("Courier", 400, true),
        S::Helvetica => ("Helvetica", 400, false),
        S::HelveticaBold => ("Helvetica", 700, false),
        S::HelveticaBoldOblique => ("Helvetica", 700, true),
        S::HelveticaOblique => ("Helvetica", 400, true),
        S::TimesRoman => ("Times", 400, false),
        S::TimesBold => ("Times", 700, false),
        S::TimesBoldItalic => ("Times", 700, true),
        S::TimesItalic => ("Times", 400, true),
        S::Symbol => ("Symbol", 400, false),
        S::ZapfDingbats => ("ZapfDingbats", 400, false),
    }
}

/// `familyKey`: families compare without case, spaces, hyphens, underscores
/// and quotes, which is the fork's own rule.
pub fn family_key(family: &str) -> String {
    family
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '_' | '\'' | '"'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// `STANDARD_FAMILY_KEYS`: the family names that mean a standard family.
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

/// `faceForFreeTextFont`: the face a `fontFamily` (or a run's `family`) asks
/// for. A standard name's family, weight and italic; a registered key's
/// resolved identity; or, for anything else, the string itself as a family.
pub fn face_for_free_text_font(font: &str, describe: Option<DescribeFont<'_>>) -> FaceRequest {
    if let Some(standard) = StandardFont::from_family(font) {
        let (family, weight, italic) = standard_face(standard);
        return FaceRequest {
            family: family.to_owned(),
            weight: Some(weight),
            italic: Some(italic),
        };
    }
    if let Some(identity) = describe.and_then(|describe| describe(font)) {
        return FaceRequest {
            family: identity.family_name,
            weight: Some(u32::try_from(identity.weight).unwrap_or(400)),
            italic: Some(identity.italic),
        };
    }
    FaceRequest { family: font.to_owned(), weight: None, italic: None }
}

/// `freeTextFontForFace`: the DTO `fontFamily` for a body face. The
/// registered key whose identity matches, else the standard name, else the
/// family itself.
pub fn free_text_font_for_face(
    family: &str,
    weight: u32,
    italic: bool,
    key_for_face: Option<KeyForFace<'_>>,
) -> String {
    if let Some(key) = key_for_face.and_then(|lookup| lookup(family, weight, italic)) {
        return key;
    }
    if let Some(standard) = standard_family(&family_key(family)) {
        let bold = weight >= 600;
        let same_family =
            || StandardFont::ALL.into_iter().filter(move |f| standard_face(*f).0 == standard);
        if let Some(font) = same_family().find(|f| {
            let (_, w, i) = standard_face(*f);
            (w >= 600) == bold && i == italic
        }) {
            return font.as_str().to_owned();
        }
        // Symbol and ZapfDingbats have no bold or italic faces.
        if let Some(font) = same_family().next() {
            return font.as_str().to_owned();
        }
    }
    family.to_owned()
}

/// `withEngineFace`: a style's `family` rewritten to what the fork resolves
/// by, with the weight and italic a key or standard name implies. An explicit
/// `weight` or `italic` in the style still wins.
fn with_engine_face(
    style: &Map<String, Value>,
    describe: Option<DescribeFont<'_>>,
) -> Map<String, Value> {
    let Some(family) = style.get("family").and_then(Value::as_str) else {
        return style.clone();
    };
    let face = face_for_free_text_font(family, describe);
    let mut out = style.clone();
    out.insert("family".into(), Value::String(face.family));
    if let Some(weight) = face.weight
        && !style.contains_key("weight")
    {
        out.insert("weight".into(), Value::from(weight));
    }
    if let Some(italic) = face.italic
        && !style.contains_key("italic")
    {
        out.insert("italic".into(), Value::Bool(italic));
    }
    out
}

/// `engineRichTextJson`: the fork's JSON for a write, faces resolved.
pub fn engine_rich_text_json(
    input: &RichTextDocumentInput,
    describe: Option<DescribeFont<'_>>,
) -> String {
    let paragraphs: Vec<RichTextParagraph> = input
        .paragraphs
        .iter()
        .map(|p| RichTextParagraph {
            runs: p
                .runs
                .iter()
                .map(|r| RichTextRun {
                    text: r.text.clone(),
                    style: r.style.as_ref().map(|s| with_engine_face(s, describe)),
                })
                .collect(),
            props: p.props.clone(),
        })
        .collect();
    let document = RichTextDocumentInput {
        body: input.body.as_ref().map(|b| with_engine_face(b, describe)),
        paragraphs,
    };
    serde_json::to_string(&document).expect("a rich text document serializes")
}

/// `parseEngineRichText`: the fork's JSON in the DTO shape, its `source` and
/// `diagnostics` dropped. `None` when it is not what the fork writes.
pub fn parse_engine_rich_text(json: &str) -> Option<RichTextDocument> {
    let mut raw: Map<String, Value> = serde_json::from_str(json).ok()?;
    let mut body = match raw.remove("body") {
        Some(Value::Object(body)) => body,
        _ => return None,
    };
    let paragraphs = match raw.remove("paragraphs") {
        Some(paragraphs @ Value::Array(_)) => paragraphs,
        _ => return None,
    };
    body.remove("source");
    Some(RichTextDocument {
        body: serde_json::from_value(Value::Object(body)).ok()?,
        paragraphs: serde_json::from_value(paragraphs).ok()?,
    })
}

/// `readEngineRichText`.
pub fn read_rich_text(annotation: &pdfium::Annotation<'_>) -> Option<RichTextDocument> {
    annotation.rich_text_json().ok().flatten().as_deref().and_then(parse_engine_rich_text)
}

/// `writeRichText`: `/RC`, `/DS`, `/DA`, `/Contents` and the appearance
/// through the fork's rich writer, all or nothing.
pub fn write_rich_text(
    annotation: &pdfium::Annotation<'_>,
    input: &RichTextDocumentInput,
    describe: Option<DescribeFont<'_>>,
) -> Result<(), EngineError> {
    let json = engine_rich_text_json(input, describe);
    let ok = annotation.set_rich_text_json(&json).map_err(|_| {
        EngineError::with_message(EngineErrorCode::InvalidArg, "rich text JSON contains NUL")
    })?;
    if !ok {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "rich text could not be written (empty rect, or a font resource could not be built)",
        ));
    }
    Ok(())
}

/// `assertRichTextAgreement`: a draft or patch carrying both `contents` and
/// `richText` must agree, `contents` being the plain projection. A stale DTO
/// echo fails loud here instead of quietly restoring old text.
pub fn assert_agreement(
    contents: Option<&str>,
    rich_text: Option<&RichTextDocumentInput>,
) -> Result<(), EngineError> {
    let (Some(contents), Some(rich_text)) = (contents, rich_text) else {
        return Ok(());
    };
    if contents != plain_text(&rich_text.paragraphs) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "free-text: `contents` must equal the plain projection of `richText` when both are given",
        ));
    }
    Ok(())
}

/// `hexColor`: `#RRGGBB`, upper case, for a rich body colour.
pub fn hex_color(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standard_body_face_reads_back_as_its_kebab_name() {
        assert_eq!(free_text_font_for_face("Times", 700, false, None), "times-bold");
        assert_eq!(free_text_font_for_face("Helvetica", 400, true, None), "helvetica-oblique");
        assert_eq!(free_text_font_for_face("Arial MT", 400, false, None), "helvetica");
        assert_eq!(free_text_font_for_face("Symbol", 700, true, None), "symbol");
    }

    #[test]
    fn an_unresolvable_body_face_reads_back_as_itself_even_when_empty() {
        // a-bare-text-box in vectors/annotation/read.json: a /DA naming no
        // font the fork can resolve gives an empty body family.
        assert_eq!(free_text_font_for_face("", 400, false, None), "");
        assert_eq!(free_text_font_for_face("Noto Sans", 400, false, None), "Noto Sans");
    }

    #[test]
    fn a_registered_key_wins_over_the_standard_mapping() {
        let lookup = |family: &str, _: u32, _: bool| {
            (family_key(family) == "helvetica").then(|| "brand".to_owned())
        };
        assert_eq!(free_text_font_for_face("Helvetica", 400, false, Some(&lookup)), "brand");
    }

    #[test]
    fn a_standard_name_carries_its_face_into_the_engine_json() {
        let input = RichTextDocumentInput {
            body: Some(serde_json::from_str(r#"{"family":"courier-bold","italic":true}"#).unwrap()),
            paragraphs: paragraphs_from_plain_text("a\r\nb\nc"),
        };
        let json: Value = serde_json::from_str(&engine_rich_text_json(&input, None)).unwrap();
        assert_eq!(
            json["body"],
            serde_json::json!({"family":"Courier","weight":700,"italic":true})
        );
        assert_eq!(json["paragraphs"].as_array().unwrap().len(), 3);
        assert_eq!(json["paragraphs"][1], serde_json::json!({"runs":[{"text":"b"}]}));
    }

    #[test]
    fn the_engine_json_parses_without_its_source_and_diagnostics() {
        let json = r##"{"source":"contents","body":{"family":"","weight":400,"italic":false,
            "size":12,"color":"#000000","decoration":[],"script":"normal","letterSpacing":0,
            "horizontalScale":1,"align":"left","dir":"ltr"},"paragraphs":[{"runs":[]}],
            "diagnostics":[]}"##;
        let doc = parse_engine_rich_text(json).expect("parses");
        let back = serde_json::to_value(&doc).unwrap();
        assert!(back.get("source").is_none() && back.get("diagnostics").is_none());
        assert_eq!(back["paragraphs"], serde_json::json!([{"runs":[]}]));
        assert_eq!(doc.body.family, "");
        assert!(parse_engine_rich_text(r#"{"paragraphs":[]}"#).is_none());
    }

    #[test]
    fn contents_must_be_the_plain_projection() {
        let rich =
            RichTextDocumentInput { body: None, paragraphs: paragraphs_from_plain_text("a\nb") };
        assert!(assert_agreement(Some("a\rb"), Some(&rich)).is_ok());
        assert!(assert_agreement(Some("a\nb"), Some(&rich)).is_err());
        assert!(assert_agreement(None, Some(&rich)).is_ok());
    }

    #[test]
    fn body_colours_are_upper_case_hex() {
        assert_eq!(hex_color(200, 0, 10), "#C8000A");
    }
}
