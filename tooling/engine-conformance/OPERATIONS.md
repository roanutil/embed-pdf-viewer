# Operation matrix

Generated from `src/operations.ts`. Do not edit by hand; run
`pnpm --filter @embedpdf/tooling-engine-conformance run operations`.

Every one of the 65 `WorkerRequest` kinds, the port group that owns it,
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
| `open.fatMem` | A | 1 | queued-only | planned | No throwIfAborted reaches PdfDocumentOpener: once an open starts it runs to completion. |
| `open.layerMemBase` | A | 1 | queued-only | planned |  |
| `open.layerFileBase` | A | 1 | queued-only | planned |  |
| `document.checkPasswordPermissions` | A | 1 | queued-only | planned |  |
| `document.probeSecurityFile` | A | 1 | queued-only | planned |  |
| `close` | control | 1 | control | planned | Idempotent: rejects new work, invalidates handles, releases on the owner thread. Closing an engine closes its documents. |
| `layer.close` | control | 1 | control | planned |  |
| `abort` | control | 1 | control | planned | Handled before the WorkerHost switch (WorkerHost.ts:199), which is why that switch has 64 arms and the protocol has 65 kinds. |
| `shutdown` | control | 1 | control | planned |  |

## G4

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.render` | A | 1 | non-preemptible | planned | Cooperative up to FPDF_RenderPageBitmapWithMatrix (PageRenderReader.ts:51), which cannot be interrupted. |
| `fonts.register` | A | 1 | queued-only | planned | Engine-level, not per document. One Rust FontRegistry per combined worker owns the key-to-ID map and the retained handles. |
| `fonts.addFallback` | A | 1 | queued-only | planned |  |
| `fonts.clearFallbacks` | A | 1 | queued-only | planned |  |
| `fonts.clear` | A | 1 | queued-only | planned |  |

## G5

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.text` | A | 2 | cooperative | planned | Already coarse (PageTextReader.ts:93). Gate on correctness, not on the boundary. |
| `search.query` | A | 2 | cooperative | planned | Entry gate: a dialect vector file for regex.ts and fold.ts semantics before any porting starts. |

## G6

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.read` | A | 2 | cooperative | planned |  |
| `pages.list` | A | 2 | cooperative | planned | Page identity must agree between lazy per-page lookup and full enumeration (DocumentSession.ts:41-44). |
| `actions.read` | A | 2 | cooperative | planned |  |

## G7

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.listRawAll` | A | 2 | cooperative | planned |  |
| `annotations.listRawPage` | A | 2 | cooperative | planned |  |
| `annotations.listFullPage` | A | 2 | cooperative | planned | Reaches the cached form model through joinWidgetField.ts:23, so it carries a repeated-read benchmark against that cache. |
| `annotations.renderAppearances` | A | 2 | non-preemptible | planned |  |

## G8

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `attachments.list` | A | 2 | cooperative | planned |  |
| `attachments.readFile` | A | 2 | cooperative | planned |  |
| `annotations.readFile` | A | 2 | cooperative | planned | Byte equality with the TypeScript path, not a semantic comparison. |
| `pieceInfo.read` | A | 2 | cooperative | planned |  |
| `pieceInfo.applications` | A | 2 | cooperative | planned |  |
| `forms.list` | A | 2 | cooperative | planned | Filed by behaviour: a read despite the forms. prefix. |
| `forms.export` | A | 2 | cooperative | planned |  |

## G9

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.extract` | A | 2 | cooperative | planned | Creates and closes its own FPDF_CreateNewDocument scratch document (PagesExtractor.ts:56-89), a named exception to Stage A "retains nothing, closes nothing". |

## G10

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `document.saveBuffer` | B | 4 | after-first-write | planned |  |
| `document.saveFile` | B | 4 | after-first-write | planned |  |
| `document.saveLayerBuffer` | B | 4 | after-first-write | planned |  |

## G11

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.create` | C | 4 | after-first-write | planned | The Phase 0 Stage B bridge spike runs over this operation, then is thrown away. |
| `annotations.update` | C | 4 | after-first-write | planned |  |
| `annotations.delete` | C | 4 | after-first-write | planned |  |
| `annotations.move` | C | 4 | after-first-write | planned |  |

## G12

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `forms.setValue` | C | 4 | after-first-write | planned |  |
| `forms.reset` | C | 4 | after-first-write | planned |  |
| `forms.applyEffects` | C | 4 | after-first-write | planned |  |
| `forms.import` | C | 4 | after-first-write | planned |  |
| `forms.repair` | C | 4 | after-first-write | planned |  |
| `forms.createField` | C | 4 | after-first-write | planned |  |
| `forms.updateField` | C | 4 | after-first-write | planned |  |
| `forms.deleteField` | C | 4 | after-first-write | planned |  |
| `forms.attachWidget` | C | 4 | after-first-write | planned |  |
| `forms.detachWidget` | C | 4 | after-first-write | planned |  |

## G13

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.rotate` | C | 4 | after-first-write | planned |  |
| `pages.move` | C | 4 | after-first-write | planned | Page identity must survive reorder within the same live document. |
| `pages.delete` | C | 4 | after-first-write | planned |  |
| `pages.insertBlank` | C | 4 | after-first-write | planned |  |
| `pages.insert` | C | 4 | after-first-write | planned | Source retention lives on one owner (PagesInserter.ts:77, session.retainUntilClose). A source document handle never crosses owners. |
| `pages.flatten` | C | 4 | after-first-write | planned |  |
| `redaction.apply` | C | 4 | after-first-write | planned |  |

## G14

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.update` | C | 4 | after-first-write | planned | MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep \| Clear \| Set(T). Operation parity lands here. |
| `pieceInfo.update` | C | 4 | after-first-write | planned |  |
| `pieceInfo.clear` | C | 4 | after-first-write | planned |  |
| `attachments.create` | C | 4 | after-first-write | planned |  |
| `attachments.delete` | C | 4 | after-first-write | planned |  |

## G15

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.renderEncoded` | C | 5 | non-preemptible | planned | Sole owner of the wire kind; the raster step comes from G4 internal entry point. Encoding stays host-injected. |
| `annotations.renderAppearancesEncoded` | C | 5 | non-preemptible | planned |  |
| `document.renderPageFile` | C | 5 | non-preemptible | planned |  |
| `document.renderPageFileEncoded` | C | 5 | non-preemptible | planned |  |
