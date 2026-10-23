//! A kernel with the interaction hub and the selection plugin over the
//! conformance fake engine, for the behavioral tests. Pages are 600 by 800,
//! their geometry answered as `page.text.layout` (`$textLayout`, page space)
//! and their text as `page.text.get`.

#![allow(dead_code)]

use std::rc::Rc;

use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

/// A registry row for page `pon` at `index`: the members the plugin reads
/// and a few more. The fake engine copies every row on each call, so a long
/// document's rows stay small.
pub fn page_row(index: u32, pon: u32) -> Value {
    json!({
        "index": index, "ref": { "kind": "objectNumber", "objectNumber": pon }, "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1
    })
}

/// One upright run of `count` 6-wide glyphs from `x`, top at `y`, 12 high,
/// starting at character `start`.
pub fn run(start: u32, count: u32, x: f64, y: f64) -> Value {
    let glyphs: Vec<Value> = (0..count)
        .map(|i| {
            let left = x + 6.0 * f64::from(i);
            json!({ "loose": { "x": left, "y": y, "width": 6, "height": 12 } })
        })
        .collect();
    json!({
        "rect": { "x": x, "y": y, "width": 6.0 * f64::from(count), "height": 12 },
        "start": start, "fontSize": 12, "glyphs": glyphs
    })
}

/// A `page.text.layout` answer for `runs`.
pub fn layout(runs: Vec<Value>) -> Value {
    json!({ "$textLayout": { "runs": runs } })
}

/// `pages` pages answering `abc` at x 50..68, y 50..62, plus `responses`
/// tried first.
pub fn kernel(pages: u32, allows: &[&str], responses: Vec<Value>) -> (Kernel, Rc<FakeEngine>) {
    rt::reset();
    let rows: Vec<Value> = (0..pages).map(|i| page_row(i, i + 1)).collect();
    let mut all = responses;
    all.push(json!({ "path": "page.text.layout", "value": layout(vec![run(0, 3, 50.0, 50.0)]) }));
    all.push(json!({ "path": "page.text.get", "value": { "text": "abc", "charCount": 3 } }));
    let fake = FakeEngine::new(json!({
        "documents": { "a": { "pages": rows, "security": { "allows": allows }, "responses": all } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake.clone())),
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
    (kernel, fake)
}

pub const BOTH: &[&str] = &["doc.text.select", "doc.text.copy"];

pub fn selection(kernel: &Kernel) -> SelectionCapability {
    kernel.capability(&SELECTION, Some("a")).expect("the selection capability")
}
