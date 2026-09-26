//! `text-item.ts`: the core's geometry-only `textBoxes` joined with the
//! DTO-derived font, the rich paragraphs and the CSS into render-ready
//! [`TextItem`]s.

use epdf_annotation_core::richtext::{RichTextParagraph, RichTextRun};
use epdf_annotation_core::{Model, PageRef, ViewEnv, initial_text_style, text_boxes, text_plate_inset};
use serde_json::json;

use crate::contract::{RichTextParagraphs, TextItem, TextItemCss};
use crate::rich_text::{css_font_family_for_font, rich_doc_of, strip_body_defaults};
use crate::value::{get, truthy_num};

/// `buildTextItems(m, page, view)`.
pub fn build_text_items(m: &Model, page: PageRef, view: Option<ViewEnv>) -> Vec<TextItem> {
    text_boxes(m, page, view)
        .into_iter()
        .map(|tb| {
            let a = m.by_id.get(&tb.id);
            let t = a.and_then(|a| a.text.clone()).unwrap_or_else(initial_text_style);
            let sw = a.map_or(0.0, |a| a.style.stroke_width);
            let contents = a
                .and_then(|a| a.data.as_ref())
                .and_then(|d| get(d, "contents"))
                .filter(|c| !c.is_null())
                .cloned()
                .unwrap_or_else(|| json!(""));
            let paragraphs = match a {
                Some(a) => {
                    let doc = rich_doc_of(a, None);
                    strip_body_defaults(&doc.paragraphs, &doc.body)
                }
                None => vec![RichTextParagraph {
                    runs: vec![RichTextRun { text: String::new(), style: None }],
                    props: Default::default(),
                }],
            };
            let on = |f: Option<bool>| f == Some(true);
            TextItem {
                id: tb.id.clone(),
                ref_: a.and_then(|a| a.ref_.clone()),
                box_: tb.box_,
                contents,
                rich_text: RichTextParagraphs { paragraphs },
                editing: tb.editing,
                rot: tb.rot.filter(|r| truthy_num(*r)),
                css: TextItemCss {
                    font_family: css_font_family_for_font(&t.font_family),
                    font_size: t.font_size,
                    color: t.font_color,
                    font_weight: if on(t.bold) { 700.0 } else { 400.0 },
                    font_style: if on(t.italic) { "italic" } else { "normal" },
                    text_decoration: if on(t.underline) { "underline" } else { "none" },
                    align: t.text_align,
                    padding: text_plate_inset(sw),
                },
            }
        })
        .collect()
}
