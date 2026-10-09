//! Page-range work decodes the registry once per call, not once per page:
//! a select-all over a long document settles, and its text reads, in well
//! under a second.

use std::rc::Rc;
use std::time::Instant;

use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

/// Each recompute rebuilds the segments of every loaded page, as the
/// TypeScript's does, so a select-all settles in O(pages^2). Before the
/// registry was decoded once, it was O(pages^3) and this took minutes.
const PAGES: u32 = 400;

/// `PAGES` pages, each "abc" at x 50..68.
fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let glyph = |left: f64| json!({ "looseBox": { "left": left, "bottom": 738, "right": left + 6.0, "top": 750 }, "flags": 0 });
    let pages: Vec<_> = (0..PAGES)
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
            "security": { "allows": ["doc.text.select", "doc.text.copy"] },
            "responses": [
                { "path": "page.geometry.read", "value": { "runs": [{
                    "rect": { "left": 50, "bottom": 738, "right": 68, "top": 750 },
                    "charStart": 0,
                    "glyphs": [glyph(50.0), glyph(56.0), glyph(62.0)]
                }] } },
                { "path": "page.text.read", "value": { "text": "abc", "charCount": 3 } }
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
fn select_all_over_a_long_document_settles_fast() {
    let kernel = kernel();
    let selection = kernel.capability(&SELECTION, Some("a")).unwrap();
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
