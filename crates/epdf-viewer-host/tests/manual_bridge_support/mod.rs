//! Thin helpers over the C ABI: each one is a `wait` over a single C call,
//! with the result copied out inside the callback the way Swift's bridge
//! reads it.

#![allow(dead_code)]

use std::ffi::{CStr, CString, c_void};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use epdf_manual_bridge::*;
use epdf_plugin_render::ffi::RenderConfig;
use epdf_plugin_search::ffi::SearchConfig;
use epdf_viewer_host::{
    DocumentInfo, OpenDocumentOptions, ViewerConfig, ViewerDelegate, ViewerHost,
};
use futures::executor::block_on;

struct Delegate;
impl ViewerDelegate for Delegate {
    fn state_changed(&self, _revision: u64) {}
    fn report(&self, _error: epdf_ffi::ViewerError) {}
}

/// A host with search and render, started, with `fixture` open.
pub fn host_with(fixture: &str) -> (Arc<ViewerHost>, DocumentInfo) {
    let config = ViewerConfig {
        search: Some(SearchConfig::default()),
        render: Some(RenderConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let options = OpenDocumentOptions { name: None, password: None, activate: true };
    let doc = block_on(host.open_document(test_support::fixture(fixture), options)).unwrap();
    (host, doc)
}

pub fn host_with_report() -> (Arc<ViewerHost>, DocumentInfo) {
    host_with(test_support::REPORT)
}

pub fn host_with_colors() -> (Arc<ViewerHost>, DocumentInfo) {
    host_with(test_support::COLORS)
}

/// What an `EpdfError` said, copied out of the callback.
#[derive(Debug, PartialEq)]
pub struct Failure {
    pub kind: i32,
    pub code: Option<String>,
    pub detail: Option<String>,
}

/// Raw pointers, moved across the channel. The test owns what they point at.
pub struct Sent<T>(pub T);
unsafe impl<T> Send for Sent<T> {}

type Reply = Box<dyn FnOnce(&EpdfError, *const c_void) + Send>;

extern "C" fn trampoline(ctx: *mut c_void, error: EpdfError, result: *const c_void) {
    let reply = unsafe { Box::from_raw(ctx as *mut Reply) };
    reply(&error, result);
    unsafe { epdf_error_free(error) };
}

fn text(ptr: *const std::ffi::c_char) -> Option<String> {
    (!ptr.is_null()).then(|| unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
}

/// Start one C call with a fresh context, and wait for its callback. `read`
/// runs inside the callback, on the kernel thread, while `result` is valid.
pub fn wait<R: Send + 'static>(
    start: impl FnOnce(*mut c_void, EpdfComplete),
    read: impl FnOnce(*const c_void) -> R + Send + 'static,
) -> Result<R, Failure> {
    let (tx, rx) = mpsc::channel();
    let reply: Reply = Box::new(move |error: &EpdfError, result: *const c_void| {
        let answer = if error.kind == 0 {
            Ok(read(result))
        } else {
            Err(Failure { kind: error.kind, code: text(error.code), detail: text(error.detail) })
        };
        let _ = tx.send(answer);
    });
    start(Box::into_raw(Box::new(reply)) as *mut c_void, trampoline);
    rx.recv_timeout(Duration::from_secs(30)).expect("the callback never ran")
}

pub fn cstring(s: &str) -> CString {
    CString::new(s).unwrap()
}

pub fn open_search_via_c(host: &ViewerHost, doc: &DocumentInfo) -> *mut EpdfSearch {
    let id = cstring(&doc.id);
    let kernel = host.experimental_kernel_token();
    let raw = wait(
        |ctx, cb| unsafe { epdf_search_open(kernel, id.as_ptr(), ctx, cb) },
        |result| Sent(result as *mut EpdfSearch),
    )
    .unwrap();
    raw.0
}

pub fn open_render_via_c(host: &ViewerHost, doc: &DocumentInfo) -> *mut EpdfRender {
    let id = cstring(&doc.id);
    let kernel = host.experimental_kernel_token();
    let raw = wait(
        |ctx, cb| unsafe { epdf_render_open(kernel, id.as_ptr(), ctx, cb) },
        |result| Sent(result as *mut EpdfRender),
    )
    .unwrap();
    raw.0
}

pub fn hit_count_via_c(search: *mut EpdfSearch) -> u32 {
    let search = Sent(search);
    wait(
        move |ctx, cb| unsafe { epdf_search_hit_count(search.0, ctx, cb) },
        |result| unsafe { *(result as *const u32) },
    )
    .unwrap()
}

pub fn search_via_c(search: *mut EpdfSearch, query: &str) -> Result<u32, Failure> {
    let search = Sent(search);
    let text = cstring(query);
    wait(
        move |ctx, cb| {
            unsafe { epdf_search_search(search.0, text.as_ptr(), ctx, cb) };
        },
        |result| unsafe { *(result as *const u32) },
    )
}

/// One hit with its segments and snippet, copied out of the C structs.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedHit {
    pub page_object_number: u32,
    pub page_index: u32,
    pub char_start: f64,
    pub char_count: f64,
    pub bounds: Option<[f64; 4]>,
    pub segments: Vec<([f64; 8], [f64; 4], f64)>,
    pub snippet: Option<(String, f64, f64)>,
}

/// Copy every hit out of an `EpdfHits`, inside the callback.
pub fn copy_hits(hits: &EpdfHits) -> Vec<CopiedHit> {
    let list = unsafe { std::slice::from_raw_parts(hits.hits, hits.hit_count) };
    let segments = unsafe { std::slice::from_raw_parts(hits.segments, hits.segment_count) };
    list.iter()
        .map(|h| CopiedHit {
            page_object_number: h.page_object_number,
            page_index: h.page_index,
            char_start: h.char_start,
            char_count: h.char_count,
            bounds: (h.has_bounds != 0).then_some([
                h.bounds.x,
                h.bounds.y,
                h.bounds.width,
                h.bounds.height,
            ]),
            segments: segments
                [h.segment_offset as usize..(h.segment_offset + h.segment_count) as usize]
                .iter()
                .map(|s| {
                    let q = &s.quad;
                    (
                        [
                            q.upper_start.x,
                            q.upper_start.y,
                            q.upper_end.x,
                            q.upper_end.y,
                            q.lower_start.x,
                            q.lower_start.y,
                            q.lower_end.x,
                            q.lower_end.y,
                        ],
                        [s.rect.x, s.rect.y, s.rect.width, s.rect.height],
                        s.advance,
                    )
                })
                .collect(),
            snippet: (h.has_snippet != 0).then(|| {
                let bytes = unsafe { std::slice::from_raw_parts(h.snippet_text, h.snippet_len) };
                (
                    String::from_utf8(bytes.to_vec()).unwrap(),
                    h.snippet_match_start,
                    h.snippet_match_length,
                )
            }),
        })
        .collect()
}

/// The same shape, from a uniffi hit.
pub fn copy_uniffi_hit(h: &epdf_plugin_search::ffi::SearchHit) -> CopiedHit {
    CopiedHit {
        page_object_number: h.page.page_object_number,
        page_index: h.page_index,
        char_start: h.start,
        char_count: h.count,
        bounds: h.bounds.map(|b| [b.x, b.y, b.width, b.height]),
        segments: h
            .segments
            .iter()
            .map(|s| {
                let q = &s.quad;
                (
                    [
                        q.upper_left.x,
                        q.upper_left.y,
                        q.upper_right.x,
                        q.upper_right.y,
                        q.lower_left.x,
                        q.lower_left.y,
                        q.lower_right.x,
                        q.lower_right.y,
                    ],
                    [s.rect.x, s.rect.y, s.rect.width, s.rect.height],
                    s.advance,
                )
            })
            .collect(),
        // The C bridge joins the snippet and names the match by UTF-16
        // offset and length.
        snippet: h.snippet.as_ref().map(|s| {
            (
                format!("{}{}{}", s.before, s.matched, s.after),
                s.before.encode_utf16().count() as f64,
                s.matched.encode_utf16().count() as f64,
            )
        }),
    }
}

/// `epdf_search_hits`, copied out and freed.
pub fn hits_via_c(search: *mut EpdfSearch) -> Vec<CopiedHit> {
    let search = Sent(search);
    let (copied, owner) = wait(
        move |ctx, cb| unsafe { epdf_search_hits(search.0, ctx, cb) },
        |result| {
            let hits = unsafe { &*(result as *const EpdfHits) };
            (copy_hits(hits), Sent(hits.owner))
        },
    )
    .unwrap();
    unsafe { epdf_hits_free(owner.0) };
    copied
}

/// `epdf_search_find_all`, copied out and freed.
pub fn find_all_via_c(search: *mut EpdfSearch, query: &str) -> Result<Vec<CopiedHit>, Failure> {
    let search = Sent(search);
    let text = cstring(query);
    let (copied, owner) = wait(
        move |ctx, cb| {
            unsafe { epdf_search_find_all(search.0, text.as_ptr(), ctx, cb) };
        },
        |result| {
            let hits = unsafe { &*(result as *const EpdfHits) };
            (copy_hits(hits), Sent(hits.owner))
        },
    )?;
    unsafe { epdf_hits_free(owner.0) };
    Ok(copied)
}

/// `epdf_render_page`: the struct copied out of the callback. Its `data`
/// stays valid until `epdf_pixels_free(owner)`.
pub fn render_page_via_c(
    render: *mut EpdfRender,
    page_object_number: u32,
    scale: f64,
) -> Result<EpdfPixels, Failure> {
    let render = Sent(render);
    wait(
        move |ctx, cb| {
            unsafe { epdf_render_page(render.0, page_object_number, scale, ctx, cb) };
        },
        |result| Sent(unsafe { std::ptr::read(result as *const EpdfPixels) }),
    )
    .map(|sent| sent.0)
}

pub fn pixel(pixels: &EpdfPixels, x: u32, y: u32) -> [u8; 4] {
    let bytes = unsafe { std::slice::from_raw_parts(pixels.data, pixels.len) };
    let at = (y * pixels.stride + x * 4) as usize;
    [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
}
