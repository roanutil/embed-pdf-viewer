//! The Crux app, first without PDFium and then through the real middleware.
//!
//! The first half is the property Crux is for: the app's behaviour is tested
//! by resolving its effects by hand, so a viewer flow can be pinned without an
//! engine, a device or a shell. The second half is the property this spike is
//! for: the same app, with the engine resolved in Rust on its owner thread,
//! answers a shell exactly the way the shell will be answered over the FFI.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crux_core::Core;
use crux_core::middleware::{HandleEffectLayer, Layer as _, MapEffectLayer};
use epdf_crux::ShellEffect;
use epdf_crux::app::{Blob, DocumentState, EngineReply, Event, Model, PdfViewer};
use epdf_crux::blobs::BlobStore;
use epdf_crux::middleware::EngineMiddleware;

const PLAIN: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";
const ENCRYPTED: &str = "tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf";

fn bytes(fixture: &str) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(fixture))
        .unwrap_or_else(|e| panic!("read {fixture}: {e}"))
}

fn reply(json: serde_json::Value) -> EngineReply {
    EngineReply { json: json.to_string(), payloads: Vec::new() }
}

// ── Without PDFium ─────────────────────────────────────────────────────────

#[test]
fn an_open_asks_the_engine_to_open_then_to_list_and_the_view_follows() {
    use crux_core::App as _;
    use epdf_crux::app::Effect;

    let app = PdfViewer;
    let mut model = Model::default();
    let mut command = app.update(
        Event::Open { doc_id: "d".into(), document: Blob { id: 9 }, password: None },
        &mut model,
    );

    let mut effects: Vec<Effect> = command.effects().collect();
    assert!(effects.iter().any(|e| matches!(e, Effect::Render(_))), "no redraw for Opening");
    let Some(Effect::Engine(mut open)) = effects.drain(..).find(|e| matches!(e, Effect::Engine(_)))
    else {
        panic!("the open did not reach the engine");
    };
    let envelope: serde_json::Value = serde_json::from_str(&open.operation.json).unwrap();
    assert_eq!(envelope, serde_json::json!({ "kind": "open.fatMem", "docId": "d" }));
    assert_eq!(open.operation.payloads, vec![Blob { id: 9 }], "the document is a payload");
    assert_eq!(app.view(&model).documents[0].state, DocumentState::Opening);

    open.resolve(reply(serde_json::json!({ "result": { "security": {} } }))).unwrap();
    let Some(Effect::Engine(mut list)) = command.effects().next() else {
        panic!("a successful open did not ask for the page list");
    };
    assert!(list.operation.json.contains("\"pages.list\""));
    list.resolve(reply(serde_json::json!({ "result": { "pageCount": 1, "pages": [{
        "index": 0, "pageObjectNumber": 7, "size": { "width": 612.0, "height": 792.0 }
    }]}})))
    .unwrap();

    let opened: Vec<Event> = command.events().collect();
    assert_eq!(opened.len(), 1);
    let _ = app.update(opened.into_iter().next().unwrap(), &mut model);
    let view = app.view(&model);
    assert_eq!(view.documents[0].state, DocumentState::Ready);
    assert_eq!(view.documents[0].pages[0].page_object_number, 7);
    assert_eq!(view.documents[0].pages[0].height, 792.0);
}

#[test]
fn a_refused_listing_for_want_of_a_password_is_a_prompt_not_a_failure() {
    use crux_core::App as _;
    use epdf_crux::app::Effect;

    let app = PdfViewer;
    let mut model = Model::default();
    let mut command = app.update(
        Event::Open { doc_id: "locked".into(), document: Blob { id: 9 }, password: None },
        &mut model,
    );
    // Collected rather than searched: the Opening redraw is queued behind the
    // open, and leaving it there would make it the next effect below.
    let effects: Vec<Effect> = command.effects().collect();
    let Some(Effect::Engine(mut open)) =
        effects.into_iter().find(|e| matches!(e, Effect::Engine(_)))
    else {
        panic!("no open");
    };
    open.resolve(reply(serde_json::json!({ "result": { "security": {} } }))).unwrap();
    let Some(Effect::Engine(mut list)) = command.effects().next() else { panic!("no list") };
    list.resolve(reply(serde_json::json!({
        "error": { "code": "DocPasswordRequired", "message": "locked" }
    })))
    .unwrap();
    let _ = app.update(command.events().next().unwrap(), &mut model);
    assert_eq!(app.view(&model).documents[0].state, DocumentState::NeedsPassword);
}

// ── Through the middleware, with PDFium ────────────────────────────────────

type Stack = MapEffectLayer<HandleEffectLayer<Core<PdfViewer>, EngineMiddleware>, ShellEffect>;

/// The core as `ffi.rs` builds it, with a channel standing in for the
/// shell's `process_effects` callback.
struct Shell {
    core: Stack,
    blobs: Arc<BlobStore>,
    later: Sender<ShellEffect>,
    rx: Receiver<ShellEffect>,
}

impl Shell {
    fn new() -> Self {
        let blobs = BlobStore::new();
        let core = Core::<PdfViewer>::new()
            .handle_effects_using(EngineMiddleware::new(Arc::clone(&blobs)))
            .map_effect::<ShellEffect>();
        let (later, rx) = channel();
        Self { core, blobs, later, rx }
    }

    /// Sends an event and returns what came back synchronously.
    fn send(&self, event: Event) -> Vec<ShellEffect> {
        let later = Mutex::new(self.later.clone());
        self.core.update(event, move |effects| {
            let later = later.lock().unwrap();
            for effect in effects {
                let _ = later.send(effect);
            }
        })
    }

    /// Waits for the next later effect the predicate accepts, skipping the rest.
    fn wait_for(&self, accept: impl Fn(&ShellEffect) -> bool) -> ShellEffect {
        loop {
            let effect =
                self.rx.recv_timeout(Duration::from_secs(20)).expect("the engine answered");
            if accept(&effect) {
                return effect;
            }
        }
    }

    fn redraw(&self) {
        self.wait_for(|e| matches!(e, ShellEffect::Render(_)));
    }

    fn view(&self) -> epdf_crux::app::ViewModel {
        self.core.view()
    }
}

#[test]
fn a_shell_opens_lists_and_renders_through_the_engine_middleware() {
    let shell = Shell::new();

    // The immediate answer is the Opening redraw; the engine has not run yet.
    let now = shell.send(Event::Open {
        doc_id: "d".into(),
        document: shell.blobs.put(bytes(PLAIN)),
        password: None,
    });
    assert!(now.iter().all(|e| matches!(e, ShellEffect::Render(_))), "{now:?}");

    // Everything the engine produced arrives later, through the callback.
    shell.redraw();
    let view = shell.view();
    assert_eq!(view.documents[0].state, DocumentState::Ready, "{view:?}");
    let page = view.documents[0].pages[0].clone();
    assert!(page.width > 0.0 && page.height > 0.0);

    let now = shell.send(Event::Render {
        doc_id: "d".into(),
        page_object_number: page.page_object_number,
        scale: 0.5,
    });
    assert!(now.is_empty(), "a render is answered later, not now: {now:?}");
    let ShellEffect::Bitmap(bitmap) = shell.wait_for(|e| matches!(e, ShellEffect::Bitmap(_)))
    else {
        unreachable!()
    };
    let bitmap = bitmap.operation;
    assert_eq!(bitmap.width, (page.width * 0.5).round() as u32);
    assert!(bitmap.stride >= bitmap.width * 4);
    let rgba = shell.blobs.take(bitmap.rgba).expect("the raster's bytes are in the store");
    assert_eq!(rgba.len(), (bitmap.stride * bitmap.height) as usize);
    assert!(rgba.iter().any(|&b| b != 0), "the raster is blank");
    assert!(shell.blobs.is_empty(), "the document's bytes were not taken by the open");
}

#[test]
fn a_blob_that_was_never_put_is_a_refusal_not_a_hang() {
    let shell = Shell::new();
    shell.send(Event::Open { doc_id: "d".into(), document: Blob { id: 404 }, password: None });
    shell.redraw();
    let view = shell.view();
    let DocumentState::Failed { code, message } = &view.documents[0].state else {
        panic!("{view:?}");
    };
    assert_eq!(code, "WireFormat");
    assert!(message.contains("404"), "{message}");
}

#[test]
fn every_other_wire_kind_is_reachable_untyped_and_answers_by_job() {
    let shell = Shell::new();
    shell.send(Event::Open {
        doc_id: "d".into(),
        document: shell.blobs.put(bytes(PLAIN)),
        password: None,
    });
    shell.redraw();

    let json = serde_json::json!({ "kind": "metadata.read", "docId": "d" }).to_string();
    shell.send(Event::Call { job: 41, json, payloads: Vec::new() });
    let ShellEffect::Reply(reply) = shell.wait_for(|e| matches!(e, ShellEffect::Reply(_))) else {
        unreachable!()
    };
    assert_eq!(reply.operation.job, 41);
    let envelope: serde_json::Value = serde_json::from_str(&reply.operation.json).unwrap();
    assert!(envelope["result"].is_object(), "{envelope}");
}

#[test]
fn an_encrypted_document_parks_as_needing_a_password() {
    let shell = Shell::new();
    shell.send(Event::Open {
        doc_id: "locked".into(),
        document: shell.blobs.put(bytes(ENCRYPTED)),
        password: None,
    });
    shell.redraw();
    assert_eq!(shell.view().documents[0].state, DocumentState::NeedsPassword);

    shell.send(Event::Close { doc_id: "locked".into() });
    shell.redraw();
    assert!(shell.view().documents.is_empty());
    shell.send(Event::Open {
        doc_id: "locked".into(),
        document: shell.blobs.put(bytes(ENCRYPTED)),
        password: Some("user".into()),
    });
    shell.redraw();
    let view = shell.view();
    assert_eq!(view.documents[0].state, DocumentState::Ready, "{view:?}");
}
