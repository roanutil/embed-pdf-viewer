//! The app half: pure, synchronous, and unaware that PDFium exists.
//!
//! Everything the engine does arrives here as the output of an
//! [`EngineRequest`] effect, which [`crate::middleware::EngineMiddleware`]
//! resolves on the owner thread. That is what lets this module be a Crux
//! `App` at all: Crux's apps cannot perform I/O, and a PDFium call is I/O
//! from Crux's point of view even though it never leaves the process.
//!
//! The model is what a viewer needs to lay out a document, and deliberately
//! no more: which documents are open, whether each one wants a password, and
//! its pages by durable identity. Rasters do not go in the view model. A
//! `view()` is re-serialized on every render, and a page of RGBA in it would
//! be copied across the FFI every time anything changed. They go out as a
//! notification instead, once, and as a [`Blob`] rather than as bytes: see
//! [`crate::blobs`] for why.

use std::collections::BTreeMap;

use crux_core::{App, Command, macros::effect, render::RenderOperation};
use facet::Facet;
use serde::{Deserialize, Serialize};

pub use crate::blobs::Blob;

// ── Operations ─────────────────────────────────────────────────────────────

/// One wire request, exactly as `epdf_engine::host::Host::call` takes it.
///
/// The JSON is the same envelope every other binding sends, so this crate
/// adds no second protocol: the typed events below are built on top of it
/// rather than beside it.
#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EngineRequest {
    pub json: String,
    pub payloads: Vec<Blob>,
}

/// What the engine answered: `{"result": …}` or `{"error": …}`, plus the
/// binary outputs, positional.
#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EngineReply {
    pub json: String,
    pub payloads: Vec<Blob>,
}

impl crux_core::capability::Operation for EngineRequest {
    type Output = EngineReply;
}

// Every event a shell sends for an answer carries a `request` number the
// shell chose, and exactly one of the four notifications below comes back
// with it. Crux has no correlation of its own: a notification is not a reply
// to anything, so without the number two renders of the same page could not
// be told apart. That is the one thing an SDK facade needs that an app shell
// does not, because an app just redraws from whatever arrived.

/// A rendered page.
///
/// RGBA8 with straight alpha and an explicit stride, the same contract as
/// `epdf-uniffi`'s `PdfBitmap`.
#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PageBitmap {
    pub request: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rgba: Blob,
}

impl crux_core::capability::Operation for PageBitmap {
    type Output = ();
}

/// A page's text in display order, UTF-16 faithful.
#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PageText {
    pub request: u32,
    pub text: String,
}

impl crux_core::capability::Operation for PageText {
    type Output = ();
}

/// The raw envelope answering an [`Event::Call`].
#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CallReply {
    pub request: u32,
    pub json: String,
    pub payloads: Vec<Blob>,
}

impl crux_core::capability::Operation for CallReply {
    type Output = ();
}

/// A typed request the engine refused, with its `EngineErrorCode` spelling.
///
/// A failed render fails that render, not the document: the view model's
/// `Failed` state is for a document that could not be opened at all.
#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RequestFailed {
    pub request: u32,
    pub code: String,
    pub message: String,
}

impl crux_core::capability::Operation for RequestFailed {
    type Output = ();
}

// ── Events ─────────────────────────────────────────────────────────────────

#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[repr(C)]
pub enum Event {
    /// Open a document from bytes the shell put in the blob store. A missing
    /// or wrong password is not a
    /// failure: the document shows as `NeedsPassword`. The wire has no kind
    /// that unlocks a parked session yet, so the shell closes and reopens.
    Open {
        doc_id: String,
        document: Blob,
        password: Option<String>,
    },
    Close {
        doc_id: String,
    },
    /// Render one page at `scale` device pixels per PDF point. Answered by
    /// `Bitmap` or `Failed`, not through the view model.
    Render {
        request: u32,
        doc_id: String,
        page_object_number: u32,
        scale: f64,
    },
    /// Read one page's text. Answered by `Text` or `Failed`.
    Text {
        request: u32,
        doc_id: String,
        page_object_number: u32,
    },
    /// Any of the sixty-five wire kinds, untyped. The escape hatch that keeps
    /// the whole engine reachable while the typed surface above is small.
    /// Answered by `Reply`, whose envelope carries any refusal.
    Call {
        request: u32,
        json: String,
        payloads: Vec<Blob>,
    },

    // Internal: the engine's answers. Never sent by a shell and never
    // generated into one, which is what the two `skip`s say.
    #[serde(skip)]
    #[facet(skip)]
    Opened {
        doc_id: String,
        open: EngineReply,
        pages: Option<EngineReply>,
    },
    #[serde(skip)]
    #[facet(skip)]
    Closed {
        doc_id: String,
    },
}

// ── Effects ────────────────────────────────────────────────────────────────

/// Everything the app asks for, including the engine.
///
/// The `Engine` variant never reaches a shell: the middleware consumes it and
/// [`crate::ShellEffect`] is what crosses the FFI.
#[effect]
#[derive(Debug)]
pub enum Effect {
    Render(RenderOperation),
    Engine(EngineRequest),
    Bitmap(PageBitmap),
    Text(PageText),
    Reply(CallReply),
    Failed(RequestFailed),
}

// ── Model and view ─────────────────────────────────────────────────────────

#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PageView {
    pub index: u32,
    pub page_object_number: u32,
    pub width: f64,
    pub height: f64,
}

#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[repr(C)]
pub enum DocumentState {
    Opening,
    NeedsPassword,
    Ready,
    Failed { code: String, message: String },
}

#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DocumentView {
    pub doc_id: String,
    pub state: DocumentState,
    pub pages: Vec<PageView>,
}

#[derive(Facet, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ViewModel {
    pub documents: Vec<DocumentView>,
}

#[derive(Debug)]
struct DocumentModel {
    state: DocumentState,
    pages: Vec<PageView>,
}

#[derive(Debug, Default)]
pub struct Model {
    documents: BTreeMap<String, DocumentModel>,
}

// ── The app ────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct PdfViewer;

impl App for PdfViewer {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        match event {
            Event::Open { doc_id, document, password } => {
                model.documents.insert(
                    doc_id.clone(),
                    DocumentModel { state: DocumentState::Opening, pages: Vec::new() },
                );
                let mut request = serde_json::json!({ "kind": "open.fatMem", "docId": doc_id });
                if let Some(password) = password {
                    request["password"] = password.into();
                }
                open_then_list(doc_id, request.to_string(), vec![document])
                    .and(crux_core::render::render())
            }
            Event::Opened { doc_id, open, pages } => {
                let Some(document) = model.documents.get_mut(&doc_id) else {
                    return Command::done();
                };
                apply_open(document, &open, pages.as_ref());
                crux_core::render::render()
            }
            Event::Close { doc_id } => {
                let request = serde_json::json!({ "kind": "close", "docId": doc_id });
                Command::request_from_shell(EngineRequest {
                    json: request.to_string(),
                    payloads: Vec::new(),
                })
                .then_send(move |_| Event::Closed { doc_id })
            }
            Event::Closed { doc_id } => {
                model.documents.remove(&doc_id);
                crux_core::render::render()
            }
            // Render, Text and Call change no model, so none of them needs an
            // internal event: each is one command that asks the engine and
            // notifies the shell, correlated by the shell's own number.
            Event::Render { request, doc_id, page_object_number, scale } => {
                let json = serde_json::json!({
                    "kind": "pages.render",
                    "docId": doc_id,
                    "pageObjectNumber": page_object_number,
                    "options": { "viewport": { "kind": "scale", "scale": scale } },
                });
                Command::new(move |ctx| async move {
                    let reply = ctx.request_from_shell(engine(json)).await;
                    // A raster reply always carries its pixels as the first
                    // payload; one without is a wire bug, reported as one.
                    let raster = result_of(&reply).and_then(|result| {
                        let rgba = reply.payloads.first().copied().ok_or_else(|| {
                            (
                                "WireFormat".to_string(),
                                "a render answered without pixels".to_string(),
                            )
                        })?;
                        Ok((result, rgba))
                    });
                    match raster {
                        Ok((result, rgba)) => {
                            let field = |name: &str| {
                                result.get(name).and_then(serde_json::Value::as_u64).unwrap_or(0)
                                    as u32
                            };
                            ctx.notify_shell(PageBitmap {
                                request,
                                width: field("width"),
                                height: field("height"),
                                stride: field("stride"),
                                rgba,
                            });
                        }
                        Err((code, message)) => {
                            ctx.notify_shell(RequestFailed { request, code, message })
                        }
                    }
                })
            }
            Event::Text { request, doc_id, page_object_number } => {
                let json = serde_json::json!({
                    "kind": "pages.text",
                    "docId": doc_id,
                    "pageObjectNumber": page_object_number,
                });
                Command::new(move |ctx| async move {
                    let reply = ctx.request_from_shell(engine(json)).await;
                    match result_of(&reply) {
                        Ok(result) => ctx.notify_shell(PageText {
                            request,
                            text: result["text"].as_str().unwrap_or_default().to_string(),
                        }),
                        Err((code, message)) => {
                            ctx.notify_shell(RequestFailed { request, code, message })
                        }
                    }
                })
            }
            Event::Call { request, json, payloads } => Command::new(move |ctx| async move {
                let reply = ctx.request_from_shell(EngineRequest { json, payloads }).await;
                ctx.notify_shell(CallReply { request, json: reply.json, payloads: reply.payloads });
            }),
        }
    }

    fn view(&self, model: &Model) -> ViewModel {
        ViewModel {
            documents: model
                .documents
                .iter()
                .map(|(doc_id, document)| DocumentView {
                    doc_id: doc_id.clone(),
                    state: document.state.clone(),
                    pages: document.pages.clone(),
                })
                .collect(),
        }
    }
}

/// A request that carries no payloads.
fn engine(json: serde_json::Value) -> EngineRequest {
    EngineRequest { json: json.to_string(), payloads: Vec::new() }
}

/// Opens and, when that worked, lists the pages, in one command
/// so the view never shows a `Ready` document with no pages.
fn open_then_list(doc_id: String, json: String, payloads: Vec<Blob>) -> Command<Effect, Event> {
    Command::new(|ctx| async move {
        let open = ctx.request_from_shell(EngineRequest { json, payloads }).await;
        let pages = if result_of(&open).is_ok() {
            let request = serde_json::json!({ "kind": "pages.list", "docId": doc_id });
            Some(
                ctx.request_from_shell(EngineRequest {
                    json: request.to_string(),
                    payloads: Vec::new(),
                })
                .await,
            )
        } else {
            None
        };
        ctx.send_event(Event::Opened { doc_id, open, pages });
    })
}

fn apply_open(document: &mut DocumentModel, open: &EngineReply, pages: Option<&EngineReply>) {
    if let Err((code, message)) = result_of(open) {
        document.state = DocumentState::Failed { code, message };
        return;
    }
    let Some(pages) = pages else {
        document.state =
            DocumentState::Failed { code: "Unknown".into(), message: "no page list".into() };
        return;
    };
    match result_of(pages) {
        Ok(result) => {
            let listed = result["pages"].as_array().cloned().unwrap_or_default();
            document.pages = listed
                .iter()
                .map(|page| PageView {
                    index: page["index"].as_u64().unwrap_or(0) as u32,
                    page_object_number: page["pageObjectNumber"].as_u64().unwrap_or(0) as u32,
                    width: page["size"]["width"].as_f64().unwrap_or(0.0),
                    height: page["size"]["height"].as_f64().unwrap_or(0.0),
                })
                .collect();
            document.state = DocumentState::Ready;
        }
        // A parked open answers the open with a probe and then refuses the
        // listing with this code, which is the prompt a viewer wants.
        Err((code, _)) if code == "DocPasswordRequired" => {
            document.state = DocumentState::NeedsPassword;
        }
        Err((code, message)) => document.state = DocumentState::Failed { code, message },
    }
}

/// The envelope's `result`, or its error's code and message.
fn result_of(reply: &EngineReply) -> Result<serde_json::Value, (String, String)> {
    let mut envelope: serde_json::Value =
        serde_json::from_str(&reply.json).map_err(|e| ("WireFormat".to_string(), e.to_string()))?;
    if let Some(error) = envelope.get("error") {
        return Err((
            error["code"].as_str().unwrap_or("Unknown").to_string(),
            error["message"].as_str().unwrap_or_default().to_string(),
        ));
    }
    Ok(envelope["result"].take())
}
