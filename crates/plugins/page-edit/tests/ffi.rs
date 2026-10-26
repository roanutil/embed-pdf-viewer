#![cfg(feature = "uniffi")]
//! The uniffi handle over the current page-edit contract. Migrated by hand
//! in Task 13.19 against the change-queue wire (`apply` with page ops); not
//! compiled until `epdf-ffi` builds again (13.33).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{CancelToken, KernelThread, PageRef, Size, ViewerError};
use epdf_plugin_page_edit::ffi::{
    PageEditBlankOptions, PageEditHandle, PageEditPlacement, PageEditRotation, PageEditTurn,
};
use epdf_plugin_page_edit::{PAGE_EDIT, page_edit_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

/// A current `PageLayout`, 612 by 792 points.
fn page(pon: u32, index: usize, rotation: u32) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": 612.0, "height": 792.0 });
    json!({
        "index": index,
        "ref": { "kind": "objectNumber", "objectNumber": pon },
        "label": null,
        "size": { "width": 612.0, "height": 792.0 },
        "rotation": rotation,
        "userUnit": 1,
        "tabOrder": null,
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b },
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 612.0, "top": 792.0 },
    })
}

fn pon(page: &Value) -> u64 {
    page["ref"]["objectNumber"].as_u64().unwrap()
}

fn pons(op: &Value) -> Vec<u64> {
    op["pages"].as_array().unwrap().iter().map(|r| r["objectNumber"].as_u64().unwrap()).collect()
}

const META: fn(&Value) -> Value = |args| json!({ "affectedPages": [], "cacheDelta": null, "opId": args[1]["opId"], "undoable": true });

/// What the engine answers one page op and the fact it publishes first.
fn applied(args: &[Value], op: &Value, pages: &[Value], inserted: Option<Vec<u32>>) -> Reply {
    let fact_type = match op["type"].as_str().unwrap() {
        "pages.rotate" => "pages.rotated",
        "pages.reorder" => "pages.reordered",
        "pages.delete" => "pages.deleted",
        _ => "pages.inserted",
    };
    let mut item = op.clone();
    item["layout"] = json!({ "pageCount": pages.len(), "pages": pages, "namedPages": [] });
    item["meta"] = META(args);
    if let Some(inserted) = inserted {
        item["insertedPages"] =
            inserted.iter().map(|p| json!({ "kind": "objectNumber", "objectNumber": p })).collect();
    }
    let mut fact = item.clone();
    fact["type"] = json!(fact_type);
    fact["origin"] =
        json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null });
    Reply { value: json!({ "items": [item], "meta": META(args) }), events: vec![fact] }
}

/// Three pages, 10, 11 and 12, and no responders yet. The first responder
/// for a path answers it.
fn engine(allows: Value) -> TestSpec {
    TestSpec::new(json!({
        "documents": { DOCUMENT: {
            "id": DOCUMENT, "security": { "allows": allows },
            "pages": [page(10, 0, 0), page(11, 1, 0), page(12, 2, 0)]
        } }
    }))
    .respond("pages.extract", |_| Ok(Reply::value(json!({ "$bytes": "JVBERi0=" }))))
}

/// [`engine`], whose `apply` runs each change's one page op on a shared
/// layout, publishes the fact a real engine publishes and answers the
/// change. An inserted blank page is page 20.
fn spec(allows: Value) -> TestSpec {
    let layout = Rc::new(RefCell::new(vec![page(10, 0, 0), page(11, 1, 0), page(12, 2, 0)]));
    engine(allows).respond("apply", move |args| {
        let op = args[0]["ops"][0].clone();
        let targets = || pons(&op);
        let mut pages = layout.borrow_mut();
        let mut inserted = None;
        match op["type"].as_str().unwrap() {
            "pages.rotate" => {
                for p in pages.iter_mut().filter(|p| targets().contains(&pon(p))) {
                    p["rotation"] = op["rotation"].clone();
                }
            }
            "pages.delete" => pages.retain(|p| !targets().contains(&pon(p))),
            "pages.reorder" => {
                let (mut moved, mut kept): (Vec<_>, Vec<_>) =
                    pages.drain(..).partition(|p| targets().contains(&pon(p)));
                assert_eq!(op["position"], json!("end"), "the test engine moves to the end only");
                kept.append(&mut moved);
                *pages = kept;
            }
            "pages.insertBlank" => {
                let index = pages.len();
                pages.push(page(20, index, 0));
                inserted = Some(vec![20]);
            }
            other => panic!("unexpected op {other}"),
        }
        for (i, p) in pages.iter_mut().enumerate() {
            p["index"] = json!(i);
        }
        Ok(applied(args, &op, &pages, inserted))
    })
}

fn handle_over(
    spec: impl FnOnce() -> TestSpec + Send + 'static,
) -> (KernelThread<TestKernel>, Arc<PageEditHandle>) {
    let thread =
        KernelThread::spawn(|_| TestKernel::open(vec![page_edit_plugin()], spec())).unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| PageEditHandle::new(k.capability(&PAGE_EDIT).unwrap(), post)),
    )
    .unwrap();
    (thread, handle)
}

fn handle() -> (KernelThread<TestKernel>, Arc<PageEditHandle>) {
    handle_over(|| spec(json!("*")))
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

const P10: PageRef = PageRef { page_object_number: 10 };
const P11: PageRef = PageRef { page_object_number: 11 };

#[test]
fn can_edit_follows_the_assemble_grant() {
    let (_thread, edit) = handle();
    assert!(block_on(edit.can_edit()).unwrap());
    let (_thread, denied) = handle_over(|| spec(json!(["doc.text.search"])));
    assert!(!block_on(denied.can_edit()).unwrap());
}

#[test]
fn set_rotation_answers_the_new_layout() {
    let (_thread, edit) = handle();
    let layout =
        block_on(edit.set_rotation(vec![P10], PageEditRotation::Clockwise90, token())).unwrap();
    assert_eq!(layout.pages.len(), 3);
    assert_eq!(layout.pages[0].page, P10);
    assert_eq!(layout.pages[0].rotation, 90);
    assert_eq!(layout.pages[1].rotation, 0);
}

#[test]
fn rotate_by_turns_from_the_rotation_the_last_edit_left() {
    let (_thread, edit) = handle();
    block_on(edit.rotate_by(vec![P10], PageEditTurn::Clockwise, token())).unwrap();
    let layout = block_on(edit.rotate_by(vec![P10], PageEditTurn::Clockwise, token())).unwrap();
    assert_eq!(layout.pages[0].rotation, 180);
    let layout =
        block_on(edit.rotate_by(vec![P10], PageEditTurn::CounterClockwise, token())).unwrap();
    assert_eq!(layout.pages[0].rotation, 90);
}

#[test]
fn rotate_by_with_no_pages_is_invalid_input() {
    let (_thread, edit) = handle();
    let error = block_on(edit.rotate_by(vec![], PageEditTurn::Half, token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn delete_pages_answers_the_survivors() {
    let (_thread, edit) = handle();
    let layout = block_on(edit.delete_pages(vec![P11], token())).unwrap();
    let pons: Vec<u32> = layout.pages.iter().map(|p| p.page.page_object_number).collect();
    assert_eq!(pons, [10, 12]);
}

#[test]
fn insert_blank_answers_the_inserted_pages_and_the_layout() {
    let (_thread, edit) = handle();
    let options = PageEditBlankOptions {
        count: Some(1),
        size: Some(Size { width: 300.0, height: 400.0 }),
        placement: Some(PageEditPlacement::End),
    };
    let insertion = block_on(edit.insert_blank(options, token())).unwrap();
    assert_eq!(insertion.inserted, [PageRef { page_object_number: 20 }]);
    assert_eq!(insertion.pages.len(), 4);
}

#[test]
fn a_placement_after_an_unknown_page_is_not_found() {
    let (_thread, edit) = handle();
    let options = PageEditBlankOptions {
        placement: Some(PageEditPlacement::After { page: PageRef { page_object_number: 99 } }),
        ..Default::default()
    };
    let error = block_on(edit.insert_blank(options, token())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn extract_answers_the_pdf_bytes() {
    let (_thread, edit) = handle();
    assert_eq!(block_on(edit.extract(vec![P10], token())).unwrap(), b"%PDF-".to_vec());
}

/// Was `end_moves_the_block_after_the_pages_left_behind`, which checked the
/// index the pre-13.19 port computed for `End` (the engine counted it once
/// the moved pages were detached). `End` is now the engine's own `'end'`
/// position (`controller.ts:69`), and an unknown page refuses the move
/// (`refsOf`, `controller.ts:62-65`) instead of not counting.
#[test]
fn end_moves_the_block_after_the_pages_left_behind() {
    let (_thread, edit) = handle();
    let layout = block_on(edit.move_pages(vec![P10], PageEditPlacement::End, token())).unwrap();
    let pons: Vec<u32> = layout.pages.iter().map(|p| p.page.page_object_number).collect();
    assert_eq!(pons, [11, 12, 10]);
    let unknown = PageRef { page_object_number: 99 };
    let error =
        block_on(edit.move_pages(vec![P10, unknown], PageEditPlacement::End, token())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_layout_the_record_cannot_hold_is_invalid_state() {
    let (_thread, edit) = handle_over(|| {
        engine(json!("*")).respond("apply", |args| {
            let op = args[0]["ops"][0].clone();
            // A page without a size: the registry takes it, the record can't.
            let pages =
                [json!({ "index": 0, "ref": { "kind": "objectNumber", "objectNumber": 10 } })];
            Ok(applied(args, &op, &pages, None))
        })
    });
    let error = block_on(edit.move_pages(vec![P10], PageEditPlacement::End, token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn a_token_cancelled_before_the_job_runs_skips_the_edit() {
    let (thread, edit) = handle();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let rotate = edit.set_rotation(vec![P10], PageEditRotation::Clockwise180, cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(rotate).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    let layout = block_on(edit.set_rotation(vec![P11], PageEditRotation::None, token())).unwrap();
    assert_eq!(layout.pages[0].rotation, 0, "the cancelled rotation never ran");
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, edit) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(edit.can_edit()).map(|_| ()).unwrap_err(),
        block_on(edit.delete_pages(vec![P10], token())).map(|_| ()).unwrap_err(),
        block_on(edit.extract(vec![P10], token())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}
