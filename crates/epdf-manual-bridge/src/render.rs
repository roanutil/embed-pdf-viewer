//! `render_page` over the C ABI, without the copy. The raster's bytes are
//! an `Arc<Vec<u8>>` the host table shares with the render plugin's raster
//! store; the bridge hands the caller a pointer into that same buffer and
//! boxes one more `Arc` as the owner. The uniffi export copies the bytes
//! into a `Vec` for its `RenderedImage`, then again into a `RustBuffer`.

use std::ffi::{c_char, c_void};
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{KernelBound, KernelPost, PageRef};
use epdf_plugin_render::RenderCapability;
use epdf_plugin_render::ffi::{
    PixelSource, RenderPageOptions, RenderPixelFormat, start_render_page,
};

use crate::{Call, Completion, EpdfComplete, arg, live, registry};

/// Four bytes a pixel in memory order red, green, blue, alpha, straight
/// (not premultiplied) alpha: the uniffi bridge's `RenderPixelFormat::Rgba8`.
pub const EPDF_PIXEL_FORMAT_RGBA8: u32 = 0;

/// A page raster. `data` is `len` bytes, `stride` a row, valid until
/// `epdf_pixels_free(owner)`, from any thread.
#[repr(C)]
#[derive(Debug)]
pub struct EpdfPixels {
    pub data: *const u8,
    pub len: usize,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    /// Device pixels per PDF point across the page's width.
    pub scale: f64,
    pub owner: *mut c_void,
}

struct Bound {
    cap: RenderCapability,
    pixels: PixelSource,
}

/// A document's render capability, held on the kernel thread.
pub struct EpdfRender {
    bound: KernelBound<Bound>,
    post: KernelPost,
}

/// Open `document_id`'s render capability. `result` is the `EpdfRender*`,
/// which the caller frees with `epdf_render_free`.
///
/// # Safety
/// `document_id` is NUL-terminated; `done` is called once with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_render_open(
    kernel: u64,
    document_id: *const c_char,
    ctx: *mut c_void,
    done: EpdfComplete,
) {
    let completion = Completion::new(ctx, done);
    let id = match arg(document_id, "the document id") {
        Ok(id) => id,
        Err(error) => return completion.fail(error),
    };
    let Some(registered) = registry::lookup(kernel) else { return };
    let post = registered.post.clone();
    let source = registered.pixels;
    (registered.run)(Box::new(move |kernel| {
        match kernel.capability(&epdf_plugin_render::RENDER, Some(&id)) {
            Ok(cap) => {
                let pixels: PixelSource = Rc::new(source);
                let render = Box::new(EpdfRender {
                    bound: KernelBound::new(Bound { cap, pixels }, post.clone()),
                    post,
                });
                completion.ok_ptr(Box::into_raw(render) as *const c_void);
            }
            Err(error) => completion.fail(error),
        }
    }));
}

/// # Safety
/// `render` came from `epdf_render_open` and is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_render_free(render: *mut EpdfRender) {
    if !render.is_null() {
        drop(unsafe { Box::from_raw(render) });
    }
}

/// Render one page at `scale` device pixels a point. Answers the call id
/// for `epdf_cancel`; `result` is a `const EpdfPixels*`, whose `owner` the
/// caller frees.
///
/// # Safety
/// `render` is live or null (which answers `invalid-input`); `done` is
/// called once with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_render_page(
    render: *mut EpdfRender,
    page_object_number: u32,
    scale: f64,
    ctx: *mut c_void,
    done: EpdfComplete,
) -> u64 {
    let completion = Completion::new(ctx, done);
    let render = match live(render, "the render handle") {
        Ok(render) => render,
        Err(error) => {
            completion.fail(error);
            return 0;
        }
    };
    let call = Call::begin(&render.post);
    let id = call.id();
    render.bound.fire(move |b| {
        let page = PageRef { page_object_number };
        let options = RenderPageOptions { scale: Some(scale), ..RenderPageOptions::default() };
        match start_render_page(&b.cap, &b.pixels, page, options) {
            Ok(promise) => call.settle(promise, move |result| match result {
                Ok(raster) => {
                    let format = match raster.format {
                        RenderPixelFormat::Rgba8 => EPDF_PIXEL_FORMAT_RGBA8,
                    };
                    let owner: Box<Arc<Vec<u8>>> = Box::new(raster.bytes.clone());
                    let pixels = EpdfPixels {
                        data: owner.as_ptr(),
                        len: raster.stride as usize * raster.height as usize,
                        width: raster.width,
                        height: raster.height,
                        stride: raster.stride,
                        format,
                        scale: raster.scale,
                        owner: Box::into_raw(owner) as *mut c_void,
                    };
                    completion.ok(&pixels);
                }
                Err(error) => completion.fail(error.clone()),
            }),
            Err(error) => completion.fail(error),
        }
    });
    id
}

/// # Safety
/// `owner` came from an `EpdfPixels` and is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_pixels_free(owner: *mut c_void) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner as *mut Arc<Vec<u8>>) });
    }
}
