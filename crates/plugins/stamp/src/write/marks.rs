//! `write/marks.ts`: authoring. A drawn or typed mark rendered to a one-page
//! PDF (the annotation is authored on a scratch page and its appearance
//! exported), a page of a PDF as a single-page PDF, and the bytes an asset
//! is made from.

use epdf_kernel::Error;
use epdf_kernel::rt;
use epdf_plugin_annotation::binary::resolve_binary_source;
use serde_json::{Value, json};

use crate::blank_library::blank_library_pdf;
use crate::contract::{AddAssetInput, MarkSource};
use crate::controller::StampCapability;
use crate::promise::await_engine;
use crate::services::asset_engine::{bytes_of, call, close, page_call};
use crate::value::{get, num};
use crate::write::libraries::pon_of_ref;

/// The bytes an asset is made from.
pub(crate) struct ResolvedSource {
    pub bytes: Vec<u8>,
}

/// `hexColor(hex)`: `#rrggbb` to the engine's sRGB triplet.
fn hex_color(hex: &str) -> Result<Value, Error> {
    let trimmed = hex.trim();
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::engine(
            "InvalidArg",
            format!("[stamp] mark color must be #rrggbb, got '{hex}'"),
        ));
    }
    let n = u32::from_str_radix(digits, 16).unwrap_or(0);
    Ok(json!({ "r": (n >> 16) & 0xff, "g": (n >> 8) & 0xff, "b": n & 0xff }))
}

fn number(x: f64) -> Value {
    epdf_plugin_annotation::value::number(x)
}

/// `inkBounds(strokes, strokeWidth)`: the rect an ink mark occupies, padded
/// by its stroke.
fn ink_bounds(strokes: &[Vec<(f64, f64)>], stroke_width: f64) -> Result<Value, Error> {
    let (mut left, mut bottom, mut right, mut top) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for stroke in strokes {
        for &(x, y) in stroke {
            if x < left {
                left = x;
            }
            if x > right {
                right = x;
            }
            if y < bottom {
                bottom = y;
            }
            if y > top {
                top = y;
            }
        }
    }
    if !left.is_finite() {
        return Err(Error::engine("InvalidArg", "[stamp] an ink mark needs at least one point"));
    }
    let pad = stroke_width;
    Ok(json!({
        "left": number(left - pad),
        "bottom": number(bottom - pad),
        "right": number(right + pad),
        "top": number(top + pad),
    }))
}

/// `textBounds(text, fontSize)`: a generous box; the export crops to the
/// glyphs.
fn text_bounds(text: &str, font_size: f64) -> Value {
    let length = text.encode_utf16().count().max(1) as f64;
    json!({
        "left": 0,
        "bottom": 0,
        "right": number(length * font_size * 0.75 + font_size),
        "top": number(font_size * 1.6),
    })
}

impl StampCapability {
    /// `renderMark(mark)`: a drawn or typed mark as a one-page PDF.
    async fn render_mark(&self, mark: &MarkSource) -> Result<Vec<u8>, Error> {
        let doc = rt::returned(self.open_asset_document(blank_library_pdf())).await?;
        let body = async {
            let layout = await_engine(call(&doc, "pages.list", Vec::new())).await?;
            let scratch = get(&layout, "pages").and_then(Value::as_array).and_then(|p| p.first()).cloned();
            let Some(scratch) = scratch else {
                return Err(Error::engine("Unknown", "[stamp] no scratch page"));
            };
            let pon = get(&scratch, "ref").and_then(pon_of_ref).unwrap_or(0);
            doc.page_index(pon)?;
            if !doc.has("page.annotations.exportAppearance") {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] authoring a mark needs an asset engine that can export annotation appearances",
                ));
            }
            let draft = match mark {
                MarkSource::Ink { strokes, color, stroke_width } => {
                    let color = hex_color(color.as_deref().unwrap_or("#1d2b53"))?;
                    let width = stroke_width.unwrap_or(2.0);
                    let ink: Vec<Value> = strokes
                        .iter()
                        .map(|s| json!(s.iter().map(|&(x, y)| json!({ "x": number(x), "y": number(y) })).collect::<Vec<_>>()))
                        .collect();
                    json!({
                        "subtype": "ink",
                        "inkList": ink,
                        "rect": ink_bounds(strokes, width)?,
                        "color": color,
                        "strokeWidth": number(width),
                    })
                }
                MarkSource::Text { text, font_family, color, font_size } => {
                    let color = hex_color(color.as_deref().unwrap_or("#1d2b53"))?;
                    let size = font_size.unwrap_or(36.0);
                    json!({
                        "subtype": "free-text",
                        "intent": "free-text",
                        "contents": text,
                        "fontFamily": font_family,
                        "fontSize": number(size),
                        "textAlign": "left",
                        "rect": text_bounds(text, size),
                        "fontColor": color,
                        "strokeWidth": 0,
                    })
                }
                _ => unreachable!("render_mark takes an ink or text mark"),
            };
            let created =
                await_engine(page_call(&doc, "page.annotations.create", pon, vec![draft])).await?;
            let created_ref = get(&created, "created").and_then(|c| get(c, "ref")).cloned();
            let Some(created_ref) = created_ref else {
                return Err(crate::services::asset_engine::read_off(
                    get(&created, "created").unwrap_or(&epdf_kernel::undefined()),
                    "ref",
                ));
            };
            let exported = await_engine(page_call(
                &doc,
                "page.annotations.exportAppearance",
                pon,
                vec![json!([created_ref])],
            ))
            .await?;
            Ok(bytes_of(&exported).unwrap_or_default())
        }
        .await;
        close(&doc, body).await
    }

    /// `extractPage(bytes, pageIndex)`: one page of a PDF as a single-page
    /// PDF.
    async fn extract_page(&self, bytes: Vec<u8>, page_index: f64) -> Result<Vec<u8>, Error> {
        let doc = rt::returned(self.open_asset_document(bytes)).await?;
        let body = async {
            let layout = await_engine(call(&doc, "pages.list", Vec::new())).await?;
            let pages =
                get(&layout, "pages").and_then(Value::as_array).cloned().unwrap_or_default();
            let page = (page_index >= 0.0 && page_index.fract() == 0.0)
                .then(|| pages.get(page_index as usize).cloned())
                .flatten();
            let Some(page) = page else {
                return Err(Error::engine(
                    "InvalidArg",
                    format!(
                        "[stamp] the PDF has no page {} ({} pages)",
                        epdf_geometry::js::number_to_string(page_index),
                        epdf_geometry::js::number_to_string(
                            num(&layout, "pageCount").unwrap_or(f64::NAN)
                        )
                    ),
                ));
            };
            let page_ref = get(&page, "ref").cloned().unwrap_or(Value::Null);
            let extracted =
                await_engine(call(&doc, "pages.extract", vec![json!([page_ref])])).await?;
            Ok(bytes_of(&extracted).unwrap_or_default())
        }
        .await;
        close(&doc, body).await
    }

    /// `resolveAssetSource(input)`: the caller's `source`, or its `mark`
    /// authored into bytes.
    pub(crate) async fn resolve_asset_source(
        &self,
        input: &AddAssetInput,
    ) -> Result<ResolvedSource, Error> {
        if input.source.is_some() == input.mark.is_some() {
            return Err(Error::engine(
                "InvalidArg",
                "[stamp] addAsset takes exactly one of `source` / `mark`",
            ));
        }
        let adopt_source = |source: &Value| {
            let promise = resolve_binary_source(source);
            async move {
                // `return resolveBinarySource(source)` from an async function.
                let resolved = crate::promise::adopt(promise).await?;
                Ok::<_, Error>(ResolvedSource { bytes: resolved.bytes })
            }
        };
        if let Some(source) = &input.source {
            return adopt_source(source).await;
        }
        match input.mark.as_ref().expect("checked above") {
            MarkSource::Image { source } => adopt_source(source).await,
            MarkSource::Pdf { source, page_index } => {
                let resolved = resolve_binary_source(source).await?;
                let resolved = ResolvedSource { bytes: resolved.bytes };
                let Some(page_index) = page_index else { return Ok(resolved) };
                let bytes = rt::returned(self.extract_page(resolved.bytes, *page_index)).await?;
                Ok(ResolvedSource { bytes })
            }
            mark => {
                let bytes = rt::returned(self.render_mark(mark)).await?;
                Ok(ResolvedSource { bytes })
            }
        }
    }
}
