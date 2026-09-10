# TypeScript designs that do not port to Rust as written

Scope: modules in `packages/engine/services` and `packages/engine/core` whose
current design relies on JavaScript runtime features, host objects, or
TypeScript type-system idioms with no direct Rust equivalent. Each entry names
the TypeScript design, why a line-by-line translation fails, the Rust design
that replaces it, and the size of the rework. Omission is not evidence of a
mechanical port; every group still needs its conformance and ownership review.

Paths beginning `services/` or `core/` are relative to `packages/engine/`; bare
filenames refer to the module named in that entry. Counts describe non-test
TypeScript source in the reviewed checkout, not estimated Rust effort. Proposed
Rust names are design sketches, not existing APIs or compile-tested signatures.
Official stack references are in the governing plan’s Section 15 and linked at
the relevant design constraints below.

## 1. Severity legend

| Level    | Meaning                                                                                             |
| -------- | --------------------------------------------------------------------------------------------------- |
| Redesign | The structure changes. Callers change. Needs a design note before code.                             |
| Rewrite  | Same structure, different mechanism. Every call site changes but mechanically.                      |
| Contract | Code ports directly; the observable behavior needs a written contract because the platforms differ. |

## 2. Redesign

### 2.1 WASM memory access layer

**TypeScript:** `services/src/runtime/memory/{bits,scratch,strings,structs}.ts`
(193 lines); `PdfRuntimeModule`, `PdfFunctions`, `PdfRuntimeMemory` from
`packages/engine/runtime`

**Pervasiveness:** 102 of 145 non-test service files import
`@embedpdf/engine-runtime`. Feature code repeatedly uses `mem` allocation,
string/struct access and `withScratch` / `withScratchN`; constructors generally
receive a runtime and session, then obtain `fn`, `mem` and handles internally.

**Why it does not port:** The entire service layer is written as a JavaScript
client of WASM linear memory: manual `alloc` / `free`, hand-pinned struct
offsets (`EPDF_CHAR_GEOMETRY_LAYOUT` at `structs.ts:17`, 124 bytes with 6
offsets), UTF-16 string marshalling through scratch buffers. In Rust the PDFium
call is a direct FFI call and the struct is a `bindgen` type. There is nothing
to translate; the layer disappears.

**Rust design:** `crates/pdfium-sys` (`bindgen`) plus `crates/pdfium` RAII
types. Out-parameters become stack locals. Strings go through `widestring` /
`Vec<u16>`.

**Rework:** Every function signature in `features/` changes shape. The logic
inside survives; the plumbing around each PDFium call is rewritten. This is the
single largest source of diff volume in the port.

**Port order:** G0 (`multiplatform-port-order.md`)

**Retirement:** The Rust layer exists from G0, but the TypeScript one is not
deleted then. Stage A keeps TypeScript as session owner and every unflipped
service still reaches PDFium through it, so porting a family only ends its use
on the Rust path. `runtime/memory/`, generated `PdfFunctions`, and
`PdfRuntimeMemory` stay maintained through Phase 5 for the TypeScript server and
rollback backends. Final removal in Phase 6 requires passing the server
migration and retiring the TypeScript fallback.

### 2.2 Document session, page pool, revisions

**TypeScript:** `services/src/document-session/DocumentSession.ts` (354);
`pages/PagePtrPool.ts` (75); `revisions/RevisionAuthority.ts` (103);
`lifecycle/{PdfDocumentOpener,BaseDocumentRegistry}.ts` (285)

**Why it does not port:** Nullable fields mutated in place
(`docPtr: Ptr | null`, `closeDocument: (() => void) | null`,
`pages: PagePtrPool | null` at `DocumentSession.ts:35-48`). Page lifetime is a
`Map<PageObjectNumber, { ptr, refs }>` with manual `acquire` / `release` in
`finally` blocks and an `isHeld` assertion that structural mutations call to
detect leaked acquires. `BaseDocumentRegistry` refcounts base documents across
layer sessions by closure. Rust lifetimes can protect safe wrapper references;
they do not prevent a copied raw pointer from outliving a guard. The FFI
boundary must prevent dereferencing that pointer after release.

**Rust design:** `Session` as an owned state machine
(`Opening | Open(OpenState) | Closing | Closed`), not nullable fields. Within
one Rust call, a scoped `PageLease<'s>` can borrow page state and release
through `Drop`; structural mutation requires exclusive access. Retained pages in
the session cannot simply borrow another field of that same movable session: use
audited owner-managed handles and an explicit drop order, as in the prototype
wrappers. Stage B host leases and continuations need a runtime lease table; keep
structural-mutation checks for them. A keyed base-document registry holds
`Weak<BaseDocument>` entries and hands sessions `Rc` ownership, preserving
lookup/reuse without keeping closed bases alive. Checkout, return and finalize
remain separate synchronous bridge entry points; block conflicting operations
through finalization and defer close until it completes.

**Rework:** High. Every feature module's entry point changes from
`(runtime, session)` constructor injection to a `&Session` or `&mut Session`
parameter with lease scopes. The Stage B facade on the TypeScript side is new
code.

**Port order:** G3, G10

### 2.3 Annotation DTO layering: handwritten schemas and registries

**TypeScript:** `core/src/annotation/kinds/*` (19 subtype directories, each
`dto`, `schema`, `patch`, `draft`, some `normalize`);
`core/src/annotation/{base.schema,subtype,registry}.ts`;
`services/src/features/annotations/internal/read/annotationReaderRegistry.ts`
(86) and `internal/write/annotationWriterRegistry.ts` (250); 28 files in
`core/src` outside `wire/` use `z.object` / `z.infer` / `z.discriminatedUnion`

**Why it does not port:** Not because of zod direction: outside `wire/`,
`z.infer` appears zero times. `kinds/highlight/dto.ts` hand-declares
`HighlightAnnotationDTO` and `kinds/highlight/schema.ts:14` types the schema
against it as `z.ZodType<HighlightAnnotationDTO>`, so this layer is already
type-first and inverting it is mostly codegen. What does not port is the
dispatch and the three-state encoding. Dispatch is
`Partial<Record<AnnotationSubtype, AnnotationSubtypeReader>>`; each reader
returns the broad `AnnotationDTO` union, so the map type does not enforce
key-to-return-variant agreement. Patches use `?: T | null` for three-state
fields (`patch-base.ts:7-9`, `MetadataPatch.ts:7-9`): absent leaves, `null`
clears, value sets. `serde` `Option<T>` collapses absent and `null` into `None`
by default. The `unsupported` fallback preserves an unknown raw subtype code
without a wire-format bump.

**Rust design:** `enum AnnotationDTO` with internally tagged Serde variants. Add
explicit wire names for variants (`free-text`, `file-attachment`, etc.) and
fields (`rawSubtypeCode`); a plain derive with Rust names does not preserve the
protocol. Readers and writers become `match` arms; the registry indirection is
deleted. Three-state fields use an explicit
`enum Patch<T> { Keep, Clear, Set(T) }` with custom `Serialize` / `Deserialize`
and `#[serde(default, skip_serializing_if = "Patch::is_keep")]`; `ts-rs` needs
explicit field/type mapping to emit `?: T | null`; it does not infer a custom
serializer’s wire shape. Prove the mapping with the pinned version and a
generated-output fixture. Exported Zod schemas (including shared shapes, form,
geometry and action schemas) stay handwritten and are tested against generated
output in both directions (omitted vs null, unknown enums, and each schema’s
strip/passthrough/strict behavior). That test covers `wire/schemas.ts` and,
equally, the 20 handwritten schema files under `annotation/` (`base.schema.ts`
plus `kinds/*/schema.ts`, 1,070 lines). Type annotations provide partial
schema/type checking, but casts such as `kinds/highlight/schema.ts` bypass it.
Generated types retain that partial checking; runtime parity tests are still
necessary for rejection and normalization behavior. Direction inverts: Rust
canonical, TypeScript generated.

**Rework:** Medium by breadth, not depth: 19 subtype directories including
`unsupported`; the 20-name catalog also includes `popup`, which has no dedicated
kind directory. The behavior/type inventory is 1,517 lines of `kinds/` plus 997
top-level lines. Lower than a true schema-first inversion would be, because the
types are already handwritten. The three-state serializer and the schema parity
test are the design work. Phase 0 proves it on `MetadataPatch` before the
annotation kinds are touched.

**Port order:** G1 (patch type), G7, G11

### 2.4 `WorkerHost`: transport fused with engine behavior

**TypeScript:** `services/src/worker-host/WorkerHost.ts` (1,459 lines, 64 `case`
arms)

**Why it does not port:** One class holds the session map,
`BaseDocumentRegistry`, per-kind request decoding, `finishMutation` (lines 1380
to 1404: mutation-sequence bump with exemptions for `forms.*`, `pages.flatten`,
`redaction.apply`) plus `saveLayerArtifact` (lines 1405 to 1414), and
`postMessage` plumbing. Native needs a scheduler and dispatch but not the
JavaScript worker protocol. The two methods are about 35 lines; session lookup,
open/password state, artifact persistence and font ownership also require
explicit owners. The source split cannot be decided from those 35 lines alone.

**Rust design:**
`epdf-engine::session::finalize_mutation(&mut Session, MutationOutcome) -> Finalized`
with the sequence accounting expressed in the mutation outcome
(`sequence_already_bumped`, changed/indeterminate state, revision impact),
rather than a string-prefix test. Preserve bumps required before post-write
readback; finalization must not delay those or bump twice. `crates/epdf-wasm`
decodes `WorkerRequest` into typed calls. TypeScript `WorkerHost` shrinks to
transport.

**Rework:** Medium. Split, not port. Exactly-once test for the sequence bump on
each host.

**Port order:** G10

## 3. Rewrite

### 3.1 Search dialect and text folding

**TypeScript:** `core/src/search/regex.ts` (158): `new RegExp(pattern, 'mu')` at
line 93 validates, line 142 executes with `gmu` / `gimu`.
`core/src/search/fold.ts` (114): `cp.normalize('NFKD')` at line 62, `/\p{M}/gu`
at line 49, upper-then-lower case round trip.

**Why it does not port:** The validator accepts JavaScript `u`-mode syntax
except backreferences and lookaround. Its comment says the server uses RE2, but
the current shared `SearchReader` calls `matchRegex`, which constructs
JavaScript `RegExp`; `cloudpdf/server/src/runtime/worker-entry.ts` uses that
shared host. There is no RE2 implementation/dependency in the reviewed CloudPDF
tree. Rust `regex` has different syntax and Unicode defaults. JavaScript `\d` is
ASCII; `\w` and `\b` also admit characters that fold to ASCII word characters
under `iu` (for example K and ſ). Simply rewriting them to ASCII is incorrect.

**Rust design:** Phase 0 compares `regress` (ECMAScript-oriented, backtracking)
with `regex` plus a syntax-aware compatibility layer. Neither is selected yet.
Test `\s`, Unicode properties, dot/line terminators, `m` anchors, `iu` folding,
empty matches, whole-word post-filtering, invalid syntax and UTF-16 offsets.
Preserve raw UTF-16 where needed: Rust UTF-8 byte offsets and strings cannot
silently stand in for JavaScript code units or lone surrogates. Pin
normalization/case tables and include their version in the search epoch. Bound
pattern size, input size, output and execution; rejecting
lookaround/backreferences alone does not prevent expensive backtracking. A
timeout queued on the blocked owner thread cannot interrupt it.

**Rework:** Medium code, high contract risk. The entry gate is the dialect
vector file generated by running `RegExp` and `foldText`, covering accepted and
rejected syntax, Unicode classes, case folding, empty matches, and offset
mapping back to UTF-16.

**Port order:** G5

### 3.2 Cancellation via `AbortSignal`

**TypeScript:** `services/src/shared/abort.ts` (14); 86 `throwIfAborted(signal)`
call sites across `features/`; `core/src/promise/AbortablePromise.ts` (148)

**Why it does not port:** `AbortSignal` is a host object. Many feature entry
points take one and use `throwIfAborted`; helpers and some operations do not. On
the web worker an abort message is not observed while a synchronous handler runs
(`WorkerHost.ts:199`, `:225`), so today's "cooperative" checks only fire between
message-loop turns for chunked work, and never inside one WASM call.

**Rust design:** A clonable `CancelToken(Arc<AtomicBool>)` with checks at
corresponding pre-apply boundaries; never copy a post-write abort check into a
path that must finish bookkeeping. Native: the owner thread's job carries the
token and the facade's `Task` / `Job` cancellation sets it. Web Stage A: token
is set only between jobs (cancellation of waiting). Web Stage B: yield between
bounded chunks so abort messages can run. A separately designed shared-memory
build could poll an atomic flag, but COOP/COEP alone does not make the existing
unshared WASM memory writable by another thread. It requires shared
`WebAssembly.Memory`, compatible atomics/toolchain settings and a separate
fallback artifact; defer it until justified. Never promise interruption inside
an uninstrumented PDFium call. `AbortablePromise` stays in TypeScript unchanged.

**Rework:** Mechanical at call sites. The work is the per-operation contract
table (four cases in the plan, Section 7) and the tests for queued, pre-apply,
post-write, and late-result cancellation.

**Port order:** G1

### 3.3 File access and file write handles

**TypeScript:** `PdfRuntimeModule.fileAccess.fromNodeFile(path)` and
`fileWrite.toNodeFile(path)`
(`packages/engine/runtime/src/core/pdf-runtime-module.ts:46,58,66,67`),
implemented in `runtime/src/wasm/wasm-runtime.ts` and
`runtime/src/native/native-runtime.ts`. Callers: `FontRegistrar.ts:108-111`
(`EPDFFont_RegisterFont`, handles retained in `fileHandles` at line 51 until
`clear`), `attachmentPrimitives.ts:193-197` (`EPDFAttachment_ExtractFile`,
server mode), `PdfDocumentOpener.ts:108` (`open.layerFileBase`),
`BaseDocumentRegistry.acquireFileBase`.

**Why it does not port:** Services never touch `FPDF_FILEACCESS` /
`FPDF_FILEWRITE` directly; the runtime package builds those structs and their
function-table entries. The legacy runtime plumbing is removed with the memory
layer only after the server migration passes and the TypeScript fallback is
retired (2.1); the loader stays. Rust needs its own mechanism before that
removal. Two lifetime rules ride on it: a font registered from a path keeps its
handle alive until the registry is cleared (PDFium range-reads lazily), and a
file-backed base document keeps its handle for the session.

**Rust design:** `extern "C"` trampolines recover a stable, boxed concrete
context from the C user pointer. That context may contain `Box<dyn ReadSeek>`
where `trait ReadSeek: Read + Seek`, or `Box<dyn Write>`; a trait-object pointer
carries a vtable and cannot be recovered by casting a thin `void*` directly.
Match the generated C signatures, validate offsets/lengths, translate I/O errors
to C failure codes and contain Rust unwinding before returning across C. Owned
handle types (`FileAccess`, `FileWrite`) whose `Drop` runs after the PDFium
object that references them; `FontRegistry` and `Session` hold them in `Vec`s.
Web has no file path inputs and keeps returning `NotImplemented` for those
kinds.

**Rework:** Medium. Cover every file-access/file-write consumer, including
`DocumentSaver` and worker file-render sinks. The retained-handle lifetimes need
a test each.

**Port order:** G3, G4, G8

G4 moves engine-level font ownership before document mutators: the combined
worker has one Rust `FontRegistry` owning the key-to-ID map, fallback order and
retained file handles, selected before startup registration. The TypeScript
`FontRegistrar` facade delegates every operation, including `idFor` for retained
annotation writers; it owns no second map or handles. A file handle is released
only after PDFium stops using it. The unmigrated server keeps its TypeScript
owner until Phase 5. Font ownership is independent of document backend
selection; full font rollback recreates the worker and replays registrations
from host-held bytes or file paths and fallback order by stable key, never by
numeric ID. G4 verifies rendering on both paths, TypeScript FreeText authoring,
clear/re-register, fallback order and worker-recreation rollback (governing plan
Section 6).

### 3.4 Form model cache: a cached native handle keyed on mutation sequence

**TypeScript:** `services/src/features/forms/internal/formModelCache.ts` (57):
module-level `WeakMap<DocumentSession, { seq, ptr }>` at line 29; consumers
`readFormSnapshot.ts`,
`features/annotations/internal/read/joinWidgetField.ts:23`, `FormMutator.ts`,
`FormsEffectsApplier.ts`

**Why it does not port:** One `EPDFForm_LoadModel` pointer per session, rebuilt
when `session.mutationSeq()` moves, held across calls, closed by
`disposeFormModel` at session close. Under Stage A, Rust may not create or
retain it (TypeScript lends the pointer per call). Under Stage B, Rust owns it
and invalidates it when the mutation sequence changes, including before
post-write readback; finalization must not duplicate a bump. The same read code
must work in both modes.

**Rust design:** Use a scoped `FormModelView` for the read itself. Stage A
constructs it from the model pointer lent by TypeScript; Stage B prepares or
refreshes the owned cache before lending a view. Cache acquisition is fallible
and may mutate the cache, so an infallible `model(&self) -> &FormModel` sketch
would omit required behavior. Annotation reads consume that scoped view through
the shared access abstraction. This pattern is not specific to the form model:
it is the general answer to "the same read code must work in both modes", and
`multiplatform-port-order.md` G1 hoists it into `DocumentAccess` / `PageAccess`
so that every Stage A reader is written once rather than once per stage.

**Rework:** Low code; a repeated-read benchmark against today's cached path is
the gate, because a port that rebuilt the model per call would pass conformance
and lose the operation.

**Port order:** G7, G12

### 3.5 Forms effects: input from a JavaScript sandbox

**TypeScript:** `services/src/features/forms/FormsEffectsApplier.ts` (452);
`ActionReadBudgetTracker` from `features/actions/ActionModelReader.ts` shared by
reference across preflight and apply

**Why it does not port:** The applier consumes `FormEffect[]` produced by a
client-side script run in `packages/core/js-sandbox`. The applier itself is
data-in, data-out and ports. The pipeline that produces its input does not exist
on mobile. The shared budget needs an explicit mutable owner; sequential `&mut`
reborrows across preflight and apply are valid Rust. Only overlapping
incompatible borrows are rejected.

**Rust design:**
`apply_effects(&mut Session, &[FormEffect], budget: &mut ActionBudget)` with the
budget threaded explicitly. Mobile reports form JavaScript as an unsupported
capability until a separately validated sandbox and behavior contract exist;
`rquickjs` is only a candidate.

**Rework:** Low for the applier. The capability gap is a product statement, not
code.

**Port order:** G12

### 3.6 Event delivery on native

**TypeScript:** `services/src/events/EventHub.ts` (99: `EventHub` and
`SessionEventPublisher`); the `EventHub` class is constructed only in
`engine/main/src/document/LocalDocumentHandle.ts:84`; the
`SessionEventPublisher` it exports is threaded through 8 files under
`engine/main/src/document/`

**Why it does not port:** Publication already happens in the web adapter after
the service returns; feature methods return results; the client adapter
publishes mutation events. That part is fine and matches global constraint 3 (no
host callback from `&mut self`). Native has no `engine/main` adapter, so nothing
produces or delivers events there today.

**Rust design:** Finalization produces the mutation outcome and ordered events,
including partial failure. Web: keep `SessionEventPublisher` delivery, but add
an acknowledged event stream independent of request-promise completion; request
IDs and event sequence numbers prevent duplicate publication. Native: the owner
enqueues events and facades deliver them away from the PDFium thread as
`AsyncSequence` / `Flow`. Define bounded retention, subscriber lag/resync and
close behavior. Exactly-once applies within a live session; process failure
needs an explicit recovery/resync contract.

**Rework:** Low on web, medium on native. The exactly-once property when the web
queue discards a late response must be re-proven with Rust producing the events.

**Port order:** G10

### 3.7 Search cursor as `JSON.stringify` of state

**TypeScript:** `services/src/features/search/internal/searchCursor.ts` (79):
`JSON.stringify` at lines 29 and 40, `JSON.parse` at 51

**Why it does not port:** The cursor is an opaque string handed to clients and
passed back. The outer JSON object is decoded by field name, so property order
need not be byte-identical. The embedded `key` string from `searchQueryKey` does
need a compatible representation. Backend selection is fixed per session:
preview does not route successive requests to different engines. A
saved/reopened document must restart its search, not inherit the old cursor.

**Rust design:** Preserve the existing `v: 1` JSON fields and `searchQueryKey`
semantics first; test cross-decoding with identical controlled query/version
state. Validate finite integer positions and bounds. Change versions only for a
required semantic change, with an explicit overlap decoder. Preserve the
server’s separate authenticated cursor format; it is not the local cursor
contract.

**Rework:** Low. Contract plus a cross-backend decode test.

**Port order:** G5

## 4. Contract only

| Module                                                                                                                                       | Concern                                                                                                                                                                                                                                                                                                                                                                                                   | Rust handling                                                                                                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SecurityReader.ts:32,106,151`, `writeAnnotationBase.ts:122`, `PieceInfoAccessor.ts:107`, `applyMetadataPatch.ts:81,85`, `shared/uuid.ts:19` | `Date.now()`, `new Date()` and random ID generation make results nondeterministic, which the cross-language corpus cannot freeze. The ID source is `globalThis.crypto.getRandomValues`, not `crypto.randomUUID()`: `shared/uuid.ts:7-9` records that `randomUUID` was deliberately rejected because it is gated to secure contexts, and it appears nowhere in `engine/services/src` or `engine/core/src`. | `Clock` and `IdSource` traits injected into the session; corpus runners supply fixed values                                                                                                                                                                                                                                 |
| `EngineError` with `details?: Record<string, unknown>`; `serializeError`                                                                     | Arbitrary detail objects cross the wire                                                                                                                                                                                                                                                                                                                                                                   | Inventory emitted detail shapes first. An optional JSON object can use Serde values, but arbitrary JavaScript `unknown` values are not automatically JSON-compatible. Preserve existing emitted shapes through adapters; specify normalization/rejection for non-JSON values. Codes stay identical to `EngineErrorCode.ts`. |
| `DocumentSaver.ts` (`EPDF_SaveDocumentToOwnedBuffer`, layer artifacts)                                                                       | Returned buffers are `ArrayBuffer` transferred to the host; ownership and release are implicit                                                                                                                                                                                                                                                                                                            | Buffer contract from the plan (layout, allocator, release op, lifetime); explicit release call from the host adapter                                                                                                                                                                                                        |
| `core/src/text/layout.ts` (548), `core/src/geometry/convert.ts`                                                                              | Pure, port directly, but web selection plugins keep calling the TypeScript copy                                                                                                                                                                                                                                                                                                                           | Deliberate duplicate with shared vectors and a named owner; web switches only on a measured win                                                                                                                                                                                                                             |
| `core/src/auth/scope/*` (904)                                                                                                                | Pure string parsing and resolution; web enforcement stays in `engine/main` (`LocalPageTextService.ts:48`)                                                                                                                                                                                                                                                                                                 | Rust port owns native enforcement; shared vectors keep the two resolvers in agreement for as long as web/server enforcement retains the TypeScript resolver                                                                                                                                                                 |
| `PieceInfoAccessor.ts` `{ type: 'unknown' }` tagged results                                                                                  | Tagged union over arbitrary PDF objects                                                                                                                                                                                                                                                                                                                                                                   | Plain `enum PieceInfoValue`; no design issue                                                                                                                                                                                                                                                                                |
| `ActionModelReader.ts` (414) recursive action trees with a read budget                                                                       | Recursion plus a mutable budget                                                                                                                                                                                                                                                                                                                                                                           | `&mut ActionBudget` parameter; recursion depth bound made explicit                                                                                                                                                                                                                                                          |
| `PdfDocumentOpener.ts:108` `open.layerFileBase`                                                                                              | `runtime.fileAccess.fromNodeFile(path)`                                                                                                                                                                                                                                                                                                                                                                   | Native and Node: `std::fs` behind the 3.3 handle type; web: unsupported as today                                                                                                                                                                                                                                            |
| `PagesInserter.ts:77` `session.retainUntilClose(closure)`                                                                                    | Imported source document kept alive by a closure on the session until close                                                                                                                                                                                                                                                                                                                               | `Vec<RetainedSource>` of owned `Document` values in the session; `Drop` order after the destination's pages                                                                                                                                                                                                                 |

## 5. Remaining modules still need review

The procedural readers and mutators are candidates for preserving their
algorithmic structure, not for literal line-by-line translation.
`PagesInserter`, `PagesFlattener`, `RedactionApplier`, annotation writers and
`computeMutationImpact` still depend on close order, post-write failure
handling, page identity and cache/revision rules. Those are covered by the
relevant group gates.

Pure geometry, text layout, DTO, identity and revision code also needs
numeric-width, UTF-16, optional-field and serialization checks. Passing the
original TypeScript suites is necessary; differential tests must independently
verify outputs where a self-consistent implementation could satisfy a property
suite with the wrong answer.
