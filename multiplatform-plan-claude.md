# Multiplatform EmbedPDF: a Rust core with native wrappers

**Goal:** One Rust implementation of the PDF engine, consumed by web through the existing WASM
worker, by iOS through a Swift package, and by Android through a Kotlin library, without
reimplementing engine behavior per platform.

**Architecture:** Rust owns everything from PDFium's C ABI up through the 60 document operations in
`packages/engine/core/src/wire/worker-protocol.ts`. TypeScript keeps everything above that line on
web: the kernel, the annotation brain, the plugins, the framework adapters, the viewer. The native
SDKs are handwritten Swift and Kotlin facades over a `uniffi` ABI, holding no PDF logic. Migration
runs operation by operation behind a routing table, so at every commit the shipping engine is part
TypeScript and part Rust and both answers are compared in CI.

**Tech stack:** `rustc` 1.98.0 with `edition = "2024"`, `bindgen = "=0.72.1"`, `uniffi = "=0.32.0"`,
`napi = "=3.12.2"`, `ts-rs` for the canonical DTOs, `cargo-ndk` 4.1.2, emsdk 3.1.72 with Binaryen 132
selected through `EM_BINARYEN_ROOT`, Swift 6.3.3 / Xcode 26.6 (17F113), Kotlin 2.2.20 / AGP 9.4.0 /
NDK 29.0.14206865, Node 24.20.0, pnpm 10.34.0.

## Global constraints

Every phase below inherits these. They are not restated per phase.

The JavaScript-to-Rust boundary is per operation, never per item. A per-glyph boundary measured 2.6x
slower than a per-page one in WASM and 7.16x slower on Node
(`pdfium-binding-benchmarks/RESULTS.md`). No convenience API may reintroduce a per-item call.

No Rust export taking `&mut self` may call back into the host. Doing so in
`viewer-rust-port-benchmarks/rust-geometry-and-state` produced `attempted to take ownership of Rust
value while it was borrowed` on `free()`, surfacing far from its cause.

PDFium handles never cross threads. Native builds set `EMBEDPDF_TLS_GLOBALS=true` and each engine
owns one dedicated OS thread; `wasm32` and `wasm32-eh` leave it off because each instance isolates
its globals in its own linear memory. This is the contract already documented in
`.agents/skills/embedpdf-conventions/references/runtime-confinement.md`, and the prototype's
`crates/epdf-ops/src/dispatch/{threaded,inline}.rs` is the two-target implementation of it.

Text offsets and lengths at every boundary are UTF-16 code units. The frozen `charCount` for
`fixtures/report.pdf` page 4 is 8,106 code units and 7,992 grapheme clusters, and Swift's default
`String.count` gives the second number.

The Rust definition is canonical for every DTO and wire envelope. TypeScript types are generated and
checked in; a dirty diff fails CI.

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

`pdfium-binding-benchmarks` said the cheap thing is cheap and the expensive thing is the boundary. A
thin Rust frame in front of PDFium costs 1 to 4 percent in WASM and 3,802 extra bytes on a 4,676,714
byte module, measured from the worst side of it, with JavaScript reaching arm b through `cwrap`
where the real core will call straight through. So `pdfium-sys` is not a thing to optimize. Build it
the boring way. What actually costs is the crossing count: `PageGeometryReader` makes two PDFium
calls per glyph plus up to 21 `mem.peek` reads to decode each struct, so page 4 of
`examples/snippet-react/public/report.pdf` with its 8,106 glyphs runs about 25,000
JavaScript-to-PDFium crossings today. Moving that loop inside the module bought 2.6x in WASM and
7.16x on Node. And whether Rust or C++ owned the loop was indistinguishable, `d/c` between 0.962 and
1.048, which under a Rust core is the good news rather than the bad: the loop can live wherever it
is cheapest to maintain.

`viewer-rust-port-benchmarks` said the opposite about cheap math, and said it loudly. Three copies of
the same toy viewer, and the Rust-owns-state arm never beat the TypeScript baseline on a single one
of 20 frame cells. It ran 1.6x to 3.1x slower everywhere. At 1,000 anchored shapes with a projecting
view, the exact workload the port plan cares about, TypeScript took 127 µs and Rust took 207 µs for a
frame that crossed the boundary once against TypeScript's zero. Stripping the `DisplayList` decode,
which is 44 to 53 percent of the Rust frame cost at n=1000 and above, still left Rust 1.35x to 1.75x
behind. The fine-grained arm won 0 of 20 cells and ran 4.6x to 8.4x slower.

So here is the rule those two spikes hand us, and it is the only rule that matters for deciding what
to port: put Rust where the compute behind one crossing is large. Twenty-five thousand PDFium calls
behind one crossing is a 2.6x win. Four multiplies and a `Math.max` behind one crossing is a 3.1x
loss.

(One measured caution on the second spike, because it is easy to over-read. The toy's per-shape math
is a handful of multiplies, which is precisely the shape V8 optimizes best. `cloudy.ts` is 570 lines
and ink resampling is not four floats and a winding test. The result does not say Rust would lose on
the real annotation workload. It says the boundary plus JavaScript object materialization eats any
win on math that cheap.)

`pdfium-multiplatform-prototype` said the binding surface works, and then told us what the test was
not catching. Six code paths, five of them crossing a language boundary, all rendered page 4 into a
121,176-byte BGRA buffer matching a golden generated by the shipping TypeScript runtime at mean
absolute difference 0 and max 0. That is the byte pipeline proven transparent. But page 4 is entirely
grayscale, B equals G equals R in all 30,294 pixels, so transforming the golden with a B-to-R swap and
re-running the same diff also scores mean 0.0 and max 0. A grayscale golden cannot tell a correct
BGRA read from an RGBA-for-BGRA one. And the tolerances that ship, mean 2 and max 16, catch an
all-white buffer at 16.78, an all-black one at 238.22, a one-pixel horizontal shift at 6.12 and a
zeroed alpha channel at 63.75, but let a uniform 6-level darkening of every non-white byte through at
mean 1.74 and max 6. That is the signature of a wrong color space or an unapplied gamma curve.

Which is why the verification section below is longer than any phase.

## Where the line goes

Rust owns PDFium through the engine's operation surface. That is `crates/pdfium-sys`, a safe
`crates/pdfium`, and then a port of the 16,620 source lines in `packages/engine/services` plus the
engine-behavior parts of `packages/engine/core`: text layout, the search fold and cursor, DTO
encoding, error codes.

TypeScript keeps `packages/core/*`, all 17 plugins, `packages/framework/*` and `packages/viewer/*`.
On web, permanently. `packages/core/annotation` is 7,527 lines with a working synchronous
tree-shakeable contract, and every frame metric in `viewer-rust-port-benchmarks` says a Rust version
would be slower on web. `packages/core/main` and `packages/core/stage` are worse still: the store is
the one piece of infrastructure every plugin loads, so replacing it costs the whole bundle for no
measured gain.

`worker-host/WorkerHost.ts` is excluded on purpose. It is 1,459 lines of web transport, and on mobile
the transport is a direct function call.

Node and the server are a fourth consumer, and they come along for free rather than needing a wave.
`@cloudpdf/server` already runs `engine-services` unchanged in a `worker_thread`, so when a slice
moves to Rust the server's worker gets the Rust one through the same routing table, and the nine
native targets in `packages/engine/runtime/npm/` reach it through `napi-rs`. Use `f64` pointers there
rather than BigInt: the shipping addon takes 8.642 ms on glyph geometry where a `napi-rs` addon takes
4.134 ms for the same PDFium calls in the same order, and 39.2 ns against 15.0 ns on a single `peek`.
That comparison changed the binding framework, the pointer representation and the string type-tag
dispatch at `packages/engine/runtime/src/native/native-runtime.ts:57` all at once, so it does not
attribute cleanly, and a coarse boundary makes it mostly moot. The cheap convention is still the
right default.

The honest reason to do any of this is code sharing, not web performance. Web gets faster in exactly
one place, the geometry and text loops, and slower nowhere it can measure. iOS and Android otherwise
need a second implementation of 16,620 lines where PDF spec correctness lives, and that second
implementation will drift. That is the whole argument.

## The seam we already have

Almost everything that makes this affordable was built for other reasons.

`Engine` and `DocumentHandle` in `packages/engine/core/src/engine/` are already implemented twice,
by `LocalEngine` over a worker and by `@cloudpdf/engine` over HTTPS, and the interface's own doc
comment says the only observable difference is transport. A Rust-backed engine is the third
implementation of an interface that already proved it can have more than one.

`WorkerRequest` at `packages/engine/core/src/wire/worker-protocol.ts:866` is a 64-member union, four
of which (`close`, `layer.close`, `abort`, `shutdown`) are control frames. One message per operation.
That is already the coarse shape the benchmarks say to build, so the contract to port to is the
contract that exists.

And `packages/engine/core/src/conformance/` holds 21 transport-agnostic suites taking
`makeEngine: () => Engine`, including 2,431 lines in `runAnnotationMutationConformance.ts`. Pointing
those at two engines is a differential test we get for free.

## The break budget

Consumers talk to `Engine` and `DocumentHandle`, not to services, so most of the port is invisible to
them. Here is the complete list of what we intend to break in v4. Anything not on this list needs a
line in the migration guide before it lands.

| Break | Why it is forced | Mitigation |
|---|---|---|
| Browser support floor rises to WASM exception handling | `rustc`'s std for `wasm32-unknown-emscripten` lowers panics through native WASM EH, needing `em++`'s `-fwasm-exceptions`, and PDFium genuinely uses `setjmp`/`longjmp` for libpng and libjpeg errors (`core/fxcodec/png/pngmodule.cpp`, `core/fxcodec/jpeg/jpeg_common.c`). `em++` refuses `-fwasm-exceptions` with `-sSUPPORT_LONGJMP=emscripten`, so the target becomes `wasm32-eh` with `-sSUPPORT_LONGJMP=wasm`. | Publish a tested browser floor. Phase 1 establishes it against release Firefox and Safari, not just Chrome 152.0.7977.83 and Playwright Firefox 132.0 Nightly. |
| DTO shape changes where generated Rust types differ from hand-written TypeScript | The Rust definition becomes canonical, and cosmetic mismatches (numeric widening, nullable versus optional, enum spelling) will exist. | Fix Rust to match TypeScript wherever the mismatch is cosmetic, not the other way around. Diff generated against current in Phase 0 and land the list. |
| `@embedpdf/engine-services` deprecated as a public package | Its contents move to `crates/`. Anyone importing it directly loses it. | Keep publishing an empty shim for one major with a deprecation notice. It is a package almost nobody should have been importing. |
| Web error surface unified on `EngineErrorCode` | Web currently exposes message strings on some paths where Node returns stable codes. Two engines cannot keep two error vocabularies. | Codes are additive for anyone already switching on `EngineErrorCode`; only string matching breaks. |
| Uncancellable operations start rejecting `abort()` explicitly instead of appearing to accept it | See the cancellation section. Today the contract is silent about which operations honor an abort. | Publish the tier per operation kind. The change is truthfulness, and callers who never aborted see nothing. |

Everything else stays: `localEngine()` construction stays synchronous, `open()` keeps returning
`AbortablePromise<DocumentHandle>`, `warmup()` keeps its meaning, and every service on
`DocumentHandle` keeps its signature. The WASM module already loads lazily inside the worker, so a
Rust core adds no new await anywhere.

Latest published stable is 2.15.0 and the current line is `3.0.0-next.11`, so this work targets v4.
If any of it lands before 3.0.0 goes stable, that part of the break budget is free and should be
spent then.

## Native APIs that feel native

`uniffi` is the ABI, not the API. That is the one rule here, and the prototype is why.

`uniffi`'s Kotlin generator makes every object `AutoCloseable` and emits its own `override fun
close()`, so the exported `close` had to become `closeSync`. It also gives every exception variant an
`override val message`, so `OpsError::EngineFailed`'s Rust field named `message` had to become
`detail`. Both renames live in `packages/kotlin/uniffi.toml`, and the result is that the Swift and
Kotlin surfaces are not name-identical for six operations. At sixty they will not be recognizable as
the same SDK, and neither will look like something a platform engineer would have written.

So each platform gets a handwritten facade whose only jobs are naming, type conversion,
thread and dispatcher adaptation, and lifetime. Swift:

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

`PDFDocument` is `Sendable` and its `async` methods bridge to the engine's own thread; Swift `Task`
cancellation maps onto the engine's abort token, and so does Kotlin structured concurrency. Errors
are a typed `PDFError` and a sealed `PdfException` hierarchy carrying the same `EngineErrorCode`
values the web engine returns.

The cost of handwriting these is two more surfaces to maintain, and they can drift from each other in
naming. What they cannot drift in is behavior, because the cross-language conformance corpus below
runs through the facade rather than through `uniffi`'s output. A facade that converts a bitmap wrong
fails a test.

The hard rule: no facade file contains a decision. Anything that branches on PDF content, computes a
rectangle, or picks a default belongs in `crates/`. When a facade needs logic, the logic goes in Rust
and the facade calls it.

## Cancellation, the one gating decision

`engine-core` is built on `AbortablePromise`, some PDFium calls run long and cannot be interrupted,
and nobody has decided which ones. The prototype deferred it: `Worker::run` blocks on `recv` and
rendering calls monolithic `FPDF_RenderPageBitmap`. This has to be settled in Phase 0, because every
operation ported afterwards inherits whatever we decide.

Three tiers, declared per operation kind in `worker-protocol.ts` and asserted in a conformance suite:

Tier 1, pre-flight. The operation checks the token before doing native work. Aborting before the
queued job starts already works this way through `WorkerQueue`, and every operation gets at least
this.

Tier 2, cooperative. The operation checks between units of work: per page in search and text
extraction, per glyph batch in geometry, and through PDFium's progressive render APIs and
`IFSDK_PAUSE` for render. `packages/engine/services/src/features/render/PageRenderReader.ts` already
demonstrates boundary abort checks, so this is generalizing an existing pattern rather than inventing
one.

Tier 3, uncancellable. Save, flatten, redaction apply and most mutations. `AbortablePromise` settles
only after the native call returns, and `abort()` on a tier 3 operation rejects immediately with a
distinct code rather than pretending. Hard deadlines need worker or process replacement and document
reopening, which `cloudpdf/server/src/runtime/` already implements in `QuarantiningEnginePool` and
`EngineRecycler`. Nothing in the core tries to destroy a handle from another thread.

## Verification is the spine

Three mechanisms, and the third is the only one that does not already exist.

The 21 conformance suites run against both engines. Concretely, `WorkerHost` gains a routing table,
a `Set<WorkerRequest['kind']>` naming the operations Rust owns, and under `EPDF_DIFF_RUST=1` it runs
both implementations for every kind in that set and fails on divergence. That gives a differential
test for every ported operation at the cost of one environment variable, and it makes a bad slice
revertable by deleting one entry from a set.

Frozen vectors carry what conformance cannot. `pdfium-multiplatform-prototype/vectors/` is the
pattern: JSON written once by a generator that drives the shipping TypeScript runtime, never
regenerated during a test run, read by suites in Rust, Swift, Kotlin, Node and the browser. Page
counts, page sizes, text, search hits and `charCount` checked exactly; rendered bitmaps by pixel diff
rather than by hash, because a hash pins float and rasterization behavior across four architectures
and would fail for reasons that have nothing to do with our code. The tolerances get split: exact
interior BGRA expectations on a colored fixture for channel and color-space correctness, and the
loose whole-page gate kept only for gross drift.

The third mechanism is a cross-language conformance corpus, and without it mobile drifts. The 21
suites are TypeScript functions taking a TypeScript `Engine`, so Swift and Kotlin currently get five
and six smoke tests against sixty operations. The fix is to express each suite's cases as data,
a fixture id plus an operation plus an expected result, emitted from the existing suites into
`vectors/conformance/*.json`, with a small runner per host reading that corpus through the platform
facade. This is the main new investment in the plan and the thing most likely to be skipped under
pressure. Skipping it means shipping mobile SDKs verified by smoke test.

And then the discipline the benchmarks taught, which is about not believing our own numbers. That
study found three of its own measurement bugs in development, each of which reversed or manufactured
a conclusion: a decoder allocating roughly 26,000 objects per iteration that only some arms paid,
one arm compiled at `-O0` against another's `-O3`, and arm order never rotated within a round. The
fairness gate caught none of them, because the answers stayed identical throughout. The viewer spike
separately hid an O(n²) `rows.find` inside a per-item loop, invisible at demo sizes and 184x slower
at 10,000 shapes. So: every benchmark runs a differential answer gate before any timing, every
comparison gets an explicit audit that both arms do the same work, arm order rotates within rounds,
and we report medians and p95 rather than means.

The performance gates that ship with the port, each with its measured baseline: glyph geometry on
`report.pdf` page 4 must beat the current TypeScript reader by at least 2x in WASM and 5x on Node;
WASM module size gets published per phase against the current 4,676,714 bytes; and cold start gets
measured against today's, knowing the prototype's Rust addition cost 6.3 ms to instantiate 130.7 kB
and 7.8 ms for 277.2 kB in a toy.

---

## Phase 0: foundations

Exit: the shipping `@embedpdf/engine` worker links a Rust staticlib that answers exactly one
operation, `pages.geometry`, at differential parity with `PageGeometryReader`, reproducibly from a
clean checkout.

We start with `pages.geometry` because it is where the measured 2.6x and 7.16x land, and because its
output is a flat numeric buffer that shared vectors pin exactly.

The crates, following the prototype's split and keeping only what survives it. `crates/build-support`
holds the shared `build.rs` logic, and it exists because Cargo does not propagate a dependency's
`cargo:rustc-link-arg` to a dependent's test or bin targets, only `rustc-link-lib` and
`rustc-link-search`, so every crate that links `libembedpdf` needs its own build script.
`crates/pdfium-sys` is `bindgen` 0.72.1 over the same fork headers
`packages/engine/runtime/build/generate-functions.mjs` already walks, with the same
`FPDF|EPDF|FORM|PDFiumExt_` filter; the prototype hand-wrote 21 declarations to dodge a sysroot
problem and that shortcut is not worth carrying. `crates/pdfium` is the safe RAII layer.
`crates/epdf-engine` is the ported engine behavior. `crates/epdf-uniffi` stays a separate crate so
`uniffi::setup_scaffolding!()` never has to build for `wasm32-unknown-emscripten`, at the cost of a
newtype and a `From` impl per type.

Three things get decided here rather than discovered later. Cancellation, per the tier table above,
written into `worker-protocol.ts` and covered by a new conformance suite. The DTO codegen direction:
`ts-rs` generating `packages/engine/core/src/dto/` from Rust, proven on one type end to end, with the
zod HTTP schemas in `wire/schemas.ts` staying hand-written for the cloud transport and gaining a CI
test that every generated DTO parses against its schema. And the response transport on web: Rust
returns a pointer and length into linear memory, the worker copies out and hands the buffer to
`wirePack(payload, [buffer])`, which is the ownership manifest
`packages/engine/core/src/wire/WirePack.ts` already exists to express. That copy is unchanged from
what the runtime does today.

Toolchain pinning is part of the exit criteria, not a footnote. `-O3` makes `em++` run `wasm-opt`,
and emsdk 3.1.72's bundled v119 rejects two WASM features present in `rustc` 1.98.0's precompiled
std. Emscripten takes Binaryen from its config rather than `PATH`, so the build must select it
explicitly with `EM_BINARYEN_ROOT` and assert the version it actually got.

## Phase 1: publish the targets

Exit: a fork release carrying 14 targets, a clean checkout building web, iOS simulator and Android
emulator with no local pins, and a browser support matrix including release Firefox and Safari.

This runs in parallel with everything from Phase 0 onward, and it is the likeliest critical path,
because it is the only work here that needs someone else's permission. The current release pin
`d7b4caa26289d4846e94ff54e63b586b9bbe9fd6` supplies nine targets;
`.github/workflows/release-libpdfium.yml` still lists nine; `ios-arm64`, `ios-sim-arm64`,
`android-arm64`, `android-x64` and `wasm32-eh` are `"status": "pending"`. The local fork builds all
five, verified against fork commit `7f0fa5dbe`, and the checkout used for the 2026-09-08 audit had
`push: false`.

The mobile libc++ problem is already fixed by `use_custom_libcxx=false` plus
`use_clang_modules=false`, with zero `__Cr` symbols across all four cached mobile archives, so
keeping those settings is the whole requirement there. Android additionally needs the matching
`libc++_shared.so` packaged and 16 KB page sizes verified; the audit's emulator returned 16384 from
`getconf PAGE_SIZE` and rendered 14 pages at 612 × 792 pt with 120 search hits. Apple needs a real
XCFramework with bundle-relative runpaths rather than the checkout-absolute paths in `project.yml`,
or the static arrangement that already built an unsigned ARM64 device slice, plus actual device
signing and a device run.

The browser floor needs honest measurement. The rebuilt module disassembles to 441 `try`, 367
`catch_all`, 285 `rethrow` and zero `try_table`, so it is legacy EH, and the only passes on record are
Chrome 152.0.7977.83 and cached Playwright Firefox 132.0 Nightly. A Nightly pass is not evidence about
release Firefox. Windows has never been linked at all, and musl has never been run.

## Phase 2: the read half

Exit: a mobile SDK that opens, renders, extracts text, searches and reads annotations, with no
JavaScript on the device, and a web engine whose routing table covers every read operation.

Slice order follows read-before-write and easiest-to-verify-first, which is the ordering
`docs/research/rust-core-port.md` already argues for:

1. `pages.geometry` (Phase 0) and `pages.render` / `pages.renderEncoded`, verified by pixel diff
   against the colored fixture with exact interior BGRA plus the loose whole-page gate.
2. `pages.text`, `search.query`, and the text layout and fold logic in `engine-core`, verified by
   `runSearchConformance`, `runPageTextConformance` and `runTextDivergenceConformance`.
3. `metadata.read`, `pages.list`, `actions.read`, `document.probeSecurityFile`,
   `document.checkPasswordPermissions`.
4. `annotations.listRawAll`, `annotations.listRawPage`, `annotations.listFullPage`,
   `annotations.renderAppearances`, verified by `runAnnotationReadConformance` and
   `runAnnotationAppearanceConformance`.
5. `attachments.list`, `attachments.readFile`, `annotations.readFile`, `pieceInfo.read`,
   `pieceInfo.applications`.

Each slice adds its kinds to the routing table, runs the relevant conformance suites in differential
mode, extends the frozen vectors and the cross-language corpus, and reports the WASM size delta. The
Swift and Kotlin facades grow slice by slice, which means the mobile SDK's capability is always a
prefix of this list, and that is the right shipping story: display and search first.

## Phase 3: the write half

Exit: the routing table has no TypeScript entries, `@embedpdf/engine-services` is a deprecated shim,
and the v4 branch cuts.

`annotations.create`, `annotations.update`, `annotations.delete`, `annotations.move`; the twelve
`forms.*` operations; `pages.rotate`, `pages.delete`, `pages.flatten`, `pages.extract`,
`pages.insert`, `pages.insertBlank`, `pages.move`; `redaction.apply`; `metadata.update`;
`pieceInfo.update`, `pieceInfo.clear`; `attachments.create`, `attachments.delete`; the four `fonts.*`
operations; `document.saveBuffer`, `document.saveFile`, `document.saveLayerBuffer`,
`document.renderPageFile`; and `document-session` itself.

The 2,431 lines in `runAnnotationMutationConformance.ts` are what makes this affordable. Annotation
write correctness stops being a matter of opinion the moment both engines run that suite.

Mutation safety carries a constraint the read half did not. Per
`.agents/skills/embedpdf-conventions/references/engine-services.md`, multi-step mutators validate
everything and check cancellation before the first native write, and after that apply boundary they
must not introduce a throw path, because these mutations are not rollback-atomic. Ordered batch verbs
record the current item as `failed`, mark the rest `skipped`, and finalize whenever state changed.
Rust's error handling makes it easy to add a `?` past the apply boundary and quietly break that. It
needs to be a review rule and a test, not a comment.

The costs to state plainly: for the whole of Phases 2 and 3 the worker ships both implementations,
so the WASM module grows until the last kind flips, and two implementations of the same behavior sit
in the tree. That is the price of not attempting 16,620 lines in one commit, and it buys a revertable
slice and a differential test per operation.

## Phase 4: hardening

Exit: the contracts the prototype deferred are implemented and tested, and the mobile threading model
passes the same gate the server's does.

Cancellation tiers implemented rather than declared. Admission limits and byte and document budgets:
native dispatch currently uses an unbounded `mpsc::channel`, and the 256 MiB bitmap cap bounds one
bitmap rather than aggregate memory, PDFium's internal allocations, or the number of open documents.
Engine replacement, because the prototype's Node addon holds a process-wide `OnceLock<Arc<Engine>>`
with no reset API. Crash containment by reusing `cloudpdf/server/src/runtime/`'s supervised host
design, since thread confinement is a correctness contract and not crash isolation: a malformed-PDF
segfault still kills the process that owns the threads. Input robustness at the lowest public
boundary, where `Document::from_bytes` copies and casts its length to `i32`.

And the threading gate, run against the mobile arrangement rather than assumed from the server's:
`testing/tools:epdf_thread_soak` under `EMBEDPDF_TSAN=1`, plus the single-thread embeddertest
regression with `EMBEDPDF_TLS_GLOBALS=false` as the baseline comparison.

## Phase 5: cut v4

Exit: v4 published, migration guide shipped, both engines' conformance green in CI, native SDKs
published to SPM and Maven.

The migration guide contains exactly the break budget table above and nothing else. If it grows, we
broke something we said we would not.

## What this plan defers, and what would change that

`packages/core/annotation` on mobile, 7,527 lines, deferred until mobile needs interactive drawing
rather than annotation read and write through the API. That single answer moves the total from roughly
20,000 lines to roughly 28,000, and it is the largest unknown in the plan. The trigger is a product
decision, not a technical one, so it should be asked rather than waited for. If it comes back yes,
the package is already shaped for it: expose `update(msg) -> [model, effects]` and
`scene(model, view) -> DisplayList`, one call per event and one per frame, nothing finer, and keep
TypeScript as the web path held to the Rust one by shared vectors.

`packages/core/main` and `packages/core/stage`, deferred indefinitely. Highest cost, lowest sharing
benefit, worst effect on the web bundle.

Coarse `EPDF*` wrappers whose only job is collapsing JavaScript crossings, dropped. The port deletes
those crossings for free. `EPDF*` functions that save work inside PDFium keep earning their place:
`EPDFText_GetCharGeometry` gathers boxes, quads, matrix and flags from one traversal instead of
recomputing per field, and that value survives the port intact. There is a real case for landing
`EPDF_GetPageGeometryFull` now as a bridge, since `pdfium-binding-benchmarks/cpp/ops.cc`'s
`cc_read_page_geometry` already passes the fairness gate and would buy 2.6x on web today, but it
becomes a public C API in a fork with no caller the moment Phase 0 lands. Not worth it if Phase 0 is
close, and Phase 0 is close.

`packages/core/js-sandbox` would be replaced rather than ported, by embedding `rquickjs` directly.
Nothing in `framework/*` or `viewer/*` is ever a candidate.

## What I don't know

Whether 16,620 lines of `engine-services` port well. That is Phases 2 and 3 and it is the actual
risk in this plan. Nothing measured so far says anything about it in either direction, and the
routing table exists precisely because I don't trust an estimate here.

Whether the cross-language conformance corpus is a week or a quarter. I have never built one. The
generator side looks straightforward because the cases are already written; the runner side has to
work in Swift, Kotlin, Rust, Node and a browser, and the last time we assumed a five-host test story
was easy we got two smoke tests and a grayscale golden.

Mobile performance, completely. Not one number in any of the three spikes touches iOS or Android
runtime behavior. The prototype rendered on a simulator and an emulator, which proves it runs.

And whether the Apple and Android facades stay thin. The rule is that no facade file contains a
decision, and I expect the first genuinely awkward platform conversion to be the moment someone puts
a small one in.
