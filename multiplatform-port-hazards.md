# TypeScript designs that do not port to Rust as written

Status: proposal. Date: 2026-09-09. Companion to `multiplatform-plan-claude-remix.md` and
`multiplatform-port-order.md`.

Scope: modules in `packages/engine/services` and `packages/engine/core` whose current design relies
on JavaScript runtime features, host objects, or TypeScript type-system idioms with no direct Rust
equivalent. Each entry names the TypeScript design, why a line-by-line translation fails, the Rust
design that replaces it, and the size of the rework. Modules not listed here port mechanically.

Counts are from `grep` over non-test `.ts` files at commit `12574f5f4`. Every file cited below was
opened on 2026-09-09: in full for `PagePtrPool.ts`, `formModelCache.ts`, `BaseDocumentRegistry.ts`
(first 60 lines), `annotationReaderRegistry.ts`; at the cited lines for the rest. Section 5 files
were scanned by structure (class and method lines) and by a grep for runtime-dependent patterns, not
read line by line.

## 1. Severity legend

| Level | Meaning |
|---|---|
| Redesign | The structure changes. Callers change. Needs a design note before code. |
| Rewrite | Same structure, different mechanism. Every call site changes but mechanically. |
| Contract | Code ports directly; the observable behavior needs a written contract because the platforms differ. |

## 2. Redesign

### 2.1 WASM memory access layer

| Field | Value |
|---|---|
| TypeScript | `services/src/runtime/memory/{bits,scratch,strings,structs}.ts` (193 lines); `PdfRuntimeModule`, `PdfFunctions`, `PdfRuntimeMemory` from `packages/engine/runtime` |
| Pervasiveness | 102 of 145 service files import `@embedpdf/engine-runtime`. 302 call sites: 244 `mem.*` calls (`free` 64, `poke` 62, `alloc` 47, `peek` 34, string and byte helpers 37) plus 58 scratch scopes (`withScratch` 33, `withScratchN` 25). Every reader and writer takes `(fn, mem, ptr)`. |
| Why it does not port | The entire service layer is written as a JavaScript client of WASM linear memory: manual `alloc` / `free`, hand-pinned struct offsets (`EPDF_CHAR_GEOMETRY_LAYOUT` at `structs.ts:17`, 124 bytes with 6 offsets), UTF-16 string marshalling through scratch buffers. In Rust the PDFium call is a direct FFI call and the struct is a `bindgen` type. There is nothing to translate; the layer disappears. |
| Rust design | `crates/pdfium-sys` (`bindgen`) plus `crates/pdfium` RAII types. Out-parameters become stack locals. Strings go through `widestring` / `Vec<u16>`. |
| Rework | Every function signature in `features/` changes shape. The logic inside survives; the plumbing around each PDFium call is rewritten. This is the single largest source of diff volume in the port. |
| Port order | G0 (`multiplatform-port-order.md`) |
| Retirement | The Rust layer exists from G0, but the TypeScript one is not deleted then. Stage A keeps TypeScript as session owner and every unflipped service still reaches PDFium through it, so porting a family only ends its use on the Rust path. `runtime/memory/`, generated `PdfFunctions`, and `PdfRuntimeMemory` stay maintained through Phase 5 for the TypeScript server and rollback backends. Final removal in Phase 6 requires passing the server migration and retiring the TypeScript fallback. |

### 2.2 Document session, page pool, revisions

| Field | Value |
|---|---|
| TypeScript | `services/src/document-session/DocumentSession.ts` (354); `pages/PagePtrPool.ts` (75); `revisions/RevisionAuthority.ts` (103); `lifecycle/{PdfDocumentOpener,BaseDocumentRegistry}.ts` (285) |
| Why it does not port | Nullable fields mutated in place (`docPtr: Ptr \| null`, `closeDocument: (() => void) \| null`, `pages: PagePtrPool \| null` at `DocumentSession.ts:35-48`). Page lifetime is a `Map<PageObjectNumber, { ptr, refs }>` with manual `acquire` / `release` in `finally` blocks and an `isHeld` assertion that structural mutations call to detect leaked acquires. `BaseDocumentRegistry` refcounts base documents across layer sessions by closure. Rust's borrow checker rejects a page pointer captured in a local that outlives the guard, which is the property this design enforces at run time with `isHeld`. |
| Rust design | `Session` as an owned state machine (`Opening \| Open(OpenState) \| Closing \| Closed`), not nullable fields. `PageLease<'s>` RAII guard borrowing the session; `Drop` releases. Structural mutations take `&mut Session`, which the compiler refuses while a lease is live, replacing `isHeld`. `BaseDocumentRegistry` becomes `Rc<BaseDocument>` on the owner thread. Stage B additionally needs the lease-token bridge for TypeScript callers (three separate entry points: checkout, return, finalize). |
| Rework | High. Every feature module's entry point changes from `(runtime, session)` constructor injection to a `&Session` or `&mut Session` parameter with lease scopes. The Stage B facade on the TypeScript side is new code. |
| Port order | G3, G10 |

### 2.3 Annotation DTO layering: handwritten schemas and registries

| Field | Value |
|---|---|
| TypeScript | `core/src/annotation/kinds/*` (19 subtype directories, each `dto`, `schema`, `patch`, `draft`, some `normalize`); `core/src/annotation/{base.schema,subtype,registry}.ts`; `services/src/features/annotations/internal/read/annotationReaderRegistry.ts` (86) and `internal/write/annotationWriterRegistry.ts` (250); 28 files in `core/src` outside `wire/` use `z.object` / `z.infer` / `z.discriminatedUnion` |
| Why it does not port | Not because of zod direction: outside `wire/`, `z.infer` appears zero times. `kinds/highlight/dto.ts` hand-declares `HighlightAnnotationDTO` and `kinds/highlight/schema.ts:14` types the schema against it as `z.ZodType<HighlightAnnotationDTO>`, so this layer is already type-first and inverting it is mostly codegen. What does not port is the dispatch and the three-state encoding. Dispatch is `Partial<Record<AnnotationSubtype, ReaderFn>>` with structural typing linking each reader to its DTO variant. Patches use `?: T \| null` for three-state fields (`patch-base.ts:7-9`, `MetadataPatch.ts:7-9`): absent leaves, `null` clears, value sets. `serde` `Option<T>` collapses absent and `null` into `None` by default. The `unsupported` fallback preserves an unknown raw subtype code without a wire-format bump. |
| Rust design | `enum AnnotationDTO { Highlight(HighlightDTO), ... , Unsupported { raw_subtype: i32, .. } }` with `#[serde(tag = "subtype")]`. Readers and writers become `match` arms; the registry indirection is deleted. Three-state fields use an explicit `enum Patch<T> { Keep, Clear, Set(T) }` with custom `Serialize` / `Deserialize` and `#[serde(default, skip_serializing_if = "Patch::is_keep")]`; `ts-rs` emits `?: T \| null`. Zod schemas stay handwritten and are tested against generated output in both directions (omitted vs null, unknown enum, `.strict()` rejection parity). That test covers `wire/schemas.ts` and, equally, the 20 handwritten schema files under `annotation/` (`base.schema.ts` plus `kinds/*/schema.ts`, 1,070 lines). Today the compiler enforces schema-to-type agreement there, except `kinds/highlight/schema.ts:14` already defeats it with `as unknown as z.ZodType<...>`; once the type is `ts-rs`-generated, nothing but the parity test stops those 20 files from drifting. Direction inverts: Rust canonical, TypeScript generated. |
| Rework | Medium by breadth, not depth: 19 subtypes × read + write + patch + draft, over 1,517 lines of `kinds/` plus 997 top-level. Lower than a true schema-first inversion would be, because the types are already handwritten. The three-state serializer and the strictness parity test are the design work. Phase 0 proves it on `MetadataPatch` before the annotation kinds are touched. |
| Port order | G1 (patch type), G7, G11 |

### 2.4 `WorkerHost`: transport fused with engine behavior

| Field | Value |
|---|---|
| TypeScript | `services/src/worker-host/WorkerHost.ts` (1,459 lines, 64 `case` arms) |
| Why it does not port | One class holds the session map, `BaseDocumentRegistry`, per-kind request decoding, `finishMutation` (lines 1380 to 1404: mutation-sequence bump with exemptions for `forms.*`, `pages.flatten`, `redaction.apply`) plus `saveLayerArtifact` (lines 1405 to 1414), and `postMessage` plumbing. On native there is no message loop, so most of the file has no target. Those ~35 lines are engine behavior that must reach Rust; the rest must not. |
| Rust design | `epdf-engine::session::finalize_mutation(&mut Session, MutationOutcome) -> Finalized` with the exemption set expressed as a property of each mutator's result type (`bumps_sequence: bool`), not a string-prefix test on the wire tag. `crates/epdf-wasm` decodes `WorkerRequest` into typed calls. TypeScript `WorkerHost` shrinks to transport. |
| Rework | Medium. Split, not port. Exactly-once test for the sequence bump on each host. |
| Port order | G10 |

## 3. Rewrite

### 3.1 Search dialect and text folding

| Field | Value |
|---|---|
| TypeScript | `core/src/search/regex.ts` (158): `new RegExp(pattern, 'mu')` at line 93 validates, line 142 executes with `gmu` / `gimu`. `core/src/search/fold.ts` (114): `cp.normalize('NFKD')` at line 62, `/\p{M}/gu` at line 49, upper-then-lower case round trip. |
| Why it does not port | The public query dialect is defined as "JavaScript `u`-mode minus backreferences and lookaround, executed by V8 locally and RE2 on the server". No Rust regex engine implements JavaScript semantics by default: the `regex` crate makes `\d` `\w` `\b` Unicode-aware where JavaScript `u` mode keeps them ASCII; case-insensitive matching uses whichever Unicode version each library ships; zero-length match advancement differs. Folding depends on V8's ICU tables for NFKD and case mapping. |
| Rust design | Two candidates, chosen in Phase 0 by running the dialect vector file: `regress` (ECMAScript-compatible, used by the Boa engine) or `regex` with a translation layer that rewrites `\d` `\w` `\b` to ASCII classes and rejects what the dialect bans. Fold via `unicode-normalization` plus explicit case mapping with a pinned Unicode version; the version becomes part of the search content epoch so cached results invalidate on a table change. |
| Rework | Medium code, high contract risk. The entry gate is the dialect vector file generated by running `RegExp` and `foldText`, covering accepted and rejected syntax, Unicode classes, case folding, empty matches, and offset mapping back to UTF-16. |
| Port order | G5 |

### 3.2 Cancellation via `AbortSignal`

| Field | Value |
|---|---|
| TypeScript | `services/src/shared/abort.ts` (14); 86 `throwIfAborted(signal)` call sites across `features/`; `core/src/promise/AbortablePromise.ts` (148) |
| Why it does not port | `AbortSignal` is a host object. Every service method takes one and throws `AbortError` from it. On the web worker an abort message is not observed while a synchronous handler runs (`WorkerHost.ts:199`, `:225`), so today's "cooperative" checks only fire between message-loop turns for chunked work, and never inside one WASM call. |
| Rust design | `CancelToken(Arc<AtomicBool>)` passed by value; `token.check()?` at the same 86 points. Native: the owner thread's job carries the token and the facade's `Task` / `Job` cancellation sets it. Web Stage A: token is set only between jobs (cancellation of waiting). Web Stage B: optional `SharedArrayBuffer` flag polled from WASM for embedders that can set COOP and COEP. `AbortablePromise` stays in TypeScript unchanged. |
| Rework | Mechanical at call sites. The work is the per-operation contract table (four cases in the plan, Section 7) and the tests for queued, pre-apply, post-write, and late-result cancellation. |
| Port order | G1 |

### 3.3 File access and file write handles

| Field | Value |
|---|---|
| TypeScript | `PdfRuntimeModule.fileAccess.fromNodeFile(path)` and `fileWrite.toNodeFile(path)` (`packages/engine/runtime/src/core/pdf-runtime-module.ts:46,58,66,67`), implemented in `runtime/src/wasm/wasm-runtime.ts` and `runtime/src/native/native-runtime.ts`. Callers: `FontRegistrar.ts:108-111` (`EPDFFont_RegisterFont`, handles retained in `fileHandles` at line 51 until `clear`), `attachmentPrimitives.ts:193-197` (`EPDFAttachment_ExtractFile`, server mode), `PdfDocumentOpener.ts:108` (`open.layerFileBase`), `BaseDocumentRegistry.acquireFileBase`. |
| Why it does not port | Services never touch `FPDF_FILEACCESS` / `FPDF_FILEWRITE` directly; the runtime package builds those structs and their function-table entries. The legacy runtime plumbing is removed with the memory layer only after the server migration passes and the TypeScript fallback is retired (2.1); the loader stays. Rust needs its own mechanism before that removal. Two lifetime rules ride on it: a font registered from a path keeps its handle alive until the registry is cleared (PDFium range-reads lazily), and a file-backed base document keeps its handle for the session. |
| Rust design | `extern "C" fn` trampolines recovering a `&mut dyn Read + Seek` / `&mut dyn Write` from the `void*` param. Owned handle types (`FileAccess`, `FileWrite`) whose `Drop` runs after the PDFium object that references them; `FontRegistry` and `Session` hold them in `Vec`s. Web has no file path inputs and keeps returning `NotImplemented` for those kinds. |
| Rework | Low to medium. Four call sites. The retained-handle lifetimes need a test each. |
| Port order | G3, G4, G8 |

G4 moves engine-level font ownership before document mutators: the combined worker has one Rust
`FontRegistry` owning the key-to-ID map, fallback order and retained file handles, selected before
startup registration. The TypeScript `FontRegistrar` facade delegates every operation, including
`idFor` for retained annotation writers; it owns no second map or handles. A file handle is released
only after PDFium stops using it. The unmigrated server keeps its TypeScript owner until Phase 5.
Font ownership is independent of document backend selection; full font rollback recreates the worker
and replays registrations from host-held bytes or file paths and fallback order by stable key,
never by numeric ID. G4 verifies rendering on both paths, TypeScript FreeText authoring,
clear/re-register, fallback order and worker-recreation rollback (governing plan Section 6).

### 3.4 Form model cache: a cached native handle keyed on mutation sequence

| Field | Value |
|---|---|
| TypeScript | `services/src/features/forms/internal/formModelCache.ts` (57): module-level `WeakMap<DocumentSession, { seq, ptr }>` at line 29; consumers `readFormSnapshot.ts`, `features/annotations/internal/read/joinWidgetField.ts:23`, `FormMutator.ts`, `FormsEffectsApplier.ts` |
| Why it does not port | One `EPDFForm_LoadModel` pointer per session, rebuilt when `session.mutationSeq()` moves, held across calls, closed by `disposeFormModel` at session close. Under Stage A, Rust may not create or retain it (TypeScript lends the pointer per call). Under Stage B, Rust owns it and its invalidation is tied to `finalize_mutation`. The same read code must work in both modes. |
| Rust design | `trait FormModelSource { fn model(&self) -> &FormModel }` with two implementations: `Borrowed(*mut)` for Stage A and `Owned { ptr, seq }` inside the session for Stage B. Annotation reads take the trait, not the pointer. This pattern is not specific to the form model: it is the general answer to "the same read code must work in both modes", and `multiplatform-port-order.md` G1 hoists it into `DocumentAccess` / `PageAccess` so that every Stage A reader is written once rather than once per stage. |
| Rework | Low code; a repeated-read benchmark against today's cached path is the gate, because a port that rebuilt the model per call would pass conformance and lose the operation. |
| Port order | G7, G12 |

### 3.5 Forms effects: input from a JavaScript sandbox

| Field | Value |
|---|---|
| TypeScript | `services/src/features/forms/FormsEffectsApplier.ts` (452); `ActionReadBudgetTracker` from `features/actions/ActionModelReader.ts` shared by reference across preflight and apply |
| Why it does not port | The applier consumes `FormEffect[]` produced by a client-side script run in `packages/core/js-sandbox`. The applier itself is data-in, data-out and ports. The pipeline that produces its input does not exist on mobile. The shared mutable budget object passed through two phases is an aliasing pattern the borrow checker rejects. |
| Rust design | `apply_effects(&mut Session, &[FormEffect], budget: &mut ActionBudget)` with the budget threaded explicitly. Mobile reports form JavaScript as an unsupported capability until an `rquickjs` sandbox and behavior contract exist. |
| Rework | Low for the applier. The capability gap is a product statement, not code. |
| Port order | G12 |

### 3.6 Event delivery on native

| Field | Value |
|---|---|
| TypeScript | `services/src/events/EventHub.ts` (99: `EventHub` and `SessionEventPublisher`); the `EventHub` class is constructed only in `engine/main/src/document/LocalDocumentHandle.ts:84`; the `SessionEventPublisher` it exports is threaded through 8 files under `engine/main/src/document/` |
| Why it does not port | Publication already happens in the web adapter after the service returns; services return results and never call listeners. That part is fine and matches global constraint 3 (no host callback from `&mut self`). Native has no `engine/main` adapter, so nothing produces or delivers events there today. |
| Rust design | Each mutation returns `(Result, Vec<DocumentEvent>)`. Web: `LocalDocumentHandle` keeps publishing through `SessionEventPublisher`. Native: the Rust session appends to a per-document queue drained on the owner thread; facades expose `AsyncSequence` / `Flow`. Publication authority (which events, in what order, exactly once) moves to Rust in Stage B; delivery stays per host. |
| Rework | Low on web, medium on native. The exactly-once property when the web queue discards a late response must be re-proven with Rust producing the events. |
| Port order | G10 |

### 3.7 Search cursor as `JSON.stringify` of state

| Field | Value |
|---|---|
| TypeScript | `services/src/features/search/internal/searchCursor.ts` (79): `JSON.stringify` at lines 29 and 40, `JSON.parse` at 51 |
| Why it does not port | The cursor is an opaque string handed to clients and passed back. Its bytes depend on JavaScript property order and number formatting. A Rust `serde_json` encoding of the same struct is not byte-identical, and a client may hold a cursor from one backend while the next request lands on the other during preview. |
| Rust design | Define the cursor as a versioned, field-ordered format (`v1:` prefix plus canonical JSON with sorted keys, or a compact binary base64). Both backends accept both encodings during overlap. |
| Rework | Low. Contract plus a cross-backend decode test. |
| Port order | G5 |

## 4. Contract only

| Module | Concern | Rust handling |
|---|---|---|
| `SecurityReader.ts:32,106,151`, `writeAnnotationBase.ts:122`, `PieceInfoAccessor.ts:107`, `applyMetadataPatch.ts:81,85`, `shared/uuid.ts:19` | `Date.now()`, `new Date()` and random ID generation make results nondeterministic, which the cross-language corpus cannot freeze. The ID source is `globalThis.crypto.getRandomValues`, not `crypto.randomUUID()`: `shared/uuid.ts:7-9` records that `randomUUID` was deliberately rejected because it is gated to secure contexts, and it appears nowhere in `engine/services/src` or `engine/core/src`. | `Clock` and `IdSource` traits injected into the session; corpus runners supply fixed values |
| `EngineError` with `details: unknown`; `serializeError` | Arbitrary detail objects cross the wire | `details: serde_json::Value` for parity now; typed detail enum per code later. Codes stay byte-identical to `EngineErrorCode.ts`. |
| `DocumentSaver.ts` (`EPDF_SaveDocumentToOwnedBuffer`, layer artifacts) | Returned buffers are `ArrayBuffer` transferred to the host; ownership and release are implicit | Buffer contract from the plan (layout, allocator, release op, lifetime); explicit release call from the host adapter |
| `core/src/text/layout.ts` (548), `core/src/geometry/convert.ts` | Pure, port directly, but web selection plugins keep calling the TypeScript copy | Deliberate duplicate with shared vectors and a named owner; web switches only on a measured win |
| `core/src/auth/scope/*` (904) | Pure string parsing and resolution; web enforcement stays in `engine/main` (`LocalPageTextService.ts:48`) | Rust port owns native enforcement; shared vectors keep the two resolvers in agreement until Stage C |
| `PieceInfoAccessor.ts` `{ type: 'unknown' }` tagged results | Tagged union over arbitrary PDF objects | Plain `enum PieceInfoValue`; no design issue |
| `ActionModelReader.ts` (414) recursive action trees with a read budget | Recursion plus a mutable budget | `&mut ActionBudget` parameter; recursion depth bound made explicit |
| `PdfDocumentOpener.ts:108` `open.layerFileBase` | `runtime.fileAccess.fromNodeFile(path)` | Native and Node: `std::fs` behind the 3.3 handle type; web: unsupported as today |
| `PagesInserter.ts:77` `session.retainUntilClose(closure)` | Imported source document kept alive by a closure on the session until close | `Vec<RetainedSource>` of owned `Document` values in the session; `Drop` order after the destination's pages |

## 5. Not hazards

Scanned by structure and by grep for `async`, `Promise`, `Map`, `Set`, `WeakMap`, `RegExp`,
`JSON`, `Date`, `Object.keys`, `structuredClone`, `typeof`, `instanceof`, `unknown`, `any`, `Symbol`:
`PagesMutator.ts`, `PagesInserter.ts`, `PagesFlattener.ts`, `PagesExtractor.ts`,
`RedactionApplier.ts`, `AttachmentMutator.ts`, `MetadataMutator.ts`, `computeMutationImpact.ts`,
`readLinkAnnotation.ts`, `writeStampAnnotation.ts`, `annotationWriterRegistry.ts`. Hits were
`new Set` / `new Map` for page bookkeeping and `Record<string, number>` lookup tables. These are
procedural PDFium call sequences over `(fn, mem, ptr)` and translate line for line after the memory
layer (2.1) is replaced. `PageGeometryReader.ts`, `PageTextReader.ts`, `PageRenderReader.ts`,
`deviceRaster.ts`, `PagesReader.ts`, `MetadataReader.ts`, `readDestination.ts`, the other
`read*Annotation.ts` / `write*Annotation.ts` bodies, `core/src/text/charmap.ts`,
`core/src/search/{epoch,rects,snippet,literal,matcher}.ts`, `core/src/dto/*`, `core/src/identity/*`,
`core/src/revision/*`, `core/src/mutation/*` were read at header or cited-line level only; nothing
seen there suggests a hazard, but they have not been scanned to the same standard.
