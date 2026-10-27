//! Page-range work decodes the registry once per page list, not once per
//! page (M22). Each recompute rebuilds the segments of every loaded page,
//! as the TypeScript's does, so a select-all that settles page by page is
//! O(pages^2); while every page lookup re-decoded the registry it was
//! O(pages^3), and 400 pages took minutes. `read_model` had a second path
//! of the same kind.
//!
//! The guard counts decodes rather than timing them (Ruling13.15, revised
//! after review): a 400-page select-all and its text read must decode the
//! registry a small constant number of times. A 10 s ceiling still catches
//! a hang.

mod common;

use std::time::{Duration, Instant};

use epdf_kernel::rt;
use epdf_plugin_selection::controller::registry_decodes;
use epdf_plugin_selection::*;

use common::{BOTH, kernel, selection};

const PAGES: u32 = 400;

#[test]
fn a_long_select_all_and_read_decode_the_registry_a_constant_number_of_times() {
    let (kernel, _) = kernel(PAGES, BOTH, Vec::new());
    let selection = selection(&kernel);
    let started = Instant::now();
    let before = registry_decodes();
    selection.select_all().unwrap();
    rt::run_until_stalled();

    let snapshot = selection.get_snapshot();
    assert_eq!(snapshot.pages.len(), PAGES as usize, "every page has its segments");
    let range = snapshot.range.unwrap();
    assert_eq!(range.end.page.page_object_number, PAGES);
    assert_eq!(range.end.index, 3);

    let text = selection.read_text(OperationOptions::default());
    rt::run_until_stalled();
    let text = text.peek().expect("settled").expect("read");
    assert_eq!(text, vec!["abc"; PAGES as usize].join("\n"));

    let decodes = registry_decodes() - before;
    assert!(decodes <= 2, "decoded the registry {decodes} times for {PAGES} pages");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(10), "{PAGES} pages took {elapsed:?}");
}
