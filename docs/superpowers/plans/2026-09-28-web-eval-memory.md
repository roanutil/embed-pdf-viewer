# Bounding memory in the Rust kernel web eval build

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A long session in `examples/rust-kernel-eval` holds a bounded amount of memory, the
way the TypeScript viewer does, so an evaluator never has to reload because the heap grew.

**Architecture:** Every table that crosses the JS/Rust boundary today only grows. Each one gets a
release path tied to the lifetime of the thing it holds. Rendered images stop crossing into Rust
as base64: Rust keeps only a host id and a byte count, and tells JS when its last reference to the
image drops. Promises are handed to JS once and then forgotten. Functions and refs are released
when the other side drops them, through `Drop` in Rust and `FinalizationRegistry` in JS.

**Tech Stack:** Rust 2024, `wasm-bindgen` 0.2.127, TypeScript, Vitest (Node, `--expose-gc` for
the release tests), the Chrome DevTools MCP for heap snapshots.

## Global Constraints

- Same scope as `docs/superpowers/plans/2026-09-27-web-rust-plugins.md`: dev-only, nothing
  published, no source changes in `packages/framework/*`, `packages/viewer/*`, `packages/core/*`
  or `packages/plugin/*`.
- The vectors keep passing with unchanged counts. A change in a plugin crate outside its
  `harness` module is an addition whose default (`None`, or no host hook) reproduces today's
  behaviour exactly. After each task, run
  `cargo test -j 4 -p plugin-conformance -p epdf-kernel -p epdf-plugin-render -p epdf-plugin-stage`.
- Release must never free something still reachable. A released id that comes back is a bug the
  code has to report loudly (`console.error` plus a thrown `Error`), never a silent `undefined`.
- Machine safety as before: `-j 4`, no `cargo-mutants`, no `git stash`.

---

## Where the memory goes today

Seven tables grow for the whole session. The first two are most of the bytes.

1. The engine bridge's host table (`packages/core-wasm/src/engine-bridge.ts:244-254`) keeps every
   `PageImageHandle` the engine returns, and each one holds its encoded image as a `Uint8Array`.
   Nothing ever deletes from it.
2. The same image also crosses into Rust as base64 inside the handle's `source.bytes`
   (`$bytes`), where `PageImageHandle` (`crates/plugins/render/src/raster_store.rs:28-59`) keeps
   it as `Rc<Value>`. So every tile is held twice, and base64 is a third bigger than the bytes.
   `cost()` also decodes that base64 on every call to measure its length.
3. The harness promise table (`crates/plugin-conformance/src/harness.rs:99,205-227`) keeps every
   promise handed to JS, with its settled value, forever.
4. The harness ref table (`harness.rs:98,190-197`) keeps every function handed to JS. Stage's
   `listVisiblePages` registers six new transform refs per visible page on every call
   (`crates/plugins/stage/src/harness.rs:211-240`), so it grows per frame while scrolling, even
   though the shim rebuilds transforms in JS and never calls those refs.
5. The shim's function table (`packages/core-wasm/src/proxy.ts:114-116`) keeps every JS callback
   passed into Rust, and everything those closures capture.
6. The capability memo in `WasmKernel` (`crates/epdf-web/src/exports.rs:138,346-351`) keeps the
   capabilities of closed documents.
7. `kernel.destroy()` (`packages/core-wasm/src/kernel.ts:162-166`) never unsubscribes the Rust
   listener or calls `free()` on the `WasmKernel`, so a viewer remount keeps a whole kernel alive.

The TypeScript viewer holds only what its raster store's LRU budget allows. The Rust raster store
has the same budget and does evict, but eviction frees only its own copy: tables 1 and 2 never
learn about it.

## Tasks and estimates

| Task | Deliverable | Fixes | Estimate |
|---|---|---|---|
| 1 | Counters and a soak test that shows the growth | nothing yet | half a day |
| 2 | Images cross as a host id and a byte count, not base64 | 2 | half a day |
| 3 | Rust releases a host image when its last handle drops | 1 | 1 day |
| 4 | Promises are taken once | 3 | 2 hours |
| 5 | Refs are released when JS drops them; stage stops minting transform refs | 4 | 1 day |
| 6 | JS callbacks are released when Rust drops them | 5 | 1 day |
| 7 | Closed documents leave the memo; `destroy` frees the kernel | 6, 7 | half a day |
| 8 | Soak and heap-snapshot verification; findings updated | | half a day |

About five days for one person. Tasks 2 and 3 are the ones that matter for "hundreds of MB". If
the budget is tight, stop after Task 4 and re-measure.

---

### Task 1: Make the growth visible

**Files:**
- Modify: `crates/plugin-conformance/src/harness.rs` (counts accessor)
- Modify: `crates/epdf-web/src/exports.rs` (`debug_counts` export)
- Modify: `packages/core-wasm/src/engine-bridge.ts`, `packages/core-wasm/src/proxy.ts`,
  `packages/core-wasm/src/kernel.ts` (counts)
- Modify: `examples/rust-kernel-eval/src/main.ts` (expose `window.__epdfCounts()`)
- Create: `packages/core-wasm/test/soak.test.ts`

**Interfaces:**
- Produces:
  - `impl Harness { pub fn counts(&self) -> Value }`: `{"refs": n, "promises": n}`, counting
    live entries (so later tasks, which empty slots, report correctly).
  - `WasmKernel::debug_counts(&self) -> String`: JSON of the harness counts plus
    `{"described": n}`.
  - `bridge.counts(): { hosts: number; documents: number }`.
  - `Wire.counts(): { fns: number; refs: number; proxies: number }`.
  - `kernelCounts(kernel: Kernel): Record<string, number>` exported from `@embedpdf/core-wasm`,
    merging all three.

- [ ] **Step 1: Write the soak test.** Open `ebook.pdf` from `packages/viewer/main/public/` with
  stage, render and interaction over the real Node engine. Loop 300 times: `panBy(0, 40)`, read
  `listVisiblePages()`, call render's `getPlan` for each visible page, and drain with
  `await new Promise(r => setTimeout(r, 0))`. Record `kernelCounts` and
  `process.memoryUsage().heapUsed` at iterations 50 and 300. The assertions for bounded growth
  come in Task 8. For now the test prints both samples and asserts only that the counters exist.
- [ ] **Step 2: Implement the counters** and run the test:
  `pnpm --filter @embedpdf/core-wasm run test:wasm -- soak`. Record the two samples in the task
  report. That is the baseline the other tasks measure against.
- [ ] **Step 3: Commit** "Count what the Rust kernel eval build holds".

### Task 2: Stop sending image bytes into Rust

**Files:**
- Modify: `packages/core-wasm/src/engine-bridge.ts` (host encoding of a `PageImageHandle`)
- Modify: `crates/plugins/render/src/raster_store.rs` (`cost` reads a byte count)
- Test: `crates/plugins/render/tests/` (a unit case for the new cost branch),
  `packages/core-wasm/test/engine-bridge.test.ts`

**Interfaces:**
- Produces: a host image crosses as its data members plus `$host`, with
  `source: {"kind": "bytes", "bytes": {"$byteLength": n}}` instead of `{"$bytes": base64}`.
  `PageImageHandle::cost()` returns `n` for that shape. `PageImageHandle::bytes()` returns `None`
  for it. No Rust caller outside `harness` modules reads `bytes()`; check with
  `grep -rn "\.bytes()" crates/plugins` first, and stop if one does.

- [ ] **Step 1: Failing Rust test.** Build a `PageImageHandle` from
  `json!({"source": {"kind": "bytes", "bytes": {"$byteLength": 4096}}, "width": 10, "height": 10})`
  and assert `cost() == 4096.0`. Run `cargo test -j 4 -p epdf-plugin-render`. Expected: FAIL,
  `NaN`.
- [ ] **Step 2: Add the branch** in `cost()`, before the base64 path: if `source.bytes` is an
  object with a numeric `$byteLength`, return it.
- [ ] **Step 3: Failing TS test.** In `engine-bridge.test.ts`, render a page and assert the JSON
  Rust would receive has `source.bytes.$byteLength` equal to the handle's `bytes.byteLength`, and
  no `$bytes` anywhere in the text.
- [ ] **Step 4: Change the encoder** in `engine-bridge.ts`: while encoding a host object, replace a
  `Uint8Array` at `source.bytes` with `{"$byteLength": bytes.byteLength}`. `decode` already turns
  `$host` back into the original live object, so JS still has the bytes.
- [ ] **Step 5: Run** both tests plus the render vectors (unchanged count), then commit
  "Keep rendered image bytes on the JavaScript side".

The cost: a Rust caller that wanted the pixels would now get `None`. None exists today. A future
one needs a `bridge.host(n)` round trip, which is also cheaper than base64.

### Task 3: Release a host image when Rust drops its last handle

**Files:**
- Modify: `crates/plugins/render/src/host_contract.rs` (`RenderHost::release_image`)
- Modify: `crates/plugins/render/src/raster_store.rs` (`PageImageHandle` gains a release guard)
- Modify: the render controller where it builds the `RasterStore`, to pass the hook through
- Modify: `crates/plugins/render/src/harness.rs` (`live_host` wires `releaseImage`)
- Modify: `packages/core-wasm/src/engine-bridge.ts` (`bridge.release(n)`),
  `packages/core-wasm/src/plugins.ts` (the render factory supplies the hook)
- Test: `crates/plugins/render/tests/`, `packages/core-wasm/test/soak.test.ts`

**Interfaces:**
- Produces:
  - `RenderHost { ..., pub release_image: Option<Rc<dyn Fn(&Value)>> }`, `None` by default.
  - `PageImageHandle(Rc<Inner>)` with `Inner { value: Value, release: Option<Rc<dyn Fn(&Value)>> }`
    and `impl Drop for Inner` calling `release(&value)`. `PageImageHandle::new(value)` keeps
    today's signature with `release: None`; `PageImageHandle::with_release(value, release)` is
    the addition.
  - `bridge.release(n: number): void` deletes host `n`. A later `host(n)` throws
    `[engine-bridge] host value n was released`.

Rust calls the hook exactly when the last clone of a handle drops: eviction from the raster store,
and the paint plans and tile state that referenced it. JS keeps a released image alive only as long
as something on the page still holds the decoded object, which is the normal GC contract.

- [ ] **Step 1: Failing Rust test.** A `RasterStore` with a budget of one image and a recording
  `release_image` hook. Fetch key `a`, then key `b`. Assert the hook saw `a`'s JSON once and `b`
  not at all. Then drop the store and assert it saw `b`.
- [ ] **Step 2: Implement** the guard and plumb the hook from `RenderHost` into the store.
- [ ] **Step 3: Wire it live.** In the render adapter's `live_host`, a `releaseImage` `$fn` spec
  becomes the hook. In the shim's render factory, pass
  `releaseImage: (image) => bridge.release(image.$host)`.
- [ ] **Step 4: Extend the soak test.** After 300 iterations, `counts().hosts` is at most the
  raster budget's image count plus the images on screen.
- [ ] **Step 5: Run** the render vectors (unchanged), the soak test, and a browser smoke of the
  eval app: scroll `ebook.pdf` to the end and back, and check that every tile repaints (a
  released-too-early image shows as a blank tile and a thrown `host value n was released`). Commit
  "Release a rendered image when the Rust side drops it".

### Task 4: Take each promise once

**Files:**
- Modify: `crates/plugin-conformance/src/harness.rs` (`promises` becomes slot-reusing on a live
  harness)
- Modify: `crates/epdf-web/src/exports.rs` (`promise(id)` takes)

**Interfaces:**
- Produces: on a live harness, `Harness::take_promise(id) -> Option<Promise<Value>>` removes the
  entry. `WasmKernel::promise(id)` uses it, and a second call for the same id throws
  `[epdf-web] promise n was already taken`. The scripted path keeps `Harness::promise(id)` and the
  `Vec` exactly as today.

The shim calls `promise(n)` once per `{"promise": n}` outcome (`proxy.ts`), so nothing needs the
entry after that. `abort_promise(id)` must still work while the JS promise is pending: keep an abort
handle beside the JS promise in the shim, not in the harness table.

- [ ] **Step 1: Failing wasm test.** Invoke a ref that returns a promise, call `promise(n)`, and
  assert `debug_counts().promises` dropped by one. A second `promise(n)` throws.
- [ ] **Step 2: Implement.** The live table becomes `RefCell<HashMap<u32, Promise<Value>>>` with a
  counter for ids.
- [ ] **Step 3: Fix the shim** so `abort_promise` goes through the handle the shim kept.
- [ ] **Step 4: Run** the wasm tests, `test:wasm`, and the conformance suites. Commit "Hand each
  promise to JavaScript once".

### Task 5: Release refs JS no longer holds

**Files:**
- Modify: `crates/plugin-conformance/src/harness.rs` (live ref table with pinning)
- Modify: `crates/epdf-web/src/exports.rs` (`release_ref`)
- Modify: `crates/plugins/stage/src/harness.rs` (live `visible_page_json` without transform refs)
- Modify: `packages/core-wasm/src/proxy.ts` (`FinalizationRegistry` over decoded refs)
- Test: `packages/core-wasm/test/release.test.ts` (Vitest with `--expose-gc` in its pool options)

**Interfaces:**
- Produces:
  - Live harness refs in a `HashMap<u32, RefFn>`. A ref registered while describing a capability
    is pinned, because the memo in `WasmKernel` hands the same id out again. Any other ref is
    releasable.
  - `WasmKernel::release_ref(id: u32)`: removes a releasable ref and ignores a pinned one.
  - The shim registers each function it builds from a releasable `$ref` with a
    `FinalizationRegistry` whose callback calls `release_ref(n)`. The describe output marks pinned
    refs (for example `{"$ref": n, "pinned": true}`) so the shim knows which is which.
  - In live mode, stage's `visible_page_json` emits the transform's data without the six method
    refs. `rehydrate.ts` already rebuilds them with `pageTransform(...)`; its fallback path (the
    ref-backed transform) goes away, and a transform that cannot be rebuilt becomes a loud error.

- [ ] **Step 1: Failing test for stage.** At a fixed camera, call `listVisiblePages()` 100 times
  and assert `debug_counts().refs` did not change.
- [ ] **Step 2: Implement** the live stage branch and the rehydrate change.
- [ ] **Step 3: Failing test for release.** Invoke a member that returns an unsubscribe function,
  drop the JS reference, force `gc()`, wait a turn, and assert `debug_counts().refs` dropped by
  one. Also assert a pinned capability ref survives the same sequence.
- [ ] **Step 4: Implement** pinning, `release_ref` and the registry.
- [ ] **Step 5: Run** everything, including the stage vectors (unchanged, since the scripted path
  still prints the refs). Commit "Release refs JavaScript has dropped".

The cost: `FinalizationRegistry` callbacks run whenever the JS engine chooses, so this bounds
growth over time rather than making it instant. The tests can only prove it with `--expose-gc`.

### Task 6: Release JS callbacks Rust no longer holds

**Files:**
- Modify: `crates/plugin-conformance/src/harness.rs` (a live `Callback` carries a release guard)
- Modify: `crates/epdf-web/src/exports.rs` (the guard calls a new `release_js` hook)
- Modify: `packages/core-wasm/src/proxy.ts` (reference-counted function table)
- Test: `crates/epdf-web/tests/exports.rs`, `packages/core-wasm/test/release.test.ts`

**Interfaces:**
- Produces:
  - A live `Callback` holds an `Rc<JsFnGuard>`. When the last clone drops, its `Drop` calls the
    host's `release_js(id)`. Scripted callbacks are unchanged.
  - `WasmKernel::new` takes one more JS function, `release_js(id)`.
  - The shim's `fns`/`fnIds` tables count uses per id, because the same JS function passed twice
    shares one id. `release_js` decrements the count and deletes the entry at zero.

- [ ] **Step 1: Failing test.** Register an interaction handler, then unregister it through the
  returned function. Assert the shim's `counts().fns` went back to where it was.
- [ ] **Step 2: Implement** the guard, the hook and the counted table.
- [ ] **Step 3: Run** the wasm tests, `test:wasm`, and the conformance suites. Commit "Release
  JavaScript callbacks the Rust side has dropped".

### Task 7: Closed documents and destroyed kernels

**Files:**
- Modify: `crates/epdf-web/src/exports.rs` (memo keyed by document; eviction on close)
- Modify: `packages/core-wasm/src/kernel.ts` (`destroy` unsubscribes and frees)
- Test: `crates/epdf-web/tests/exports.rs`, `packages/core-wasm/test/kernel.test.ts`

**Interfaces:**
- Produces:
  - The memo records each capability's document id. On the documents capability's closed event,
    `WasmKernel` evicts that document's entries and unpins their refs so Task 5 can release them.
  - `kernel.destroy()` calls the Rust `unsubscribe`, then `rust.free()` once the Rust destroy
    promise settles, then clears the `Wire` tables and the bridge host table. Any later call on the
    kernel throws `[core-wasm] kernel destroyed`.

- [ ] **Step 1: Failing tests.** (a) Open, read a document capability, close, and assert
  `debug_counts().described` went back down. (b) Create and destroy a kernel, and assert a later
  `capability()` throws the destroyed error instead of `recursive use of an object` or
  `null pointer passed to rust`.
- [ ] **Step 2: Implement** both.
- [ ] **Step 3: Run** everything and commit "Drop closed documents and destroyed kernels".

The bridge's document table keeps closed handles on purpose (post-close calls must answer as the
real engine does, `docs/research/rust-plugins-web-eval.md`). A closed `LocalDocumentHandle` is
small, so it stays.

### Task 8: Verify and record

**Files:**
- Modify: `packages/core-wasm/test/soak.test.ts` (assertions)
- Modify: `docs/research/rust-plugins-web-eval.md` (the memory lines in "Known gaps")
- Modify: `examples/rust-kernel-eval/README.md` (remove the "reload if it gets slow" advice once it
  is no longer true)

- [ ] **Step 1: Bound the soak test.** Between iterations 50 and 300, `refs`, `promises`, `fns` and
  `described` do not grow, and `hosts` stays within the budget from Task 3. `heapUsed` grows by
  less than 20 MB. That threshold is a guess, to be set from Task 1's baseline so the test would
  have failed before the fixes.
- [ ] **Step 2: Browser check.** With the Chrome DevTools MCP, open `ebook.pdf` in the eval app and
  take a heap snapshot. Scroll to the end and back five times, take another snapshot, and do the
  same on `packages/viewer/main` dev. Record both deltas in the findings file, with the commit.
- [ ] **Step 3: Update** the findings file's "Known gaps" and the README. Name what still grows
  (for example the closed-document handles in the bridge) with its measured size.
- [ ] **Step 4: Commit** "Record the eval build's memory after the release work".

## Not in this plan

It does not make the scripted harness release anything. The vectors depend on ids being stable
for a whole case, and a case is short.

It does not port the TypeScript's object-URL lifecycle into Rust. The URL is still created lazily
in JS from the handle and revoked by its caller, as today.
