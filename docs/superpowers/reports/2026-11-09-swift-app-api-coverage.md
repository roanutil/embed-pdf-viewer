# Swift app-facing API coverage — 2026-11-09

## Delivered scope

The plugins branch exposes typed app-facing capabilities over the existing Rust
owners. Source changes do not publish a runtime or update either engine pin.
Minimum platforms remain iOS 17 and macOS 14; dependencies are unchanged.

- Documents: URL loading through KernelOptions.fetch and Foundation URLSession;
  retry/rename/close-all; document lookup, selection and order; page lookup/index/
  revision; permission/download checks; layer save and base+layer reopen.
- Settings: current/read/update/reset for actions, annotation, commands, form,
  i18n, interaction, measurement, redaction, render, search, selection, signature
  and stamp. Reset restores registration values and retains registered ports.
  Viewer settings are exposed; independent observers use observeStateRevisions.
- Editing: per-command disable/enable/read/set and structured permanent refusals,
  named-action probing, annotation bundle transfer and selected-link editing,
  independent form widget edits/deletion and calculation order.
- Form payload audit: create(draftJSON:), update(_:patchJSON:),
  addWidget(to:placementJSON:) preserve the engine's open payloads alongside
  typed conveniences. fieldSettings(_:) returns typed family-specific panel
  descriptors plus values JSON. Action writes use {type, script, next}, not the
  read tree's root wrapper. Omission preserves values; explicit null clears
  nullable entries. Invalid JSON/non-object payloads refuse before mutation.
- Native viewports: held snapshot metadata and cancellable image resolution,
  clear/box/change observation, zoom-box operations, owned pixels after clear/
  close, and crop/intrinsic/display rotation with per-render admitted geometry.

Rust retains permissions, staging, state, geometry, reset semantics, cancellation
and lifetime. Caller cancellation ends a staged-write wait; an admitted write
continues to reconcile through the kernel. Reads do not initiate engine work.

## Audit accounting

The checker includes public plugin capability/Core/ViewDemand methods and public
kernel Documents and Kernel methods. It checks Rust exports and handwritten Swift
declarations, including the form payload overloads. Counts measure reviewed
accounting, not implementation parity or independent Swift operations.

| Inventory | Mapped | Excluded | Total |
| --- | ---: | ---: | ---: |
| Plugin controller/core/view demand | 686 | 106 | 792 |
| Kernel Documents and Kernel | 38 | 30 | 68 |
| Total | 724 | 136 | 860 |

All 31 plugin-directory crates are classified. Nine pure libraries (acrojs,
annotation-core, fonts, form-core, geometry, signature-core, stage-core,
text-core, ui-core) remain consumed by their Rust owners, with no fictional
plugin capability. Other kernel types—stores, queues, scopes, mirrors, signals,
event hooks, engine/host ports and settings implementation—are explicitly
internal resources rather than app capabilities.

| Exclusion classification | Members |
| --- | ---: |
| Host presentation | 64 |
| Internal resource | 58 |
| Equivalent representation | 9 |
| Host integration (printer) | 4 |
| Registration resource | 1 |

The machine-readable inventory gives a reason for every exclusion. Remaining
categories are host presentation/painters/text-editor/pointer seams, internal
resources and sibling scripting ports, equivalent settings representations,
startup batch registration, and four printer operations. Printing is **not
implemented**: ViewerHost requires a native HostPrinter providing prepared PDF/
picture representations and disposal lifetime. Signers/trust/script factories/
locale loaders/assets remain registered host ports. Native painting continues
through owned images and render demand; this work does not build every editor UI.

## Validation

Rust inputs were committed and frozen at
`a68a422fc5b37bbd6258d57a060e72ff931b9b1d` before the four-slice build.
Fresh generated Swift compilation exposed missing public viewer-settings aliases
and asynchronous reads inside synchronous CustomDump assertion autoclosures. The
Swift facade aliases and tests were corrected; Rust inputs remained frozen. The
complete suites below pass with those corrections.

- New missing-form-API regression failed before implementation; object validation,
  settings panel, held cancellation and 10 form FFI tests then passed.
- Two real-host tests pass: direct option/script and widget authoring with
  omitted/null patch behavior and save/reopen; design/script permission refusals.
- Core-inclusive checker regression failed before implementation; all 7 checker
  tests pass, including missing core mappings, pure-library isolation and open
  payload overload checks. The inventory checker passes with the counts above.
- Clippy passes for kernel, viewer host and all 14 affected plugin crates,
  all targets, with -D warnings. Rust formatting passes for changed Rust files.
- Broader affected Rust suites: 1,045 passed, 0 failed or ignored across 127
  result groups (kernel, viewer host and the same 14 plugins). Log:
  /private/tmp/swift-task5-rust-tests.log.
- All four Apple slices rebuilt successfully: aarch64-apple-ios,
  aarch64-apple-ios-sim, aarch64-apple-darwin and x86_64-apple-darwin.
  Generated graph comparison and XCFramework creation passed. Command:
  `LIB_ROOT=<local fork runtime staging> EMBEDPDF_NM=<Rust llvm-nm> bash platforms/apple/build-xcframework.sh`.
  Log: /private/tmp/swift-task5-framework.log.
- Complete macOS suite: **427 passed**, 0 failures (378 API + 41 UI/model +
  8 generated-binding smoke). Command:
  `EMBEDPDF_LOCAL_ENGINE=1 swift test --package-path platforms/apple/EmbedPDF`.
  Log: /private/tmp/swift-task5-macos.log.
- Complete iOS 26.5 simulator suite: **427 passed**, 0 failures (same breakdown),
  xcodebuild exit 0, TEST SUCCEEDED. From platforms/apple/EmbedPDF:
  `EMBEDPDF_LOCAL_ENGINE=1 xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,id=5FA32A5D-6186-4C16-BE25-C87829DD1BFA' -derivedDataPath /private/tmp/swift-task5-ios-derived`.
  Log: /private/tmp/swift-task5-ios.log. The xcresult is under that derived-data
  directory's Logs/Test. Xcode 27.2 performed both native validations.

Final checks: inventory checker and all 7 checker tests pass; git diff --check
passes. No native runtime failures remain. Broad Rust/native tests were not
repeated after the final green runs because only this evidence report changed.

### Final fix wave

The whole-change review of `35b18b786..5e3236108` found one snapshot bug and
three accounting issues. Source freeze `96bb9c519` fixes all four, and a scoped
rereview of `5e3236108..96bb9c519` confirmed each one closed.

The snapshot raster now divides by the resolved image's `point_width()`
instead of a width captured before `changes.when_settled()`. Before the fix, a
layout change between capture and render admission returned a 2x image labelled
scale 1. `snapshot_uses_geometry_at_render_admission_after_the_settlement_gate`
in `crates/plugins/render/tests/ffi_snapshot.rs` covers 0 and 90 degree pages;
it failed with scale 1.0 before the fix. The checker now matches Swift argument
labels, so dropping `create(draftJSON:)`, `update(_:patchJSON:)` or
`addWidget(to:placementJSON:)` while keeping the typed overload is reported.
`StageCapability.list_current_pages` maps to `currentPages()`, which moves one
member from excluded to mapped. `Form.swift` no longer imports Foundation.

Evidence for `96bb9c519`:

- Affected Rust suites (render, stage, viewer host with uniffi features):
  351 passed, 0 failed or ignored across 26 result groups. Clippy all-targets
  with `-D warnings` and `cargo fmt --check` pass.
- Checker: 10 tests pass, and the inventory reports 724 mapped, 136 excluded,
  860 total.
- Four Apple slices rebuilt and the native graph verified.
- macOS: 427 passed (378 API, 41 UI/model, 8 binding smoke).
- iOS 26.5 simulator: 427 passed, same breakdown, `TEST SUCCEEDED`.

The fresh logs show no `UnusedImportAccess` warning. The iOS log still has
three `Metadata extraction skipped, no AppIntents.framework dependency found`
warnings. Xcode's `appintentsmetadataprocessor` emits them for any target that
doesn't link AppIntents. They are not build problems, and adding the dependency
to silence them would be the wrong fix.

Known gaps left open. The label parser fails closed on a few Swift shapes no
current entry uses: generic functions (`func f<T>(`), a `->` inside a generic
argument, a bare `<` in a default value, and parentheses inside `//` comments in
a parameter list. Each produces a false "missing Swift" error, never a false
pass. Separately, `test_native_graph.RawBuilderTests.test_raw_builder_builds_all_four_explicit_fork_and_sdk_pairs`
fails because it can't find `signature-compat/keys/TEST-ONLY-private-key.pem`.
That test was last changed in `c15b9671e`, and this fix wave doesn't touch it.

## Distribution boundary

Only source and handwritten documentation/tests are committed. Generated Swift,
fixtures and XCFramework archives remain ignored. The current downloaded engine
releases predate these exports. A matching runtime artifact must be published
and both package manifests pinned to it before default binary consumption can
use these APIs. No runtime release, checksum or package pin is changed here.
Local source-engine native results validate the source graph only.
