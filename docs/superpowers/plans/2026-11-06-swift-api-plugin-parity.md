# Swift API and Plugin Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The user approved execution on 2026-11-06; do not merge or publish.

**Goal:** Expose all Rust viewer plugins through the supported Swift API, complete missing members, and ship SwiftUI controls for history, outlines and attachments.

**Architecture:** Extend the existing Rust capability → UniFFI handle → `ViewerHost` → Swift capability chain. Rust owns semantics and permissions; `EmbedPDFUI` owns presentation and platform file interaction. Independent revision streams let multiple controls refresh snapshots safely.

**Tech Stack:** Rust 2024, UniFFI `=0.32.0`, Swift tools 6.2, Swift Testing, Observation, SwiftUI; existing Xcode/simulator toolchain and Apple build scripts.

**Spec:** [`../specs/2026-11-06-swift-api-plugin-parity-design.md`](../specs/2026-11-06-swift-api-plugin-parity-design.md).

## Global constraints

- Swift tools version 6.2, iOS 17, macOS 14; generated bindings keep Swift 5 language mode and the current UniFFI pin `=0.32.0`.
- All kernel access uses `KernelBound`/kernel jobs. Every exported promise operation forwards `CancelToken`; Swift uses `bridgedCancellable`.
- Swift adds platform adaptation and presentation only. Mirrors, undo policy, permissions, attachment naming, outline tree rules, and confirmed event origin stay in Rust.
- Preserve existing config defaults, typed `PDFError` codes, source compatibility, and generated-file exclusions. Do not hand-edit generated bindings or pin binaries that do not contain the new API.
- The initial planning commit changes documentation only. Implementation changes no npm package unless a separately justified boundary requires it; any directly affected npm packages then need separate changeset files.

## Review focus

- Two controls watching one viewer both receive revisions; cancelling either does not stop the other (Task 5).
- A close during a pending load/download/write terminates calls and subscriptions without leaked byte buffers (Tasks 2–4).
- Outline patch omission differs from explicit clearing; object-number refs survive move/delete/save/reopen (Task 3).
- Duplicate Unicode attachment names, zero-byte files, and document/page file identity retain Rust behavior (Task 4).
- A permission change or document switch during a UI action cannot leave an enabled stale control or apply an old snapshot (Tasks 6–8).

## Execution notes and verification commands

Work from `research/swift-api-plugin-parity-plan`, based on
`8fa01cc1ec4d0739af535feec6928e6a81163fd9`. The pre-existing untracked
`platforms/apple/EmbedPDF/.swiftpm/` belongs to the local workspace; do not add it.
This plan has not built or tested product changes.

Task order: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9. Task 1's member-level
audit is the completeness gate; stop and revise this plan if it reveals another
missing semantic API family rather than silently dropping it.

Read `docs/conventions/{README,architecture,plugins,state-and-sync,events,naming,permissions,testing}.md`
and the existing October 6 plugin umbrella before implementing. Generated names
share one module: prefix boundary records with their domain. Shared refs and
origin records belong in `epdf-ffi`; use existing ones before defining new ones.

Run Rust tests from the repository root. Before the first test, inspect the
existing local PDFium setup in `platforms/apple/README.md` and repository Cargo
configuration; use that setup rather than downloading an unrelated runtime.

For each Rust boundary change, rebuild the local Apple engine:

```sh
bash platforms/apple/build-xcframework.sh
```

Run the Swift tests from `platforms/apple/EmbedPDF` with
`EMBEDPDF_LOCAL_ENGINE=1` and a unique simulator destination selected from
`xcrun simctl list devices available`:

```sh
EMBEDPDF_LOCAL_ENGINE=1 xcodebuild test -scheme EmbedPDF-Package \
  -destination "$EMBEDPDF_TEST_DESTINATION" \
  -only-testing:EmbedPDFTests -only-testing:EmbedPDFUITests
```

Each task first runs its new focused test and records the expected behavioral
failure or missing-symbol error, then implements and reruns the focused suite.
Run the full command above at Task 9. A task's commit includes its tests and
documentation, with exact paths staged; do not use `git add .`.

## Task 1: Close verified wrapper gaps and establish the parity inventory

**Files:** Modify `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/{Metadata,Form,Stage,PageEdit,Redaction,Signature,Stamp,Link,Interaction}.swift` and corresponding existing tests under `Tests/EmbedPDFTests/`. Create `platforms/apple/swift-api-coverage.json` and `platforms/apple/check-swift-api-coverage.py`.

**Interfaces:** Existing exported Rust handle signatures are authoritative. Add:

| Capability | Swift declarations; all are `async throws(PDFError)` |
| --- | --- |
| Metadata | `customSnapshot() -> CustomMetadata?`, `customStatus() -> ResourceStatus`, `updateCustom(_ patch: [String: String?]) -> CustomMetadata`, `refreshCustom()` |
| Form | `validate() -> FormValidationResult`, `canRestoreAttribution() -> Bool`, `canWriteScripts() -> Bool` |
| Stage | `pageCount() -> UInt32`, `currentDestination() -> StagePageDestination?`, `goToDestination(_ destination: StagePageDestination, behavior: StageScrollBehavior? = nil)`, `canZoomIn() -> Bool`, `canZoomOut() -> Bool` |
| PageEdit | `selectedPages() -> [PageRef]`, `select(_ pages: [PageRef])`, `canExtract() -> Bool` |
| Redaction | `canUnmark(_ ref: AnnotationRef) -> Bool`, `canUpdateLabel(_ ref: AnnotationRef) -> Bool` |
| Signature | `canReadRevision() -> Bool` |
| Stamp | `canCreateFromAnnotations(in document: DocumentInfo? = nil) -> Bool` |
| Link | `status(for page: PageRef) -> String` (preserve existing Rust status vocabulary) |
| Interaction | `hasCursorClaim() -> Bool` |

`CustomMetadata` is a Swift typealias for `[String: String]` (the Rust alias
is not emitted as a generated Swift type). `FormValidationResult` and
`StagePageDestination` alias existing generated records. Also export the
existing `CustomMetadataUpdatedEvent` used by the metadata event enum.

- [x] Add focused tests to the existing suites: custom metadata set/remove/refresh; required-field validation before/after fill; selected page refs after reorder; destination navigation and zoom availability; each new permission read under a denied scope; cursor claim release. Prove the new methods are missing by running the focused Swift tests against the current binary.
- [x] Forward the methods above through `bridged` or `bridgedCancellable`, matching the Rust signature; add missing type aliases. Rerun each changed Swift suite. Assert custom deletion with an explicitly present dictionary key whose value is `nil`, not by assigning a dictionary subscript `nil` (which removes the key).
- [x] Build the coverage manifest from all public Rust controller/sub-capability methods, exported handle methods and Swift mappings, including feedback. Each entry records crate, Rust symbol/path, exported symbol, Swift symbol/path, and either coverage or a specific exclusion reason. Classify all eight pure core crates explicitly. Capture controller members that have never reached UniFFI, not just wrapper omissions.
- [x] Implement the coverage checker over that manifest and source inventories. Test that a temporary unaccounted controller member is detected and that escaped/renamed Swift methods, feedback in a separate file, constructors and internal conversion helpers are classified correctly. Run `python3 platforms/apple/check-swift-api-coverage.py`; only the three missing plugin families assigned to Tasks 2–4 may be marked pending at this stage.
- [x] Commit as `feat(apple): complete existing Swift capability methods and track parity`. Record any additional substantive gaps as plan changes before continuing.

## Task 2: History capability

**Files:** Create `crates/plugins/history/src/ffi.rs`, `crates/plugins/history/tests/ffi.rs`, `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/History.swift`, and `Tests/EmbedPDFTests/HistoryTests.swift`. Modify history `Cargo.toml`/`src/lib.rs`, `crates/epdf-viewer-host/src/lib.rs`, `platforms/apple/swift-api-coverage.json`; extend shared `epdf-ffi` records only if needed.

**Interfaces:** Add `ViewerHost.history(document_id) -> Arc<HistoryHandle>` and
`PDFViewer.history(for: DocumentInfo) async throws(PDFError) -> HistoryCapability`.
All reads/submissions use existing handle scheduling. Swift exposes
`canUndo()/canRedo() -> Bool`, `undoLabel()/redoLabel() -> HistoryChangeLabel?`,
`undo()/redo()/clear()`, `settings() -> HistorySettings`,
`updateSettings(_ patch: HistorySettingsPatch)`, `resetSettings()`;
these are asynchronous and throw `PDFError`. Map `ChangeLabel` (JSON containing an i18n key and values) losslessly to a
`HistoryChangeLabel` record; preserve interpolation value kinds through the
existing i18n value boundary types where compatible.
`HistorySettings.limit` and its optional patch are `Double`, preserving Rust's
existing defaults/validation. `events` is `AsyncStream<HistoryEvent>` with
undone, undoFailed, and settingsChanged cases, retaining labels, redo, skipped
count, refusal reason, and structured error.

- [x] Add failing Rust handle/host tests for absent plugin (`unsupported`), independent document histories, settings, an annotation edit then undo/redo, skipped/refused outcomes, and completion-event ordering, including empty-history no-ops that emit no
completion event. Add Swift tests for accessor, labels, error codes, and event termination.
- [x] Export typed boundary records, listener and handle via `KernelBound`. Reuse `ViewerConfig.history`; it already installs the plugin. Delegate synchronous undo/redo submissions without treating their return as engine confirmation. Own subscriptions through the bound instance.
- [x] Add the Swift wrapper and relay using `StreamRegistry`. No cancellation token is needed for synchronous submission; pending engine work and completion keep their existing Rust semantics. Test close/destroy and a subscription created after close.
- [x] Run `cargo test -p epdf-plugin-history -p epdf-viewer-host`, regenerate Apple bindings, and run `EmbedPDFTests/HistoryTests`. Require both accepted submission and eventual confirmation tests to pass; update coverage entries.
- [x] Commit as `feat(apple): expose document history capability`.

## Task 3: Outline capability

**Files:** Create outline `src/ffi.rs` and `tests/ffi.rs`, Swift `Plugins/Outline.swift` and `Tests/EmbedPDFTests/OutlineTests.swift`. Modify outline crate exports/dependencies, host `src/lib.rs`, coverage manifest; modify `crates/epdf-ffi/src/` only for genuinely shared types and `translate.rs` only if real-engine tests expose a missing route conversion.

**Interfaces:** `ViewerHost.outline(document_id) -> Arc<OutlineHandle>`;
`PDFViewer.outline(for:) -> OutlineCapability`. Expose `list`, `item`,
`children(of:)`, `rows`, `currentItem`, `status`, `isIncomplete`, `ensureLoaded`,
`refresh`, `isExpanded`, `expand`, `collapse`, `toggle`, `expandAll`,
`collapseAll`, `reveal`, `selectedItems`, `select`, `activate`, `canEdit`,
`currentViewTarget`, `create`, `update`, `move`, and `delete`.
Reads and verbs are `async throws(PDFError)`. Exact arguments mirror
`outline/src/controller.rs` with typed refs/records in place of `Value`.

Define `OutlineItemRef(objectNumber: UInt32)`, `OutlineItem`, `OutlineRow`,
`OutlineItemInput`, `OutlineItemPatch`, `OutlinePosition` (start/end/before/after),
and `OutlineCreateOptions(select: Bool = false)`. Nullable patch members use
optional values plus `clear: [OutlinePatchKey]`, following the existing form
patch precedent. Targets/actions reuse link types where lossless; preserve
unrecognized read-only action payloads explicitly rather than dropping them.
`OutlineEvent` has changed and resynced cases with original refs/origin/data.

- [x] Add failing codec tests for top-level/null parents, positions, opaque action data, and omit/clear; handle tests for every controller operation; real-host tests for lazy load, create/select/update/move/delete and current-view targets. Include unavailable optional stage/actions/link plugins.
- [x] Implement boundary records/conversions, `KernelBound` handle, event listener, host accessor, and Swift capability. Every promise-taking load/write gets `CancelToken`; preserve Rust expansion and selection behavior. Add no Swift tree-building policy.
- [x] Add save/reopen tests for nested outlines, explicit target/color clearing and stable refs; denied edit scope must leave state unchanged. Test pending write cancellation and close, using deterministic Rust engine control where timing matters.
- [x] Run `cargo test -p epdf-plugin-outline -p epdf-viewer-host`, regenerate bindings, and run `EmbedPDFTests/OutlineTests`. Assert fact events once with original origin and resync separate from per-item changes. Update coverage entries.
- [x] Commit as `feat(apple): expose typed outline capability`.

## Task 4: Attachment capability

**Files:** Create attachment `src/ffi.rs` and `tests/ffi.rs`, Swift `Plugins/Attachment.swift` and `Tests/EmbedPDFTests/AttachmentTests.swift`. Modify attachment exports/dependencies, host accessor, coverage manifest, and shared attachment refs under `epdf-ffi` as needed.

**Interfaces:** `ViewerHost.attachments(document_id) -> Arc<AttachmentHandle>`;
`PDFViewer.attachments(for:) -> AttachmentCapability`. Swift methods are
`files() -> [AttachedFile]`, `file(_ id: String) -> AttachedFile?`,
`selectedFiles() -> [AttachedFile]`, `select(_ files: [AttachedFile])`,
`isPending(_ file: AttachedFile) -> Bool`, `status() -> ResourceStatus`,
`ensureLoaded()`, `refresh()`, `canDownload() -> Bool`, `canAdd() -> Bool`,
`canRemove(_ file: AttachedFile) -> Bool`, `add(_ sources: [AttachmentFileSource]) -> [AttachedFile]`,
`remove(_ file: AttachedFile)`, `download(_ file: AttachedFile) -> AttachmentContent`,
`reveal(_ file: AttachedFile)`; all `async throws(PDFError)`.
Preserve the Rust controller's batch add semantics and result ordering; an empty
input returns an empty array. Single-file UI import calls this with one source.

`AttachedFile` preserves stable `id`, name/type/description/size/checksum/times
and `home: AttachmentHome` (document key ref or page annotation ref).
`AttachmentFileSource(data: Data, name: String?, mimeType: String?, description: String?)`
and `AttachmentContent(data: Data, name: String, mimeType: String?)` use owned
UniFFI byte buffers. `events: AsyncStream<AttachmentEvent>` preserves the
controller's changed/resynced events. Page annotation changes remain annotation
events; UI refresh also uses viewer revisions.

- [x] Add failing codecs and real-host tests for lazy list, add/download/remove, zero bytes, duplicate Unicode names, selecting both homes, pending status, optional annotation/stage absence, and permissions. Assert byte-for-byte download equality and stable IDs distinct from display names.
- [x] Implement typed handle, host accessor, Swift wrapper and relay. Adapt kernel byte JSON internally using existing `Bytes` helpers; do not route binary data through Swift JSON strings. Preserve all optional metadata and the controller's file-name behavior.
- [x] Add cancellation/close tests while download and add are held pending, checking host byte-table cleanup; add saved-document reopen coverage and page-attachment download/reveal tests. Ensure failed writes leave confirmed records intact.
- [x] Run `cargo test -p epdf-plugin-attachment -p epdf-viewer-host`, regenerate bindings, and run `EmbedPDFTests/AttachmentTests`; update coverage entries.
- [x] Commit as `feat(apple): expose document and page attachments`.

## Task 5: Independent revision observation for UI models

**Files:** Modify `Sources/EmbedPDF/{PDFViewer,EventRelay}.swift`, `Tests/EmbedPDFTests/ViewerTests.swift`. Extend `crates/epdf-viewer-host/{src/lib.rs,tests/viewer_host.rs}` only if the chosen implementation needs a new host subscription; prefer fan-out of the existing delegate.

**Interfaces:** `PDFViewer.observeStateRevisions() -> AsyncStream<UInt64>` is a
new nonthrowing method. Every call returns its own subscription, buffers the
newest revision, and finishes on destroy/deinit. Keep the existing public
`stateRevisions` stream and its single-consumer contract intact. A lock-protected
Swift relay hub may broadcast the existing delegate notifications without
adding a second kernel state owner.

- [x] Add failing tests with two fresh subscribers and the legacy subscriber: both fresh consumers observe a later mutation; cancelling one leaves the other live; destroy finishes all; subscribe after destroy immediately ends. Use bounded timeouts and explicit mutation barriers, never sleeps as proof of delivery. Mutate during
the initial snapshot reads to prove the subscriber was installed synchronously
in the delegate fan-out before those reads; stream construction alone is not
proof of a registered Rust listener.
- [x] Implement fan-out with registry-owned termination, no strong viewer/continuation cycle, and no lock held while invoking external callbacks. Revisions may coalesce; callers query current snapshots and do not count them as events.
- [x] Test repeated subscribe/cancel/destroy interleavings and verify the final revision reaches each remaining consumer. Run `EmbedPDFTests/ViewerTests` and existing UI suites to establish legacy compatibility.
- [x] Update coverage and public concurrency documentation; commit as `feat(apple): add independent viewer revision streams`.

## Task 6: History controls

**Files:** Create `Sources/EmbedPDFUI/{HistoryModel,HistoryControls}.swift`, `Tests/EmbedPDFUITests/HistoryModelTests.swift`; add a focused view test/demo scenario under `platforms/apple/EmbedPDFDemo` using its existing structure.

**Interfaces:** `@MainActor @Observable public final class HistoryModel` with
`init(viewer: PDFViewer, history: HistoryCapability)`, `task() async`, `undo() async`, `redo() async`;
read-only `canUndo`, `canRedo`, labels, `submitting`, and `error: PDFError?`.
`HistoryControls.init(model: HistoryModel)` renders native buttons.
Use the explicitly passed viewer for revision observation; `EmbedPDFUI` cannot
reach the capability's internal retained viewer. Tasks 7–8 use the same pattern.

- [ ] Add failing model tests for initial disabled state, edit → undo/redo availability, completion vs submission, refused/skipped undo, and cancellation/document replacement. Subscribe before initial reads; use a refresh generation or serialized refresh loop to discard stale answers.
- [ ] Implement model task ownership with independent revisions plus history events. Render labels and errors, disable buttons while submission is active, and refresh availability from Rust. Clear `submitting` when the submission call returns, including an empty-history
no-op; never wait indefinitely for a completion event. A submitted undo is not
an immediately confirmed edit. Completion/failure events still update feedback.
- [ ] Add keyboard/VoiceOver labels and a demo scenario that edits an annotation, undoes, and redoes. Verify Dynamic Type and toolbar placement on iOS and macOS.
- [ ] Run `EmbedPDFUITests/HistoryModelTests` and the demo scenario; commit as `feat(apple-ui): add history controls`.

## Task 7: Outline panel

**Files:** Create `Sources/EmbedPDFUI/{OutlineModel,OutlinePanel}.swift`, `Tests/EmbedPDFUITests/OutlineModelTests.swift`; extend the demo's existing reader presentation.

**Interfaces:** `@MainActor @Observable public final class OutlineModel` with
`init(viewer: PDFViewer, outline: OutlineCapability)`, `task() async` and intent
methods matching select/activate/expand/collapse/create/update/move/delete.
Read-only rows, selected refs, status, incomplete, `canEdit`, operation state,
and `error`. `OutlinePanel.init(model: OutlineModel)`.

- [ ] Add failing tests for initial load/empty/error, Rust-provided flattened row indentation, independent expansion and selection, activation/current location, denied edits, explicit clearing, and document switch while refresh is pending. Two instances observing the same viewer must both refresh.
- [ ] Implement snapshot projection from the capability, independent revisions and load/fact events; keep tree rules in Rust. Expose native edit sheets for title/target/style, add child/sibling, move to parent/position, and confirmed destructive delete. Use refs for list identity.
- [ ] Add VoiceOver hierarchy/expanded state and keyboard activation; expose incomplete data explicitly. Integrate a panel in the demo alongside the reader and history controls without changing camera geometry in Swift.
- [ ] Run `EmbedPDFUITests/OutlineModelTests`, `EmbedPDFTests/OutlineTests`, and the navigation/edit demo scenario on iOS and macOS; commit as `feat(apple-ui): add outline navigation and editing panel`.

## Task 8: Attachment panel and platform file handling

**Files:** Create `Sources/EmbedPDFUI/{AttachmentModel,AttachmentPanel,AttachmentExportDocument}.swift`, `Tests/EmbedPDFUITests/AttachmentModelTests.swift`; extend demo presentation and UI scenarios.

**Interfaces:** `@MainActor @Observable public final class AttachmentModel` with
`init(viewer: PDFViewer, attachments: AttachmentCapability)`, `task() async`,
`select(_:)`, `add(_ sources: [AttachmentFileSource]) async`, `remove(_:) async`,
`download(_:) async -> AttachmentContent?`, and `reveal(_:) async`.
Expose files/selection/status/availability/pending/error as read-only state.
`AttachmentPanel.init(model: AttachmentModel)` owns fileImporter/fileExporter;
`AttachmentExportDocument` conforms to SwiftUI `FileDocument` and stores `Data`.

- [ ] Add failing tests for both file homes, optional MIME type, import/download/export bytes, duplicate labels with distinct IDs, denied operations, cancellation, file I/O errors, and view replacement while a download completes.
- [ ] Implement the model with independent revisions and controller events. Do file reads off the main actor, balance security-scoped URL access with `defer`, and forward copied bytes to the capability. File exporter appears only after a user download action. Ask before deleting an attachment in the UI.
- [ ] Implement native list selection, page reveal, import, remove and export; show loading/empty/error and per-file availability. Give unknown MIME types an ordinary data export without guessing executable behavior. Add accessible labels and keyboard actions.
- [ ] Run `EmbedPDFUITests/AttachmentModelTests`, `EmbedPDFTests/AttachmentTests`, and demo import/export scenarios on iOS/macOS; commit as `feat(apple-ui): add attachment panel and file import export`.

## Task 9: Integration, distribution and documentation

**Files:** Modify `platforms/apple/README.md`, `multiplatform-port-status.md`, `platforms/apple/swift-api-coverage.json`, relevant host/Swift integration tests and Apple fixture copy list. Update `platforms/apple/EmbedPDF/Package.swift` engine pin only after a matching artifact exists. Keep existing publish machinery as the owner of hashes and checksums.

- [ ] Add a full-config real-host/Swift integration test opening a document with all plugins, editing outline/attachments/annotations, undoing/redoing, saving/reopening, observing three controls simultaneously, closing and destroying. Include restricted scopes, locked documents, absent plugins, and cancellation. Source-mode fixture changes must be copied to every relevant generated test bundle by the existing build script.
- [ ] Run the parity checker with **zero pending plugin/member entries** and justified exclusions; run `cargo test -p epdf-viewer-host -p epdf-plugin-history -p epdf-plugin-outline -p epdf-plugin-attachment` and clippy for changed crates with `--all-targets -- -D warnings`. Rebuild bindings and run the full source-mode Apple package test command above, plus a macOS package test destination.
- [ ] Run `bash platforms/apple/package-engine-framework.sh` and the same Apple package suite with `EMBEDPDF_PREBUILT_ENGINE=local` (unset `EMBEDPDF_LOCAL_ENGINE`), proving the packaged module contains the new APIs. Record test counts and artifact size. Do not treat a source build alone as distribution verification.
- [ ] Update plugin tables and examples with the new capabilities, controls, feature switches, lifecycle/cancellation, history submission semantics and supported exclusions. Correct the README's obsolete statement that the facade/UI targets are empty. Include a minimal reader using all three controls and independent revision streams.
- [ ] Commit verified source/docs. A public engine release is a separate publishing action: prepare it with the existing `scripts/publish-engine-payload.mjs --dry-run --only apple` workflow; publish only when authorized, then pin its actual checksum. Until then mark the new API as requiring the local source/packaged engine, and do not claim the default downloaded binary contains it.

## Review handoff

Review the linked design's approach, three new capability surfaces, and control
scope before implementation. Estimated effort is 7–12 engineer-days including
Rust/Swift tests, UI integration and packaged-engine validation; this is a
planning estimate, not measured execution time. No implementation, publishing,
dependency changes or baseline product test run is part of this planning task.
