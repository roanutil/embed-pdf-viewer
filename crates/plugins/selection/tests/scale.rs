//! Page-range work decodes the registry once per call, not once per page
//! (M22): a select-all over a long document settles, and its text reads,
//! in well under a second.

mod common;

use std::time::Instant;

use epdf_kernel::rt;
use epdf_plugin_selection::*;

use common::{BOTH, kernel, selection};

/// Each recompute rebuilds the segments of every loaded page, as the
/// TypeScript's does, so a select-all settles in O(pages^2). Before the
/// registry was decoded once, it was O(pages^3) and this took minutes.
const PAGES: u32 = 400;

#[test]
fn select_all_over_a_long_document_settles_fast() {
    let (kernel, _) = kernel(PAGES, BOTH, Vec::new());
    let selection = selection(&kernel);
    let started = Instant::now();
    selection.select_all().unwrap();
    rt::run_until_stalled();
    let settled = started.elapsed();

    let snapshot = selection.get_snapshot();
    assert_eq!(snapshot.pages.len(), PAGES as usize, "every page has its segments");
    let range = snapshot.range.unwrap();
    assert_eq!(range.end.page.page_object_number, PAGES);
    assert_eq!(range.end.index, 3);

    let text = selection.read_text(OperationOptions::default());
    rt::run_until_stalled();
    let text = text.peek().expect("settled").expect("read");
    assert_eq!(text, vec!["abc"; PAGES as usize].join("\n"));
    let elapsed = started.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "settled in {settled:?}, read in {elapsed:?}");
}
