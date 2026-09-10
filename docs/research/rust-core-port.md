# Porting the core to Rust

I spent an afternoon on a narrow question: could `AnchorMode` in
`packages/core/annotation/src/anchor.ts` be the first thing we port to Rust? It is 196 lines of
pure similarity math with no I/O. It looks like the ideal first slice.

It is close to the worst first slice in the repo. Working out why is most of this document.

The goal, to be clear about it, is one Rust core with platform-specific bindings: web through WASM,
iOS through Swift, Android through Kotlin. Performance on web is not the goal. If it were, almost
none of what follows would apply.

Update, and it changes how to read everything below: the port is happening. This document was
written to work out whether and where to start, so treat the waves as a plan now rather than a
proposal. Performance on web is still not the goal. But it is no longer obviously a cost either, and
the distinction turns out to depend on which layer you mean. At the `engine-services` boundary the
port makes web faster, because the per-glyph loop stops crossing anything; measured, that is about
2.6x on glyph geometry. At the `core/annotation` layer the earlier toy benchmark in
`viewer-rust-port-benchmarks/` still says the opposite, that V8 beats Rust-in-wasm on arithmetic that cheap.
Both can be true because they are different amounts of compute behind each crossing, which is the
argument the rest of this document already makes.

How the PDFium fork itself should be handled now has its own record in
[the runtime decision](./pdfium-rust-wrapper-decision.md). The short version: bind to its C ABI from
Rust, and stop growing coarse `EPDF*` wrappers whose only job is collapsing JavaScript crossings,
because the port deletes those crossings for free.

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
makes thousands, so I expect it is unusable.

Update: the first option works, and it is the one to use. A Rust `staticlib` built for
`wasm32-unknown-emscripten` goes on the same `em++` line as `libembedpdf.a` and links, with no
second module, no cross-module calls and no shared-memory problem.
`pdfium-binding-benchmarks/build/link-wasm.sh` does it four times over, and it was proven first on
`EPDF_GetPageSizeByIndexNormalized`, exactly as this paragraph asked. One snag for whoever wires it
into the real build: `-O3` makes `em++` run `wasm-opt`, and emsdk 3.1.72's bundled v119 rejects two
wasm features present in rustc 1.98.0's precompiled std, so it needs a newer `wasm-opt` than the one
in the SDK.

So wave 0 has one question left rather than two, and the remaining one is the harder of the pair.

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

Step 1 is also where the boundary win lands, which is worth knowing before anyone argues about the
ordering. `PageGeometryReader` makes two PDFium calls per glyph plus up to 21 `mem.peek` reads to
decode each geometry struct, so a page with 8,106 glyphs costs something like 25,000
JavaScript-to-PDFium crossings today. Moving that loop into the core does not make the calls
cheaper. It stops them being crossings, and that alone measured about 2.6x in wasm and 7.16x on
Node. Nothing has to be redesigned to collect it.

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
today, and no published iOS or Android artifact. The local fork now builds both;
the [September 8 audit](./platform-gap-audit.md) verified local simulator/emulator renders.
Completing release automation and device packaging is a prerequisite to shipping,
and is independent of every port above, so it can run in parallel from wave 0.
`pdfium-multiplatform-prototype/` at the repo root already scaffolded this binding surface, uniffi to Swift and
to Kotlin over the same six coarse operations. Local pins can supply `ios-arm64`,
`ios-sim-arm64`, `android-arm64`, and `android-x64` now; a fresh checkout still needs
those artifacts published and pinned to reproduce the mobile builds.

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

Update: the two things this section used to open with are answered, and the answers moved up into
wave 0 and into [the PDFium wrapper benchmark](./pdfium-rust-wrapper-benchmarks.md). The linkage
works. The boundary is priced: a wasm call into PDFium costs around 350 ns through a hand-written
`cwrap`, putting a Rust frame in front of it costs 1 to 4 percent, and collapsing the per-glyph loop
is worth about 2.6x in wasm and 7.16x on Node.

What that does not settle is the thing I actually said I wanted. Those are PDFium payloads, not
`Geom` payloads, and the coarse win came out identical whether Rust or C++ sat behind it, so it
prices the boundary rather than the language. One machine, one fixture page, one PDF.

The wave-0 cancellation question is still open, and it is now the one gating item there. `engine-core`
is built on `AbortablePromise`, some PDFium calls run long and cannot be interrupted, and nobody has
decided which operations are honestly uncancellable.

And I have assumed mobile needs display, search, and annotation read and write, but not interactive
drawing. If that assumption is wrong, wave 4 stops being optional and the total roughly grows from
20k lines to 28k. That one answer moves more than anything else in this plan, and I do not have it.
