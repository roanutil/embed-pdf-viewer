# CU-C7: integration, full checks, wasm and platform wrappers

`research/catchup-2026-11-12-plugins` now holds the impl catch-up, the starting plugins tip and all
three port branches, and every check passes on it: `cargo test --workspace` 3,305 passed and 0
failed (1 ignored), engine `:check` 35 of 35, the four plugin checks, evidence 679 of 679, TS
package tests 98 of 98, `core-wasm` 102 of 102 and `test:gc` 13 of 13 on a Rust-core payload built
here, `epdf-web`'s wasm tests, `.NET` 33 of 33 with `validate.sh --rid linux-arm64` passing, and
Kotlin on the JVM. Swift on Linux fails the same 38 of 504 tests as the starting plugins tip, all
stub or Linux causes. The three CU-C6 findings and CU-C1 R1 are fixed, there is no NEW
DIVERGENCE, and nothing under `packages/**` changed. Decisions are in
[`../decisions/CU-C7.md`](../decisions/CU-C7.md); the consolidated log of the whole catch-up is
[`../decisions.md`](../decisions.md).

Base `0879a6d225d16fbfff92fcc20e4140ea2faf60e3` (CU-C3's tip, where the port branches start). The
code head the checks ran on is `4fd6375ee6ed5e0ac826122e6c1d7154d01f322b`; the docs commit that
adds this report sits on it and changes no code.

## Commits

| Commit | Summary |
|---|---|
| `c339af05d` | Merge `research/catchup-2026-11-12-c4` (engine port, `changes` vectors) |
| `c4b25c066` | Merge `research/catchup-2026-11-12-c5` (annotation and form family) |
| `87b752368` | Merge `research/catchup-2026-11-12-c6` (kernel, stage, the rest); `coverage.json` regenerated |
| `27b218242` | commands FFI test notifies the store before reading a released native lock (`7af98f4df`) |
| `1820918b6` | commands: the resolved label is translated from `labelParams` as JSON (CU-C6 F1) |
| `6cf349a12` | payload script unsets the image's `EM_BINARYEN_ROOT` and `EM_CONFIG` (CU-C1 R1) |
| `aafad823e` | commands cases `label-params-that-are-not-finite` and `refusal-sentences-with-i18n` (CU-C6 F1, F2); vector and `coverage.json` |
| `a11b18eb3` | commands evidence re-captured; live evidence counts the catch-up cases |
| `643e73c56` | messaging references removed from `decisions/CU-C6.md` and `reports/CU-C4.md` (CU-C6 F4) |
| `40bef3b47` | payload script installs emsdk 4's sysroot headers first |
| `791a8ee29` | Kotlin and Swift command tests follow once-per-store-change asking |
| `4fd6375ee` | scripts: `swift-linux/`, `platforms.sh`, `evrun.sh` |
| the docs commit | this report, `decisions/CU-C7.md`, `decisions.md`, the ledger |

## Merges

c4 shares no file with c5 or c6. c5 and c6 share four kernel files
(`changes.rs`, `kernel.rs`, `page_predict.rs`, `page_view.rs`) whose changes are the same
cherry-picked patches (D-10), so they merged without a conflict. The only conflict was
`tooling/plugin-conformance/vectors/coverage.json`, regenerated after `prep.sh`: statements 72,838
of 77,093, against 72,776 (c4+c5) and 72,242 (c6). Measurement scale through the native engine
(D-12, CU-C5-D10) passes on the integrated branch.

## Checks, before and after

"Merge" is `87b752368`, the three merges before any CU-C7 fix. "Now" is `4fd6375ee`.

| Check | CU-C3 (`0879a6d22`) | Merge (`87b752368`) | Now (`4fd6375ee`) |
|---|---|---|---|
| `cargo test --workspace` | 3,269 passed, 23 failed | 3,304 passed, 1 failed | 3,305 passed, 0 failed, 1 ignored |
| engine `:check` scripts | 34 of 34 | 35 of 35 | 35 of 35 |
| plugin `vectors:check`, `coverage:check`, `test:harness`, `typecheck` | all pass | all pass | all pass; statements 72,853 of 77,093 |
| plugin evidence tests | 661 of 677 | 671 of 679 | 679 of 679 |
| TS package tests | 98 of 98 | 98 of 98 (all cached) | 98 of 98 (57 ran, 41 cached) |

The one cargo failure at the merge was `epdf-plugin-commands` `tests/ffi.rs`
`a_native_command_lock_disables_resolution_and_releases` (category: test pinned removed behaviour,
CU-C7-D2). It only builds with the `uniffi` feature, which the workspace build turns on and
CU-C6's per-crate run didn't. The eight evidence failures were one cause each in `commands-current`,
`link-current`, `i18n-current`, `metadata-current`, `page-edit-current`, `shell-current` and
`view-manager-current` (the `derivations` covers text CU-C6 changed, recorded in the archives) and
`current-suites` (CU-C6's three stage-turns cases not counted) (CU-C7-D5).

### wasm

| Check | Reference (2026-11-07 final) | Now |
|---|---|---|
| Rust-core `wasm32` payload (`wasm32-eh`, emsdk 4.0.0) | built | built; all 1,061 bound functions match |
| `core-wasm` vitest | 92 of 92 | 102 of 102 (12 files) |
| `core-wasm` `test:gc` | 13 of 13 | 13 of 13 |
| `epdf-web` wasm tests (`wasm-bindgen-test-runner`) | | 9 binaries, all pass |
| `core-wasm` `typecheck` | | pass |
| `@mfb/embedpdf-rust-viewer` `typecheck` | TS7016 `#inline-transport` at `index.ts(476)` | same error at `index.ts(499,50)` (CU-C7-D13) |

The first payload attempt failed (`fpdf_ext.h: 'time.h' file not found`) once emcc 4.0.0 ran with
its own config; `40bef3b47` installs the sysroot first (CU-C7-D6). No shim in `core-wasm`, `epdf-web`
or `rust-viewer-bundle` needed a change for upstream's API.

### Platform wrappers, starting plugins tip against now

Both sides ran `scripts/platforms.sh` in the container with the same PDFium (`aa59ce554`), runtime
manifest and toolchains (CU-C7-D7, CU-C7-D15).

| Check | Start (`7990560d6`) | Now (`4fd6375ee`) |
|---|---|---|
| `.NET` `dotnet test` (`EmbedPDF.Tests`) | 33 of 33 | 33 of 33 |
| `.NET` parity self-test | 16 of 16 | 16 of 16 |
| `.NET` packaging unit tests | OK | OK |
| `pack.sh` + `validate.sh --rid linux-arm64` | PASS | PASS |
| Kotlin `test-command-context.sh` on JDK 21 | pass | pass |
| Swift on Linux, `EmbedPDFBindingsSmokeTests` | 8 of 8 | 8 of 8 |
| Swift on Linux, Swift Testing | 466 of 504; the same 38 fail | 466 of 504; the same 38 fail |

The 38 Swift failures by file: `SignatureTests` 22, `KeychainSignerTests` 4 and `SecurityTests` 1
(the `Security` stub makes no keys), `JavaScriptCoreEngineTests` 4 (the `JavaScriptCore` stub
evaluates nothing), `StampTests` 2 (no image decoder), `ProductsTests` 2 (they read the repository's
root `Package.swift`, which isn't beside the generated package), `DocumentAPITests` 1 (Linux
`URLSession` doesn't route through the test's `URLProtocol`), `AnnotationDraftTests` 1 (Foundation on
Linux compares a boolean `NSNumber` differently). Before the test fixes in `791a8ee29`, head also
failed Kotlin's command-context test and Swift's `everyHookReceivesTheWholeReplacementAppState`
(CU-C7-D2). No binding renames were needed (CU-C7-D10).

## Vectors regenerated

| Group | Change | Upstream |
|---|---|---|
| `plugins/commands/plugin.json` | two new cases, nothing else changed (`git diff` is additions only); identical bytes over two runs | `7af98f4df` |
| `coverage.json` | regenerated at the merge (conflict) and after the new cases | n/a |

No engine vector changed in this job.

## Carried findings

- CU-C1 `34c143b6-…/R1` (minor): fixed in `6cf349a12` and `40bef3b47`. Before the fix emcc 4.0.0
  reported `old sanity: 4.0.0|/opt/emsdk/upstream/bin`, so earlier payloads were built with the
  image's LLVM; the payload here is the first built with emsdk 4's own toolchain.
- CU-C6 `67634bb0-…/F1` (minor): fixed in `1820918b6`, case in `aafad823e` (CU-C7-D3).
- CU-C6 `…/F2` (minor): case in `aafad823e`. It reaches every commands line CU-C6-D9 listed:
  `controller.ts` 352, `availability.ts` 96, 105, 107, 113, 148, 152, 194 (CU-C7-D4).
- CU-C6 `…/F4` (minor): fixed in `643e73c56`. A case-insensitive search for the messaging tool's name over
  `docs/research/upstream-catchup-2026-11-12` and the ledger finds nothing else.
- CU-C5 `3fb17273-…/R6` (advisory, optional): not done (CU-C7-D14).

## Host branches

`research/catchup-2026-11-12-plugins` is fetched into the host repository from a bundle of the
container branch. `research/multi-platform-rust-core-impl` was fast-forwarded
`e4c17f48d..a7c75f5ad` with `git fetch . research/catchup-2026-11-12-impl:research/multi-platform-rust-core-impl`
(CU-C7-D16). `research/multi-platform-rust-core-impl-plugins` isn't moved; the command for the
owner is in the ledger.

## Known gaps

- `core-wasm` and `epdf-web` have no same-day start-tip run (CU-C7-D11).
- `packages/engine/runtime/npm/wasm32/wasm-inline.js` keeps its earlier build: `build-target.sh`
  rebuilds the payload but not the browser-inline copy, which only the runtime package's `build`
  script writes. `core-wasm`'s tests load `embedpdf.node.*` and `embedpdf.wasm`, both rebuilt.
- The Swift harness is new; its counts aren't comparable with 2026-11-07's 293 of 325 (CU-C7-D8).
- The wrappers don't surface `wheel_by` or `scrollChaining` in their own APIs (CU-C7-D10).

## Evidence

Logs are in `/tmp/cu1112/logs/CU-C7/evidence/` on the host, with a `SHA256SUMS` manifest (63 files,
`shasum -a 256 -c SHA256SUMS` passes). Paths below are relative to that directory.

| Check | Head | Command | Exit | Log | SHA-256 |
|---|---|---|---|---|---|
| full suite, cargo | `4fd6375ee` | `suite.sh /workspaces/wt/c7 /workspaces/results/CU-C7-final` (cargo) | 0 | `final-cargo.log` | `92b593da60f4921691dcc78305cc47996095afc6212ce8bfa711b2fc7e161942` |
| engine `:check` | `4fd6375ee` | `suite.sh` (engconf) | 0, 35 ok | `final-engconf.log` | `1398a775ab5d532731d1fdcde05024e0ed6beed76baf2246b2d274622eda76a5` |
| plugin `vectors:check` | `4fd6375ee` | `suite.sh` (plugconf) | 0 | `final-plugconf-vectors-check.log` | `e9e2bf3b2f4000f75513cc1b9fc42425c22557e69052506c406b1da3bb09e134` |
| plugin `coverage:check` | `4fd6375ee` | `suite.sh` (plugconf) | 0 | `final-plugconf-coverage-check.log` | `4f4870a6b64e55a50aeb035d32e6f02089dcb415a0fe781a336deac6a3cbdb28` |
| plugin `test:harness` | `4fd6375ee` | `suite.sh` (plugconf) | 0 | `final-plugconf-test-harness.log` | `cab76414a6ea114f7c2d39eea7c2040a8f8f5a4a8c7e7e8c781c7ebadabc827a` |
| plugin `typecheck` | `4fd6375ee` | `suite.sh` (plugconf) | 0 | `final-plugconf-typecheck.log` | `b79358398d69c66d2bca3c1418c255992c7b1886dbf69f083223ee4eccb79dc4` |
| TS package tests | `4fd6375ee` | `turbo run test --continue --concurrency=6 --filter='./packages/**'` | 0 | `final-ts.log` | `7f29d2f033f5840b4a7c57a10b4f7ac897e10ef360af686b8feb2d0232df200a` |
| evidence tests | `4fd6375ee` | `evrun.sh final` | 0, 679/0 | `final-evidence-summary.txt` | `1821e81fda6004dd3524ded0de7e885cbdc9d638af0b3d2b4c0d8fd0ea4767cd` |
| suite at the merge | `87b752368` | `suite.sh` + `evrun.sh merge` | cargo 101 (1 failed) | `CU-C7-suite1.txt`, `merge-cargo.failed`, `merge-evidence-summary.txt` | `884ed448…`, `f4e4ec57…`, `b26c9388…` |
| commands crate, `uniffi` | `a11b18eb3` tree | `cargo test -p epdf-plugin-commands --features uniffi` | 0 | `CU-C7-fix2-commands.log` | `2721ee7259775980c11ca8afed4043b773a016c8d73c99a7f03c2a1eeece7e32` |
| coverage write and plugin checks | `a11b18eb3` tree | `pnpm run coverage` and the four checks | all 0 | `CU-C7-plugchecks.txt` | `bb934c653d8392711bf17aa6c8c666f908d69948e5c363ff7582f3cda5d45b83` |
| chain re-pin | `a11b18eb3` tree | `chainpins.py` (root `/workspaces/wt/c7`) | 0, 1 pin | `CU-C7-chainpins.log` | `cb6beea5f9d2db6cc3e0ae53326dfb8ea65a804705e70e4c41a9f3f10e5e3beb` |
| Rust-core payload | `40bef3b47` | `build-rust-payload.sh /workspaces/wt/c7` | 0 | `CU-C7-payload.log` | `e94028a0c343c95ca68257e2a94ae540e47551c60b023133d9556e904bd8d106` |
| `core-wasm` vitest | `40bef3b47` | `npx vitest run` | 0, 102/102 | `CU-C7-core-wasm-vitest.log` | `87b05c6715225fc5a336a5cb4684ee781a98dff39ee76d401ac7a13285c38c81` |
| `core-wasm` `test:gc` | `40bef3b47` | `npx vitest run -c vitest.gc.config.ts` | 0, 13/13 | `CU-C7-core-wasm-gc.log` | `af4da6e49b16708982abffc409a292ba97cff68a8cea18b3160ed00d7510369e` |
| `epdf-web` wasm tests | `40bef3b47` | `cargo test -p epdf-web --target wasm32-unknown-unknown` | 0 | `CU-C7-epdf-web.log` | `6c233ed8de4b6f27afa209ea7de9c2084b624cbaf4ec676bc65827c4719e3700` |
| platforms, head | `40bef3b47` (+ test edits of `791a8ee29`) | `platforms.sh /workspaces/wt/c7 head` | see summary | `plat-head/summary.txt` | `d815b7f9baad47200315c695b39786be3cca2e5eb73715b3d653ed4816708366` |
| platforms, start | `7990560d6` | `platforms.sh /workspaces/wt/c7-base base <head manifest>` | see summary | `plat-base/summary.txt` | `381ef27421620b791496b499e9879c8fb99d6f369633de6fdb70c4a2cd0ed30e` |
| Swift on Linux, head after fixes | `791a8ee29` tree | `swift-linux.sh /workspaces/wt/c7 …` | test 1 (38 failing, as base) | `swift-head-final/test.log` | `016b02e3cd3f70826f617237bb71aed86f81bcc88064486260c4c0e88deda8c9` |
| Swift on Linux, start | `7990560d6` | `swift-linux.sh` via `platforms.sh` | test 1 (38 failing) | `plat-base/swift-test.log` | `5c78f2f1779e4be1a41a2939cea9b9fd86f7eff8c10481253b52f9d2c5d4f1dc` |

Environment: container
`embed-pdf-catchup1112`, image `sha256:30b97dec3bd8d007b9197ab8fc0027f3b62ea3bd25390a5f516b4cbab085df84`,
linux-arm64, Rust 1.98.0, Swift 6.3.1, .NET SDK 10.0.403, JDK 21.0.13, kotlinc 2.2.21, emsdk 4.0.0,
`wasm-bindgen-cli` 0.2.127. Every heavy step ran through `heavy-build.sh` with owner `CU-C7-<label>`.
Unrun: Mac and Android emulator steps (D-06), the other four `.NET` RIDs.

## Cleanup

Before the report: deleted `/workspaces/wt/c7/target` (12 GB) and `/workspaces/wt/c7-base/target`
(2.6 GB), removed worktree `/workspaces/wt/c7-base` (clean, detached at `7990560d6`), and deleted
the bundles `CU-C1-runtime`, `CU-C2-in-c1`, `CU-C2`, `CU-C3`, `CU-C4`, `CU-C5`, `CU-C6-from-c5`,
`CU-C7-inputs` and `viewer-c1` from `/workspaces/bundles` after checking every commit they carried is
on the host (runtime `17d8073d4` on `catchup/2026-11-12-runtime`). No other job worktree remained
under `/workspaces/wt`. After the branch reaches the host, `/workspaces/wt/c7` and the CU-C7 bundle
go too; the work record lists that step. The container `embed-pdf-catchup1112` and volume
`embedpdf-catchup1112` stay for the manager (D-07). Volume use went from 230 GB to 214 GB; the
host's free space read 534 GiB at the start and 516 GiB after (Docker's disk image doesn't shrink
when files inside it are deleted).

FEEDBACK: WORKS heavy-build plus `work_wait` carried 14 heavy steps (prep, suites, captures, payload,
platform runs) without a single model poll. BROKEN the 2026-11-07 Swift-on-Linux harness lived only
on a deleted volume, so this job rebuilt it from nothing; committing run scripts with the report, as
`scripts/swift-linux/` now is, should be part of every catch-up job's brief.
