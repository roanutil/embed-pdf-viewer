//! The controller's behaviour the brief names, through the typed API: a
//! hit on an edge two characters share, a selection across pages whose
//! geometry arrives late, the permission twins, a read cut short by the
//! document closing, and a page with no text.

mod common;

use epdf_geometry::Point;
use epdf_kernel::error::code;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt;
use epdf_plugin_selection::*;
use serde_json::json;

use common::{BOTH, kernel, layout, run, selection};

fn page(n: u32) -> PageRef {
    PageRef::new(n)
}

fn code_of<T: std::fmt::Debug>(result: Result<T, epdf_kernel::Error>) -> Option<String> {
    result.expect_err("refused").code().map(str::to_string)
}

#[test]
fn a_point_on_an_edge_two_characters_share_hits_the_first() {
    let (kernel, _) = kernel(1, BOTH, Vec::new());
    let selection = selection(&kernel);
    let _ = selection.ensure_loaded(page(1));
    rt::run_until_stalled();
    // "abc" at x 50..68: x 56 is the edge glyphs 0 and 1 share. Just above
    // the line, no tight box holds the point, so the near-miss pass decides,
    // and both glyphs are as near.
    assert!(selection.begin_gesture_at(page(1), Point { x: 56.0, y: 49.5 }));
    let range = selection.get_range().unwrap();
    assert_eq!((range.start.index, range.end.index), (0, 1));
}

#[test]
fn a_selection_across_pages_fills_in_as_their_geometry_arrives() {
    let (kernel, fake) = kernel(
        3,
        BOTH,
        vec![json!({ "path": "page.text.layout", "pon": 2, "defer": "two", "times": 1 })],
    );
    let selection = selection(&kernel);
    let _ = selection.ensure_loaded(page(1));
    rt::run_until_stalled();
    let range = TextRange {
        start: TextPosition { page: page(1), index: 1 },
        end: TextPosition { page: page(3), index: 2 },
    };
    selection.select(SelectionRangeInput::Range(range)).unwrap();
    rt::run_until_stalled();
    // Page 1 had geometry; pages 2 and 3 were warmed, and 3 has arrived.
    assert_eq!(selection.list_selected_pages(), vec![page(1), page(3)]);
    assert!(!selection.is_loaded(page(2)));
    assert!(fake.settle("two", Ok(layout(vec![run(0, 3, 50.0, 50.0)]))));
    rt::run_until_stalled();
    assert_eq!(selection.list_selected_pages(), vec![page(1), page(2), page(3)]);
    let snapshot = selection.get_snapshot();
    assert_eq!(snapshot.direction, SelectionDirection::Forward);
    assert_eq!(snapshot.range, Some(range));
    // Glyph 1's cell on page 1, glyph 1's on page 3 (the end is exclusive).
    assert_eq!(snapshot.start.unwrap().rect.x, 56.0);
    assert_eq!(snapshot.end.unwrap().rect.x, 56.0);
    let text = selection.read_text(OperationOptions::default());
    rt::run_until_stalled();
    assert_eq!(text.peek().unwrap().unwrap(), "bc\nabc\nab");
}

#[test]
fn neither_permission_implies_the_other() {
    let (kernel, fake) = kernel(1, &["doc.text.copy"], Vec::new());
    let selection = selection(&kernel);
    assert!(!selection.can_select() && selection.can_copy());
    let warm = selection.ensure_loaded(page(1));
    rt::run_until_stalled();
    assert!(warm.peek().unwrap().is_ok(), "warming resolves without the read");
    assert!(
        fake.log_since(0).iter().all(|call| call["path"] != "page.text.layout"),
        "no geometry read is issued without doc.text.select"
    );
    let refused = selection.select(SelectionRangeInput::Span { page: page(1), start: 0, count: 1 });
    assert_eq!(code_of(refused).as_deref(), Some(code::PERMISSION_DENIED));
    assert_eq!(code_of(selection.select_all()).as_deref(), Some(code::PERMISSION_DENIED));
    assert!(!selection.begin_gesture_at(page(1), Point { x: 53.0, y: 56.0 }));
    let range = TextRange {
        start: TextPosition { page: page(1), index: 0 },
        end: TextPosition { page: page(1), index: 2 },
    };
    let text = selection.read_text_in_range(range, OperationOptions::default());
    rt::run_until_stalled();
    assert_eq!(text.peek().unwrap().unwrap(), "ab", "copy works with doc.text.copy alone");

    let (select_only, _) = common::kernel(1, &["doc.text.select"], Vec::new());
    let selection = common::selection(&select_only);
    let read = selection.read_text(OperationOptions::default());
    rt::run_until_stalled();
    assert_eq!(code_of(read.peek().unwrap()).as_deref(), Some(code::PERMISSION_DENIED));
}

#[test]
fn a_page_that_is_not_in_the_document_is_refused_by_verbs_and_empty_to_reads() {
    let (kernel, _) = kernel(1, BOTH, Vec::new());
    let selection = selection(&kernel);
    assert_eq!(code_of(selection.select_page(page(7))).as_deref(), Some(code::NOT_FOUND));
    let by_index = selection.select_page(PageTarget::Index(3.0)).expect_err("no page 3");
    assert_eq!(by_index.message, "there is no page 3 in this document");
    assert!(selection.list_segments(page(7)).is_empty());
    assert!(selection.list_rects(PageTarget::Index(1.5)).is_empty());
    selection.select_page(0).unwrap();
    rt::run_until_stalled();
    assert_eq!(selection.list_segments(0).len(), 1, "an index addresses the page list");
}

#[test]
fn closing_the_document_ends_a_read_in_flight() {
    let (kernel, fake) = kernel(
        1,
        BOTH,
        vec![json!({ "path": "page.text.get", "pon": 1, "defer": "text", "times": 1 })],
    );
    let selection = selection(&kernel);
    let range = TextRange {
        start: TextPosition { page: page(1), index: 0 },
        end: TextPosition { page: page(1), index: 3 },
    };
    let read = selection.read_text_in_range(range, OperationOptions::default());
    rt::run_until_stalled();
    assert!(read.peek().is_none(), "the read waits on the engine");
    let _ = kernel.documents().close("a");
    rt::run_until_stalled();
    let _ = fake.settle("text", Ok(json!({ "text": "abc", "charCount": 3 })));
    rt::run_until_stalled();
    let error = read.peek().expect("settled").expect_err("the instance closed");
    assert_eq!(error.code(), Some(code::INSTANCE_CLOSED), "{error:?}");
}

#[test]
fn a_page_with_no_text_selects_nothing_and_reads_empty() {
    let (kernel, _) = kernel(
        1,
        BOTH,
        vec![
            json!({ "path": "page.text.layout", "value": layout(Vec::new()) }),
            json!({ "path": "page.text.get", "value": { "text": "", "charCount": 0 } }),
        ],
    );
    let selection = selection(&kernel);
    let _ = selection.ensure_loaded(page(1));
    rt::run_until_stalled();
    assert!(selection.is_loaded(page(1)));
    assert!(!selection.is_over_text(page(1), Point { x: 53.0, y: 56.0 }));
    assert_eq!(selection.select_word_at(page(1), Point { x: 53.0, y: 56.0 }), Ok(false));
    selection.select_page(page(1)).unwrap();
    rt::run_until_stalled();
    // The open end clamps to glyph 0 of no glyphs: a range, with no segment.
    assert!(selection.has_selection());
    assert!(selection.list_selected_pages().is_empty());
    assert_eq!(selection.get_anchor(), None);
    let text = selection.read_text(OperationOptions::default());
    rt::run_until_stalled();
    assert_eq!(text.peek().unwrap().unwrap(), "");
}
