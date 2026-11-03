//! The native engine's render guards (`LocalPageRenderService.raw`) and its
//! plugin permissions (`LocalDocumentSecurityService.allows`).
//!
//! Each kind of picture needs its own right and no other (8638dc749): a
//! session holding only `doc.render.thumbnail` has thumbnails and no page
//! pictures. A plugin's permission holds when the scope names it or holds
//! `*` (ddd26250d).

use epdf_engine::host::Host;
use epdf_ffi::KernelThread;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::Engine;
use epdf_viewer_host::engine_worker::EngineWorker;
use epdf_viewer_host::native_engine::NativeEngine;
use futures::channel::oneshot;
use futures::executor::block_on;
use serde_json::{Value, json};

mod common;
use common::fixture;

/// Each `options` rendered on the report's first page, by a native engine
/// opened with `scope`: the image, or the refusal as `{name, code,
/// required}`.
fn renders(scope: &[&str], each: Vec<Value>) -> Vec<Result<Value, Value>> {
    let scope: Vec<String> = scope.iter().map(|s| s.to_string()).collect();
    let worker = EngineWorker::spawn_with(Host::new);
    let thread =
        KernelThread::spawn(move |post| Ok(NativeEngine::new(worker, post, Some(scope)))).unwrap();
    let answer = block_on(thread.call(move |engine: &mut NativeEngine| {
        let (tx, rx) = oneshot::channel::<Vec<Result<Value, Value>>>();
        let input =
            json!({ "kind": "bytes", "bytes": Bytes(fixture(test_support::REPORT)).to_json() });
        engine.open(input, None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().expect("opens");
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let pon =
                    list.as_ref().unwrap()["pages"][0]["ref"]["objectNumber"].as_u64().unwrap();
                next(doc, pon as u32, each, Vec::new(), tx);
            });
        });
        rx
    }))
    .unwrap();
    block_on(answer).unwrap()
}

fn next(
    doc: std::rc::Rc<dyn epdf_kernel::engine::DocumentHandle>,
    pon: u32,
    mut rest: Vec<Value>,
    mut done: Vec<Result<Value, Value>>,
    tx: oneshot::Sender<Vec<Result<Value, Value>>>,
) {
    if rest.is_empty() {
        let _ = tx.send(done);
        return;
    }
    let options = rest.remove(0);
    doc.clone().call("page.render.image", Some(pon), vec![options]).on_settle(move |image| {
        done.push(match image {
            Ok(image) => Ok(image.clone()),
            Err(error) => Err(json!({
                "name": error.name,
                "code": error.code(),
                "required": error.member("required"),
            })),
        });
        next(doc, pon, rest, done, tx);
    });
}

fn refusal(required: &str) -> Value {
    json!({ "name": "PermissionDenied", "code": "Forbidden", "required": required })
}

#[test]
fn a_thumbnail_right_alone_gives_thumbnails_and_no_page_pictures() {
    let answers = renders(
        &["doc.open", "doc.render.thumbnail"],
        vec![
            json!({ "viewport": { "kind": "thumbnail" } }),
            json!({ "viewport": { "kind": "width", "width": 64 } }),
            json!({}),
            json!({ "usage": "print", "viewport": { "kind": "dpi", "dpi": 150 } }),
        ],
    );
    let thumbnail = answers[0].as_ref().expect("a thumbnail");
    let (width, height) =
        (thumbnail["width"].as_u64().unwrap(), thumbnail["height"].as_u64().unwrap());
    assert_eq!(width.max(height), 512, "{thumbnail}");
    assert_eq!(answers[1], Err(refusal("doc.render")));
    assert_eq!(answers[2], Err(refusal("doc.render")));
    assert_eq!(answers[3], Err(refusal("doc.print")));
}

#[test]
fn doc_render_gives_thumbnails_too_and_printing_is_its_own_right() {
    let answers = renders(
        &["doc.open", "doc.render", "doc.print"],
        vec![
            json!({ "viewport": { "kind": "thumbnail" } }),
            json!({ "viewport": { "kind": "width", "width": 64 } }),
            json!({ "usage": "print", "viewport": { "kind": "dpi", "dpi": 150 } }),
            json!({ "usage": "print", "viewport": { "kind": "dpi", "dpi": 300 } }),
        ],
    );
    assert!(answers[0].is_ok(), "{:?}", answers[0]);
    assert_eq!(answers[1].as_ref().unwrap()["width"], json!(64));
    assert!(answers[2].is_ok(), "{:?}", answers[2]);
    assert_eq!(answers[3], Err(refusal("doc.print.high")));
}

#[test]
fn a_picture_draws_only_what_the_caller_may_read() {
    let answers = renders(
        &["doc.open", "doc.render"],
        vec![
            json!({ "viewport": { "kind": "width", "width": 64 } }),
            json!({ "viewport": { "kind": "width", "width": 64 }, "includeAnnotations": true }),
        ],
    );
    assert!(answers[0].is_ok(), "{:?}", answers[0]);
    assert_eq!(answers[1], Err(refusal("doc.annotate.read")));
}

#[test]
fn the_default_scope_renders_every_picture() {
    let answers = renders(
        &["*"],
        vec![
            json!({ "viewport": { "kind": "thumbnail" } }),
            json!({ "usage": "print", "viewport": { "kind": "dpi", "dpi": 300 } }),
            json!({ "viewport": { "kind": "width", "width": 64 }, "includeAnnotations": true }),
        ],
    );
    for answer in &answers {
        assert!(answer.is_ok(), "{answer:?}");
    }
}

/// `security().allows(permission)` for each permission, under `scope`.
fn allows(scope: &[&str], each: &[&str]) -> Vec<bool> {
    let scope: Vec<String> = scope.iter().map(|s| s.to_string()).collect();
    let each: Vec<String> = each.iter().map(|s| s.to_string()).collect();
    let worker = EngineWorker::spawn_with(Host::new);
    let thread =
        KernelThread::spawn(move |post| Ok(NativeEngine::new(worker, post, Some(scope)))).unwrap();
    let answer = block_on(thread.call(move |engine: &mut NativeEngine| {
        let (tx, rx) = oneshot::channel::<Vec<bool>>();
        let input =
            json!({ "kind": "bytes", "bytes": Bytes(fixture(test_support::REPORT)).to_json() });
        engine.open(input, None).on_settle(move |doc| {
            let security = match doc {
                Ok(doc) => doc.security(),
                Err(error) => panic!("opens: {}", error.message),
            };
            let _ = tx.send(each.iter().map(|p| security.allows(p)).collect());
        });
        rx
    }))
    .unwrap();
    block_on(answer).unwrap()
}

#[test]
fn a_plugin_permission_holds_when_the_scope_names_it_or_holds_the_wildcard() {
    let named = allows(
        &["doc.open", "protected-display.view"],
        &["protected-display.view", "protected-display.export", "snapshot.create", "doc.open"],
    );
    assert_eq!(named, [true, false, false, true]);

    let everything = allows(&["*"], &["protected-display.view", "snapshot.create", "doc.print"]);
    assert_eq!(everything, [true, true, true]);

    // A plugin permission grants no capability, and a core word that isn't
    // one is still unknown.
    let plugin_only = allows(&["protected-display.view"], &["doc.open", "doc.future"]);
    assert_eq!(plugin_only, [false, false]);
}
