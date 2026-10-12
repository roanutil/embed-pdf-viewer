//! `buildCommentThreads` stays linear in the annotation count: key lookups,
//! child lists and the visited set are hashed, as the TypeScript's `Map`
//! and `Set` are.

use std::time::Instant;

use epdf_plugin_annotation::comments::build::build_comment_threads;
use serde_json::{Value, json};

fn dto(n: usize, irt: Option<usize>) -> Value {
    let r = |i: usize| json!({ "kind": "objectNumber", "page": { "pageObjectNumber": 1 }, "annotObjectNumber": i });
    let mut a = json!({
        "subtype": "text",
        "ref": r(n),
        "page": { "pageObjectNumber": 1 },
        "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 },
        "index": n,
    });
    if let Some(parent) = irt {
        a["inReplyTo"] = r(parent);
    }
    a
}

#[test]
fn threading_a_large_document_runs_in_linear_time() {
    const ROOTS: usize = 2_000;
    const REPLIES: usize = 4;
    let mut annotations = Vec::new();
    for t in 0..ROOTS {
        let root = t * (REPLIES + 1);
        annotations.push(dto(root, None));
        // A reply chain: each reply answers the one before it.
        for k in 1..=REPLIES {
            annotations.push(dto(root + k, Some(root + k - 1)));
        }
    }

    let start = Instant::now();
    let threads = build_comment_threads(&annotations, None);
    let elapsed = start.elapsed();

    assert_eq!(threads.len(), ROOTS);
    assert!(threads.iter().all(|t| t.replies.len() == REPLIES));
    assert_eq!(threads[1].root["ref"]["annotObjectNumber"], json!(REPLIES + 1));
    assert!(elapsed.as_millis() < 1000, "threading took {elapsed:?}");
}
