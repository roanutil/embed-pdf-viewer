# Issue 11: Browser Change Clock Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task by task. Steps use checkbox syntax for tracking. Implementation is outside this triage request.

**Goal:** Creating annotations in the browser succeeds without trapping, and subsequent asynchronous capability calls keep settling.

**Architecture:** Install the kernel's existing UUID source from the JavaScript host runtime before constructing plugins. Retain one source guard for the WASM runtime thread, use JavaScript wall time and browser entropy, and restore the previous source when the test runtime resets. Keep native kernel defaults and deterministic conformance overrides intact.

**Tech Stack:** Rust, wasm-bindgen, js-sys, existing getrandom 0.2 JS support, wasm-bindgen-test, Vitest, the evaluation viewer bundle.

**Spec:** [roanutil/embed-pdf-viewer#11](https://github.com/roanutil/embed-pdf-viewer/issues/11).

## Triage

Confirmed defect; recommended priority: high, blocking annotation editing in the evaluation bundle. The issue identifies `@mfb/embedpdf-rust-viewer@0.0.0-hackathon.2`, built from `research/multi-platform-rust-core-impl-plugins` at `0ad0b74c2153bdd33d998c21227855a334a63c23`. That is also the inspected local branch tip. Implement against that branch, not the current `align/visibility-render-resolution` checkout.

`crates/plugins/annotation/src/services/store.rs` stages annotation operations through the document change queue. `crates/epdf-kernel/src/changes.rs:161` assigns their operation IDs using `generate_uuid_v7(None)`. In `crates/epdf-kernel/src/identity.rs:45`, the absent source falls back to `std::time::SystemTime::now()`, which traps on `wasm32-unknown-unknown`.

`WasmKernel::new` calls `runtime::start_clock`, but that installs the scheduler's virtual/monotonic clock only. Neither it nor the browser host installation installs `with_uuid_source`. The bundle's `annotation.randomUuid` host names annotation objects; it does not supply the change queue's operation IDs.

The hang after the trap follows from `crates/epdf-web/src/runtime.rs:88`: a WASM panic aborts without dropping `Entry`, leaving `DEPTH` elevated. Subsequent entries no longer drain the runtime. Prevent the panic at its source; changing the depth counter or swallowing JavaScript exceptions does not repair an aborted Rust stack.

### Evidence collected

1. Compiled the branch's exact `identity.rs` into an isolated `wasm32-unknown-unknown` module. In fresh instances, an explicit timestamp returns a 36-character UUID, ambient time traps with `RuntimeError: unreachable`, and `with_uuid_source` returns a 36-character UUID.
2. Drove the worktree's existing Node WASM artifact through `WasmKernel`, using the real interaction/annotation plugins over a stub engine with a valid page, allowed writes and available object numbers. `canCreate` returns true; square creation reports `time not implemented on this platform` and traps before an engine apply call. Its stack includes WASM functions 5657 and 8413, also present in the report.
3. Traced browser construction, UUID source callers, annotation staging and the separate engine clock. No production files were changed. These checks reproduce the kernel defect, not the reporter's complete Chrome/React application or downloaded release archive. Regression scripts are temporary files under `/private/tmp/embedpdf-issue-11-triage/`.

### Fix choice and scope

Recommend the host-installed source: `with_uuid_source` already provides the intended boundary, and `epdf-web` already depends on `js-sys` and wasm-targeted `getrandom` with JS support. A target-specific JavaScript dependency in `epdf-kernel` would also avoid the panic, but would make the portable kernel own browser policy. Passing time at one staging call would leave other default UUID callers exposed.

Do not retain the guard on individual kernels: it changes thread-local state, and viewers can be destroyed in either order. Do not discard a temporary startup guard: dropping it restores the broken fallback before later edits. A retained runtime source must be installed once, before plugin construction, and cleared by `runtime::reset` for isolated tests.

`epdf-engine::SystemClock` also uses `SystemTime`, but is not the reproduced call site. The bundle defaults to `rustReads: true`, selecting Rust geometry over TypeScript-owned engine sessions; `epdf-web` delegates engine calls through a JavaScript bridge. `epdf-wasm` separately targets Emscripten, whose clock environment differs from bare `wasm32-unknown-unknown`. Do not broaden this fix merely on `target_arch = "wasm32"`; a separate engine-clock change needs its own target-specific reproduction.

## Global Constraints

- Preserve `generate_uuid_v7(now: Option<f64>)`, explicit-time precedence, UUID version/variant bits and nested `with_uuid_source` restoration.
- Keep browser initialization automatic through `WasmKernel::new`; consumers must not install a source themselves.
- Use `js_sys::Date::now()` for epoch milliseconds and the existing browser entropy dependency for UUID bytes; retain native defaults unchanged.
- Follow the branch's architecture and testing conventions. Native `cargo test` alone does not execute `#[wasm_bindgen_test]` tests.
- Planning does not authorize publishing a release or posting an issue comment. The implementation should produce a locally reviewable replacement tarball.

## Review Focus

- Multiple viewers and destruction in either order must leave the survivor able to stage edits.
- Nested deterministic sources must restore the live browser source when dropped.
- Runtime reset and restart must restore source ownership without leaking guards or contaminating tests.
- Same-millisecond edits must have distinct operation IDs with valid UUIDv7 timestamp, version and variant fields.
- A real edit followed by an asynchronous read must settle within a bounded test timeout.

## Task 1: Install the browser UUID source with runtime ownership

**Files:** Modify `crates/epdf-web/src/runtime.rs`; extend `crates/epdf-web/tests/clock.rs`. The existing call from `crates/epdf-web/src/exports.rs` should remain the initialization entry point.

**Interfaces:** Consume `epdf_kernel::identity::{with_uuid_source, UuidSourceGuard, generate_uuid_v7}`. Keep `start_clock()` and `reset()` signatures unchanged. Introduce runtime-owned thread-local storage for one retained guard.

- [ ] Add WASM tests named `started_runtime_generates_live_uuid_v7`, `same_millisecond_operation_ids_are_distinct`, `nested_uuid_source_restores_browser_source`, and `reset_releases_uuid_source_before_restart`. Assert 36-character UUIDs, byte 6's version nibble 7, byte 8's RFC variant bits, and decoded epoch milliseconds between surrounding `Date.now()` calls. Use a fixed injected time for same-millisecond generation and check a batch of 100 distinct IDs. For reset, install a known outer source, start/reset the runtime, and assert the outer source is restored before restarting.
- [ ] Run `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test -p epdf-web --target wasm32-unknown-unknown --test clock`. Confirm the live UUID test traps on the unfixed branch. A trapping test can abort the test instance; use individual name filters when diagnosing.
- [ ] Install the source once in `start_clock()` before plugin construction, retaining its guard in runtime thread-local storage. Its time closure reads `js_sys::Date::now`; its byte closure fills all 16 bytes using the existing wasm-targeted `getrandom` 0.2 dependency. Drop the retained guard in `reset()`. Repeated kernel construction must not stack replacement guards; destruction must not uninstall the source. Preserve existing deterministic override behavior.
- [ ] Repeat the WASM clock command and run the complete WASM suite with `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test -p epdf-web --target wasm32-unknown-unknown`. Expected: clock assertions and existing export/runtime tests pass. Run `cargo test -p plugin-conformance --test current_harness` to verify the existing deterministic UUID tests still pass natively.
- [ ] Commit the runtime fix and WASM regressions together after reviewing `git diff --check`.

## Task 2: Prove annotation writes and subsequent work through the actual shim

**Files:** Extend `packages/core-wasm/test/kernel.test.ts` and `crates/epdf-web/tests/exports.rs`.

**Interfaces:** Use the existing `openFixture`, `createKernel`, annotation and selection capabilities, and real local engine in the TypeScript suite. Use the branch's signatures: direct creation is `create(page, draft, resources?, options?)`, with a `rect` field. Existing tests currently pass a single object containing `page` and `bounds`; correct the two annotation creation tests to exercise the actual branch API rather than copying those stale calls.

- [ ] Add a stub-engine export regression that constructs a kernel with `interaction` and `annotation-live`, returns a real page from `pages.list`, allows annotation creation, and provides object numbers. Record the first change reaching the engine and assert a valid UUIDv7 operation ID. Before Task 1's fix, this path must trap before apply. Keep confirmed event publication before mutation resolution in the stub.
- [ ] Correct the existing annotation creation test calls, then add real-engine cases for square creation and `createFromSelection('highlight')`. Create a nonempty selection using the selection capability's existing range APIs over a text fixture. Assert each mutation resolves, the resulting record is listed, `onCreated` fires exactly once, and a subsequent `selection.readText()` settles with the expected text. Bound waits to 5,000 milliseconds so a poisoned runtime fails visibly.
- [ ] Add a two-kernel lifecycle regression: create both viewers, destroy the first, and create an annotation on the second; repeat with destruction order reversed. Both surviving viewers must complete their write and follow-up read.
- [ ] Rebuild with `pnpm --filter @embedpdf/core-wasm build:wasm`, then run `pnpm --filter @embedpdf/core-wasm exec vitest run test/kernel.test.ts` and `pnpm --filter @embedpdf/core-wasm test:wasm`. Run `pnpm --filter @embedpdf/core-wasm typecheck` after correcting the branch API calls. Expected: real writes, callbacks, reads and lifecycle assertions pass using freshly generated WASM.
- [ ] Commit the shim/export regressions after reviewing `git diff --check`.

## Task 3: Verify a fresh consumer bundle

**Files:** Modify `packages/rust-viewer-bundle/package.json` and version references in `packages/rust-viewer-bundle/README.md`.

**Interfaces:** Preserve `createViewer` and `buildInfo`. Produce a local `@mfb/embedpdf-rust-viewer@0.0.0-hackathon.3` tarball, provided that version is still unused at implementation time; otherwise choose the next unused evaluation version.

- [ ] In the configured devcontainer, run `pnpm --filter @mfb/embedpdf-rust-viewer build:all`, then `pnpm --filter @mfb/embedpdf-rust-viewer pack:tarball`. Ensure `buildInfo` identifies the fix commit and intended branch, and rebuild after committing if an artifact with `dirty: false` is required.
- [ ] Install that tarball in a minimal Vite consumer and open a text PDF in Chromium. Relax annotation read-only settings, select text, and create a highlight with the API and the built-in selection-strip button. Separately create a square with `create(page, draft)`. Each must appear, emit one `onCreated`, and leave `selection.readText()` responsive. Fail the smoke test on a WASM panic or an `unreachable` console/page error.
- [ ] Check two simultaneous viewers and destroy/remount one; the remaining viewer must still create annotations. Reload once before comparing with a previously trapped build, because the old instance is poisoned.
- [ ] Record the artifact version, source commit, browser version and verification results alongside the implementation handoff. Audit changesets using `embedpdf-changesets`: the expected patch touches nonpublished Rust crates and private evaluation packages, so no publishable-package changeset is expected. If scope expands to publishable packages, add one changeset per directly affected package.
- [ ] Commit version/documentation changes, run `git diff --check`, and present the local tarball for review. Publishing and sending a reporter response remain separate actions.

## Completion Criteria

Both UUID-level and annotation-level WASM regressions pass against newly built artifacts. A fresh consumer can create a highlight and square, receives creation events, and completes follow-up asynchronous reads. Multiple viewers remain functional across teardown. Native deterministic UUID tests retain their existing behavior.
