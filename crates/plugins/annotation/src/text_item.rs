//! `text-item.ts`: the free-text presentation projection, the core's
//! geometry-only `textBoxes` joined with the font and CSS into render-ready
//! `TextItem`s.

use epdf_annotation_core::font_css::standard_paint_font_of;
use epdf_annotation_core::object::Json;
use epdf_annotation_core::props::initial_text_style;
use epdf_annotation_core::record::{style_of, text_of};
use epdf_annotation_core::shapes::text_box::text_plate_inset;
use epdf_annotation_core::types::{Model, PageRef, ViewEnv};
use epdf_annotation_core::view::text_boxes;
use serde_json::{Map, Value, json};

use crate::js::num;
use crate::rich_text::{rich_doc_of, strip_body_defaults};

/// `buildTextItems(model, page, view)`.
pub fn build_text_items(model: &Model, page: &PageRef, view: Option<&ViewEnv>) -> Vec<Value> {
    text_boxes(model, page, view)
        .into_iter()
        .map(|tb| {
            let record = model.by_id.get(&tb.id);
            let style = record.and_then(|r| text_of(&r.annotation)).unwrap_or_else(initial_text_style);
            let sw = record.map_or(0.0, |r| style_of(&r.annotation).stroke_width);
            let doc = record.map(|r| rich_doc_of(&r.annotation, None));
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
            // Without the fonts plugin, the box draws in the standard font the
            // engine falls back to for its font (`standardFontCss`).
            let family = style.font_family.as_ref().and_then(Value::as_str).unwrap_or("undefined");
            let body_css = standard_paint_font_of(family);
            item.insert("programs".into(), json!([body_css.program.0.members()]));
            item.insert("editing".into(), json!(tb.editing));
            if tb.rot != 0.0 && !tb.rot.is_nan() {
                item.insert("rot".into(), num(tb.rot));
            }
            item.insert(
                "css".into(),
                json!({
                    "fontFamily": body_css.font_family,
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
