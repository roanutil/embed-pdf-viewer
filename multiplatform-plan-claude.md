# Multiplatform EmbedPDF: a Rust core with native wrappers

> Superseded by `multiplatform-plan-claude-remix.md` (2026-09-09). Kept for the detail it carries.
> Where the two disagree on phasing, counts or sequence, the remix is correct. Known stale here:
> the operation counts below, and the phase assignments that `multiplatform-port-order.md` revises.

**Goal:** One Rust implementation of the PDF engine, consumed by web through the existing WASM
worker, by iOS through a Swift package, and by Android through a Kotlin library, without
reimplementing engine behavior per platform. Plus a usable native reader on both mobile platforms,
which is a separate deliverable from the engine and roughly as large.

**Architecture:** Rust owns everything from PDFium's C ABI up through the 61 document operations in
`packages/engine/core/src/wire/worker-protocol.ts`. TypeScript keeps everything above that line on
web: the kernel, the annotation brain, the plugins, the framework adapters, the viewer. The native
SDKs are handwritten Swift and Kotlin facades over a `uniffi` ABI. Migration moves in three
ownership stages, not operation by operation: a borrowed-handle bridge for coarse reads, then
document session authority as one atomic move, then Reader and Mutator families behind unchanged
public services.

**Tech stack:** `rustc` 1.98.0 with `edition = "2024"`, `bindgen = "=0.72.1"`, `uniffi = "=0.32.0"`,
`napi = "=3.12.2"`, `ts-rs` for the canonical DTOs, `cargo-ndk` 4.1.2, emsdk 3.1.72 with Binaryen 132
selected through `EM_BINARYEN_ROOT`, Swift 6.3.3 / Xcode 26.6 (17F113), Kotlin 2.2.20 / AGP 9.4.0 /
NDK 29.0.14206865, Node 24.20.0, pnpm 10.34.0.

## Global constraints

Every phase inherits these. They are not restated per phase.

One owner per resource and per state transition. At no point may TypeScript and Rust both hold
authority over a document's handle, page cache, revision counter or mutation sequence. This is the
constraint that shapes the migration, and it is stated first because it is the one that kills a plan
if it is discovered late.

The JavaScript-to-Rust boundary is per operation, never per item. In the committed WASM table
(`pdfium-binding-benchmarks/RESULTS.md:169`), glyph geometry costs 2.788 ms through per-item calls
and 1.070 ms through one coarse operation, and search costs 102.36 µs against 30.61 µs. The native
table (`RESULTS.md:415`) puts geometry at 4.134 ms against 577.37 µs between two arms of the same
`napi-rs` addon. No convenience API may reintroduce a per-item call.

No Rust export taking `&mut self` may call back into the host. Doing so in
`viewer-rust-port-benchmarks/rust-geometry-and-state` produced `attempted to take ownership of Rust
value while it was borrowed` on `free()`, surfacing far from its cause.

Public bindings hand out opaque handles carrying an engine generation, never raw pointers. A handle
from a replaced engine must fail rather than resolve against new state. Pointer representation is an
internal detail of the runtime binding, not part of any consumer API.

PDFium handles never cross threads. Native builds set `EMBEDPDF_TLS_GLOBALS=true` and each engine
owns one dedicated OS thread; `wasm32` and `wasm32-eh` leave it off because each instance isolates
its globals in its own linear memory. This is the contract in
`.agents/skills/embedpdf-conventions/references/runtime-confinement.md`, and the prototype's
`crates/epdf-ops/src/dispatch/{threaded,inline}.rs` is the two-target implementation of it.

A page has two text index spaces, and the port carries both.
`packages/engine/core/src/text/charmap.ts` is the one place they meet. CHARACTER space is PDFium's
internal character list: the space geometry runs tile (`PageGeometryRun.charStart`), hit-testing
addresses and selection ranges live in, sized by `PageTextSnapshot.charCount`. TEXT space is UTF-16
code-unit offsets into the extracted string, which is what search matching and slicing operate on.
They diverge in exactly two ways, both encoded by `PageTextSnapshot.charMap`: a non-printing character
occupies a character slot and contributes zero text units, and a supplementary-plane character
contributes two. So every boundary DTO says which space it is in, and the Rust port carries
`charmap.ts`'s conversion semantics rather than reinventing them, including the shared
`charMapViolation` validator that makes a malformed map a rejection instead of a silent repair.

The fixtures cannot establish that contract today.
`pdfium-multiplatform-prototype/vectors/report-page4.json` has `charCount` 8,106 over a `text` of
exactly 8,106 UTF-16 units and no `charMap`, so the two spaces coincide and every reading of the
number passes. (Which is why `pdfium-multiplatform-prototype/README.md:343` calls 8,106 "UTF-16 code
units". On that page it is true. It is not the contract.) So the text slice in Phase 2 does not start
until the corpus holds fixtures with surrogate pairs and with non-printing characters, where
`charCount !== text.length` and `charMap` is non-empty. Swift's default `String.count` gives 7,992 on the
same page, counting grapheme clusters, which is a third thing again: a Swift convenience returning
`String.Index` ranges maps through a checked conversion or does not ship.

The Rust definition is canonical for internal wire DTOs, generated into TypeScript and checked in;
a dirty diff fails CI. Public web DTOs that intentionally differ keep an adapter. The generated
serialization layout is never itself a public API, and the protocol carries a version so mismatched
JS, WASM and native artifacts are detected rather than misread.

Rust source lives in a Cargo workspace at the repository root (`Cargo.toml`, `crates/`), and native
SDK sources under `platforms/apple/` and `platforms/android/`. Neither tree is in the npm namespace,
which needs one line added to `.agents/skills/embedpdf-conventions/references/naming.md` alongside
the existing carve-outs for `examples/` and `tooling/`.

Each phase gets its own plan under `docs/superpowers/plans/` with checkbox steps, written before that
phase starts. This document is the program, not the task list.

---

## What the three spikes settled

I went into these expecting the port to be an optimization. It isn't, and each spike killed a
different version of that belief.

Read them from the committed result tables, not from the summaries in `docs/research/`. Those
summaries carry figures from earlier sittings that the committed tables have since superseded.
Every number below is from
`pdfium-binding-benchmarks/RESULTS.md` or `viewer-rust-port-benchmarks/bench/RESULTS.md`.

`pdfium-binding-benchmarks` said the cheap thing is cheap and the expensive thing is the boundary. A
thin Rust frame in front of PDFium costs 1 to 4 percent in WASM and 3,802 extra bytes on a 4,676,714
byte module, measured from the worst side of it, with JavaScript reaching arm b through `cwrap` where
the real core will call straight through. So `pdfium-sys` is not a thing to optimize. Build it the
boring way. What costs is the crossing count: `PageGeometryReader` makes two PDFium calls per glyph
plus up to 21 `mem.peek` reads to decode each struct, so page 4 of
`examples/snippet-react/public/report.pdf` with its 8,106 glyphs runs about 25,000
JavaScript-to-PDFium crossings today, and collapsing that loop took geometry from 2.788 ms to
1.070 ms, a 2.6x win. Whether Rust or C++ owned the loop was indistinguishable, `d/c` between 0.962
and 1.048.

Two qualifications on that win, both from the same tables and both load-bearing for the phase
ordering. It is not general: `text` lands within 2 percent across all four arms because
`PageTextReader.ts:93` is already coarse, and `render-1x` and `render-4x` are dominated by PDFium's
own bitmap work, so coarsening buys nothing on either. And the native half needs reading carefully
rather than quoting. `RESULTS.md:3` says the committed native measurements predate the readback-copy
fix in `bench/arm.mjs`, that arms b/c/d paid an extra JavaScript copy in `readBytes` and `readU16`,
and that native comparisons involving those paths need a fresh interleaved run. That qualifier is
doing real work: `geometryThin` reads through `readGlyphRaw` (`bench/workloads.mjs:239`) with
`mem.peek32` and `mem.peekF32` scalars, `geometryCoarse` (`:307`) takes a zero-copy `mem.viewBuffer`,
and `searchThin` is PDFium calls plus four `peekF64` per rect. `readU16` appears only in the text
workload (`:380`, `:385`) and `readBytes` only in render-copyout (`:411`, `:416`) and the three
copyout probes. So the `geometry` and `search` rows stand and the `text` and readback rows are the
ones awaiting a rerun. Native geometry is 4.134 ms in arm b against 577.37 µs in arm c
(`RESULTS.md:415`), a 7.16x boundary win measured between two arms of the same `napi-rs` addon, which
is the comparison that isolates the boundary rather than the binding framework. Native search wins
only 1.62x against WASM's 3.3x (`RESULTS.md:541`), so the win is workload-specific in both
directions.

`viewer-rust-port-benchmarks` said the opposite about cheap math. The Rust-owns-state arm won none of
20 frame cells. At 1,000 anchored shapes with a projecting view, TypeScript took 121.96 µs and Rust
took 200.76 µs for a frame that crossed the boundary once against TypeScript's zero
(`bench/RESULTS.md:26`). Returning the raw `Float64Array` without materializing `DisplayList`
objects still left Rust at 171.43 µs, 1.41x behind. The decode share is workload-dependent rather
than a headline: 15 percent in that cell, 45 to 51 percent in the plain and anchored-flat cells at
n≥1000. Across n≥10 the Rust-with-state arm ran 1.60x to 3.52x slower; the n=1 cells reach 10.57x.
The fine-grained arm won 0 of 20 and ran 5.06x to 7.72x slower on frames.

So here is the rule those spikes hand us, and it is the only rule that matters for deciding what to
port: put Rust where the compute behind one crossing is large. Twenty-five thousand PDFium calls
behind one crossing is a 2.6x win. Four multiplies and a `Math.max` behind one crossing is a 3x loss.
An already-coarse operation like `text` is neither.

(One measured caution on the second spike, because it is easy to over-read. The toy's per-shape math
is a handful of multiplies, which is precisely the shape V8 optimizes best. `cloudy.ts` is 570 lines
and ink resampling is not four floats and a winding test. The result does not say Rust would lose on
the real annotation workload. It says the boundary plus JavaScript object materialization eats any
win on math that cheap.)

`pdfium-multiplatform-prototype` said the binding surface works, and then told us what the test was
not catching. Six code paths, five of them crossing a language boundary, all rendered page 4 into a
121,176-byte BGRA buffer matching a golden generated by the shipping TypeScript runtime at mean
absolute difference 0 and max 0. But page 4 is entirely grayscale, B equals G equals R in all 30,294
pixels, so transforming the golden with a B-to-R swap and re-running the same diff also scores mean
0.0 and max 0. A grayscale golden cannot tell a correct BGRA read from an RGBA-for-BGRA one. And the
shipping tolerances of mean 2 and max 16 catch an all-white buffer at 16.78, an all-black one at
238.22, a one-pixel horizontal shift at 6.12 and a zeroed alpha channel at 63.75, but let a uniform
6-level darkening of every non-white byte through at mean 1.74 and max 6. That is the signature of a
wrong color space or an unapplied gamma curve.

Which is why the verification section is longer than any phase.

## Where the line goes

Rust owns PDFium through the engine's operation surface. That is `crates/pdfium-sys`, a safe
`crates/pdfium`, and then a port of the 16,620 source lines in `packages/engine/services` plus the
engine-behavior parts of `packages/engine/core`: text layout, the search fold and cursor, DTO
encoding, error codes.

TypeScript keeps `packages/core/*`, all 17 plugins, `packages/framework/*` and `packages/viewer/*`
on web, unless a measured workload justifies moving a specific piece. That is deliberately a weak
claim: the frame tables that argue against porting
`packages/core/annotation` (7,527 lines) come from a rectangle toy, and the annotation core's real
per-shape work in `cloudy.ts`, `ink.ts` and `hit.ts` is nothing like four multiplies. The default is
TypeScript and the burden of proof is on moving a piece, measured against an optimized TypeScript
baseline rather than against the current one. `packages/core/main` and `packages/core/stage` carry
the strongest presumption against: the store is the one piece of infrastructure every plugin loads.

`worker-host/WorkerHost.ts` is mostly excluded, and "mostly" is load-bearing.
Most of its 1,459 lines are web transport, and on mobile the transport is a direct function call. But
`finishMutation` at `packages/engine/services/src/worker-host/WorkerHost.ts:1380` is not transport. It
owns the mutation-sequence bump that invalidates version-keyed caches, the per-tag exemptions for
`forms.*`, `pages.flatten` and `redaction.apply` that already bump themselves, and layer-artifact
persistence. That is engine behavior living in a file named after its transport. It ports with the
write families in Phase 4, with an exactly-once test, rather than being left behind with the message
loop.

Which points at the rule a package-shaped boundary hides: draw the boundary by
responsibility. Permission enforcement is the other thing that gets lost otherwise, because it does
not all live in `engine/services`. `packages/engine/main/src/document/LocalPageTextService.ts:48`
calls `this.guard.assertCapability('doc.text.copy')` before it enqueues `pages.text`, mirroring the
cloud route's `requireResource`. A native host calling the ported worker operation directly walks
straight past it. So the port inventory names an owner for each of permission enforcement, mutation
finalization, layer-artifact persistence and document-event publication, and native denial tests are
part of the read-services exit rather than something to notice after a security review.

What mobile cannot inherit is the presentation layer, and this is the part an engine-shaped plan
hides. Tile scheduling lives in `packages/plugin/render` (2,013 lines), selection in
`packages/plugin/selection` (1,325), gestures and input in `packages/plugin/interaction` (583) with
its DOM half in `packages/framework/web` (1,817), search navigation in `packages/plugin/search`
(634), viewport and page virtualization in `packages/core/stage` (919) and
`packages/plugin/view-manager` (405). None of it reaches a device with no JavaScript runtime, so all
of that behavior gets written twice natively, in SwiftUI/UIKit and in Compose/View. Call it
comparable in size to the annotation core, and unlike the annotation core it is not optional if the
goal is a multiplatform viewer rather than a multiplatform engine. It gets its own phase.

Node and the server share the behavior but not the delivery. `@cloudpdf/server` already runs
`engine-services` in a `worker_thread`, so a ported service reaches it through the same crate, and
the nine native targets in `packages/engine/runtime/npm/` reach it through `napi-rs`. That is the
easy half. Packaging, target coverage including the never-linked Windows and never-run musl arms,
and the server's own isolation and rollout validation are real work with their own gates, and none of
it belongs in the first mobile release.

The honest reason to do any of this is code sharing, not web performance. Web gets faster in the
geometry and search loops and is expected to be roughly neutral elsewhere, but "slower nowhere" is an
aspiration until Phase 0 captures a baseline and Phase 2 measures against it. iOS and Android
otherwise need a second implementation of 16,620 lines where PDF spec correctness lives, and that
second implementation will drift. That is the whole argument.

## The seam we already have

Almost everything that makes this affordable was built for other reasons.

`Engine` and `DocumentHandle` in `packages/engine/core/src/engine/` are already implemented twice, by
`LocalEngine` over a worker and by `@cloudpdf/engine` over HTTPS, and the interface's own doc comment
says the only observable difference is transport. A Rust-backed engine is the third implementation of
an interface that already proved it can have more than one.

`WorkerRequest` at `packages/engine/core/src/wire/worker-protocol.ts:866` is a 63-member union
expanding to 65 request kinds (`OpenWorkerRequest` covers three `open.*` kinds), four of which
(`close`, `layer.close`, `abort`, `shutdown`) are control frames. One message per operation.
That is already the coarse shape the benchmarks say to build.

And `packages/engine/core/src/conformance/` holds 21 transport-agnostic suites taking
`makeEngine: () => Engine`, including 2,431 lines in `runAnnotationMutationConformance.ts`.

## Ownership during coexistence

This is where an incremental port actually succeeds or fails. "Some operations are now Rust" is not
a migration plan. `DocumentSession`
(`packages/engine/services/src/document-session/`, 832 lines) owns document bytes and lifetime, page
identity and leases, the revision counter, the mutation sequence and password-locked state. Two
languages cannot each believe they own those.

Stage A, coarse reads over borrowed handles. TypeScript's `DocumentSession` stays the sole owner of
lifecycle and revisions. Rust gains private coarse functions in the same worker module that borrow a
validated document or page handle synchronously for exactly one operation. They never retain it,
never close it, and never construct a second owning wrapper around it. Any transient they allocate is
released before return, and a read that would update a cache or weak-annotation knowledge returns
that information for the session owner to apply.

One session-owned native handle already sits outside "release before return", and it decides whether
that rule is followable. `formModelCache.ts:29` keeps one `EPDFForm_LoadModel` snapshot per session,
keyed on the mutation sequence, because rebuilding it walks the field tree and every page's `/Annots`.
And `features/annotations/internal/read/joinWidgetField.ts:23` means annotation reads use it, not
only `forms.list`. Stage A does not let Rust create or retain that model. TypeScript's session keeps
owning it, exactly as it owns the document pointer, and lends the model pointer to a Rust read
alongside the document handle for the duration of one call. Rust calls neither `EPDFForm_LoadModel`
nor `EPDFForm_CloseModel` under Stage A. The Phase 2 slices that touch it, annotation reads and the
two form reads, carry a repeated-read benchmark against today's cached path, because a Rust read that
quietly rebuilt the model per call would pass conformance and lose the operation.

This path is internal, hidden from every consumer
package, tested for close-and-borrow ordering, and forbidden from crossing a worker or WASM instance.
Cancelling a Stage A read cancels the waiting, not the execution: a chunked operation would need state
retained across a yield, which is exactly what Stage A forbids.
Native hosts do not use it: they get a Rust-owned session over the same service code from the start.
Stage A buys the coarse-boundary win on reads without making full engine parity a prerequisite for
anything.

Stage B, session authority moves as one change. Document bytes and lifetime, the page registry and
leases, revision authority, the mutation sequence, password-locked state and retained import
dependencies move to Rust together, and the TypeScript `DocumentSession` becomes a facade over it.
Remaining TypeScript services then reach the runtime through a private synchronous bridge that leases
a handle for one scoped operation, refuses reentrancy, and finalizes every mutation outcome through
Rust. That bridge exists only for legacy web code and carries a written removal criterion. If scoped
borrowing cannot be made safe for a feature, that whole feature stays on the legacy backend until its
dependency cluster ports.

That bridge is Stage A run backwards, TypeScript borrowing from Rust instead of Rust borrowing from
TypeScript, and every write family in Phase 4 rides on it. So it does not wait for Phase 4 to be
tried. Phase 0 proves it as a throwaway spike over one legacy mutation, `annotations.create` through
the existing `AnnotationMutator`, against a document that Rust opened and owns: the lease, the write,
a failure injected after the native write, the form-model cache at
`packages/engine/services/src/features/forms/internal/formModelCache.ts:29` invalidating on the
sequence bump, and a `close` arriving while the lease is out.

How the lease avoids the `&mut`-across-callback problem from the global constraints: Rust hands
TypeScript a lease token and a pointer copy, never a borrow of its own state, and holds no exclusive
borrow while host code runs. Checkout, return and finalization are three separate synchronous entry
points on the Rust session, each taking the borrow for its own duration only, and a second checkout
while one is outstanding is a rejection rather than a wait. A `close` with a lease outstanding marks
the session closing and completes on lease return, never underneath it. If the spike cannot make that
ordering hold, this plan finds out before the read port instead of after it. The cost is a Phase 0
that already carries the lifecycle contract growing by a spike whose code is discarded.

The rule Stage B exists to enforce: never route reads to a newly opened Rust document while writes
continue against a separate TypeScript one, and never switch an open, edited document between
backends. Backend selection happens at open. Rollback means selecting legacy for subsequent sessions,
or an explicit save and reopen with the user aware of it.

Stage C, Reader and Mutator families retire in whole clusters behind unchanged public services. Rust
owns validation, native work, mutation impact, cache invalidation and revision publication;
TypeScript keeps request orchestration and presentation. Each old implementation and its bridge
access is deleted when parity and performance gates pass for its whole family, not per operation.

There is still a routing table, and it is narrow. It selects a
backend per family at document open. An entry becomes flippable only once the ownership rule for the
resources that family touches exists and is tested, which for every mutator means after Stage B.

Where a pure algorithm ends up implemented twice on purpose, web geometry against Rust geometry for
instance, it gets shared vectors, a named owner and a written exit criterion: switch web when
measured budgets pass, or keep the platform-specific one deliberately. Nothing gets made slower on
web to reach a percentage.

## Cancellation

`abort()` at `packages/engine/core/src/promise/AbortablePromise.ts:91` returns `void`, rejects the
public promise synchronously with an `AbortError`, and fires the signal. That is cancellation of the
caller's wait, it already works uniformly, and nothing here changes it. Rejecting with a distinct
code for uncancellable operations would be a needless break and a confusion of two different things,
so it is off the table.

What needs specifying is cancellation of execution, and there are four honest cases.

Queued and not started: drop the job, release its inputs, publish nothing. This works today through
`WorkerQueue` and is the only case that is currently reliable for a long operation.

Cooperative, before any write: the operation checks the token between units of work and stops. The
existing pattern is `throwIfAborted(signal)` peppered between PDFium calls
(`packages/engine/services/src/shared/abort.ts`, used at
`features/render/PageRenderReader.ts:23,29,50,52`), and it extends to per-page loops in search and
text and to PDFium's progressive render pause points. Mutations belong here too: a mutator is
cancellable right up to its apply boundary, and should be.

Non-preemptible: a single synchronous native call with no pause point, `FPDF_RenderPageBitmap` being
the obvious one. The wait ends; the work does not. This has to be documented per operation rather
than implied.

After the first write: bookkeeping completes. Per
`.agents/skills/embedpdf-conventions/references/engine-services.md`, these mutations are not
rollback-atomic, so a late cancellation must not introduce a throw path that pretends no write
occurred. Ordered batches report the current item `failed`, the rest `skipped`, and finalize changed
or indeterminate state.

The web constraint that makes tier boundaries real rather than aspirational: `WorkerHost.ts:199`
handles an `abort` message by calling `AbortController.abort()`, and the worker's message loop is
single-threaded. An abort arriving while a synchronous handler runs is not observed until that handler
returns, which is why `WorkerHost.ts:225` already notes that some operations are "intentionally
non-abortable". So cooperative cancellation inside one long WASM call needs either an operation split
across several messages so the loop returns to the pump, or a shared-memory flag the WASM side polls,
and a `SharedArrayBuffer` needs COOP and COEP headers that not every embedder can set.

The message-split option collides with Stage A, and the collision is better stated here than
discovered in Phase 2. Stage A borrows a handle synchronously for one operation and retains nothing.
A chunked operation is the opposite. It needs continuation state that survives between messages, and
the pump it yields to can deliver a `close` or a mutation before the next chunk arrives. Those do not
fit together, so the promise is per stage rather than global.

Under Stage A, web cancellation of a coarse Rust read is cancellation of waiting. The job is dropped
before it starts, or the caller's promise rejects while the native call runs to completion and its
result is discarded. Nothing chunks, because nothing can hold a lease across a yield. Under Stage B,
where Rust owns the session, chunking becomes expressible, and it ships only with a named continuation
owner, a page lease held by that continuation, a revision check on resume that fails the operation
rather than mixing revisions, and a close ordering that either drains outstanding continuations or
cancels them. Shared-memory polling stays the alternative for embedders that can set COOP and COEP.

Phase 0 picks one per operation family and writes down what the other choice would have cost. Guessing
here is how a cancellation contract ends up true only in Node.

## Native APIs that feel native

`uniffi` is the ABI, not the API. The prototype is why: `uniffi`'s Kotlin generator makes every object
`AutoCloseable` and emits its own `override fun close()`, so the exported `close` had to become
`closeSync`, and it gives every exception variant an `override val message`, so
`OpsError::EngineFailed`'s Rust field named `message` had to become `detail`. Both renames live in
`pdfium-multiplatform-prototype/packages/kotlin/uniffi.toml`. At six operations the Swift and Kotlin surfaces already diverge; at
sixty they will not read as the same SDK, and neither will read as something a platform engineer
would have written.

So each platform gets a handwritten facade. Swift:

```swift
let engine = try PDFEngine()
let document = try await engine.open(data: pdfData)

document.pageCount                                  // Int
let page = document.page(at: 3)
let image = try await page.render(scale: 2)         // CGImage
let text = try await page.text()                    // String, UTF-16 offsets preserved

for try await event in document.events { ... }      // AsyncSequence<DocumentEvent>

try await document.annotations.create(
  .highlight(quads: quads, color: .systemYellow)
)
```

Kotlin:

```kotlin
val engine = PdfEngine()
engine.open(bytes).use { document ->
    document.pageCount
    val bitmap = document.page(3).render(scale = 2f)   // android.graphics.Bitmap
    val text = document.page(3).text()

    document.events.collect { event -> /* Flow<DocumentEvent> */ }

    document.annotations.create(Annotation.Highlight(quads, Color.YELLOW))
}
```

`PDFDocument` is `Sendable` and its `async` methods bridge to the engine's own thread. Swift `Task`
cancellation and Kotlin structured concurrency reach the scheduler, not merely the await. For a
queued or cooperative operation, a cancelled `Task` that only stops listening while native work
continues is the failure mode to test for. For a non-preemptible call the wait ends and the work
completes, exactly as on web, and each facade method documents which class it is in rather than
letting the platform idiom imply more than the engine delivers. Errors
are a typed `PDFError` and a sealed `PdfException` hierarchy carrying the same `EngineErrorCode`
values the web engine returns. Cleanup distinguishes "rejects new work and schedules disposal" from
"all native resources released", and each helper says which it provides; on Android the scoped
suspending form must not block the UI thread.

The rule for what may live in a facade: it must not duplicate document semantics or shared business
rules. Platform coordinate conversion, image adaptation, accessibility behavior, scheduling and
platform-specific defaults belong there, and pushing them into Rust would recreate the fine-grained
crossings the benchmarks warn about. Anything that decides what a PDF means belongs in `crates/`. The
stricter rule, no decision of any kind in a facade file, is not followable and would push platform
choices into Rust.

Feature availability rides the existing capability twins from
`.agents/skills/embedpdf-conventions/references/permissions.md`, exposed as natural platform APIs.
Native UI reads the capability's answer instead of reinterpreting scope strings, engine and server
enforcement stay authoritative, and an unsupported mobile feature reports unsupported rather than
silently succeeding.

## The compatibility ledger

Every exported surface gets a row: unchanged, adapter-preserved, deprecated, or intentionally
breaking. The ledger is the deliverable, and the migration guide documents every actual change rather
than a list predicted at the start. A fixed break budget rewards under-documenting, so there is none.

The breaks we currently expect to be forced:

| Break | Why it is forced |
|---|---|
| Browser floor rises to WASM exception handling | `rustc`'s std for `wasm32-unknown-emscripten` lowers panics through native WASM EH, needing `-fwasm-exceptions`, and PDFium uses `setjmp`/`longjmp` for libpng and libjpeg errors (`core/fxcodec/png/pngmodule.cpp`, `core/fxcodec/jpeg/jpeg_common.c`). `em++` refuses both flags together, so the target becomes `wasm32-eh` with `-sSUPPORT_LONGJMP=wasm`. The floor is set from real-module testing in Phase 1, not asserted now. |
| Internal wire DTO shapes where generated Rust differs from hand-written TypeScript | Rust becomes canonical. Cosmetic mismatches get fixed on the Rust side; genuine differences keep an adapter so the public DTO does not move. |
| Direct `@embedpdf/engine-services` imports | Its contents move to `crates/`. This is a removal, not a shim: an empty package preserves nothing. Either the package keeps re-exporting the Rust-backed equivalents or it goes away and the ledger says so. |
| Web error surface unified on `EngineErrorCode` | Web exposes message strings on some paths where Node returns stable codes. Two engines cannot keep two error vocabularies. Code-based callers are unaffected; string matching breaks. |
| Documented execution-cancellation behavior per operation | Today the contract is silent about which operations stop native work. Publishing the four cases changes no code path and makes some current behavior visibly weaker than callers may assume. |

Preserved on purpose, and each needs a test rather than an intention: synchronous `localEngine()`
construction, `open()` returning `AbortablePromise<DocumentHandle>`, every `DocumentHandle` service
signature, the built-in viewer entry point, and the engine-injected `/core` entry point staying free
of PDFium symbols and WASM assets so a remote or custom engine pulls neither. SSR imports stay safe
by deferring worker and WASM initialization to a client lifecycle, and the synchronous pure-TypeScript
packages stay usable without async Rust init. Custom engines and `@cloudpdf/engine` get exercised
independently, and representative downstream integrations get typechecked.

Latest published stable is 2.15.0 and the current line is `3.0.0-next.11`, so this targets v4. The
Rust backend ships opt-in during preview with the legacy backend selectable at open, and its removal
condition is web conformance and performance acceptance, not a date. During overlap, critical fixes
land in both paths with a shared regression fixture.

## Verification

Four mechanisms. The third and fourth do not exist yet.

The 21 conformance suites run against both backends, using their existing `makeEngine` seam. This is
cheap and it is the behavioral oracle for geometry and orientation, text and search, annotation reads
and mutations and appearance, forms, page edits, redaction, attachments, piece info and document
events.

Differential execution is a test harness, not a production mode. Running both implementations
against one document behind an environment variable is wrong the moment a mutation is involved: two
backends over one document creates the annotation twice or deletes an already-deleted page, and
comparing their results compares different starting states. Instead the harness opens two documents
from identical bytes on independent sessions, replays the same operation sequence against both, and
compares results, emitted document events, revision sequences, and the structure read back after a
save and reopen. Bytes compare only where deterministic serialization has been established for that
operation, which as of this writing is nowhere. Production routing selects one backend at open and
never runs both.

Frozen vectors carry what conformance cannot, following `pdfium-multiplatform-prototype/vectors/`:
JSON written once by a generator driving the shipping TypeScript runtime, never regenerated during a
test run, read by suites in Rust, Swift, Kotlin, Node and the browser. Page counts, sizes, text and
search hits checked exactly; bitmaps by pixel diff rather than hash, because a hash pins float and
rasterization behavior across four architectures. The tolerances split: exact interior BGRA on a
colored fixture with explicit color-space expectations for channel correctness, and the loose
whole-page gate kept only for gross drift.

The cross-language conformance corpus is the main new investment, and without it mobile drifts. The
21 suites are TypeScript functions taking a TypeScript `Engine`, so Swift and Kotlin currently get
five and six smoke tests against sixty operations. Express each suite's cases as data, emitted from the
existing suites into `vectors/conformance/*.json`, with a runner per host reading that corpus through
the platform facade rather than through `uniffi`'s output, so a facade bug is a test failure. This is
the piece most likely to be skipped under pressure, and skipping it means shipping mobile SDKs
verified by smoke test.

A fixture plus an operation plus an expected result is not enough, which is worth saying because that
is the shape a corpus defaults to. It cannot express a mutation sequence, where step three operates on
the annotation step one created. It cannot express a returned identifier, because the id is assigned at
run time and differs per host. It cannot express event ordering, a cancellation race, or a lifetime
failure, which are assertions about what happened between steps rather than about a value.

So the format is a scenario: an ordered list of steps, each binding its result to a name that later
steps refer to, each carrying its expected result and the events expected to have been published by
that point. Nondeterminism gets injected rather than sampled. The suites do not do that today
(`runAnnotationMutationConformance.ts:1217` and `:1257` build contents strings out of `Date.now()`),
so a scenario names a clock and a seed and the runner supplies them.

Some things stay procedural host tests, and claiming otherwise would be worse than admitting it.
Concurrency, cleanup ordering, close-under-load and lease violation are properties of a host runtime,
not of a data vector. Saved documents compare semantically after reopening, by reading back the
structure the mutation was supposed to produce. Byte equality is an assertion only where deterministic
serialization has been established for that operation, and it has not been established for any of them
yet.

The fixture corpus has to grow with it: rotated and cropped pages, non-BMP and non-printing text (the
two things that drive `charCount` and `text.length` apart), combining and RTL text, large scanned
documents, complex vector pages, embedded fonts and ICC profiles, encrypted documents, forms,
attachments, malformed input, and dense annotation and ink pages.

And then the discipline the benchmarks taught, which is about not believing our own numbers. That
study found three of its own measurement bugs in development, each of which reversed or manufactured
a conclusion: a decoder allocating roughly 26,000 objects per iteration that only some arms paid, one
arm compiled at `-O0` against another's `-O3`, and arm order never rotated within a round. The
fairness gate caught none of them, because the answers stayed identical. The viewer spike separately
hid an O(n²) `rows.find` inside a per-item loop, invisible at demo sizes and 184x slower at 10,000
shapes. So: a differential answer gate before any timing, an explicit audit that both arms do equal
work at release optimization, rotated arm order within rounds, consumed results, retained raw
measurements stamped with source and toolchain, and medians and p95 rather than means. Include an
optimized TypeScript arm, not just the current one, before attributing a win to Rust.

## Performance gates

Microbenchmarks keep their place and lose their monopoly. The geometry and search loops get the
coarse-boundary gates they earned: no per-glyph host call in geometry or search, and geometry beating
the current TypeScript reader in WASM by a margin set from the 2.788 ms to 1.070 ms committed result,
and on Node from the 4.134 ms to 577.37 µs one. Set both gates below the measured ratios so a
correctly built port passes with headroom rather than needing to reproduce a benchmark. No gate rides
the `text` or readback rows until those are rerun.

Release gates on end-to-end behavior instead: open-to-first-visible-page latency, sustained scroll
and pinch frame timing with missed-frame and input-to-presentation counts, search latency across the
corpus, peak and steady-state memory across repeated open, render and close, and the full pixel
delivery path measured in its separate parts (WASM-to-host copy, worker transfer, channel conversion,
GPU upload). Mobile numbers come from real devices, old and current, cold and warm, with thermal and
battery behavior recorded. Desktop Node timings are not a mobile budget.

Web regression policy: investigate anything repeatable past 5 percent on a meaningful end-to-end
workload, and block a default-backend switch past 10 percent on p95 open, first-page or search
latency or on peak memory, unless a product budget explicitly replaces that threshold. Absolute
floors alongside the percentages, so a 40 percent regression on a 200 µs operation does not veto a
release.

Also tracked because they are what users feel on first load: compressed JS and WASM size, compile
and instantiate time, native download and install size, and uncached first useful paint on a
controlled network.

CI separates fast core and conformance tests from platform binding and packaging tests, from real
browser and simulator and emulator jobs, from scheduled device, performance and soak runs. The
browser matrix includes release Chrome, Firefox and Safari, and tests the real linked
exception-handling module rather than a small probe.

---

## Phase 0: foundations, lifecycle contract, and a baseline

Exit: the shipping `@embedpdf/engine` worker answers one operation, `pages.geometry`, through the
Stage A borrowed-handle bridge at differential parity with `PageGeometryReader`; one legacy
mutation, `annotations.create`, finishes through the Stage B bridge spike over a Rust-owned document
with failure-after-write, cache invalidation and close-under-lease exercised; both reproducibly from
a clean checkout, with the lifecycle contracts implemented rather than promised, the operation matrix
written, and today's end-to-end performance recorded.

`pages.geometry` first because it is where the measured WASM win lands. Its internal representation
can be a flat numeric buffer that shared vectors pin exactly. Its public result is not one.
`PageGeometrySnapshot` is a discriminated union of upright and rotated runs, each holding an array of
glyph objects with an optional `tightBox` or `tightQuad` and an optional `fontSize`, and
`packages/engine/services/src/features/geometry/PageGeometryReader.ts:86` materializes all of it on
every read. So the packed buffer is an internal encoding with a decode on the far side, and the Phase 0
measurement runs through the unchanged service boundary with grouping, run materialization, transfer
and allocation counted. Until that number exists, the benchmark ratios are a hypothesis about the inner
loop rather than a claim about the operation.

The crates follow the prototype's split, keeping only what survives it. `crates/build-support` holds
shared `build.rs` logic, and it exists because Cargo does not propagate a dependency's
`cargo:rustc-link-arg` to a dependent's test or bin targets, only `rustc-link-lib` and
`rustc-link-search`. `crates/pdfium-sys` is `bindgen` 0.72.1 over the same fork headers
`packages/engine/runtime/build/generate-functions.mjs` already walks, with the same
`FPDF|EPDF|FORM|PDFiumExt_` filter; the prototype's 21 hand-written declarations were a sysroot
workaround and are not worth carrying. `crates/pdfium` is the safe RAII layer, and its unsafe
boundaries and lifecycle assumptions get audited before promotion rather than inherited.
`crates/epdf-engine` is the ported behavior. `crates/epdf-uniffi` stays separate so
`uniffi::setup_scaffolding!()` never has to build for `wasm32-unknown-emscripten`, at the cost of a
newtype and a `From` impl per type. The prototype's operation objects, dispatch and host adapters are
scaffolding to redesign, not code to promote.

The lifecycle work belongs here, not after the port, because it shapes every operation and binding
that follows. Bounded queues for jobs, open documents, input bytes, raster and cache bytes and search
output, with admission checked before large allocations and a reserved control route so saturation
cannot block close or cancellation. Cleanup ordering on the owner thread, with close idempotent,
rejecting new work, invalidating handles, and closing an engine closing its documents. Engine
replacement with generation IDs on handles, and no process-wide singleton with no reset. Panic
handling that treats a panic after a potentially mutating operation as a poisoned session unless an
operation-specific invariant says otherwise. Aggregate memory monitored at the process and WASM level,
because PDFium's own allocations are not visible to Rust budgets.

Three contracts get written down here and are the reason this phase is not just plumbing.
Cancellation, per the four cases above, with the web mechanism chosen per operation family. The
buffer contract: layout, version, alignment, stride, pixel format, alpha mode, color space, allocator,
release operation and lifetime, with lengths and multiplications validated before allocation, no
freeing by guessed capacity, and heap views rebuilt after WASM memory growth. And coordinates:
normalized page points, origin and axis direction, crop and rotation transforms, device scale, and
durable page identity, because an index is not identity across reordering.

DTO codegen direction gets proven on one type end to end, `ts-rs` generating into
`packages/engine/core/src/dto/` with the zod HTTP schemas in `wire/schemas.ts` and the 20 annotation
schema files (`base.schema.ts` plus `kinds/*/schema.ts`) staying handwritten. CI tests bidirectional
parity with generated types and Rust serialization, extended to annotation DTOs, drafts and patches
in G7/G11. Parsing is the weak half of that test. `MetadataPatch` (`wire/schemas.ts:105`) is three-state, absent leaves a
field, `null` clears it and a value sets it, and the annotation patches carry the same semantics,
which `serde`'s `Option<T>` flattens to two states unless the Rust type says otherwise. So the one
type is a patch, not a read DTO. Rust gets an explicit `Keep | Clear | Set(T)` field type with its
own serialization, generated into TypeScript as `?: T | null`, and the test drives all three states
through an actual `metadata.update` on each host and reads the metadata back. Alongside it: rejection
cases for unknown fields, and a check that nothing a `.strict()` schema would refuse is silently
dropped on the generated side. A DTO that parses can still mean something different.

Toolchain pinning is an exit criterion, not a footnote. `-O3` makes `em++` run `wasm-opt`, emsdk
3.1.72's bundled v119 rejects two WASM features present in `rustc` 1.98.0's precompiled std, and
Emscripten takes Binaryen from its config rather than `PATH`, so the build selects it explicitly
through `EM_BINARYEN_ROOT` and asserts the version it actually got.

Last, three reruns so later phases gate against real baselines: the native `text` and readback rows
after the `bench/arm.mjs` readback-copy fix, since `geometry` and `search` do not touch those paths
and stand as they are; the viewer allocation measurement including array buffers and external memory,
which the committed samples omit; and cold start off a controlled network rather than cached
localhost assets.

## Phase 1: reproducible distribution

Exit: a fork release carrying the mobile and WASM-EH targets, clean-checkout builds with no local
pins or developer-absolute paths, a browser floor established from the real linked module, and fresh
consumer apps installing from distributable artifacts alone.

This runs in parallel with everything from Phase 0 onward and is the likeliest critical path, because
it is the only work here needing someone else's permission. The release pin
`d7b4caa26289d4846e94ff54e63b586b9bbe9fd6` supplies nine targets,
`.github/workflows/release-libpdfium.yml` still lists nine, and `ios-arm64`, `ios-sim-arm64`,
`android-arm64`, `android-x64` and `wasm32-eh` are `"status": "pending"`. The local fork builds all
five, verified against fork commit `7f0fa5dbe`, and the checkout used for the 2026-09-08 audit had
`push: false`. Local pins stay a development route; release verification uses published artifacts.

The mobile libc++ problem is fixed by `use_custom_libcxx=false` plus `use_clang_modules=false`, with
zero `__Cr` symbols across all four cached mobile archives. Android additionally needs the matching
`libc++_shared.so` packaged, consumer shrinker rules, release and shrunk builds validated, and 16 KB
page sizes verified; the audit's emulator returned 16384 from `getconf PAGE_SIZE` and rendered 14
pages at 612 × 792 pt with 120 search hits. Apple ships a self-contained XCFramework in a Swift
Package, following the static-link arrangement that already built an unsigned ARM64 device slice, and
device signing with an actual device run is part of the exit, not a follow-up.

The browser floor needs measurement rather than assertion. The rebuilt module disassembles to 441
`try`, 367 `catch_all`, 285 `rethrow` and zero `try_table`, so it is legacy EH, and the only recorded
passes are Chrome 152.0.7977.83 and cached Playwright Firefox 132.0 Nightly. A Nightly pass says
nothing about release Firefox or Safari. If an alternate EH build turns out to be needed, price its
maintenance and payload cost explicitly before adopting it.

## Phase 2: read services

Exit: every read operation in the operation matrix answered by Rust through Stage A on web and
through a Rust-owned session on native, the read conformance suites green on both backends, and a
native headless SDK preview that is usable, shipped only with the concurrent-engine soak green or
one active engine per process enforced in the API. That is the same gate Phase 3 carries, because
the preview is the first native artifact a consumer can hold two of. "Conformance green" means the
read suites, and the qualifier is load-bearing: the mutation, save and document-event suites still
run against the TypeScript backend here, because native writes do not exist until Phase 4.

Slices, read-before-write and easiest-to-verify-first:

1. `pages.geometry` (Phase 0) and `pages.render`, verified by pixel diff against the colored fixture
   plus the loose whole-page gate. Coarsening buys nothing on render itself; the reason to move it is
   that native hosts need it and the pixel path needs one owner. `pages.renderEncoded` splits instead
   of moving: the raster generation is the same ported code, but the encode is not engine behavior.
   `packages/engine/core/src/wire/worker-protocol.ts:446` says it is a cloud-server surface where the
   host injects the encoder, and that browser and local workers reject the kind with `NotImplemented`.
   So Rust owns the raster, the server host keeps owning encoder injection, and `pages.renderEncoded`
   is not part of the web read exit. G4 also moves engine-level font ownership in Phase 1–2:
   one Rust registry per combined worker, initialized before startup fonts, with TypeScript
   `FontRegistrar` delegating all operations and `idFor` for retained annotation writers. Document
   backend selection does not select fonts. The unmigrated server keeps its TypeScript owner until
   Phase 5. Full font rollback recreates the worker and replays registrations and fallback order
   by stable key; the governing plan Section 6 defines the ownership and coexistence gates.
2. `pages.text`, `search.query`, and the text layout and fold logic in `engine-core`, verified by
   `runSearchConformance`, `runPageTextConformance` and `runTextDivergenceConformance`. Text is
   already coarse, so expect no boundary win there and gate it on correctness. Search also has a
   public query language, and that is the part the conformance suites do not pin.
   `packages/engine/core/src/search/regex.ts:5` defines the dialect as JavaScript `u`-mode syntax
   minus backreferences and lookaround, executed by `RegExp` locally and RE2 on the server, and
   `search/fold.ts:6` defines literal folding as whitespace collapse, NFKD, mark stripping and an
   upper-then-lower case round trip, with a code-unit map back to the original text. A Rust port
   needs a regex engine and a normalization library that agree with V8 on the places they differ by
   default: `\d`, `\w` and `\b` are ASCII under JavaScript `u` mode and Unicode in the `regex` crate;
   case-insensitive matching and the case round trip depend on which Unicode version's tables each
   side ships; `^` and `$` under `m`; zero-length matches and how the cursor advances past them; and
   the fold map's conversion back to UTF-16 offsets. So this slice has an entry gate, not only an exit
   gate. Phase 0 picks the crates and writes a dialect vector file covering accepted and rejected
   syntax, Unicode classes, case folding, empty matches and offsets, with expectations generated by
   running `RegExp` and `foldText`. Representative searches passing says nothing about whether the
   query language survived.
3. `metadata.read`, `pages.list`, `actions.read`, `document.probeSecurityFile`,
   `document.checkPasswordPermissions`, preserving stable page identity and permission behavior.
4. `annotations.listRawAll`, `annotations.listRawPage`, `annotations.listFullPage`,
   `annotations.renderAppearances`, verified by `runAnnotationReadConformance` and
   `runAnnotationAppearanceConformance`.
5. `attachments.list`, `attachments.readFile`, `annotations.readFile`, `pieceInfo.read`,
   `pieceInfo.applications`.
6. `forms.list` and `forms.export`, verified by the read half of `runFormConformance`. Both are reads.
   Filing them with the ten mutating `forms.*` operations because they share a package prefix is
   filing by name rather than by behavior. The ten mutating ones stay in Phase 4.
7. `pages.extract`. It reads: `features/pages/PagesExtractor.ts:10` copies pages into a scratch
   document with `FPDF_ImportPagesByIndex` and serializes that, and `WorkerHost.ts:674` skips
   `finishMutation` for it. It fits Stage A because the scratch document is a transient closed before
   return. It lands last in this phase because it drags the save-write callback into Rust ahead of
   the Phase 4 save family, which `document.saveBuffer` then reuses rather than reimplements.

Native gets one more slice that is not an operation. Before the headless preview ships, a throwaway
app on the oldest supported iPhone and a low-end Android device opens a document, renders tiles into
`CGImage` and `Bitmap`, and scrolls and pinches through the same render path the preview exposes,
with the pixel delivery cost measured in its parts. This is the earliest point with a native session
and a render to measure, and every mobile number in this plan is unknown until it runs. A week of
work gets thrown away. The alternative is discovering Phase 3's budget in Phase 3.

Each slice runs its conformance suites differentially on independent sessions, extends the frozen
vectors and the cross-language corpus, reports the WASM size delta, and passes the cancellation,
memory and lifecycle stress tests from Phase 0.

Those slices summarize a file Phase 0 has to produce first: an operation matrix covering all 64
`WorkerRequest` kinds, each with the platforms that support it, what it depends on, the migration
stage it lands in, and the conformance suites that gate it. It is what catches a
`pages.renderEncoded` filed as a web read, a `forms.list` filed as a write, or a `pages.extract`
filed as a write. All three slipped past a prose review of this document, which is most of the
argument for keeping the matrix somewhere a diff can check it rather than in prose. And the matrix
covers the whole union, not the feature families: the three `open.*` kinds, password handling,
`close`, `layer.close` and the control frames each get a row naming the phase in which a native host
gets that lifecycle. A matrix of feature families alone would let this phase declare native reads
done with no native way to open a document. The phase structure in this document is provisional
until that file exists and has been read against it.

## Phase 3: native reader products

Exit: SwiftUI/UIKit and Compose/View readers with continuous scrolling, pinch zoom, page
virtualization, text selection and copy, search navigation, links, annotation display, accessibility
and file lifecycle, verified on physical devices, installing from distributable SDK artifacts alone,
with no PDF work on the UI thread, and the concurrent-engine soak green or one active engine per
process enforced in the API.

This is the phase an engine-shaped plan omits, and it can overlap Phase 2 once the read contract
stabilizes. The behavioral references are TypeScript: `packages/plugin/render` for visible-first tile
scheduling and retaining low-resolution imagery while sharper tiles arrive,
`packages/plugin/selection` for selection geometry and handles, `packages/plugin/interaction` and
`packages/framework/web` for gesture semantics, `packages/plugin/search` for match navigation,
`packages/core/stage` and `packages/plugin/view-manager` for viewport and virtualization. Read them
as specifications; the implementations are native.

Two rules from the measurements apply directly. Never render a full-page high-resolution bitmap on a
pinch or scroll event; the committed WASM table has `render-4x` at 16.095 ms, which is a dropped frame
on its own. And batch sampled pointer input preserving order rather than crossing per primitive,
which is the same per-item lesson from a different direction.

Image delivery gets decided by measurement, not by promise. Prefer native image creation from
explicitly owned buffers, benchmark `uniffi` byte sequences against a specialized buffer bridge before
choosing, and a small C or JNI bridge purely for pixel ownership is acceptable if it wins. No
zero-copy `CGImage` or `Bitmap` claim ships without a lifetime and format test behind it.

Mobile lifecycle is part of this phase, not a hardening afterthought: backgrounding, memory warnings,
rotation, view disposal and Android process recreation each need a stated presentation and session
policy.

## Phase 4: session authority, then write services

Exit: Stage B complete with no dual authority anywhere, write families ported in dependency order,
and mutation, save-and-reopen, document-event and revision conformance green on both backends.

Stage B lands first and alone: bytes and lifetime, page registry and leases, revision authority,
mutation sequence, password-locked state and retained import dependencies, moved together, with the
TypeScript session becoming a facade and the private owner-worker bridge carrying its removal
criterion.

Then Stage C by family: annotation writes and appearances; the ten mutating `forms.*` operations
(`forms.list` and `forms.export` landed in Phase 2); page assembly (`pages.rotate`, `pages.delete`,
`pages.flatten`, `pages.insert`, `pages.insertBlank`, `pages.move`; `pages.extract` is a read and
landed in Phase 2); `redaction.apply`; `metadata.update`; `pieceInfo.update` and `pieceInfo.clear`;
`attachments.create` and `attachments.delete`; and save and export
(`document.saveBuffer`, `document.saveFile`, `document.saveLayerBuffer`, `document.renderPageFile`).
The four `fonts.*` operations and their shared registry already moved in G4 (Phase 1–2).
Multi-document operations coordinate on one owner: do not pass a source document's handle across
owners for page import, transfer the bytes or reopen a controlled snapshot on the destination.

The 2,431 lines in `runAnnotationMutationConformance.ts` are what make this affordable, and
cancellation-after-apply plus partial native failure semantics get their own tests. Rust's `?` makes
it easy to add a throw path past the apply boundary and quietly break the non-atomic mutation
contract; that needs a review rule and a test, not a comment.

The cost to state plainly: through Phase 5 the worker ships both implementations for rollback, so
two implementations of the same behavior and their runtime dependencies sit in the tree until the
server migration passes and the TypeScript fallback is retired. That is the price of not attempting 16,620 lines in one commit.

## Phase 5: scale and isolation

Exit: budgets validated under load rather than in unit tests, native worker-pool scaling gated the way
the server's is, and a written mobile crash-containment assessment.

The threading gate splits, because half of it cannot wait for this phase. Each engine owns one
dedicated OS thread, so an app holding two engine instances is already running PDFium on two threads,
and it gets there without anyone enabling a worker pool. `EMBEDPDF_TLS_GLOBALS=true` is supposed to
make that safe by giving each thread its own globals.

Supposed to.

So the concurrent-engine soak is a prerequisite for native distribution rather than a Phase 5 item:
`testing/tools:epdf_thread_soak` under `EMBEDPDF_TSAN=1` with multiple simultaneous engines, plus the
single-thread embeddertest regression with `EMBEDPDF_TLS_GLOBALS=false` as the baseline, run against
the mobile arrangement rather than assumed from the server's. If it is not green before any native
artifact leaves the repository, the Phase 2 headless preview included, the SDK enforces one active
engine per process until it is, and it enforces it in the API rather than mentioning it in a release
note.

What genuinely belongs here is throughput at scale. Additional native workers require the TLS-enabled
build, sticky-by-document routing and sustained-load numbers before the pool size moves.

Crash containment needs two separate answers. On the server, reuse `cloudpdf/server/src/runtime/`'s
supervised host, `QuarantiningEnginePool` and `EngineRecycler`. On mobile there is no equivalent: an
in-process native fault kills the app, `catch_unwind` contains nothing, and process isolation on iOS
and Android is a platform-specific investigation with its own cost. The server design does not cover
mobile. This phase produces the assessment; it is not a prerequisite for the reader release, and the
audit's own conclusion says the same.

The server is also where legacy removal gets its gate. `cloudpdf/server/package.json:56` and
`cloudpdf/engine/package.json:48` depend on `@embedpdf/engine-services` today, and
`cloudpdf/server/src/runtime/worker-entry.ts:18` imports `WorkerHost` from it into the
`worker_thread` pool. Phase 6 cannot delete that package while the server still imports it, and
mobile readiness plus web parity say nothing about whether the server is ready. So the server and
Node migration is a named milestone in this phase: `worker-entry` hosting the Rust crate through
`napi-rs` on the Linux and macOS targets the server ships on, the supervised host,
`QuarantiningEnginePool` and `EngineRecycler` exercised against it, and the server's own conformance
and rollout validation green. Windows and musl stay deferred past it. Until that milestone passes,
`engine-services` stays in the tree with its maintenance obligation, and its row in the
compatibility table reads retained rather than removed.

## Phase 6: v4 rollout

Exit: v4 published, the compatibility ledger complete and the migration guide written from actual
changes, the default web backend switched only where parity and performance are demonstrated, retired
bridges removed, legacy `engine-services` and runtime plumbing (`runtime/memory/`, generated
`PdfFunctions` and `PdfRuntimeMemory`) removed only if the Phase 5 server milestone has passed and
the TypeScript fallback is retired, otherwise retained with the ledger saying so, and a rehearsed
rollback.

A mobile reader release may precede native editing and may precede the web default switch. The
documentation has to say so rather than implying parity, and the web default must not move because a
mobile milestone passed.

## What this defers, and what would change it

Interactive annotation editing on mobile, and with it any port of `packages/core/annotation`. The
trigger is a product decision, so it should be asked rather than waited for. If it comes back yes, the
package is already shaped for it: `update(msg) -> [model, effects]` and
`scene(model, view) -> DisplayList`, one call per event and one per frame, nothing finer, with web
keeping TypeScript unless a real-workload measurement against an optimized TypeScript baseline says
otherwise. Benchmark simple and complex annotations, ink, cloudy borders, snapping and hit-test misses
before adopting any shared interaction core.

Shared tile planning, evaluated separately from annotation editing. A Rust planner returning tile
demands and invalidations is plausible; native wrappers would keep compositor resource ownership
either way.

Coarse `EPDF*` wrappers whose only job is collapsing JavaScript crossings. The port deletes those
crossings. `EPDF*` functions that save work inside PDFium keep earning their place:
`EPDFText_GetCharGeometry` gathers boxes, quads, matrix and flags from one traversal instead of
recomputing per field. There is a real case for `EPDF_GetPageGeometryFull` as a bridge, since
`pdfium-binding-benchmarks/cpp/ops.cc`'s `cc_read_page_geometry` already passes the fairness gate and
would buy the 2.6x on web today, but it becomes a public C API in a fork with no caller the moment
Phase 0 lands. Not worth it if Phase 0 is close.

`packages/core/js-sandbox` would be replaced rather than ported, by embedding `rquickjs`. PDF form
JavaScript keeps its current web path until a native sandbox and behavior contract are validated, and
mobile reports unsupported scripts explicitly. Nothing in `framework/*` or `viewer/*` is a candidate.
Windows and musl coverage, and the server's own rollout, are not part of the first mobile release.

## What I don't know

Whether 16,620 lines of `engine-services` port well. That is Phases 2 and 4 and it is the actual risk.
Nothing measured so far says anything about it in either direction, which is why Stage A exists: it
buys the read win without betting the schedule on the answer.

Whether the cross-language conformance corpus is a week or a quarter. I have never built one. The
generator side looks straightforward because the cases are already written; the runner side has to
work in Swift, Kotlin, Rust, Node and a browser, and the last time this repo assumed a five-host test
story was easy it got two smoke tests and a grayscale golden.

Mobile performance, completely. Not one number in any of the three spikes touches iOS or Android
runtime behavior, and Phase 3 is the largest piece of work here with the least evidence behind its
estimates. The device prototype in Phase 2 exists to shrink that gap before Phase 3 is budgeted, not
to close it.

And whether the facades stay thin under real UI pressure. The rule is that they must not duplicate
document semantics, which is looser than what I first wrote and still the thing most likely to erode
one convenient method at a time.
