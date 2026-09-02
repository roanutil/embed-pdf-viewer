# Porting the core to Rust

I spent an afternoon on a narrow question: could `AnchorMode` in
`packages/core/annotation/src/anchor.ts` be the first thing we port to Rust? It is 196 lines of
pure similarity math with no I/O. It looks like the ideal first slice.

It is close to the worst first slice in the repo. Working out why is most of this document.

The goal, to be clear about it, is one Rust core with platform-specific bindings: web through WASM,
iOS through Swift, Android through Kotlin. Performance on web is not the goal. If it were, almost
none of what follows would apply.

## Why anchor.ts is the wrong start

Four reasons, and only the first is about speed.

`anchoredGeom` returns its input unchanged when the projection is the identity. That is deliberate,
and callers lean on it: `view.ts`, `hit.ts`, `snap.ts`, and `update.ts` all call it unconditionally
on every annotation and rely on getting the same object back. Cross a WASM boundary and you
deserialize into a fresh object every time. Reference equality dies, and so does every memo check
downstream. That is a behavior change, not a benchmark regression.

Then it is not a leaf. It calls `geomBounds`, `geomScaleAbout`, `geomRotateAbout`, `geomTranslate`,
`rotatePoint`, and `normalizeDeg` from `geometry.ts` (1,437 lines), plus `capsFor` from `kinds.ts`
(574 lines). So porting it alone means calling back into TypeScript from Rust on every projection,
which is the slowest arrangement available. Porting its dependencies means about 2,000 lines on day
one.

Third, the cost is inverted. `factors()` is two comparisons and a `Math.max`. Serializing a
`{ t: 'ink', strokes: Vec[][] }` across the boundary is vastly more work than the math it enables. I
have not measured this in our code, so treat the magnitude as unknown, but the direction is not in
doubt.

And fourth, `@embedpdf/core-annotation` is synchronous. Its `package.json` exports `./src/index.ts`
directly, with `sideEffects: false`. WASM instantiation is async, and synchronous
`new WebAssembly.Module` on anything over 4KB is blocked on the browser main thread. Every consumer
of the annotation brain would grow an await.

## Where the boundary already exists

Here is the reframing that makes the rest of this tractable. Every TypeScript-to-WASM crossing costs
a serialization, so the cheapest place to put Rust is a boundary that already pays for one.

We have exactly such a boundary, and we built it for other reasons. The `Engine` and
`DocumentHandle` interfaces in `packages/engine/core` already cross a Web Worker through
`src/wire/worker-protocol.ts` (1,138 lines). They already return `AbortablePromise`. And
`packages/engine/services` already runs unchanged in a browser worker and in a server
`worker_thread`, which its own package description says out loud.

Swap the implementation behind that interface for Rust and the interop cost changes by roughly
nothing. We are replacing the insides of a worker, not introducing a new hop.

(It is worth noticing how much of this plan is a consequence of past discipline rather than new
work. The `*-core` purity rule, the DOM confined to `@embedpdf/web`, the transport-agnostic
conformance harness: none of it was written with Rust in mind, and all of it is what makes Rust
cheap here.)

## The waves

Wave 0 ports nothing. It exists to answer one question that can invalidate everything below it.

PDFium is built with Emscripten today, which is why `packages/engine/runtime/npm/wasm32` exists.
Rust compiled to `wasm32-unknown-unknown` cannot link an Emscripten object. There are three ways
out: build the crate for `wasm32-unknown-emscripten` and link normally, rebuild PDFium for
`wasm32-wasip1` and drop the Emscripten runtime, or keep the two as separate modules and have Rust
call PDFium through imports. The third pays a crossing per FFI call, of which a single page render
makes thousands, so I expect it is unusable. Prove one of the first two on
`EPDF_GetPageSizeByIndexNormalized` before writing anything else.

The second wave-0 question is cancellation. `engine-core` is built on `AbortablePromise`, and some
PDFium calls run long and cannot be interrupted. Decide how a Rust cancellation token maps onto that
contract, and which operations are honestly uncancellable, before porting a service.

The rest of wave 0 is plumbing: a Cargo workspace under `crates/` registered as a turbo task,
`bindgen` over the same headers `packages/engine/runtime/build/generate-functions.mjs` already walks
with the same `FPDF|EPDF|FORM|PDFiumExt_` filter, `wasm-bindgen` for web and `uniffi` for Swift and
Kotlin, and the conformance harness in `packages/engine/core/src/conformance` wired to run against a
Rust-backed engine.

Wave 1 is `packages/core/geometry`. It is 785 lines with zero dependencies, and its own description
calls it the bottom of the pyramid. Web keeps its TypeScript copy and nothing changes for users. The
point is not the code. The point is proving the build, the CI job, the `uniffi` output, and the
shared-test-vector mechanism on something that cannot break anyone. Establish that mechanism here,
because every later wave depends on it: extract the test cases to JSON and run the same vectors
through both implementations. If a module's behavior cannot be pinned by shared vectors or by the
conformance harness, it is not ready to port.

Wave 2 is `packages/engine/services`, 16,281 lines, and it is the actual prize. This is the code
that is most expensive to reimplement per platform and most dangerous to let diverge, because PDF
spec correctness lives here. Read before write, and easiest-to-verify first:

1. `features/geometry/PageGeometryReader` (333) and `features/render/deviceRaster` (211), verified by pixel diff.
2. `engine-core src/text/layout.ts` (548) and the text features, verified by `runSearchConformance`.
3. `annotations/internal/read/annotationReadPrimitives` (418) and `AnnotationAppearanceReader` (260).
4. `annotations/internal/write/*` (roughly 1,400) and `AnnotationMutator` (784).
5. `features/forms/*` with `FormMutator` (616) and `FormsEffectsApplier` (452).
6. `features/pages/*`, `RedactionApplier` (274), `PieceInfoAccessor` (370).
7. `document-session/DocumentSession` (354).

Step 4 is where the plan earns its cost back. `runAnnotationMutationConformance.ts` is 2,255 lines
of transport-agnostic tests that already exist. Point it at both engines and annotation write
correctness stops being a matter of opinion.

`worker-host/WorkerHost.ts` (1,459 lines) is deliberately excluded. It is the web transport, and on
mobile the transport is a direct function call.

One decision has to be made before step 1, and it is easy to get wrong by default. Right now
`src/wire/schemas.ts` (1,188 lines) and the DTO types are hand-written TypeScript. Once Rust owns
the engine, every one of those types exists twice with nothing checking the pair. Generate the
TypeScript from Rust with `ts-rs` or `tsify` and make the Rust definition canonical. The cost is a
codegen step in the build and a less pleasant editing experience for anyone who only works in
TypeScript. I still think it is right, because the alternative is a two-file edit where mismatches
surface at runtime.

Wave 3 is the Swift and Kotlin bindings, and it is the first real payoff: a native SDK that opens,
renders, searches, and reads and writes annotations, with no JavaScript runtime on the device. Note
that `packages/engine/runtime/npm` ships wasm32 plus darwin, linux, linuxmusl and win32 natives
today, and no iOS or Android target at all. Adding those two to the PDFium build is a prerequisite,
and it is independent of every port above, so it can run in parallel from wave 0.

Wave 4 is `packages/core/annotation`, 7,288 production lines, and it exists only if mobile needs
interactive editing rather than display and read. In dependency order that is `types.ts` and
`flags.ts`, then `geometry.ts`, then `kinds.ts`, then `anchor.ts`, then `hit.ts` and `snap.ts`, then
the compute-heavy leaves in `cloudy.ts` and `ink.ts` and `endings.ts`, then `scene.ts` and `view.ts`,
and `update.ts` last.

So `anchor.ts`, the module that started this whole investigation, lands fourth inside the fifth wave.

The package is already shaped for a coarse boundary, which is the one thing that makes wave 4
survivable. Expose `update(msg) → [model, effects]` and `scene(model, view) → DisplayList`, one call
per event and one call per frame, and nothing finer. A per-annotation call is N crossings a frame and
loses the identity short-circuit. A per-frame call is one crossing over a flat buffer, and the
identity check moves inside Rust where it stays free.

I should name the cost honestly, because it is the real objection to wave 4. Web has a working,
synchronous, tree-shakeable `@embedpdf/core-annotation`. Rust gives two options and neither is free.
Keep TypeScript as the web path and maintain two implementations of 7,288 lines held together only
by shared test vectors. Or cut web over to WASM and accept async init in a package whose entire
contract is being synchronous, plus the bundle growth, plus a broken SSR story. My read is keep
TypeScript on web and hold the line with vectors. But that is a judgment call I would want made
deliberately rather than discovered halfway through.

Wave 5 is `packages/core/main` (1,580 lines) and `packages/core/stage` (919). The store in
`src/store.ts` is already reducer-shaped, so a Rust version with `dispatch` plus a snapshot
subscription fits on paper. I would still defer it indefinitely. Highest cost, lowest marginal
sharing benefit, and the worst effect on the web bundle, because it replaces the one piece of
infrastructure every plugin loads.

Nothing in `framework/*` or `viewer/*` is ever a candidate. `core/ui` (1,207 lines) is portable in
principle, but a web toolbar fit solver has no mobile consumer. And `core/js-sandbox` would not be
ported so much as replaced, since a Rust core would embed `rquickjs` directly.

## What I do not know

The PDFium linkage question is genuinely open and I have not tried any of the three options. Until
someone does, wave 2 onward is contingent.

Update: the linkage question above is answered, and it went the friendly way. A Rust `staticlib` built for
`wasm32-unknown-emscripten` goes on the same `em++` command line as `libembedpdf.a` and links, with
no second module, no cross-module calls and no shared-memory problem. `pdfium-rust-spike/` does it
four times over, and `build/link-wasm.sh` in that directory is the proof, so wave 2 onward is not
contingent on it any more.

I do have a number for the crossing now, and it is in
[the PDFium wrapper benchmark](./pdfium-rust-wrapper-benchmarks.md). A wasm call into PDFium costs
around 340 ns through a hand-written `cwrap`. Putting a Rust frame in front of it costs 1 to 4
percent on workloads that do real PDFium work, and the null probe built to price that frame in
isolation could not resolve it at all. Collapsing the per-glyph loop in
`PageGeometryReader` into a single crossing is worth about 2.6x in wasm and 7.10x to 15.08x
natively, because `mem.peek` is a typed-array read in one place and a full N-API round trip in the
other.

So the boundary is priced and the answer is that a thin one is nearly free and a chatty one is
expensive, which is what the ordering above already assumed. What it does not settle is the thing I
actually said I wanted: those are PDFium payloads, not `Geom` payloads, and the coarse win came out
identical whether Rust or C++ sat behind it. One machine, one fixture page, one PDF.

And I have assumed mobile needs display, search, and annotation read and write, but not interactive
drawing. If that assumption is wrong, wave 4 stops being optional and the total roughly grows from
20k lines to 28k. That one answer moves more than anything else in this plan, and I do not have it.
