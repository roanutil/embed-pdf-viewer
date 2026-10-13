#![cfg(feature = "uniffi")]

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

fn page(pon: u32, index: u32, rotation: u32) -> Value {
    json!({
        "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
        "pageObjectNumber": pon, "index": index, "rotation": rotation,
        "size": { "width": 612.0, "height": 792.0 }
    })
}

/// Three pages, 10, 11 and 12. `pages.rotate` and `pages.delete` apply to
/// a shared layout, publish the event a real engine publishes, and answer
/// `{layout}`. `pages.insertBlank` appends page 20.
fn spec(allows: Value) -> TestSpec {
    let layout = Rc::new(RefCell::new(vec![page(10, 0, 0), page(11, 1, 0), page(12, 2, 0)]));
    let answer = |kind: &str, pages: &[Value], inserted: Option<Vec<u32>>| {
        let layout = json!({ "pageCount": pages.len(), "pages": pages });
        let mut value = json!({ "layout": layout.clone(), "cache": null });
        if let Some(inserted) = inserted {
            value["insertedPages"] = inserted
                .iter()
                .map(|p| json!({ "kind": "objectNumber", "pageObjectNumber": p }))
                .collect();
        }
        Reply { value, events: vec![json!({ "type": kind, "layout": layout })] }
    };
    let pons = |args: &[Value]| -> Vec<u64> {
        args[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["pageObjectNumber"].as_u64().unwrap())
            .collect()
    };
    let (rotated, deleted, inserted) = (layout.clone(), layout.clone(), layout);
    TestSpec::new(json!({
        "documents": { DOCUMENT: {
            "id": DOCUMENT, "security": { "allows": allows },
            "pages": [page(10, 0, 0), page(11, 1, 0), page(12, 2, 0)]
        } }
    }))
    .respond("pages.rotate", move |args| {
        let targets = pons(args);
        let rotation = args[1].as_f64().unwrap() as u32;
        let mut pages = rotated.borrow_mut();
        for p in pages.iter_mut() {
            if targets.contains(&p["pageObjectNumber"].as_u64().unwrap()) {
                p["rotation"] = json!(rotation);
            }
        }
        Ok(answer("pages.rotated", &pages, None))
    })
    .respond("pages.delete", move |args| {
        let targets = pons(args);
        let mut pages = deleted.borrow_mut();
        if targets.len() >= pages.len() {
            return Err(epdf_kernel::Error::engine("InvalidArg", "cannot delete every page"));
        }
        pages.retain(|p| !targets.contains(&p["pageObjectNumber"].as_u64().unwrap()));
        for (i, p) in pages.iter_mut().enumerate() {
            p["index"] = json!(i);
        }
        Ok(answer("pages.deleted", &pages, None))
    })
    .respond("pages.insertBlank", move |_| {
        let mut pages = inserted.borrow_mut();
        let index = pages.len() as u32;
        pages.push(page(20, index, 0));
        Ok(answer("pages.inserted", &pages, Some(vec![20])))
    })
    .respond("pages.extract", |_| Ok(Reply::value(json!({ "$bytes": "JVBERi0=" }))))
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

#[test]
fn a_layout_the_record_cannot_hold_is_invalid_state() {
    let (_thread, edit) = handle_over(|| {
        spec(json!("*")).respond("pages.move", |_| Ok(Reply::value(json!({ "layout": {} }))))
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
