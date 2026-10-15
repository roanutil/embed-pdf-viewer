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
        }
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
        match self.route(request, payloads) {
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

        let result = match header.kind.as_str() {
            "open.fatMem" => self.open_fat_memory(&header, &body, payloads)?,
            "open.layerMemBase" | "open.layerFileBase" => {
                self.open_layer(&header, &body, payloads)?
            }
            "close" => self.close_document(&header)?,
            "layer.close" => self.close_layer(&header)?,
            "shutdown" => {
                self.sessions.shutdown();
                serde_json::Value::Null
            }
            // A cancellation, not an operation. It names the job to stop
            // rather than a document, and it answers whether there was one
            // to stop: a host that aborted a job which had already finished
            // must not report it as cancelled.
            "abort" => {
                let job_id = string_field(&body, "jobId")?;
                serde_json::json!({ "aborted": self.sessions.abort(&job_id) })
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
                let key = string_field(&body, "fontKey")?;
                let fonts = self.fonts()?;
                let description = pdfium::FontDescription {
                    family_name: string_field(&body, "familyName")?,
                    weight: field(&body, "weight")?,
                    italic: crate::features::fonts::italic_from_wire(field(&body, "italic")?),
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
                let key = string_field(&body, "fontKey")?;
                self.fonts()?.add_fallback(&key)?;
                serde_json::Value::Null
            }
            "fonts.clearFallbacks" => {
                self.fonts()?.clear_fallbacks();
                serde_json::Value::Null
            }
            // `WorkerHost.handleFontsAuthorizeEditing`.
            "fonts.authorizeEditing" => {
                let key = string_field(&body, "fontKey")?;
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
                let path = string_field(&body, "path")?;
                let password: Option<String> = optional(&body, "password")?;
                let probe = self.sessions.probe_security_file(
                    std::path::Path::new(&path),
                    password.as_deref(),
                    self.clock.as_ref(),
                )?;
                value(&probe)?
            }
            "document.renderPageFile" | "document.renderPageFileEncoded" => {
                let path = string_field(&body, "path")?;
                let password: Option<String> = optional(&body, "password")?;
                let page_index: u32 = field(&body, "pageIndex")?;
                let options = render_options(&body)?;
                let rendered = crate::features::server::render_page_file(
                    &pdfium::Library::acquire(),
                    std::path::Path::new(&path),
                    password.as_deref(),
                    page_index,
                    &options,
                    &cancel,
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
                    let encode: crate::features::server::RenderEncode = field(&body, "encode")?;
                    let encoded = crate::features::server::encode_raster(
                        self.encoder.as_deref(),
                        &rendered.raster,
                        &encode,
                        &cancel,
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
            _ => return self.route_session(&header, &body, payloads, &cancel),
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
        if MUTATING_KINDS.contains(&header.kind.as_str())
            && let Ok(session) = self.sessions.session(&key)
        {
            f::signature::mutator::assert_no_pending_signing(session)?;
        }
        match header.kind.as_str() {
            // ── Document reads ────────────────────────────────────────
            "metadata.read" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::metadata::read_metadata(d, cancel)?)?))
            }
            "pages.list" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::pages::list_pages(d, cancel)?)?))
            }
            "actions.read" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::actions::document::read_document_actions(d, cancel)?)?))
            }
            "forms.list" => {
                let d = self.document(&key)?;
                let model = d.form_model().ok_or_else(form_model_missing)?;
                Ok(ok(value(&f::forms::snapshot::read_form_snapshot(&model, cancel)?)?))
            }
            "forms.export" => {
                let format: f::forms::write::FormDataFormat = field(body, "format")?;
                let d = self.document(&key)?;
                let bytes = d.export_form_data(format.native()).map_err(|e| {
                    EngineError::with_message(
                        EngineErrorCode::Unknown,
                        format!("exporting the form data failed: {e}"),
                    )
                })?;
                // The format goes back beside the bytes. A caller that asked
                // for FDF and got XFDF would write the wrong extension, and
                // nothing in the bytes themselves says which it is.
                Ok(HostResponse {
                    json: envelope(serde_json::json!({
                        "format": format,
                        "size": bytes.len(),
                    })),
                    payloads: vec![bytes],
                })
            }
            "attachments.list" => {
                let d = self.document(&key)?;
                Ok(ok(value(&f::files::attachments::list_attachments(d, cancel)?)?))
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
                if working_copy {
                    let session = self.sessions.session(&key)?;
                    let snapshot = f::signature::analyzer::read_working_copy_snapshot(session)?;
                    return Ok(ok(value(&snapshot)?));
                }
                let d = self.document(&key)?;
                Ok(ok(value(&f::signature::reader::read_snapshot(d)?)?))
            }
            // ── Signatures (writes) ──────────────────────────────────
            "signatures.prepare" => {
                let input: f::signature::mutator::SignaturePrepareInput = field(body, "input")?;
                let session = self.sessions.session(&key)?;
                session.require(DocCapability::DocSign)?;
                if input.certify.is_some() {
                    session.require(DocCapability::DocSignCertify)?;
                }
                let appearance = input.appearance.and(payloads.first().map(Vec::as_slice));
                let signing_id = self.ids.next_uuid_v4();
                let (prepared, digest) =
                    f::signature::mutator::prepare(session, &input, appearance, signing_id)?;
                let prepared = value(&prepared)?;
                // `LocalDocumentSignaturesService.prepare` publishes the
                // parked candidate: its id and the field the call named.
                let notice = crate::session::save::DocumentEventInit::SignaturePrepared {
                    signing_id: prepared["signingId"].as_str().unwrap_or_default().to_owned(),
                    field: value(&input.field)?,
                };
                self.answer_with_notice(&key, prepared, notice, vec![digest])
            }
            "signatures.complete" => {
                let input: f::signature::mutator::SignatureCompleteInput = field(body, "input")?;
                let cms = payload(&payloads, 0, "the CMS")?.to_vec();
                let session = self.sessions.session_mut(&key)?;
                session.require(DocCapability::DocSign)?;
                let result = f::signature::mutator::complete(session, &input, &cms)?;
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
            "signatures.abort" => {
                let signing_id = string_field(body, "signingId")?;
                let session = self.sessions.session(&key)?;
                session.require(DocCapability::DocSign)?;
                let result = value(&f::signature::mutator::abort(session, &signing_id))?;
                // `LocalDocumentSignaturesService.abort` publishes only an
                // abort that happened, not one that found nothing parked.
                if result["status"] != "aborted" {
                    return Ok(ok(result));
                }
                let notice =
                    crate::session::save::DocumentEventInit::SignatureAborted { signing_id };
                self.answer_with_notice(&key, result, notice, Vec::new())
            }
            "forms.setSignatureAppearance" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let page_index: i32 = optional(body, "pageIndex")?.unwrap_or(0);
                let pdf = payload(&payloads, 0, "the appearance PDF")?.to_vec();
                let session = self.sessions.session_mut(&key)?;
                session.require(DocCapability::DocFormsFill)?;
                let result = f::signature::mutator::set_signature_appearance(
                    session, &reference, &pdf, page_index, cancel,
                )?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormFieldUpdated(value(&result)?),
                )
            }
            "signatures.analyze" => {
                self.sessions.session(&key)?.require(DocCapability::DocFormsRead)?;
                let input: f::signature::analysis::types::AnalyzeInput = field(body, "input")?;
                let session = self.sessions.session(&key)?;
                Ok(ok(value(&f::signature::analyzer::analyze(session, &input)?)?))
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
            "document.version" => {
                let session = self.sessions.session(&key)?;
                Ok(ok(value(&f::signature::reader::version(session)?)?))
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
            "measure.viewports" => {
                self.on_page(&key, body, |page| f::measure::read_viewports(page.view(), cancel))
            }
            "pages.render" | "pages.renderEncoded" => {
                let options = render_options(body)?;
                let raster = self.on_page_raw(&key, body, |page| {
                    f::render::raster::render_page(page, &options, cancel)
                })?;
                if header.kind == "pages.render" {
                    return Ok(raster_response(raster));
                }
                let encode: f::server::RenderEncode = field(body, "encode")?;
                let encoded =
                    f::server::encode_raster(self.encoder.as_deref(), &raster, &encode, cancel)?;
                Ok(encoded_response(encoded))
            }
            "annotations.renderAppearances" | "annotations.renderAppearancesEncoded" => {
                let options: f::annotation::appearance::AppearanceRenderOptions =
                    optional(body, "options")?.unwrap_or_default();
                let page = page_field(body)?;
                let revision = self.sessions.session_mut(&key)?.page_state(page)?.revision;
                let owned = self.sessions.session(&key)?.document()?;
                let opened = owned.page(page)?;
                let result = f::annotation::appearance::render_page_appearances(
                    &opened, page, &revision, &options, cancel,
                )?;
                // The index rides in the JSON and each appearance's pixels
                // in their own payload, in the same order. A page of stamps
                // is tens of megabytes of RGBA, and putting that through the
                // JSON would be the single most expensive thing this
                // dispatcher does.
                let encode: Option<f::server::RenderEncode> = optional(body, "encode")?;
                let mut index = Vec::with_capacity(result.appearances.len());
                let mut payloads = Vec::with_capacity(result.appearances.len());
                for appearance in result.appearances {
                    let mut entry = serde_json::json!({
                        "ref": value(&appearance.annotation_ref)?,
                        "mode": value(&appearance.mode)?,
                        "rect": value(&appearance.rect)?,
                        "width": appearance.raster.width,
                        "height": appearance.raster.height,
                    });
                    match &encode {
                        None => {
                            entry["stride"] = appearance.raster.stride.into();
                            payloads.push(appearance.raster.rgba);
                        }
                        Some(encode) => {
                            let image = f::server::encode_raster(
                                self.encoder.as_deref(),
                                &appearance.raster,
                                encode,
                                cancel,
                            )?;
                            entry["mimeType"] = image.content_type.clone().into();
                            payloads.push(image.bytes);
                        }
                    }
                    index.push(entry);
                }
                Ok(HostResponse {
                    json: envelope(serde_json::json!({
                        "pageState": value(&result.page_state)?,
                        "appearances": index,
                    })),
                    payloads,
                })
            }
            "annotations.listRawPage" | "annotations.listFullPage" => {
                let page = page_field(body)?;
                let session = self.sessions.session_mut(&key)?;
                let state = session.page_state(page)?;
                let index = page_index_of(session.document()?.document(), page)?;
                let snapshot = f::annotation::read::page::read_page_annotations(
                    session.document()?.document(),
                    index,
                    page,
                    &state.revision,
                    &mut f::actions::vocabulary::ActionReadBudgetTracker::default(),
                    cancel,
                )?;
                Ok(ok(value(&snapshot)?))
            }
            "annotations.listRawAll" => {
                let session = self.sessions.session_mut(&key)?;
                // Every page's token comes from the session's own authority,
                // so a client's weak reference can be checked against the
                // same generation the listing reported.
                let tokens = page_tokens(session)?;
                let snapshot = f::annotation::read::page::read_document_annotations(
                    session.document()?.document(),
                    |page| {
                        tokens
                            .get(&page)
                            .cloned()
                            .unwrap_or_else(|| crate::revision::RevisionToken::new("", page, 0))
                    },
                    cancel,
                )?;
                Ok(ok(value(&snapshot)?))
            }
            "annotations.readFile" => {
                let page = page_field(body)?;
                let annotation_ref: crate::identity::AnnotationRef = field(body, "ref")?;
                let ceiling: Option<u64> = optional(body, "maxDecodedBytes")?;
                let owned = self.sessions.session(&key)?.document()?;
                let opened = owned.page(page)?;
                let found = f::annotation::write::resolve::resolve_annotation(
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
            "pages.extract" => {
                let pages = pages_field(body)?;
                let session = self.sessions.session_mut(&key)?;
                let bytes = f::extract::extract_pages(session, &pages, cancel)?;
                Ok(bytes_response(bytes))
            }
            "document.saveBuffer" => {
                let mode: f::save::SaveMode = field(body, "mode")?;
                let session = self.sessions.session(&key)?;
                Ok(bytes_response(f::save::save_to_buffer(session, mode, cancel)?))
            }
            "document.saveLayerBuffer" => {
                // No argument. A layer session has exactly one artifact to
                // hand back, and the TypeScript's request carries nothing
                // beyond the envelope; naming a kind here would invent a
                // choice the wire does not offer.
                let session = self.sessions.session(&key)?;
                let bytes = f::save::save_layer_to_buffer(
                    session,
                    f::save::LayerArtifactKind::Delta,
                    cancel,
                )?;
                Ok(bytes_response(bytes))
            }
            "document.saveFile" => {
                let mode: f::save::SaveMode = field(body, "mode")?;
                let path = string_field(body, "path")?;
                let session = self.sessions.session(&key)?;
                let bytes = f::save::save_to_buffer(session, mode, cancel)?;
                let size = bytes.len();
                std::fs::write(&path, bytes).map_err(|e| {
                    EngineError::with_message(
                        EngineErrorCode::Unknown,
                        format!("writing '{path}' failed: {e}"),
                    )
                })?;
                Ok(ok(serde_json::json!({ "size": size })))
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
                // The protection beside the probe, as the TypeScript's
                // answer carries it, so a host that just unlocked knows
                // what the signatures forbid.
                let mut answer = value(&probe)?;
                answer["protection"] = self.protection_of(&key);
                Ok(ok(answer))
            }

            // ── Mutations ─────────────────────────────────────────────
            "metadata.update" => {
                self.sessions.session(&key)?.require(DocCapability::DocMetadataModify)?;
                let patch: crate::dto::MetadataPatch = field(body, "patch")?;
                let result = {
                    let d = self.document(&key)?;
                    f::metadata_write::update_metadata(d, &patch, cancel)?
                };
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::MetadataUpdated(with_local_cache(
                        value(&result)?,
                    )),
                )
            }
            "annotations.create" => {
                let page = page_field(body)?;
                let draft: f::annotation::write::draft::AnnotationDraft = field(body, "draft")?;
                let actor: f::annotation::write::draft::AnnotationActor =
                    optional(body, "actor")?.unwrap_or_default();
                let owned_payloads = resource_map(body, &payloads)?;
                let resources = borrowed_resources(&owned_payloads);
                let context = annotation_context(self.fonts.as_ref(), &resources);
                let result = f::annotation::write::mutator::create(
                    self.sessions.session_mut(&key)?,
                    page,
                    &draft,
                    &actor,
                    &context,
                    self.clock.as_ref(),
                    cancel,
                )?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::AnnotationCreated {
                        page_object_number: page,
                        result: value(&result)?,
                    },
                )
            }
            "annotations.update" => {
                let annotation_ref: crate::identity::AnnotationRef = field(body, "ref")?;
                let patch: f::annotation::write::patch::AnnotationPatch = field(body, "patch")?;
                let owned_payloads = resource_map(body, &payloads)?;
                let resources = borrowed_resources(&owned_payloads);
                let context = annotation_context(self.fonts.as_ref(), &resources);
                let result = f::annotation::write::mutator::update(
                    self.sessions.session_mut(&key)?,
                    &annotation_ref,
                    &patch,
                    &context,
                    self.clock.as_ref(),
                    self.ids.as_ref(),
                    cancel,
                )?;
                // `LocalPageAnnotationsService` publishes the page it was
                // asked through, which is the page the ref names.
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::AnnotationUpdated {
                        page_object_number: annotation_ref.page(),
                        result: value(&result)?,
                    },
                )
            }
            "annotations.delete" => {
                let annotation_ref: crate::identity::AnnotationRef = field(body, "ref")?;
                let result = f::annotation::write::mutator::delete(
                    self.sessions.session_mut(&key)?,
                    &annotation_ref,
                    cancel,
                )?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::AnnotationDeleted {
                        page_object_number: annotation_ref.page(),
                        result: value(&result)?,
                    },
                )
            }
            "annotations.move" => {
                let page = page_field(body)?;
                let refs: Vec<crate::identity::AnnotationRef> = field(body, "refs")?;
                let to_index: u32 = field(body, "toIndex")?;
                let result = f::annotation::write::mutator::move_to(
                    self.sessions.session_mut(&key)?,
                    page,
                    &refs,
                    to_index,
                    self.ids.as_ref(),
                    cancel,
                )?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::AnnotationMoved {
                        page_object_number: page,
                        result: value(&result)?,
                    },
                )
            }

            // `WorkerHost.handleAnnotationsFlatten`. Nothing applied is no
            // mutation: no event, no artifact, as the TypeScript answers it.
            "annotations.flatten" => {
                let page = page_field(body)?;
                let refs: Vec<crate::identity::AnnotationRef> = field(body, "refs")?;
                let usage: f::pages_write::FlattenUsage = field(body, "usage")?;
                let result = f::annotation::write::mutator::flatten(
                    self.sessions.session_mut(&key)?,
                    page,
                    &refs,
                    usage,
                    cancel,
                )?;
                if result.meta.is_none() {
                    let session = self.sessions.session(&key)?;
                    return Ok(ok(serde_json::json!({
                        "result": value(&result)?,
                        "mutationSequence": session.mutation_sequence(),
                    })));
                }
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::AnnotationsFlattened(value(&result)?),
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

            "forms.setValue" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let form_value: f::forms::write::FormFieldValue = field(body, "value")?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::write::set_value(d, &reference, &form_value, cancel)?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormValueChanged(with_empty_meta(
                        value(&result)?,
                    )),
                )
            }
            "forms.reset" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::write::reset(d, &reference, cancel)?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormValueChanged(with_empty_meta(
                        value(&result)?,
                    )),
                )
            }
            "forms.applyEffects" => {
                let effects: Vec<f::forms::effects::FormEffect> = field(body, "effects")?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::effects::apply(d, &effects, cancel)?
                };
                // A batch that wrote nothing publishes nothing. `meta` is
                // the applier's own answer to "did anything land", and
                // finalizing an all-no-op run would bump the version and
                // invalidate every cache for a script that changed nothing.
                if result.meta.is_none() {
                    return Ok(ok(value(&result)?));
                }
                // `FormsEffectsApplier` notes the mutation once per effect
                // that applied or failed, before re-reading its fields.
                for _ in result.results.iter().filter(|r| {
                    matches!(
                        r.status,
                        f::forms::effects::FormEffectStatus::Applied
                            | f::forms::effects::FormEffectStatus::Failed
                    )
                }) {
                    self.note_form_write(&key)?;
                }
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormEffectsApplied(value(&result)?),
                )
            }
            "forms.import" => {
                // The data is a PAYLOAD, never a JSON field. An FDF is a file,
                // and base64 through a string would double its size on a path
                // that already exists for exactly this.
                let data = payload(&payloads, 0, "the form data")?;
                let format: Option<f::forms::write::FormDataFormat> = optional(body, "format")?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::write::import_data(d, data, format.map(|f| f.native()), cancel)?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormImported(with_empty_meta(value(
                        &result,
                    )?)),
                )
            }
            "forms.repair" => {
                let bake: bool = optional(body, "bakeAppearances")?.unwrap_or(false);
                let result = {
                    let d = self.document(&key)?;
                    f::forms::write::repair(d, bake, cancel)?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormRepaired(with_empty_meta(value(
                        &result,
                    )?)),
                )
            }
            "forms.createField" => {
                let draft: f::forms::field::FormFieldDraft = field(body, "draft")?;
                let result = {
                    let owned = self.sessions.session(&key)?.document()?;
                    let source = f::forms::field::OwnedPageWidgets::new(owned);
                    f::forms::field::create_field(owned.document(), &source, &draft, cancel)?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormFieldCreated(field_result(
                        &result,
                    )?),
                )
            }
            "forms.updateField" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let patch: f::forms::field::FormFieldPatch = field(body, "patch")?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::field::update_field(d, &reference, &patch, cancel)?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormFieldUpdated(field_result(
                        &result,
                    )?),
                )
            }
            "forms.deleteField" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::field::delete_field(d, &reference, cancel)?
                };
                self.note_form_write(&key)?;
                // `WorkerHost.handleFormsDeleteField`: the field let its
                // widgets go, and the annotation plane deletes them, so the
                // page's `/Annots` and revision move in one place. A widget
                // with no ref (direct, or on no page) has nothing to delete.
                for widget in &result.detached_widgets {
                    let (Ok(page), true) = (
                        crate::identity::PageObjectNumber::new(widget.page_object_number),
                        widget.annot_object_number > 0,
                    ) else {
                        continue;
                    };
                    f::annotation::write::mutator::delete_unnoted(
                        self.sessions.session_mut(&key)?,
                        &crate::identity::AnnotationRef::ObjectNumber {
                            page,
                            annot: widget.annot_object_number,
                        },
                        cancel,
                    )?;
                }
                // The deletes above wrote `/Annots` after the field's own
                // note, so note again, as the TypeScript does.
                self.note_form_write(&key)?;
                // `FormFieldDeleteResult`: the widgets are `removedWidgets`,
                // which the render plugin reads to repaint their pages.
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormFieldDeleted(serde_json::json!({
                        "deletedFieldObjectNumber": result.deleted_field_object_number,
                        "removedWidgets": value(&result.detached_widgets)?,
                        "meta": empty_meta(),
                    })),
                )
            }
            "forms.attachWidget" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let widget = widget_field(body)?;
                let on_state: Option<String> = optional(body, "onState")?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::field::attach_widget(
                        d,
                        &reference,
                        widget,
                        on_state.as_deref(),
                        cancel,
                    )?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormWidgetAttached(field_result(
                        &result,
                    )?),
                )
            }
            "forms.detachWidget" => {
                let reference: f::forms::snapshot::FormFieldRef = field(body, "ref")?;
                let widget = widget_field(body)?;
                let result = {
                    let d = self.document(&key)?;
                    f::forms::field::detach_widget(d, &reference, widget, cancel)?
                };
                self.note_form_write(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::FormWidgetDetached(field_result(
                        &result,
                    )?),
                )
            }

            "pages.move" => {
                let pages = pages_field(body)?;
                let destination: u32 = field(body, "destIndex")?;
                let result = {
                    let d = self.document(&key)?;
                    f::pages_write::move_pages(d, &pages, destination, cancel)?
                };
                self.refresh_pages(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesMoved {
                        page_object_numbers: pages,
                        dest_index: destination,
                        result: with_local_cache(value(&result)?),
                    },
                )
            }
            "pages.setName" => {
                let name: String = field(body, "name")?;
                let page = page_field(body)?;
                let replace: Option<String> = optional(body, "replace")?;
                let result = {
                    let d = self.document(&key)?;
                    f::pages_write::set_name(d, &name, page, replace.as_deref(), cancel)?
                };
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesNamed {
                        name,
                        page_object_number: Some(page),
                        result: with_local_cache(value(&result)?),
                    },
                )
            }
            "pages.removeName" => {
                let name: String = field(body, "name")?;
                let result = {
                    let d = self.document(&key)?;
                    f::pages_write::remove_name(d, &name, cancel)?
                };
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesNamed {
                        name,
                        page_object_number: None,
                        result: with_local_cache(value(&result)?),
                    },
                )
            }
            "pages.rotate" => {
                let pages = pages_field(body)?;
                let rotation: crate::geom::PdfRotation = field(body, "rotation")?;
                let result = {
                    let d = self.document(&key)?;
                    f::pages_write::rotate_pages(d, &pages, rotation, cancel)?
                };
                self.refresh_pages(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesRotated {
                        page_object_numbers: pages,
                        rotation,
                        result: with_local_cache(value(&result)?),
                    },
                )
            }
            "pages.delete" => {
                let pages = pages_field(body)?;
                let result = {
                    let d = self.document(&key)?;
                    f::pages_write::delete_pages(d, &pages, cancel)?
                };
                // The deleted pages' per-session state goes with them. A
                // retired object number can never come back, so anything
                // keyed on it is dead weight that would also answer stale.
                for page in &pages {
                    self.sessions.session_mut(&key)?.drop_page_state(*page)?;
                }
                self.refresh_pages(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesDeleted {
                        page_object_numbers: pages,
                        result: with_local_cache(value(&result)?),
                    },
                )
            }
            "pages.flatten" => {
                let pages = pages_field(body)?;
                let usage: f::pages_write::FlattenUsage = field(body, "usage")?;
                let result = {
                    let owned = self.sessions.session(&key)?.document()?;
                    f::pages_write::flatten_pages(
                        owned.document(),
                        |page| owned.page(page),
                        &pages,
                        usage,
                        cancel,
                    )?
                };
                // `PagesFlattener.flatten`: a page that was applied or failed
                // changed (its annotations are content now, or partly so),
                // and only those are re-pinned. Nothing changed is no
                // mutation: `meta: null`, no event, no artifact.
                let changed: Vec<_> = result
                    .results
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.status,
                            f::pages_write::FlattenStatus::Applied
                                | f::pages_write::FlattenStatus::Failed
                        )
                    })
                    .map(|r| r.page_object_number)
                    .collect();
                self.finish_rewrite(&key, value(&result)?, &changed, |result| {
                    crate::session::save::DocumentEventInit::PagesFlattened(result)
                })
            }
            "pages.insertBlank" => {
                let size: crate::geom::PdfSize = field(body, "size")?;
                let count: u32 = optional(body, "count")?.unwrap_or(1);
                let destination: Option<u32> = optional(body, "destIndex")?;
                let result = {
                    let d = self.document(&key)?;
                    f::pages_write::insert_blank_pages(
                        d,
                        size.width,
                        size.height,
                        count,
                        destination,
                        cancel,
                    )?
                };
                self.refresh_pages(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesInserted {
                        dest_index: destination,
                        result: with_local_cache(value(&result)?),
                    },
                )
            }
            "pages.insert" => {
                // The source document arrives as a payload and is opened
                // here, held only for the copy. `import_pages` leaves the
                // imported streams reading through it, so it must outlive
                // the call and not the operation.
                let bytes = payload(&payloads, 0, "the document to insert from")?;
                let destination: Option<u32> = optional(body, "destIndex")?;
                let source = pdfium::Document::from_bytes(pdfium::Library::acquire(), bytes, None)
                    .map_err(|e| {
                        EngineError::with_message(
                            EngineErrorCode::DocOpenFailed,
                            format!("the document to insert from could not be opened: {e}"),
                        )
                    })?;
                let result = {
                    let d = self.document(&key)?;
                    f::pages_write::insert_pages_from(d, &source, destination, cancel)?
                };
                self.refresh_pages(&key)?;
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PagesInserted {
                        dest_index: destination,
                        result: with_local_cache(value(&result)?),
                    },
                )
            }
            "redaction.apply" => {
                let scope: f::redaction::RedactionScope = field(body, "scope")?;
                let result = {
                    let owned = self.sessions.session(&key)?.document()?;
                    f::redaction::apply(owned.document(), |page| owned.page(page), &scope, cancel)?
                };
                // `RedactionApplier.apply`: applying removes the marking and
                // whatever went with it, so a page that was applied or failed
                // is re-pinned. Nothing changed is no mutation.
                let changed: Vec<_> = result
                    .results
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.status,
                            f::redaction::RedactionStatus::Applied
                                | f::redaction::RedactionStatus::Failed
                        )
                    })
                    .map(|r| r.page_object_number)
                    .collect();
                self.finish_rewrite(&key, value(&result)?, &changed, |result| {
                    crate::session::save::DocumentEventInit::RedactionApplied(result)
                })
            }

            "attachments.create" => {
                let file: WireAttachmentFile = field(body, "file")?;
                let bytes = payload(&payloads, 0, "the file to embed")?;
                let result = {
                    let d = self.document(&key)?;
                    f::files::attachments::create_attachment(
                        d,
                        &file.name,
                        bytes,
                        file.mime_type.as_deref(),
                        file.description.as_deref(),
                        cancel,
                    )?
                };
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::AttachmentCreated(with_local_cache(
                        value(&result)?,
                    )),
                )
            }
            "attachments.delete" => {
                let name = embedded_file_key(body)?;
                {
                    let d = self.document(&key)?;
                    f::files::attachments::delete_attachment(d, &name, cancel)?;
                }
                self.finish(
                    &key,
                    // `AttachmentDeleteResult`: the ref the call named.
                    crate::session::save::DocumentEventInit::AttachmentDeleted(serde_json::json!({
                        "deleted": { "kind": "key", "key": name },
                        "cache": null,
                    })),
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
                let measure = Some(measure).filter(|m| !m.is_null());
                {
                    let owned = self.sessions.session(&key)?.document()?;
                    let index = owned.page_index(pon)?;
                    let page = owned.page(pon)?;
                    f::measure::set_scale(owned.document(), index, page.view(), measure, cancel)?;
                }
                // `PageScaleResult`, naming the page as the request did. A
                // calibration changes no page pixels and no annotation
                // indices, so `meta` is empty.
                self.finish(
                    &key,
                    crate::session::save::DocumentEventInit::PageViewportsChanged(
                        serde_json::json!({
                            "page": crate::identity::PageRef::new(pon),
                            "meta": empty_meta(),
                        }),
                    ),
                )
            }
            "pieceInfo.update" | "pieceInfo.clear" => {
                let holder = piece_info_holder(body)?;
                let application = string_field(body, "application")?;
                if header.kind == "pieceInfo.clear" {
                    let d = self.document(&key)?;
                    f::files::piece_info::clear_piece_info(d, holder, &application, cancel)?;
                } else {
                    let patch: f::files::piece_info::PieceInfoPatch = field(body, "patch")?;
                    let d = self.document(&key)?;
                    f::files::piece_info::update_piece_info(
                        d,
                        holder,
                        &application,
                        &patch,
                        self.clock.as_ref(),
                        cancel,
                    )?;
                }
                // `LocalPieceInfoService` publishes nothing: `/PieceInfo` is
                // private application data, and the event this once sent as
                // `metadata.updated` carried no `metadata` for the metadata
                // plugin to read. Nor is there a result: `WorkerHost` answers
                // `{tag}` alone, and both calls resolve `void`.
                self.finish_quietly(&key, serde_json::Value::Null)
            }

            other => {
                let _ = payloads;
                Err(EngineError::with_message(
                    EngineErrorCode::NotImplemented,
                    format!("this build does not route '{other}'"),
                ))
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

    /// `session.noteMutation()` as `FormMutator` and `FormsEffectsApplier`
    /// call it after a write lands: the form writers are `SelfSequenced`,
    /// so the finish does not bump for them.
    fn note_form_write(&self, key: &SessionKey) -> Result<(), EngineError> {
        self.sessions.session(key)?.note_mutation();
        Ok(())
    }

    /// [`Host::finish`] for a mutation the TypeScript publishes no event
    /// for (`pieceInfo.update`, `pieceInfo.clear`): the sequence still
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

        let session = self.sessions.session(key)?;
        let finished = save::finish_mutation(session, family, &CancelToken::never())?;
        let mut answer = serde_json::json!({
            "result": result,
            "mutationSequence": finished.mutation_sequence,
        });
        if let Some(event) = event {
            answer["event"] = serde_json::to_value(self.stamp(key, event)?).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::WireFormat,
                    format!("the event could not be serialized: {e}"),
                )
            })?;
        }
        let json = envelope(answer);
        Ok(HostResponse { json, payloads: finished.artifact.map(|a| vec![a]).unwrap_or_default() })
    }

    /// Finish a content rewrite (`pages.flatten`, `redaction.apply`) whose
    /// result carries `meta`, as `WorkerHost.handlePagesFlatten` answers
    /// it: `null` with no finalization and no event when no page changed,
    /// otherwise the changed pages settled and the event published.
    fn finish_rewrite(
        &mut self,
        key: &SessionKey,
        mut result: serde_json::Value,
        changed: &[crate::identity::PageObjectNumber],
        event: impl FnOnce(serde_json::Value) -> crate::session::save::DocumentEventInit,
    ) -> Result<HostResponse, EngineError> {
        if changed.is_empty() {
            result["meta"] = serde_json::Value::Null;
            let session = self.sessions.session(key)?;
            return Ok(ok(serde_json::json!({
                "result": result,
                "mutationSequence": session.mutation_sequence(),
            })));
        }
        let meta = crate::features::annotation::write::mutator::settle_rewritten_pages(
            self.sessions.session_mut(key)?,
            changed,
        )?;
        result["meta"] = value(&meta)?;
        self.finish(key, event(result))
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
            },
        ))
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

    /// Re-read the page registry after the page list changed.
    ///
    /// Anything that inserts, removes or reorders pages leaves every cached
    /// record addressing the wrong position. Skipping this is not a stale
    /// read but a wrong one: the next operation would resolve a page object
    /// number through an index that now names a different page.
    fn refresh_pages(&mut self, key: &SessionKey) -> Result<(), EngineError> {
        self.sessions.session_mut(key)?.refresh_page_registry()
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
        use crate::access::DocumentAccess;
        let page = page_field(body)?;
        let owned = self.sessions.session(key)?.document()?;
        let opened = owned.page(page)?;
        Ok(ok(read(&opened)?))
    }

    /// The same for a closure whose result is not JSON.
    fn on_page_raw<T>(
        &mut self,
        key: &SessionKey,
        body: &serde_json::Value,
        read: impl FnOnce(&dyn crate::access::PageAccess) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        use crate::access::DocumentAccess;
        let page = page_field(body)?;
        let owned = self.sessions.session(key)?.document()?;
        let opened = owned.page(page)?;
        read(&opened)
    }

    /// Open a layer over a base document.
    ///
    /// Two frames, one path: the base arrives either as bytes or as a path,
    /// and everything after that is identical. The base is registered under
    /// its OWN key rather than the layer's, because several layer sessions
    /// over one base should share the one open document, which is the whole
    /// reason the base registry exists.
    fn open_layer(
        &mut self,
        header: &RequestHeader,
        body: &serde_json::Value,
        payloads: Payloads,
    ) -> Result<serde_json::Value, EngineError> {
        let layer = string_field(body, "layer")?;
        let doc_id = header.doc_id.as_deref().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::WireFormat, "a layer open names no document")
        })?;
        let key = SessionKey::layer(doc_id, &layer);
        let base_key = string_field(body, "baseKey")?;
        let password: Option<String> = optional(body, "password")?;

        if header.kind == "open.layerFileBase" {
            let path = string_field(body, "basePath")?;
            self.sessions.open_layer_file(
                key,
                &base_key,
                std::path::Path::new(&path),
                password.as_deref(),
                SessionAuthority::unrestricted(),
            )?;
        } else {
            // The base bytes are a payload, never a JSON field, for the same
            // reason `open.fatMem`'s are.
            let bytes = payload(&payloads, 0, "the base document")?;
            self.sessions.open_layer_memory(
                key,
                &base_key,
                bytes,
                password.as_deref(),
                SessionAuthority::unrestricted(),
            )?;
        }
        Ok(serde_json::json!({ "layerName": layer, "baseKey": base_key }))
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
        match self.sessions.open_memory(key.clone(), bytes, password, authority) {
            Ok(()) => {}
            // A password is a STATE on the wire, not a failure. The registry
            // reports it as an error because internally it IS one: the
            // document is not readable. To a client it is a prompt, and the
            // session has already parked with its key and its bytes, so an
            // error here would make a caller tear down something that is
            // waiting for a password. This is the shape `WorkerHost`'s own
            // open path produces, deliberately.
            Err(error) if error.is(EngineErrorCode::DocPasswordRequired) => {}
            Err(error) => return Err(error),
        }
        Ok(serde_json::json!({
            "security": value(&self.security_on_open(&key)?)?,
            "protection": self.protection_of(&key),
        }))
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

    fn close_document(&mut self, header: &RequestHeader) -> Result<serde_json::Value, EngineError> {
        let doc_id = header.doc_id.as_deref().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::WireFormat, "a close names no document")
        })?;
        let closed = self.sessions.close_document(doc_id);
        Ok(serde_json::json!({ "closedSessions": closed }))
    }

    fn close_layer(&mut self, header: &RequestHeader) -> Result<serde_json::Value, EngineError> {
        let key = header.key()?;
        Ok(serde_json::json!({ "closed": self.sessions.close_layer(&key) }))
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

/// `EMPTY_FORM_META` (`WorkerHost`) and `EMPTY_META` (`FormMutator`): a
/// write with no page pins, on a local engine with no cache delta.
fn empty_meta() -> serde_json::Value {
    serde_json::json!({ "affectedPages": [], "cacheDelta": null })
}

/// A form write's result as the TypeScript answers it: with `meta`.
fn with_empty_meta(mut result: serde_json::Value) -> serde_json::Value {
    if let Some(object) = result.as_object_mut() {
        object.insert("meta".into(), empty_meta());
    }
    result
}

/// A field edit's result (`FormFieldCreateResult`, `FormFieldUpdateResult`,
/// `FormWidgetLinkResult`): the field read back, and `meta`.
fn field_result(
    field: &crate::features::forms::snapshot::FormFieldDto,
) -> Result<serde_json::Value, EngineError> {
    Ok(serde_json::json!({ "field": value(field)?, "meta": empty_meta() }))
}

/// A page-structure, metadata or attachment result as a local engine
/// answers it: `cache: null`, the cloud's coherence pins that a local engine
/// never has (`PageMoveResult.cache`, `MetadataUpdateResult.cache`, ...).
fn with_local_cache(mut result: serde_json::Value) -> serde_json::Value {
    if let Some(object) = result.as_object_mut() {
        object.entry("cache").or_insert(serde_json::Value::Null);
    }
    result
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

/// What `attachments.create` says about the file it is embedding.
///
/// The bytes are not here: they ride as a payload. A name, a type and a
/// description are metadata a JSON field can carry; a file is not.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireAttachmentFile {
    name: String,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// A response whose result is a raster.
///
/// The pixels leave as a payload rather than as JSON: a page at a readable
/// scale is megabytes, and the transfer list on the other side exists for
/// exactly this.
fn raster_response(raster: crate::features::render::raster::PageRaster) -> HostResponse {
    HostResponse { json: envelope(raster_json(&raster)), payloads: vec![raster.rgba] }
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

/// Render options, defaulted.
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

/// Every kind that writes to a session's document: refused while a signing
/// is parked (`WorkerHost.ts` `MUTATING_KINDS`, the kinds this host routes).
const MUTATING_KINDS: &[&str] = &[
    "metadata.update",
    "annotations.create",
    "annotations.update",
    "annotations.delete",
    "annotations.move",
    "annotations.flatten",
    "forms.setValue",
    "forms.reset",
    "forms.applyEffects",
    "forms.import",
    "forms.repair",
    "forms.createField",
    "forms.updateField",
    "forms.setSignatureAppearance",
    "forms.deleteField",
    "forms.attachWidget",
    "forms.detachWidget",
    "pages.move",
    "pages.rotate",
    "pages.delete",
    "pages.setName",
    "pages.removeName",
    "pages.flatten",
    "pages.insert",
    "pages.insertBlank",
    "redaction.apply",
    "attachments.create",
    "attachments.delete",
    "measure.setScale",
    "pieceInfo.update",
    "pieceInfo.clear",
];

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

/// The widget a field link names: its `AnnotationRef`, which must be by
/// object number (`FormMutator.widgetObjectNumber`). A name or an index
/// cannot join a field, because the field tree holds widgets by reference.
fn widget_field(body: &serde_json::Value) -> Result<u32, EngineError> {
    match field(body, "widget")? {
        crate::identity::AnnotationRef::ObjectNumber { annot, .. } => Ok(annot),
        _ => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "widget must be addressed by object number",
        )),
    }
}

/// The name-tree key an embedded-file reference names.
///
/// `EmbeddedFileRef` is a tagged union with exactly one arm today, and it is
/// decoded as one rather than reached into: a second arm would then be a
/// compile error here instead of a key silently read as empty.
fn embedded_file_key(body: &serde_json::Value) -> Result<String, EngineError> {
    #[derive(Deserialize)]
    #[serde(tag = "kind", rename_all = "camelCase")]
    enum EmbeddedFileRef {
        Key { key: String },
    }
    let reference: EmbeddedFileRef = field(body, "ref")?;
    let EmbeddedFileRef::Key { key } = reference;
    Ok(key)
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

/// The binaries a mutation carried, owned for the length of one call.
///
/// The bytes are owned here and borrowed by [`borrowed_resources`], because
/// a writer takes a borrow and the payloads arrive as a `Vec` this function
/// indexes into: nothing else owns the right slices for the right keys.
type OwnedResources = std::collections::BTreeMap<String, (Vec<u8>, Option<String>)>;

/// The binaries a mutation carried, keyed as its draft names them.
///
/// The JSON says which payload each key is; the payloads say what is in them.
/// Splitting it that way is what lets a draft reference an image without the
/// image ever entering the JSON.
fn resource_map(
    body: &serde_json::Value,
    payloads: &Payloads,
) -> Result<OwnedResources, EngineError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WireResource {
        /// Which payload holds the bytes.
        payload: usize,
        #[serde(default)]
        mime_type: Option<String>,
    }

    let Some(declared) = body.get("resources") else {
        return Ok(std::collections::BTreeMap::new());
    };
    if declared.is_null() {
        return Ok(std::collections::BTreeMap::new());
    }
    let declared: std::collections::BTreeMap<String, WireResource> =
        serde_json::from_value(declared.clone()).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::WireFormat,
                format!("'resources' is not a map of payload references: {e}"),
            )
        })?;

    let mut out = OwnedResources::new();
    for (key, resource) in declared {
        let bytes = payload(payloads, resource.payload, &format!("resource '{key}'"))?;
        out.insert(key, (bytes.to_vec(), resource.mime_type));
    }
    Ok(out)
}

/// What an annotation writer is given beyond its draft.
///
/// The fonts are present only when this host has claimed the thread's
/// registry. A draft naming a registered family without one is refused by
/// the context itself, saying that this host has no registry, which is more
/// useful than a silent fall back to Helvetica: the document would get the
/// wrong glyphs and nothing downstream could tell.
fn annotation_context<'a>(
    fonts: Option<&'a crate::features::fonts::FontRegistry>,
    resources: &'a crate::features::annotation::write::context::WriteResourceMap<'a>,
) -> crate::features::annotation::write::context::AnnotationWriteContext<'a> {
    use crate::features::annotation::write::context::AnnotationWriteContext;
    match fonts {
        Some(fonts) => AnnotationWriteContext::with_fonts(fonts),
        None => AnnotationWriteContext::empty(),
    }
    .with_resources(resources)
}

/// Borrow the owned resource map in the shape a writer takes.
fn borrowed_resources(
    owned: &OwnedResources,
) -> crate::features::annotation::write::context::WriteResourceMap<'_> {
    owned
        .iter()
        .map(|(key, (bytes, mime))| {
            (
                key.clone(),
                crate::features::annotation::write::context::WriteResource {
                    bytes: bytes.as_slice(),
                    mime_type: mime.as_deref(),
                },
            )
        })
        .collect()
}

/// Where a page sits in the document today.
fn page_index_of(
    document: &pdfium::Document,
    page: crate::identity::PageObjectNumber,
) -> Result<u32, EngineError> {
    (0..document.page_count())
        .find(|index| document.page_object_number_by_index(*index) == page.get())
        .ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no page with object number {}", page.get()),
            )
        })
}

/// Every page's current revision token.
///
/// Collected before the listing rather than looked up per page, because the
/// reader takes a closure and borrowing the session inside it would conflict
/// with the document borrow the listing already holds.
fn page_tokens(
    session: &mut crate::session::document::DocumentSession,
) -> Result<
    std::collections::BTreeMap<crate::identity::PageObjectNumber, crate::revision::RevisionToken>,
    EngineError,
> {
    let count = session.document()?.document().page_count();
    let numbers: Vec<crate::identity::PageObjectNumber> = (0..count)
        .filter_map(|index| {
            crate::identity::PageObjectNumber::new(
                session.document().ok()?.document().page_object_number_by_index(index),
            )
            .ok()
        })
        .collect();
    let mut tokens = std::collections::BTreeMap::new();
    for page in numbers {
        tokens.insert(page, session.page_state(page)?.revision);
    }
    Ok(tokens)
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
