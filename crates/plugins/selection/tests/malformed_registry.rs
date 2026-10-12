//! One registry row that fails to decode costs only its own page, as
//! `pages[i]?.ref.pageObjectNumber` does in the TypeScript: the other
//! pages still select and read.

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

/// Three pages answering "abc", page 2's row with no `boxes`.
fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let glyph = |left: f64| json!({ "looseBox": { "left": left, "bottom": 738, "right": left + 6.0, "top": 750 }, "flags": 0 });
    let pages: Vec<Value> = (0..3u32)
        .map(|i| {
            let mut row = json!({
                "index": i, "ref": { "kind": "objectNumber", "pageObjectNumber": i + 1 }, "label": null,
                "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ }
            });
            if i == 1 {
                row.as_object_mut().unwrap().remove("boxes");
            }
            row
        })
        .collect();
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": pages,
            "security": { "allows": ["doc.text.select", "doc.text.copy"] },
            "responses": [
                { "path": "page.text.read", "value": { "text": "abc", "charCount": 3 } },
                { "path": "page.geometry.read", "value": { "runs": [{
                    "rect": { "left": 50, "bottom": 738, "right": 68, "top": 750 },
                    "charStart": 0,
                    "glyphs": [glyph(50.0), glyph(56.0), glyph(62.0)]
                }] } }
            ]
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![
            interaction_plugin(InteractionConfig::default()),
            selection_plugin(SelectionConfig::default()),
        ],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel
        .documents()
        .open(OpenSource::Input(json!({ "kind": "bytes", "id": "a" })), OpenOptions::default());
    rt::run_until_stalled();
    kernel
}

#[test]
fn a_page_beside_a_malformed_row_still_selects() {
    let kernel = kernel();
    let selection = kernel.capability(&SELECTION, Some("a")).unwrap();
    let _ = selection.ensure_loaded(PageRef::new(1));
    rt::run_until_stalled();
    selection.select_page(PageRef::new(1)).unwrap();
    rt::run_until_stalled();
    assert_eq!(selection.list_selected_pages(), vec![PageRef::new(1)]);
    assert!(!selection.list_segments(PageRef::new(1)).is_empty());
}

#[test]
fn a_read_across_a_malformed_row_reads_every_page() {
    let kernel = kernel();
    let selection = kernel.capability(&SELECTION, Some("a")).unwrap();
    let range = TextRange {
        start: TextPosition { page: PageRef::new(1), index: 0 },
        end: TextPosition { page: PageRef::new(3), index: 3 },
    };
    let read = selection.read_text_in_range(range, OperationOptions::default());
    rt::run_until_stalled();
    assert_eq!(read.peek().expect("settled").expect("read"), "abc\nabc\nabc");
}
