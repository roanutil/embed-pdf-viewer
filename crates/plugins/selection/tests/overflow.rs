//! `select` with extreme `i64` positions saturates instead of overflowing
//! (S3; the TypeScript's float arithmetic never overflows either).

mod common;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt;
use epdf_plugin_selection::*;

use common::{kernel, selection};

#[test]
fn a_span_whose_end_passes_i64_saturates() {
    let (kernel, _) = kernel(2, &["doc.text.select"], Vec::new());
    let selection = selection(&kernel);
    let page = PageRef::new(1);
    selection.select(SelectionRangeInput::Span { page, start: 1, count: i64::MAX }).unwrap();
    rt::run_until_stalled();
    assert!(selection.has_selection());
}

#[test]
fn a_range_ending_at_i64_min_on_a_later_page_saturates() {
    let (kernel, _) = kernel(2, &["doc.text.select"], Vec::new());
    let selection = selection(&kernel);
    let range = TextRange {
        start: TextPosition { page: PageRef::new(1), index: 0 },
        end: TextPosition { page: PageRef::new(2), index: i64::MIN },
    };
    selection.select(SelectionRangeInput::Range(range)).unwrap();
    rt::run_until_stalled();
    // The focus, `i64::MIN - 1`, saturates at `i64::MIN` and clamps to glyph 0 of
    // page 2 once its geometry loads: page 1 whole, and page 2's first glyph.
    let range = selection.get_range().expect("a selection");
    assert_eq!((range.start.page, range.start.index), (PageRef::new(1), 0));
    assert_eq!((range.end.page, range.end.index), (PageRef::new(2), 1));
}
