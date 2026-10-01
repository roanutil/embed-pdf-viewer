# Performance findings, 2026-09-29

The Rust kernel on the plugins branch is slow because of its architecture, not its compiler flags.
Every store change re-serializes the whole kernel state to JSON, many times, and nearly every read
of the page registry deep-clones every page layout. Both scale with page count. Fixing three
things (a revision-cached `getState`, coalesced change notifications, and a shared page registry)
should remove most of the 12x gap to `main` on a 400-page document. Every build setting we found is
worth single-digit percent by comparison.

This comes from main-thread and worker CPU profiles, call counts, heap snapshots and JSON-size
probes taken in the harness browser, plus a read-only review of the Rust kernel, the wasm/TS wire,
the Rust engine core and the build settings. Nothing on the branches was changed. The benchmark
numbers it explains are in `RESULTS.md`.

Paths are relative to a checkout of the branch named in each section:
`research/multi-platform-rust-core-impl-plugins` at `bef223d9d` ("plugins") or
`research/multi-platform-rust-core-impl` at `c8085bd3c` ("impl"). Line numbers are at those commits.

## What the profiles show

All numbers are for the `plugins-lto` arm on `large.pdf` (400 pages), Chromium 140, Apple M4 Max.

A 60-wheel scroll spends 14.2 s on the main thread. 7.3 s of that is under JS `getState` calling
the wasm export `state_json`, which runs `epdf_kernel::store::Store::state_json` and serializes the
entire store. Precise coverage counted 920 `state_json` calls for 10 wheel events: 92 full-state
serializations per event. In those 10 events the page ran `JSON.parse` 66,692 times over 27.1 MiB
of text, 182 of those parses over 100 KiB (largest 167 KiB). On the 4-page `ebook.pdf` the largest
payload is 28 KiB, so the state grows with the document, at roughly 350 bytes per page.

Kernel self time is dominated by allocation and `Value` handling: dlmalloc `free`, `malloc` and
`unlink_chunk` (about 3.4 s), `IndexMap<String, serde_json::Value>` clone and insert (about 2 s),
SipHash (0.7 s) and `serde_json` serialization (about 1.7 s). The measurement plugin's
`refresh` then `publish` path costs 3.6 s during a scroll that never touches a measurement.

Opening the document takes 2.6 s of main thread, of which `state_json` is 859 ms and the
measurement plugin's `connect_viewports` then `ensure_loaded` path is 996 ms.

Heap snapshots after soak cycles 5 and 30 grow from 39.7 to 54.2 MiB. The growth is JS-side:
+12.9 MB of `ArrayBuffer` data across 250 buffers (about 10 per cycle, about 51 KB each), with
+600 `AbortController`/`AbortSignal` pairs and +600 `DOMRectReadOnly`. Strings grew only 236 KiB, so
this isn't a cache of JSON text. It looks like retained render requests, but the retainer isn't
traced yet.

In the engine worker, `handlePagesGeometry` costs 5.8 ms on `impl-ts` and 5.4 ms on `impl-rust`
across the whole `select` sequence, against 283 ms for `handlePagesRender`. Geometry is about 2% of
the worker's time there, which is why the Rust geometry reader shows no difference in the viewer.

## Plugins kernel: the fixes that matter

### 1. `getState` re-serializes everything on every read

`Wire.getState` (`packages/core-wasm/src/proxy.ts:418-454`) reuses its parsed state only while its
`version` hasn't moved, but `version` is bumped after every real `invoke` (`proxy.ts:588`), every
Rust-to-JS callback (`proxy.ts:363`) and every kernel notification. During a notify pass the hooks
alternate invokes and `getState` reads, so the cache never hits. Each `useT()`
(`packages/framework/react/src/i18n.ts:41`, about 35 call sites in the chrome) reads
`getState().plugins['i18n']`, and each miss rebuilds every slice from scratch
(`crates/epdf-kernel/src/store.rs:295-302`), including `DocumentMeta.pages`, all 400 page layouts
(`crates/epdf-kernel/src/types.rs:42`).

The fix is a Rust `state_revision()` bumped at mutation time, JSON cached per slice keyed on the
slice's `Rc`, and `useT` reading an i18n capability value instead of `getState`. This should remove
most of the 7.3 s in scroll and the 859 ms at open. The cost is small; per-slice objects must keep
reference identity while their revision is unchanged, which the existing slice cache already does.

### 2. Change notifications aren't coalesced

`SliceLease::commit` calls `emit_change` on every reducer result (`store.rs:135-152`), and one
listener is `listener.call0` into JS (`crates/epdf-web/src/exports.rs:559-569`), which notifies every
mounted hook synchronously, from inside the Rust call (`packages/core-wasm/src/kernel.ts:156-160`).
There are 200 to 300 such subscriptions, and Preact's `useSyncExternalStore` reads the snapshot again
during render and in a layout effect, because `get` is a new closure each render
(`packages/framework/react/src/runtime.tsx:59`, `:152`, `:181`). One scroll event makes several store
changes (camera, cursor, motion cause, camera rest), each a full pass.

The fix is to have the Rust listener set a dirty flag and call JS once at the end of the outermost
entry (`runtime::at_exit`, `crates/epdf-web/src/runtime.rs:77-86`), or from one microtask. It's about
ten lines. The proxy already tolerates a notification that lags the dispatch (`proxy.ts:41-51`), so
the risk is low. A stable `getSnapshot` in `runtime.tsx` cuts the Preact re-reads, but that adapter is
shared with `main`.

### 3. The page registry is deep-cloned on almost every read

`DocumentMeta.pages` is a bare `serde_json::Value`, and `PluginContext::document()`
(`crates/epdf-kernel/src/plugin.rs:278-286`), `ControllerContext::meta()` and `page_of()`
(`plugin.rs:717-758`), `Session::staged_meta()` (`crates/epdf-kernel/src/kernel.rs:158-160`) and
`Documents::meta_of`/`pages_of` (`kernel.rs:1660-1671`) all clone it. That's roughly 20,000
allocations per clone at 400 pages. Stage does five to ten of these per store change, sometimes to
read one integer (`crates/plugins/stage/src/sync/watchers.rs:65`), and every `build_scene` does two
even on a cache hit (`crates/plugins/stage/src/services/scene.rs:42`, `:65`, `:268`). The same
pattern is in measurement, annotation, selection, link, actions, redaction and page-edit. Render
already avoids it with `with_meta` (`crates/plugins/render/src/controller.rs:139-148`).

The fix is `Rc<DocumentMeta>` in `CoreState`, pages parsed once into `Rc<[PageLayout]>` with a
`pon -> index` map, and accessors that don't clone (`document_revision()`, `page_count()`,
`page_by_pon()`). It's about 25 mechanical call-site edits with no change in behavior, and it
removes most of the allocator, clone and SipHash time.

### 4. Measurement is O(pages²) at open and keeps publishing during scroll

`reconcile` (`crates/plugins/measurement/src/sync/viewports.rs:206-227`) runs `ensure_loaded` and
`refresh` for each of the 400 pages. Each refresh does a registry clone and a linear scan
(`services/store.rs:92-94`), two bridge calls into JS, and a `PageScale` dispatch whose reducer starts
with `let mut next = s.clone();` over the whole 400-entry map (`model.rs:71`). Each dispatch then runs
the full fan-out from items 1 to 3, including `reconcile` itself. The 400 async viewport answers
land over the next seconds and each publishes again (`viewports.rs:40-63`), which is the 3.6 s under
`publish` in the scroll profile.

The fix is items 1 to 3 plus a revision-keyed `pon -> PageMeta` cache in `Services`, `Rc` entries in
`MeasurementState.pages`, and optionally one batched `PageScales` action at open. The batching changes
action granularity, so the conformance vectors need checking.

### 5. Per-call waste on the wire

These are individually smaller but multiply with the notify fan-out:

- `useCapability` makes two crossings per read (`take_evicted` then `try_capability`) and hashes a
  KB-sized description string (`proxy.ts:407-411`, `:492-498`). Cache proxies in JS and invalidate on
  a documents-lifecycle revision.
- `usePages` returns all 400 layouts to read `.length` (`packages/framework/react/src/stage.tsx:386`,
  used three times in `viewer/chrome/src/ui/panels.tsx`). Read `pageCount` instead.
- `invoke` parses the answer before comparing it with the cached text (`proxy.ts:575-636`). Compare
  first, and skip the ref-marker regex when there are no markers.
- Every engine answer is JSON-encoded twice and parsed twice: the bridge resolves with JSON text
  (`packages/core-wasm/src/engine-bridge.ts:527`), and `from_js` stringifies that string again
  (`crates/epdf-web/src/runtime.rs:255-270`) before `engine_call` parses it (`engine.rs:137-139`).
- The PDF's bytes cross at open as base64 JSON, about six passes over 1.4 MB
  (`engine-bridge.ts:101-114`). Pass them as a `$host` handle; Rust never needs them.
- The toolbar re-renders on every store change and resolves every command, each resolution calling
  back into JS predicates (`packages/framework/react/src/toolbar.tsx:508-551`).
- Every export runs `after_entry` (`runtime.rs:160-200`) even for read-only calls.

### 6. Every hook re-runs on every change

Measured 2026-09-30 on `research/rust-kernel-wire` at `776f0c265` (the `plugins-getstate` arm),
with an unminified build and source maps; line numbers are at that commit. After items 1 to 5 the
60-wheel scroll is 1292 ms busy on the main thread. By source, 569 ms is Rust, 287 ms is the JS wire
(`packages/core-wasm/src/proxy.ts` and `engine-bridge.ts`), 127 ms is Preact and about 35 ms is the
app's own components. Rendering is not where the time goes. What rendering decides is how many wire
calls there are.

Every hook subscribes to the whole kernel change stream: `useKernelValue`, `useSelector` and
`useOptionalSelector` all pass `kernel.subscribe` to `useSyncExternalStore`
(`packages/framework/react/src/runtime.tsx:65`, `:158`, `:196`). Each change re-runs every selector
on the page, the wire's per-change answer cache is dropped on that same change, and so every re-run
is a wasm call. Tallying `Wire.invoke` over the scroll gave 10,150 `resolveCommand` misses (98 ms),
12,992 `isOpen` misses, and about 2,500 misses each for some 20 per-page reads (`getChromeSettings`,
`listPageItems`, `getSourceKey`, `listLinks` and the rest) that a scroll doesn't change.

Three changes, in order of payoff for effort:

- The toolbar forces a render on every change (`packages/framework/react/src/toolbar.tsx:509`) and
  resolves every command each time. The scroll rendered `Toolbar` 404 times, `CommandButton` 11,312
  times and `Icon` 9,696 times, 67 ms inclusive before counting the `resolveCommand` crossings. It
  should re-render only when a resolved command differs, which `resolvedCommandsEqual` already
  decides. `main` has the same line, so the TS kernel gains too, just less.
- The command predicates ask the shell `isOpen(id)` once per surface: `mode:view` alone checks all
  six mode surfaces (`packages/viewer/chrome/src/config/commands.ts:417`, and `:785`, `:860`, plus
  `packages/viewer/chrome/src/Shell.tsx:71`). `listOpenSurfaces()` takes no arguments, so reads of
  it can share one cached answer. (`menus.isOpen` in `MenuAnchoredButton` looks like the culprit but
  is a local array check.)
- A selector could skip its re-run while its capability's slice revision holds, using the per-slice
  revisions item 1 added. That's up to about 200 ms (the selector re-runs under `flush`), but some
  reads depend on other slices (`getPlan` reads the stage camera), so each capability has to declare
  what it reads. A wrong declaration shows up as stale UI, not as an error, which makes this the
  riskiest of the three.

The first two are prototyped on `research/rust-kernel-toolbar` at `e4d533d18` (the
`plugins-toolbar` arm). The toolbar no longer renders during a scroll: 0 renders where there were
404. It still resolves as many commands (10,125 against 10,150), because the change check now does
the reads the render used to. `isOpen` calls fell from 12,992 to 5,265, but only 2,025 of 5,670
`listOpenSurfaces` reads hit the cache: `Wire.bump()` drops every cached answer after any call that
missed (`packages/core-wasm/src/proxy.ts:309`), so the cache only helps calls made back to back. A
local profile of the scroll went from 1,277 to 1,126 ms busy, Preact from 132 to 39 ms.

The quick remote run (R=3, `results/2026-09-30T18-07-04-233Z-2023MBP16Grey-desktop`) agrees only in
part. `zoom` main-thread task time fell 12% and `open(ebook)` 5%, and `scroll` task time didn't move
beyond noise. But script time rose: 20% for `scroll`, 24% for `open(large)` and 15% for `annotate`.
The full R=10 run (`results/2026-09-30T18-24-11-976Z-2023MBP16Grey-desktop`) says it's real, and
so is the saving: against `plugins-getstate`, scroll task time fell 8% while its script time rose
12%, and `open(large)` script time rose 22% with task time unchanged.

Split into one arm per change (R=3, `results/2026-09-30T20-09-12-448Z-2023MBP16Grey-desktop`), the
toolbar gate alone (`plugins-item1`, `1efa42de6`) carries both effects: scroll task time -8%, script
+12.5%, `open(large)` script +24%. The surface predicates alone (`plugins-item3`, `eeea6d289`) show
nothing beyond noise on 15 of 17 main-thread rows, and read slower on `jump` script time (+36%, from a
noisy baseline) and `search` task time (+24%). On its own it isn't worth keeping.

Keeping cached answers while the store's revision holds (`plugins-bump`, `eba60e600`, on top of both)
is the clear win. Against the toolbar gate alone it cuts scroll task time 10.5% and script time
7.6%, `zoom` task time 12%, and `select` task time 25% with 12 ms off its settle. Against
`plugins-getstate` that's scroll task time -18% and wall time -8%, `zoom` -13% and `select` -31%.
The `open(large)` script cost of the gate is still there (+23%).

That cost isn't more JavaScript. A main-thread trace of `open(large)` (the `devtools.timeline`
category, self time) shows the same JS on both arms, 419.8 ms on `plugins-getstate` and 420.2 ms
with the gate. What moved is where it runs: `FunctionCall` went from 292.6 to 356.1 ms and
`RunMicrotasks` from 127.2 to 64.1 ms. A Preact render is scheduled as a microtask, and the
`ScriptDuration` metric `mainScriptMs` reads doesn't count microtask JS; the change check runs in the
kernel's change listener, which it does count. So `mainScriptMs` undercounts any arm that renders
through microtasks, `main` included, and `mainTaskMs` is the one to trust. Item 3 is reverted on
`research/rust-kernel-bump` (`dd1b86fcc`).

Without item 3 the cache fix holds (R=3, `results/2026-09-30T20-45-12-681Z-2023MBP16Grey-desktop`).
Against the gate alone, scroll task time falls 8.5%. Against `plugins-getstate`, the gate and the
cache fix together make scroll task time 16.5% lower and wall time 7.4% lower, `zoom` 16%, `select`
17%, `annotate` 12% and `open(ebook)` 6.4%, with `zoom2SettleMs` 19 ms sooner.

The third fix, prototyped without declarations (`research/rust-kernel-read-sets` at `685a5472b`, the
`plugins-reads` arm): the store records which slices, and whether core, each capability call reads
(`Store::track_begin`/`track_end`), and the wire keeps a read's answer across store changes until
one of those parts moves. A read that called JavaScript back, read nothing from the store, or read
the whole tree isn't kept, and a call that isn't a read retires the kept answers of its own
capability (the wire learns each member ref's capability from its pinned description). It works as
a cache: in the 60-wheel scroll, misses in the 40 busiest members fell from 61,341 to 27,994 and the
per-page reads from about 2,200 misses each to about 1,300. It doesn't pay. Against `plugins-bump`
(R=3, `results/2026-09-30T22-28-55-068Z-2023MBP16Grey-desktop`) no row moves beyond noise; the point
estimates are 2 to 3% slower for `scroll` and 5 to 8% slower for `jump` and `zoom` task time, 11 to
17% faster for `select`, `search` and `annotate`. What a miss costs now is small, and each kept
answer pays for a `last_reads` crossing and parse, a `BTreeSet<String>` per tracked call, and a
`state_revisions` parse per change. We'd leave it out. Its residual risk also stays: a read that
depends on another plugin's state outside the store, changed by a call that doesn't touch the
store, would read stale.

The selector re-runs themselves are now mostly JavaScript: `plugins-reads` spends 654 ms of JS in
the scroll (`mainJsMs`) against 306 ms on `main`, which runs the same hooks against a TypeScript
kernel.

That run also prices the trace behind `mainJsMs`. Against the untraced run before it
(`results/2026-09-30T20-45-12-681Z-2023MBP16Grey-desktop`), most timing rows on `main` and
`plugins-bump` moved by 3% or less, which is inside run-to-run noise. `zoom` settle moved more on
both, 16% and 11%, so traced and untraced `zoom` timings shouldn't be compared.

The toolbar gate on the TypeScript kernel (`research/rust-core-toolbar-gate` at `30f4295d7`, the
impl pin plus the gate, the `impl-gate` arm) is a clear win too, and a larger one in proportion
(R=3, `results/2026-10-01T01-22-09-931Z-2023MBP16Grey-desktop`, A/A 39 of 39). Against `impl-ts`,
`open(large)` task time falls 38% (264 to 164 ms), scroll 16% (400 to 337 ms), `zoom` 15%,
`annotate` 14% and `search` 37%, and a jump settles 16 ms sooner. JavaScript time falls 21 to 61%
on the same rows. One row reads slower, `open(ebook) / readyMs` by 3.4 ms, which is measured
before any document exists and so before the gate runs. `main` has the same subscription
(`packages/framework/react/src/toolbar.tsx:509`), so the gate would help it as well.

### Serialization formats

The first win isn't a faster encoder but not serializing at all: items 1 and 2 remove the repeated
full-state builds. What remains is dominated by building `serde_json::Value` trees, not by writing
text. Serializing typed structs straight to a reused buffer with `serde_json::to_writer`, caching
each slice's text, and swapping SipHash for `rustc-hash` or `ahash` in the kernel's own maps all help
more than a format change would. The SIMD JSON crates (`simd-json`, `sonic-rs`) don't target
`wasm32-unknown-unknown` as built.

On the JS side, `JSON.parse` is native and hard to beat for building object graphs. MessagePack
(`rmp-serde` with `msgpackr`) or CBOR (`ciborium` with `cbor-x`) would shrink numeric payloads and
might match `JSON.parse` speed, but they change both ends of every call. We haven't measured them.
`serde-wasm-bindgen` avoids text entirely but makes one boundary call per field, which usually
loses on large payloads. For bulk numeric data (page layouts, glyph geometry) packed `Float32Array`
views over wasm memory are the clear answer: no parse at all, and transferable across the worker
boundary.

## The heap leak

About ten `ArrayBuffer`s and 24 `AbortController`s survive each soak cycle. The next step is a
retainer path from a heap snapshot. Separately, `Session.staged_meta` is never cleared after
`commit_ready` (`kernel.rs:617-633`), so each open document holds a second full copy of its pages;
`staged_meta.take()` fixes that in one line.

## Engine, on every branch

These apply to `main` too, since they're in the shared TypeScript engine and its build.

The engine's final link is unoptimized. `packages/engine/runtime/build/compile.esm.sh` (`:35-50` on
`main`, `:80-97` on impl) passes no `-O` and sets `-sASSERTIONS=1`. That wraps all 915 exports in
checking wrappers, links emcc's debug system libraries, and blocks metadce. Linking with
`-O2 -sASSERTIONS=0 -sSTACK_OVERFLOW_CHECK=0` cuts the wasm 8% raw (`wasm-opt -O3` alone takes the
impl engine from 11,084,440 to 10,198,993 bytes) and should speed up the worker by an estimated 2 to
8%. It can be tested without a branch edit, because emcc appends `EMCC_CFLAGS` last. The cost is
that a runtime misuse becomes an opaque trap instead of a named assert.

The TypeScript per-glyph cost is mostly glue. Each PDFium call goes through `cwrap`, a `bridge`
closure with `args.map`, `ccall` and the assert wrapper (`packages/engine/runtime/src/wasm/wasm-runtime.ts:246-272`,
`:473-487`). Each returned pointer allocates a BigInt (`wasm-runtime.ts:43-45`), and each f32 read goes
through `getValue` (`structs.ts:69-71`), about 20 per glyph. Binding raw exports and reading
`HEAPF32` views in hot loops helps geometry and annotation reads on every branch.

Search always uses the TypeScript geometry reader, with no cache, for up to 64 matched pages per
slice (`packages/engine/services/src/features/search/SearchReader.ts:139`), and its folding loop
allocates several strings per code point (`packages/engine/core/src/search/fold.ts:60-86`). Caching
geometry per page by `mutationSeq`, as `pageCorpusCache.ts` already does for text, and an ASCII fast
path in folding are both small.

Geometry travels as about 1.7 MB of JSON per dense page, with every f32 widened to f64 and printed to
17 digits (`crates/epdf-engine/src/features/geometry.rs:344-360`). It's then structured-cloned to the
main thread with no transfer list (`WorkerHost.ts:1299`), about 25,000 objects per page. Packed
typed arrays on the transfer list fix both, at the cost of changing the `PageGeometrySnapshot` DTO.

Every text read loads and closes the text page (`PageGeometryReader.ts:94`, `PageTextReader.ts:46`,
`geometry.rs:264`), and a matched search page loads it twice. Caching `FPDF_TEXTPAGE` in
`PagePtrPool` needs its lifetime and leak checks extended.

## Build settings

These are cheap and each worth measuring as its own arm, but none changes the picture above.

| Change | Evidence | Expected |
| --- | --- | --- |
| Real LTO for `epdf-web`: `crate-type = ["cdylib", "rlib"]` makes Cargo skip `-C lto` | measured with the `plugins-lto` arm | about 10% on kernel-heavy rows |
| `wasm-opt -O3 --strip-debug` after `wasm-bindgen` (`crates/epdf-web/scripts/build.sh` runs neither) | measured on the built wasm | 11.3 to 8.3 MB raw, brotli 2.13 to 2.03 MB; speed 0 to 5%, unmeasured |
| WebAssembly SIMD: no `+simd128` in any `target_features` section, no `-msimd128` in the fork | artifact inspection | 0 to 15% on render rows, a guess |
| `main`'s payload uses emsdk 3.1.72 without bulk-memory: large `memcpy` and longjmp go through JS imports | artifact imports | within the noise band for these fixtures |
| Two copies of `preact/compat` in every bundle, including the shipped viewer (`compat.mjs` via the alias, `compat.module.js` via `compat/client`) | bundle contents | 1 to 3% of main-thread script, plus a correctness risk |
| Stage B's 65-op `Host` is linked into every Rust-core payload (`compile.esm.sh:68-69`) | export list | most of the 7.19 to 10.57 MiB engine growth |
| `serde_json`'s `float_roundtrip` is on across the kernel | crate features | about 2x on float parsing; risky for JS parity |
| PDFium with `-flto` | none | 5 to 15%, a guess, at much longer builds |

Checked and left alone: the kernel already aborts on panic (`wasm32-unknown-unknown` defaults to it),
the engine needs unwind for its poisoned-session path, the allocator and memory settings are
reasonable, the exnref exception encoding costs nothing when nothing throws, `wasm-bindgen` already
uses externref, both wasm files are streamed, and the Vite build is a clean production build.

## What the harness should measure next

The `select` flow doesn't time `pages.geometry`, because page 1's geometry is loaded before `t0`.
`impl-ts` against `impl-rust` needs a flow that reads cold geometry on many dense pages, and the gate
should prove the Rust route ran rather than only that the payload contains the core
(`lib/gate.mjs`). The runs are cold, so wasm starts in Liftoff; a diagnostic arm with
`--js-flags=--no-liftoff` would show how much of `openedMs` is baseline-tier execution.

The cheapest high-value next step is items 1 and 2 on the plugins branch, re-measured as a new arm.

## Not tried yet

As of 2026-09-30, with sections 1 to 6 prototyped and the kept fixes merged into
`research/multi-platform-rust-core-impl-plugins` (`416ee81b8`), these are still open. Each is
described in the section named. The first five are cheap, and two of them are already measured.

1. LTO in the branch's own build (Build settings). Tried since, and measured on its own below: it
   no longer pays.
2. The engine's final link with `-O2 -sASSERTIONS=0` (Engine, on every branch).
   `packages/engine/runtime/build/compile.esm.sh:88` still passes `-sASSERTIONS=1`.
3. `wasm-opt -O3 --strip-debug` on the kernel wasm (Build settings). Tried since, and measured on
   its own below: a small win, kept.
4. `staged_meta.take()` (The heap leak). Tried since, and left alone: it isn't a second copy any
   more, and the conformance vectors pin what it holds. See below.
5. The second copy of `preact/compat` in every bundle (Build settings). Tried since, and kept:
   see below.
6. Typed serialization with `serde_json::to_writer` into reused buffers, and `rustc-hash` for the
   kernel's own maps (Serialization formats).
7. The engine glue and geometry work shared with `main` (Engine, on every branch): raw exports and
   `HEAPF32` reads, a per-page search geometry cache and an ASCII folding fast path, geometry as
   transferable typed arrays, and a cached `FPDF_TEXTPAGE`.
8. The PDF's bytes as a `$host` handle instead of base64 at open (section 5). About 20 ms of main
   thread, and every Rust call site that takes bytes changes.
9. SIMD, PDFium with `-flto`, and `float_roundtrip` (Build settings). All three expectations are
   guesses, and `float_roundtrip` risks JS parity.
10. The harness gaps above: a cold-geometry flow, a gate that proves the Rust route ran, and a
    `--no-liftoff` diagnostic arm.

Decided against: a binary wire format (Serialization formats; JS-side decode is about 2% of busy
time), skipping `after_entry` on read-only exports (section 5; the capability cache removed most of
what it would save), and read sets for the hook selectors (section 6; no row moved).

### LTO and `wasm-opt`, each on its own

Both are prototyped on the plugins tip (`51130b042`) as separate branches, so either can go
without the other, and timed against that tip built the branch's own way, without LTO (R=3,
`results/2026-10-01T02-19-00-897Z-2023MBP16Grey-desktop`, A/A 39 of 39).

LTO (`research/rust-kernel-build-lto` at `99d1a8c04`, the `plugins-buildlto` arm) builds `epdf-web`
as a cdylib only in `crates/epdf-web/scripts/build.sh`, since the crate's integration tests need the
rlib in `Cargo.toml`. No row moves beyond noise: the point estimates are 3% faster on scroll task
time, 0.5 to 1% slower on the open rows, and 15 ms later on jump settle with an interval from -12 to
+49 ms. It costs 921,595 bytes of kernel wasm (+8.1%), 117,163 brotli (+6.3%). The 10% it was
worth on `plugins-lto` came from a kernel that spent most of every row in Rust; the fixes since
took most of that time away. It's built exactly as the harness's `rust-kernel-lto` recipe builds
it (the same 12,254,473 bytes, and no difference from `plugins-bump` on any row). We'd leave it
out.

`wasm-opt -O3 --strip-debug` after `wasm-bindgen` (`research/rust-kernel-wasm-opt` at
`e981e0cb9`, the `plugins-wasmopt` arm) takes the kernel from 11,332,878 to 8,311,869 bytes
(-26.7%), 1,849,657 to 1,776,551 brotli (-4.0%). `readyMs` is 7.8% sooner on `open(ebook)` (82.1
to 75.7 ms) and 6.4% on `open(large)`: a smaller module compiles and instantiates sooner. No row
is slower, and the open and scroll point estimates are 2 to 5% faster. Its cost is a build
dependency: `build.sh` now needs binaryen's `wasm-opt` (version 133 here) wherever the kernel is
built. It's merged into `research/multi-platform-rust-core-impl-plugins` (a fast-forward to
`e981e0cb9`); the LTO branch is deleted.

### The engine's optimized link

`-O2 -sASSERTIONS=0` in the final emcc link (`packages/engine/runtime/build/compile.esm.sh`),
measured on both research branches (`research/rust-core-engine-link` at `7ed02e399` on impl,
`research/rust-kernel-engine-link` at `dfb5c43c8` on plugins; R=3,
`results/2026-10-01T03-11-14-653Z-2023MBP16Grey-desktop`, A/A 38 of 39). The engine wasm goes from
11,084,440 to 10,065,349 bytes (-9.2%), 2,999,741 to 2,986,548 brotli. On impl, `open(ebook)`
opens 6% sooner and `open(large)` opens 4% and settles 7% sooner (226.1 to 211.3 ms); on plugins
the open rows don't move beyond noise. `search` main-thread task time reads slower on both (2.5 to
3.3 ms on impl, 9.0 to 17.1 ms on plugins against a baseline p95 of 15.3 ms).

The link keeps `-sSTACK_OVERFLOW_CHECK=1`, which `ASSERTIONS=1` used to turn on: a cookie at the
top of the C stack, checked at the end of each tick, so a stack overflow aborts instead of silently
overwriting the data below the stack. What turning assertions off costs otherwise is the JS glue's
named aborts (a call with the wrong argument count, a runtime method that wasn't exported), and it
cost us twice before it landed:

- Without assertions, emscripten's `cwrap` returns the raw export for every all-numeric signature
  and counts `'boolean'` as numeric (`library_ccall.js`), so C `bool` results came back as 0 and
  1. Ten engine tests failed that way. `toJsResult` in `packages/engine/runtime/src/wasm/wasm-runtime.ts`
  now maps them to booleans whatever the build.
- The argument-count assertion is what exposed a real bug on both research branches: their
  `wasm32-eh` engine is linked with emcc 4.0.0, which turns `WASM_BIGINT` on by default, while the
  TypeScript runtime splits each i64 argument into two i32 halves. `EPDFSig_Seal` was called with
  13 arguments where it takes 10, and signing, attachment extraction and revision reads aborted (47
  of the 68 engine test failures). `-sWASM_BIGINT=0` fixes it (`fba088f7d` on impl, `dea3e7e77` on
  plugins). With assertions off it would have read wrong arguments instead of aborting.

Both conditions are now met. `build/verify-wasm-abi.mjs` reads the linked wasm's export types and
fails the link when a bound function's parameter count doesn't match what `wasm-runtime.ts` passes
(an i64 counts twice) or an export takes an i64; the old pin fails it on 16 functions, the fixed
builds pass all 918. The engine-link branches are rebased onto the fixed tips
(`research/rust-core-engine-link` at `2a3bb96ed`, `research/rust-kernel-engine-link` at
`48fda657e`), and every arm fails the same 21 engine tests as its base.

Re-measured on the rebased branches (R=3, `results/2026-10-01T03-48-18-182Z-2023MBP16Grey-desktop`,
A/A 39 of 39), no row is slower on either branch, and the `search` slowdown above doesn't
reappear. On impl, `open(large)` opens 5.6% sooner (143.4 to 135.3 ms), `jump` main-thread task
time is 14% lower (57.6 to 49.4 ms) and the first zoom step settles 8.6 ms sooner; on plugins,
`open(large)` opens 3.8% sooner (204.0 to 196.3 ms). The engine is 11,120,042 to 10,098,549 bytes
(-9.2%), 3,019,872 to 2,994,491 brotli. We'd keep it.

Of the 21 engine test failures left on every arm, 17 need the native Node build
(`pdf-runtime.node` and `libembedpdf.dylib`), which the harness's worktrees don't build; with it
present those 7 files pass, 38 of 38. The other 4 looked like the Rust core's own but were test bugs:
the rehearsal test read `doc.security.encryptionState`, where the state is
`doc.security.current.encryption` (`test/session-rust-rehearsal.test.ts:59`, `:75`), and the parity
test addressed pages by `layout.pageObjectNumber`, where the layout carries a `PageRef`
(`test/geometry-rust-parity.test.ts:65`). With those reads fixed (`65d9798d6` on impl, `4f9ceee9c`
on plugins) the session and parity tests pass, 7 of 7, so the Rust session and geometry reader
agree with TypeScript on both fixtures. What's left is the 17 that need the native build.

#### `-O2` and assertions, apart

The link above changes two things, so each is measured on its own as well: `-O2` with assertions
kept (`research/rust-core-engine-o2` at `977a874e4`, `research/rust-kernel-engine-o2` at
`785e68335`), against the same base and the full link (R=3,
`results/2026-10-01T04-27-30-603Z-2023MBP16Grey-desktop`, A/A 39 of 39).

| Build | Engine wasm | Brotli |
| --- | ---: | ---: |
| base | 11,120,042 | 3,019,872 |
| `-O2`, assertions on | 10,164,511 (-8.6%) | 3,013,134 (-0.2%) |
| `-O2`, assertions off | 10,098,549 (-9.2%) | 2,994,491 (-0.8%) |

`-O2` is 94% of the size. On speed, only the open rows move, and only by 3 to 7%. On impl, `-O2`
alone opens `ebook.pdf` 4.7% sooner, and turning assertions off on top shows no detectable
difference. On plugins, `-O2` alone shows none, and turning assertions off on top opens `ebook.pdf`
4% sooner and settles it 3.4% sooner. No row is slower in any of the four comparisons. The 14% on
`jump` in the run before didn't reproduce.

We'd take `-O2` and keep assertions: it gets nearly all the size and part of the speed, and what
assertions cost is a few percent on one row of one branch. They already caught one real bug here
(the i64 split), and the named abort is worth more than 3% on opening a small document. That's
what's merged: `-O2` with assertions on, plus the `toJsResult` bool fix, which the raw-export
fallback needs in any build (`a1cc26fe3` and `ec18b766d` on impl, `f625ccfe4` and `fa97fb3e8` on
plugins). The assertions-off branches are deleted.

### `staged_meta`

The heap-leak section's one-line fix is out of date and wrong. Since the page-registry fix,
`DocumentMeta.pages` is an `Rc<Value>` (`crates/epdf-kernel/src/types.rs:45`), so the staged meta
shares the pages core holds rather than copying them. What it does hold on to is the meta the
document opened with, until the document closes; after a page edit replaces the registry, that's
one stale page registry per edited open document.

And that stale meta is behavior the TypeScript kernel has, which two conformance vectors pin:

- `staged_meta.borrow_mut().take()` in `commit_ready` fails `plugins/page-edit.json`,
  `a-captured-capability-outlives-its-document`: a capability captured before the close reads its
  document's meta through `ctx.document()` and must reach the lifetime guard ("document instance
  a#1 was closed"); with the meta gone it fails earlier, "no page 20".
- Parking core's current meta at close as well fixes that and fails `plugins/stage-feeds.json`,
  `page-mutations-refit`, step 11: stage's refit during the close has to see the pages the document
  opened with to report what the TypeScript kernel reports.

So the kernel keeps it, at the cost of one page registry per page-edited open document, until
close. Changing it means changing the TypeScript reference and the vectors with it.

### One copy of `preact/compat`

The viewer's `react` and `react-dom` aliases resolve `preact/compat` the way Node does, to
`compat/dist/compat.mjs`, but `compat/client.mjs` imports `preact/compat` by name and Vite resolves
that the way a browser does, to `compat/dist/compat.module.js`. So every bundle carried both, the
shipped viewer included, and each copy patches Preact's `options` hooks when it loads. Aliasing
them to `compat.module.js` leaves one (source maps of the built viewer list `compat/client.mjs`
and `compat/dist/compat.module.js` only). The bundle shrinks by about 5.4 KB raw and doesn't move
compressed.

Measured with the harness's `dedupeCompat` switch, each arm against its own build (R=3,
`results/2026-10-01T06-10-46-897Z-2023MBP16Grey-desktop`, A/A 39 of 39): on `main`, `open(large)`
main-thread task time is 7.1% lower (262.7 to 244.0 ms) and JavaScript time 7.7% lower; on impl
and plugins no row moves beyond noise. `main`'s toolbar still re-renders on every change, which
the toolbar gate removed on the research branches, so there's less there for a second set of hooks
to slow down. Kept on both research branches (`packages/viewer/main/vite.config.ts`, and plugins'
`examples/rust-kernel-eval/vite.config.ts`) for the single instance it gives. The cost is an alias
to a file inside preact's package, which a preact release that renames its dist files would break
at build time. `main` has the same aliases and would gain the 7%.
