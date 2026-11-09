# Final bounded fix wave — immediate handoff

Status: PAUSED at the user's usage-limit request. No new builds/tests should be launched until resumed. No push or runtime publication. Source edits are complete but UNCOMMITTED. No Apple rebuild has started. BASE/HEAD remains `5e3236108968d84ac45666ccac14c3b2d03e3c3a`.

## Completed source edits (nine uncommitted files)

- `crates/plugins/render/src/contract.rs`: removed obsolete pre-render snapshot display width.
- `crates/plugins/render/src/ffi/snapshot.rs`: Raster conversion uses resolved PageImageHandle.point_width(), with invalid-state if absent; metadata still immediate.
- `crates/plugins/render/src/snapshot.rs`: removed capture-time divisor storage; preserves original capture and settlement gate.
- `crates/plugins/render/tests/ffi_snapshot.rs`: new gated regression, 0-degree width shrink and 90-degree height shrink, immediate metadata/event rect unchanged, actual clipped request checked, pixels-per-point scale stays 2.
- `crates/plugins/render/tests/support/mod.rs`: capture_before_resize dispatches capture and layout event in the same kernel turn, before the existing settlement gate opens.
- `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Form.swift`: removed unused public Foundation import; only bindings import remains.
- `platforms/apple/check-swift-api-coverage.py`: optional Swift argument-label matching, splitting top-level parameters while respecting nested types/default expressions; preserves existing base-name checks for unlabeled inventory entries.
- `platforms/apple/swift-api-coverage.json`: typed and JSON form overload labels recorded; list_current_pages maps to StageHandle.current_pages / StageCapability.currentPages. Counts 724 mapped / 136 excluded / 860 total.
- `platforms/apple/tests/test_swift_api_coverage.py`: three new tests remove each typed/JSON overload independently, six removal subcases; now ten tests.

## Findings and evidence

Read complete final-review.md and task-5-report.md. Reviewer reproduction inputs were read at `/private/tmp/swift-final-snapshot-review/{regression.rs,support.rs,result.log,reproduce.py}`. Repository regression uses the same settlement-gate interleaving, expanded to quarter-turn geometry and immediate metadata/event preservation.

Logs live in `.superpowers/sdd/2026-11-09-swift-app-api-coverage/final-fix-logs/`.

- `snapshot-red.log`: expected feature RED, exit 101; actual `(width,height,scale) = (2,4,1.0)` vs expected `(2,4,2.0)`. Initial test-only JSON integer/float mismatch was corrected before recording this feature failure.
- `rust-tests.log`: new `snapshot_uses_geometry_at_render_admission_after_the_settlement_gate` is GREEN. Full affected render/stage/host suite was already running when pause arrived; unified exec session **16712**. Latest inspected progress: 18 successful result groups, 169 passed/0 failed, beginning native `render_rights` suite. Do not infer final total from partial log. Keep this useful command running; inspect its eventual exit/log on resume. Expected final count likely 351, but use actual output.
- `checker-red.log`: ten tests executed; six expected removal-subcase failures because old checker matched only base names.
- `checker-green.log`: **10 tests, OK**, exit 0.
- `inventory.log`: **31 plugin-directory crates + kernel, 860 members (724 mapped, 136 exclusions)**, exit 0.
- `clippy.log`: complete affected all-targets clippy with `-D warnings`, exit 0 / `Finished dev profile`; unified exec session **66238** (completed).
- `fmt.log`: cargo fmt applied to render only, exit 0. `git diff --check` was run after edits and passed. No final explicit --check yet; run on resume before source freeze if needed.

Affected Rust commands already launched (do not start duplicate work):

```sh
env PDF_RUNTIME_LIB_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work/fork/3c6e0229738d19c8f724251bb9c479aecbd18a00/runtime-src/out/embedpdf-runtime-staging EMBEDPDF_FORK_TARGET=darwin-static-arm64 cargo test -p epdf-plugin-render -p epdf-plugin-stage -p epdf-viewer-host --features epdf-plugin-render/uniffi,epdf-plugin-stage/uniffi
env PDF_RUNTIME_LIB_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work/fork/3c6e0229738d19c8f724251bb9c479aecbd18a00/runtime-src/out/embedpdf-runtime-staging EMBEDPDF_FORK_TARGET=darwin-static-arm64 cargo clippy -p epdf-plugin-render -p epdf-plugin-stage -p epdf-viewer-host --features epdf-plugin-render/uniffi,epdf-plugin-stage/uniffi --all-targets -- -D warnings
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s platforms/apple/tests -p test_swift_api_coverage.py
python3 platforms/apple/check-swift-api-coverage.py
```

## Remaining bounded steps on resume

1. Collect existing Rust session 16712 completion, count results, inspect final source diff, format/diff checks, and commit the explicit nine source/test files. Record exact source-freeze SHA. Do not repeat the unrelated 1,045-test suite. No generated/binary files in commits.
2. With source frozen, rebuild all four Apple slices:

```sh
env LIB_ROOT=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work/fork/3c6e0229738d19c8f724251bb9c479aecbd18a00/runtime-src/out/embedpdf-runtime-staging EMBEDPDF_NM=/Users/andrew/.rustup/toolchains/1.98.0-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-nm bash platforms/apple/build-xcframework.sh
```

3. Run complete local-engine suites on rebuilt source (expected same 427 each; use actual counts):

```sh
EMBEDPDF_LOCAL_ENGINE=1 swift test --package-path platforms/apple/EmbedPDF
# From platforms/apple/EmbedPDF:
EMBEDPDF_LOCAL_ENGINE=1 xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,id=5FA32A5D-6186-4C16-BE25-C87829DD1BFA' -derivedDataPath /private/tmp/swift-task5-ios-derived
```

Use require_escalated for Cargo/native cache and git writes; redirect logs into final-fix-logs. Existing derived data is authorized for reuse. Do not change source during build.

4. Update tracked `docs/superpowers/reports/2026-11-09-swift-app-api-coverage.md` and `platforms/apple/README.md`: totals 724 mapped/136 excluded; plugin 686 mapped/106 excluded; host-presentation exclusions 64; checker ten tests. These tracked documents currently still have OLD counts and OLD build/test evidence. Preserve historic 1,045-test evidence as prior validation; add exact new affected-suite/build/native evidence and source SHA. Explain Xcode AppIntents metadata-extraction warnings as benign tool-generated messages (no AppIntents framework dependency), and verify unused Foundation import warning is absent in fresh native logs. The reviewer saw three AppIntents warnings in previous iOS logs; report actual new output rather than assuming exact recurrence count.
5. Append final results/SHAs here and commit documentation/evidence separately. No push; coordinator owns it. No runtime publishing.

## Accepted boundaries to retain

Printing remains unimplemented pending a real HostPrinter prepared-representation/disposal contract. Default downloaded binaries and both pins remain unchanged; rebuilding/source commits are not runtime publication. Keep iOS17/macOS14 and dependencies unchanged. Private painters, cache resources, editor layout and sibling scripting seams remain with their Rust/native owners. Engine session authority and close-driven retry cancellation remain the approved contracts. The separately pushed parity merge/core-only-branch decision is not reopened. No new broad behavioral review, delegation, or scope expansion is authorized in this one fix wave.

## Resumed and closed (2026-11-09)

Source freeze `96bb9c519`. Affected Rust suites 351 passed; checker 10 tests; four slices rebuilt; macOS and iOS 427 passed each. Scoped rereview confirmed all four findings fixed. Evidence is in `docs/superpowers/reports/2026-11-09-swift-app-api-coverage.md` under "Final fix wave".
