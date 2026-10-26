//! `write/marks.ts`: authoring. A drawn or typed mark rendered to a
//! one-page PDF (the annotation is authored on a scratch page and exported
//! the way "stamp from selection" exports), a page of a PDF as a
//! single-page PDF, and the bytes an asset is made from.

use epdf_kernel::error::code;
use epdf_kernel::js::number_to_string;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, undefined};
use epdf_plugin_annotation::binary::{WireResource, resolve_binary_source};
use serde_json::{Value, json};

use crate::blank_library::blank_library_pdf;
use crate::contract::{AddAssetInput, MarkSource};
use crate::controller::StampCapability;
use crate::js::{adopt, member, nullish};
use crate::services::asset_engine::{bytes_of, call, close, page_call, read_off};
use crate::services::errors::stamp_error;

/// `hexColor(hex)`: a mark's `#rrggbb` color, checked before anything is
/// drawn.
fn hex_color(hex: &str) -> Result<String, Error> {
    let color = hex.trim();
    let valid = color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if !valid {
        return Err(stamp_error(
            code::INVALID_INPUT,
            format!("mark color must be #rrggbb, got '{hex}'"),
        ));
    }
    Ok(color.to_string())
}

/// `inkListOf(strokes)`: an ink mark's strokes, as the engine takes them; a
/// mark needs at least one point.
fn ink_list_of(strokes: &[Vec<(f64, f64)>]) -> Result<Value, Error> {
    if !strokes.iter().any(|stroke| !stroke.is_empty()) {
        return Err(stamp_error(code::INVALID_INPUT, "an ink mark needs at least one point"));
    }
    Ok(Value::Array(
        strokes
            .iter()
            .map(|stroke| {
                Value::Array(stroke.iter().map(|(x, y)| json!({ "x": x, "y": y })).collect())
            })
            .collect(),
    ))
}

/// `textBounds(text, fontSize)`: a generous box for typed text. The export
/// keeps the whole box, so the text is centered in it.
fn text_bounds(text: &str, font_size: f64) -> Value {
    let length = text.encode_utf16().count() as f64;
    json!({
        "x": 0,
        "y": 0,
        "width": length.max(1.0) * font_size * 0.75 + font_size,
        "height": font_size * 1.6,
    })
}

impl StampCapability {
    /// `renderMark(mark)`: a drawn or typed mark rendered to a one-page PDF,
    /// a fresh page the size of the mark, paths kept as paths, the font
    /// subset-embedded.
    fn render_mark(&self, mark: MarkSource) -> Promise<Vec<u8>> {
        let opening = self.open_asset_document(blank_library_pdf());
        Promise::spawn(async move {
            let doc = opening.await?;
            let body = async {
                let layout = call(&doc, "pages.list", Vec::new()).await?;
                let pages = member(&layout, "pages")?;
                if nullish(&pages) {
                    return Err(read_off(&pages, "0"));
                }
                let Some(scratch) = pages.as_array().and_then(|p| p.first()).cloned() else {
                    return Err(stamp_error(
                        code::OPERATION_FAILED,
                        "the scratch document has no page",
                    ));
                };
                let reference = scratch.get("ref").cloned().unwrap_or_else(undefined);
                let pon =
                    reference.get("objectNumber").and_then(Value::as_f64).unwrap_or(0.0) as u32;
                let created = match &mark {
                    MarkSource::Ink { strokes, color, stroke_width } => {
                        let color = hex_color(color.as_deref().unwrap_or("#1d2b53"))?;
                        let draft = json!({
                            "subtype": "ink",
                            "inkList": ink_list_of(strokes)?,
                            "color": color,
                            "strokeWidth": stroke_width.unwrap_or(2.0),
                        });
                        page_call(&doc, "page.annotations.create", pon, vec![draft]).await?
                    }
                    MarkSource::Text { text, font_family, color, font_size } => {
                        let color = hex_color(color.as_deref().unwrap_or("#1d2b53"))?;
                        let size = font_size.unwrap_or(36.0);
                        let draft = json!({
                            "subtype": "free-text",
                            "intent": "free-text",
                            "contents": text,
                            "fontFamily": font_family,
                            "fontSize": size,
                            "textAlign": "center",
                            "verticalAlign": "middle",
                            "box": text_bounds(text, size),
                            "fontColor": color,
                            "strokeWidth": 0,
                        });
                        page_call(&doc, "page.annotations.create", pon, vec![draft]).await?
                    }
                    MarkSource::Image { .. } | MarkSource::Pdf { .. } => {
                        return Err(Error::new("[stamp] renderMark takes an ink or text mark"));
                    }
                };
                let annotation = member(&created, "annotation")?;
                let reference = member(&annotation, "ref")?;
                let exported = page_call(
                    &doc,
                    "page.annotations.exportAppearance",
                    pon,
                    vec![json!([reference])],
                )
                .await?;
                Ok(bytes_of(&exported).unwrap_or_default())
            }
            .await;
            close(&doc, body).await
        })
    }

    /// `extractPage(bytes, pageIndex)`: one page of a PDF as a single-page
    /// PDF (a `pdf` mark with `pageIndex`).
    fn extract_page(&self, bytes: Vec<u8>, page_index: f64) -> Promise<Vec<u8>> {
        let opening = self.open_asset_document(bytes);
        Promise::spawn(async move {
            let doc = opening.await?;
            let body = async {
                let layout = call(&doc, "pages.list", Vec::new()).await?;
                let pages = member(&layout, "pages")?;
                let page = (page_index >= 0.0 && page_index.fract() == 0.0)
                    .then(|| pages.as_array().and_then(|p| p.get(page_index as usize)).cloned())
                    .flatten();
                let Some(page) = page else {
                    let count = layout.get("pageCount").cloned().unwrap_or_else(undefined);
                    let count =
                        count.as_f64().map_or_else(|| "undefined".to_string(), number_to_string);
                    return Err(stamp_error(
                        code::INVALID_INPUT,
                        format!(
                            "the PDF has no page {} ({count} pages)",
                            number_to_string(page_index)
                        ),
                    ));
                };
                let reference = page.get("ref").cloned().unwrap_or_else(undefined);
                Ok(bytes_of(&call(&doc, "pages.extract", vec![json!([reference])]).await?)
                    .unwrap_or_default())
            }
            .await;
            close(&doc, body).await
        })
    }

    /// `resolveAssetSource(input)`: the bytes an asset is made from, the
    /// caller's `source`, or its `mark` authored into one.
    pub(crate) fn resolve_asset_source(&self, input: &AddAssetInput) -> Promise<WireResource> {
        if input.source.is_some() == input.mark.is_some() {
            return Promise::rejected(stamp_error(
                code::INVALID_INPUT,
                "createAsset takes exactly one of `source` / `mark`",
            ));
        }
        let this = self.clone();
        let source = input.source.clone();
        let mark = input.mark.clone();
        Promise::spawn(async move {
            if let Some(source) = source {
                return adopt(resolve_binary_source(&source)).await;
            }
            let Some(mark) = mark else { unreachable!("one of source / mark is present") };
            let made = |bytes: Vec<u8>| WireResource { bytes, mime_type: None, name: None };
            match mark {
                MarkSource::Image { source } => adopt(resolve_binary_source(&source)).await,
                MarkSource::Pdf { source, page_index } => {
                    let resolved = resolve_binary_source(&source).await?;
                    match page_index {
                        None => Ok(resolved),
                        Some(index) => Ok(made(this.extract_page(resolved.bytes, index).await?)),
                    }
                }
                ink_or_text => Ok(made(this.render_mark(ink_or_text).await?)),
            }
        })
    }
}
