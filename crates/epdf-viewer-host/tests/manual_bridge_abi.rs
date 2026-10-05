//! `crates/epdf-manual-bridge`'s C ABI, called from Rust the way Swift
//! calls it: `extern "C"` callbacks with a boxed context, results read
//! inside the callback. Experimental: built only with `manual-bridge`
//! (`cargo test -p epdf-viewer-host --features manual-bridge
//! --test manual_bridge_abi`), so the workspace build never turns the
//! feature on for the shipping host.

#[path = "manual_bridge_support/mod.rs"]
mod support;

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use epdf_ffi::{CancelToken, PageRef};
use epdf_manual_bridge::*;
use epdf_plugin_render::ffi::RenderPageOptions;
use epdf_plugin_search::ffi::SearchQuery;
use futures::executor::block_on;
use support::*;

fn query(text: &str) -> SearchQuery {
    SearchQuery { text: text.into(), ..Default::default() }
}

#[test]
fn search_through_the_c_abi_matches_uniffi() {
    let (host, doc) = host_with_report();
    let search = open_search_via_c(&host, &doc);
    let found = search_via_c(search, "the").unwrap();
    assert!(found > 0);
    assert_eq!(hit_count_via_c(search), found);

    let typed = block_on(host.search(doc.id.clone())).unwrap();
    assert_eq!(block_on(typed.hit_count(None)).unwrap(), found);
    unsafe { epdf_search_free(search) };
}

#[test]
fn hits_through_the_c_abi_match_uniffi_hit_for_hit() {
    let (host, doc) = host_with_report();
    let search = open_search_via_c(&host, &doc);
    search_via_c(search, "the").unwrap();
    let manual = hits_via_c(search);

    let typed = block_on(host.search(doc.id.clone())).unwrap();
    let uniffi: Vec<CopiedHit> =
        block_on(typed.hits(None)).unwrap().iter().map(copy_uniffi_hit).collect();
    assert!(!manual.is_empty());
    assert!(manual.iter().any(|h| h.snippet.is_some()), "{:?}", manual[0]);
    assert_eq!(manual, uniffi);
    unsafe { epdf_search_free(search) };
}

#[test]
fn find_all_through_the_c_abi_matches_uniffi() {
    let (host, doc) = host_with_report();
    let search = open_search_via_c(&host, &doc);
    let manual = find_all_via_c(search, "the").unwrap();

    let typed = block_on(host.search(doc.id.clone())).unwrap();
    let uniffi: Vec<CopiedHit> = block_on(typed.find_all(query("the"), None, CancelToken::new()))
        .unwrap()
        .iter()
        .map(copy_uniffi_hit)
        .collect();
    assert!(!manual.is_empty());
    assert_eq!(manual, uniffi);
    unsafe { epdf_search_free(search) };
}

/// The `colors` fixture is an 80 by 20 point page of four 20-point squares:
/// red, green, blue, then (0.2, 0.4, 0.6).
#[test]
fn pixels_cross_without_a_copy_and_free_once() {
    let (host, doc) = host_with_colors();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let render = open_render_via_c(&host, &doc);
    let pixels = render_page_via_c(render, page.page_object_number, 0.5).unwrap();
    assert_eq!((pixels.width, pixels.height), (40, 10));
    assert_eq!(pixels.len, (pixels.stride * pixels.height) as usize);
    assert_eq!(pixels.format, EPDF_PIXEL_FORMAT_RGBA8);
    assert_eq!(pixels.scale, 0.5);
    assert_eq!(pixel(&pixels, 5, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 35, 5), [51, 102, 153, 255]);

    // The same bytes uniffi copies out.
    let typed = block_on(host.render(doc.id.clone())).unwrap();
    let options = RenderPageOptions { scale: Some(0.5), ..Default::default() };
    let image = block_on(typed.render_page(page, options, CancelToken::new())).unwrap();
    let manual = unsafe { std::slice::from_raw_parts(pixels.data, pixels.len) };
    assert_eq!(manual, &image.pixels[..]);

    // A second render of the same raster is the same allocation: no copy
    // was made for either caller.
    let again = render_page_via_c(render, page.page_object_number, 0.5).unwrap();
    assert_eq!(again.data, pixels.data);

    unsafe { epdf_pixels_free(pixels.owner) };
    unsafe { epdf_pixels_free(again.owner) };
    unsafe { epdf_render_free(render) };
    drop(typed);
    // The host table still holds the raster for the raster store, and only
    // that: freeing the C owners released nothing it needed.
    assert!(block_on(host.host_table_entries()).unwrap() >= 1);
}

#[test]
fn a_bad_scale_is_refused_with_its_code() {
    let (host, doc) = host_with_colors();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let render = open_render_via_c(&host, &doc);
    let failure = render_page_via_c(render, page.page_object_number, 0.0).unwrap_err();
    assert_eq!(failure.kind, EPDF_ERROR_KERNEL);
    assert_eq!(failure.code.as_deref(), Some("invalid-input"));
    unsafe { epdf_render_free(render) };
}

#[test]
fn cancelling_find_all_rejects_it() {
    let (host, doc) = host_with_report();
    let search = open_search_via_c(&host, &doc);
    let text = cstring("e");
    let (tx, rx) = mpsc::channel();
    let search_sent = Sent(search);
    let call = wait(
        move |ctx, cb| {
            let id = unsafe { epdf_search_find_all(search_sent.0, text.as_ptr(), ctx, cb) };
            epdf_cancel(id);
            let _ = tx.send(id);
        },
        |_| (),
    )
    .unwrap_err();
    let id = rx.recv().unwrap();
    assert_ne!(id, 0);
    assert_eq!(call.kind, EPDF_ERROR_KERNEL);
    assert_eq!(call.code.as_deref(), Some("operation-cancelled"));
    // Cancelling a call that already finished does nothing.
    epdf_cancel(id);
    unsafe { epdf_search_free(search) };
}

struct Seen {
    progress: AtomicUsize,
    completed: AtomicUsize,
    released: AtomicUsize,
    done: Mutex<Option<mpsc::Sender<()>>>,
}

extern "C" fn on_event(ctx: *mut c_void, kind: i32, payload: *const c_void) {
    let seen = unsafe { &*(ctx as *const Seen) };
    match kind {
        EPDF_SEARCH_EVENT_PROGRESS => {
            let event = unsafe { &*(payload as *const EpdfSearchProgress) };
            assert!(event.scanned <= event.total);
            assert!(!event.operation_id.is_null());
            seen.progress.fetch_add(1, Ordering::SeqCst);
        }
        EPDF_SEARCH_EVENT_COMPLETED => {
            let event = unsafe { &*(payload as *const EpdfSearchCompleted) };
            assert!(event.hit_count > 0);
            seen.completed.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = seen.done.lock().unwrap().take() {
                let _ = tx.send(());
            }
        }
        _ => {}
    }
}

extern "C" fn release(ctx: *mut c_void) {
    let seen = unsafe { Arc::from_raw(ctx as *const Seen) };
    seen.released.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn progress_events_arrive_during_a_search_and_the_listener_is_released_once() {
    let (host, doc) = host_with_report();
    let search = open_search_via_c(&host, &doc);
    let (tx, rx) = mpsc::channel();
    let seen = Arc::new(Seen {
        progress: AtomicUsize::new(0),
        completed: AtomicUsize::new(0),
        released: AtomicUsize::new(0),
        done: Mutex::new(Some(tx)),
    });
    let ctx = Arc::into_raw(seen.clone()) as *mut c_void;
    let subscription = unsafe { epdf_search_subscribe(search, ctx, on_event, Some(release)) };
    search_via_c(search, "e").unwrap();
    rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert!(seen.progress.load(Ordering::SeqCst) > 0);
    assert_eq!(seen.completed.load(Ordering::SeqCst), 1);

    unsafe { epdf_subscription_free(subscription) };
    // The release runs on the kernel thread once the hooks are gone.
    let _ = hit_count_via_c(search);
    assert_eq!(seen.released.load(Ordering::SeqCst), 1);
    unsafe { epdf_search_free(search) };
}

#[test]
fn noop_hop_answers_from_the_kernel_thread() {
    let (host, _doc) = host_with_report();
    let kernel = host.experimental_kernel_token();
    epdf_bench_noop();
    let thread = wait(
        |ctx, cb| unsafe { epdf_bench_noop_hop(kernel, ctx, cb) },
        |_| std::thread::current().name().map(String::from),
    )
    .unwrap();
    assert_eq!(thread.as_deref(), Some("epdf-kernel"));
    block_on(host.bench_noop_hop()).unwrap();
}

#[test]
fn an_unknown_kernel_token_answers_closed() {
    let id = cstring("doc");
    let failure =
        wait(|ctx, cb| unsafe { epdf_search_open(u64::MAX, id.as_ptr(), ctx, cb) }, |_| ())
            .unwrap_err();
    assert_eq!(failure.kind, EPDF_ERROR_CLOSED);
}

#[test]
fn an_unknown_document_is_a_kernel_error() {
    let (host, _doc) = host_with_report();
    let id = cstring("no-such-document");
    let kernel = host.experimental_kernel_token();
    let failure = wait(|ctx, cb| unsafe { epdf_search_open(kernel, id.as_ptr(), ctx, cb) }, |_| ())
        .unwrap_err();
    assert_eq!(failure.kind, EPDF_ERROR_KERNEL);
}

#[test]
fn a_call_after_shutdown_answers_closed_and_frees_cleanly() {
    let (host, doc) = host_with_report();
    let search = open_search_via_c(&host, &doc);
    let render = open_render_via_c(&host, &doc);
    let kernel = host.experimental_kernel_token();
    host.shutdown();
    let search_sent = Sent(search);
    let failure =
        wait(move |ctx, cb| unsafe { epdf_search_hit_count(search_sent.0, ctx, cb) }, |_| ())
            .unwrap_err();
    assert_eq!(failure.kind, EPDF_ERROR_CLOSED);
    let failure =
        wait(|ctx, cb| unsafe { epdf_bench_noop_hop(kernel, ctx, cb) }, |_| ()).unwrap_err();
    assert_eq!(failure.kind, EPDF_ERROR_CLOSED);
    unsafe { epdf_search_free(search) };
    unsafe { epdf_render_free(render) };
}

#[test]
fn the_kernel_token_is_per_host() {
    let (a, _) = host_with_report();
    let (b, _) = host_with_report();
    assert_ne!(a.experimental_kernel_token(), b.experimental_kernel_token());
    let _ = PageRef { page_object_number: 0 };
}
