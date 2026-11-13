# CU-C7 decisions

Decisions made while integrating the three port branches onto `research/catchup-2026-11-12-plugins`,
running the full checks, building the Rust-core payload and checking the `platforms/` wrappers.
Each names the commit that carries it; "the docs commit" is the one that adds this file. None is a
NEW DIVERGENCE, and nothing under `packages/**` changed.

## CU-C7-D1. Merge in the order c4, c5, c6, and regenerate the one conflict

Decision: three `--no-ff` merges, `c339af05d` (c4), `c4b25c066` (c5) and `87b752368` (c6), with
git's default messages. The only conflict was `tooling/plugin-conformance/vectors/coverage.json`;
it was regenerated with `pnpm run coverage`, not resolved by picking a side.
Reason: c4 and c5 touch no common file. c5 and c6 share four kernel files, and their changes there
are the same cherry-picked patches (D-10), so they merged cleanly. The regenerated file reaches
72,838 statements of 77,093, more than either side (c4+c5 72,776, c6 72,242).
Cost: none.
Reverse: n/a.

## CU-C7-D2. Wrapper and FFI tests follow once-per-store-change asking

Decision: three tests that expected a command's hooks to be asked on every read now follow
upstream `7af98f4df`, which asks once per store change: `crates/plugins/commands/tests/ffi.rs`
`a_native_command_lock_disables_resolution_and_releases` notifies the store before reading the
released lock (`27b218242`); the Kotlin `CommandContextTest.kt` sets the app state before each of
its three resolves, and the Swift `everyHookReceivesTheWholeReplacementAppState` lower bound drops
from 9 to 7 (`791a8ee29`).
Reason: CU-C6 ported the semantics and updated the Rust behaviour tests it ran (CU-C6-D8). The FFI
test needs the `uniffi` feature, so it only runs in the workspace build, and the wrapper tests run
only in this job's platform pass.
Cost: none; the tests still check what they named.
Reverse: n/a.

## CU-C7-D3. `labelParams` that aren't finite match upstream (CU-C6 F1)

Decision: the answers memo translates the label from `params_through_json(&params)`, which turns a
non-finite number into the text `null`, as upstream's `JSON.parse(JSON.stringify(labelParams))` does
(`1820918b6`). `getRefusal` still reads `labelParams` directly and shows `NaN`, as upstream does.
Case: `label-params-that-are-not-finite` (`aafad823e`).
Reason: the port can match exactly, so it isn't logged as a divergence. `Param::Text("null")`
interpolates as `null` and, being no number, picks no plural form, which is what a JSON `null` does
upstream.
Cost: none.
Reverse: revert `1820918b6` and log a NEW DIVERGENCE instead.

## CU-C7-D4. Commands cases for the translated refusal sentences (CU-C6 F2)

Decision: case `refusal-sentences-with-i18n` (`aafad823e`) covers the turned-off, not-allowed and
locked sentences through i18n, each key's fallback, and a `locked` that throws. The commands suite's
`decodeCommandValue` now reads `{"$number": "NaN"}` as canonical.ts writes it, and the suite type
lists `locked` as a scripted member.
Reason: the generator records plugin configs as plain JSON, so a literal `NaN` reached the vector as
`null` and the Rust side read no number at all. Writing the value as canonical.ts does keeps both
sides on the same input.
Cost: the suite has one more encoding rule to know about.
Reverse: revert `aafad823e` and regenerate.

## CU-C7-D5. Re-capture the commands evidence and count the catch-up cases

Decision: the C-series captures rerun in dependency order, rewriting
`historical/task-12b2c5/current-inputs-runtime.json`; `chainpins.py` moved its one integrity pin. The
live evidence counts the two new commands cases and CU-C6's three stage-turns cases as catch-up
additions, each count written as `archived + additions` with the upstream commits named
(`a11b18eb3`).
Reason: CU-C6 changed the `derivations` covers text and added the wheel cases without rerunning the
evidence tests (8 of 679 failed at the merge). The method is CU-C3-D5 and CU-C5-D12's.
Cost: those test files move a third step from the bytes the original migration preserved.
Reverse: revert `a11b18eb3`.

## CU-C7-D6. The payload script uses emsdk 4's own config and sysroot (CU-C1 R1)

Decision: `build-rust-payload.sh` unsets `EM_BINARYEN_ROOT` and `EM_CONFIG` before sourcing emsdk
4.0.0 (`6cf349a12`) and runs `embuilder build sysroot` before the build (`40bef3b47`).
Reason: with the image's `EM_CONFIG`, emcc 4.0.0 read `/opt/emsdk/.emscripten` and took LLVM from
`/opt/emsdk/upstream/bin` and the image's binaryen, so the earlier payloads were built with the
3.1.72 toolchain under emcc 4.0.0's driver. With its own config emcc starts from an empty cache, and
`pdfium-sys` runs bindgen against that cache's sysroot before emcc installs headers, which failed
with `fpdf_ext.h: 'time.h' file not found` on the first attempt.
Cost: the first build after the change regenerates emsdk 4's cache.
Reverse: revert both commits.

## CU-C7-D7. Platform baselines run on the starting plugins tip with the same PDFium

Decision: the baseline for `.NET`, Kotlin and Swift is `7990560d6` in container worktree
`/workspaces/wt/c7-base`, with the integrated head's linux-arm64 PDFium (`aa59ce554`) and, for
`pack` and `validate`, the head's `engine-runtime-build.json` and runtime archive.
Reason: the brief asks for a baseline on the starting tip, and the 2026-11-07 platform runs weren't
recorded in a reusable form. The fork pin at `7990560d6` (`3c6e02297`) exports a subset of
`aa59ce554`'s functions (CU-C1: 5 added, none removed), and using one runtime for both sides keeps the
comparison about the wrappers.
Cost: the baseline doesn't exercise the starting tip's own PDFium.
Reverse: fetch the `3c6e02297` linux-arm64 library and rerun `platforms.sh` on the base worktree.

## CU-C7-D8. A new Swift-on-Linux harness, compared head against base

Decision: `scripts/swift-linux/` builds `epdf-viewer-host` as a shared library, generates its Swift
bindings, and assembles a SwiftPM package of `EmbedPDFBindings`, `EmbedPDF` and the
`EmbedPDFBindingsSmokeTests` and `EmbedPDFTests` targets over stub modules for `CoreGraphics`,
`ImageIO`, `Security`, `JavaScriptCore` and `os` (`4fd6375ee`). `EmbedPDFUI` is left out. Every
copied source gets `import CoreGraphics` first, which re-exports `Foundation` and
`FoundationNetworking` as Apple's Foundation would; test targets build with
`-disable-cross-import-overlays`.
Reason: the 2026-11-07 harness lived only on that catch-up's deleted volume. Its 293 of 325 isn't
comparable with this one's counts; running the same harness on both sides is.
Cost: the stubs fail every keychain, script and image-decoding test by design, so 38 of 504 tests
fail on both sides.
Reverse: n/a.

## CU-C7-D9. Kotlin on the JVM means `test-command-context.sh`

Decision: the Kotlin check is `platforms/android/test-command-context.sh` (the command-context test
and the checksum-mismatch test) with kotlinc 2.2.21, JDK 21.0.13, JNA 5.14.0 and coroutines 1.10.2
under `/workspaces/tools`. The AAR's instrumented `SmokeTest.kt` isn't run.
Reason: it's the repository's JVM entry point; the smoke test is an Android instrumentation test,
and the emulator is out of scope (D-06).
Cost: the bindings the AAR ships are compiled here only through the full viewer bindings this test
compiles.
Reverse: n/a.

## CU-C7-D10. No binding renames to apply

Decision: no wrapper source changed for bindings. CU-C4 and CU-C5 recorded no binding change, and
CU-C6-D7's stage additions (`scroll_chaining`, `StageSettingKey::ScrollChaining`, `wheel_by`) are
additive.
Reason: every wrapper builds against regenerated bindings on both sides: `.NET` 33 of 33, Kotlin
passes, Swift builds.
Cost: the wrappers don't expose `wheel_by` or `scrollChaining` in their own APIs yet.
Reverse: n/a.

## CU-C7-D11. core-wasm and epdf-web are compared with the 2026-11-07 final state

Decision: `core-wasm` vitest, `test:gc` and the `epdf-web` wasm tests ran only on the integrated
head, with the Rust-core payload built here. Their reference is the 2026-11-07 ledger's final row
(92 of 92 and 13 of 13).
Reason: a start-tip run needs a second full prep and payload build of `7990560d6`, and the
integrated head passes everything.
Cost: no same-day start column for these three.
Reverse: run `prep.sh`, `build-rust-payload.sh` and the two vitest configs in the base worktree.

## CU-C7-D12. TS package tests are counted with turbo's cache

Decision: `turbo run test` over `packages/**` is reported as run: 98 of 98, of which 41 tasks came
from turbo's cache in the final run and 57 executed (all 98 were cache hits at the merge).
Reason: nothing under `packages/**` changed in the port or the integration, so a cached task's inputs
are the ones CU-C3's passing run hashed.
Cost: 41 of the 98 tasks didn't execute again in this job.
Reverse: `pnpm exec turbo run test --force --filter='./packages/**'`.

## CU-C7-D13. The rust-viewer-bundle typecheck error stays as found

Decision: `@mfb/embedpdf-rust-viewer` `typecheck` fails with TS7016 on
`packages/engine/main/src/index.ts(499,50)`: no declaration for `#inline-transport`. Not fixed.
Reason: the 2026-11-07 ledger recorded the same error at line 476, and the file is upstream's.
`core-wasm` typechecks.
Cost: the bundle's typecheck stays red.
Reverse: add a declaration for the `#inline-transport` import in the bundle's own types.

## CU-C7-D14. CU-C5 R6 left as a remaining advisory

Decision: no case for form `sync/predict.ts` 272-273.
Reason: the finding was optional; a case needs a pending copy whose source field leaves the form
while the copy waits, a new scenario shape in the form-design suite.
Cost: the two lines stay unreached by vectors.
Reverse: add the case.

## CU-C7-D15. Toolchains for the platform checks live on the volume

Decision: .NET SDK 10.0.403, Temurin JDK 21.0.13, kotlinc 2.2.21 and the two jars are installed under
`/workspaces/tools` (the volume, removed with it). The .NET install script came from the
`dotnet/install-scripts` GitHub repository because `dot.net` timed out from the container.
Reason: the image has none of them.
Cost: none after the volume goes.
Reverse: n/a.

## CU-C7-D16. Local impl branch fast-forwarded

Decision: `git fetch . research/catchup-2026-11-12-impl:research/multi-platform-rust-core-impl`
moved the local impl branch `e4c17f48d..a7c75f5ad`. The plugins branch isn't moved; the command is
in the ledger.
Reason: the brief; impl's tip was an ancestor and the branch isn't checked out anywhere.
Cost: none.
Reverse: `git branch -f research/multi-platform-rust-core-impl e4c17f48d`.
