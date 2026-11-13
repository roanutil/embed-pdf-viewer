# CU-C6: Rust port of the kernel, stage and the remaining plugins

All 13 conformance failures CU-C3 assigned to CU-C6 now pass, and nothing else in my crates broke:
the 21 crates go from 684 passed and 13 failed at `0879a6d22` to 697 passed and 0 failed. The
workspace builds with its tests, and the plugin checks pass. New upstream stage behaviour that no
vector reached (the wheel, scroll chaining, coasting) now has three generator cases. Three kernel
commits from CU-C5 port `a4805a492`, `b3f1c447e` and `c7f8d75ff`, taken unchanged on the manager's
instruction. There is no NEW DIVERGENCE and no change under `packages/**`. Decisions are in
[`../decisions/CU-C6.md`](../decisions/CU-C6.md).

## Commits

On `research/catchup-2026-11-12-c6`, base `0879a6d22` (CU-C3's tip):

| Commit | Summary |
|---|---|
| `ad34b9960` | (CU-C5 `c00f1d345`) kernel reads the page list again after `stream.desynced` (`a4805a492`) |
| `9aac78c70` | (CU-C5 `bbd9bebf8`) kernel stages a change an engine verb makes (`b3f1c447e`) |
| `b42c7b80e` | (CU-C5 `51a07da98`) kernel predicts and undoes `pages.setScale` (`c7f8d75ff`) |
| `7d9a633b5` | (CU-C5 `ece09927b`) a refused copy refuses the changes naming its copies (`1481e4556`, `a390171cc`) |
| `157724242` | generator: three wheel cases in stage-turns; the commands `derivations` covers text (R1) |
| `f0f1c06be` | vectors: stage-turns, commands, `coverage.json` regenerated |
| `25b8b9563` | kernel: store change count and `store_change_count` (`95fb3b567`, `7af98f4df`); page-mirror cell holds a write count (`9b5125408`) |
| `81d899ced` | stage-core: `item_of_page` reads a table indexed by page (`2eb93c5ca`, `0e35dc937`) |
| `861de9764` | stage: `wheel_by`, `scroll_chaining`, coasting, `leads_somewhere`, `push_began` (`13305a14b`, `138c54fe7`) |
| `61b12266b` | commands: each command asked once per store change (`7af98f4df`) |
| `d68645c6d` | stage: clippy fix in `leads_somewhere` |
| this commit | report and decisions |

## Upstream commits read

Every non-merge commit in `074a190c9..ed064e0c8` under `packages/core`, `packages/plugin` and
`packages/framework` was read. Mine to port:

| Upstream | Package | Port |
|---|---|---|
| `13305a14b`, `138c54fe7` | plugin/stage | `861de9764` (the `framework/web` binding half has no Rust counterpart) |
| `2eb93c5ca`, `0e35dc937` | core/stage | `81d899ced` |
| `9b5125408` | core/main page-mirror | `25b8b9563` |
| `95fb3b567`, `7af98f4df` | core/main store, context; plugin/commands; core/ui | `25b8b9563`, `61b12266b`; core/ui `omittedCommandsOf` has no Rust port (D6) |
| `a4805a492` | core/main kernel | CU-C5's `ad34b9960` |
| `b3f1c447e` | core/main changes | CU-C5's `9aac78c70` |
| `c7f8d75ff`, `8bc6546e5` | core/main page-view, engine-core predict | CU-C5's `b42c7b80e`; `testing.ts` (the TypeScript test context) has no Rust port |
| `memo.ts` loop (in `7af98f4df`) | core/main | performance only; the Rust memos already compare in a loop |

Not mine or nothing to port: `59864ae01`, `f0bb7b047`, `1481e4556`, `9eead5531`, `db8b5ffb1`,
`f8ac5a351`, `a390171cc`, `6fefd709b` (annotation, form, history labels: CU-C5; the history plugin's
own sources did not change), README and CHANGELOG commits (`d295732dc`, `ba6053b5b`, `5745c92f2`,
`d31d8e4ad`, `3f31ef057`, `4354d6af3`), framework-only commits (`8e1587211`, `d907f9ee4`, `52aeeb7ae`,
`f0704d14d`).

## Tests before and after

| Set | Before (`0879a6d22`) | After |
|---|---|---|
| `cargo test` on the 21 CU-C6 crates | 684 passed, 13 failed | 697 passed, 0 failed |
| `cargo build --workspace --tests` | built (CU-C3) | exit 0 |
| `clippy --tests` on the 4 changed crates | not run | clean |
| plugin `vectors:check`, `coverage:check`, `test:harness`, `typecheck` | pass (CU-C3) | pass |

The 21 crates: `epdf-kernel`, `plugin-conformance`, `epdf-geometry`, `epdf-stage-core`,
`epdf-plugin-{stage,render,search,selection,view-manager,commands,i18n,actions,attachment,history,interaction,link,metadata,outline,page-edit,shell}`,
`epdf-ui-core`. The 13 that failed, each with the commit that fixed it (category: port bug fixed):

| Test | Fixed by |
|---|---|
| `epdf-plugin-actions` `triggers_match_the_typescript` | `ad34b9960` |
| `epdf-plugin-attachment` `attachment_matches_the_typescript` | `861de9764` |
| `epdf-plugin-commands` `the_plugin_matches_the_typescript` | `61b12266b` |
| `epdf-plugin-link` `scenarios_match_the_typescript` | `861de9764` |
| `epdf-plugin-outline` `outline_matches_the_typescript` | `861de9764` |
| `epdf-plugin-search` `stage_integration_matches_the_typescript` | `861de9764` |
| `epdf-plugin-selection` `selection_matches_the_typescript` | `25b8b9563` |
| `epdf-plugin-stage`: capability, change hooks, feeds, motion, page turns, pure modules | `861de9764` |

Three Rust-only `epdf-plugin-commands` behavior tests pinned the old re-derive-per-read behaviour and
were updated (D8). Remaining failures in my crates: none.

## Vectors regenerated

`TZ=UTC pnpm run vectors -- --only plugins/commands/` and `--only plugins/stage-turns`, twice;
the sha256 lists match (`deterministic=yes`). Reviewed by per-case diff:

| File | Change | Upstream |
|---|---|---|
| `plugins/commands/plugin.json` | only the `derivations` covers string (R1) | `7af98f4df` |
| `plugins/commands/functions.json` | rewritten, byte-identical | none |
| `plugins/stage-turns.json` | three new cases; existing cases unchanged | `13305a14b`, `138c54fe7` |
| `coverage.json` | rewritten with `pnpm run coverage` | follows the new cases |

The new cases' answers were checked against the TypeScript's logic before the Rust replayed them:
a stream beginning at the end of the pages answers `false` for every wheel (a zero wheel included)
and moves nothing; one beginning with room answers `true` and stops at the end; a speed-up past 2.5
times the slowest coasting speed is a new push left to the page; mouse notches 100 ms apart never
read as a push; in paged flow the push after a turn's coasting scrolls the new page by 60 px.

## Carried finding

`019a6653-693d-4d30-9c72-4424baa54cba/R1` [minor]: fixed. The covers text now says an empty string
also means the active document (`157724242`), the regenerated `plugins/commands/plugin.json` changes
only that string (`f0f1c06be`), and the Rust treats `""` as the active document (`61b12266b`).

## Coverage of new upstream behaviour

The stage gaps CU-C3 listed (`camera/coasting.ts`, `camera/wheel.ts`, `navigation/turn.ts`) are
closed. What is left in my areas is in D9, with reasons; owner unassigned.

## For CU-C7

The stage uniffi surface grows: `scroll_chaining` on `StageSettings` and `StageSettingsPatch`,
`StageSettingKey::ScrollChaining`, and `async wheel_by(dx, dy, time_ms) -> bool` (D7). No renames.
`epdf_kernel::page_mirror::page_cell_value_copies` (doc-hidden test hook) is removed. My kernel
changes outside CU-C5's commits touch `store.rs`, `plugin.rs`, `page_mirror.rs`, `mirror.rs` and
`tests/current_contracts.rs`.

## Evidence

Base `0879a6d225d16fbfff92fcc20e4140ea2faf60e3`. Head is the commit that adds this file; the code it
reports on is `d68645c6d`. Builds ran in `embed-pdf-catchup1112`, worktree `/workspaces/wt/c6`,
on the host files synced over the base (D1), through `heavy-build.sh` with `CARGO_BUILD_JOBS=4
CARGO_INCREMENTAL=0` and no debug info. The final run's tree was `61b12266b`; `d68645c6d` only
renames a boolean in `leads_somewhere`, rechecked by `final2`. Logs are in
`/tmp/cu1112/logs/CU-C6/` on the host, hashed in its `SHA256SUMS` (first 8 hex digits shown).

| Check | Command | Exit | Log | sha256 |
|---|---|---|---|---|
| Prep | `heavy-build.sh CU-C6-prep -- docker exec … bash /workspaces/logs/CU-C6-prep.sh` (`prep.sh`) | 0 | `CU-C6-prep.log` | `09dbcd76` |
| Baseline cargo, 21 crates | `cargo test -p … --no-fail-fast` | 101 | `CU-C6-cargo-base.log` | `78c9d494` |
| Port pass 1 cargo | `CU-C6-t.sh t5 test -p … --no-fail-fast` | 101 (3 behavior tests) | `CU-C6-t5.log` | `a31ef966` |
| Generator typecheck | `pnpm -s run typecheck` | 0 | `CU-C6-r1-typecheck.log` | `044cde58` |
| Regen, runs 1 and 2 | `TZ=UTC pnpm -s run vectors -- --only …` | 0 | `CU-C6-r1-regen-run{1,2}.log`, `.sha` | logs `990974e2`, sha lists `a64d52c3` (both) |
| `coverage.json` rewrite | `pnpm -s run coverage` | 0 | `CU-C6-r1-coverage-write.log` | `548edd82` |
| Regen-step cargo | `CU-C6-t.sh r1-cargo test -p …` | 0 | `CU-C6-r1-cargo.log` | `9f5d7feb` |
| Clippy, changed crates | `cargo clippy -p epdf-kernel -p epdf-stage-core -p epdf-plugin-stage -p epdf-plugin-commands --tests` | 0 (1 warning, fixed) | `CU-C6-f-clippy.log` | `45e37a7c` |
| Workspace build | `cargo build --workspace --tests` | 0 | `CU-C6-f-build.log` | `93f800e6` |
| Final cargo, 21 crates | `CU-C6-t.sh f-test test -p … --no-fail-fast` | 0 (697 passed, 0 failed) | `CU-C6-f-test.log` | `1ddda223` |
| `vectors:check` | `pnpm -s run vectors:check` | 0 | `CU-C6-f-plug-vectors:check.log` | `794a8d22` |
| `coverage:check` | `pnpm -s run coverage:check` | 0 | `CU-C6-f-plug-coverage:check.log` | `49e13164` |
| `test:harness` | `pnpm -s run test:harness` | 0 | `CU-C6-f-plug-test:harness.log` | `a6d84f78` |
| Plugin `typecheck` | `pnpm -s run typecheck` | 0 | `CU-C6-f-plug-typecheck.log` | `044cde58` |
| Clippy after the fix | `cargo clippy -p epdf-plugin-stage --tests` | 0, no warnings | `CU-C6-f2-clippy.log` | `083727e1` |
| Stage-family cargo after the fix | `cargo test -p epdf-plugin-{stage,search,link,outline,attachment}` | 0 (112 passed) | `CU-C6-f2-test.log` | `1acf38bd` |
| Coverage gaps | `python3 -I gaps.py` (CU-C3's, root pointed at this worktree) | 0 | `CU-C6-coverage-gaps.txt` | `c62378eb` |

Runs t1 to t4 produced no test evidence: t1 and t2 stopped on a compile error since fixed, and t3 and
t4 passed the crate list as one argument (t3 compiled the workspace and filtered every test out).

## Known gaps

- The coverage gaps in D9 (core/main error paths, the TypeScript test context, commands sentences
  with i18n present).
- TS package tests and engine checks were not run: no TypeScript changed, and the engine is CU-C4's.
- The cargo runs cover my 21 crates, not the whole workspace's tests; CU-C7 runs the full suite.

## Cleanup

After this commit is on the host: `/workspaces/wt/c6/target` and the container worktree
`/workspaces/wt/c6` (by exact path) are removed. The container, its volume and
`/workspaces/logs/CU-C6-*` stay for CU-C7 and the manager.

FEEDBACK: BROKEN zsh does not split an unquoted `$A`, so `cargo test $A` with a list of `-p` flags
passed one argument and t3 "passed" with every test filtered out; suggest the shared `suite.sh`
take a crate list file and build the `-p` flags in bash. WORKS heavy-build plus `work_wait` ran
nine heavy steps with no model polling, and the manager's relay of CU-C5's kernel commits avoided a
duplicate port.
