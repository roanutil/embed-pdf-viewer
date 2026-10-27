//! A text read that a content change overtakes fails instead of returning
//! the text from before the change (M23). The TypeScript returns that text
//! (the race is shared, `controller.ts` `readRange`); this refusal is the
//! Rust's own, and the vectors never race a read against a change.

mod common;

use std::rc::Rc;

use epdf_kernel::error::code;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Kernel, rt};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::FakeEngine;
use serde_json::{Value, json};

use common::{BOTH, selection};

const PAGES: u32 = 10;

/// Ten pages whose text reads answer "abc", except page `deferred`'s first
/// read, which waits for the label "late".
fn kernel(deferred: u32) -> (Kernel, Rc<FakeEngine>) {
    common::kernel(
        PAGES,
        BOTH,
        vec![json!({ "path": "page.text.get", "pon": deferred, "defer": "late", "times": 1 })],
    )
}

fn read_all(kernel: &Kernel) -> Promise<String> {
    let range = TextRange {
        start: TextPosition { page: PageRef::new(1), index: 0 },
        end: TextPosition { page: PageRef::new(PAGES), index: 3 },
    };
    let read = selection(kernel).read_text_in_range(range, OperationOptions::default());
    rt::run_until_stalled();
    read
}

fn finish(fake: &FakeEngine, read: &Promise<String>) -> Result<String, epdf_kernel::Error> {
    assert!(fake.settle("late", Ok(json!({ "text": "secret", "charCount": 6 }))));
    rt::run_until_stalled();
    read.peek().expect("settled")
}

/// A confirmed event that changed page 1's content.
fn changed(kind: &str) -> Value {
    json!({ "type": kind, "results": [
        { "page": { "kind": "objectNumber", "objectNumber": 1 }, "status": "applied" }
    ] })
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
    fake.emit("a", &changed("pages.flattened")).unwrap();
    let error = finish(&fake, &read).expect_err("stale");
    assert_eq!(error.code(), Some(code::CONFLICT));
}

#[test]
fn a_change_during_a_later_batch_fails_the_read() {
    // Pages 1 to 8 are the first batch; page 9 is in the second.
    let (kernel, fake) = kernel(9);
    let read = read_all(&kernel);
    fake.emit("a", &changed("redaction.applied")).unwrap();
    let error = finish(&fake, &read).expect_err("stale");
    assert_eq!(error.code(), Some(code::CONFLICT));
}

#[test]
fn an_event_that_changed_no_content_leaves_the_read_alone() {
    let (kernel, fake) = kernel(1);
    let read = read_all(&kernel);
    fake.emit("a", &json!({ "type": "pages.flattened", "results": [] })).unwrap();
    assert!(finish(&fake, &read).expect("read").starts_with("secret\nabc"));
}
