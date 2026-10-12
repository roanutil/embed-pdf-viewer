//! A text read that a content change overtakes fails instead of returning
//! the text from before the change. The TypeScript returns that text (the
//! race is shared, controller.ts readTextInRange); this refusal is the
//! Rust's own, and the vectors never race a read against a change.

use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

const PAGES: u32 = 10;

/// Ten pages whose text reads answer "abc", except page `deferred`'s first
/// read, which waits for the label "late".
fn kernel(deferred: u32) -> (Kernel, Rc<FakeEngine>) {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let pages: Vec<Value> = (0..PAGES)
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
                { "path": "page.text.read", "pon": deferred, "defer": "late", "times": 1 },
                { "path": "page.text.read", "value": { "text": "abc", "charCount": 3 } }
            ]
        } }
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
    let _ = kernel
        .documents()
        .open(OpenSource::Input(json!({ "kind": "bytes", "id": "a" })), OpenOptions::default());
    rt::run_until_stalled();
    (kernel, fake)
}

fn read_all(kernel: &Kernel) -> Promise<String> {
    let selection = kernel.capability(&SELECTION, Some("a")).unwrap();
    let range = TextRange {
        start: TextPosition { page: PageRef::new(1), index: 0 },
        end: TextPosition { page: PageRef::new(PAGES), index: 3 },
    };
    let read = selection.read_text_in_range(range, OperationOptions::default());
    rt::run_until_stalled();
    read
}

fn finish(fake: &FakeEngine, read: &Promise<String>) -> Result<String, epdf_kernel::Error> {
    assert!(fake.settle("late", Ok(json!({ "text": "secret", "charCount": 6 }))));
    rt::run_until_stalled();
    read.peek().expect("settled")
}

#[test]
fn a_read_with_no_change_returns_its_text() {
    let (kernel, fake) = kernel(1);
    let read = read_all(&kernel);
    let text = finish(&fake, &read).expect("read");
    assert!(text.starts_with("secret\nabc"));
}

#[test]
fn a_change_during_the_first_batch_fails_the_read() {
    let (kernel, fake) = kernel(1);
    let read = read_all(&kernel);
    fake.emit("a", &json!({ "type": "pages.flattened" })).unwrap();
    let error = finish(&fake, &read).expect_err("stale");
    assert_eq!(error.code(), Some(code::CONFLICT));
}

#[test]
fn a_change_during_a_later_batch_fails_the_read() {
    // Pages 1 to 8 are the first batch; page 9 is in the second.
    let (kernel, fake) = kernel(9);
    let read = read_all(&kernel);
    fake.emit("a", &json!({ "type": "redaction.applied" })).unwrap();
    let error = finish(&fake, &read).expect_err("stale");
    assert_eq!(error.code(), Some(code::CONFLICT));
}
