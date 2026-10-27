//! `update/text.ts`: free-text typing. The editor's plain or rich result
//! becomes the record's text at once, and a `text` effect asks for the write.

use std::rc::Rc;

use serde_json::Value;

use crate::json::{self, Object};
use crate::richtext::{
    RichTextDocumentInput, RichTextParagraph, normalize_doc, paragraphs_from_plain_text,
    plain_text_of,
};
use crate::types::{Effect, Model};
use crate::update::changes::with_record;

fn paragraphs_value(paragraphs: &[RichTextParagraph]) -> Value {
    serde_json::to_value(paragraphs).unwrap_or(Value::Array(Vec::new()))
}

/// `setText(model, id, text)`.
pub fn set_text(model: &Model, id: &str, text: &str) -> (Model, Vec<Effect>) {
    let Some(record) = model.record(id) else { return (model.clone(), Vec::new()) };
    let paragraphs = paragraphs_from_plain_text(text);
    let mut annotation: Object = (*record.annotation).clone();
    if record.subtype() == "free-text" {
        annotation.insert("contents".into(), Value::String(plain_text_of(&paragraphs)));
        let mut rich = json::get(&record.annotation, "richText")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        rich.insert("paragraphs".into(), paragraphs_value(&paragraphs));
        annotation.insert("richText".into(), Value::Object(rich));
    } else {
        annotation.insert("contents".into(), Value::String(text.into()));
    }
    (with_record(model, Rc::new(record.with_annotation(annotation))), vec![Effect::text(id)])
}

/// `setRichText(model, id, doc)`.
pub fn set_rich_text(model: &Model, id: &str, doc: &Object) -> (Model, Vec<Effect>) {
    let Some(record) = model.record(id) else { return (model.clone(), Vec::new()) };
    if record.subtype() != "free-text" {
        return (model.clone(), Vec::new());
    }
    let Ok(input) = serde_json::from_value::<RichTextDocumentInput>(Value::Object(doc.clone()))
    else {
        return (model.clone(), Vec::new());
    };
    let normalized = normalize_doc(&input);
    let mut body = record
        .annotation
        .get("richText")
        .and_then(|r| r.get("body"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(over) = normalized.body.as_ref() {
        for (k, v) in over {
            body.insert(k.clone(), v.clone());
        }
    }
    let rich = json::object([
        ("body", Value::Object(body)),
        ("paragraphs", paragraphs_value(&normalized.paragraphs)),
    ]);
    let mut annotation: Object = (*record.annotation).clone();
    annotation.insert("richText".into(), Value::Object(rich));
    annotation.insert("contents".into(), Value::String(plain_text_of(&normalized.paragraphs)));
    (with_record(model, Rc::new(record.with_annotation(annotation))), vec![Effect::text(id)])
}
