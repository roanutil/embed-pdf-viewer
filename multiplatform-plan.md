# Multiplatform EmbedPDF: Rust core, native wrappers

## 1. Goal

One Rust implementation of the PDF engine, consumed by:

| Platform      | Delivery                                                          | Consumer of the Rust core                                          |
| ------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------ |
| Web           | Existing WASM worker, one Emscripten module linking Rust + PDFium | TypeScript kernel, plugins, framework adapters, viewer (unchanged) |
| iOS           | Swift Package with self-contained XCFramework                     | Handwritten Swift facade over `uniffi`                             |
| Android       | Maven AAR with per-ABI `.so`                                      | Handwritten Kotlin facade over `uniffi`                            |
| Node / server | `napi-rs` addon in the existing `worker_thread`                   | `@cloudpdf/server`, `@cloudpdf/engine`                             |

Two deliverables; their relative effort has not been estimated:

1. The engine port starts from a 25,431-line source inventory, not a hand-port
   estimate. `packages/engine/services` (16,620) plus 8,811 lines of
   `packages/engine/core` (text layout, search folding, DTOs, annotation kinds
   and helpers, error codes, scope resolution). The inventory also identifies
   695 annotation-kind barrel lines and 1,087 root barrel lines for mechanical
   updates. The 1,070 lines of dedicated annotation schema files stay
   handwritten, with bidirectional parity tests against generated types and Rust
   serialization. Other exported Zod schemas (forms, geometry, actions and
   shared annotation shapes) also remain with validation parity.
   `multiplatform-port-order.md` section 2 itemizes these categories.
2. Native reader products on iOS and Android. With no JavaScript runtime in the
   proposed native SDKs, presentation code (tile scheduling, selection,
   gestures, search navigation, virtualization) is implemented in Swift and
   Kotlin, with shared behavioral vectors where practical.

## 2. Decision summary

| Decision                    | Choice                                                                                                                                                                                                                                                                                                                                                                                                      |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Boundary                    | Rust owns PDFium's C ABI up through the 61 non-control operations in `packages/engine/core/src/wire/worker-protocol.ts` (`WorkerRequest` is a 63-member union; `OpenWorkerRequest` expands to three `open.*` kinds, giving 65 request kinds, of which 4 are control frames). TypeScript keeps everything above.                                                                                             |
| Web kernel, plugins, viewer | Stay TypeScript. `packages/core/main`, `packages/core/stage`, `packages/core/annotation` carry the strongest presumption against porting.                                                                                                                                                                                                                                                                   |
| Native API                  | `uniffi` is the ABI, not the API. Each platform gets a handwritten facade.                                                                                                                                                                                                                                                                                                                                  |
| Migration unit              | Resource ownership, not operations. Three stages (Section 6). Proposed local-open option `PdfOpenOptions.engineBackend: 'typescript' \| 'rust'` selects a fixed routing profile for the session, defaulting to `'typescript'` during preview. In Stage A, both profiles have a TypeScript session owner; the Rust profile uses migrated coarse reads. From Stage B, the Rust profile also owns its session. |
| DTOs                        | Rust definition is canonical for internal wire DTOs; generated TypeScript is checked in; dirty diff fails CI. Public web DTOs that differ keep an adapter.                                                                                                                                                                                                                                                  |
| Errors                      | One vocabulary, `EngineErrorCode`, on every platform.                                                                                                                                                                                                                                                                                                                                                       |
| Target version              | v4. The reviewed package manifests use `3.0.0-next.11`; published release status is not a prerequisite for this proposal. Rust backend ships opt-in during preview.                                                                                                                                                                                                                                         |
| Repo layout                 | Cargo workspace at repo root (`Cargo.toml`, `crates/`); native SDK sources in `platforms/apple/`, `platforms/android/`. Neither is in the npm namespace.                                                                                                                                                                                                                                                    |

## 3. Evidence

Benchmark timings come from the committed tables:
`pdfium-binding-benchmarks/RESULTS.md` and
`viewer-rust-port-benchmarks/bench/RESULTS.md`, with additional historical
observations in `docs/research/`: the three measurement bugs and the 184x
`rows.find` (`pdfium-rust-wrapper-benchmarks.md:321`,
`rust-core-port-benchmarks.md:247-248`) and the exception handling counts and
browser passes (`platform-gap-audit.md`, “Verification performed”). The
prototype README and the audit’s implementation follow-up describe later fixes.
Historical browser runs and opcode counts apply to the audited artifact, not
every later build. Binding-benchmark timings are medians of batch means; their
p95 values are not individual-call tail latency.

| Measurement                                                                              | Result                                                                                                                                                                                                         | Rule it produces                                                                                                                                                                                                                       |
| ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| WASM geometry, per-item vs coarse (`pdfium-binding-benchmarks/RESULTS.md`, WASM medians) | 2.788 ms vs 1.070 ms (2.6x). Rust vs C++ owning the loop: `d/c` 0.962 to 1.048.                                                                                                                                | Put Rust where the compute behind one crossing is large. These arms isolate a crossing-cost benefit; they do not establish that language and allocation costs never matter.                                                            |
| WASM search, per-item vs coarse                                                          | 102.36 µs vs 30.61 µs (3.3x)                                                                                                                                                                                   | Same.                                                                                                                                                                                                                                  |
| WASM `text`, all four arms                                                               | Within about 2.1%                                                                                                                                                                                              | `PageTextReader.ts:93` is already coarse. No boundary win to gate on.                                                                                                                                                                  |
| WASM `render-1x`, `render-4x`                                                            | Dominated by PDFium bitmap work                                                                                                                                                                                | No material rasterization benefit was established by these arms. Do not promise faster rasterization.                                                                                                                                  |
| Thin Rust frame over PDFium in WASM                                                      | About +1 to 4% on the measured PDF workloads, +3,802 bytes on 4,676,714                                                                                                                                        | `pdfium-sys` is not an optimization target.                                                                                                                                                                                            |
| Native geometry, arm b vs c (`pdfium-binding-benchmarks/RESULTS.md`, native medians)     | 4.134 ms vs 577.37 µs (7.16x), same `napi-rs` addon                                                                                                                                                            | Boundary win holds natively.                                                                                                                                                                                                           |
| Native search (same native table)                                                        | b/c: 64.48 / 39.84 µs = 1.62x; WASM a/c: 3.3x                                                                                                                                                                  | Win is workload-specific in both directions.                                                                                                                                                                                           |
| Native `text` and readback rows (binding-results caveat)                                 | Predate the `bench/arm.mjs` readback-copy fix                                                                                                                                                                  | Rerun before gating anything on them. `geometry` and `search` do not touch those paths and stand.                                                                                                                                      |
| Viewer frame, 1,000 anchored shapes (viewer frame/decode tables)                         | TS 121.96 µs vs Rust 200.76 µs; Rust raw buffer 171.43 µs. Rust won 0 of 20 cells.                                                                                                                             | Cheap math behind one crossing loses. Keep web projection in TypeScript unless a real-feature measurement says otherwise.                                                                                                              |
| Viewer, n≥10                                                                             | Rust-with-state 1.60x to 3.52x slower; n=1 cells up to 10.57x                                                                                                                                                  | Same. Toy is rectangles, not `cloudy.ts`; result bounds the boundary cost, not Rust on real annotation workloads.                                                                                                                      |
| Prototype pixel golden (`pdfium-multiplatform-prototype`)                                | The prototype README records 6 paths matching at mean 0 / max 0, but page 4 is grayscale: a B-to-R swap also scores 0. Tolerances mean 2 / max 16 pass darkening every non-white byte by 6 (mean 1.74, max 6). | The grayscale golden cannot establish channel order. The prototype now also has `fixtures/colors.pdf`, `vectors/colors.json`, and exact color tests in Node, WASM, Swift and Kotlin; reuse and extend them for the shipping RGBA path. |
| Benchmark study's own bugs                                                               | 3 measurement bugs reported as reversing or manufacturing a conclusion, none caught by the fairness gate; O(n²) `rows.find` invisible at demo size, 184x at 10,000                                             | Differential answer gate before timing; equal-work audit; rotated arm order; medians and p95; optimized TypeScript arm included.                                                                                                       |

The binding benchmark’s `search` workload calls PDFium text-search functions
(`pdfium-binding-benchmarks/bench/workloads.mjs`, `searchThin` /
`searchCoarse`). The shipping `SearchReader` uses JavaScript matching, folding
and corpus/cursor logic. These ratios demonstrate a boundary effect in that
benchmark; they are not forecasts for porting the shipping search service.

## 4. Global constraints

Every phase inherits these.

1. One owner per resource and per state transition. TypeScript and Rust never
   both hold authority over a document handle, page cache, revision counter, or
   mutation sequence.
2. The host-to-Rust boundary is per operation, never per item. No convenience
   API may reintroduce a per-glyph, per-annotation, or per-rect call.
3. No Rust export taking `&mut self` may permit host re-entry while the borrow
   is live. A callback that re-entered the WASM object produced
   `attempted to take ownership of Rust value while it was borrowed` in
   `viewer-rust-port-benchmarks/rust-geometry-and-state`.
4. Public bindings hand out opaque handles carrying an engine generation. A
   handle from a replaced engine fails rather than resolving against new state.
   Raw pointers never appear in a consumer API.
5. PDFium handles never cross threads. Native builds set
   `EMBEDPDF_TLS_GLOBALS=true` and each engine owns one dedicated OS thread.
   `wasm32` targets leave it off because each instance has its own linear
   memory. Contract:
   `.agents/skills/embedpdf-conventions/references/runtime-confinement.md`.
6. Two text index spaces, both carried. CHARACTER space is PDFium's character
   list (geometry, hit-testing, selection, `PageTextSnapshot.charCount`). TEXT
   space is UTF-16 code-unit offsets into the extracted string (search,
   slicing). `PageTextSnapshot.charMap` encodes the two divergences:
   non-printing characters contribute 0 units, supplementary-plane characters
   contribute 2. Every boundary DTO names its space. Rust ports
   `packages/engine/core/src/text/charmap.ts` including `charMapViolation`. The
   prototype report vector cannot test this (the shipping text-divergence suite
   already supplies additional cases): `vectors/report-page4.json` has
   `charCount` 8,106 over 8,106 UTF-16 units and no `charMap`.
7. Rust is canonical for internal wire DTOs. Publicly exported worker types and
   Cloud HTTP DTOs remain compatibility surfaces; an internal Rust encoding must
   preserve them through generation or adapters. The protocol carries a version
   so mismatched JS, WASM and native artifacts are detected.

## 5. Architecture

### 5.1 Layers

| Layer                                                      | Owns                                                                                                                                                                                                                                                                            | Does not own                                |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| `crates/pdfium-sys`                                        | `bindgen` 0.72.1 over the fork headers `packages/engine/runtime/build/generate-functions.mjs` already walks, filter `FPDF\|EPDF\|FORM\|PDFiumExt_`                                                                                                                              | Anything safe                               |
| `crates/pdfium`                                            | RAII wrappers, checked lengths, unsafe boundaries (audited before promotion)                                                                                                                                                                                                    | Document semantics                          |
| `crates/epdf-engine`                                       | Document session (bytes, lifetime, page identity and leases, revisions, mutation sequence, password state, retained import sources), all Reader and Mutator families, text layout, search, DTOs, error codes, permission enforcement for native sessions, mutation finalization | `uniffi`, browser, UIKit, Android types     |
| `crates/epdf-uniffi`                                       | `uniffi::setup_scaffolding!()`, newtype + `From` per exported type                                                                                                                                                                                                              | Building for `wasm32-unknown-emscripten`    |
| `crates/epdf-wasm`, `crates/epdf-node`                     | Host adapters: typed operation decode, buffer handoff, `napi-rs`; TypeScript retains worker envelopes/control frames                                                                                                                                                            | Semantics                                   |
| `crates/build-support`                                     | Shared `build.rs`. Exists because Cargo does not propagate `cargo:rustc-link-arg` to a dependent's test or bin targets                                                                                                                                                          |                                             |
| TypeScript `packages/engine/main`, `worker-host` transport | Message loop, transport, capability guard on web, `LocalEngine`, service orchestration                                                                                                                                                                                          | Engine behavior                             |
| Swift / Kotlin facades                                     | Platform types, coordinate conversion, image adaptation, scheduling, accessibility                                                                                                                                                                                              | Document semantics or shared business rules |

Dependency direction, unchanged from today:
`runtime/shared → document-session → features → worker-host`. Prototype
`pdfium-sys` and safe wrapper are review inputs. Prototype operation objects,
dispatch and host adapters are scaffolding to redesign.

### 5.2 Runtime topology

| Target     | Threading                                                                                                                             | Notes                                                                                                                                                              |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Web worker | Single-threaded message loop; Rust + PDFium in one Emscripten module                                                                  | Never split into two WASM heaps joined by per-call JavaScript. An abort message is not observed while a synchronous handler runs (`WorkerHost.ts:199`, `:225`).    |
| Native     | One owner OS thread per engine, inside Rust; handle types are `!Send + !Sync`; exported engine object is thread-safe and submits jobs | A Swift actor, coroutine dispatcher or mutex does not guarantee affinity. Implementation reference: prototype `crates/epdf-ops/src/dispatch/{threaded,inline}.rs`. |
| Node       | `napi-rs` addon in the existing `worker_thread`                                                                                       | Eight native targets in `packages/engine/runtime/npm/` (nine directories, one of which is `wasm32`). Windows and musl deferred past the first mobile release.      |

Multi-document operations (page import) coordinate on one owner: transfer bytes
or reopen a controlled snapshot on the destination. Never pass a source document
handle across owners.

Thread confinement is not crash isolation. A native fault in a mobile process
kills the app; `catch_unwind` catches unwinding Rust panics; it does not contain
segfaults, process aborts, or arbitrary PDFium corruption. Server preserves
`cloudpdf/server/src/runtime/`'s selectable supervised-host mode,
`QuarantiningEnginePool` and `EngineRecycler`. Mobile isolation is a separate
investigation, not a reader-release prerequisite.

The native facade must not wrap a blocking channel round-trip in an `async`
method and call it on the UI thread. Use asynchronous submission/reply bindings
(UniFFI async exports and Node promises), or an explicitly owned blocking
executor until those bindings land. The prototype’s synchronous
`dispatch/threaded.rs` proves affinity, not nonblocking caller behavior.

### 5.3 Lifecycle contract (Phase 0 deliverable)

- Bounded queues for jobs, open documents, input bytes, raster and cache bytes,
  search output. Admission checked before large allocations. Reserved control
  route so saturation cannot prevent admission of close or cancel. A running
  synchronous PDFium call still delays disposal; native cancellation sets an
  atomic flag without waiting for the owner’s work queue.
- Close is idempotent, rejects new work, invalidates handles, drains or cancels
  per contract, releases on the owner thread. Closing an engine closes its
  documents.
- Engine replacement with generation IDs. No process-wide singleton without
  reset.
- Panic after a potentially mutating operation poisons the session unless an
  operation-specific invariant says otherwise. Pending requests reject on worker
  failure.
- Aggregate memory monitored at process and WASM level; PDFium allocations are
  invisible to Rust budgets. A live `WebAssembly.Memory` has no shrink API:
  measure live allocator usage, not `byteLength`.
- Buffer contract: layout, version, alignment, stride, pixel format, alpha mode,
  color space, allocator, release op, lifetime. Lengths and multiplications
  validated before allocation. Heap views rebuilt after WASM memory growth.
  Preserve the web `PageRender` contract: `deviceRaster.ts` returns
  `color: 'rgba8'`, `premultipliedAlpha: false`, and owned bytes, using
  `FPDF_REVERSE_BYTE_ORDER`. Prototype BGRA buffers require explicit host
  conversion; test channel order and transparent-pixel alpha at the public
  service boundary.
- Coordinate contract: normalized page points, origin and axis direction, crop
  and rotation transforms, device scale, durable page identity (index is not
  identity).

## 6. Ownership migration

`DocumentSession` (`packages/engine/services/src/document-session/`, 832 lines)
owns document bytes and lifetime, page identity and leases, the revision
counter, the mutation sequence, and password state. Two languages cannot both
own those.

| Stage                                    | Owner of session | What moves                                                                                                                                                                                                                                                                       | Bridge                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Web cancellation                                                                                                               |
| ---------------------------------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| A: coarse reads over borrowed handles    | TypeScript       | Read operations only. Rust borrows a validated document or page pointer synchronously for one call, retains nothing, closes nothing, allocates no second owning wrapper, releases transients before return. Cache or weak-annotation updates are returned to the owner to apply. | Internal, hidden from consumer packages, forbidden from crossing a worker or WASM instance.                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Cancellation of waiting only. Nothing chunks.                                                                                  |
| B: session authority moves as one change | Rust             | Bytes and lifetime, page registry and leases, revision authority, mutation sequence, password state, retained import sources, together. TypeScript `DocumentSession` becomes a facade. `finishMutation` (`WorkerHost.ts:1380`) and `BaseDocumentRegistry` move with it.          | Reverse of A: TypeScript leases a handle from Rust for one scoped operation. Lease token + pointer copy, never a borrow of Rust state. Checkout, return and finalize are three separate synchronous entry points. Second checkout while one is outstanding is a rejection. `close` with a lease out rejects new work and completes only after return and mutation finalization. While a lease or unfinalized outcome exists, reject all other document operations; enforce this at runtime because no Rust borrow spans the host call. Written removal criterion. | Chunking becomes expressible, with a named continuation owner, a page lease, a revision check on resume, and a close ordering. |
| C: retire families                       | Rust             | Reader and Mutator families in whole clusters behind unchanged public services. Rust owns validation, native work, mutation impact, cache invalidation, revision publication. TypeScript keeps orchestration and presentation.                                                   | Removed from the Rust path per family when parity and performance gates pass; retain bridges needed for rollback until the TypeScript fallback is retired.                                                                                                                                                                                                                                                                                                                                                                                                        | Per operation, documented.                                                                                                     |

Rollback, per stage:

| Stage                         | What reverts                                                                                                                                                                                                                                             | What is stranded                                                                                                   | User-visible recovery                                                                     |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| A                             | One routing entry. The bridge is internal and hidden from consumer packages, so no consumer sees the change.                                                                                                                                             | No document edits. Session cache/weak-state updates must already have been applied once or discarded consistently. | None needed.                                                                              |
| B                             | Only while the worker still ships both implementations, which it does through Phase 5 until server migration passes and the TypeScript fallback is retired. Reverting means the TypeScript `DocumentSession` stops being a facade and resumes authority. | In-flight leases and any mutation sequence advanced on the Rust session.                                           | New sessions open on the TypeScript backend; an open document must be saved and reopened. |
| C                             | Per family while its bridge is retained; backend rollback remains available through Phase 5.                                                                                                                                                             | Nothing beyond the family.                                                                                         | Same as B.                                                                                |
| After actual fallback removal | No backend rollback. Bridges are removed and `engine-services` is deleted.                                                                                                                                                                               | Everything.                                                                                                        | Revert the release.                                                                       |

All deletion statements in these documents are conditional. Phase 5 initially
validates Linux and macOS. If supported Windows/musl deployments still need
TypeScript, v4 retains that backend and its runtime plumbing until those targets
migrate or support is explicitly removed in the compatibility ledger. A mobile
release alone never authorizes deletion.

Carrying both implementations through Phase 5 is the price of B and C rollback.
It is a cost, and it is also the mechanism: delete the TypeScript backend early
and the rollback column above becomes "revert the release" three phases sooner.

Rules:

- Native hosts never use Stage A. They get a Rust-owned session over the same
  service code from the start.
- Two Stage A groups are named exceptions to "retains nothing, closes nothing".
  G9 `pages.extract` creates and closes its own scratch document
  (`FPDF_CreateNewDocument`, `PagesExtractor.ts:56-89`), which is a second
  document Rust owns end to end and never a borrow of the session's. G4 render
  hands the host a bitmap that outlives the call, under the buffer contract in
  Section 5. Neither takes ownership of the borrowed session handles: extraction
  reads the source document and rendering uses the borrowed page. Only their
  independently owned output resources may escape.
- Stage A page borrowing goes through
  `document-session/pages/PagePtrPool.acquire` and releases in `finally`. Rust
  must not call `FPDF_LoadPage` itself: it would bypass normalized rotation,
  shared refcounts, and structural-mutation leak checks.
- Stage A does not let Rust create or retain the form model
  (`formModelCache.ts:29`, one `EPDFForm_LoadModel` snapshot per session keyed
  on mutation sequence). TypeScript lends the model pointer alongside the
  document pointer. Annotation reads use it via `joinWidgetField.ts:23`, so
  those slices carry a repeated-read benchmark against today's cached path.
- Fonts are engine-level and move in G4 (Phase 1 smoke, Phase 2 complete),
  before document mutators. In each combined web worker, Rust owns one
  `FontRegistry`: the stable-key-to-native-ID map, registrations, fallback order
  and retained file-access handles. Select that owner at worker initialization,
  before startup fonts or documents are loaded. TypeScript `FontRegistrar`
  delegates registration, lookup (`idFor`), fallback changes and clear to that
  owner, including calls from retained TypeScript annotation writers. It keeps
  no duplicate map or handle ownership. Both document backends use the same
  registry in that worker; per-document `engineBackend` never selects fonts.
  This is an engine-level exception to Stage A's read-only scope. Native uses
  the Rust registry from its first render; the unmigrated server retains its
  TypeScript registry until Phase 5. Registry operations run on the PDFium owner
  thread, with no host callback from a Rust mutable borrow; numeric font IDs
  never cross workers.
- Document rollback leaves the worker's font owner intact. Rolling back the Rust
  font registry itself requires a fresh worker with the retained TypeScript
  owner: save and reopen affected documents, replay host-held font bytes or file
  paths and fallback order using stable keys, and regenerate IDs. Do not
  transfer numeric IDs or live handles. G4 gates on register-once rendering
  through both backends, retained TypeScript FreeText authoring,
  clear/re-register, fallback ordering, and worker-recreation rollback.
- Backend selection happens at open. Never route reads to a Rust document while
  writes continue against a TypeScript one. Rollback means legacy for subsequent
  sessions, or save and reopen with the user aware.
- The Stage B bridge is proven in Phase 0 as a throwaway spike over
  `annotations.create` through the existing `AnnotationMutator` against a
  Rust-owned document, with failure-after-write, form model invalidation, and
  close-under-lease exercised.
- Stage B is also rehearsed on web before it lands. G3w
  (`multiplatform-port-order.md`) opens, closes and password-checks documents
  against a Rust-owned session on web in Phase 3, behind an internal rehearsal
  flag, routing no reads. It is not a viewer-ready public backend; F9 uses a
  transport-level open/cancel test until Stage B permits normal reads. The Phase
  0 spike is a throwaway. G3w is not: it is the same session code Stage B
  promotes to authority.
- Deliberate duplicates (web TypeScript geometry vs Rust geometry) get shared
  vectors, a named owner and a written exit criterion. Nothing is made slower on
  web to reach a percentage.

## 7. Cancellation contract

`abort()` at `packages/engine/core/src/promise/AbortablePromise.ts:91` rejects
the caller's promise by settling it with `AbortError` during the call and firing
the signal; promise reactions still run asynchronously. That is cancellation of
waiting. It does not change. Cancellation of execution has four cases,
documented per operation:

| Case                          | Required behavior                                                                                                                                                                                                                               | Current basis / work needed                                                                 |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Queued, not started           | Drop job, release inputs, publish nothing                                                                                                                                                                                                       | `WorkerQueue`                                                                               |
| Cooperative, before any write | Check token between units of work and stop. Mutators are cancellable up to the apply boundary.                                                                                                                                                  | `throwIfAborted(signal)`, 86 call sites in `engine/services`                                |
| Non-preemptible               | Single synchronous native call, e.g. `FPDF_RenderPageBitmap`. Wait ends, work completes.                                                                                                                                                        | Documented per operation                                                                    |
| After first write             | Bookkeeping completes. Mutations are not rollback-atomic; no throw path may pretend no write occurred. Ordered batches report current item `failed`, rest `skipped`. Events publish exactly once even if the queue discarded the late response. | `finishMutation` handles sequence/artifacts; independent event delivery is additional work. |

Today `WorkerQueue.handleResponse` discards a result after an in-flight abort.
Merely returning Rust-generated events in that result cannot guarantee delivery.
Stage B must add an acknowledged, ordered event path independent of request
completion, deduplicate by session/event sequence, and specify resync after
worker failure. Apply the same observable rule to the retained backend before
asserting cross-backend parity.

Tests required: queued abort, in-flight pre-apply abort, post-write abort, late
success, partial failure. An `AbortError` is never evidence of rollback.

## 8. Native API design

- `uniffi` output is not the public surface. The prototype already renamed
  `close` to `closeSync` and `OpsError::EngineFailed.message` to `detail` to
  dodge generator collisions
  (`pdfium-multiplatform-prototype/packages/kotlin/uniffi.toml`).
- Facades may contain platform coordinate conversion, image adaptation,
  accessibility, scheduling, platform defaults. They may not decide what a PDF
  means.
- `PDFDocument` is `Sendable`; `async` methods bridge to the engine thread.
  Swift `Task` cancellation and Kotlin structured concurrency reach the
  scheduler, not just the await. Each method documents its cancellation case
  from Section 7. UniFFI does not supply automatic task cancellation: expose a
  separate cancel-by-job-ID channel and wire each facade to it; dropping the
  await must not drop the owner’s mutation-finalization or resource-cleanup
  work.
- Synchronous `document.page(at:)` creates a lightweight handle from loaded
  identity metadata; any engine work is `async` / `suspend`.
- Errors: typed `PDFError` and sealed `PdfException`, carrying
  `EngineErrorCode`.
- Cleanup distinguishes "rejects new work and schedules disposal" from "all
  native resources released"; each helper documents which. Android
  `AutoCloseable` must not block the UI thread. Scoped helpers await document
  cleanup before engine cleanup on success, error and cancellation.
- Capability twins from
  `.agents/skills/embedpdf-conventions/references/permissions.md` exposed as
  platform APIs. Native UI reads the answer; engine and server enforcement stay
  authoritative. Unsupported mobile features report unsupported, never a silent
  no-op.
- Returned `CGImage` / `Bitmap` own or retain pixels independently of the closed
  document.
- Compiled consumer sample apps are the API acceptance test, including
  wrong-password, cancellation while queued and in flight, and explicit
  close-and-await.

Proposed scoped APIs (to be implemented and compile-tested, not available
today). The helpers await document disposal before engine shutdown on success,
error and cancellation. Kotlin cleanup runs in a non-cancellable cleanup
context; Swift cleanup must run even when its parent task is cancelled.

```swift
let image = try await withPDFEngine { engine in
    try await engine.withDocument(data: pdfData) { document in
        try await document.page(at: 3).render(scale: 2) // CGImage
    }
}
```

```kotlin
val bitmap = withPdfEngine { engine ->
    engine.withDocument(bytes) { document ->
        document.page(3).render(scale = 2f) // android.graphics.Bitmap
    }
}
```

Event subscriptions live inside the document’s scope and terminate on close; an
unbounded `collect`/iteration is not part of a render-and-close example.

## 9. Verification

| Mechanism                                                                                                                                | Exists                                                     | Covers                                                                                                                                                                                                                                  | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 21 conformance suites, `packages/engine/core/src/conformance/`, via `makeEngine` (22 files; `diffAnnotationListSnapshot.ts` is a helper) | Yes                                                        | Geometry, text, search, annotations, forms, pages, redaction, attachments, piece info, events                                                                                                                                           | Does not cover raw rendering. "Run against both backends" is not yet true for three of them: `runPageGeometryOrientationConformance`, `runAnnotationReadConformance` and `runPieceInfoConformance` are invoked from `packages/engine/main` but not from `cloudpdf/engine`. Those three gate G2, G7 and G8, the first, largest and fiddliest read groups, so Phase 0 brings them onto the cloud runner.                                                                                                 |
| Differential replay                                                                                                                      | No                                                         | Same operation sequence on two documents from identical bytes on independent sessions; compare results, events, revision sequences, and structure read back after save and reopen                                                       | Never two backends on one live document. Saved PDFs are compared semantically after reopen; attachment and other deterministic payload bytes can be compared exactly.                                                                                                                                                                                                                                                                                                                                  |
| Frozen vectors                                                                                                                           | Prototype only (`pdfium-multiplatform-prototype/vectors/`) | Page counts, sizes, text, search hits exact; bitmaps by pixel diff, not hash                                                                                                                                                            | Reuse the existing exact BGRA color vectors, add public RGBA/alpha assertions, and keep a loose whole-page gate.                                                                                                                                                                                                                                                                                                                                                                                       |
| Cross-language corpus, `tooling/engine-conformance/vectors/`                                                                             | No                                                         | Scenario format: ordered steps, symbolic bindings for run-time IDs, expected result and events per step, injected clock and seed                                                                                                        | Runners in Rust, Node, browser, Swift, Kotlin, calling the handwritten facades. Unsupported cases report a capability reason; required cases may not skip.                                                                                                                                                                                                                                                                                                                                             |
| Web viewer end-to-end, `tooling/viewer-e2e/` driving `examples/viewer-react`                                                             | No                                                         | 14 flows: open and first paint, scroll and virtualization, zoom, selection and copy, search navigation, links, annotation and form display, open failure path, annotation, form, page-assembly and redaction round trips, undo and redo | Both backends per job, selected at open. No project declares or runs a Playwright viewer suite today (the lockfile contains optional peer references); the five `vitest.config.ts` files under `packages/` all set `environment: 'node'` and the explicit `happy-dom` overrides are four files in `packages/framework/react/test/`. Flow list and group mapping: `multiplatform-port-order.md` Section 7. Also runs once against the packed tarballs `tooling/consume-fixtures` builds for `vite-app`. |

Stays procedural, per host: concurrency, cleanup ordering, close-under-load,
lease violation.

Corpus determinism: `runAnnotationMutationConformance.ts:1217` and `:1257` use
`Date.now()`; services call `Date.now()` / `new Date()` in `SecurityReader.ts`,
`writeAnnotationBase.ts`, `PieceInfoAccessor.ts`, `applyMetadataPatch.ts`. Rust
takes an injected clock and ID source.

Fixture corpus must add: rotated and cropped pages, non-BMP and non-printing
text with non-empty `charMap`, combining and RTL text, large scanned documents,
complex vector pages, embedded fonts and ICC profiles, encrypted documents,
forms, attachments, malformed input, dense annotation and ink.

Benchmark discipline: differential answer gate before timing, equal-work audit
at release optimization, rotated arm order, consumed results, raw measurements
retained with source and toolchain hashes, medians and p95, optimized TypeScript
arm included.

## 10. Performance gates

| Gate                          | Threshold                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Geometry and search boundary  | No per-glyph host call. WASM geometry does not regress against the Phase 0 recorded baseline and meets an absolute latency ceiling in milliseconds, both set in Phase 0 from that baseline; same for Node. The committed 2.6x and 7.16x are per-item-vs-coarse ratios from a different arm: they say a win exists, not how large this one must be. The paragraph below records that decoding a packed buffer back into `PageGeometrySnapshot`'s runs and glyph objects adds work that the benchmark ratio does not isolate. No gate on `text` or readback rows until rerun. |
| Web regression                | Investigate >5% repeatable on end-to-end workloads. Block default-backend switch >10% on p95 open, first page, search latency, or peak memory. Absolute latency and memory ceilings alongside percentages.                                                                                                                                                                                                                                                                                                                                                                  |
| Frame responsiveness (native) | 60 Hz on baseline devices, 120 Hz on designated. ≤2 ms p95 SDK UI-thread interop per 60 Hz frame.                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Native reader                 | Device-specific open-to-first-page, search, sustained scroll, memory, battery and thermal targets set before beta, cold and warm, old and current devices. Desktop Node timings are not a mobile budget.                                                                                                                                                                                                                                                                                                                                                                    |
| Memory                        | Bounded repeated workload returns live allocations to baseline; capacity reaches a plateau.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Payload                       | Compressed JS and WASM size, compile and instantiate time, native download and install size, uncached first useful paint on a controlled network.                                                                                                                                                                                                                                                                                                                                                                                                                           |

Phase 0 measurement of `pages.geometry` runs through the unchanged service
boundary with run materialization, transfer and allocation counted.
`PageGeometrySnapshot` contains an array of upright/rotated run variants and
glyph objects, not a flat buffer, so the packed buffer is an internal encoding
with a decode on the far side.

## 11. Compatibility

Every exported surface gets a ledger row: unchanged, adapter-preserved,
deprecated, or intentionally breaking. No fixed break budget. Migration guide is
written from actual changes.

Compatibility decisions to resolve (not all require a public break):

| Break                                                        | Cause                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| WASM exception-handling requirement; browser floor to verify | The chosen unwinding build with rustup std for `wasm32-unknown-emscripten` needs `-fwasm-exceptions`; PDFium uses `setjmp`/`longjmp` in `core/fxcodec/png/pngmodule.cpp` and `core/fxcodec/jpeg/jpeg_common.c`. `em++` rejects combining WASM exceptions with JavaScript-based longjmp; the two mechanisms must use compatible settings. The fork artifact becomes `wasm32-eh` (the Rust target remains `wasm32-unknown-emscripten`) with `-sSUPPORT_LONGJMP=wasm`. The historical audit’s module had 441 `try`, 367 `catch_all`, 285 `rethrow`, 0 `try_table` (legacy EH). Only recorded passes: Chrome 152.0.7977.83, Playwright Firefox 132.0 Nightly. Floor set from release-browser testing in Phase 1. |
| Internal wire DTO shapes                                     | Rust canonical. Cosmetic mismatches fixed in Rust; real differences keep an adapter.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Direct `@embedpdf/engine-services` imports                   | Contents move to `crates/`. Either the package re-exports Rust-backed equivalents or it is removed and the ledger says so. An empty package is not a shim.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Web error surface                                            | Already uses `EngineErrorCode`; preserve codes and structured details. Message text is not a parity contract.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Documented execution cancellation                            | Document waiting versus execution cancellation. Independent late-event delivery and any chunking require implementation changes.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

Preserved, each with a test:

- Synchronous `localEngine()` / `EngineFactory` construction; no worker or WASM
  start until first use or `warmup()`.
- `Engine.open()` returns `AbortablePromise<DocumentHandle>` with unchanged
  input and options vocabulary.
- Every `DocumentHandle` service signature, DTO meaning, page identity, revision
  and event semantics.
- `createLocalEngine`, `createLocalEngineWithWorker`,
  `LocalEngine.fromTransport`, transport and worker type exports (`Transport`,
  `InlineTransport`, `BrowserWorkerTransport`, `LazyTransport`, `WorkerRequest`,
  `WorkerResponse`, `EngineWorkerInit`, `Priority`), with version negotiation on
  any wire change.
- Boot fonts and fallback ordering, `engine.fonts`, `LocalFontService`,
  concurrency options.
- Every export of `packages/engine/main/src/index.ts`, machine-inventoried in
  Phase 0.
- Built-in viewer entry point, tested by the viewer flows in
  `multiplatform-port-order.md` Section 7; engine-injected `/core` entry point
  free of PDFium symbols and WASM.
- SSR-safe imports; synchronous pure-TypeScript packages usable without async
  Rust init.
- `abort(reason?: unknown): void` signature and immediate rejection.
- Zod schemas stay handwritten: `packages/engine/core/src/wire/schemas.ts` and
  the 20 annotation schema files (`base.schema.ts` plus `kinds/*/schema.ts`),
  plus form, geometry, action and shared annotation-shape schemas. Rust types
  are canonical and TypeScript types are generated; schemas are maintained
  against them with bidirectional parity tests covering omitted/null/value,
  explicit undefined, fields that disallow null (such as
  `MetadataPatch.trapped`), unknown enums, numeric widths, nested unions, empty
  collections, invalid inputs and each schema's unknown-field behavior. Phase 0
  builds the harness; G7 and G11 add annotation DTO, draft and patch coverage as
  those families migrate.

Legacy backend stays selectable at open during preview and through Phase 5 for
rollback. Removal requires web conformance and performance acceptance, passing
the Phase 5 server migration, and explicit retirement of the TypeScript
fallback. Its runtime memory helpers, generated `PdfFunctions` and
`PdfRuntimeMemory` remain maintained until then. Critical fixes land in both
paths with a shared fixture.

## 12. Phases

Phases are dependency milestones, not dates. Distribution preparation in Phase 1
can run alongside Phase 0; its smoke apps depend on the G0/G1 foundation.
Distribution is a schedule risk, not a measured critical path. Phases 2 and 3
overlap once the read contract stabilizes.

### Phase 0. Foundations and baseline

**Scope:** Cargo workspace; `pdfium-sys`, `pdfium`, `epdf-engine` skeleton; Rust
linked into the shipping web worker; `pages.geometry` through Stage A; Stage B
bridge spike over `annotations.create`; lifecycle, buffer, coordinate and
cancellation contracts specified, with invariants exercised by the spikes; DTO
codegen proven on `MetadataPatch` (three-state `Keep | Clear | Set(T)`,
serialization/validation vectors on each host; operation parity lands in G14);
operation matrix for all 65 `WorkerRequest` request kinds; corpus schema and
geometry runner in `tooling/engine-conformance/`;
`.github/workflows/engine-conformance.yml` wired; end-to-end baselines recorded;
native `text` and readback rows, viewer allocation, and cold-start reruns;
`tooling/viewer-e2e/` harness with flows F1 through F5.

**Exit gate:** `pages.geometry` at differential parity with
`PageGeometryReader`, measured through the service boundary; bridge spike passes
failure-after-write, cache invalidation, close-under-lease; clean-checkout build
with actual Binaryen version and Rust/Emscripten ABI settings checked; regex
dialect/cost candidate chosen; the three missing cloud conformance suites wired;
matrix and contracts written; viewer flows F1 through F5 green on the TypeScript
backend, which is the baseline every later phase compares against.

### Phase 1. Reproducible distribution

**Scope:** Fork release with `ios-arm64`, `ios-sim-arm64`, `android-arm64`,
`android-x64`, `wasm32-eh` (currently `"status": "pending"` at pin
`d7b4caa26289d4846e94ff54e63b586b9bbe9fd6`; local fork commit `7f0fa5dbe` builds
all five); `use_custom_libcxx=false` + `use_clang_modules=false`; Android
`libc++_shared.so`, shrinker rules, 16 KB ELF and APK alignment checks for all
native dependencies, plus a 16 KB emulator run; Apple XCFramework in a Swift
Package; minimal native session, open, password, close, geometry, render and G4
font registry for smoke apps.

**Exit gate:** Clean-checkout builds from published artifacts only; real linked
EH module tested in release Chrome, Firefox, Safari; fresh simulator and
emulator apps; signed physical-device run.

### Phase 2. Read services

**Scope:** All read operations via Stage A on web and Rust-owned session on
native, in the order in `multiplatform-port-order.md` groups G2 and G4 through
G9 (G3 is the session group; the Rust session is native-only until the internal
G3w rehearsal in Phase 3). Complete G4 engine-level font ownership and
coexistence gates. Search has an entry gate: dialect vector file for `regex.ts`
and `fold.ts` semantics. Throwaway device prototype on the oldest supported
iPhone and a low-end Android measuring the pixel delivery path.

**Exit gate:** Read conformance green on both backends; native runners agree;
raw-render pixel, tile, rotation and buffer-lifetime tests pass; cancellation,
memory and lifecycle stress pass; native headless SDK preview usable;
concurrent-engine soak green or one engine per process enforced in the API;
viewer flows F1 through F8 green on both backends, F9 on TypeScript; G3w adds
transport-level Rust coverage in Phase 3, and full Rust viewer coverage follows
in Phase 4.

### Phase 3. Native reader products

**Scope:** SwiftUI/UIKit and Compose/View readers: continuous scroll, pinch
zoom, virtualization, selection and copy, search navigation, links, annotation
display, accessibility, file lifecycle. Behavioral references:
`packages/plugin/render`, `plugin/selection`, `plugin/interaction`,
`framework/web`, `plugin/search`, `core/stage`, `plugin/view-manager`.
Backgrounding, memory warnings, rotation, process recreation policies. G3w
separately rehearses Rust session open/close on web, before Stage B.

**Exit gate:** Physical-device checks; no PDF work on the UI thread; installs
from distributable artifacts alone; soak gate as Phase 2.

### Phase 4. Session authority, then writes

**Scope:** Stage B alone, then Stage C by family: annotation writes; 10 mutating
`forms.*`; page assembly; `redaction.apply`; `metadata.update`;
`pieceInfo.update` / `clear`; `attachments.create` / `delete`; save and export.
Font ownership already moved in G4. Cost: worker ships both implementations
through Phase 5 for rollback; the final family flip does not permit deleting
TypeScript runtime plumbing.

**Exit gate:** Mutation, save-and-reopen, document-event and revision
conformance green on both backends; cancellation-after-apply and partial-failure
tests; no dual authority; viewer flows F10 through F14 green on both backends,
each saved and reopened in a fresh viewer instance through the UI.

### Phase 5. Scale, isolation, server

**Scope:** Worker-pool scaling behind TSAN soak
(`testing/tools:epdf_thread_soak`, `EMBEDPDF_TSAN=1`), sticky routing, load
numbers; mobile crash-containment assessment; server milestone:
`cloudpdf/server/src/runtime/worker-entry.ts:18` hosting the Rust crate via
`napi-rs` on Linux and macOS with supervised host, `QuarantiningEnginePool`,
`EngineRecycler`.

**Exit gate:** Budgets validated under load; server conformance and rollout
validation green. Until then `engine-services` stays retained in the ledger.

### Phase 6. v4 rollout

**Scope:** Ledger complete; migration guide from actual changes; default web
backend switched only where parity and performance are demonstrated; bridges
removed; `engine-services` and legacy runtime plumbing removed only if Phase 5
server milestone passed and the TypeScript fallback is retired; rollback
rehearsed.

**Exit gate:** v4 published; every viewer flow F1 through F14 green on the Rust
backend before the default web backend moves.

A mobile reader release may precede native editing and the web default switch.
Documentation says so. The web default does not move because a mobile milestone
passed.

## 13. Deferred

| Item                                                                                       | Trigger to revisit                                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Interactive annotation editing on mobile; port of `packages/core/annotation` (7,527 lines) | Product decision. If yes: `update(msg) -> [model, effects]`, `scene(model, view) -> DisplayList`, one call per event and per frame; web keeps TypeScript unless measured against an optimized baseline on ink, cloudy borders, snapping, hit-test misses. |
| Shared tile planner                                                                        | Separate proposal naming consumers, ownership and measured benefit.                                                                                                                                                                                       |
| Coarse `EPDF*` wrappers that only collapse JS crossings                                    | Retire per caller once a coarse Rust op replaces every caller. Keep helpers that save work inside PDFium (`EPDFText_GetCharGeometry`: oriented quads, UPRIGHT, SYNTHESIZED flag). Do not add `EPDF_GetPageGeometryFull` if Phase 0 is close.              |
| `packages/core/js-sandbox` replacement via `rquickjs`                                      | After a native sandbox and behavior contract are validated. Mobile reports unsupported scripts until then.                                                                                                                                                |
| Windows and musl Rust Node targets                                                         | After the first mobile release; retain their existing TypeScript runtime until separately validated.                                                                                                                                                      |
| Mobile process isolation                                                                   | Phase 5 assessment; not a reader prerequisite.                                                                                                                                                                                                            |

## 14. Open risks

| Risk                                                                                  | Why it is open                                                                                                                                                                                                                                                                                                            | Mitigation                                                                                                                                                                                                                                                                                                                                  |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 25,431 source lines in the candidate inventory, including 16,620 in `engine-services` | The prototypes do not port these complete service families                                                                                                                                                                                                                                                                | Stage A buys the read win without betting the schedule on it. `multiplatform-port-hazards.md` lists the designs that need rework.                                                                                                                                                                                                           |
| Cross-language corpus cost                                                            | Never built one here; the prototype has operation tests, colored vectors and mobile smoke runs, but no complete engine corpus                                                                                                                                                                                             | Phase 0 delivers format plus one case; Phase 2 delivers runners. Skipping it means mobile shipped on smoke tests.                                                                                                                                                                                                                           |
| Mobile performance                                                                    | Simulator and emulator smoke runs exist; no representative physical-device performance budget has been validated                                                                                                                                                                                                          | Phase 2 device prototype before Phase 3 is budgeted.                                                                                                                                                                                                                                                                                        |
| The web viewer suite is new infrastructure and the usual home of flake                | 73 test files across `packages/plugin/*`, `packages/framework/*` and `packages/viewer/*`, none in a real browser, and none at all under `packages/viewer/*`                                                                                                                                                               | One flow per user-visible capability, synchronization on engine events/UI state with bounded failure deadlines, Chromium and Firefox per pull request with WebKit on the scheduled job, quarantine needs an open issue and expires in one release. F1 through F5 land in Phase 0 so the baseline exists before the port can hide behind it. |
| Facades thicken                                                                       | "No document semantics" is looser than "no decisions"                                                                                                                                                                                                                                                                     | Corpus runners call the facades, so facade drift is a test failure.                                                                                                                                                                                                                                                                         |
| `EMBEDPDF_TLS_GLOBALS` safety under two engines                                       | Assumed from the server arrangement, not tested for mobile                                                                                                                                                                                                                                                                | Soak gate before any native artifact ships.                                                                                                                                                                                                                                                                                                 |
| The proposed web deliverable uses one Emscripten module with WASM exception handling  | The historical audit counted 441 `try`, 367 `catch_all`, 285 `rethrow`, 0 `try_table`. Recorded passes are Chrome 152.0.7977.83 and Playwright Firefox 132.0 Nightly. A Nightly pass says nothing about release Firefox, and Safari is untested. There is no fallback if a release browser we support refuses the module. | Phase 1 decision gate: test release Firefox and Safari on real devices before the browser floor is announced. If either fails, price an alternate-EH build for both maintenance and payload before committing to it, and record the payload delta the way Section 3 records the +3,802 bytes. Do not announce a floor from Nightly results. |

## 15. Official stack references and remaining proofs

These references support the design constraints; they do not certify a
particular local artifact. Pin the chosen versions and record build/test
receipts in each phase’s implementation plan.

**Rust/Emscripten ABI and exceptions:**
[Rust target guidance](https://doc.rust-lang.org/stable/rustc/platform-support/wasm32-unknown-emscripten.html)
describes ABI sensitivity and matching/rebuilding std.
[Emscripten longjmp](https://emscripten.org/docs/porting/setjmp-longjmp.html)
explains compatible EH settings. Phase 0 verifies the exact linked combination;
Phase 1 tests the actual artifact in release browsers.

**Link configuration and confinement:**
[Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-arg)
scope linker arguments to package targets.
[UniFFI objects](https://mozilla.github.io/uniffi-rs/0.32/types/interfaces.html)
require thread-safe exported objects;
[async bindings](https://mozilla.github.io/uniffi-rs/0.32/futures.html) and
[napi async functions](https://napi.rs/docs/concepts/async-fn) are the
caller-facing mechanisms to evaluate. PDFium handles remain on their owner
thread.

**Serialization:** [Serde field attributes](https://serde.rs/field-attrs.html)
and [enum representations](https://serde.rs/enum-representations.html) define
defaults, omission and tags.
[ts-rs overrides](https://docs.rs/ts-rs/latest/ts_rs/derive.TS.html) are needed
where custom serialization differs from the Rust type. Phase 0 proves
absent/null/value and exact wire names; later groups extend the fixtures.

**Regex correctness and cost:**
[ECMAScript word characters](https://tc39.es/ecma262/multipage/text-processing.html#sec-runtime-semantics-wordcharacters-abstract-operation),
[Rust regex](https://docs.rs/regex/latest/regex/) and
[regress](https://docs.rs/regress/latest/regress/) document distinct semantics
and execution models. The Phase 0 dialect/budget spike must choose a viable
implementation before G5 begins.

**FFI, memory and packaging:**
[Rust trait objects](https://doc.rust-lang.org/reference/types/trait-object.html)
explain data/vtable pointers;
[catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html) covers
Rust unwinding only.
[Memory growth](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/Memory/grow)
affects host views;
[Emscripten threading](https://emscripten.org/docs/porting/pthreads.html)
requires a distinct shared-memory configuration.
[Android 16 KB guidance](https://developer.android.com/guide/practices/page-sizes)
requires checking ELF alignment, packaging and runtime behavior of every native
dependency, including Rust and C++ libraries.
[Apple XCFramework guidance](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle)
covers the multi-platform bundle; device signing and execution remain separate
gates.
