#![cfg(feature = "uniffi")]

use epdf_ffi::PageRef;
use epdf_plugin_selection::ffi::{SelectionInput, SelectionTextPosition, SelectionTextRange};
use epdf_plugin_selection::{SelectionRangeInput, TextRange};

const PAGE_1: PageRef = PageRef { page_object_number: 1 };

#[test]
fn a_range_and_a_span_round_trip() {
    let range = SelectionTextRange {
        start: SelectionTextPosition { page: PAGE_1, index: -3 },
        end: SelectionTextPosition { page: PAGE_1, index: 9_007_199_254_740_991 },
    };
    let core = TextRange::from(range);
    assert_eq!(core.start.index, -3);
    assert_eq!(SelectionTextRange::from(core), range);
    let span = SelectionInput::Span { page: PAGE_1, start: 2, count: 5 };
    assert!(matches!(
        SelectionRangeInput::from(span),
        SelectionRangeInput::Span { start: 2, count: 5, .. }
    ));
}
