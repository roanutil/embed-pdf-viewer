# Swift API coverage handoff — 2026-11-09

## Resume here

User requested a handoff because Codex usage is nearly exhausted. Stop launching
new work in this session. **Do not restart the implementation or broad review.**
Five implementation tasks and their individual reviews are complete. The final
whole-change review found one snapshot scale bug and three minor audit/warning
issues. A single final fix wave is in progress; its edits must be preserved.

Repository: `/Users/andrew/repos/embed-pdf-viewer`.
Branch: `research/multi-platform-rust-core-impl-plugins`.
Last committed implementation: `5e3236108968d84ac45666ccac14c3b2d03e3c3a`.
Fork: `https://github.com/roanutil/embed-pdf-viewer.git` (`fork` remote).
Last pushed parity merge: `35b18b78641c25a761684c50390734ee4a1b6436`.
The newer app-facing implementation commits have **not been pushed**.

Read `git status --short` and the latest final-fix report before acting. A
source-freeze commit may have been added after this handoff was drafted.

## User authorization and scope

User requested merging `research/swift-api-plugin-parity-plan` into the plugins
branch and pushing to fork; transferring independent non-plugin fixes to
`research/multi-platform-rust-core-impl`; then closing remaining Swift gaps.
User selected **app-facing APIs**, and explicitly **approved and requested
implementation** of the design. Continue through verification, scoped review
and ordinary fork push without another approval/menu. Never force-push.

The parity merge is already pushed and independently verified. No independent
fix belongs on the core-only branch: its crates are build-support, epdf-engine,
epdf-node, epdf-uniffi, epdf-wasm, pdfium-sys, pdfium, test-support and
uniffi-bindgen. It has none of the kernel/plugin/viewer layers being extended.
Core-only branch remains unchanged.

Printing requires a real native `HostPrinter` and is deliberately unfinished.
Keep Rust-owned caches, painters, editor/pointer plumbing and sibling scripting
internals private. No runtime release, checksum or package pin update is
authorized; downloaded engines predate these APIs. Source/local-engine tests
do not make default downloaded binaries usable with the new APIs.

## Completed work — do not repeat

1. Document/core controls, settings, layer save/reopen and URLSession through
   the kernel fetch boundary; retry preserves failed tabs and source ownership.
2. Typed live read/update/reset settings across thirteen plugins, preserving
   registrations, omission/clear behavior and Rust permission ownership.
3. Command controls, named action probe, annotation bundle/link editing,
   independent form widget editing and calculation order; form domain JSON
   create/update/widget overloads and typed settings-panel descriptors.
4. Immediate snapshot metadata with separately cancellable image readers,
   owned pixels, crop/rotation scale and native zoom-box controls. Batch render
   geometry was already fixed in `df87a1626` and reviewed.
5. Kernel-inclusive API accounting and full fresh native validation. All task
   reviews approved. Earlier review findings are closed.

Design: `docs/superpowers/specs/2026-11-09-swift-app-api-coverage-design.md`.
Plan: `docs/superpowers/plans/2026-11-09-swift-app-api-coverage.md`.
Delivered report: `docs/superpowers/reports/2026-11-09-swift-app-api-coverage.md`.
Parity integration: `docs/superpowers/reports/2026-11-09-swift-parity-integration.md`.
Final review: `docs/superpowers/reports/2026-11-09-swift-app-api-final-review.md`.

## Current final fix wave

Original snapshot implementer `/root/swift_snapshots` owns the complete wave;
no additional implementers or broad review are needed. Base is `5e3236108`.
Latest checkpoint before handoff:

1. Snapshot regression reproduced **RED: scale 1 instead of 2**. Snapshot
   `Raster::read` used display width captured before `changes.when_settled()`;
   page geometry could change before render admission. Fix uses resolved
   `PageImageHandle.point_width()` and removes obsolete pre-gate display width.
   Added 0° width-shrink and 90° height-shrink cases, checking immediate
   metadata/event rect stays unchanged and render request uses actual clipping.
   Affected Rust suites/clippy were running; inspect final-fix report for result.
2. Checker regression exposed six missed-removal cases. Fix checks Swift
   argument labels for typed and JSON overloads. **10/10 checker tests pass**.
3. Map delivered `StageCapability.list_current_pages` to `current_pages` /
   `currentPages()`. Correct accounting: **860 total, 724 mapped, 136 excluded**;
   plugin **792 = 686 mapped + 106 excluded**, kernel **68 = 38 + 30**.
4. Unused public Foundation import removed from Form.swift. Document benign
   Xcode AppIntents metadata extraction warnings in final report; do not change
   unrelated product capabilities to silence tool output.

At handoff draft, these nine files were modified and uncommitted:
`crates/plugins/render/src/{contract.rs,snapshot.rs,ffi/snapshot.rs}`,
`crates/plugins/render/tests/{ffi_snapshot.rs,support/mod.rs}`,
`platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Form.swift`,
`platforms/apple/{check-swift-api-coverage.py,swift-api-coverage.json,tests/test_swift_api_coverage.py}`.
Do not discard them. No final-fix framework rebuild/native rerun had started at
the latest checkpoint. Read agent checkpoint below for newer evidence.

## Evidence already complete

### Final checkpoint at pause

Agent stopped launching work. Nine source/test edits remain uncommitted.
New snapshot regressions are GREEN; checker **10/10 GREEN**; clippy completed
successfully. The existing affected Rust suite is still running in exec
session **16712**, log
`.superpowers/sdd/2026-11-09-swift-app-api-coverage/final-fix-logs/rust-tests.log`.
Inspect that session/log before starting another command. No Apple rebuild
has started. A tracked copy of the fix checkpoint is
`docs/superpowers/reports/2026-11-09-swift-api-final-fix-checkpoint.md`.

For Rust source freeze `a68a422fc` plus Swift fixes `5e3236108`:

- **1,045 Rust tests**, zero failed/ignored across 127 groups; clippy all-targets
  `-D warnings`; formatting and seven then-current checker tests passed.
- Four Apple slices built; generated graph comparison and XCFramework creation
  passed. Generated Swift/framework/fixtures are ignored.
- **427 macOS + 427 iOS 26.5 tests passed**, each comprising 378 API,
  41 UI/model and 8 binding smoke tests. Both used `EMBEDPDF_LOCAL_ENGINE=1`.
- Logs: `/private/tmp/swift-task5-{rust-tests,clippy,framework,macos,ios}.log`.
  iOS results: `/private/tmp/swift-task5-ios-derived/Logs/Test`.

These green native runs predate the final snapshot fix. Do not claim the final
working tree has fresh native verification until its rebuild and runs finish.
No reason to repeat the unrelated 1,045-test run or restart a broad audit.

## Remaining bounded steps

1. Read `.superpowers/sdd/2026-11-09-swift-app-api-coverage/final-fix-report.md`
   and current git state. Finish affected Rust checks if necessary; verify all
   four findings above, commit the source freeze, then rebuild four Apple slices.
2. Run complete macOS and iOS suites against the rebuilt local engine, update
   tracked final report/README counts and evidence, and commit documentation.
3. Generate a review package from `5e3236108` to final HEAD. Resume
   `/root/swift_whole_change_review` for **one scoped rereview of these four
   findings and new breakage in this fix diff only**. No second broad review.
4. With findings addressed and checks green, inspect clean status and
   `git diff --check`; push normally to `fork` on the existing plugins branch,
   verify remote SHA. Do not merge into main, create an unsolicited PR, change
   pins, publish runtime artifacts, or delete any source branch.
5. Final user response: pushed SHA, major app-facing workflows, exact final
   tests, printing and downloaded-runtime boundary, link final report. Preserve
   the rulings below. Next user action should take under two minutes.

## Build environment and commands

Use `require_escalated` for cache/native/git writes as needed. Earlier approval
review allowed them. Default runtime headers are stale; use this staging root:

```sh
export PDF_RUNTIME_LIB_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work/fork/3c6e0229738d19c8f724251bb9c479aecbd18a00/runtime-src/out/embedpdf-runtime-staging
export EMBEDPDF_FORK_TARGET=darwin-static-arm64
export LIB_ROOT="$PDF_RUNTIME_LIB_DIR"
export EMBEDPDF_NM=/Users/andrew/.rustup/toolchains/1.98.0-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-nm
bash platforms/apple/build-xcframework.sh
EMBEDPDF_LOCAL_ENGINE=1 swift test --package-path platforms/apple/EmbedPDF
```

Run iOS from `platforms/apple/EmbedPDF`:

```sh
EMBEDPDF_LOCAL_ENGINE=1 xcodebuild test -scheme EmbedPDF-Package \
  -destination 'platform=iOS Simulator,id=5FA32A5D-6186-4C16-BE25-C87829DD1BFA' \
  -derivedDataPath /private/tmp/swift-task5-ios-derived
```

Keep Rust inputs frozen during framework build. Apple nm cannot read this
Rust toolchain's bitcode; retain the llvm-nm override. Source-only Swift fixes
do not require rebuilding unchanged Rust slices.

## Recovery artifacts and process

Ignored workspace `.superpowers/sdd/2026-11-09-swift-app-api-coverage/` contains
`progress.md`, task briefs/reports/reviews, diff packages and final review.
Use these as the recovery map. Do not delete this workspace while handing off.
The final-fix report is being persisted there. Standalone reviewer reproduction
is `/private/tmp/swift-final-snapshot-review/reproduce.py`; its cached graph
paths were environment-specific, so prefer the now-added repository regression.

Applied skills: conventions; brainstorming/design approval; writing-plans;
subagent-driven-development; TDD; requesting/receiving review; verification.
Task reviews are already complete. SDD final review allows ONE fix wave and
ONE scoped rereview. This is that wave. Do not spawn duplicate reviewers or
repeat completed tasks after context recovery.

## Rulings to preserve in final handoff

1. Keep the existing named-branch checkout: explicitly requested branch and
   approved design choice; cost if wrong is lack of separate checkout isolation.
2. Sequential implementers/reviews because files overlap; cost if wrong is
   extra elapsed time compared with independent task partitions.
3. Viewer settings change presentation, not existing engine-session authority;
   retry retains kernel close-driven cancellation. Cost if wrong: callers must
   close to cancel retries and cannot reauthorize open sessions through settings.

Final review's declined scope was accepted: native printing, runtime release,
private host/painter/editor seams, reauthorization/retry redesign and separately
verified parity/core-only transfer remain as documented boundaries.
