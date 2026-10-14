# Operation matrix

Generated from `src/operations.ts`. Do not edit by hand; run
`pnpm --filter @embedpdf/tooling-engine-conformance run operations`.

Every one of the 84 `WorkerRequest` kinds, the port group that owns it,
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
| `abort` | control | 1 | control | routed | Handled before the WorkerHost switch (WorkerHost.ts:282), which is why that switch has 83 arms and the protocol has 84 kinds. |
| `shutdown` | control | 1 | control | routed |  |

## G4

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.render` | A | 1 | non-preemptible | ported | Cooperative up to FPDF_RenderPageBitmapWithMatrix (PageRenderReader.ts:51), which cannot be interrupted. |
| `fonts.register` | A | 1 | queued-only | routed | Engine-level, not per document. One Rust FontRegistry per combined worker owns the key-to-ID map and the retained handles. |
| `fonts.addFallback` | A | 1 | queued-only | routed |  |
| `fonts.clearFallbacks` | A | 1 | queued-only | routed |  |
| `fonts.clear` | A | 1 | queued-only | routed |  |
| `fonts.authorizeEditing` | A | 1 | queued-only | routed |  |
| `document.setFontSettings` | A | 1 | queued-only | routed | Per-document session state on the handle (EPDFDoc_SetFontEmbeddingPolicy, EPDFDoc_SetTypographicFeatures), never written to the file. Filed with the fonts it configures. |

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
| `measure.viewports` | A | 2 | cooperative | routed |  |

## G7

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.listRawAll` | A | 2 | cooperative | ported |  |
| `annotations.listRawPage` | A | 2 | cooperative | ported |  |
| `annotations.listFullPage` | A | 2 | cooperative | ported | Reaches the cached form model through joinWidgetField.ts:23, so it carries a repeated-read benchmark against that cache. |
| `annotations.renderAppearances` | A | 2 | non-preemptible | ported |  |

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
| `signatures.list` | A | 2 | queued-only | routed | The signature handlers take no AbortSignal (WorkerHost.ts), so no throwIfAborted reaches them. |
| `signatures.contents` | A | 2 | queued-only | routed |  |
| `signatures.digest` | A | 2 | queued-only | routed |  |
| `signatures.revisionBytes` | A | 2 | queued-only | routed |  |
| `signatures.analyze` | A | 2 | queued-only | routed |  |
| `document.version` | A | 2 | queued-only | routed | SignatureReader.version: the hash and length of the base the session is on. |

## G9

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.extract` | A | 2 | cooperative | ported | Creates and closes its own FPDF_CreateNewDocument scratch document (PagesExtractor.ts:56-89), a named exception to Stage A "retains nothing, closes nothing". |
| `annotations.exportAppearance` | A | 2 | non-preemptible | ported | A read that builds a scratch single-page document (EPDFPage_ExportAnnotationsAsDocument) and closes it before returning, like pages.extract. Gated by vectors/pages/mutate.json. |

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
| `annotations.move` | C | 4 | after-first-write | ported |  |

## G12

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `forms.setValue` | C | 4 | after-first-write | ported |  |
| `forms.reset` | C | 4 | after-first-write | ported |  |
| `forms.applyEffects` | C | 4 | after-first-write | ported |  |
| `forms.import` | C | 4 | after-first-write | ported |  |
| `forms.repair` | C | 4 | after-first-write | ported |  |
| `forms.createField` | C | 4 | after-first-write | ported |  |
| `forms.updateField` | C | 4 | after-first-write | ported |  |
| `forms.deleteField` | C | 4 | after-first-write | ported |  |
| `forms.attachWidget` | C | 4 | after-first-write | ported |  |
| `forms.detachWidget` | C | 4 | after-first-write | ported |  |
| `forms.setSignatureAppearance` | C | 4 | after-first-write | routed |  |
| `signatures.prepare` | C | 4 | queued-only | routed | Never touches the live document: the candidate is parked on the session until complete or abort. |
| `signatures.complete` | C | 4 | queued-only | routed | Installs the sealed bytes as the session document; the install advances the mutation sequence itself. |
| `signatures.abort` | C | 4 | queued-only | routed |  |

## G13

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.rotate` | C | 4 | after-first-write | ported |  |
| `pages.move` | C | 4 | after-first-write | ported | Page identity must survive reorder within the same live document. |
| `pages.delete` | C | 4 | after-first-write | ported |  |
| `pages.insertBlank` | C | 4 | after-first-write | routed |  |
| `pages.insert` | C | 4 | after-first-write | routed | Source retention lives on one owner (PagesInserter.ts:77, session.retainUntilClose). A source document handle never crosses owners. |
| `pages.setName` | C | 4 | after-first-write | routed |  |
| `pages.removeName` | C | 4 | after-first-write | routed |  |
| `pages.flatten` | C | 4 | after-first-write | ported |  |
| `annotations.flatten` | C | 4 | after-first-write | ported | pages.flatten for a chosen set of one page's annotations. Gated by vectors/pages/mutate.json. |
| `redaction.apply` | C | 4 | after-first-write | ported |  |

## G14

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.update` | C | 4 | after-first-write | ported | MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep \| Clear \| Set(T). Operation parity lands here. |
| `pieceInfo.update` | C | 4 | after-first-write | ported |  |
| `pieceInfo.clear` | C | 4 | after-first-write | ported |  |
| `measure.setScale` | C | 4 | after-first-write | routed |  |
| `attachments.create` | C | 4 | after-first-write | ported |  |
| `attachments.delete` | C | 4 | after-first-write | ported |  |

## G15

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.renderEncoded` | C | 5 | non-preemptible | routed | Sole owner of the wire kind; the raster step comes from G4 internal entry point. Encoding stays host-injected. |
| `annotations.renderAppearancesEncoded` | C | 5 | non-preemptible | routed |  |
| `document.renderPageFile` | C | 5 | non-preemptible | routed |  |
| `document.renderPageFileEncoded` | C | 5 | non-preemptible | routed |  |
| `signatures.finalizeCandidate` | C | 5 | queued-only | routed | Session-less, like document.renderPageFile: patches the caller's candidate file in place and reads it back through a document that is never a session. |
