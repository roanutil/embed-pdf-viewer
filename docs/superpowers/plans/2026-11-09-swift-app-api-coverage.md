# Swift app-facing API coverage implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining useful Swift core/plugin API gaps after integrating the parity branch.

**Architecture:** Extend existing UniFFI handles and Swift capabilities. Rust owns state, permissions, cancellation and events; Swift owns URLSession and native presentation. Keep private caches and host painter values in Rust.

**Tech Stack:** Rust 1.98, UniFFI 0.32, Swift 6.2, Foundation, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-11-09-swift-app-api-coverage-design.md` (approved by the user).

## Global Constraints

- Work on `research/multi-platform-rust-core-impl-plugins`; push verified commits to `fork`. No independent incoming fix belongs on the core-only branch.
- Preserve iOS 17/macOS 14 floors and existing package dependencies.
- New APIs are typed; domain JSON is allowed only for existing open engine payloads, never generic dispatch.
- Reset settings restores registration values. Omitted patches preserve values; explicit clears remove them.
- Every document handle rejects after close. Cancelling one shared-read consumer does not cancel unrelated readers.
- Generated Swift and framework artifacts remain ignored. A runtime release and new pins are separate from source pushes.
- Full native rebuild must occur after all Rust export changes; do not edit native source during the merge's four-slice build.

## Review Focus

1. URL failure/cancellation must preserve the failed/loading tab and retry source; Task 1 tests delayed cancellation and retry.
2. Scope/identity updates must not pretend to reauthorize existing native engine sessions; Task 1 pins the actual settings contract and documents its limit.
3. Settings reset/nullable patches must preserve registration ports; Task 2 tests nondefault registration and clear versus omission.
4. Editing must preserve binary bundle data, widget identity and other documents; Task 3 tests saved/reopened data and isolation.
5. Snapshots announce before their pixels finish; Task 4 tests pending access, cancellation isolation and owned-image survival.

## Validation environment

For Rust host tests use:

```sh
export PDF_RUNTIME_LIB_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work/fork/3c6e0229738d19c8f724251bb9c479aecbd18a00/runtime-src/out/embedpdf-runtime-staging
export EMBEDPDF_FORK_TARGET=darwin-static-arm64
```

Cargo cache writes require sandbox escalation. Keep long test output in this
plan's ignored workspace. Do not use the obsolete default darwin-arm64 headers.

### Task 1: Core document controls and native URL sources

**Files:** Extend `crates/epdf-viewer-host/src/{lib.rs,api.rs,session.rs}`; add focused `documents.rs` and `fetch.rs` modules if needed. Extend `platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift`; put URLSession provider in `DocumentFetch.swift`. Add Rust `tests/documents.rs`, `tests/fetch.rs` and Swift `Tests/EmbedPDFTests/DocumentAPITests.swift`.

**Interfaces:** Consume kernel Documents and KernelOptions.fetch. Produce typed ViewerHost/Swift methods for retry, rename, close-all, lookup, active document, order/move/swap, page-at/ref/index/revision, allows/canDownload, saveLayer, viewer settings and URL opening. Reuse DocumentInfo/PageInfo/PageRef. Add native `DocumentFetcher` callback and completion/cancellation objects; PDFViewer supplies a URLSession-backed default. Source-open options include activation.

- [ ] Write tests for rename/lookup/order/active-page state across two real documents, including missing/closed refs and download permission refusals. Write delayed fetch tests for HTTP 404/403/non-PDF, retry, task cancellation and close while loading; assert tab state and fetch count.
- [ ] Run `cargo test --offline --locked -p epdf-viewer-host --test documents --test fetch`; expect missing-API failures before implementation.
- [ ] Implement the native boundary using existing kernel methods and ForeignSettle/KernelPost. Preserve kernel error behavior rather than synthesizing state in Swift. Expose only mutable viewer settings that have meaningful semantics; keep engine authorization fixed per session and document it.
- [ ] Add Swift workflow tests, including default URLSession provider through a controlled local/URLProtocol transport, without external network dependency. Do not run Swift tests against stale generated bindings.
- [ ] Run the host suite and clippy with warnings denied; expect zero failures. Commit the task's source/tests.

### Task 2: Live plugin settings

**Files:** Existing `crates/plugins/{actions,commands,form,i18n,interaction,measurement,redaction,render,search,selection,signature,stamp}/src/ffi*`, matching Swift `Plugins/*.swift` wrappers, new boundary tests `tests/ffi_settings.rs` where appropriate and Swift `PluginSettingsTests.swift`. Annotation already exposes settings updates; add its missing readable settings boundary without duplicating its policy.

**Interfaces:** Each capability gains `settings()`, `updateSettings(_:)`, `resetSettings()` for its supported Rust operations, with generated `XSettings`/`XSettingsPatch` records as needed. Use existing typed configs only when they express the full mutable scalar subset. Preserve all existing APIs. Changes are observable through existing viewer revisions and dedicated events where already exported. No fake serializable loader/signer/asset-engine fields.

- [ ] Write boundary tests for nondefault registration, reading settings, merging a partial change, explicit nullable clear, cross-document registration scope, reset back to registered values, and closed handles. Choose real setting values from each contract, not a shared JSON round-trip that bypasses conversion.
- [ ] Run affected `cargo test --offline --locked -p <crate> --features uniffi --test ffi_settings`; expect missing-API failures.
- [ ] Implement settings records/converters and the three handle operations. Expose interaction default-tool settings, selection colors/handles/threshold, complete search reveal/highlight settings, render budget/tiles/snapshot settings, form validation/settings, redaction overlay, measurement settings, stamp preview/dynamic, signature mode/certification, i18n mutable locale/fallback, actions policy/triggers/script switches, and commands mutable settings. Keep registration resources untouched on ordinary scalar patches.
- [ ] Add matching Swift aliases/wrappers and behavioral integration tests for representative optional/clear/reset values. Inventory any concrete registration-only member separately.
- [ ] Run affected Rust suites and clippy; expect zero failures. Commit task source/tests.

### Task 3: Command, annotation and form workflows

**Files:** `crates/plugins/{commands,actions,annotation,form}/src/ffi*`; corresponding Swift `Plugins/{Commands,Actions,Annotation,Form}.swift`; boundary tests under each owning crate and native Swift tests.

**Interfaces:** Commands exposes disabledCommands/isCommandDisabled/disableCommand/enableCommand/setDisabledCommands and typed permanent refusal. Actions exposes canExecuteNamed. Annotation exposes cancellable bundle export/import using the existing bundle vocabulary and bytes. Form exposes calculation-order changes, per-widget update/delete, field/widget/selection reads needed to operate them. Expose selection-link editing if Rust has semantics beyond the existing threads API.

- [ ] Write tests for idempotent command disable/enable, category interaction, unknown commands and permanent refusal; mixed annotation bundle resources; widget mutation and calculation order surviving save/reopen. Test permissions and two-document isolation at owning boundaries.
- [ ] Run the new boundary suites; expect missing-API failures before coding.
- [ ] Implement typed records where they exist and existing domain JSON conventions where an engine payload is open. Route edits through capability operations, preserving cancellation and confirmed events.
- [ ] Add Swift workflow tests exercising the first visible result rather than forwarding mocks. Cover absent optional plugins and closed capabilities.
- [ ] Run affected suites and clippy; expect zero failures. Commit task source/tests.

### Task 4: Snapshots and native zoom boxes

**Files:** `crates/plugins/render/src/ffi.rs` plus focused submodule if needed, `crates/plugins/stage/src/ffi.rs`, Swift `Plugins/{Render,Stage}.swift`, render/stage FFI tests and Swift snapshot tests.

**Interfaces:** Render gains canSnapshot, snapshot metadata/handle, clearSnapshot, snapshotBox(on:), snapshot change observation. Snapshot metadata returns page/rect immediately; image resolution is a separate cancellable handle method returning owned RenderedImage, converted to RenderedPage in Swift. Stage gains begin/move/end/cancel zoom-drag and zoomBox if useful to a native adapter. Existing camera/gesture methods stay intact.

- [ ] Write tests for a held image promise: metadata/event visible before image resolution, cancelled reader leaves another reader alive, clear/close cancels the shared render, resolved pixels survive clear/close. Test permission refusal and pointer coordinate/zoom-box behavior.
- [ ] Run affected new FFI suites; expect missing-API failures.
- [ ] Implement snapshot handle storage on the kernel thread. Copy native pixels before resolving foreign calls; no JSON/host-table id escapes to Swift. Preserve subscription completion and avoid making snapshot reads start a new render.
- [ ] Add Swift tests for CGImage lifetime and snapshot event/metadata behavior. Expose the currently missing rect/rotation on render-page options for programmatic crop/rotation as well as snapshot workflows; verify pixel dimensions, orientation and pixels-per-point scale for cropped/rotated images.
- [ ] Run render/stage/host suites and clippy; expect zero failures. Commit task source/tests.

### Task 5: Core-inclusive audit, native verification and handoff

**Files:** `platforms/apple/check-swift-api-coverage.py`, `swift-api-coverage.json`, `tests/test_swift_api_coverage.py`, Apple README and this plan's report.

**Interfaces:** Keep existing plugin inventory behavior, add explicit inventory of public kernel Documents operations and viewer settings. Map wrappers and exports; classify implementation resources and genuine host integration requirements separately. Audit is an accounting claim, never an implementation percentage.

- [ ] Write checker tests proving a new public core operation is detected, a missing Swift/core export fails, and pure-library resources do not create fictional plugin APIs. Run tests and observe the intended failures.
- [ ] Extend the inventory, reconcile all current members and remove obsolete workflow exclusions for Tasks 1–4. Review every remaining exclusion against the approved app-facing scope and close any missed native workflow. Record printing as requiring HostPrinter integration; do not describe it as implemented.
- [ ] Rebuild all four Apple slices with `build-xcframework.sh` using the local fork staging root and the Rust llvm-nm override. Expect generated graph comparison and framework creation to succeed.
- [ ] Run the complete source-engine Swift package suite on macOS and an available iOS simulator, affected Rust suites/clippy, checker/tests and diff checks. Fix failures, then have a fresh reviewer inspect the complete change.
- [ ] Commit the updated report and push verified source commits to fork. Report exact commits, tests, deliberately remaining host integrations, and the default binary-release requirement. Never force-push or publish a runtime release implicitly.

## Execution record

The approved scope and existing task authorization permit continuing through
all tasks without another implementation permission prompt. The executor may
delegate independent domains when they do not share files; otherwise use
sequential implementers with fresh task reviews. Keep the task ledger as the
recovery map across context compaction.
