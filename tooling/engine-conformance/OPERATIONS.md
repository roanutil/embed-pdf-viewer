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
| `pages.geometry` | A | 0 | cooperative | ported | Gated by tooling/engine-conformance/vectors/pages.geometry and crates/epdf-engine/tests/geometry_corpus.rs. |

## G3

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `open.fatMem` | A | 1 | queued-only | routed | No throwIfAborted reaches PdfDocumentOpener: once an open starts it runs to completion. |
| `open.layerMemBase` | A | 1 | queued-only | routed |  |
| `open.layerFileBase` | A | 1 | queued-only | routed |  |
| `document.checkPasswordPermissions` | A | 1 | queued-only | ported |  |
| `document.probeSecurityFile` | A | 1 | queued-only | ported |  |
| `close` | control | 1 | control | routed | Idempotent: rejects new work, invalidates handles, releases on the owner thread. Closing an engine closes its documents. |
| `layer.close` | control | 1 | control | routed |  |
| `abort` | control | 1 | control | routed | Handled before the WorkerHost switch (WorkerHost.ts:199), which is why that switch has 64 arms and the protocol has 65 kinds. |
| `shutdown` | control | 1 | control | routed |  |
| `document.version` | A | 1 | queued-only | routed | SignatureReader.version (WorkerHost.ts:1018-1024): the hash and length of the base the session is on. |
| `objectNumbers.reserve` | A | 1 | queued-only | planned | A session effect: moves the layer's last object number up. Only a session that hands out object numbers itself takes it. |
| `pages.workingSet` | A | 1 | queued-only | planned | Never answers: the worker takes it on arrival, even mid-render, and jobId only names the message. Not one of the four control frames. |

## G4

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.render` | A | 1 | non-preemptible | ported | Cooperative up to FPDF_RenderPageBitmapWithMatrix (PageRenderReader.ts:51), which cannot be interrupted. |
| `fonts.register` | A | 1 | queued-only | routed | Engine-level, not per document. One Rust FontRegistry per combined worker owns the key-to-ID map and the retained handles. |
| `fonts.addFallback` | A | 1 | queued-only | routed |  |
| `fonts.clearFallbacks` | A | 1 | queued-only | routed |  |
| `fonts.clear` | A | 1 | queued-only | routed |  |
| `fonts.authorizeEditing` | A | 1 | queued-only | routed |  |
| `document.setFontSettings` | A | 1 | queued-only | routed | Per-document session state on the handle (EPDFDoc_SetFontEmbeddingPolicy, EPDFDoc_SetTypographicFeatures, WorkerHost.ts:2285-2290), never written to the file: a session effect. Filed with the fonts it configures. |

## G5

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.text` | A | 2 | cooperative | ported | Already coarse (PageTextReader.ts:93). Gate on correctness, not on the boundary. |
| `search.query` | A | 2 | cooperative | ported | Entry gate: a dialect vector file for regex.ts and fold.ts semantics before any porting starts. |

## G6

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.read` | A | 2 | cooperative | ported |  |
| `pages.list` | A | 2 | cooperative | ported | Page identity must agree between lazy per-page lookup and full enumeration (DocumentSession.ts:41-44). |
| `actions.read` | A | 2 | cooperative | ported |  |
| `outline.read` | A | 2 | cooperative | planned |  |
| `metadata.readCustom` | A | 2 | cooperative | planned |  |
| `measure.viewports` | A | 2 | cooperative | routed |  |

## G7

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.renderAppearances` | A | 2 | non-preemptible | ported |  |
| `annotations.list` | A | 2 | cooperative | planned | Replaces annotations.listRawAll, listRawPage and listFullPage. |
| `annotations.readAppearance` | A | 2 | cooperative | planned | An annotation's appearance as a one-page PDF (bytes). |

## G8

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `attachments.list` | A | 2 | cooperative | ported |  |
| `attachments.readFile` | A | 2 | cooperative | ported |  |
| `annotations.readFile` | A | 2 | cooperative | ported | Byte equality with the TypeScript path, not a semantic comparison. |
| `pieceInfo.read` | A | 2 | cooperative | ported |  |
| `pieceInfo.applications` | A | 2 | cooperative | ported |  |
| `forms.list` | A | 2 | cooperative | ported | Filed by behaviour: a read despite the forms. prefix. |
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
| `annotations.create` | C | 4 | after-first-write | ported | The Phase 0 Stage B bridge spike runs over this operation, then is thrown away. |
| `annotations.update` | C | 4 | after-first-write | ported |  |
| `annotations.delete` | C | 4 | after-first-write | ported |  |
| `annotations.import` | C | 4 | after-first-write | planned |  |
| `annotations.reorder` | C | 4 | after-first-write | planned | Replaces annotations.move. |
| `document.apply` | C | 4 | after-first-write | planned | One change as one transaction: annotation, form, page, attachment, metadata and outline ops (Change.ts). Carries the page and attachment writes that were their own kinds. |
| `document.applyChanges` | C | 4 | after-first-write | planned | A server request's changes as one job, each in its own layer transaction. |

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
| `metadata.update` | C | 4 | after-first-write | ported | MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep \| Clear \| Set(T). Operation parity lands here. |
| `pieceInfo.update` | C | 4 | after-first-write | ported |  |
| `metadata.updateCustom` | C | 4 | after-first-write | planned |  |
| `pieceInfo.delete` | C | 4 | after-first-write | planned | Replaces pieceInfo.clear. |
| `measure.setScale` | C | 4 | after-first-write | routed |  |

## G15

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.renderEncoded` | C | 5 | non-preemptible | routed | Sole owner of the wire kind; the raster step comes from G4 internal entry point. Encoding stays host-injected. |
| `annotations.renderAppearancesEncoded` | C | 5 | non-preemptible | routed |  |
| `document.renderPageFile` | C | 5 | non-preemptible | routed |  |
| `document.renderPageFileEncoded` | C | 5 | non-preemptible | routed |  |
| `signatures.list` | A | 5 | queued-only | routed | No port group in multiplatform-port-order.md covers signing yet; parked in G15 until the port order assigns one. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.analyze` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.contents` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.digest` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.revisionBytes` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.prepare` | B | 5 | queued-only | routed | A snapshot, like the saves. Never touches the live document: the candidate is parked on the session until complete or cancel. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.complete` | C | 5 | queued-only | routed | A content write. Installs the sealed bytes as the session document (DocumentSession.install), which advances the edit sequence itself. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.cancel` | A | 5 | queued-only | planned | A session effect. Still planned: this branch's Rust host routes it under its pre-rename kind, signatures.abort (crates/epdf-engine/src/host.rs:519), so a signatures.cancel request does not reach it. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.finalizeCandidate` | C | 5 | after-first-write | routed | Session-less: patches a signing candidate file in place and verifies it. |
