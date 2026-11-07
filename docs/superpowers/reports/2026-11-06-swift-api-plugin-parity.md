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
| Source engine | macOS arm64 | 355 | 35 | 8 | Passed |
| Source engine | iOS 26.5 (23F77) | 355 | 36 | 8 | Passed |
| Packaged engine | macOS arm64 | 355 | 35 | — | Passed |
| Packaged engine | iOS 26.5 (23F77) | 355 | 36 | — | Passed |

The final demo control run on iOS 26.5 executed four tests, all passing:
annotation edit/undo/redo, bookmark creation/navigation, accessibility XXXL
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

## Files investigation and remaining uncertainty

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
Manual iOS 27.2 folder activation has been requested to distinguish synthetic
interaction failure from a user-facing picker failure. Merge review should
resolve this uncertainty. macOS native file-panel interaction was not automated;
its models/capabilities and both package modes were tested.

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

## Deferred minors

Pending the one whole-branch review.
