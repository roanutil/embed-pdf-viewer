//! Page-range work decodes the registry once per call, not once per page
//! (M22). Each recompute rebuilds the segments of every loaded page, as the
//! TypeScript's does, so a select-all that settles page by page is
//! O(pages^2); before the registry was decoded once it was O(pages^3), and
//! 400 pages took minutes.
//!
//! The guard is a growth bound (Ruling13.15): select-all plus `read_text` at
//! 400 pages may take at most 6 times as long as at 200. Quadratic work
//! comes out near 4, cubic near 8. An absolute budget measured the kernel's
//! page mirror too, whose `PageMirror::write` copies every entry on each
//! write (carried to Task 14). A 10 s ceiling at 400 pages still catches a
//! hang.

mod common;

use std::time::{Duration, Instant};

use epdf_kernel::rt;
use epdf_plugin_selection::*;

use common::{BOTH, kernel, selection};

/// Select every page, settle, and read the text back; the time it took.
fn select_all_and_read(pages: u32) -> Duration {
    let (kernel, _) = kernel(pages, BOTH, Vec::new());
    let selection = selection(&kernel);
    let started = Instant::now();
    selection.select_all().unwrap();
    rt::run_until_stalled();

    let snapshot = selection.get_snapshot();
    assert_eq!(snapshot.pages.len(), pages as usize, "every page has its segments");
    let range = snapshot.range.unwrap();
    assert_eq!(range.end.page.page_object_number, pages);
    assert_eq!(range.end.index, 3);

    let text = selection.read_text(OperationOptions::default());
    rt::run_until_stalled();
    let text = text.peek().expect("settled").expect("read");
    assert_eq!(text, vec!["abc"; pages as usize].join("\n"));
    started.elapsed()
}

#[test]
fn select_all_grows_quadratically_with_the_page_count() {
    let small = select_all_and_read(200);
    let large = select_all_and_read(400);
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(ratio < 6.0, "200 pages in {small:?}, 400 in {large:?}: {ratio:.1}x");
    assert!(large < Duration::from_secs(10), "400 pages took {large:?}");
}
