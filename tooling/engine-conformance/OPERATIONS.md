# Operation matrix

Generated from `src/operations.ts`. Do not edit by hand; run
`pnpm --filter @embedpdf/tooling-engine-conformance run operations`.

Every one of the 89 `WorkerRequest` kinds, the port group that owns it,
the ownership stage it moves under, the phase that moves it, and the
cancellation case it has to document. Group assignments come from
`multiplatform-port-order.md` Section 3; cancellation cases from
`multiplatform-plan.md` Section 7.

## G2

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.geometry` | A | 0 | cooperative | ported | Answers PageGeometrySnapshot in page space: the reader answers the file's coordinates (runs with start, loose/tight boxes or quads, space/empty, rotation in degrees clockwise) and the host boundary converts them (pageGeometryOf). The C ABI export epdf_page_geometry answers the file's coordinates, which RustPageGeometryReader.ts reads as PageGeometrySnapshot<PdfCoordinates>. Gated by tooling/engine-conformance/vectors/pages.geometry through Host::call in crates/epdf-engine/tests/geometry_corpus.rs, and crates/epdf-wasm/tests/borrowed_page.rs. |

## G3

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `open.fatMem` | A | 1 | queued-only | routed | No throwIfAborted reaches PdfDocumentOpener: once an open starts it runs to completion. |
| `open.layerMemBase` | A | 1 | queued-only | routed |  |
| `open.layerFileBase` | A | 1 | queued-only | routed |  |
| `document.checkPasswordPermissions` | A | 1 | queued-only | ported | Answers { security, protection } as WorkerHost.handleDocumentCheckPasswordPermissions does; on a locked session it loads the parked document with the password. |
| `document.probeSecurityFile` | A | 1 | queued-only | ported | Answers { security } as WorkerHost.handleDocumentProbeSecurityFile does. Needs no session. |
| `close` | control | 1 | control | routed | Idempotent: rejects new work, invalidates handles, releases on the owner thread. Closing an engine closes its documents. |
| `layer.close` | control | 1 | control | routed |  |
| `abort` | control | 1 | control | routed | Handled in WorkerHost.receive before anything is queued (WorkerHost.ts:413), as pages.workingSet is (WorkerHost.ts:418). The other 87 of the 89 kinds reach a switch: 76 arms in run() (WorkerHost.ts:554) and 11 sliced reads in slicedWork() (WorkerHost.ts:1896). |
| `shutdown` | control | 1 | control | routed |  |
| `document.version` | A | 1 | queued-only | routed | SignatureReader.version (WorkerHost.ts:1018-1024): the hash and length of the base the session is on. |
| `objectNumbers.reserve` | A | 1 | queued-only | routed | A session effect: moves the layer's last object number up. Only a session that hands out object numbers itself takes it. Rust hands numbers out on a layer session only: it opens open.fatMem bytes as a plain document, where WorkerHost.openBytesAsLayer opens a layer, so a fat-memory session answers NotImplemented (wire_contract STUBBED) until Task 8.29 opens it as a layer. |
| `pages.workingSet` | A | 1 | non-preemptible | routed | Never answers and never queues: WorkerHost.receive takes it on arrival, even mid-render (WorkerHost.ts:418), and jobId only names the message. So it is not queued-only, since there is no queue to drop it from; an abort finds no job and the bookkeeping always completes, which is non-preemptible with no wait. Not one of the four control frames (worker-kinds.ts), so it cannot carry the control case. Rust records the sets on the page pool of an open session and answers null, since Host::call answers every call; input it cannot read is dropped and answered null too, as upstream drops it unanswered; the pool keeps no page past its last holder, as an upstream pool without a residency keeps none. |

## G4

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.render` | A | 1 | non-preemptible | ported | Cooperative up to FPDF_RenderPageBitmapWithMatrix (PageRenderReader.ts:51), which cannot be interrupted. Rust takes the target in page space and answers page and area, converting both at the host boundary (page_space::boundary, pdfRenderTargetOf); usage, the dpi viewport and includeFormFields are not read yet. |
| `fonts.register` | A | 1 | queued-only | routed | Engine-level, not per document. One Rust FontRegistry per combined worker owns the key-to-ID map and the retained handles. |
| `fonts.addFallback` | A | 1 | queued-only | routed |  |
| `fonts.clearFallbacks` | A | 1 | queued-only | routed |  |
| `fonts.clear` | A | 1 | queued-only | routed |  |
| `fonts.authorizeEditing` | A | 1 | queued-only | routed |  |
| `document.setFontSettings` | A | 1 | queued-only | routed | Per-document session state on the handle (EPDFDoc_SetFontEmbeddingPolicy, EPDFDoc_SetTypographicFeatures, WorkerHost.ts:2285-2290), never written to the file: a session effect. Filed with the fonts it configures. |

## G5

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.text` | A | 2 | cooperative | ported | Already coarse (PageTextReader.ts:93). Gate on correctness, not on the boundary. Answers PageTextSnapshot, which holds no places, so the boundary passes it through. Gated by vectors/pages.text in crates/epdf-engine/tests/text_corpus.rs. |
| `search.query` | A | 2 | cooperative | ported | Takes the flattened SearchScanRequest (query fields at the top level, snippets, from as a PageRef, cursor, limit) and answers SearchSlice: matches as page ranges (start, count) with a snippet of before, match and after, pagesSearched and pageCount. The cursor key is searchQueryKey, mode spelled snippets or matches. The reader answers segments in the file's coordinates and the host boundary converts them (pageSearchSliceOf). Gated by vectors/search through crates/epdf-engine/tests/search_dialect.rs, search_slice_protocol.rs, search_reader.rs and search_ignore_whitespace.rs. |

## G6

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.read` | A | 2 | cooperative | ported | Gated by vectors/metadata/read.json through crates/epdf-engine/tests/metadata_corpus.rs. The standard fields only: createdAt and modifiedAt keep the offset the file wrote (pdfDateToIso); the custom keys are metadata.readCustom. |
| `metadata.readCustom` | A | 2 | cooperative | ported | Gated with metadata.read: the generator folds metadata.custom.get() into the read it records, and metadata_corpus.rs does the same. |
| `pages.list` | A | 2 | cooperative | ported | Page identity must agree between lazy per-page lookup and full enumeration (DocumentSession.ts:41-44). Rust reads the boxes by pageBoxesOf and /Rotate by pageRotationOf, and answers them in page space at the host boundary (pageListOf). |
| `actions.read` | A | 2 | cooperative | ported | Rust reads openDestination and answers every destination in page space at the host boundary (mapDocumentActions), triggers first. |
| `outline.read` | A | 2 | cooperative | planned |  |
| `measure.viewports` | A | 2 | cooperative | routed |  |

## G7

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.renderAppearances` | A | 2 | non-preemptible | ported | One family per request (annotations, or widgets for page.forms). Every mode and every state an annotation stores, each labelled (17350fb4c); an annotation with no normal appearance is drawn in memory into EPDFAnnot_GetDrawingRect's box, never written (25dd21f38). A box kind drawn turned renders upright into its box, the turn passed to EPDF_RenderAnnotBitmapUnrotated (runtime 5e9241945). Refs by baseIndex or object number; rects converted to page space at the host boundary (pageAppearancesOf). Gated by vectors/annotation/appearances.json through crates/epdf-engine/tests/annotation_appearance.rs. |
| `annotations.list` | A | 2 | cooperative | ported | Replaces annotations.listRawAll, listRawPage and listFullPage. Unlike listFullPage it builds no widget rows (RawAnnotationReader.list with the 'annotations' family), so it never reaches the cached form model; the widget-to-field join and its repeated-read benchmark moved to forms.list. Answers AnnotationList in upstream's record shape, read in the file's coordinates and converted at the host boundary by field space (pageAnnotationOf); a promoted inline annotation reads by its birth name (EPDFLayer_GetBirthName). Gated by vectors/annotation/read.json through crates/epdf-engine/tests/annotation_reader.rs. |
| `annotations.readAppearance` | A | 2 | cooperative | ported | An annotation's appearance as a one-page PDF (bytes): EPDFAnnot_ExportAppearance with the turn the annotation reads taken out (exportUnturnedAppearance). Only a kind with an appearance resource (the stamp) has one. |

## G8

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `attachments.list` | A | 2 | cooperative | ported | Answers { attachments } (AttachmentList, a9434c0e2): each entry carries its ref, createdAt and modifiedAt as ISO dates, and null for every field the PDF has no value for. Gated by vectors/files/attachments.json through Host::call. |
| `attachments.readFile` | A | 2 | cooperative | ported |  |
| `annotations.readFile` | A | 2 | cooperative | ported | Byte equality with the TypeScript path, not a semantic comparison. Its ref resolves as upstream resolveAnnotIndexRaw does, a baseIndex on a promoted page through the layer's births. |
| `pieceInfo.read` | A | 2 | cooperative | ported |  |
| `pieceInfo.applications` | A | 2 | cooperative | ported |  |
| `forms.list` | A | 2 | cooperative | ported | Filed by behaviour: a read despite the forms. prefix. Answers FormSnapshot: the fields in upstream's shape (ref by objectNumber, the /Ff bits as readOnly, required and noExport, groupId and attribution from the field's /EMBD_Metadata) and every widget row the pages' /Annots hold, each joined to its field (widgetRows.ts, joinWidgetField.ts). The reader answers the file's coordinates and the host boundary converts them (pageFormSnapshotOf). Upstream joins through its cached form model (acquireFormModel) and carries the repeated-read benchmark annotations.listFullPage carried; the Rust host has no form model cache and builds one model per read. Gated by vectors/forms/snapshot.json through crates/epdf-engine/tests/form_reader.rs. |
| `forms.export` | A | 2 | cooperative | routed |  |
| `annotations.export` | A | 2 | cooperative | planned | Annotations and their resources as one bundle; a snapshot, like forms.export. |

## G9

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.extract` | A | 2 | cooperative | ported | Creates and closes its own FPDF_CreateNewDocument scratch document (PagesExtractor.ts:56-89), a named exception to Stage A "retains nothing, closes nothing". |
| `annotations.exportAppearance` | A | 2 | cooperative | ported | Flattens chosen annotation appearances into a new single-page PDF; a snapshot, like pages.extract. Builds a scratch document (EPDFPage_ExportAnnotationsAsDocument) and closes it before returning. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/pages_mutate.rs. |

## G10

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `document.saveBuffer` | B | 4 | after-first-write | ported |  |
| `document.saveFile` | B | 4 | after-first-write | ported |  |
| `document.saveLayerBuffer` | B | 4 | after-first-write | ported |  |

## G11

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.create` | C | 4 | after-first-write | ported | A one-op change (applyOne) and an op of document.apply: the v3 draft in page space, converted at the boundary (pdfAnnotationDraftOf), checked field by field against its kind (prepareCreate), and made on its page's /Annots without loading the page (EPDFPage_CreateAnnotRaw). Stamped with a UUIDv7 /NM when it names none; a stamp places a drawing found or added once per artwork (drawingFor); its reverse deletes it while it reads as left. Measurement fields (measure, captions, leader) are refused NotImplemented until 8.25. Gated by vectors/annotation/create.json through crates/epdf-engine/tests/annotation_create.rs. |
| `annotations.update` | C | 4 | after-first-write | ported |  |
| `annotations.delete` | C | 4 | after-first-write | ported |  |
| `annotations.import` | C | 4 | after-first-write | planned |  |
| `annotations.reorder` | C | 4 | after-first-write | planned | Replaces annotations.move. |
| `document.apply` | C | 4 | after-first-write | routed | One change as one transaction: annotation, form, page, attachment, metadata and outline ops (Change.ts). Rust runs the metadata and attachment ops (features/changes, tests/document_apply.rs, tests/files_write.rs) and answers NotImplemented naming the owner sub-task for every other op type; an attachment op's bytes ride as a request payload its resources name, { payload: n }, and an undo of a delete puts the file back from the file as the API reads it, where upstream imports a raw capture only a layer transaction allows; a refused change is taken back through its ops' reverse steps. Carries the page and attachment writes that were their own kinds, and their invariants: page identity survives a pages.reorder op within the same live document, and a pages.insert op's source is retained on one owner (PagesInserter.ts:88, session.retainUntilClose, reached through changes/internal/pageChanges.ts:53), so a source document handle never crosses owners. |
| `document.applyChanges` | C | 4 | after-first-write | routed | A server request's changes as one job, each in its own layer transaction. Rust runs the metadata ops, as document.apply does. |

## G12

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `forms.setValue` | C | 4 | after-first-write | ported |  |
| `forms.reset` | C | 4 | after-first-write | ported |  |
| `forms.import` | C | 4 | after-first-write | ported |  |
| `forms.repair` | C | 4 | after-first-write | ported |  |
| `forms.createField` | C | 4 | after-first-write | ported |  |
| `forms.updateField` | C | 4 | after-first-write | ported |  |
| `forms.deleteField` | C | 4 | after-first-write | ported |  |
| `forms.detachWidget` | C | 4 | after-first-write | ported |  |
| `forms.addWidget` | C | 4 | after-first-write | planned | Replaces forms.attachWidget. |
| `forms.updateWidget` | C | 4 | after-first-write | planned |  |
| `forms.deleteWidget` | C | 4 | after-first-write | planned |  |
| `forms.reorderWidgets` | C | 4 | after-first-write | planned |  |
| `forms.reorderCalculations` | C | 4 | after-first-write | planned |  |
| `forms.importValues` | C | 4 | after-first-write | planned |  |
| `forms.setSignatureAppearance` | C | 4 | after-first-write | routed |  |

## G13

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.flatten` | C | 4 | after-first-write | ported |  |
| `redaction.apply` | C | 4 | after-first-write | ported |  |
| `annotations.flatten` | C | 4 | after-first-write | ported | A content write: pages.flatten for a chosen set of one page's annotations. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/pages_mutate.rs. |
| `pages.setName` | C | 4 | after-first-write | routed |  |
| `pages.removeName` | C | 4 | after-first-write | routed |  |

## G14

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.update` | C | 4 | after-first-write | ported | MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep \| Clear \| Set(T). Gated by vectors/metadata/update.json through crates/epdf-engine/tests/metadata_write.rs, which replays the recorded request. Runs as a one-op change, so its opId undoes it. |
| `metadata.updateCustom` | C | 4 | after-first-write | ported | Gated with metadata.update by vectors/metadata/update.json. A standard key, a leading slash or a key outside printable ASCII is InvalidArg before anything is written; answers the custom keys alone. |
| `pieceInfo.update` | C | 4 | after-first-write | ported | Answers PieceInfoUpdateResult, { pieceInfo, meta }, as WorkerHost.handlePieceInfoUpdate does, and publishes no event. |
| `pieceInfo.delete` | C | 4 | after-first-write | ported | Replaces pieceInfo.clear. Answers { meta } and publishes no event. |
| `measure.setScale` | C | 4 | after-first-write | routed |  |

## G15

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.renderEncoded` | C | 5 | non-preemptible | routed | Sole owner of the wire kind; the raster step comes from G4 internal entry point. Encoding stays host-injected. Rust takes the target in page space, as pages.render does. |
| `annotations.renderAppearancesEncoded` | C | 5 | non-preemptible | routed |  |
| `document.renderPageFile` | C | 5 | non-preemptible | routed | The target is page space and converts on the page the render opens (renderOptionsInFileSpace), since no session carries the document to the host boundary. |
| `document.renderPageFileEncoded` | C | 5 | non-preemptible | routed | The target is page space, as document.renderPageFile takes it. |
| `signatures.list` | A | 5 | queued-only | routed | No port group in multiplatform-port-order.md covers signing yet; parked in G15 until the port order assigns one. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.analyze` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.contents` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.digest` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.revisionBytes` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.prepare` | B | 5 | after-first-write | routed | A snapshot, like the saves. Never touches the live document: the candidate is parked on the session until complete or cancel. Filed after-first-write by its stage and effect, like the saves, though WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them today. |
| `signatures.complete` | C | 5 | after-first-write | routed | A content write. Installs the sealed bytes as the session document (DocumentSession.install), which advances the edit sequence itself. Filed after-first-write by its stage and effect, like the other content writes, though WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them today. |
| `signatures.cancel` | A | 5 | queued-only | planned | A session effect. Still planned: this branch's Rust host keeps the arm under its pre-rename kind, signatures.abort, which the wire answers as an unknown kind and only Host::call_legacy reaches (crates/epdf-engine/src/host.rs, LEGACY_KINDS) until Task 8.26 renames it. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.finalizeCandidate` | C | 5 | queued-only | routed | Session-less, like document.renderPageFile: patches the caller's candidate file in place and reads it back through a document that is never a session. Kept at stage C though its effect is read: no session's document changes, but it opens, writes and closes a document of its own. queued-only, not after-first-write: the handler gets no AbortSignal (WorkerHost.ts:626-627, 1071-1083), so once it starts it runs to the end. |
