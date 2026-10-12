//! `select` with extreme `i64` positions saturates instead of overflowing
//! (the TypeScript's float arithmetic never overflows either).

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let pages: Vec<Value> = (0..2u32)
        .map(|i| {
            json!({
                "index": i, "ref": { "kind": "objectNumber", "pageObjectNumber": i + 1 }, "label": null,
                "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ }
            })
        })
        .collect();
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": pages,
            "security": { "allows": ["doc.text.select"] },
            "responses": [{ "path": "page.geometry.read", "value": { "runs": [] } }]
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
fn a_span_whose_end_passes_i64_saturates() {
    let kernel = kernel();
    let selection = kernel.capability(&SELECTION, Some("a")).unwrap();
    let page = PageRef::new(1);
    selection.select(SelectionRangeInput::Span { page, start: 1, count: i64::MAX }).unwrap();
    rt::run_until_stalled();
    assert!(selection.has_selection());
}

#[test]
fn a_range_ending_at_i64_min_on_a_later_page_saturates() {
    let kernel = kernel();
    let selection = kernel.capability(&SELECTION, Some("a")).unwrap();
    let range = TextRange {
        start: TextPosition { page: PageRef::new(1), index: 0 },
        end: TextPosition { page: PageRef::new(2), index: i64::MIN },
    };
    selection.select(SelectionRangeInput::Range(range)).unwrap();
    rt::run_until_stalled();
}
