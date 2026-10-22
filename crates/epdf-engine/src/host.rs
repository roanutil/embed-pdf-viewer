//! The wire boundary: one request in, one response out.
//!
//! Every binding crate is a thin shell over this. `epdf-wasm` hands it bytes
//! from linear memory, `epdf-uniffi` hands it a string from Swift or Kotlin,
//! `epdf-node` hands it a Buffer. None of them knows what an operation
//! means, and none of them has a second copy of the routing.
//!
//! # Why one entry point rather than one export per operation
//!
//! There are sixty-five wire kinds. Three bindings times sixty-five exports
//! is a hundred and ninety-five places for a signature to drift, and the
//! drift would be silent: a Kotlin binding that forgot an argument compiles.
//! One entry point moves all of that into one match that the compiler checks
//! exhaustively, and leaves each binding with a single function to get right.
//!
//! # The split between JSON and payloads
//!
//! A request is JSON plus a list of BINARY payloads, and a response is the
//! same. That is not a convenience: a PDF, a raster and a font are megabytes
//! that must not be base64'd through a string, and the TypeScript wire
//! already separates them the same way, as `wirePack(payload, transfer)`.
//!
//! Payloads are positional. A request that carries two of them means the
//! first and the second, named by the operation rather than by a key,
//! because the transfer list on the other side is an array.
//!
//! # What this does NOT do
//!
//! No threading, no queue, no cancellation plumbing. A host calls this on
//! the thread that owns the engine and gets an answer back; running that
//! thread is [`crate::dispatch::OwnerThread`]'s job, and deciding when to
//! cancel is the host's. The token here is per call and is never set,
//! because nothing can set it between the call starting and returning.

use serde::{Deserialize, Serialize};

use crate::access::DocumentAccess;
use crate::auth::scope::{DocCapability, IdentityClaims};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::session::authority::SessionAuthority;
use crate::session::registry::{SessionKey, SessionRegistry};

/// One call's binary payloads, in the order the operation names them.
pub type Payloads = Vec<Vec<u8>>;

/// What a call produced.
#[derive(Debug)]
pub struct HostResponse {
    /// The JSON body, which is either `{"result": …}` or `{"error": …}`.
    /// One envelope for both outcomes, so a host has one parse and one
    /// branch rather than a status code beside a body.
    pub json: String,
    /// Binary outputs, positional on the same terms as the inputs.
    pub payloads: Payloads,
}

/// The envelope every response carries.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a EngineError>,
    /// A notice an answer publishes beside a result that is not a
    /// mutation's (`signature.prepared`, `signature.aborted`). A mutation's
    /// event rides inside its result instead, as `{result,
    /// mutationSequence, event}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<serde_json::Value>,
}

/// The fields every request shares.
///
/// Deserialized separately from the operation's own arguments so that a
/// malformed argument still reports which document and which job it was
/// for: a host correlating responses needs the job id even when the body
/// was nonsense.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestHeader {
    kind: String,
    #[serde(default)]
    doc_id: Option<String>,
    #[serde(default)]
    layer_name: Option<String>,
}

impl RequestHeader {
    /// Which session this request addresses.
    ///
    /// A layer name promotes the key, which is how one document id can hold
    /// a base view and several layer views at once.
    fn key(&self) -> Result<SessionKey, EngineError> {
        let doc_id = self.doc_id.as_deref().ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::WireFormat,
                format!("the '{}' request names no document", self.kind),
            )
        })?;
        Ok(match self.layer_name.as_deref() {
            Some(layer) => SessionKey::layer(doc_id, layer),
            None => SessionKey::document(doc_id),
        })
    }
}

/// The kinds that carry `WriteJobFields` (`worker-protocol.ts`): an `opId`
/// and an optional `objectNumberFloor`.
pub const WRITE_KINDS: &[&str] = &[
    "annotations.create",
    "annotations.delete",
    "annotations.flatten",
    "annotations.import",
    "annotations.reorder",
    "annotations.update",
    "document.apply",
    "forms.addWidget",
    "forms.createField",
    "forms.deleteField",
    "forms.deleteWidget",
    "forms.detachWidget",
    "forms.import",
    "forms.importValues",
    "forms.reorderCalculations",
    "forms.reorderWidgets",
    "forms.repair",
    "forms.reset",
    "forms.setSignatureAppearance",
    "forms.setValue",
    "forms.updateField",
    "forms.updateWidget",
    "measure.setScale",
    "metadata.update",
    "metadata.updateCustom",
    "pages.flatten",
    "pages.removeName",
    "pages.setName",
    "pieceInfo.delete",
    "pieceInfo.update",
    "redaction.apply",
    "signatures.complete",
];

/// `document.applyChanges` takes an `objectNumberFloor` without being a
/// `WriteJobFields` kind: each change it carries has its own `opId`.
pub const FLOOR_ONLY_KINDS: &[&str] = &["document.applyChanges"];

/// The kinds whose request carries an `authority`: an `AnnotationAuthority`
/// or a `ChangeAuthority` (`auth/scope/authority.ts`).
pub const AUTHORITY_KINDS: &[&str] = &[
    "annotations.delete",
    "annotations.reorder",
    "annotations.update",
    "document.apply",
    "forms.createField",
    "forms.import",
    "forms.importValues",
    "forms.reset",
    "forms.setSignatureAppearance",
    "forms.setValue",
    "forms.updateField",
    "signatures.complete",
    "signatures.prepare",
];

/// `RequestEffect` (`worker-protocol.ts`, f550e9e5f): what a job does to its
/// document, which upstream's worker queue orders jobs by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestEffect {
    Read,
    Snapshot,
    Session,
    Write,
    ContentWrite,
    RuntimeWrite,
    Open,
    Close,
}

/// `WorkerJobId`, a number upstream. A string is read too, for the callers
/// that named their jobs before the catch-up; Task 8.27 decides whether it
/// stays.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum JobId {
    Number(u64),
    Text(String),
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

/// `ChangeAuthority`, or an `AnnotationAuthority` (the same without
/// `protection`): who a write acts for and what it is checked against.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireAuthority {
    pub identity: WireAuthorityIdentity,
    /// `null` when nothing is checked (a tenant writing its own document).
    pub grants: Option<WireGrants>,
    /// What the document's signatures forbid; absent on an
    /// `AnnotationAuthority`, `null` when nothing is enforced.
    #[serde(default)]
    pub protection: Option<crate::features::signature::types::DocumentProtection>,
}

/// `Identity` (`auth/scope/types.ts`), as an authority names it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireAuthorityIdentity {
    pub user_id: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub title: Option<String>,
    pub organization: Option<String>,
    pub organizational_unit: Option<String>,
    pub group_id: Option<String>,
}

/// An authority's `grants`: the caller's scope and the PDF permission bits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireGrants {
    pub scope: Vec<String>,
    pub pdf_bits: WirePdfBits,
}

/// `PdfBits`, as the wire spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct WirePdfBits {
    pub bit3: bool,
    pub bit4: bool,
    pub bit5: bool,
    pub bit6: bool,
    pub bit9: bool,
    pub bit10: bool,
    pub bit11: bool,
    pub bit12: bool,
}

impl From<WirePdfBits> for crate::auth::scope::types::PdfBits {
    fn from(w: WirePdfBits) -> Self {
        Self {
            bit3: w.bit3,
            bit4: w.bit4,
            bit5: w.bit5,
            bit6: w.bit6,
            bit9: w.bit9,
            bit10: w.bit10,
            bit11: w.bit11,
            bit12: w.bit12,
        }
    }
}

/// The fields a job request carries beside its kind's own arguments.
///
/// Every field is optional until Task 8.27, so the kernel's calls through
/// `translate.rs` and the tests that predate the envelope keep working. An
/// absent `authority` means the session's own, as `WorkerHost` treats a
/// write whose handle was checked before the job.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobEnvelope {
    pub effect: Option<RequestEffect>,
    pub job_id: Option<JobId>,
    /// Read for [`WRITE_KINDS`] only.
    pub op_id: Option<String>,
    /// Read for [`WRITE_KINDS`] and [`FLOOR_ONLY_KINDS`] only.
    pub object_number_floor: Option<u32>,
    /// Read for [`AUTHORITY_KINDS`] only.
    pub authority: Option<WireAuthority>,
}

impl JobEnvelope {
    /// The envelope of a `kind` request whose JSON is `body`.
    ///
    /// A field the kind does not take is ignored, as `WorkerHost` ignores
    /// it. A field it takes in the wrong shape is `WireFormat`, as any
    /// malformed argument is.
    pub fn parse(kind: &str, body: &serde_json::Value) -> Result<Self, EngineError> {
        let writes = WRITE_KINDS.contains(&kind);
        let floor = writes || FLOOR_ONLY_KINDS.contains(&kind);
        Ok(Self {
            effect: optional(body, "effect")?,
            job_id: optional(body, "jobId")?,
            op_id: if writes { optional(body, "opId")? } else { None },
            object_number_floor: if floor { optional(body, "objectNumberFloor")? } else { None },
            authority: if AUTHORITY_KINDS.contains(&kind) {
                optional(body, "authority")?
            } else {
                None
            },
        })
    }
}

/// `WorkerHost`'s refusal of a kind it has no case for.
fn unknown_kind(kind: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, format!("unknown request kind: {kind}"))
}

/// The engine, as a host sees it.
///
/// Holds every open session. Not `Send`: every handle beneath it is
/// thread-confined, and a host that wants to call from another thread goes
/// through [`crate::dispatch::OwnerThread`].
pub struct Host {
    sessions: SessionRegistry,
    /// Fonts this host registered, shared by every session it holds.
    ///
    /// Claimed on first use rather than at construction, and `None` until
    /// then. PDFium's registry is THREAD-global and
    /// [`crate::features::fonts::FontRegistry`] enforces one owner per
    /// thread, so building one here eagerly would make "one host per thread"
    /// a hard rule. A server pool holding two engines on one thread is a
    /// reasonable thing to do, and only the one that actually registers a
    /// font needs the registry; the second gets the registry's own refusal
    /// if it tries, which says what the conflict is.
    fonts: Option<crate::features::fonts::FontRegistry>,
    clock: Box<dyn crate::clock::Clock>,
    ids: Box<dyn crate::ids::IdSource>,
    /// Set by a host that can turn a raster into a PNG or a JPEG.
    ///
    /// Absent here by design. Encoding is codec work with its own
    /// dependencies and its own failure modes, and every host that needs it
    /// already has one; an engine that bundled a second copy would be
    /// choosing the codec on the host's behalf. The `*Encoded` kinds refuse
    /// while this is `None`, saying so.
    encoder: Option<Box<dyn crate::features::server::ImageEncoder>>,
    /// The running write's `artifactPath`: where its layer artifact goes
    /// instead of a payload. Set for the length of one call.
    artifact_path: Option<String>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

/// Written by hand because an encoder is a host's own type with no `Debug`.
///
/// What is printed is what a host debugging a dispatch actually wants: how
/// many sessions are open, how many fonts are registered, and whether the
/// encoded kinds will work at all.
impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("sessions", &self.sessions.len())
            .field("fonts", &self.fonts.as_ref().map_or(0, |f| f.len()))
            .field("encoder", &self.encoder.is_some())
            .finish()
    }
}

impl Host {
    pub fn new() -> Self {
        Self {
            sessions: SessionRegistry::new(),
            fonts: None,
            clock: Box::new(crate::clock::SystemClock),
            ids: Box::new(crate::ids::SystemIdSource),
            encoder: None,
            artifact_path: None,
        }
    }

    /// Keep at most `bytes` of parsed pages between calls, across every
    /// document this host holds (`WorkerHostOptions.parsedPageBudgetBytes`;
    /// 384 MB by default). 0 keeps none.
    pub fn with_parsed_page_budget(self, bytes: u64) -> Self {
        let residency = self.sessions.residency();
        let policy = residency.borrow().policy();
        residency.borrow_mut().set_policy(crate::session::residency::PageResidencyPolicy {
            budget_bytes: bytes,
            ..policy
        });
        self
    }

    /// Install the host's image encoder, enabling the `*Encoded` kinds.
    pub fn with_encoder(mut self, encoder: Box<dyn crate::features::server::ImageEncoder>) -> Self {
        self.encoder = Some(encoder);
        self
    }

    /// Install a fixed clock and a seeded id source.
    ///
    /// For a conformance run, where a `/M` timestamp and a generated `/NM`
    /// would otherwise differ on every invocation and make a comparison
    /// against a recorded vector impossible.
    pub fn with_determinism(
        mut self,
        clock: Box<dyn crate::clock::Clock>,
        ids: Box<dyn crate::ids::IdSource>,
    ) -> Self {
        self.clock = clock;
        self.ids = ids;
        self
    }

    /// The registry, for a host that needs to reach past the wire.
    ///
    /// Exposed because the binding crates' tests assert against session
    /// state that no response carries, such as how many are open after a
    /// close. Not part of the wire.
    pub fn sessions(&self) -> &SessionRegistry {
        &self.sessions
    }

    /// The registry, mutably.
    ///
    /// For a host that supervises the engine rather than using it: a server
    /// pool closing everything between checkouts, or a test asserting on
    /// state after a shutdown. Not part of the wire, and not a way around
    /// an operation that the wire refuses.
    pub fn sessions_mut(&mut self) -> &mut SessionRegistry {
        &mut self.sessions
    }

    /// Handle one request.
    ///
    /// Never returns an `Err`: a failure is a response with an `error`
    /// field, because the binding on the other side has one channel and a
    /// host that got no response at all could not tell a refused operation
    /// from a crashed engine.
    pub fn call(&mut self, request: &str, payloads: Payloads) -> HostResponse {
        Self::respond(self.route(request, payloads))
    }

    fn respond(routed: Result<HostResponse, EngineError>) -> HostResponse {
        match routed {
            Ok(response) => response,
            Err(error) => HostResponse {
                json: serde_json::to_string(&Envelope {
                    result: None,
                    error: Some(&error),
                    event: None,
                })
                .unwrap_or_else(|_| {
                    // A serializer that cannot write an error has left
                    // the host with nothing to report, so this is the
                    // one place that hand-builds JSON.
                    String::from(r#"{"error":{"name":"EngineError","code":"Unknown","message":"unserializable"}}"#)
                }),
                payloads: Vec::new(),
            },
        }
    }

    fn route(&mut self, request: &str, payloads: Payloads) -> Result<HostResponse, EngineError> {
        let header: RequestHeader = serde_json::from_str(request).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::WireFormat,
                format!("the request is not a wire envelope: {e}"),
            )
        })?;
        let body: serde_json::Value = serde_json::from_str(request)
            .map_err(|e| EngineError::with_message(EngineErrorCode::WireFormat, format!("{e}")))?;
        let cancel = CancelToken::never();
        // The residency's idle timer would have fired between calls.
        self.sessions.residency().borrow_mut().tick();
        // `WorkerHost.run`: a write that changes content closes the pages it
        // may change before it runs and after, done or refused, so no change
        // meets a page parsed before it (`pageEffectOf`).
        let effect = PageEffect::of(&header.kind);
        self.close_pages_changed_by(&header, effect);
        let routed = self.route_job(&header, body, payloads, &cancel);
        self.close_pages_changed_by(&header, effect);
        routed
    }

    /// [`Host::route`] once the pages a job may change are closed.
    fn route_job(
        &mut self,
        header: &RequestHeader,
        mut body: serde_json::Value,
        payloads: Payloads,
        cancel: &CancelToken,
    ) -> Result<HostResponse, EngineError> {
        // Read so that a malformed envelope is refused before any arm runs.
        let job = JobEnvelope::parse(&header.kind, &body)?;

        // `pageSpaceBoundary`: the places a caller sent in page space
        // convert to the file's coordinates before the handler runs, and the
        // places in its answer convert back after. A kind the wire does not
        // have reaches its arm as it came, and the arm refuses it.
        let kind = crate::page_space::boundary::WireKind::of(&header.kind);
        let key = header.doc_id.is_some().then(|| header.key()).transpose()?;
        if let Some(kind) = kind {
            let mut box_of = crate::page_space::boundary::session_boxes(
                &self.sessions,
                key.clone(),
                &header.kind,
            );
            crate::page_space::boundary::request_in_file_space(kind, &mut body, &mut box_of)?;
        }
        self.artifact_path = optional(&body, "artifactPath")?;
        let written = self.begin_write(header, &mut body, &job);
        let routed = written.and_then(|transacted| {
            let routed = self.route_wire(header, &body, payloads, cancel);
            self.end_write(transacted.as_ref(), routed)
        });
        self.artifact_path = None;
        let response = routed?;
        match kind {
            Some(kind) => {
                let mut box_of =
                    crate::page_space::boundary::session_boxes(&self.sessions, key, &header.kind);
                crate::page_space::boundary::result_in_page_space(
                    kind,
                    &body,
                    response,
                    &mut box_of,
                )
            }
            None => Ok(response),
        }
    }

    /// `closePagesChangedBy`: closes the pages a job of `effect` may change,
    /// those no job holds. A content write closes its document's; a runtime
    /// write (fonts) every document's.
    fn close_pages_changed_by(&self, header: &RequestHeader, effect: PageEffect) {
        match effect {
            PageEffect::Runtime => self.sessions.residency().borrow_mut().close_idle(),
            PageEffect::Document => {
                if let Ok(key) = header.key()
                    && let Ok(session) = self.sessions.session(&key)
                    && let Some(pool) = session.page_pool()
                {
                    pool.close_idle();
                }
            }
            PageEffect::None => {}
        }
    }

    /// `WorkerHost.run`, before the arm: a write to an open session runs as
    /// one layer transaction under its `opId` (the request's, or one minted
    /// for it and written into the request so the arm reads the same). A
    /// parked signing refuses the write before anything else; then the
    /// transaction opens, and the write's own objects go at or above the
    /// floor its caller set, so the layer's last object number is raised to
    /// just below it. Answers the session the transaction is open on; a
    /// write to no open session runs without one, and its arm refuses it.
    fn begin_write(
        &mut self,
        header: &RequestHeader,
        body: &mut serde_json::Value,
        job: &JobEnvelope,
    ) -> Result<Option<SessionKey>, EngineError> {
        if !TRANSACTED_KINDS.contains(&header.kind.as_str()) {
            return Ok(None);
        }
        let Ok(key) = header.key() else { return Ok(None) };
        if self.sessions.session(&key).is_err() {
            return Ok(None);
        }
        crate::features::signature::mutator::assert_no_pending_signing(
            self.sessions.session(&key)?,
        )?;
        let op_id = self.op_id_of(job)?;
        if job.op_id.is_none()
            && let Some(fields) = body.as_object_mut()
        {
            fields.insert("opId".into(), serde_json::Value::String(op_id.clone()));
        }
        let session = self.sessions.session_mut(&key)?;
        session.begin_transaction(op_id)?;
        if let Some(floor) = job.object_number_floor
            && let Err(refused) = session.raise_last_object_number(floor.saturating_sub(1))
        {
            session.abort_transaction();
            return Err(refused);
        }
        Ok(Some(key))
    }

    /// `WorkerHost.run`, after the arm: a write that is done but saved
    /// nothing (it had nothing to do) commits here; any refusal aborts the
    /// transaction, leaving the document as it was. A write that committed
    /// before its artifact was saved stays committed whatever followed.
    fn end_write(
        &mut self,
        transacted: Option<&SessionKey>,
        routed: Result<HostResponse, EngineError>,
    ) -> Result<HostResponse, EngineError> {
        let Some(session) = transacted.and_then(|key| self.sessions.entry_mut(key)) else {
            return routed;
        };
        let committed = routed.and_then(|response| {
            if session.in_transaction() {
                session.commit_transaction()?;
            }
            Ok(response)
        });
        if committed.is_err() {
            session.abort_transaction();
        }
        committed
    }

    /// The wire's kinds, in the file's coordinates: [`Host::route`] has
    /// converted the request and converts the answer.
    fn route_wire(
        &mut self,
        header: &RequestHeader,
        body: &serde_json::Value,
        payloads: Payloads,
        cancel: &CancelToken,
    ) -> Result<HostResponse, EngineError> {
        let result = match header.kind.as_str() {
            "open.fatMem" => self.open_fat_memory(header, body, payloads)?,
            "open.layerMemBase" | "open.layerFileBase" => {
                self.open_layer(header, body, payloads)?
            }
            "close" => self.close_document(header)?,
            "layer.close" => self.close_layer(header)?,
            // `WorkerHost.receive`: bookkeeping, taken on arrival and never
            // answered, so a session that is not open ignores it. This entry
            // answers every call, so it answers `null`, which a host drops.
            "pages.workingSet" => {
                self.set_working_set(header, body);
                serde_json::Value::Null
            }
            "shutdown" => {
                self.sessions.shutdown();
                serde_json::Value::Null
            }
            // A cancellation, not an operation. It names the job to stop
            // rather than a document, and it answers whether there was one
            // to stop: a host that aborted a job which had already finished
            // must not report it as cancelled.
            "abort" => {
                let job_id: JobId = field(body, "jobId")?;
                serde_json::json!({ "aborted": self.sessions.abort(&job_id.to_string()) })
            }
            // Fonts belong to the library, not to a document, so these five
            // route before a session is resolved. A document id on one of
            // them would be a lie about the scope of the change.
            //
            // `WorkerHost.handleFontsRegister`: the bytes are payload 0, and
            // `italic` is the C API's number (-1 infer, 0, 1), not a bool.
            // The answer is the key and what the runtime resolved for it;
            // the native id never leaves.
            "fonts.register" => {
                let key = string_field(body, "fontKey")?;
                let fonts = self.fonts()?;
                let description = pdfium::FontDescription {
                    family_name: string_field(body, "familyName")?,
                    weight: field(body, "weight")?,
                    italic: crate::features::fonts::italic_from_wire(field(body, "italic")?),
                };
                let data = payload(&payloads, 0, "the font file")?;
                fonts.register(key.clone(), &description, data)?;
                let identity = fonts.identity_info(&key)?;
                serde_json::json!({ "fontKey": key, "identity": identity })
            }
            // `fonts.addFallback`, `fonts.clearFallbacks` and `fonts.clear`
            // answer nothing but their tag, which this wire spells as a null
            // result (as `document.setFontSettings` does).
            "fonts.addFallback" => {
                let key = string_field(body, "fontKey")?;
                self.fonts()?.add_fallback(&key)?;
                serde_json::Value::Null
            }
            "fonts.clearFallbacks" => {
                self.fonts()?.clear_fallbacks();
                serde_json::Value::Null
            }
            // `WorkerHost.handleFontsAuthorizeEditing`.
            "fonts.authorizeEditing" => {
                let key = string_field(body, "fontKey")?;
                let identity = self.fonts()?.authorize_editing(&key)?;
                serde_json::json!({ "identity": identity })
            }
            "fonts.clear" => {
                self.fonts()?.clear();
                serde_json::Value::Null
            }
            // Both take a path and no session: the point of a probe is to
            // decide whether opening is worth attempting at all.
            "document.probeSecurityFile" => {
                let path = string_field(body, "path")?;
                let password: Option<String> = optional(body, "password")?;
                let probe = self.sessions.probe_security_file(
                    std::path::Path::new(&path),
                    password.as_deref(),
                    self.clock.as_ref(),
                )?;
                // `{ tag: 'document.probeSecurityFile', security }`
                // (`WorkerHost.handleDocumentProbeSecurityFile`).
                serde_json::json!({ "security": value(&probe)? })
            }
            "document.renderPageFile" | "document.renderPageFileEncoded" => {
                let path = string_field(body, "path")?;
                let password: Option<String> = optional(body, "password")?;
                let page_index: u32 = field(body, "pageIndex")?;
                // Page space: no session carried this document to the
                // boundary, so `render_page_file` converts the target on the
                // page it opens.
                let options: crate::features::render::raster::RenderOptions<crate::geom::PageRect> =
                    optional(body, "options")?.unwrap_or_default();
                let rendered = crate::features::server::render_page_file(
                    &pdfium::Library::acquire(),
                    std::path::Path::new(&path),
                    password.as_deref(),
                    page_index,
                    &options,
                    cancel,
                )?;
                // The rendered index's durable identity and the file's page
                // count ride beside the image, as `WorkerHost` answers them:
                // a caller that asked for page zero by index learns which
                // page that was.
                let page = crate::identity::PageRef::new(rendered.page_object_number);
                let page_count = rendered.page_count;
                let (mut answer, payload) = if header.kind == "document.renderPageFile" {
                    (raster_json(&rendered.raster), rendered.raster.rgba)
                } else {
                    let encode: crate::features::server::RenderEncode = field(body, "encode")?;
                    let encoded = crate::features::server::encode_raster(
                        self.encoder.as_deref(),
                        &rendered.raster,
                        &encode,
                        cancel,
                    )?;
                    (encoded_json(&encoded), encoded.bytes)
                };
                answer["page"] = value(&page)?;
                answer["pageCount"] = page_count.into();
                return Ok(HostResponse { json: envelope(answer), payloads: vec![payload] });
            }
            // `WorkerHost.handleSignaturesFinalizeCandidate`: session-less,
            // like `document.renderPageFile`. The candidate file is the
            // caller's, patched in place and read back through a document
            // that is never registered as a session.
            "signatures.finalizeCandidate" => {
                let input: crate::features::signature::mutator::FinalizeCandidateInput =
                    serde_json::from_value(body.clone()).map_err(|e| {
                        EngineError::with_message(
                            EngineErrorCode::WireFormat,
                            format!("signatures.finalizeCandidate is not the shape it takes: {e}"),
                        )
                    })?;
                let cms = payload(&payloads, 0, "the CMS")?;
                value(&crate::features::signature::mutator::finalize_candidate(&input, cms)?)?
            }
            _ => return self.route_session(header, body, payloads, cancel),
        };
        Ok(ok(result))
    }

    /// Everything that acts on an already-open session.
    ///
    /// Resolved per arm rather than once at the top, because the operations
    /// need different things: a read takes the document, a search and an
    /// extract take the session mutably for their own bookkeeping, and a
    /// per-page read takes a page the session opens and closes.
    ///
    /// The argument KEYS are the wire's, not this crate's.
    /// `tooling/engine-conformance/vectors/wire/requests.json` records what
    /// each kind carries, read off the TypeScript union by the compiler, and
    /// `tests/wire_contract.rs` replays it. A dispatcher is the one place
    /// where a misspelled key compiles and then silently reports a missing
    /// argument, so the spelling is pinned rather than reviewed.
    fn route_session(
        &mut self,
        header: &RequestHeader,
        body: &serde_json::Value,
        payloads: Payloads,
        cancel: &CancelToken,
    ) -> Result<HostResponse, EngineError> {
        use crate::features as f;

        let key = header.key()?;
        match header.kind.as_str() {
            // ── Document reads ────────────────────────────────────────
            "metadata.read" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::metadata::read_metadata(d, cancel)?)?))
            }
            "metadata.readCustom" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::metadata::read_custom_metadata(d, cancel)?)?))
            }
            "pages.list" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::pages::list_pages(d, cancel)?)?))
            }
            "actions.read" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::actions::document::read_document_actions(d, cancel)?)?))
            }
            // `WorkerHost.handleOutlineRead`: every item, depth first, no
            // page loaded. Answers `{ outline }`, the payload
            // `{ tag: 'outline.read', outline }` without its tag.
            "outline.read" => {
                let d = self.document(&key)?;
                let outline = f::outline::read::read_outline(d, cancel)?;
                Ok(ok(serde_json::json!({ "outline": value(&outline)? })))
            }
            "annotations.list" => {
                let pages = match body.get("pages") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(_) => Some(pages_field(body)?),
                };
                let owned = self.sessions.session(&key)?.document()?;
                let list =
                    f::annotation::read::list::list_annotations(owned, pages.as_deref(), cancel)?;
                Ok(ok(value(&list)?))
            }
            "forms.list" => {
                let d = self.document(&key)?;
                let model = d.form_model().ok_or_else(form_model_missing)?;
                Ok(ok(value(&f::forms::widgets::read_form(d, &model, cancel)?)?))
            }
            // `WorkerHost.handleFormsExport`: the fields a selection takes,
            // as a bundle in page space (7f0b73a3c, which removed FDF and
            // XFDF). A read; the bundle answers as `{ bundle }`, each
            // resource a payload.
            "forms.export" => {
                let selection: f::transfer::form_export::FormExportSelection =
                    field(body, "selection")?;
                let limits: f::transfer::BundleLimits =
                    optional(body, "limits")?.unwrap_or_default();
                let bundle = f::forms::transfer::export_form(
                    self.sessions.session(&key)?,
                    &selection,
                    &limits,
                    cancel,
                )?;
                Ok(bundle_response(bundle))
            }
            // `WorkerHost.handleAnnotationsExport`: the annotations a
            // selection takes and the bytes beside them, as one bundle. A
            // read.
            "annotations.export" => {
                let selection: f::transfer::export_selection::AnnotationExportSelection =
                    field(body, "selection")?;
                let limits: f::transfer::BundleLimits =
                    optional(body, "limits")?.unwrap_or_default();
                let bundle = f::annotation::transfer::export_annotations(
                    self.sessions.session(&key)?,
                    &selection,
                    &limits,
                    cancel,
                )?;
                Ok(bundle_response(bundle))
            }
            // `AttachmentList`, `{ tag: 'attachments.list'; attachments }`
            // without its tag (a9434c0e2).
            "attachments.list" => {
                let d = self.document(&key)?;
                let attachments = f::files::attachments::list_attachments(d, cancel)?;
                Ok(ok(serde_json::json!({ "attachments": value(&attachments)? })))
            }
            "pieceInfo.read" | "pieceInfo.applications" => {
                let holder = piece_info_holder(body)?;
                let d = self.document(&key)?;
                if header.kind == "pieceInfo.applications" {
                    let apps =
                        f::files::piece_info::read_piece_info_applications(d, holder, cancel)?;
                    return Ok(ok(value(&apps)?));
                }
                let application = string_field(body, "application")?;
                let snapshot =
                    f::files::piece_info::read_piece_info(d, holder, &application, cancel)?;
                Ok(ok(value(&snapshot)?))
            }

            // ── Signatures (reads) ───────────────────────────────────
            "signatures.list" => {
                self.sessions.session(&key)?.require(DocCapability::DocFormsRead)?;
                // The working copy: the unsaved state as one more revision
                // over the loaded bytes. The same as the loaded bytes when
                // nothing is unsaved.
                let working_copy: bool = optional(body, "workingCopy")?.unwrap_or(false);
                let snapshot = if working_copy {
                    let session = self.sessions.session(&key)?;
                    f::signature::analyzer::read_working_copy_snapshot(session)?
                } else {
                    f::signature::reader::read_snapshot(self.document(&key)?)?
                };
                // `{ tag: 'signatures.list', snapshot }`.
                Ok(ok(serde_json::json!({ "snapshot": value(&snapshot)? })))
            }
            // ── Signatures (writes) ──────────────────────────────────
            // `WorkerHost.handleSignaturesPrepare`: the field must be the
            // request authority's to sign (the session's without one).
            // Answers `{ result }`, the digest a payload its `digest` names.
            "signatures.prepare" => {
                let input: f::signature::mutator::SignaturePrepareInput = field(body, "input")?;
                let job = JobEnvelope::parse(&header.kind, body)?;
                let session = self.sessions.session(&key)?;
                require_some_signing(session)?;
                if input.certify.is_some() {
                    session.require(DocCapability::DocSignCertify)?;
                }
                let authority = match &job.authority {
                    Some(wire) => change_authority(wire)?,
                    None => f::changes::applier::ChangeAuthority::of_session(session),
                };
                let appearance = match &input.appearance {
                    Some(appearance) => Some(resource_payload(&appearance.pdf, &payloads)?),
                    None => None,
                };
                let signing_id = self.ids.next_uuid_v4();
                let (prepared, digest) = f::signature::mutator::prepare(
                    session, &input, appearance, signing_id, &authority,
                )?;
                let mut prepared = value(&prepared)?;
                // `LocalDocumentSignaturesService.prepare` publishes the
                // parked candidate: its id and the field the call named.
                let notice = crate::session::save::DocumentEventInit::SignaturePrepared {
                    signing_id: prepared["signingId"].as_str().unwrap_or_default().to_owned(),
                    field: value(&input.field)?,
                };
                prepared["digest"] = serde_json::json!({ "payload": 0 });
                let answer = serde_json::json!({ "result": prepared });
                self.answer_with_notice(&key, answer, notice, vec![digest])
            }
            // `WorkerHost.handleSignaturesComplete`: under the request's
            // `opId`, final and never undoable, the field still the
            // authority's to sign.
            "signatures.complete" => {
                let input: f::signature::mutator::SignatureCompleteInput = field(body, "input")?;
                let cms = payload(&payloads, 0, "the CMS")?.to_vec();
                let job = JobEnvelope::parse(&header.kind, body)?;
                let op_id = self.op_id_of(&job)?;
                let session = self.sessions.session_mut(&key)?;
                require_some_signing(session)?;
                let authority = match &job.authority {
                    Some(wire) => change_authority(wire)?,
                    None => f::changes::applier::ChangeAuthority::of_session(session),
                };
                let result =
                    f::signature::mutator::complete(session, &input, &cms, &op_id, &authority)?;
                let result = value(&result)?;
                if result["status"] == "already-completed" {
                    return Ok(ok(serde_json::json!({
                        "result": result,
                        "mutationSequence": session.mutation_sequence(),
                    })));
                }
                let mut event = result.clone();
                event["signingId"] = serde_json::json!(input.signing_id);
                self.finish_with(
                    &key,
                    crate::session::save::DocumentEventInit::SignatureCompleted(event),
                    result,
                )
            }
            // `WorkerHost.handleSignaturesCancel`: a session effect, not a
            // write. Answers `{ result }`; a cancel that discarded a
            // candidate also publishes its notice.
            "signatures.cancel" => {
                let signing_id = string_field(body, "signingId")?;
                let session = self.sessions.session(&key)?;
                require_some_signing(session)?;
                let result = f::signature::mutator::cancel(session, &signing_id);
                let cancelled = result.status == f::signature::mutator::CancelStatus::Cancelled;
                let answer = serde_json::json!({ "result": value(&result)? });
                if !cancelled {
                    return Ok(ok(answer));
                }
                let notice =
                    crate::session::save::DocumentEventInit::SignatureAborted { signing_id };
                self.answer_with_notice(&key, answer, notice, Vec::new())
            }
            // `WorkerHost.handleFormsSetSignatureAppearance`: a one-op
            // change under the request's authority, the PDF payload 0.
            "forms.setSignatureAppearance" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let pdf = payload(&payloads, 0, "the appearance PDF")?.to_vec();
                let op = f::changes::op::ChangeOp::FormsSetSignatureAppearance {
                    field: reference,
                    appearance: f::changes::op::SignatureAppearance {
                        pdf: f::changes::op::ResourceBytes::Bytes(pdf),
                    },
                };
                self.field_verb(&key, &header.kind, body, op, true, cancel)
            }
            "signatures.analyze" => {
                self.sessions.session(&key)?.require(DocCapability::DocFormsRead)?;
                let input: f::signature::analysis::types::AnalyzeInput = field(body, "input")?;
                let session = self.sessions.session(&key)?;
                let analysis = f::signature::analyzer::analyze(session, &input)?;
                // `{ tag: 'signatures.analyze', analysis }`.
                Ok(ok(serde_json::json!({ "analysis": value(&analysis)? })))
            }
            "signatures.contents" => {
                self.sessions.session(&key)?.require(DocCapability::DocFormsRead)?;
                let target: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let d = self.document(&key)?;
                Ok(bytes_response(f::signature::reader::read_contents(d, &target)?))
            }
            "signatures.digest" => {
                self.sessions.session(&key)?.require(DocCapability::DocFormsRead)?;
                let target: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let algorithm: f::signature::types::DigestAlgorithm = field(body, "algorithm")?;
                let d = self.document(&key)?;
                Ok(bytes_response(f::signature::reader::digest(d, &target, algorithm)?))
            }
            "signatures.revisionBytes" => {
                self.sessions.session(&key)?.require(DocCapability::DocDownload)?;
                let index: u32 = field(body, "revisionIndex")?;
                let d = self.document(&key)?;
                Ok(bytes_response(f::signature::reader::revision_bytes(d, index)?))
            }
            // `{ tag: 'document.version'; version: BaseVersionInfo }`.
            "document.version" => {
                let session = self.sessions.session(&key)?;
                let version = f::signature::reader::version(session)?;
                Ok(ok(serde_json::json!({ "version": value(&version)? })))
            }
            // `{ tag: 'objectNumbers.reserve'; range: ObjectNumberRange }`.
            "objectNumbers.reserve" => {
                let count = object_number_count(body, "count")?;
                let range = self.sessions.session_mut(&key)?.reserve_object_numbers(count)?;
                Ok(ok(serde_json::json!({ "range": value(&range)? })))
            }
            // `WorkerHost.handleDocumentSetFontSettings`: session state on
            // the handle, never written to the file, so not a mutation.
            "document.setFontSettings" => {
                let policy: Option<String> = optional(body, "embeddingPolicy")?;
                let features: Option<bool> = optional(body, "typographicFeatures")?;
                let d = self.document(&key)?;
                f::fonts::apply_document_font_settings(d, policy.as_deref(), features)?;
                Ok(ok(serde_json::Value::Null))
            }

            // ── Per-page reads ────────────────────────────────────────
            "pages.text" => self
                .on_page(&key, body, |page| value(&f::text::reader::read_page_text(page, cancel)?)),
            "pages.geometry" => self
                .on_page(&key, body, |page| value(&f::geometry::read_page_geometry(page, cancel)?)),
            // `{page, viewports}`, the page as the request named it; the
            // boundary puts each viewport in page space on it.
            "measure.viewports" => self.on_page(&key, body, |page| {
                let viewports = f::measure::read_viewports(page.view(), cancel)?;
                Ok(serde_json::json!({ "page": body.get("page"), "viewports": viewports }))
            }),
            // The boundary has put `options.target` in the file's
            // coordinates, and puts the answer's `area` back in page space.
            "pages.render" | "pages.renderEncoded" => {
                let options = render_options(body)?;
                let rendered = self.on_page_raw(&key, body, |page| {
                    f::render::raster::render_page_area(page, &options, cancel)
                })?;
                if header.kind == "pages.render" {
                    // `{tag, page, area, raster}`: the raster's fields stay
                    // at the top beside its pixels, as this host has always
                    // answered them.
                    let mut answer = raster_json(&rendered.raster);
                    answer["page"] = body.get("page").cloned().unwrap_or_default();
                    answer["area"] = value(&rendered.area)?;
                    return Ok(HostResponse {
                        json: envelope(answer),
                        payloads: vec![rendered.raster.rgba],
                    });
                }
                let encode: f::server::RenderEncode = field(body, "encode")?;
                let encoded = f::server::encode_raster(
                    self.encoder.as_deref(),
                    &rendered.raster,
                    &encode,
                    cancel,
                )?;
                Ok(encoded_response(encoded))
            }
            "annotations.renderAppearances" | "annotations.renderAppearancesEncoded" => {
                let family: f::annotation::appearance::AnnotationFamily = field(body, "family")?;
                let options: f::annotation::appearance::AppearanceRenderOptions =
                    optional(body, "options")?.unwrap_or_default();
                let page = page_field(body)?;
                let session = self.sessions.session(&key)?;
                let opened = session.acquire_page(page)?;
                let result = f::annotation::appearance::render_page_appearances(
                    session.document()?.document(),
                    &opened,
                    page,
                    family,
                    &options,
                    cancel,
                )?;
                // `{page, appearances}` (`AnnotationAppearancesResult`), each
                // appearance's pixels in their own payload, in the same
                // order. A page of stamps is tens of megabytes of RGBA, and
                // putting that through the JSON would be the single most
                // expensive thing this dispatcher does.
                let encode: Option<f::server::RenderEncode> = optional(body, "encode")?;
                let mut index = Vec::with_capacity(result.appearances.len());
                let mut payloads = Vec::with_capacity(result.appearances.len());
                for appearance in result.appearances {
                    let mut entry = serde_json::json!({
                        "ref": value(&appearance.annotation_ref)?,
                        "mode": value(&appearance.mode)?,
                        "state": appearance.state,
                        "rect": value(&appearance.rect)?,
                    });
                    match &encode {
                        None => {
                            let mut raster = raster_json(&appearance.raster);
                            raster["color"] = "rgba8".into();
                            raster["premultipliedAlpha"] = false.into();
                            entry["raster"] = raster;
                            payloads.push(appearance.raster.rgba);
                        }
                        Some(encode) => {
                            let image = f::server::encode_raster(
                                self.encoder.as_deref(),
                                &appearance.raster,
                                encode,
                                cancel,
                            )?;
                            entry["image"] = encoded_json(&image);
                            payloads.push(image.bytes);
                        }
                    }
                    index.push(entry);
                }
                Ok(HostResponse {
                    json: envelope(serde_json::json!({
                        "page": crate::identity::PageRef::from(result.page),
                        "appearances": index,
                    })),
                    payloads,
                })
            }
            "annotations.readAppearance" => {
                let page = page_field(body)?;
                let annotation_ref: crate::identity::AnnotationRef = field(body, "ref")?;
                let bytes = f::annotation::appearance::read_appearance(
                    self.sessions.session(&key)?,
                    page,
                    &annotation_ref,
                    cancel,
                )?;
                Ok(bytes_response(bytes))
            }
            "annotations.readFile" => {
                let page = page_field(body)?;
                let annotation_ref: crate::identity::AnnotationRef = field(body, "ref")?;
                let ceiling: Option<u64> = optional(body, "maxDecodedBytes")?;
                let owned = self.sessions.session(&key)?.document()?;
                let opened = owned.page(page)?;
                let found = f::annotation::write::resolve::resolve_annotation(
                    owned.document(),
                    crate::access::PageAccess::view(&opened),
                    &annotation_ref,
                )?;
                let file = f::files::attachments::read_annotation_file(&found, ceiling, cancel)?;
                Ok(attachment_response(file))
            }
            "attachments.readFile" => {
                let name = embedded_file_key(body)?;
                let ceiling: Option<u64> = optional(body, "maxDecodedBytes")?;
                let d = self.document(&key)?;
                let file = f::files::attachments::read_attachment_file(d, &name, ceiling, cancel)?;
                Ok(attachment_response(file))
            }

            // ── Session-scoped reads ──────────────────────────────────
            "search.query" => {
                let request: f::search::reader::SearchRequest = field(body, "request")?;
                let session = self.sessions.session_mut(&key)?;
                Ok(ok(value(&f::search::reader::search(session, &request, cancel)?)?))
            }
            // `WorkerHost.handlePagesExtract`: a read. `flatten` is what the
            // copy paints, already checked against the caller's rights
            // (`resolvePageExtract`); absent for a plain copy.
            "pages.extract" => {
                let pages = pages_field(body)?;
                let flatten: Option<f::extract::PageExtractFlatten> = optional(body, "flatten")?;
                let session = self.sessions.session_mut(&key)?;
                let bytes = f::extract::extract_pages(session, &pages, flatten.as_ref(), cancel)?;
                Ok(bytes_response(bytes))
            }
            "document.saveBuffer" => {
                let mode: f::save::SaveMode = field(body, "mode")?;
                let session = self.sessions.session(&key)?;
                assert_rewrite_keeps_signatures(session, mode)?;
                Ok(bytes_response(f::save::save_to_buffer(session, mode, cancel)?))
            }
            "document.saveLayerBuffer" => {
                // No argument: `WorkerHost.handleDocumentSaveLayerBuffer`
                // answers the layer artifact (`DocumentSaver.
                // saveLayerArtifact`), which names its base, never the raw
                // delta.
                let session = self.sessions.session(&key)?;
                let bytes = f::save::save_layer_to_buffer(
                    session,
                    f::save::LayerArtifactKind::Artifact,
                    cancel,
                )?;
                Ok(bytes_response(bytes))
            }
            "document.saveFile" => {
                let mode: f::save::SaveMode = field(body, "mode")?;
                let path = string_field(body, "path")?;
                let session = self.sessions.session(&key)?;
                assert_rewrite_keeps_signatures(session, mode)?;
                let bytes = f::save::save_to_buffer(session, mode, cancel)?;
                std::fs::write(&path, bytes).map_err(|e| {
                    EngineError::with_message(
                        EngineErrorCode::Unknown,
                        format!("writing '{path}' failed: {e}"),
                    )
                })?;
                // `{ tag: 'document.saveFile'; path }`.
                Ok(ok(serde_json::json!({ "path": path })))
            }
            "document.checkPasswordPermissions" => {
                let password = string_field(body, "password")?;
                // The one request a locked session accepts: on a parked
                // session, "check this password" loads the parked bytes with
                // it (`WorkerHost.handleDocumentCheckPasswordPermissions`).
                // A wrong one fails and leaves the session parked.
                if self.sessions.is_locked(&key) {
                    self.sessions.unlock_parked(&key, &password)?;
                }
                let mode: crate::features::security::reader::PasswordMode =
                    optional(body, "mode")?.unwrap_or_default();
                let probe = self.sessions.check_password_permissions(
                    &key,
                    &password,
                    mode,
                    self.clock.as_ref(),
                )?;
                // `{ tag: 'document.checkPasswordPermissions', security,
                // protection }`: the protection beside the probe, so a host
                // that just unlocked knows what the signatures forbid.
                Ok(ok(serde_json::json!({
                    "security": value(&probe)?,
                    "protection": self.protection_of(&key),
                })))
            }

            // ── Mutations ─────────────────────────────────────────────
            // `WorkerHost.handleMetadataUpdate` and `handleMetadataUpdateCustom`:
            // the single verb as a one-op change, so its `opId` undoes it and a
            // retry under it gets the first answer.
            "metadata.update" | "metadata.updateCustom" => {
                // Read as `document.apply` reads an op, so the patch keeps
                // its wire key order.
                let patch = body.get("patch").cloned().ok_or_else(|| {
                    EngineError::with_message(
                        EngineErrorCode::WireFormat,
                        "the request has no 'patch'",
                    )
                })?;
                let op = f::changes::op::parse_op(
                    0,
                    &serde_json::json!({ "type": header.kind, "patch": patch }),
                )?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let item = self.apply_one(&key, &op_id, op, None, cancel)?;
                let event = item.events()?.into_iter().next().ok_or_else(|| {
                    EngineError::with_message(EngineErrorCode::Unknown, "a write published nothing")
                })?;
                let (result, sequence, mut events, written) =
                    self.finish_change(&key, &op_id, None, vec![event], strip_item(item)?)?;
                let mut answer = serde_json::json!({
                    "result": result,
                    "mutationSequence": sequence,
                });
                if let Some(event) = events.pop() {
                    answer["event"] = event;
                }
                Ok(written.respond(answer))
            }
            // `WorkerHost.handleDocumentApply`: one change, in page space both
            // ways, each op converting as it runs. The answer carries the
            // change's `events` beside it: this host's fold of main's
            // `LocalDocumentChanges.apply`, which publishes them
            // (`publishChange`) once the worker answers (see
            // `features::changes`, "Events"). Without an `authority` the
            // change acts for the session (see `JobEnvelope`).
            "document.apply" => {
                let job = JobEnvelope::parse(&header.kind, body)?;
                let raw = body.get("change").ok_or_else(|| {
                    EngineError::with_message(
                        EngineErrorCode::WireFormat,
                        "the request has no 'change'",
                    )
                })?;
                f::changes::op::assert_no_import(raw)?;
                let mut change = f::changes::op::parse_change(raw)?;
                f::changes::op::resolve_payloads(&mut change, &payloads)?;
                let op_id = self.op_id_of(&job)?;
                let fingerprint = f::changes::fingerprint::change_fingerprint(
                    &f::changes::op::change_value(&change)?,
                );
                let undo_of = match &change {
                    f::changes::op::Change::UndoOf(undone) => Some(undone.clone()),
                    f::changes::op::Change::Ops(_) => None,
                };
                let session = self.sessions.session_mut(&key)?;
                let authority = match &job.authority {
                    Some(wire) => change_authority(wire)?,
                    None => f::changes::applier::ChangeAuthority::of_session(session),
                };
                let answered = f::changes::transaction::run_change(
                    session,
                    &op_id,
                    fingerprint,
                    change,
                    &authority,
                    cancel,
                    f::changes::applier::ChangeServices {
                        clock: self.clock.as_ref(),
                        ids: self.ids.as_ref(),
                        fonts: self.fonts.as_ref(),
                    },
                    f::changes::applier::ChangeSpace::Page,
                )?;
                let result = value(&answered.result)?;
                // A retry publishes nothing again, and a change that wrote
                // nothing needs no save.
                if answered.replayed || !answered.result.wrote() {
                    let session = self.sessions.session(&key)?;
                    return Ok(ok(serde_json::json!({
                        "result": result,
                        "replayed": answered.replayed,
                        "mutationSequence": session.mutation_sequence(),
                        "events": [],
                    })));
                }
                let mut events = Vec::new();
                for item in &answered.result.items {
                    events.extend(item.events()?);
                }
                let (result, sequence, events, written) =
                    self.finish_change(&key, &op_id, undo_of, events, result)?;
                Ok(written.respond(serde_json::json!({
                    "result": result,
                    "replayed": false,
                    "mutationSequence": sequence,
                    "events": events,
                })))
            }
            // `WorkerHost.handleDocumentApplyChanges`: a server request's
            // changes as one job, each in its own transaction, with one
            // outcome per change. One save for all that wrote. No events:
            // upstream publishes none for it, since the server relays the
            // changes from its audit log.
            "document.applyChanges" => {
                // `handleDocumentApplyChanges`: the changes' own objects go
                // at or above the floor its caller set. Each change runs in
                // its own transaction, so the raise is outside them all.
                if let Some(floor) = JobEnvelope::parse(&header.kind, body)?.object_number_floor {
                    self.sessions
                        .session(&key)?
                        .raise_last_object_number(floor.saturating_sub(1))?;
                }
                let entries: Vec<serde_json::Value> = field(body, "changes")?;
                let mut changes = Vec::with_capacity(entries.len());
                for entry in &entries {
                    changes.push(self.server_change(&key, entry, &payloads)?);
                }
                let outcomes = f::changes::transaction::run_server_changes(
                    self.sessions.session_mut(&key)?,
                    changes,
                    cancel,
                    f::changes::applier::ChangeServices {
                        clock: self.clock.as_ref(),
                        ids: self.ids.as_ref(),
                        fonts: self.fonts.as_ref(),
                    },
                )?;
                let wire = outcomes
                    .iter()
                    .map(f::changes::transaction::ServerChangeOutcome::to_wire)
                    .collect::<Result<Vec<_>, _>>()?;
                let wrote = outcomes.iter().any(|outcome| {
                    matches!(outcome,
                        f::changes::transaction::ServerChangeOutcome::Applied { result, .. }
                            if result.wrote())
                });
                if !wrote {
                    let session = self.sessions.session(&key)?;
                    return Ok(ok(serde_json::json!({
                        "outcomes": wire,
                        "mutationSequence": session.mutation_sequence(),
                    })));
                }
                let written =
                    self.finish_write(&key, crate::session::save::MutationFamily::Ordinary)?;
                let sequence = written.sequence;
                Ok(written.respond(serde_json::json!({
                    "outcomes": wire,
                    "mutationSequence": sequence,
                })))
            }
            // `WorkerHost.handleAnnotationsCreate`: the single verb as a
            // one-op change, its caller's `actor` stamped and nothing else
            // checked (`checkedAuthority`). The draft reached here in the
            // file's coordinates (the boundary converted it); the bytes are
            // the request's payloads, by role.
            "annotations.create" => {
                let page = page_field(body)?;
                let data: serde_json::Map<String, serde_json::Value> = field(body, "draft")?;
                let object_number: Option<serde_json::Number> = optional(body, "objectNumber")?;
                let mut resources: f::changes::op::OpResources =
                    optional(body, "resources")?.unwrap_or_default();
                resources.resolve(&payloads)?;
                let actor = optional::<f::annotation::write::draft::AnnotationActor>(
                    body, "actor",
                )?
                .map(|actor| IdentityClaims {
                    user_id: actor.user_id,
                    group_id: actor.group_id,
                    groups: Vec::new(),
                    display_name: actor.display_name,
                });
                let op = f::changes::op::ChangeOp::AnnotationsCreate {
                    page,
                    data,
                    object_number,
                    resources,
                };
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let authority =
                    f::changes::applier::ChangeAuthority::checked(actor.unwrap_or_default());
                let item = self.apply_one(&key, &op_id, op, Some(authority), cancel)?;
                let event = item.events()?.into_iter().next().ok_or_else(|| {
                    EngineError::with_message(EngineErrorCode::Unknown, "a write published nothing")
                })?;
                let (result, sequence, mut events, written) =
                    self.finish_change(&key, &op_id, None, vec![event], strip_item(item)?)?;
                let mut answer = serde_json::json!({
                    "result": result,
                    "mutationSequence": sequence,
                });
                if let Some(event) = events.pop() {
                    answer["event"] = event;
                }
                Ok(written.respond(answer))
            }
            // `WorkerHost.handleAnnotationsImport`: a bundle's annotations as
            // a one-op change, its caller's `actor` stamped and nothing else
            // checked (`checkedAuthority`). The bundle stays page space: the
            // import measures each create on the page it goes to. Answered
            // `{result, mutationSequence, events}`: one `annotations.created`
            // per annotation.
            "annotations.import" => {
                let mut bundle: f::changes::op::WireBundle = field(body, "bundle")?;
                bundle.resolve(&payloads)?;
                let actor: Option<f::annotation::write::draft::AnnotationActor> =
                    optional(body, "actor")?;
                let op = f::changes::op::ChangeOp::AnnotationsImport {
                    bundle,
                    pages: optional(body, "pages")?,
                    attribution: field(body, "attribution")?,
                    actor: actor.clone(),
                    limits: optional(body, "limits")?.unwrap_or_default(),
                };
                let job = JobEnvelope::parse(&header.kind, body)?;
                let identity = actor
                    .map(|actor| IdentityClaims {
                        user_id: actor.user_id,
                        group_id: actor.group_id,
                        groups: Vec::new(),
                        display_name: actor.display_name,
                    })
                    .unwrap_or_default();
                let authority = f::changes::applier::ChangeAuthority::checked(identity);
                self.import_verb(&key, &job, op, authority, cancel)
            }
            // `WorkerHost.handleAnnotationsUpdate`: the single verb as a
            // one-op change, checked against the request's authority with no
            // protection (`{ ...req.authority, protection: null }`). The
            // patch reached here in the file's coordinates; the bytes are
            // the request's payloads, by role.
            "annotations.update" => {
                let reference: crate::identity::AnnotationRef = field(body, "ref")?;
                let patch: serde_json::Map<String, serde_json::Value> = field(body, "patch")?;
                let mut resources: f::changes::op::OpResources =
                    optional(body, "resources")?.unwrap_or_default();
                resources.resolve(&payloads)?;
                let job = JobEnvelope::parse(&header.kind, body)?;
                let op_id = self.op_id_of(&job)?;
                let authority = match job.authority {
                    Some(mut wire) => {
                        wire.protection = None;
                        Some(change_authority(&wire)?)
                    }
                    None => None,
                };
                let op = f::changes::op::ChangeOp::AnnotationsUpdate {
                    reference,
                    patch,
                    resources,
                    expect: None,
                };
                let item = self.apply_one(&key, &op_id, op, authority, cancel)?;
                let event = item.events()?.into_iter().next().ok_or_else(|| {
                    EngineError::with_message(EngineErrorCode::Unknown, "a write published nothing")
                })?;
                let (result, sequence, mut events, written) =
                    self.finish_change(&key, &op_id, None, vec![event], strip_item(item)?)?;
                let mut answer = serde_json::json!({
                    "result": result,
                    "mutationSequence": sequence,
                });
                if let Some(event) = events.pop() {
                    answer["event"] = event;
                }
                Ok(written.respond(answer))
            }
            // `WorkerHost.handleAnnotationsDelete`: the single verb as a
            // one-op change, checked against the request's authority with no
            // protection (`{ ...req.authority, protection: null }`).
            "annotations.delete" => {
                let reference: crate::identity::AnnotationRef = field(body, "ref")?;
                let op = f::changes::op::ChangeOp::AnnotationsDelete { reference, expect: None };
                self.annotation_verb(&key, &header.kind, body, op, cancel)
            }
            // `WorkerHost.handleAnnotationsReorder`, as the delete.
            "annotations.reorder" => {
                let page = page_field(body)?;
                let refs: Vec<crate::identity::AnnotationRef> = field(body, "refs")?;
                let position: crate::mutation::list_position::AnnotationPosition =
                    field(body, "position")?;
                let op = f::changes::op::ChangeOp::AnnotationsReorder { page, refs, position };
                self.annotation_verb(&key, &header.kind, body, op, cancel)
            }

            // `WorkerHost.handleAnnotationsFlatten`: one write transaction
            // under the request's `opId`, finished by [`Host::finish_flatten`].
            "annotations.flatten" => {
                let page = page_field(body)?;
                let refs: Vec<crate::identity::AnnotationRef> = field(body, "refs")?;
                let usage: f::pages_write::FlattenUsage = field(body, "usage")?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| {
                        f::annotation::write::mutator::flatten(session, page, &refs, usage, cancel)
                    },
                )?;
                // A write always names its pages; applying nothing names none.
                let wrote = !result.meta.affected_pages.is_empty();
                self.finish_flatten(
                    &key,
                    &op_id,
                    value(&result)?,
                    wrote,
                    crate::session::save::DocumentEventInit::AnnotationsFlattened,
                )
            }
            // `WorkerHost.handleAnnotationsExportAppearance`: a read, so no
            // event and no artifact. The bytes travel as the payload.
            "annotations.exportAppearance" => {
                let page = page_field(body)?;
                let refs: Vec<crate::identity::AnnotationRef> = field(body, "refs")?;
                let bytes = f::annotation::write::mutator::export_appearance(
                    self.sessions.session(&key)?,
                    page,
                    &refs,
                    cancel,
                )?;
                Ok(bytes_response(bytes))
            }

            // `WorkerHost.handleFormsSetValue`: the single verb as a one-op
            // change, so its `opId` undoes it and a retry under it gets the
            // first answer. Answered `{result, mutationSequence, event}`,
            // the event `forms.valueSet` as main publishes it.
            "forms.setValue" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let value: f::forms::value::FormFieldValue = field(body, "value")?;
                let op = f::changes::op::ChangeOp::FormsSetValue {
                    field: reference,
                    value,
                    expect: None,
                };
                self.form_verb(&key, &header.kind, body, op, cancel)
            }
            // `WorkerHost.handleFormsReset`: the named fields, or every field
            // without `refs`, as a one-op change. Main publishes one
            // `forms.valueSet` per field that changed (`formResetFacts`), so
            // this answer carries them as `events`, as `document.apply`'s
            // does (see `features::changes`, "Events").
            "forms.reset" => {
                let fields: Option<Vec<f::forms::snapshot::FormFieldRef>> = optional(body, "refs")?;
                let op = f::changes::op::ChangeOp::FormsReset { fields };
                self.form_verb(&key, &header.kind, body, op, cancel)
            }
            // `WorkerHost.handleFormsImport` and `handleFormsImportValues`: a
            // bundle's design, or its values, as a one-op change, under the
            // request's authority (the session's without one), its identity
            // the actor (`actorOf`). Answered `{result, mutationSequence,
            // events}`: one `forms.created` or `forms.valueSet` per field.
            "forms.import" | "forms.importValues" => {
                let mut bundle: f::changes::op::WireBundle = field(body, "bundle")?;
                bundle.resolve(&payloads)?;
                let attribution: f::changes::op::Attribution = field(body, "attribution")?;
                let limits: f::transfer::BundleLimits =
                    optional(body, "limits")?.unwrap_or_default();
                let job = JobEnvelope::parse(&header.kind, body)?;
                let authority = match &job.authority {
                    Some(wire) => change_authority(wire)?,
                    None => f::changes::applier::ChangeAuthority::of_session(
                        self.sessions.session(&key)?,
                    ),
                };
                let actor = Some(f::annotation::write::draft::AnnotationActor {
                    display_name: authority.identity.display_name.clone(),
                    user_id: authority.identity.user_id.clone(),
                    group_id: authority.identity.group_id.clone(),
                });
                let op = if header.kind == "forms.import" {
                    f::changes::op::ChangeOp::FormsImport {
                        bundle,
                        pages: optional(body, "pages")?,
                        attribution,
                        values: Some(field(body, "values")?),
                        actor,
                        limits,
                        may_script: Some(field(body, "mayScript")?),
                    }
                } else {
                    f::changes::op::ChangeOp::FormsImportValues {
                        bundle,
                        attribution,
                        actor,
                        limits,
                    }
                };
                self.import_verb(&key, &job, op, authority, cancel)
            }
            // `WorkerHost.handleFormsRepair`: not a change. A final change
            // (`FINAL_CHANGES`): it ends undo, and its meta says it can't be
            // undone.
            "forms.repair" => {
                let bake: bool = optional(body, "bakeAppearances")?.unwrap_or(false);
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let stamp = crate::session::document::WriteStamp { op_id, undoable: false };
                let result = {
                    let d = self.document(&key)?;
                    f::forms::write::repair(d, bake, &stamp, cancel)?
                };
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormRepaired(value(&result)?),
                )
            }
            // `WorkerHost.handleFormsCreateField` and the other field and
            // widget verbs: each a one-op change, so its `opId` undoes it and
            // a retry under it gets the first answer, answered `{result,
            // mutationSequence, event}` with the item's `type` (and an
            // update's `skipped`) stripped. A create and an update check the
            // request's authority; the rest run unchecked
            // (`checkedAuthority()`), as upstream's do.
            "forms.createField" => {
                let draft: serde_json::Map<String, serde_json::Value> = field(body, "draft")?;
                let object_number: Option<serde_json::Number> = optional(body, "objectNumber")?;
                let widget_object_numbers: Option<Vec<serde_json::Number>> =
                    optional(body, "widgetObjectNumbers")?;
                let op = f::changes::op::ChangeOp::FormsCreate {
                    draft,
                    object_number,
                    widget_object_numbers,
                };
                self.field_verb(&key, &header.kind, body, op, true, cancel)
            }
            "forms.updateField" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let patch: serde_json::Map<String, serde_json::Value> = field(body, "patch")?;
                let op =
                    f::changes::op::ChangeOp::FormsUpdate { field: reference, patch, expect: None };
                self.field_verb(&key, &header.kind, body, op, true, cancel)
            }
            "forms.deleteField" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let op = f::changes::op::ChangeOp::FormsDelete { field: reference, expect: None };
                self.field_verb(&key, &header.kind, body, op, false, cancel)
            }
            "forms.addWidget" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let placement: serde_json::Map<String, serde_json::Value> =
                    field(body, "placement")?;
                let object_number: Option<serde_json::Number> = optional(body, "objectNumber")?;
                let split_object_number: Option<serde_json::Number> =
                    optional(body, "splitObjectNumber")?;
                let op = f::changes::op::ChangeOp::FormsAddWidget {
                    field: reference,
                    placement,
                    object_number,
                    split_object_number,
                };
                self.field_verb(&key, &header.kind, body, op, false, cancel)
            }
            "forms.detachWidget" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let widget: crate::identity::AnnotationRef = field(body, "widget")?;
                let op = f::changes::op::ChangeOp::FormsRemoveWidget { field: reference, widget };
                self.field_verb(&key, &header.kind, body, op, false, cancel)
            }
            "forms.updateWidget" => {
                let widget: crate::identity::AnnotationRef = field(body, "widget")?;
                let patch: serde_json::Map<String, serde_json::Value> = field(body, "patch")?;
                let op =
                    f::changes::op::ChangeOp::FormsUpdateWidget { widget, patch, expect: None };
                self.field_verb(&key, &header.kind, body, op, false, cancel)
            }
            "forms.deleteWidget" => {
                let widget: crate::identity::AnnotationRef = field(body, "widget")?;
                let op = f::changes::op::ChangeOp::FormsDeleteWidget { widget, expect: None };
                self.field_verb(&key, &header.kind, body, op, false, cancel)
            }
            "forms.reorderCalculations" => {
                let fields: Vec<f::forms::snapshot::FormFieldRef> = field(body, "fields")?;
                let position: f::forms::field::FieldPosition = field(body, "position")?;
                let op = f::changes::op::ChangeOp::FormsReorderCalculations { fields, position };
                self.field_verb(&key, &header.kind, body, op, false, cancel)
            }
            "forms.reorderWidgets" => {
                let page = page_field(body)?;
                let widgets: Vec<crate::identity::AnnotationRef> = field(body, "widgets")?;
                let position: crate::mutation::list_position::AnnotationPosition =
                    field(body, "position")?;
                let op = f::changes::op::ChangeOp::FormsReorderWidgets { page, widgets, position };
                self.field_verb(&key, &header.kind, body, op, false, cancel)
            }

            // `WorkerHost.handlePagesSetName` and `handlePagesRemoveName`: a
            // write in its own transaction, whose stamp the result's `meta`
            // carries, answering the new layout (`PageNameResult`).
            "pages.setName" => {
                let name: String = field(body, "name")?;
                let page: crate::identity::PageRef = field(body, "page")?;
                let replace: Option<String> = optional(body, "replace")?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| {
                        f::pages_write::set_name(session, &name, page, replace.as_deref(), cancel)
                    },
                )?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesNamed {
                        name,
                        page_object_number: Some(page.object_number()),
                        result: value(&result)?,
                    },
                )
            }
            "pages.removeName" => {
                let name: String = field(body, "name")?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| f::pages_write::remove_name(session, &name, cancel),
                )?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesNamed {
                        name,
                        page_object_number: None,
                        result: value(&result)?,
                    },
                )
            }
            // `WorkerHost.handlePagesFlatten`, as `annotations.flatten`.
            "pages.flatten" => {
                let pages = pages_field(body)?;
                let usage: f::pages_write::FlattenUsage = field(body, "usage")?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| f::pages_write::flatten_pages(session, &pages, usage, cancel),
                )?;
                let wrote = !result.meta.affected_pages.is_empty();
                self.finish_flatten(
                    &key,
                    &op_id,
                    value(&result)?,
                    wrote,
                    crate::session::save::DocumentEventInit::PagesFlattened,
                )
            }
            // `WorkerHost.handleRedactionApply`, as `pages.flatten`.
            "redaction.apply" => {
                let scope: f::redaction::RedactionScope = field(body, "scope")?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| f::redaction::apply(session, &scope, cancel),
                )?;
                let wrote = !result.meta.affected_pages.is_empty();
                self.finish_flatten(
                    &key,
                    &op_id,
                    value(&result)?,
                    wrote,
                    crate::session::save::DocumentEventInit::RedactionApplied,
                )
            }

            "measure.setScale" => {
                use crate::access::{DocumentAccess, PageAccess};
                let pon = page_field(body)?;
                // `PdfMeasure | null`: `null` clears. An absent `measure` is
                // refused, as `assertWritableMeasure(undefined)` refuses it in
                // the TypeScript, rather than read as a clear.
                let measure = body.get("measure").ok_or_else(|| {
                    EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        "a scale write names no measure (null clears)",
                    )
                })?;
                let measure = Some(measure.clone()).filter(|m| !m.is_null());
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                // `PageScaleResult`, naming the page as the request did. A
                // calibration changes no page pixels and no annotation
                // indices: `meta` names no page and carries the write's stamp.
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| {
                        {
                            let owned = session.document()?;
                            let index = owned.page_index(pon)?;
                            let page = owned.page(pon)?;
                            f::measure::set_scale(
                                owned.document(),
                                index,
                                page.view(),
                                measure.as_ref(),
                                cancel,
                            )?;
                        }
                        let meta = crate::mutation::MutationMeta::local(
                            Vec::new(),
                            &session.write_stamp()?,
                        );
                        Ok(serde_json::json!({
                            "page": crate::identity::PageRef::new(pon),
                            "meta": value(&meta)?,
                        }))
                    },
                )?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PageViewportsChanged(result),
                )
            }
            // `WorkerHost.handlePieceInfoUpdate`: the write in its own
            // transaction, answering `PieceInfoUpdateResult`, the
            // application's data read back and `meta`. `LocalPieceInfoService`
            // publishes nothing: `/PieceInfo` is private application data.
            "pieceInfo.update" => {
                let holder = piece_info_holder(body)?;
                let application = string_field(body, "application")?;
                // Read as a value and checked as upstream checks it, so a
                // patch value outside the vocabulary is `InvalidArg` rather
                // than a decode failure.
                let patch = f::files::piece_info::patch_from_wire(
                    body.get("patch").unwrap_or(&serde_json::Value::Null),
                )?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let clock = self.clock.as_ref();
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| {
                        let d = session.document()?.document();
                        f::files::piece_info::update_piece_info(
                            d,
                            holder,
                            &application,
                            &patch,
                            clock,
                            cancel,
                        )?;
                        let piece_info =
                            f::files::piece_info::read_piece_info(d, holder, &application, cancel)?;
                        let meta = crate::mutation::MutationMeta::local(
                            Vec::new(),
                            &session.write_stamp()?,
                        );
                        Ok(serde_json::json!({ "pieceInfo": value(&piece_info)?,
                            "meta": value(&meta)? }))
                    },
                )?;
                self.finish_quietly(&key, result)
            }
            // `WorkerHost.handlePieceInfoDelete` (a9434c0e2, in place of
            // `pieceInfo.clear`): nothing exists after it, so only `meta`.
            // No event, as for an update.
            "pieceInfo.delete" => {
                let holder = piece_info_holder(body)?;
                let application = string_field(body, "application")?;
                let op_id = self.op_id_of(&JobEnvelope::parse(&header.kind, body)?)?;
                let result = f::changes::transaction::in_transaction(
                    self.sessions.session_mut(&key)?,
                    &op_id,
                    |session| {
                        let d = session.document()?.document();
                        f::files::piece_info::delete_piece_info(d, holder, &application, cancel)?;
                        let meta = crate::mutation::MutationMeta::local(
                            Vec::new(),
                            &session.write_stamp()?,
                        );
                        Ok(serde_json::json!({ "meta": value(&meta)? }))
                    },
                )?;
                self.finish_quietly(&key, result)
            }

            // `WorkerHost`'s `default:` arm, word for word.
            other => {
                let _ = payloads;
                Err(unknown_kind(other))
            }
        }
    }

    /// This thread's font registry, claimed on first use.
    ///
    /// The refusal a second host on one thread gets is the registry's own,
    /// which names the conflict. Reporting it here rather than at
    /// construction means a host that never registers a font never collides.
    fn fonts(&mut self) -> Result<&mut crate::features::fonts::FontRegistry, EngineError> {
        if self.fonts.is_none() {
            self.fonts = Some(crate::features::fonts::FontRegistry::new()?);
        }
        Ok(self.fonts.as_mut().expect("just set"))
    }

    /// Bump the sequence, save the layer artifact, and return the result.
    ///
    /// Every mutation ends here so that the sequence bump and the event are
    /// decided in one place rather than at each of the twenty-odd call sites.
    /// The family comes off the event rather than the caller, which is what
    /// stops a form write being finalized as an ordinary one.
    fn finish(
        &mut self,
        key: &SessionKey,
        event: crate::session::save::DocumentEventInit,
    ) -> Result<HostResponse, EngineError> {
        let result = event.result().cloned().unwrap_or(serde_json::Value::Null);
        self.finish_with(key, event, result)
    }

    /// [`Host::finish`], answering `result` where the event carries more
    /// (a completed signature's event adds its `signingId`).
    fn finish_with(
        &mut self,
        key: &SessionKey,
        event: crate::session::save::DocumentEventInit,
        result: serde_json::Value,
    ) -> Result<HostResponse, EngineError> {
        let family = event.family();
        self.finalize(key, family, Some(event), result)
    }

    /// [`Host::finish`] for a mutation the TypeScript publishes no event
    /// for (`pieceInfo.update`, `pieceInfo.delete`): the sequence still
    /// bumps and a layer still persists, but nobody is told.
    fn finish_quietly(
        &mut self,
        key: &SessionKey,
        result: serde_json::Value,
    ) -> Result<HostResponse, EngineError> {
        self.finalize(key, crate::session::save::MutationFamily::Ordinary, None, result)
    }

    fn finalize(
        &mut self,
        key: &SessionKey,
        family: crate::session::save::MutationFamily,
        event: Option<crate::session::save::DocumentEventInit>,
        result: serde_json::Value,
    ) -> Result<HostResponse, EngineError> {
        use crate::session::save;

        // A final change ends undo for itself and every change before it
        // (`FINAL_CHANGES` in `WorkerHost.finishMutation`): form repair and a
        // completed signing, each of which finishes here with its event. The
        // flattens and redaction apply end it in [`Host::finish_flatten`].
        if event.as_ref().is_some_and(save::DocumentEventInit::ends_undo) {
            self.sessions.session_mut(key)?.ledger_mut().end_undo();
        }
        let written = self.finish_write(key, family)?;
        let mut answer = serde_json::json!({
            "result": result,
            "mutationSequence": written.sequence,
        });
        if let Some(event) = event {
            answer["event"] = serde_json::to_value(self.stamp(key, event)?).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::WireFormat,
                    format!("the event could not be serialized: {e}"),
                )
            })?;
        }
        Ok(written.respond(answer))
    }

    /// `finishMutation` and `saveLayerArtifact`: commit the write's
    /// transaction, bump the sequence, and save a layer session's artifact,
    /// named in the answer as upstream names it (`artifact`, its bytes the
    /// response's payload, or `artifactFile` when the request gave an
    /// `artifactPath`), with the layer's last object number.
    fn finish_write(
        &mut self,
        key: &SessionKey,
        family: crate::session::save::MutationFamily,
    ) -> Result<Written, EngineError> {
        let finished = crate::session::save::finish_mutation(
            self.sessions.session_mut(key)?,
            family,
            &CancelToken::never(),
        )?;
        let (artifact, payloads) = match (finished.artifact, self.artifact_path.take()) {
            (None, _) => (None, Vec::new()),
            (Some(bytes), Some(path)) => {
                std::fs::write(&path, &bytes).map_err(|e| {
                    EngineError::with_message(
                        EngineErrorCode::Unknown,
                        format!("writing the layer artifact to '{path}' failed: {e}"),
                    )
                })?;
                let file = serde_json::json!({
                    "path": path,
                    "lastObjectNumber": finished.last_object_number,
                });
                (Some(("artifactFile", file)), Vec::new())
            }
            (Some(bytes), None) => {
                let artifact = serde_json::json!({
                    "payload": 0,
                    "size": bytes.len(),
                    "lastObjectNumber": finished.last_object_number,
                });
                (Some(("artifact", artifact)), vec![bytes])
            }
        };
        Ok(Written { sequence: finished.mutation_sequence, artifact, payloads })
    }

    /// Finish a flatten or a redaction (`handlePagesFlatten`,
    /// `handleAnnotationsFlatten`, `handleRedactionApply`),
    /// answered `{result, wrote, mutationSequence, event}`. One that wrote is
    /// a final change: it ends undo for itself and every change before it,
    /// the sequence bumps once, a layer saves its artifact, and the event
    /// goes out as the one fact of the write `op_id` (`publishWrite`). One
    /// that wrote nothing is no mutation: no event, no artifact, and the
    /// sequence it found, which upstream leaves out of the answer (a kept
    /// divergence, 87b61b533).
    fn finish_flatten(
        &mut self,
        key: &SessionKey,
        op_id: &str,
        result: serde_json::Value,
        wrote: bool,
        event: impl FnOnce(serde_json::Value) -> crate::session::save::DocumentEventInit,
    ) -> Result<HostResponse, EngineError> {
        use crate::session::save;

        if !wrote {
            let session = self.sessions.session(key)?;
            return Ok(ok(serde_json::json!({
                "result": result,
                "wrote": false,
                "mutationSequence": session.mutation_sequence(),
            })));
        }
        self.sessions.session_mut(key)?.ledger_mut().end_undo();
        let written = self.finish_write(key, save::MutationFamily::Ordinary)?;
        let init = value(&event(result.clone()))?;
        let mut events = self.stamp_change(key, op_id, None, vec![init])?;
        let answer = serde_json::json!({
            "result": result,
            "wrote": true,
            "mutationSequence": written.sequence,
            "event": events.pop(),
        });
        Ok(written.respond(answer))
    }

    /// Stamp `event` as this engine's own: a local origin under the
    /// session's id, at the clock's now.
    fn stamp(
        &self,
        key: &SessionKey,
        event: crate::session::save::DocumentEventInit,
    ) -> Result<crate::session::save::DocumentEvent, EngineError> {
        use crate::session::save;

        let session = self.sessions.session(key)?;
        Ok(save::publish(
            event,
            save::EventOrigin {
                kind: save::OriginKind::Local,
                session_id: session.session_id().to_owned(),
                sub: None,
                ts: self.clock.now().epoch_millis,
                server_id: None,
                tx: None,
                undo_of: None,
            },
        ))
    }

    /// The `opId` a write runs under (`opIdOf`): the request's, or a fresh
    /// one. The request's must be 1 to 255 visible ASCII characters, or the
    /// write is refused with `InvalidArg`.
    fn op_id_of(&self, job: &JobEnvelope) -> Result<String, EngineError> {
        match &job.op_id {
            None => Ok(self.ids.next_uuid_v4()),
            Some(op_id) => {
                check_op_id(op_id)?;
                Ok(op_id.clone())
            }
        }
    }

    /// A form value verb as a one-op change under the request's
    /// authority (`handleFormsSetValue`, `handleFormsReset`): its item
    /// without `type` and `skipped`, and its events. One event answers as
    /// `event`; a reset, whose facts are one per field, as `events`.
    fn form_verb(
        &mut self,
        key: &SessionKey,
        kind: &str,
        body: &serde_json::Value,
        op: crate::features::changes::op::ChangeOp,
        cancel: &CancelToken,
    ) -> Result<HostResponse, EngineError> {
        let job = JobEnvelope::parse(kind, body)?;
        let op_id = self.op_id_of(&job)?;
        let authority = job.authority.as_ref().map(change_authority).transpose()?;
        let item = self.apply_one(key, &op_id, op, authority, cancel)?;
        let events = item.events()?;
        let (result, sequence, mut events, written) =
            self.finish_change(key, &op_id, None, events, strip_item(item)?)?;
        let mut answer = serde_json::json!({
            "result": result,
            "mutationSequence": sequence,
        });
        if kind == "forms.reset" {
            answer["events"] = serde_json::Value::Array(events);
        } else if let Some(event) = events.pop() {
            answer["event"] = event;
        }
        Ok(written.respond(answer))
    }

    /// A single field or widget verb as a one-op change
    /// (`handleFormsCreateField` and its siblings): checked against the
    /// request's authority when `checked` (a create and an update), else
    /// run unchecked as `checkedAuthority()` runs it. Answered `{result,
    /// mutationSequence, event}`, the item's `type` and `skipped` stripped.
    fn field_verb(
        &mut self,
        key: &SessionKey,
        kind: &str,
        body: &serde_json::Value,
        op: crate::features::changes::op::ChangeOp,
        checked: bool,
        cancel: &CancelToken,
    ) -> Result<HostResponse, EngineError> {
        let job = JobEnvelope::parse(kind, body)?;
        let op_id = self.op_id_of(&job)?;
        let authority = match (checked, job.authority.as_ref()) {
            (true, Some(wire)) => Some(change_authority(wire)?),
            (true, None) => None,
            (false, _) => Some(crate::features::changes::applier::ChangeAuthority::checked(
                crate::auth::scope::IdentityClaims::default(),
            )),
        };
        let item = self.apply_one(key, &op_id, op, authority, cancel)?;
        let events = item.events()?;
        let (result, sequence, mut events, written) =
            self.finish_change(key, &op_id, None, events, strip_item(item)?)?;
        let mut answer = serde_json::json!({
            "result": result,
            "mutationSequence": sequence,
        });
        if let Some(event) = events.pop() {
            answer["event"] = event;
        }
        Ok(written.respond(answer))
    }

    /// A single annotation verb as a one-op change, checked against the
    /// request's authority with no protection (`{ ...req.authority,
    /// protection: null }`), answered `{result, mutationSequence, event}`
    /// with the item's `type` and `page` stripped (`handleAnnotationsDelete`,
    /// `handleAnnotationsReorder`).
    fn annotation_verb(
        &mut self,
        key: &SessionKey,
        kind: &str,
        body: &serde_json::Value,
        op: crate::features::changes::op::ChangeOp,
        cancel: &CancelToken,
    ) -> Result<HostResponse, EngineError> {
        let job = JobEnvelope::parse(kind, body)?;
        let op_id = self.op_id_of(&job)?;
        let authority = match job.authority {
            Some(mut wire) => {
                wire.protection = None;
                Some(change_authority(&wire)?)
            }
            None => None,
        };
        let item = self.apply_one(key, &op_id, op, authority, cancel)?;
        let event = item.events()?.into_iter().next().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::Unknown, "a write published nothing")
        })?;
        let (result, sequence, mut events, written) =
            self.finish_change(key, &op_id, None, vec![event], strip_item(item)?)?;
        let mut answer = serde_json::json!({
            "result": result,
            "mutationSequence": sequence,
        });
        if let Some(event) = events.pop() {
            answer["event"] = event;
        }
        Ok(written.respond(answer))
    }

    /// An import verb as a one-op change (`handleAnnotationsImport`,
    /// `handleFormsImport`, `handleFormsImportValues`): kept under its
    /// `opId` with the record that undoes it, so a retry under it gets the
    /// first answer. One that left everything out wrote nothing: it answers
    /// its result with no mutation finished and no events.
    fn import_verb(
        &mut self,
        key: &SessionKey,
        job: &JobEnvelope,
        op: crate::features::changes::op::ChangeOp,
        authority: crate::features::changes::applier::ChangeAuthority,
        cancel: &CancelToken,
    ) -> Result<HostResponse, EngineError> {
        let op_id = self.op_id_of(job)?;
        let item = self.apply_one_item(key, &op_id, op, Some(authority), cancel)?;
        if !item.wrote() {
            let sequence = self.sessions.session(key)?.mutation_sequence();
            let answer = serde_json::json!({
                "result": strip_item(item)?,
                "mutationSequence": sequence,
                "events": [],
            });
            return Ok(HostResponse { json: envelope(answer), payloads: Vec::new() });
        }
        let events = item.events()?;
        let (result, sequence, events, written) =
            self.finish_change(key, &op_id, None, events, strip_item(item)?)?;
        let answer = serde_json::json!({
            "result": result,
            "mutationSequence": sequence,
            "events": events,
        });
        Ok(written.respond(answer))
    }

    /// `applyOne`: a single verb as a one-op change in the file's own
    /// coordinates (the host's boundary converts its request and answer),
    /// answering the op's item.
    fn apply_one(
        &mut self,
        key: &SessionKey,
        op_id: &str,
        op: crate::features::changes::op::ChangeOp,
        authority: Option<crate::features::changes::applier::ChangeAuthority>,
        cancel: &CancelToken,
    ) -> Result<crate::features::changes::item::ChangeItem, EngineError> {
        let kind = op.type_name();
        match self.apply_one_item(key, op_id, op, authority, cancel)? {
            item if item.wrote() => Ok(item),
            _ => Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("{kind} wrote nothing"),
            )),
        }
    }

    /// The op's item of a one-op change, whether or not it wrote.
    fn apply_one_item(
        &mut self,
        key: &SessionKey,
        op_id: &str,
        op: crate::features::changes::op::ChangeOp,
        authority: Option<crate::features::changes::applier::ChangeAuthority>,
        cancel: &CancelToken,
    ) -> Result<crate::features::changes::item::ChangeItem, EngineError> {
        use crate::features::changes::op::{self, Change};
        use crate::features::changes::{applier, fingerprint, transaction};

        let change = Change::Ops(vec![op]);
        let fingerprint = fingerprint::change_fingerprint(&op::change_value(&change)?);
        let session = self.sessions.session_mut(key)?;
        let authority = authority.unwrap_or_else(|| applier::ChangeAuthority::of_session(session));
        let services = applier::ChangeServices {
            clock: self.clock.as_ref(),
            ids: self.ids.as_ref(),
            fonts: self.fonts.as_ref(),
        };
        let answered = transaction::run_change(
            session,
            op_id,
            fingerprint,
            change,
            &authority,
            cancel,
            services,
            applier::ChangeSpace::File,
        )?;
        answered.result.items.into_iter().next().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::Unknown, "a one-op change answered no item")
        })
    }

    /// Finish a write that ran as a change: its transaction commits, the
    /// sequence bumps, a layer saves its artifact, and each event is stamped
    /// as one fact of the write `op_id` (`publishChange`). Answers the
    /// result as given, the sequence, the stamped events and what was
    /// written.
    fn finish_change(
        &mut self,
        key: &SessionKey,
        op_id: &str,
        undo_of: Option<String>,
        events: Vec<serde_json::Value>,
        result: serde_json::Value,
    ) -> Result<(serde_json::Value, u64, Vec<serde_json::Value>, Written), EngineError> {
        let written = self.finish_write(key, crate::session::save::MutationFamily::Ordinary)?;
        let events = self.stamp_change(key, op_id, undo_of, events)?;
        Ok((result, written.sequence, events, written))
    }

    /// Stamp a change's events as this engine's own, each one fact of the
    /// write `op_id`: `origin.tx` counts them, and an undo's carry the
    /// `opId` it undid.
    fn stamp_change(
        &self,
        key: &SessionKey,
        op_id: &str,
        undo_of: Option<String>,
        events: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        use crate::session::save;

        let session = self.sessions.session(key)?;
        let ts = self.clock.now().epoch_millis;
        let count = events.len();
        events
            .into_iter()
            .enumerate()
            .map(|(index, mut event)| {
                let origin = save::EventOrigin {
                    kind: save::OriginKind::Local,
                    session_id: session.session_id().to_owned(),
                    sub: None,
                    ts,
                    server_id: None,
                    tx: Some(save::EventTx { id: op_id.to_owned(), index, count }),
                    undo_of: undo_of.clone(),
                };
                event["origin"] = value(&origin)?;
                Ok(event)
            })
            .collect()
    }

    /// One change of a `document.applyChanges` request (`ServerChange`).
    fn server_change(
        &self,
        key: &SessionKey,
        entry: &serde_json::Value,
        payloads: &[Vec<u8>],
    ) -> Result<crate::features::changes::transaction::ServerChange, EngineError> {
        use crate::features::changes::{applier, op, record, transaction};

        let op_id: String = field(entry, "opId")?;
        check_op_id(&op_id)?;
        let mut change = op::parse_change(entry.get("change").ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::WireFormat, "a change has no 'change'")
        })?)?;
        // A resource names a payload of the request, whichever change it
        // is in.
        op::resolve_payloads(&mut change, payloads)?;
        let authority = match optional::<WireAuthority>(entry, "authority")? {
            Some(wire) => change_authority(&wire)?,
            None => applier::ChangeAuthority::of_session(self.sessions.session(key)?),
        };
        let record = match entry.get("record") {
            None => None,
            Some(serde_json::Value::Null) => Some(None),
            Some(_) => Some(Some(field::<record::ChangeRecord>(entry, "record")?)),
        };
        Ok(transaction::ServerChange { op_id, change, authority, record })
    }

    /// Answer `result` with a notice beside it in the envelope: the
    /// signing notices, which follow a call that is not a mutation.
    fn answer_with_notice(
        &self,
        key: &SessionKey,
        result: serde_json::Value,
        notice: crate::session::save::DocumentEventInit,
        payloads: Vec<Vec<u8>>,
    ) -> Result<HostResponse, EngineError> {
        let event = value(&self.stamp(key, notice)?)?;
        let json = serde_json::to_string(&Envelope {
            result: Some(result),
            error: None,
            event: Some(event),
        })
        .unwrap_or_else(|_| {
            String::from(r#"{"error":{"name":"EngineError","code":"Unknown","message":"Unknown"}}"#)
        });
        Ok(HostResponse { json, payloads })
    }

    /// The open document behind a key.
    fn document(&self, key: &SessionKey) -> Result<&pdfium::Document, EngineError> {
        Ok(self.sessions.session(key)?.document()?.document())
    }

    /// Run a closure over one page, named by object number.
    fn on_page(
        &mut self,
        key: &SessionKey,
        body: &serde_json::Value,
        read: impl FnOnce(&dyn crate::access::PageAccess) -> Result<serde_json::Value, EngineError>,
    ) -> Result<HostResponse, EngineError> {
        let page = page_field(body)?;
        let opened = self.sessions.session(key)?.acquire_page(page)?;
        Ok(ok(read(&opened)?))
    }

    /// The same for a closure whose result is not JSON.
    fn on_page_raw<T>(
        &mut self,
        key: &SessionKey,
        body: &serde_json::Value,
        read: impl FnOnce(&dyn crate::access::PageAccess) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let page = page_field(body)?;
        let opened = self.sessions.session(key)?.acquire_page(page)?;
        read(&opened)
    }

    /// Open a layer over a base document.
    ///
    /// Two frames, one path: the base arrives either as bytes or as a path,
    /// and everything after that is identical. The base is registered under
    /// its OWN key rather than the layer's, because several layer sessions
    /// over one base should share the one open document, which is the whole
    /// reason the base registry exists.
    ///
    /// `WorkerHost.handleOpen`: the session is `docId` with the optional
    /// `layerName`, and `layer` (`LayerOpenSource`) says what the layer
    /// starts from. Its bytes, for a `raw-delta` or an `artifact`, are the
    /// payload after the base's: payload 1 beside `open.layerMemBase`'s base
    /// bytes, payload 0 for `open.layerFileBase`. `baseSha256` is an identity
    /// claim that saves the runtime a hash pass; this engine does not take
    /// it.
    fn open_layer(
        &mut self,
        header: &RequestHeader,
        body: &serde_json::Value,
        payloads: Payloads,
    ) -> Result<serde_json::Value, EngineError> {
        let key = header.key()?;
        let options = OpenOptions::parse(body)?;
        let base_key = string_field(body, "baseKey")?;
        let password: Option<String> = optional(body, "password")?;
        let file_base = header.kind == "open.layerFileBase";
        let source = layer_source(body, &payloads, if file_base { 0 } else { 1 })?;

        let opened = if file_base {
            let path = string_field(body, "basePath")?;
            self.sessions.open_layer_file_from(
                key.clone(),
                &base_key,
                std::path::Path::new(&path),
                &source,
                password.as_deref(),
                SessionAuthority::unrestricted(),
            )
        } else {
            // The base bytes are a payload, never a JSON field, for the same
            // reason `open.fatMem`'s are.
            let bytes = payload(&payloads, 0, "the base document")?;
            self.sessions.open_layer_memory_from(
                key.clone(),
                &base_key,
                bytes,
                &source,
                password.as_deref(),
                SessionAuthority::unrestricted(),
            )
        };
        self.parked_or(&key, opened)?;
        self.open_answer(&key, options, password.as_deref())
    }

    /// `WorkerHost.handleOpen`: a password failure is a state, not an
    /// error. The registry parked the session with its key and how to load
    /// it, so an error here would make a caller tear down something that is
    /// waiting for a password; the open answers locked instead.
    fn parked_or(
        &self,
        key: &SessionKey,
        opened: Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        match opened {
            Err(error)
                if crate::session::registry::is_password_open_error(&error)
                    && self.sessions.is_locked(key) =>
            {
                Ok(())
            }
            other => other,
        }
    }

    fn open_fat_memory(
        &mut self,
        header: &RequestHeader,
        body: &serde_json::Value,
        mut payloads: Payloads,
    ) -> Result<serde_json::Value, EngineError> {
        // The document's bytes are the first payload, never a field: a PDF
        // base64'd through JSON is the same megabytes twice.
        if payloads.is_empty() {
            return Err(EngineError::with_message(
                EngineErrorCode::WireFormat,
                "an open carries the document's bytes as its first payload",
            ));
        }
        let bytes = payloads.remove(0);
        let password = body.get("password").and_then(serde_json::Value::as_str);
        let key = header.key()?;
        let options = OpenOptions::parse(body)?;
        // A native extension of the wire: the TypeScript enforces scope in
        // `engine/main`, above the worker, and a native host has no such
        // layer. Absent is `["*"]`, everything.
        let scope: Vec<String> = optional(body, "scope")?.unwrap_or_else(|| vec!["*".to_string()]);
        let identity: WireIdentity = optional(body, "identity")?.unwrap_or_default();
        let authority = SessionAuthority::new(&scope, identity.into(), Default::default())
            .map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("invalid scope: {e}"),
                )
            })?;
        let opened = self.sessions.open_memory(key.clone(), bytes, password, authority);
        self.parked_or(&key, opened)?;
        self.open_answer(&key, options, password)
    }

    /// The open answer (`{ tag: 'open', docId, security, protection,
    /// objectNumbers?, lastObjectNumber? }`), once the session for `key`
    /// exists, open or parked.
    ///
    /// `WorkerHost.handleOpen` sets the session's object number authority
    /// and signed-document policy before it loads, so a parked session keeps
    /// them; and only an open that loaded hands out the numbers it asked
    /// for and reports the layer's last object number. A failed reserve
    /// fails the open and leaves the session open, as upstream's does. A
    /// parked open answers the security probe alone (no `protection`), and
    /// `passwordRejected` when a password was given and was wrong.
    fn open_answer(
        &mut self,
        key: &SessionKey,
        options: OpenOptions,
        password: Option<&str>,
    ) -> Result<serde_json::Value, EngineError> {
        if let Some(session) = self.sessions.configure(key) {
            session.set_object_number_authority(options.object_numbers);
            session.set_signed_document_policy(options.signed_document_policy);
        }
        let mut answer = serde_json::json!({
            "docId": key.doc_id,
            "security": value(&self.security_on_open(key)?)?,
        });
        if self.sessions.is_locked(key) {
            if password.is_some_and(|password| !password.is_empty()) {
                answer["passwordRejected"] = serde_json::Value::Bool(true);
            }
            return Ok(answer);
        }
        answer["protection"] = self.protection_of(key);
        let session = self.sessions.session_mut(key)?;
        if let Some(count) = options.reserve_object_numbers {
            answer["objectNumbers"] = value(&session.reserve_object_numbers(count)?)?;
        }
        if let Some(last) = session.last_object_number()? {
            answer["lastObjectNumber"] = serde_json::json!(last);
        }
        Ok(answer)
    }

    /// `pages.workingSet` (`WorkerHost.setWorkingSet`): what `view` shows of
    /// an open session's pages, replacing its last set. Upstream looks the
    /// session up first and never answers, so a request for no open session,
    /// or one it cannot read, changes nothing and reports nothing.
    fn set_working_set(&mut self, header: &RequestHeader, body: &serde_json::Value) {
        let Ok(key) = header.key() else { return };
        let Ok(session) = self.sessions.session(&key) else { return };
        let Some(pool) = session.page_pool() else { return };
        let view: Option<String> = optional(body, "view").ok().flatten();
        let pages: Option<Vec<crate::session::pages::WorkingSetPage>> =
            optional(body, "pages").ok().flatten();
        if let (Some(view), Some(pages)) = (view, pages) {
            pool.set_working_set(&view, &pages);
        }
    }

    /// What an open document's signatures forbid (`WorkerHost.probeProtection`),
    /// `null` for a locked, unsigned or unreadable one.
    fn protection_of(&self, key: &SessionKey) -> serde_json::Value {
        self.sessions
            .session(key)
            .ok()
            .and_then(|s| s.authority().protection())
            .and_then(|p| serde_json::to_value(p).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    /// The security probe an open reports.
    ///
    /// An open answers with one because the client handle comes up carrying
    /// it: a viewer decides whether to show a password prompt from this, and
    /// a second round trip to ask would be a visible delay on every document.
    ///
    /// A password-parked session reports `password required` rather than
    /// failing the open. That is a STATE, not an error: the session holds its
    /// key and its bytes, and a later `document.checkPasswordPermissions`
    /// does the actual load without transferring the document again.
    fn security_on_open(
        &self,
        key: &SessionKey,
    ) -> Result<crate::features::security::state::SecurityProbeInfo, EngineError> {
        use crate::features::security::{reader::read_live, state::password_blocked};

        let session = match self.sessions.session(key) {
            Ok(session) => session,
            // `session` refuses a locked one with `DocPasswordRequired`
            // rather than serving it, which is the parked case and the one
            // answer worth turning back into a probe.
            Err(error) if error.is(EngineErrorCode::DocPasswordRequired) => {
                return Ok(password_blocked(self.clock.now().epoch_millis));
            }
            Err(error) => return Err(error),
        };
        Ok(read_live(session.document()?.document(), self.clock.as_ref()))
    }

    /// `WorkerHost.handleClose`: every session of the document, answered
    /// with the tag alone (`{ tag: 'close' }`), which this wire spells as a
    /// null result. Closing nothing is the state the caller asked for.
    fn close_document(&mut self, header: &RequestHeader) -> Result<serde_json::Value, EngineError> {
        let doc_id = header.doc_id.as_deref().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::WireFormat, "a close names no document")
        })?;
        self.sessions.close_document(doc_id);
        Ok(serde_json::Value::Null)
    }

    /// `WorkerHost.handleLayerClose`: one session, answered as `close` is.
    fn close_layer(&mut self, header: &RequestHeader) -> Result<serde_json::Value, EngineError> {
        let key = header.key()?;
        self.sessions.close_layer(&key);
        Ok(serde_json::Value::Null)
    }
}

/// What finishing a write leaves beside its answer (`finishMutation`): the
/// sequence, and the layer artifact as the answer names it, with its bytes.
struct Written {
    sequence: u64,
    artifact: Option<(&'static str, serde_json::Value)>,
    payloads: Payloads,
}

impl Written {
    /// `answer`, naming the artifact beside its own fields
    /// (`{ ...payload, ...saved.payload }`), its bytes the payloads.
    fn respond(self, mut answer: serde_json::Value) -> HostResponse {
        if let Some((name, artifact)) = self.artifact {
            answer[name] = artifact;
        }
        HostResponse { json: envelope(answer), payloads: self.payloads }
    }
}

/// A successful response with no payloads, which is most of them.
fn ok(result: serde_json::Value) -> HostResponse {
    HostResponse {
        json: serde_json::to_string(&Envelope { result: Some(result), error: None, event: None })
            .unwrap_or_else(|_| {
                String::from(
                    r#"{"error":{"name":"EngineError","code":"Unknown","message":"Unknown"}}"#,
                )
            }),
        payloads: Vec::new(),
    }
}

/// Serialize an operation's result into the envelope's `result`.
fn value<T: Serialize>(result: &T) -> Result<serde_json::Value, EngineError> {
    serde_json::to_value(result).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::WireFormat,
            format!("the result could not be serialized: {e}"),
        )
    })
}

/// What a raster's JSON says about the pixels in its payload.
fn raster_json(raster: &crate::features::render::raster::PageRaster) -> serde_json::Value {
    serde_json::json!({
        "width": raster.width,
        "height": raster.height,
        "stride": raster.stride,
    })
}

/// A response whose result is an encoded image.
fn encoded_response(encoded: crate::features::server::EncodedImage) -> HostResponse {
    HostResponse { json: envelope(encoded_json(&encoded)), payloads: vec![encoded.bytes] }
}

/// What an encoded image's JSON says about the bytes in its payload.
fn encoded_json(encoded: &crate::features::server::EncodedImage) -> serde_json::Value {
    serde_json::json!({
        "mimeType": encoded.content_type,
        "width": encoded.width,
        "height": encoded.height,
        "size": encoded.bytes.len(),
    })
}

/// A response carrying one decoded embedded file.
///
/// The metadata rides in the JSON and the bytes in a payload, so a caller can
/// show the name and the type without waiting for a copy of a file it may be
/// about to hand straight to a download.
fn attachment_response(
    file: crate::features::files::attachments::AttachmentContent,
) -> HostResponse {
    HostResponse {
        json: envelope(serde_json::json!({
            "name": file.name,
            "mimeType": file.mime_type,
            "size": file.size,
        })),
        payloads: vec![file.bytes],
    }
}

/// A response whose whole result is bytes.
///
/// The JSON carries the length so a host can size its copy before reading
/// the payload, which is what a transferable buffer on the other side wants.
/// A bundle as a read answers it (`{ tag, bundle }` without its tag): its
/// manifest, and each resource a payload of the response, `{ "payload": n }`,
/// in order.
fn bundle_response(bundle: crate::features::transfer::Bundle) -> HostResponse {
    let mut answer = bundle.manifest;
    let mut payloads = Vec::with_capacity(bundle.resources.len());
    let mut resources = serde_json::Map::new();
    for (id, bytes) in bundle.resources {
        resources.insert(id, serde_json::json!({ "payload": payloads.len() }));
        payloads.push(bytes);
    }
    answer.insert("resources".into(), serde_json::Value::Object(resources));
    HostResponse { json: envelope(serde_json::json!({ "bundle": answer })), payloads }
}

/// `gate('sign-some-field')` (`ScopeGuard.assertSomeFieldWrite('sign')`):
/// the session may sign some signature field, its group's or any; the
/// write checks which.
fn require_some_signing(
    session: &crate::session::document::DocumentSession,
) -> Result<(), EngineError> {
    if session.authority().allows_some_field_write(crate::auth::scope::FieldWriteAction::Sign) {
        return Ok(());
    }
    session.require(DocCapability::DocSign)
}

/// `assertRewriteKeepsSignatures`: a rewrite drops every revision, and with
/// them every signature, so a signed document refuses it unless the session
/// permits breaking signatures (`signedDocumentPolicy: 'permit'`). A
/// document whose signatures cannot be read is not known to be signed.
fn assert_rewrite_keeps_signatures(
    session: &crate::session::document::DocumentSession,
    mode: crate::features::save::SaveMode,
) -> Result<(), EngineError> {
    use crate::features::signature::types::SignedDocumentPolicy;
    if mode != crate::features::save::SaveMode::Rewrite
        || session.signed_document_policy() != SignedDocumentPolicy::Protect
    {
        return Ok(());
    }
    let protection =
        crate::features::signature::reader::read_protection(session.document()?.document()).ok();
    if protection.is_some_and(|protection| protection.judged.is_some()) {
        return Err(EngineError::with_message(
            EngineErrorCode::ProtectedDocument,
            "the document is signed: a rewrite save would void every signature (use an incremental save)",
        ));
    }
    Ok(())
}

/// The bytes a request's resource names, `{ "payload": n }`.
fn resource_payload<'a>(
    resource: &crate::features::changes::op::ResourceBytes,
    payloads: &'a Payloads,
) -> Result<&'a [u8], EngineError> {
    match resource {
        crate::features::changes::op::ResourceBytes::Payload(index) => {
            payload(payloads, *index, "the appearance PDF")
        }
        crate::features::changes::op::ResourceBytes::Bytes(_) => Err(EngineError::with_message(
            EngineErrorCode::WireFormat,
            "a resource names its payload",
        )),
    }
}

fn bytes_response(bytes: Vec<u8>) -> HostResponse {
    HostResponse {
        json: envelope(serde_json::json!({ "size": bytes.len() })),
        payloads: vec![bytes],
    }
}

fn envelope(result: serde_json::Value) -> String {
    serde_json::to_string(&Envelope { result: Some(result), error: None, event: None })
        .unwrap_or_else(|_| {
            String::from(r#"{"error":{"name":"EngineError","code":"Unknown","message":"Unknown"}}"#)
        })
}

fn form_model_missing() -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, "the form model could not be loaded")
}

/// One binary input, by position.
///
/// Positional rather than keyed, because the transfer list on the other side
/// is an array and naming the slots here would invent a correspondence the
/// wire does not carry.
fn payload<'a>(payloads: &'a Payloads, index: usize, what: &str) -> Result<&'a [u8], EngineError> {
    payloads.get(index).map(Vec::as_slice).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::WireFormat,
            format!("{what} should have arrived as payload {index}, and no payload did"),
        )
    })
}

/// Render options, defaulted, their target in the file's coordinates (the
/// boundary converted it).
///
/// Optional on the wire because the common render is "the page, at its own
/// size". A required field here would make the simplest request an error.
fn render_options(
    body: &serde_json::Value,
) -> Result<crate::features::render::raster::RenderOptions, EngineError> {
    Ok(optional(body, "options")?.unwrap_or_default())
}

/// One named argument, decoded.
fn field<T: for<'de> Deserialize<'de>>(
    body: &serde_json::Value,
    name: &str,
) -> Result<T, EngineError> {
    let value = body.get(name).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::WireFormat,
            format!("the request has no '{name}'"),
        )
    })?;
    serde_json::from_value(value.clone()).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::WireFormat,
            format!("'{name}' is not the shape this operation takes: {e}"),
        )
    })
}

fn string_field(body: &serde_json::Value, name: &str) -> Result<String, EngineError> {
    field(body, name)
}

/// One argument that may be absent.
///
/// Absent and null are the same answer here, because a wire that omits an
/// optional field and one that sends it as null both mean "not set", and
/// distinguishing them would make a caller's JSON serializer part of the
/// contract. Where the difference is load-bearing the operation takes a
/// [`crate::dto::Patch`] instead.
fn optional<T: for<'de> Deserialize<'de>>(
    body: &serde_json::Value,
    name: &str,
) -> Result<Option<T>, EngineError> {
    match body.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(_) => field(body, name).map(Some),
    }
}

/// What an `opId` may be: what an HTTP header carries (`OP_ID`, 1 to 255
/// visible ASCII characters).
fn check_op_id(op_id: &str) -> Result<(), EngineError> {
    let visible = op_id.bytes().all(|byte| (0x21..=0x7e).contains(&byte));
    if visible && (1..=255).contains(&op_id.len()) {
        return Ok(());
    }
    Err(EngineError::with_message(
        EngineErrorCode::InvalidArg,
        "opId must be 1 to 255 visible ASCII characters",
    )
    .with_details(serde_json::json!({ "field": "opId" })))
}

/// A request's `ChangeAuthority`: who the change acts for, and the grants
/// each op is checked against (`null`: nothing is checked).
fn change_authority(
    wire: &WireAuthority,
) -> Result<crate::features::changes::applier::ChangeAuthority, EngineError> {
    let user_id = wire.identity.user_id.clone();
    let grants = match &wire.grants {
        None => None,
        Some(grants) => {
            let identity = IdentityClaims {
                user_id: user_id.clone(),
                group_id: wire.identity.group_id.clone(),
                groups: Vec::new(),
                display_name: wire.identity.display_name.clone(),
            };
            let mut authority =
                SessionAuthority::new(&grants.scope, identity, grants.pdf_bits.into()).map_err(
                    |e| {
                        EngineError::with_message(
                            EngineErrorCode::InvalidArg,
                            format!("the authority's scope is not valid: {e}"),
                        )
                    },
                )?;
            authority.set_protection(wire.protection.clone());
            Some(authority)
        }
    };
    let identity = IdentityClaims {
        user_id: user_id.clone(),
        group_id: wire.identity.group_id.clone(),
        groups: Vec::new(),
        display_name: wire.identity.display_name.clone(),
    };
    Ok(crate::features::changes::applier::ChangeAuthority { user_id, identity, grants })
}

/// A single verb's result: its item without the item's `type` and
/// `skipped`, as `WorkerHost` strips them.
fn strip_item(
    item: crate::features::changes::item::ChangeItem,
) -> Result<serde_json::Value, EngineError> {
    use crate::features::changes::item::ChangeItem;
    match item {
        ChangeItem::MetadataUpdate { result, .. } => value(&result),
        ChangeItem::MetadataUpdateCustom { result, .. } => value(&result),
        ChangeItem::AttachmentsCreate { attachment, meta }
        | ChangeItem::AttachmentsRestore { attachment, meta } => {
            Ok(serde_json::json!({ "attachment": value(&attachment)?, "meta": value(&meta)? }))
        }
        ChangeItem::AttachmentsDelete { meta } => Ok(serde_json::json!({ "meta": value(&meta)? })),
        // Without its `page` too, as `handleAnnotationsCreate` strips it.
        ChangeItem::AnnotationsCreate { annotation, meta, .. } => {
            Ok(serde_json::json!({ "annotation": annotation, "meta": value(&meta)? }))
        }
        // Without its `page` and `skipped`, as `handleAnnotationsUpdate`
        // strips them.
        ChangeItem::AnnotationsUpdate { annotation, appearance, meta, .. } => {
            Ok(serde_json::json!({ "annotation": annotation, "appearance": value(&appearance)?,
                "meta": value(&meta)? }))
        }
        ChangeItem::AnnotationsDelete { meta, .. } => {
            Ok(serde_json::json!({ "meta": value(&meta)? }))
        }
        ChangeItem::AnnotationsReorder { order, meta, .. } => {
            Ok(serde_json::json!({ "order": value(&order)?, "meta": value(&meta)? }))
        }
        // Only an undo answers a restore; a single verb never does.
        ChangeItem::AnnotationsRestore { annotations, meta, .. } => {
            Ok(serde_json::json!({ "annotations": annotations, "meta": value(&meta)? }))
        }
        // Without its `skipped` too, as `handleFormsReset` strips it.
        ChangeItem::FormsSetValue { field, widgets, meta }
        | ChangeItem::FormsSetDisplay { field, widgets, meta }
        | ChangeItem::FormsSetAppearanceText { field, widgets, meta }
        | ChangeItem::FormsSetSignatureAppearance { field, widgets, meta } => {
            Ok(serde_json::json!({ "field": field, "widgets": widgets, "meta": value(&meta)? }))
        }
        ChangeItem::FormsReset { fields, widgets, meta, .. } => {
            Ok(serde_json::json!({ "fields": fields, "widgets": widgets, "meta": value(&meta)? }))
        }
        // The rest of the form's ops: the item without its `type` and
        // `skipped`.
        item @ (ChangeItem::FormsCreate { .. }
        | ChangeItem::FormsUpdate { .. }
        | ChangeItem::FormsDelete { .. }
        | ChangeItem::FormsRestore { .. }
        | ChangeItem::FormsAddWidget { .. }
        | ChangeItem::FormsRemoveWidget { .. }
        | ChangeItem::FormsDeleteWidget { .. }
        | ChangeItem::FormsRestoreWidget { .. }
        | ChangeItem::FormsReorderWidgets { .. }
        | ChangeItem::FormsReorderCalculations { .. }
        | ChangeItem::FormsUpdateWidget { .. }
        | ChangeItem::AnnotationsImport { .. }
        | ChangeItem::FormsImport { .. }
        | ChangeItem::FormsImportValues { .. }) => {
            let mut result = value(&item)?;
            if let Some(fields) = result.as_object_mut() {
                fields.remove("type");
                fields.remove("skipped");
            }
            Ok(result)
        }
        ChangeItem::Skipped { .. } => {
            Err(EngineError::with_message(EngineErrorCode::Unknown, "a write wrote nothing"))
        }
        // The page ops have no single verb (119685ea4): only
        // `document.apply` runs them, and it answers the items whole.
        ChangeItem::PagesRotate { .. }
        | ChangeItem::PagesReorder { .. }
        | ChangeItem::PagesDelete { .. }
        | ChangeItem::PagesInsertBlank { .. }
        | ChangeItem::PagesInsert { .. }
        | ChangeItem::PagesRestore { .. } => {
            Err(EngineError::with_message(EngineErrorCode::Unknown, "a page op has no single verb"))
        }
        // Nor do the outline ops (67ccb76dc): `doc.outline`'s verbs are
        // one-op changes through `document.apply` (`outlineVerbs`).
        ChangeItem::OutlineCreate { .. }
        | ChangeItem::OutlineUpdate { .. }
        | ChangeItem::OutlineMove { .. }
        | ChangeItem::OutlineDelete { .. }
        | ChangeItem::OutlineRestore { .. } => Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "an outline op has no single verb",
        )),
    }
}

/// What a job does to the parsed pages the host keeps (`pageEffectOf`,
/// from each kind's `RequestEffect`): a `contentWrite` changes content or
/// the page tree in place, so its document's pages close before and after
/// it; a `runtimeWrite` changes the fonts text is set in, for every
/// document, so every page closes. Any other job keeps them: a `write`
/// changes what a page reads afresh at every render, and a page a write in a
/// layer transaction did change closes when it is next taken
/// (`EPDFPage_IsContentCurrent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageEffect {
    None,
    Document,
    Runtime,
}

impl PageEffect {
    fn of(kind: &str) -> Self {
        match kind {
            "annotations.flatten" | "pages.flatten" | "redaction.apply" | "signatures.complete" => {
                Self::Document
            }
            "fonts.register"
            | "fonts.addFallback"
            | "fonts.clearFallbacks"
            | "fonts.clear"
            | "fonts.authorizeEditing" => Self::Runtime,
            _ => Self::None,
        }
    }
}

/// The kinds that run as one layer transaction (`writesDocument`): every
/// write to an open document but `document.applyChanges`, whose changes
/// each run in their own, and `signatures.complete`, which replaces the
/// document's bytes instead.
const TRANSACTED_KINDS: &[&str] = &[
    "metadata.update",
    "metadata.updateCustom",
    "document.apply",
    "annotations.create",
    "annotations.update",
    "annotations.delete",
    "annotations.reorder",
    "annotations.flatten",
    "annotations.import",
    "forms.setValue",
    "forms.reset",
    "forms.import",
    "forms.importValues",
    "forms.repair",
    "forms.createField",
    "forms.updateField",
    "forms.setSignatureAppearance",
    "forms.deleteField",
    "forms.addWidget",
    "forms.detachWidget",
    "forms.updateWidget",
    "forms.deleteWidget",
    "forms.reorderCalculations",
    "forms.reorderWidgets",
    "pages.setName",
    "pages.removeName",
    "pages.flatten",
    "redaction.apply",
    "measure.setScale",
    "pieceInfo.update",
    "pieceInfo.delete",
];

/// What the three open kinds ask of the session beside the document
/// (`OpenFatMemoryWorkerRequest` and the layer opens).
#[derive(Debug, Clone, Copy, Default)]
struct OpenOptions {
    /// `objectNumbers`, default `'session'`.
    object_numbers: crate::session::document::ObjectNumberAuthority,
    /// `signedDocumentPolicy`, default `protect`.
    signed_document_policy: crate::features::signature::types::SignedDocumentPolicy,
    /// `reserveObjectNumbers`; `0` and absent ask for none.
    reserve_object_numbers: Option<u64>,
}

impl OpenOptions {
    fn parse(body: &serde_json::Value) -> Result<Self, EngineError> {
        let reserve = match body.get("reserveObjectNumbers") {
            None | Some(serde_json::Value::Null) => None,
            Some(n) if n.as_f64() == Some(0.0) => None,
            Some(_) => Some(object_number_count(body, "reserveObjectNumbers")?),
        };
        Ok(Self {
            object_numbers: optional(body, "objectNumbers")?.unwrap_or_default(),
            signed_document_policy: optional(body, "signedDocumentPolicy")?.unwrap_or_default(),
            reserve_object_numbers: reserve,
        })
    }
}

/// A count of object numbers to hand out. Anything but a whole number of
/// at least 1 is `InvalidArg` on `count`, as `DocumentSession.
/// reserveObjectNumbers` refuses it (`Number.isInteger(count) && count >=
/// 1`). A count too large for any document passes here and is refused as
/// `LayerFull` against the session's last object number.
fn object_number_count(body: &serde_json::Value, name: &str) -> Result<u64, EngineError> {
    let raw = body.get(name).and_then(serde_json::Value::as_f64);
    match raw {
        Some(n) if n.fract() == 0.0 && n >= 1.0 => {
            Ok(if n >= u64::MAX as f64 { u64::MAX } else { n as u64 })
        }
        _ => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "cannot reserve {} object numbers",
                body.get(name).unwrap_or(&serde_json::Value::Null)
            ),
        )
        .with_details(serde_json::json!({ "field": "count" }))),
    }
}

/// `LayerOpenSource`, its bytes from payload `index`.
fn layer_source(
    body: &serde_json::Value,
    payloads: &Payloads,
    index: usize,
) -> Result<crate::session::document::LayerOpenSource, EngineError> {
    use crate::session::document::LayerOpenSource;

    #[derive(Deserialize)]
    #[serde(tag = "kind", rename_all = "kebab-case")]
    enum Wire {
        Fresh,
        RawDelta,
        Artifact,
        ArtifactFile { path: String },
    }
    Ok(match field::<Wire>(body, "layer")? {
        Wire::Fresh => LayerOpenSource::Fresh,
        Wire::RawDelta => {
            LayerOpenSource::RawDelta(payload(payloads, index, "the layer's delta")?.to_vec())
        }
        Wire::Artifact => {
            LayerOpenSource::Artifact(payload(payloads, index, "the layer artifact")?.to_vec())
        }
        Wire::ArtifactFile { path } => LayerOpenSource::ArtifactFile(path.into()),
    })
}

/// The identity an `open.fatMem` may carry, spelled as the TypeScript's
/// `IdentityClaims` (`auth/scope/types.ts`), snake_case keys and all.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireIdentity {
    user_id: Option<String>,
    group_id: Option<String>,
    groups: Vec<String>,
    display_name: Option<String>,
}

impl From<WireIdentity> for IdentityClaims {
    fn from(w: WireIdentity) -> Self {
        IdentityClaims {
            user_id: w.user_id,
            group_id: w.group_id,
            groups: w.groups,
            display_name: w.display_name,
        }
    }
}

/// The page an operation names: `page: PageRef`, resolved to the object
/// number the session keys its records on (`DocumentSession.resolvePageRef`).
///
/// Absent is a malformed request, like any other missing argument; present
/// but not a `PageRef` is `InvalidArg`. An unknown page is neither: it is
/// `NotFound` where the page is opened.
fn page_field(body: &serde_json::Value) -> Result<crate::identity::PageObjectNumber, EngineError> {
    let value = body.get("page").ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::WireFormat, "the request has no 'page'")
    })?;
    Ok(crate::identity::PageRef::from_value(value, "page")?.object_number())
}

/// The pages an operation names: `pages: PageRef[]`, in the caller's order.
fn pages_field(
    body: &serde_json::Value,
) -> Result<Vec<crate::identity::PageObjectNumber>, EngineError> {
    let value = body.get("pages").ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::WireFormat, "the request has no 'pages'")
    })?;
    let list = value.as_array().ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::InvalidArg, "'pages' is not a PageRef[]")
    })?;
    list.iter()
        .map(|page| Ok(crate::identity::PageRef::from_value(page, "pages")?.object_number()))
        .collect()
}

/// The name-tree key an embedded-file reference names.
///
/// `EmbeddedFileRef` is a tagged union with exactly one arm today, and it is
/// decoded as one rather than reached into: a second arm would then be a
/// compile error here instead of a key silently read as empty.
fn embedded_file_key(body: &serde_json::Value) -> Result<String, EngineError> {
    let reference: crate::features::files::attachments::AttachmentRef = field(body, "ref")?;
    Ok(reference.as_key().to_owned())
}

/// Which `/PieceInfo` holder a request means.
///
/// A `page` selects the page holder and its absence selects the catalog,
/// which is the same distinction the TypeScript draws by having two services
/// rather than a flag.
fn piece_info_holder(body: &serde_json::Value) -> Result<pdfium::PieceInfoHolder, EngineError> {
    match body.get("page") {
        None | Some(serde_json::Value::Null) => Ok(pdfium::PieceInfoHolder::Catalog),
        Some(_) => Ok(pdfium::PieceInfoHolder::Page(page_field(body)?.get())),
    }
}

/// Framing for a host that has one buffer rather than a list of them.
///
/// The uniffi and napi bindings hand `Vec<Vec<u8>>` across directly, because
/// their generated glue already knows how to carry a list of byte arrays.
/// The C ABI does not: it has a pointer and a length, so the payloads have
/// to be laid out in one allocation and taken apart on the other side.
///
/// The layout is little-endian `u32` throughout, which is what WASM's linear
/// memory is and what a `DataView` on the other side reads without a flag:
///
/// ```text
/// [u32 json_len][json bytes][u32 count]([u32 len][bytes])*count
/// ```
///
/// Lengths rather than offsets, so a reader walks forward once and never
/// computes a position that could point outside the buffer. A `u32` caps one
/// payload at four gigabytes, which is also the cap on the whole address
/// space this framing is used in.
pub mod frame {
    use super::{HostResponse, Payloads};

    /// Pack a response into one buffer.
    pub fn pack(response: &HostResponse) -> Vec<u8> {
        let json = response.json.as_bytes();
        let size =
            4 + json.len() + 4 + response.payloads.iter().map(|p| 4 + p.len()).sum::<usize>();
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(json);
        out.extend_from_slice(&(response.payloads.len() as u32).to_le_bytes());
        for payload in &response.payloads {
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(payload);
        }
        out
    }

    /// Take a request's payloads out of one buffer.
    ///
    /// `None` for anything that does not parse: a truncated length, a count
    /// that overruns, a trailing byte. A host that framed its own request
    /// wrong is a bug on its side, and guessing at a partial read would hand
    /// an operation bytes that are not the ones it was sent.
    pub fn unpack_payloads(bytes: &[u8]) -> Option<Payloads> {
        let mut at = 0usize;
        let count = read_u32(bytes, &mut at)? as usize;
        let mut payloads = Vec::with_capacity(count.min(64));
        for _ in 0..count {
            let len = read_u32(bytes, &mut at)? as usize;
            let end = at.checked_add(len)?;
            payloads.push(bytes.get(at..end)?.to_vec());
            at = end;
        }
        (at == bytes.len()).then_some(payloads)
    }

    fn read_u32(bytes: &[u8], at: &mut usize) -> Option<u32> {
        let end = at.checked_add(4)?;
        let value = u32::from_le_bytes(bytes.get(*at..end)?.try_into().ok()?);
        *at = end;
        Some(value)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn response(json: &str, payloads: Vec<Vec<u8>>) -> HostResponse {
            HostResponse { json: json.to_owned(), payloads }
        }

        /// Reading a packed response back, the way a host does.
        fn unpack(bytes: &[u8]) -> Option<(String, Payloads)> {
            let mut at = 0usize;
            let json_len = read_u32(bytes, &mut at)? as usize;
            let end = at.checked_add(json_len)?;
            let json = String::from_utf8(bytes.get(at..end)?.to_vec()).ok()?;
            Some((json, unpack_payloads(bytes.get(end..)?)?))
        }

        #[test]
        fn a_response_survives_the_round_trip() {
            let original = response(r#"{"result":{"size":3}}"#, vec![vec![1, 2, 3], vec![9]]);
            let (json, payloads) = unpack(&pack(&original)).expect("it unpacks");
            assert_eq!(json, original.json);
            assert_eq!(payloads, original.payloads);
        }

        #[test]
        fn a_response_with_no_payloads_still_carries_the_count() {
            // Zero is written rather than omitted. A reader that had to infer
            // "no count field" from a buffer ending early could not tell that
            // from a truncated one.
            let packed = pack(&response("{}", Vec::new()));
            assert_eq!(packed.len(), 4 + 2 + 4);
            let (json, payloads) = unpack(&packed).expect("it unpacks");
            assert_eq!(json, "{}");
            assert!(payloads.is_empty());
        }

        #[test]
        fn an_empty_payload_is_kept_rather_than_dropped() {
            // A zero-length payload is a real answer: an attachment can hold
            // no bytes, and collapsing it to "no payload" would shift every
            // payload after it by one position.
            let original = response("{}", vec![Vec::new(), vec![7]]);
            let (_, payloads) = unpack(&pack(&original)).expect("it unpacks");
            assert_eq!(payloads, original.payloads);
        }

        #[test]
        fn a_truncated_buffer_is_refused_rather_than_partly_read() {
            let packed = pack(&response("{}", vec![vec![1, 2, 3, 4]]));
            for cut in 1..packed.len() {
                assert!(unpack(&packed[..cut]).is_none(), "a buffer cut at {cut} was accepted",);
            }
        }

        #[test]
        fn trailing_bytes_are_refused() {
            // A count that does not account for the whole buffer means the
            // two sides disagree about the framing, and reading the part
            // they agree on would hide that.
            let mut packed = pack(&response("{}", vec![vec![1]]));
            packed.push(0);
            assert!(unpack(&packed).is_none());
        }

        #[test]
        fn a_count_that_overruns_is_refused() {
            // The shape a malicious or corrupted frame takes: a huge count
            // with nothing behind it. It must not allocate against the
            // count, which is why the reader reserves a bounded amount and
            // grows as it actually reads.
            let mut bytes = u32::MAX.to_le_bytes().to_vec();
            bytes.extend_from_slice(&4u32.to_le_bytes());
            assert!(unpack_payloads(&bytes).is_none());
        }
    }
}
