# Swift plugin parity review report

Branch: `research/swift-api-plugin-parity-plan`.
Base: `research/multi-platform-rust-core-impl-plugins` at
`8fa01cc1ec4d0739af535feec6928e6a81163fd9`.
No merge, push or publication is authorized by this report.

## Delivered surface

Typed history, outline and attachment capabilities now cross the Rust/UniFFI
boundary and are exposed by `PDFViewer`. Existing wrapper gaps are filled.
Native SwiftUI controls have independent observation, cancellation/stale-read
protection, Rust-owned permissions, native editing forms and file interaction.
The demo uses Rust's stage for navigation and provides all three controls.

The member audit covers 30 crates and 779 members: 594 mapped and 185 explicit
exclusions, zero pending. This is an accounting audit, not a claim that every
public Rust workflow is exposed. Most dynamic settings, annotation bundles,
command-disable/refusal details, individual widget/calculation-order editing
and native zoom-box gestures remain excluded. Each rationale is recorded in
[`swift-api-coverage.json`](../../../platforms/apple/swift-api-coverage.json).

## Verification

| Mode | Destination | Core | UI models/integration | Binding smoke | Result |
|---|---|---:|---:|---:|---|
| Source engine | macOS arm64 | 355 | 41 | 8 | Passed |
| Source engine | iOS 26.5 (23F77) | 355 | 41 | 8 | Passed |
| Packaged engine | macOS arm64 | 355 | 41 | — | Passed |
| Packaged engine | iOS 26.5 (23F77) | 355 | 41 | — | Passed |

The final demo control run on iOS 26.5 executed four tests, all passing:
annotation edit/undo/redo with displayed-pixel assertions, bookmark creation/navigation, accessibility XXXL
history controls, and native attachment import/save/replace. The exported
file was checked on disk: all 33 bytes match the original attachment.

Rust host/history/outline/attachment suites: 253 tests passed. Clippy passed
for all changed Rust crates and the host with `--all-targets -- -D warnings`.
The coverage checker and its three regression tests passed.

Bindings and all four native slices were rebuilt against runtime commit
`82f2dfabe2542208c783d49e210ed87f214ac0c3`. Symbol inspection uses the fork's
reviewed LLVM `llvm-nm`, since Apple `nm` cannot read Rust 1.98's LLVM bitcode.
The existing published fork engine was usable for initial wrapper tests, but
predates the newly added native exports.

## Whole-branch review and fixes

One fresh whole-branch review found zero Critical, three Important and one Minor
issue. All three Important issues were reproduced with failing regressions,
fixed and committed in the single review fix pass:

1. Cancelled initial outline/attachment reads no longer leave errors in retained
   models. Successful initial reads clear obsolete read errors while preserving
   real action and file-operation errors. Focused model suite: 11 passed.
2. Outline and attachment presentation state now belongs to a model-identity
   subtree. Replacing a model disposes pending editors, confirmation state and
   owned import/download tasks; obsolete callbacks are rejected. A real SwiftUI
   hosting regression with two documents and colliding outline references caught
   cross-document Save/Delete and stale-task behavior. Focused suite: 12 passed.
3. Rust render invalidations now advance per-page reader revisions and rerender
   retained page images. The native demo regression verifies changed pixels after
   adding a note, original pixels after undo and edited pixels after redo. A
   separate regression confirms unaffected pages and camera remain unchanged.

The final tree passed all four source/packaged package runs above (404 tests
per source destination; 396 per packaged destination), Rust tests, clippy and
the API audit. The four native demo cases also passed after the fixes.
No second review was run. The minor audit issue below is deliberately deferred.

## Distribution

The new API currently requires the local source or locally packaged engine.
The default published engine pin and repository-root package remain on the
previous engine. Do not use this branch through the default downloaded binary
until a matching release is published and both package manifests are updated.

The existing `publish-engine-payload.mjs --dry-run --only apple` workflow passed
on committed viewer source `f256b35e859f121e57d0ab5fad95102ac1d5025f`. It rebuilt
all slices, packaged the module, verified its SHA-256 and tested the exact binary
on iOS (391 tests), macOS arm64 (390 tests), and macOS x86_64 (390 tests).

- Candidate release: `apple-engine-a53239de452ea7f5` on `roanutil/embed-pdf-runtime`.
- Payload: `platforms/apple/.build/publish/EmbedPDFBindings.xcframework.zip`.
- Size: 348,612,317 bytes (332.46 MiB).
- SHA-256: `b4ee25422d7e4c7ed0e953bddb37a48de0c1344dfa54544cb59fa7f1003743d3`.

The dry run printed the proposed release and pin operations. It performed neither.
Both manifests still pin the previous published engine. Publication is separate
work and requires authorization. This payload is ready for that review.

## Files investigation and manual confirmation

On iOS 27.2 build 24B8117, the native attachment picker test cannot activate the
demo folder. This reproduced on the original iPhone 17 simulator and a new,
isolated iPhone 17 simulator. The folder displays the correct indexed item
count; both PDF and text fixtures exist in the app's current Documents folder.
Generic/cell/icon/double taps, returning to the location root and serial testing
did not resolve it. Temporarily disabling multi-selection also failed and was
reverted. Disabling hit testing on the covered reader also failed and was
reverted. The same source and ordinary single-tap navigation succeed
on iOS 26.5, including actual import and export.

This establishes a version-dependent picker/automation failure, not its exact
cause. A standalone Files-app comparison did not reach a file grid, so it does
not prove an Apple system defect. A theory based on cloned application IDs was
rejected: those UUIDs were Xcode attachment-filename suffixes. No unsupported
provider reset, file-filter workaround or skipped test was added to the package.
The final post-review-fix retest on the original iOS 27.2 simulator also failed:
the picker restored an empty “On My iPhone” location and exposed neither the
demo folder nor the fixture. The lifecycle fixes did not establish a working
iOS 27.2 import path.

The user subsequently reproduced the failure manually on the isolated iOS 27.2
simulator: tapping “EmbedPDF Demo” animates as though navigating, but remains in
“On My iPhone”. This rules out a failure limited to synthetic taps. Together
with the successful iOS 26.5 roundtrip, the evidence points to an iOS Files or
simulator issue; the user accepts that assessment. No EmbedPDF workaround is
planned, and no further input is needed for this investigation. The exact system
cause and behavior on physical iOS 27.2 devices remain unverified.

macOS native file-panel interaction was not automated; its models/capabilities
and both package modes were tested.

## Rulings I made

The following is the chronological execution record. Intermediate audit counts
are historical snapshots; final counts are given above. It includes the
reasoning and known cost recorded when each decision was made.

1. Ruling: execute in current dedicated planning branch — user explicitly requested this branch and commits, no separate worktree needed — no branch isolation cost beyond existing dedicated branch.

2. Ruling: rebuild runtime-src at pinned commit 82f2dfabe2542208c783d49e210ed87f214ac0c3 using buildFork cache — older cached slices have incompatible public headers — costs native compilation time; no runtime pin change.

3. Ruling: inventory includes host-only resources and public workflows outside the approved concrete additions as explicit exclusions, not exported API parity — 537 mapped, 182 exclusions, 60 pending new plugin members — deferred annotation bundle transfer, per-command disabling, dynamic plugin settings and widget-specific design remain visible in the manifest for review.

4. Ruling: use domain-prefixed HistoryLabelValue rather than depend on i18n just for its equivalent enum — avoids coupling independent plugins — cost is a small conversion for localized labels in UI.

5. Ruling: prepare unlinked outline source and future tests while history's four-slice build runs — keeps work moving without changing the build's module graph — no generated API is enabled until history passes.

6. Ruling: fix annotation FFI create_attachment's internal field from box to rect — necessary for the approved page-attachment workflow and backed by the failing real-host test — no public signature change.

7. Ruling: prepare the independent revision hub while attachment's native build runs — pure Foundation source can be RED/GREEN tested without changing the Rust graph or viewer wiring — viewer integration and its commit remain Task 5.

8. Ruling: migrate demo Info sheet to createdAt/modifiedAt plus customSnapshot — required for the approved demo scenario to compile and preserves its existing custom-info behavior — no supported API change.

9. Ruling: continue preparing Task 7 while the already-built history UI test waits for first-boot migration — independent Swift UI work does not alter the running built app — Task 6 is not recorded complete until its UI scenario passes.

10. Task 7: Ruling: use the existing StageReaderView in the demo — its Rust stage is needed for visible bookmark navigation; removed demo-only ScrollView camera rendering, adapted profiling selector to any matching pages element. Cost if wrong: demo scroll profiling must be rebaselined.

11. Task 7: Ruling: test pending-read replacement at shared SnapshotRefresh boundary and real closed-document model boundary — avoids introducing test-only capability abstractions; cost if wrong: an integration-specific ordering case could remain.

12. Task 8: Ruling: export arbitrary attachment bytes as public.data with original suggested filename/extension — FileDocument writable types are static, and passing a MIME-derived type outside that set causes SwiftUI to ignore it (reproduced warning and regression test RED→GREEN). Owned content retains original MIME metadata in Swift API; cost if wrong: export provider may infer type from filename instead of receiving the original MIME type.

13. Task 8: Ruling: existing demo is iOS-only; native Files interaction is automated there, while macOS exercises real models/capabilities, compiles all native controls and tests both engine modes — avoids broadening this task into porting the existing demo. Cost if wrong: a macOS-only file-panel or keyboard integration issue can remain.

14. Task 8: Ruling: pending read/download replacement is covered at SnapshotRefresh boundary plus real closed-document model tests and cancellation checks before presenting exporter — avoids introducing mock capability abstractions. Cost if wrong: a platform presentation ordering case can remain.

15. Ruling: iOS 27.2 picker root cause remains an unresolved validation risk, not a proven Apple bug — manual check is pending and must be visible for merge review — cost if wrong: native attachment import may fail for users on that OS.

16. Ruling: macOS native file-panel/keyboard interactions are not certified by compilation/model tests — retain the scoped research handoff and flag platform integration validation before distribution — cost if wrong: a macOS-only interaction bug may remain.

17. Ruling: default downloaded compatibility stays deferred exactly as Task 9 authorizes; local source/packaged engine required until separately approved publication and pin updates — cost if wrong: default package consumption of this branch does not compile.

18. Ruling: explicitly excluded broader workflows remain outside the concrete additions and documented, rather than claimed as full exported parity — cost if wrong: additional live-settings/interchange/widget/command/zoom workflows need implementation.

19. Ruling: removed whole-page search border is decorative; Rust stage still reveals selected hits — keep demo stage navigation — cost if wrong: weaker visual search feedback needs follow-up.

20. Ruling: accept the iOS 27.2 folder-navigation failure as an iOS Files/simulator issue for this handoff — the user reproduced the animation-without-navigation manually and agrees with that assessment; iOS 26.5 completes the roundtrip — no speculative EmbedPDF workaround; physical-device behavior and exact system cause remain unverified. This supersedes the pending manual check in ruling 15.

## Deferred minors

Eleven implemented i18n members are incorrectly grouped with translator-closure
factory exclusions in `platforms/apple/swift-api-coverage.json`. Their Swift
wrappers exist, but the checker does not protect those mappings. A follow-up
should map those members and keep `get_translator` excluded. This affects the
audit's classification, not runtime feature availability.

## Handoff

All nine implementation tasks and the three Important review fixes are committed
on the dedicated branch. Keep the branch for user review; no merge, push or
publication was performed. Diagnostic logs and native screenshots remain in the
ignored plan workspace as evidence of the iOS 27.2 Files/simulator limitation.

The requested manual check is complete; no further user input is required.
Next action: review this branch's diff against
`research/multi-platform-rust-core-impl-plugins` before merging.
