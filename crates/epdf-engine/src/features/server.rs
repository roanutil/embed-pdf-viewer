//! G15. Server and file surfaces.
//!
//! Operations: `document.renderPageFile`, and the three `*Encoded` kinds
//! that wrap G4's raster in an image encoder.
//!
//! # The encoder is injected, and a build without one refuses
//!
//! Nothing here encodes anything. A PNG or a JPEG is a host's business:
//! a browser has `createImageBitmap` and a server has whatever it linked,
//! and an engine that picked one would be shipping an image library to
//! every target to satisfy the one that needs it. So [`ImageEncoder`] is a
//! trait the host implements, and a host that implements none gets
//! `NotImplemented` rather than a raw raster it did not ask for.
//!
//! That refusal is the SHIPPING behaviour on two of the three targets:
//! the browser and the local worker both answer the encoded kinds that way
//! today, and this port keeps them doing it.
//!
//! # Rendering a file nobody opened
//!
//! `render_page_file` is the odd one out. Every other operation works on a
//! session a caller opened and will close; this one takes a path, opens it,
//! renders one page and closes it again. It exists for a server that is
//! answering a thumbnail request about a file it has never seen before, and
//! holding a session per such request would be a session pool the size of
//! the bucket.

use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::render::raster::{PageRaster, RenderOptions, render_page};
use crate::identity::PageObjectNumber;

/// What format to encode a raster as (`RenderEncode`, `worker-protocol.ts`):
/// a `PageNetworkRenderFormat` and an optional quality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderEncode {
    pub format: RenderEncodeFormat,
    /// WebP quality from 0 (smallest) to 1 (best), passed through to the
    /// encoder as upstream's worker passes it. PNG is lossless and ignores
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
}

impl RenderEncode {
    /// A PNG, which takes no quality.
    pub const PNG: Self = Self { format: RenderEncodeFormat::Png, quality: None };
}

/// `PageNetworkRenderFormat` (`dto/PageRender.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderEncodeFormat {
    Png,
    Webp,
}

/// An encoded image, and what it is.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedImage {
    /// The MIME type the encoder produced, which is the encoder's answer
    /// rather than a restatement of the request: an encoder asked for WebP
    /// that fell back to PNG says so here.
    pub content_type: String,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// What a host plugs in to answer the encoded kinds.
///
/// One method, and it is fallible: an encoder that cannot produce the
/// requested format says so rather than silently producing another.
pub trait ImageEncoder {
    fn encode(
        &self,
        raster: &PageRaster,
        encode: &RenderEncode,
    ) -> Result<EncodedImage, EngineError>;
}

/// Encode a raster a caller already has.
///
/// Separate from the render so the two compose: every encoded kind is a
/// render this engine already does followed by this, and a host that wants
/// the raster instead simply does not call it.
pub fn encode_raster(
    encoder: Option<&dyn ImageEncoder>,
    raster: &PageRaster,
    encode: &RenderEncode,
    cancel: &CancelToken,
) -> Result<EncodedImage, EngineError> {
    let encoder = encoder.ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotImplemented,
            "this engine has no image encoder: the encoded render kinds are a server surface",
        )
    })?;
    let encoded = encoder.encode(raster, encode)?;
    // Checked AFTER, because an encoder is not interruptible: a token set
    // while it ran is honoured by refusing rather than by handing back a
    // result for a job that is already gone.
    cancel.check()?;
    Ok(encoded)
}

/// A page held only for the length of one render.
///
/// The read paths take a [`crate::access::PageAccess`], and a page loaded
/// straight off a file-backed document is not one of the shapes that
/// already implements it: it belongs to a document this function opened and
/// will close, rather than to a session.
struct BorrowedFilePage<'a>(&'a pdfium::Page<'a>);

impl crate::access::PageAccess for BorrowedFilePage<'_> {
    fn view(&self) -> &pdfium::PageView<'_> {
        self.0
    }
}

/// What rendering a page out of a file produced.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPageFile {
    /// The page's durable identity within that file.
    pub page_object_number: PageObjectNumber,
    /// How many pages the file has, which a caller asking for a thumbnail
    /// of page zero usually wants next.
    pub page_count: u32,
    pub raster: PageRaster,
}

/// Open a file, render one page by INDEX, and close it.
///
/// By index rather than by object number, because the caller has not seen
/// this file and has no object number to name. The object number comes back
/// in the result, so a second request about the same page can be durable.
pub fn render_page_file(
    library: &std::rc::Rc<pdfium::Library>,
    path: &std::path::Path,
    password: Option<&str>,
    page_index: u32,
    options: &RenderOptions<crate::geom::PageRect>,
    cancel: &CancelToken,
) -> Result<RenderedPageFile, EngineError> {
    cancel.check()?;
    // Range-read rather than slurped: a thumbnail of page one of a large
    // file should not pay for the rest of it.
    let access = pdfium::FileAccess::from_file(path).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("the file could not be opened: {e}"),
        )
    })?;
    let document =
        pdfium::Document::from_file_access(library.clone(), &access, password).map_err(|e| {
            EngineError::with_message(
                match e {
                    pdfium::PdfiumError::Password => EngineErrorCode::DocPasswordRequired,
                    pdfium::PdfiumError::Security => EngineErrorCode::DocOpenFailed,
                    _ => EngineErrorCode::MalformedPdf,
                },
                format!("the file is not a document this build can open: {e}"),
            )
        })?;

    cancel.check()?;
    let page_count = document.page_count();
    if page_index >= page_count {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no page at index {page_index}: the file has {page_count}"),
        ));
    }
    let page_object_number =
        PageObjectNumber::new(document.page_object_number_by_index(page_index))?;

    // Through the ordinary page loader, so a rotated page renders the way
    // it does everywhere else in this engine rather than the way
    // `FPDF_LoadPage` alone would place it.
    let page = document.page_by_object_number(page_object_number.get()).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::MalformedPdf,
            format!("the page could not be loaded: {e}"),
        )
    })?;
    // `options` are page space, and no session carried this document to the
    // host's boundary, so the target converts here, on the page it opened
    // (`renderOptionsInFileSpace(req.options, () => page.pdfCropBox)`).
    let options = crate::page_space::rendering::render_options_in_file_space(*options, || {
        Ok::<_, EngineError>(crate::features::pages::read_boxes(&document, page_index).crop)
    })?;
    let raster = render_page(&BorrowedFilePage(&page), &options, cancel)?;
    Ok(RenderedPageFile { page_object_number, page_count, raster })
}
