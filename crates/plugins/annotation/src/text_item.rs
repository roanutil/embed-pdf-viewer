//! `text-item.ts`: the free-text presentation projection, the core's
//! geometry-only `textBoxes` joined with the font and CSS into render-ready
//! `TextItem`s; and the part of `read/text-fonts.ts` that answers without
//! the fonts plugin, which this port has no counterpart of.

use epdf_annotation_core::font_css::standard_paint_font_of;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::props::initial_text_style;
use epdf_annotation_core::record::{style_of, text_of};
use epdf_annotation_core::shapes::text_box::text_plate_inset;
use epdf_annotation_core::types::{Model, PageRef, ViewEnv};
use epdf_annotation_core::view::text_boxes;
use serde_json::{Map, Value, json};

use crate::js::num;
use crate::rich_text::{
    Face, FontLookup, face_for_font, rich_doc_of, standard_font_for_face, strip_body_defaults,
};

/// `fonts` of `buildTextItems`: the plugin's font lookup, with its text
/// fonts (`createTextFonts`) as they answer when no fonts plugin is there.
#[derive(Clone, Copy)]
pub struct TextItemFonts<'a> {
    pub lookup: FontLookup<'a>,
}

/// A face's standard CSS (`standardFontCss(font)`): its family list and
/// programs, each program's `load` a function.
fn standard_css(font: &str) -> (String, Vec<Value>) {
    let css = standard_paint_font_of(font);
    (css.font_family, vec![Value::Object(css.program.0.members())])
}

/// `bodyIsItsFont(style, body)`: the body's face is the one its font names
/// alone, so the box's font is the one drawn.
fn body_is_its_font(font: &str, body: &Face, lookup: FontLookup) -> bool {
    let face = face_for_font(font, lookup);
    face.weight.unwrap_or(400.0) == body.weight.unwrap_or(400.0)
        && face.italic.unwrap_or(false) == body.italic.unwrap_or(false)
}

/// `textFonts.bodyCss(id, style, body)` without the fonts plugin: the
/// standard font the engine falls back to for the box's font, or for its
/// body's face when that isn't the font's.
fn body_css(font: &str, body: &Face, lookup: FontLookup) -> (String, Vec<Value>) {
    if body_is_its_font(font, body, lookup) {
        standard_css(font)
    } else {
        standard_css(&standard_font_for_face(body))
    }
}

/// A run style's member that is there (`own?.x !== undefined`).
fn defined(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !epdf_kernel::is_undefined(v))
}

/// `programsOf(id, doc, body, bodyPrograms, text)`: the body's programs,
/// then those of each face a run names (`textFonts.faceCss`, the standard
/// font for the face without the fonts plugin), each program once.
fn programs_of(paragraphs: &[Value], body: &Face, body_programs: Vec<Value>) -> Vec<Value> {
    let mut programs = body_programs;
    for paragraph in paragraphs {
        let runs = paragraph.get("runs").and_then(Value::as_array).cloned().unwrap_or_default();
        for run in runs {
            let own = run.get("style");
            let family = defined(own.and_then(|s| s.get("family")));
            let weight = defined(own.and_then(|s| s.get("weight")));
            let italic = defined(own.and_then(|s| s.get("italic")));
            if family.is_none() && weight.is_none() && italic.is_none() {
                continue;
            }
            let face = Face {
                family: family
                    .and_then(Value::as_str)
                    .map_or_else(|| body.family.clone(), str::to_string),
                weight: weight.map_or(body.weight, Value::as_f64),
                italic: italic.map_or(body.italic, Value::as_bool),
            };
            for program in standard_css(&standard_font_for_face(&face)).1 {
                let hash = program.get("hash");
                if !programs.iter().any(|known| known.get("hash") == hash) {
                    programs.push(program);
                }
            }
        }
    }
    programs
}

/// `buildTextItems(model, page, view, fonts)`.
pub fn build_text_items(
    model: &Model,
    page: &PageRef,
    view: Option<&ViewEnv>,
    fonts: Option<TextItemFonts>,
) -> Vec<Value> {
    text_boxes(model, page, view)
        .into_iter()
        .map(|tb| {
            let record = model.by_id.get(&tb.id);
            let style = record.and_then(|r| text_of(&r.annotation)).unwrap_or_else(initial_text_style);
            let sw = record.map_or(0.0, |r| style_of(&r.annotation).stroke_width);
            let doc = record.map(|r| rich_doc_of(&r.annotation, fonts.and_then(|f| f.lookup)));
            let paragraphs = match &doc {
                Some(doc) => Value::Array(strip_body_defaults(&doc.paragraphs, &doc.body)),
                None => json!([{ "runs": [{ "text": "" }] }]),
            };
            let mut item = Map::new();
            item.insert("id".into(), json!(tb.id));
            item.insert(
                "ref".into(),
                record.and_then(|r| r.annotation.get("ref").cloned()).unwrap_or(Value::Null),
            );
            item.insert("box".into(), tb.rect.json());
            let contents = record
                .and_then(|r| r.annotation.get("contents"))
                .filter(|c| !c.is_null() && !epdf_kernel::is_undefined(c))
                .cloned()
                .unwrap_or_else(|| json!(""));
            item.insert("contents".into(), contents);
            item.insert("richText".into(), json!({ "paragraphs": paragraphs }));
            let face = match &doc {
                Some(doc) => json!({
                    "family": doc.body.get("family").cloned().unwrap_or(Value::Null),
                    "weight": doc.body.get("weight").cloned().unwrap_or(Value::Null),
                    "italic": doc.body.get("italic").cloned().unwrap_or(Value::Null),
                }),
                None => json!({ "family": "Helvetica", "weight": 400, "italic": false }),
            };
            item.insert("face".into(), face);
            // The box's font's standard CSS (`standardFontCss`), or with the
            // plugin's fonts what its text fonts answer.
            let family = style.font_family.as_ref().and_then(Value::as_str).unwrap_or("undefined");
            let (font_family, programs) = match (fonts, &doc) {
                (Some(fonts), Some(doc)) => {
                    let body = Face::read(&item["face"]);
                    let (font_family, programs) = body_css(family, &body, fonts.lookup);
                    (font_family, programs_of(&doc.paragraphs, &body, programs))
                }
                _ => standard_css(family),
            };
            item.insert("programs".into(), Value::Array(programs));
            item.insert("editing".into(), json!(tb.editing));
            if tb.rot != 0.0 && !tb.rot.is_nan() {
                item.insert("rot".into(), num(tb.rot));
            }
            item.insert(
                "css".into(),
                json!({
                    "fontFamily": font_family,
                    "fontSize": style.font_size.clone().unwrap_or_else(epdf_kernel::undefined),
                    "color": style.font_color.clone().unwrap_or_else(epdf_kernel::undefined),
                    "fontWeight": if style.bold == Some(true) { 700 } else { 400 },
                    "fontStyle": if style.italic == Some(true) { "italic" } else { "normal" },
                    "textDecoration": if style.underline == Some(true) { "underline" } else { "none" },
                    "align": style.text_align.clone().unwrap_or_else(epdf_kernel::undefined),
                    "padding": num(text_plate_inset(sw)),
                }),
            );
            Value::Object(item)
        })
        .collect()
}
