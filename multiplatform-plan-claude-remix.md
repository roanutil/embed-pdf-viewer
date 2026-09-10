# Multiplatform EmbedPDF: Rust core, native wrappers

Status: proposal. Date: 2026-09-09. This document governs. Companions: `multiplatform-port-order.md`
(module groups and sequence) and `multiplatform-port-hazards.md` (TypeScript designs that need
redesign in Rust). It supersedes `multiplatform-plan-claude.md` and `multiplatform-plan-codex.md`,
which are kept for the detail they carry but are not authoritative. Where any of them disagree with
this document on phasing, counts or sequence, this one is correct.

## 1. Goal

One Rust implementation of the PDF engine, consumed by:

| Platform | Delivery | Consumer of the Rust core |
|---|---|---|
| Web | Existing WASM worker, one Emscripten module linking Rust + PDFium | TypeScript kernel, plugins, framework adapters, viewer (unchanged) |
| iOS | Swift Package with self-contained XCFramework | Handwritten Swift facade over `uniffi` |
| Android | Maven AAR with per-ABI `.so` | Handwritten Kotlin facade over `uniffi` |
| Node / server | `napi-rs` addon in the existing `worker_thread` | `@cloudpdf/server`, `@cloudpdf/engine` |

Two deliverables, roughly equal in size:

1. The engine port: 25,431 hand-ported lines. `packages/engine/services` (16,620) plus 8,811 lines
   of `packages/engine/core` (text layout, search fold and cursor, DTOs, annotation kinds and
   helpers, error codes, scope resolution). A further 1,765 lines of core become generated schemas
   or mechanical barrels. `multiplatform-port-order.md` section 2 itemizes both.
2. Native reader products on iOS and Android. Presentation code (tile scheduling, selection,
   gestures, search navigation, virtualization) has no JavaScript runtime to run in and is written
   twice natively.

The reason to do this is code sharing, not web speed. Web gains in geometry and search loops and is
expected to be neutral elsewhere. Without the port, iOS and Android need a second implementation of
25,431 lines of PDF-spec correctness that will drift.

## 2. Decision summary

| Decision | Choice |
|---|---|
| Boundary | Rust owns PDFium's C ABI up through the 61 document operations in `packages/engine/core/src/wire/worker-protocol.ts` (`WorkerRequest` is a 63-member union; `OpenWorkerRequest` expands to three `open.*` kinds, giving 65 request kinds, of which 4 are control frames). TypeScript keeps everything above. |
| Web kernel, plugins, viewer | Stay TypeScript. `packages/core/main`, `packages/core/stage`, `packages/core/annotation` carry the strongest presumption against porting. |
| Native API | `uniffi` is the ABI, not the API. Each platform gets a handwritten facade. |
| Migration unit | Resource ownership, not operations. Three stages (Section 6). Backend selected per document at open through `PdfOpenOptions.engineBackend: 'typescript' \| 'rust'`, defaulting to `'typescript'` for the whole preview, never switched mid-session. |
| DTOs | Rust definition is canonical for internal wire DTOs; generated TypeScript is checked in; dirty diff fails CI. Public web DTOs that differ keep an adapter. |
| Errors | One vocabulary, `EngineErrorCode`, on every platform. |
| Target version | v4. Latest stable is 2.15.0; current line is `3.0.0-next.11`. Rust backend ships opt-in during preview. |
| Repo layout | Cargo workspace at repo root (`Cargo.toml`, `crates/`); native SDK sources in `platforms/apple/`, `platforms/android/`. Neither is in the npm namespace. |

Toolchain pins: `rustc` 1.98.0 (`edition = "2024"`), `bindgen = "=0.72.1"`, `uniffi = "=0.32.0"`,
`napi = "=3.12.2"`, `ts-rs`, `cargo-ndk` 4.1.2, emsdk 3.1.72 with Binaryen 132 via
`EM_BINARYEN_ROOT`, Swift 6.3.3 / Xcode 26.6, Kotlin 2.2.20 / AGP 9.4.0 / NDK 29.0.14206865,
Node 24.20.0, pnpm 10.34.0.

Two of those pins do not match what the repo builds and tests on today, and Phase 0 has to close
both. The shipping web runtime builds under emsdk 3.1.70 (`packages/engine/runtime/Dockerfile:2`),
not 3.1.72, and Phase 0's exit gate is a clean-checkout build of the shipping worker. Six CI
workflows run Node 22, including `.github/workflows/engine-conformance.yml:53`, the exact file
Phase 0 wires the corpus job into; `mise.toml` and the benchmark environment are on 24.20.0.
Unresolved: whether `-fwasm-exceptions` works against the rustup-shipped `wasm32-unknown-emscripten`
std on stable `rustc` 1.98.0, or whether it needs `-Zbuild-std` on nightly. The answer changes the
toolchain pin and it is a Phase 0 question, not a Phase 1 one.

## 3. Evidence

Numbers come from the committed tables only: `pdfium-binding-benchmarks/RESULTS.md` and
`viewer-rust-port-benchmarks/bench/RESULTS.md`, except for four figures that exist only in
`docs/research/` and are cited as such: the three measurement bugs and the 184x `rows.find`
(`pdfium-rust-wrapper-benchmarks.md:321`, `rust-core-port-benchmarks.md:247-248`) and the exception
handling counts and browser passes (`platform-gap-audit.md:65-70`). The narrative summaries in
`docs/research/` are superseded; those tables are not.

| Measurement | Result | Rule it produces |
|---|---|---|
| WASM geometry, per-item vs coarse (`RESULTS.md:169`) | 2.788 ms vs 1.070 ms (2.6x). Rust vs C++ owning the loop: `d/c` 0.962 to 1.048. | Put Rust where the compute behind one crossing is large. Language is irrelevant; crossing count is everything. |
| WASM search, per-item vs coarse | 102.36 µs vs 30.61 µs (3.3x) | Same. |
| WASM `text`, all four arms | Within 2% | `PageTextReader.ts:93` is already coarse. No boundary win to gate on. |
| WASM `render-1x`, `render-4x` | Dominated by PDFium bitmap work | Coarsening buys nothing on render. Do not promise faster rasterization. |
| Thin Rust frame over PDFium in WASM | +1 to 4% time, +3,802 bytes on 4,676,714 | `pdfium-sys` is not an optimization target. |
| Native geometry, arm b vs c (`RESULTS.md:415`) | 4.134 ms vs 577.37 µs (7.16x), same `napi-rs` addon | Boundary win holds natively. |
| Native search (`RESULTS.md:541`) | 1.62x vs WASM's 3.3x | Win is workload-specific in both directions. |
| Native `text` and readback rows (`RESULTS.md:3`) | Predate the `bench/arm.mjs` readback-copy fix | Rerun before gating anything on them. `geometry` and `search` do not touch those paths and stand. |
| Viewer frame, 1,000 anchored shapes (`bench/RESULTS.md:26`) | TS 121.96 µs vs Rust 200.76 µs; Rust raw buffer 171.43 µs. Rust won 0 of 20 cells. | Cheap math behind one crossing loses. Keep web projection in TypeScript unless a real-feature measurement says otherwise. |
| Viewer, n≥10 | Rust-with-state 1.60x to 3.52x slower; n=1 cells 10.57x | Same. Toy is rectangles, not `cloudy.ts`; result bounds the boundary cost, not Rust on real annotation workloads. |
| Prototype pixel golden (`pdfium-multiplatform-prototype`) | 6 paths match at mean 0 / max 0, but page 4 is grayscale: a B-to-R swap also scores 0. Tolerances mean 2 / max 16 pass a uniform 6-level darkening (mean 1.74, max 6). | Color-channel tests on a colored fixture are mandatory. Grayscale goldens prove nothing about channel order. |
| Benchmark study's own bugs | 3 measurement bugs, each reversing a conclusion, none caught by the fairness gate; O(n²) `rows.find` invisible at demo size, 184x at 10,000 | Differential answer gate before timing; equal-work audit; rotated arm order; medians and p95; optimized TypeScript arm included. |

## 4. Global constraints

Every phase inherits these.

1. One owner per resource and per state transition. TypeScript and Rust never both hold authority
   over a document handle, page cache, revision counter, or mutation sequence.
2. The host-to-Rust boundary is per operation, never per item. No convenience API may reintroduce a
   per-glyph, per-annotation, or per-rect call.
3. No Rust export taking `&mut self` may call back into the host. Doing so produced
   `attempted to take ownership of Rust value while it was borrowed` in
   `viewer-rust-port-benchmarks/rust-geometry-and-state`.
4. Public bindings hand out opaque handles carrying an engine generation. A handle from a replaced
   engine fails rather than resolving against new state. Raw pointers never appear in a consumer API.
5. PDFium handles never cross threads. Native builds set `EMBEDPDF_TLS_GLOBALS=true` and each
   engine owns one dedicated OS thread. `wasm32` targets leave it off because each instance has its
   own linear memory. Contract: `.agents/skills/embedpdf-conventions/references/runtime-confinement.md`.
6. Two text index spaces, both carried. CHARACTER space is PDFium's character list (geometry,
   hit-testing, selection, `PageTextSnapshot.charCount`). TEXT space is UTF-16 code-unit offsets
   into the extracted string (search, slicing). `PageTextSnapshot.charMap` encodes the two
   divergences: non-printing characters contribute 0 units, supplementary-plane characters
   contribute 2. Every boundary DTO names its space. Rust ports `packages/engine/core/src/text/charmap.ts`
   including `charMapViolation`. Current fixtures cannot test this: `vectors/report-page4.json` has
   `charCount` 8,106 over 8,106 UTF-16 units and no `charMap`.
7. Rust is canonical for internal wire DTOs. The generated serialization layout is never a public
   API. The protocol carries a version so mismatched JS, WASM and native artifacts are detected.
8. Each phase gets its own checkbox plan under `docs/superpowers/plans/` before it starts. This
   document is the program.

## 5. Architecture

### 5.1 Layers

| Layer | Owns | Does not own |
|---|---|---|
| `crates/pdfium-sys` | `bindgen` 0.72.1 over the fork headers `packages/engine/runtime/build/generate-functions.mjs` already walks, filter `FPDF\|EPDF\|FORM\|PDFiumExt_` | Anything safe |
| `crates/pdfium` | RAII wrappers, checked lengths, unsafe boundaries (audited before promotion) | Document semantics |
| `crates/epdf-engine` | Document session (bytes, lifetime, page identity and leases, revisions, mutation sequence, password state, retained import sources), all Reader and Mutator families, text layout, search, DTOs, error codes, permission enforcement for native sessions, mutation finalization | `uniffi`, browser, UIKit, Android types |
| `crates/epdf-uniffi` | `uniffi::setup_scaffolding!()`, newtype + `From` per exported type | Building for `wasm32-unknown-emscripten` |
| `crates/epdf-wasm`, `crates/epdf-node` | Host adapters: worker protocol decode, buffer handoff, `napi-rs` | Semantics |
| `crates/build-support` | Shared `build.rs`. Exists because Cargo does not propagate `cargo:rustc-link-arg` to a dependent's test or bin targets | |
| TypeScript `packages/engine/main`, `worker-host` transport | Message loop, transport, capability guard on web, `LocalEngine`, service orchestration | Engine behavior |
| Swift / Kotlin facades | Platform types, coordinate conversion, image adaptation, scheduling, accessibility | Document semantics or shared business rules |

Dependency direction, unchanged from today: `runtime/shared → document-session → features →
worker-host`. Prototype `pdfium-sys` and safe wrapper are review inputs. Prototype operation
objects, dispatch and host adapters are scaffolding to redesign.

### 5.2 Runtime topology

| Target | Threading | Notes |
|---|---|---|
| Web worker | Single-threaded message loop; Rust + PDFium in one Emscripten module | Never split into two WASM heaps joined by per-call JavaScript. An abort message is not observed while a synchronous handler runs (`WorkerHost.ts:199`, `:225`). |
| Native | One owner OS thread per engine, inside Rust; handle types are `!Send + !Sync`; exported engine object is thread-safe and submits jobs | A Swift actor, coroutine dispatcher or mutex does not guarantee affinity. Implementation reference: prototype `crates/epdf-ops/src/dispatch/{threaded,inline}.rs`. |
| Node | `napi-rs` addon in the existing `worker_thread` | Eight native targets in `packages/engine/runtime/npm/` (nine directories, one of which is `wasm32`). Windows and musl deferred past the first mobile release. |

Multi-document operations (page import) coordinate on one owner: transfer bytes or reopen a
controlled snapshot on the destination. Never pass a source document handle across owners.

Thread confinement is not crash isolation. A native fault in a mobile process kills the app;
`catch_unwind` contains nothing. Server keeps `cloudpdf/server/src/runtime/`'s supervised host,
`QuarantiningEnginePool` and `EngineRecycler`. Mobile isolation is a separate investigation, not a
reader-release prerequisite.

### 5.3 Lifecycle contract (Phase 0 deliverable)

- Bounded queues for jobs, open documents, input bytes, raster and cache bytes, search output.
  Admission checked before large allocations. Reserved control route so saturation cannot block
  close or cancel.
- Close is idempotent, rejects new work, invalidates handles, drains or cancels per contract,
  releases on the owner thread. Closing an engine closes its documents.
- Engine replacement with generation IDs. No process-wide singleton without reset.
- Panic after a potentially mutating operation poisons the session unless an operation-specific
  invariant says otherwise. Pending requests reject on worker failure.
- Aggregate memory monitored at process and WASM level; PDFium allocations are invisible to Rust
  budgets. `WebAssembly.Memory` never shrinks: measure live allocator usage, not `byteLength`.
- Buffer contract: layout, version, alignment, stride, pixel format, alpha mode, color space,
  allocator, release op, lifetime. Lengths and multiplications validated before allocation. Heap
  views rebuilt after WASM memory growth.
- Coordinate contract: normalized page points, origin and axis direction, crop and rotation
  transforms, device scale, durable page identity (index is not identity).

## 6. Ownership migration

`DocumentSession` (`packages/engine/services/src/document-session/`, 832 lines) owns document bytes
and lifetime, page identity and leases, the revision counter, the mutation sequence, and password
state. Two languages cannot both own those.

| Stage | Owner of session | What moves | Bridge | Web cancellation |
|---|---|---|---|---|
| A: coarse reads over borrowed handles | TypeScript | Read operations only. Rust borrows a validated document or page pointer synchronously for one call, retains nothing, closes nothing, allocates no second owning wrapper, releases transients before return. Cache or weak-annotation updates are returned to the owner to apply. | Internal, hidden from consumer packages, forbidden from crossing a worker or WASM instance. | Cancellation of waiting only. Nothing chunks. |
| B: session authority moves as one change | Rust | Bytes and lifetime, page registry and leases, revision authority, mutation sequence, password state, retained import sources, together. TypeScript `DocumentSession` becomes a facade. `finishMutation` (`WorkerHost.ts:1380`) and `BaseDocumentRegistry` move with it. | Reverse of A: TypeScript leases a handle from Rust for one scoped operation. Lease token + pointer copy, never a borrow of Rust state. Checkout, return and finalize are three separate synchronous entry points. Second checkout while one is outstanding is a rejection. `close` with a lease out completes on return. Written removal criterion. | Chunking becomes expressible, with a named continuation owner, a page lease, a revision check on resume, and a close ordering. |
| C: retire families | Rust | Reader and Mutator families in whole clusters behind unchanged public services. Rust owns validation, native work, mutation impact, cache invalidation, revision publication. TypeScript keeps orchestration and presentation. | Deleted per family when parity and performance gates pass. | Per operation, documented. |

Rollback, per stage:

| Stage | What reverts | What is stranded | User-visible recovery |
|---|---|---|---|
| A | One routing entry. The bridge is internal and hidden from consumer packages, so no consumer sees the change. | Nothing. Reads are pure. | None needed. |
| B | Only while the worker still ships both implementations, which it does until the last family flips. Reverting means the TypeScript `DocumentSession` stops being a facade and resumes authority. | In-flight leases and any mutation sequence advanced on the Rust session. | New sessions open on the TypeScript backend; an open document must be saved and reopened. |
| C | Per family, until that family's TypeScript bridge is deleted. | Nothing beyond the family. | Same as B. |
| After Phase 6 | Nothing. Bridges are removed and `engine-services` is deleted. | Everything. | Revert the release. |

Carrying both implementations through Phase 5 is the price of B and C rollback. It is a cost, and
it is also the mechanism: delete the TypeScript backend early and the rollback column above becomes
"revert the release" three phases sooner.

Rules:

- Native hosts never use Stage A. They get a Rust-owned session over the same service code from the
  start.
- Two Stage A groups are named exceptions to "retains nothing, closes nothing". G9 `pages.extract`
  creates and closes its own scratch document (`FPDF_CreateNewDocument`, `PagesExtractor.ts:56-89`),
  which is a second document Rust owns end to end and never a borrow of the session's. G4 render
  hands the host a bitmap that outlives the call, under the buffer contract in Section 5. Neither
  touches the session's document or page pointers, which is what the rule protects.
- Stage A page borrowing goes through `document-session/pages/PagePtrPool.acquire` and releases in
  `finally`. Rust must not call `FPDF_LoadPage` itself: it would bypass normalized rotation, shared
  refcounts, and structural-mutation leak checks.
- Stage A does not let Rust create or retain the form model (`formModelCache.ts:29`, one
  `EPDFForm_LoadModel` snapshot per session keyed on mutation sequence). TypeScript lends the model
  pointer alongside the document pointer. Annotation reads use it via `joinWidgetField.ts:23`, so
  those slices carry a repeated-read benchmark against today's cached path.
- Backend selection happens at open. Never route reads to a Rust document while writes continue
  against a TypeScript one. Rollback means legacy for subsequent sessions, or save and reopen with
  the user aware.
- The Stage B bridge is proven in Phase 0 as a throwaway spike over `annotations.create` through
  the existing `AnnotationMutator` against a Rust-owned document, with failure-after-write, form
  model invalidation, and close-under-lease exercised.
- Stage B is also rehearsed on web before it lands. G3w (`multiplatform-port-order.md`) opens,
  closes and password-checks documents against a Rust-owned session on web in Phase 3, behind an
  opt-in flag, routing no reads. The Phase 0 spike is a throwaway. G3w is not: it is the same
  session code Stage B promotes to authority.
- Deliberate duplicates (web TypeScript geometry vs Rust geometry) get shared vectors, a named
  owner and a written exit criterion. Nothing is made slower on web to reach a percentage.

## 7. Cancellation contract

`abort()` at `packages/engine/core/src/promise/AbortablePromise.ts:91` rejects the caller's promise
synchronously with `AbortError` and fires the signal. That is cancellation of waiting. It does not
change. Cancellation of execution has four cases, documented per operation:

| Case | Behavior | Mechanism today |
|---|---|---|
| Queued, not started | Drop job, release inputs, publish nothing | `WorkerQueue` |
| Cooperative, before any write | Check token between units of work and stop. Mutators are cancellable up to the apply boundary. | `throwIfAborted(signal)`, 86 call sites in `engine/services` |
| Non-preemptible | Single synchronous native call, e.g. `FPDF_RenderPageBitmap`. Wait ends, work completes. | Documented per operation |
| After first write | Bookkeeping completes. Mutations are not rollback-atomic; no throw path may pretend no write occurred. Ordered batches report current item `failed`, rest `skipped`. Events publish exactly once even if the queue discarded the late response. | `finishMutation` |

Tests required: queued abort, in-flight pre-apply abort, post-write abort, late success, partial
failure. An `AbortError` is never evidence of rollback.

## 8. Native API design

- `uniffi` output is not the public surface. The prototype already renamed `close` to `closeSync`
  and `OpsError::EngineFailed.message` to `detail` to dodge generator collisions
  (`pdfium-multiplatform-prototype/packages/kotlin/uniffi.toml`).
- Facades may contain platform coordinate conversion, image adaptation, accessibility, scheduling,
  platform defaults. They may not decide what a PDF means.
- `PDFDocument` is `Sendable`; `async` methods bridge to the engine thread. Swift `Task`
  cancellation and Kotlin structured concurrency reach the scheduler, not just the await. Each
  method documents its cancellation case from Section 7.
- Synchronous `document.page(at:)` creates a lightweight handle from loaded identity metadata; any
  engine work is `async` / `suspend`.
- Errors: typed `PDFError` and sealed `PdfException`, carrying `EngineErrorCode`.
- Cleanup distinguishes "rejects new work and schedules disposal" from "all native resources
  released"; each helper documents which. Android `AutoCloseable` must not block the UI thread.
  Scoped helpers await document cleanup before engine cleanup on success, error and cancellation.
- Capability twins from `.agents/skills/embedpdf-conventions/references/permissions.md` exposed as
  platform APIs. Native UI reads the answer; engine and server enforcement stay authoritative.
  Unsupported mobile features report unsupported, never a silent no-op.
- Returned `CGImage` / `Bitmap` own or retain pixels independently of the closed document.
- Compiled consumer sample apps are the API acceptance test, including wrong-password, cancellation
  while queued and in flight, and explicit close-and-await.

```swift
let engine = try PDFEngine()
let document = try await engine.open(data: pdfData)
let image = try await document.page(at: 3).render(scale: 2)     // CGImage
for try await event in document.events { ... }                   // AsyncSequence<DocumentEvent>
```

```kotlin
PdfEngine().open(bytes).use { document ->
    val bitmap = document.page(3).render(scale = 2f)             // android.graphics.Bitmap
    document.events.collect { event -> /* Flow<DocumentEvent> */ }
}
```

## 9. Verification

| Mechanism | Exists | Covers | Notes |
|---|---|---|---|
| 21 conformance suites, `packages/engine/core/src/conformance/`, via `makeEngine` (22 files; `diffAnnotationListSnapshot.ts` is a helper) | Yes | Geometry, text, search, annotations, forms, pages, redaction, attachments, piece info, events | Does not cover raw rendering. "Run against both backends" is not yet true for three of them: `runPageGeometryOrientationConformance`, `runAnnotationReadConformance` and `runPieceInfoConformance` are invoked from `packages/engine/main` but not from `cloudpdf/engine`. Those three gate G2, G7 and G8, the first, largest and fiddliest read groups, so Phase 0 brings them onto the cloud runner. |
| Differential replay | No | Same operation sequence on two documents from identical bytes on independent sessions; compare results, events, revision sequences, and structure read back after save and reopen | Never two backends on one live document. Byte equality only where deterministic serialization is established (currently nowhere). |
| Frozen vectors | Prototype only (`pdfium-multiplatform-prototype/vectors/`) | Page counts, sizes, text, search hits exact; bitmaps by pixel diff, not hash | Exact interior BGRA on a colored fixture plus loose whole-page gate. |
| Cross-language corpus, `tooling/engine-conformance/vectors/` | No | Scenario format: ordered steps, symbolic bindings for run-time IDs, expected result and events per step, injected clock and seed | Runners in Rust, Node, browser, Swift, Kotlin, calling the handwritten facades. Unsupported cases report a capability reason; required cases may not skip. |

Stays procedural, per host: concurrency, cleanup ordering, close-under-load, lease violation.

Corpus determinism: `runAnnotationMutationConformance.ts:1217` and `:1257` use `Date.now()`;
services call `Date.now()` / `new Date()` in `SecurityReader.ts`, `writeAnnotationBase.ts`,
`PieceInfoAccessor.ts`, `applyMetadataPatch.ts`. Rust takes an injected clock and ID source.

Fixture corpus must add: rotated and cropped pages, non-BMP and non-printing text with non-empty
`charMap`, combining and RTL text, large scanned documents, complex vector pages, embedded fonts and
ICC profiles, encrypted documents, forms, attachments, malformed input, dense annotation and ink.

Benchmark discipline: differential answer gate before timing, equal-work audit at release
optimization, rotated arm order, consumed results, raw measurements retained with source and
toolchain hashes, medians and p95, optimized TypeScript arm included.

## 10. Performance gates

| Gate | Threshold |
|---|---|
| Geometry and search boundary | No per-glyph host call. WASM geometry does not regress against the Phase 0 recorded baseline and clears an absolute floor in milliseconds, both set in Phase 0 from that baseline; same for Node. The committed 2.6x and 7.16x are per-item-vs-coarse ratios from a different arm: they say a win exists, not how large this one must be. Section 8 records that decoding a packed buffer back into `PageGeometrySnapshot`'s glyph-object union eats part of it. No gate on `text` or readback rows until rerun. |
| Web regression | Investigate >5% repeatable on end-to-end workloads. Block default-backend switch >10% on p95 open, first page, search latency, or peak memory. Absolute floors alongside percentages. |
| Frame responsiveness (native) | 60 Hz on baseline devices, 120 Hz on designated. ≤2 ms p95 SDK UI-thread interop per 60 Hz frame. |
| Native reader | Device-specific open-to-first-page, search, sustained scroll, memory, battery and thermal targets set before beta, cold and warm, old and current devices. Desktop Node timings are not a mobile budget. |
| Memory | Bounded repeated workload returns live allocations to baseline; capacity reaches a plateau. |
| Payload | Compressed JS and WASM size, compile and instantiate time, native download and install size, uncached first useful paint on a controlled network. |

Phase 0 measurement of `pages.geometry` runs through the unchanged service boundary with run
materialization, transfer and allocation counted. `PageGeometrySnapshot` is a union of glyph
objects, not a flat buffer, so the packed buffer is an internal encoding with a decode on the far
side.

## 11. Compatibility

Every exported surface gets a ledger row: unchanged, adapter-preserved, deprecated, or intentionally
breaking. No fixed break budget. Migration guide is written from actual changes.

Expected forced breaks:

| Break | Cause |
|---|---|
| Browser floor rises to WASM exception handling | `rustc` std for `wasm32-unknown-emscripten` needs `-fwasm-exceptions`; PDFium uses `setjmp`/`longjmp` in `core/fxcodec/png/pngmodule.cpp` and `core/fxcodec/jpeg/jpeg_common.c`. `em++` refuses both. Target becomes `wasm32-eh` with `-sSUPPORT_LONGJMP=wasm`. Rebuilt module has 441 `try`, 367 `catch_all`, 285 `rethrow`, 0 `try_table` (legacy EH). Only recorded passes: Chrome 152.0.7977.83, Playwright Firefox 132.0 Nightly. Floor set from release-browser testing in Phase 1. |
| Internal wire DTO shapes | Rust canonical. Cosmetic mismatches fixed in Rust; real differences keep an adapter. |
| Direct `@embedpdf/engine-services` imports | Contents move to `crates/`. Either the package re-exports Rust-backed equivalents or it is removed and the ledger says so. An empty package is not a shim. |
| Web error surface | Unified on `EngineErrorCode`. String matching on messages breaks. |
| Documented execution cancellation | No code change; makes current behavior visibly weaker than callers may assume. |

Preserved, each with a test:

- Synchronous `localEngine()` / `EngineFactory` construction; no worker or WASM start until first
  use or `warmup()`.
- `Engine.open()` returns `AbortablePromise<DocumentHandle>` with unchanged input and options
  vocabulary.
- Every `DocumentHandle` service signature, DTO meaning, page identity, revision and event semantics.
- `createLocalEngine`, `createLocalEngineWithWorker`, `LocalEngine.fromTransport`, transport and
  worker type exports (`Transport`, `InlineTransport`, `BrowserWorkerTransport`, `LazyTransport`,
  `WorkerRequest`, `WorkerResponse`, `EngineWorkerInit`, `Priority`), with version negotiation on
  any wire change.
- Boot fonts and fallback ordering, `engine.fonts`, `LocalFontService`, concurrency options.
- Every export of `packages/engine/main/src/index.ts`, machine-inventoried in Phase 0.
- Built-in viewer entry point; engine-injected `/core` entry point free of PDFium symbols and WASM.
- SSR-safe imports; synchronous pure-TypeScript packages usable without async Rust init.
- `abort(reason?: unknown): void` signature and immediate rejection.
- Handwritten zod HTTP schemas in `packages/engine/core/src/wire/schemas.ts` stay separate and are
  tested in both directions against canonical Rust values (omitted vs null, unknown enums, numeric
  widths, nested unions, empty collections, invalid inputs).

Legacy backend stays selectable at open during preview. Removal condition is web conformance and
performance acceptance, not a date. Critical fixes land in both paths with a shared fixture.

## 12. Phases

Phases are dependency milestones, not dates. Phase 1 runs in parallel from the start and is the
likeliest critical path. Phases 2 and 3 overlap once the read contract stabilizes.

| Phase | Scope | Exit gate |
|---|---|---|
| 0. Foundations and baseline | Cargo workspace; `pdfium-sys`, `pdfium`, `epdf-engine` skeleton; Rust linked into the shipping web worker; `pages.geometry` through Stage A; Stage B bridge spike over `annotations.create`; lifecycle, buffer, coordinate and cancellation contracts implemented; DTO codegen proven on `MetadataPatch` (three-state `Keep \| Clear \| Set(T)`, driven through `metadata.update` on each host); operation matrix for all 65 `WorkerRequest` request kinds; corpus schema and geometry runner in `tooling/engine-conformance/`; `.github/workflows/engine-conformance.yml` wired; end-to-end baselines recorded; native `text` and readback rows, viewer allocation, and cold-start reruns. | `pages.geometry` at differential parity with `PageGeometryReader`, measured through the service boundary; bridge spike passes failure-after-write, cache invalidation, close-under-lease; clean-checkout build with `EM_BINARYEN_ROOT` asserted; matrix and contracts written. |
| 1. Reproducible distribution | Fork release with `ios-arm64`, `ios-sim-arm64`, `android-arm64`, `android-x64`, `wasm32-eh` (currently `"status": "pending"` at pin `d7b4caa26289d4846e94ff54e63b586b9bbe9fd6`; local fork commit `7f0fa5dbe` builds all five); `use_custom_libcxx=false` + `use_clang_modules=false`; Android `libc++_shared.so`, shrinker rules, 16 KB pages; Apple XCFramework in a Swift Package; minimal native session, open, password, close, geometry, render for smoke apps. | Clean-checkout builds from published artifacts only; real linked EH module tested in release Chrome, Firefox, Safari; fresh simulator and emulator apps; signed physical-device run. |
| 2. Read services | All read operations via Stage A on web and Rust-owned session on native, in the order in `multiplatform-port-order.md` groups G2 and G4 through G9 (G3 is the session group; on web it is native-only until G3w in Phase 3). Search has an entry gate: dialect vector file for `regex.ts` and `fold.ts` semantics. Throwaway device prototype on the oldest supported iPhone and a low-end Android measuring the pixel delivery path. | Read conformance green on both backends; native runners agree; raw-render pixel, tile, rotation and buffer-lifetime tests pass; cancellation, memory and lifecycle stress pass; native headless SDK preview usable; concurrent-engine soak green or one engine per process enforced in the API. |
| 3. Native reader products | SwiftUI/UIKit and Compose/View readers: continuous scroll, pinch zoom, virtualization, selection and copy, search navigation, links, annotation display, accessibility, file lifecycle. Behavioral references: `packages/plugin/render`, `plugin/selection`, `plugin/interaction`, `framework/web`, `plugin/search`, `core/stage`, `plugin/view-manager`. Backgrounding, memory warnings, rotation, process recreation policies. | Physical-device checks; no PDF work on the UI thread; installs from distributable artifacts alone; soak gate as Phase 2. |
| 4. Session authority, then writes | Stage B alone, then Stage C by family: annotation writes; 10 mutating `forms.*`; page assembly; `redaction.apply`; `metadata.update`; `pieceInfo.update` / `clear`; `attachments.create` / `delete`; `fonts.*`; save and export. Cost: worker ships both implementations until the last family flips. | Mutation, save-and-reopen, document-event and revision conformance green on both backends; cancellation-after-apply and partial-failure tests; no dual authority. |
| 5. Scale, isolation, server | Worker-pool scaling behind TSAN soak (`testing/tools:epdf_thread_soak`, `EMBEDPDF_TSAN=1`), sticky routing, load numbers; mobile crash-containment assessment; server milestone: `cloudpdf/server/src/runtime/worker-entry.ts:18` hosting the Rust crate via `napi-rs` on Linux and macOS with supervised host, `QuarantiningEnginePool`, `EngineRecycler`. | Budgets validated under load; server conformance and rollout validation green. Until then `engine-services` stays retained in the ledger. |
| 6. v4 rollout | Ledger complete; migration guide from actual changes; default web backend switched only where parity and performance are demonstrated; bridges removed; `engine-services` removed only if Phase 5 server milestone passed; rollback rehearsed. | v4 published. |

A mobile reader release may precede native editing and the web default switch. Documentation says
so. The web default does not move because a mobile milestone passed.

## 13. Deferred

| Item | Trigger to revisit |
|---|---|
| Interactive annotation editing on mobile; port of `packages/core/annotation` (7,527 lines) | Product decision. If yes: `update(msg) -> [model, effects]`, `scene(model, view) -> DisplayList`, one call per event and per frame; web keeps TypeScript unless measured against an optimized baseline on ink, cloudy borders, snapping, hit-test misses. |
| Shared tile planner | Separate proposal naming consumers, ownership and measured benefit. |
| Coarse `EPDF*` wrappers that only collapse JS crossings | Retire per caller once a coarse Rust op replaces every caller. Keep helpers that save work inside PDFium (`EPDFText_GetCharGeometry`: oriented quads, UPRIGHT, SYNTHESIZED flag). Do not add `EPDF_GetPageGeometryFull` if Phase 0 is close. |
| `packages/core/js-sandbox` replacement via `rquickjs` | After a native sandbox and behavior contract are validated. Mobile reports unsupported scripts until then. |
| Windows and musl Node targets; server rollout | After the first mobile release. |
| Mobile process isolation | Phase 5 assessment; not a reader prerequisite. |

## 14. Open risks

| Risk | Why it is open | Mitigation |
|---|---|---|
| 25,431 hand-ported lines, 16,620 of them `engine-services`, may not port cleanly | Nothing measured touches it | Stage A buys the read win without betting the schedule on it. `multiplatform-port-hazards.md` lists the designs that need rework. |
| Cross-language corpus cost | Never built one here; the last five-host test story produced two smoke tests and a grayscale golden | Phase 0 delivers format plus one case; Phase 2 delivers runners. Skipping it means mobile shipped on smoke tests. |
| Mobile performance | No spike touches iOS or Android runtime | Phase 2 device prototype before Phase 3 is budgeted. |
| Facades thicken | "No document semantics" is looser than "no decisions" | Corpus runners call the facades, so facade drift is a test failure. |
| `EMBEDPDF_TLS_GLOBALS` safety under two engines | Assumed from the server arrangement, not tested for mobile | Soak gate before any native artifact ships. |
| The whole web deliverable rides on one Emscripten module built with legacy WASM exception handling | 441 `try`, 367 `catch_all`, 285 `rethrow`, 0 `try_table`. Recorded passes are Chrome 152.0.7977.83 and Playwright Firefox 132.0 Nightly. A Nightly pass says nothing about release Firefox, and Safari is untested. There is no fallback if a release browser we support refuses the module. | Phase 1 decision gate: test release Firefox and Safari on real devices before the browser floor is announced. If either fails, price an alternate-EH build for both maintenance and payload before committing to it, and record the payload delta the way Section 3 records the +3,802 bytes. Do not announce a floor from Nightly results. |
