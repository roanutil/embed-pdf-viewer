//! One registry row that fails to decode costs only its own page (S1), as
//! `pages[i]?.ref` does in the TypeScript: the other pages still select and
//! read.

mod common;

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

use common::{layout, page_row, run};

/// Three pages answering "abc", page 2's row with a ref of no kind.
fn kernel() -> Kernel {
    rt::reset();
    let pages: Vec<Value> = (0..3u32)
        .map(|i| {
            let mut row = page_row(i, i + 1);
            if i == 1 {
                row["ref"] = json!({ "objectNumber": 2 });
            }
            row
        })
        .collect();
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": pages,
            "security": { "allows": ["doc.text.select", "doc.text.copy"] },
            "responses": [
                { "path": "page.text.get", "value": { "text": "abc", "charCount": 3 } },
                { "path": "page.text.layout", "value": layout(vec![run(0, 3, 50.0, 50.0)]) }
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
    let _ = kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    kernel
}

#[test]
fn a_page_beside_a_malformed_row_still_selects() {
    let kernel = kernel();
    let selection = common::selection(&kernel);
    let _ = selection.ensure_loaded(PageRef::new(1));
    rt::run_until_stalled();
    selection.select_page(PageRef::new(1)).unwrap();
    rt::run_until_stalled();
    assert_eq!(selection.list_selected_pages(), vec![PageRef::new(1)]);
    assert!(!selection.list_segments(PageRef::new(1)).is_empty());
}

#[test]
fn a_read_across_a_malformed_row_reads_every_other_page() {
    let kernel = kernel();
    let selection = common::selection(&kernel);
    let range = TextRange {
        start: TextPosition { page: PageRef::new(1), index: 0 },
        end: TextPosition { page: PageRef::new(3), index: 3 },
    };
    let read = selection.read_text_in_range(range, OperationOptions::default());
    rt::run_until_stalled();
    assert_eq!(read.peek().expect("settled").expect("read"), "abc\nabc");
}
