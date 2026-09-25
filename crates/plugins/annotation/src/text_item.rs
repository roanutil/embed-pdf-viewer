//! `text-item.ts`: the core's geometry-only `textBoxes` joined with the
//! DTO-derived font and CSS into render-ready [`TextItem`]s.

use epdf_annotation_core::{
    Geom, Model, PageObjectNumber, ViewEnv, initial_text_style, text_boxes,
};
use serde_json::json;

use crate::types::{TextItem, TextItemCss};
use crate::value::{get, truthy_num};

/// `cssFontFor(font)`: a standard PDF font to a web stack with the same
/// metrics; anything else is a registered font key.
pub fn css_font_for(font: &str) -> String {
    match font {
        "helvetica" | "helvetica-bold" => "Helvetica, Arial, sans-serif".into(),
        "courier" => "\"Courier New\", Courier, monospace".into(),
        "times-roman" => "\"Times New Roman\", Times, serif".into(),
        "symbol" | "zapf-dingbats" => "serif".into(),
        other => format!("\"{other}\", sans-serif"),
    }
}

/// `buildTextItems(m, pon, view)`.
pub fn build_text_items(m: &Model, pon: PageObjectNumber, view: Option<ViewEnv>) -> Vec<TextItem> {
    text_boxes(m, pon, view)
        .into_iter()
        .map(|tb| {
            let a = m.by_id.get(&tb.id);
            let t = a.and_then(|a| a.text.clone()).unwrap_or_else(initial_text_style);
            let sw = a.map_or(0.0, |a| a.style.stroke_width);
            let is_callout =
                a.is_some_and(|a| matches!(a.geom, Geom::Text { callout: Some(_), .. }));
            let contents = a
                .and_then(|a| a.data.as_ref())
                .and_then(|d| get(d, "contents"))
                .filter(|c| !c.is_null())
                .cloned()
                .unwrap_or_else(|| json!(""));
            TextItem {
                id: tb.id.clone(),
                ref_: a.and_then(|a| a.ref_.clone()),
                box_: tb.box_,
                contents,
                editing: tb.editing,
                rot: tb.rot.filter(|r| truthy_num(*r)),
                css: TextItemCss {
                    font_family: css_font_for(&t.font_family),
                    font_size: t.font_size,
                    line_height: t.font_size,
                    color: t.font_color,
                    align: t.text_align,
                    padding: if is_callout { sw + 2.0 } else { sw },
                    background: a.and_then(|a| a.style.interior_color.clone()),
                },
            }
        })
        .collect()
}
