# CU-C5: Rust port, annotation and form family

`research/catchup-2026-11-12-c5` ports upstream `074a190c9..ed064e0c8` into the annotation and form
family. All 10 cargo failures CU-C3 assigned here pass, and so do the 16 measurement and stamp
evidence tests it deferred. Annotation duplicate and form field duplicate are ported, with new
vector cases. `cargo test --workspace` goes from 3,269 passed and 23 failed (CU-C3, `b8ade7fe8`) to
3,281 passed and 14 failed at `1db5ca7c6`. Of the 14, 12 are CU-C6's and 2 are CU-C4's. Evidence
tests: 679 of 679. The first review's findings are fixed below. Decisions are in [`../decisions/CU-C5.md`](../decisions/CU-C5.md).

## Commits

| Commit | Summary |
|---|---|
| `19e2643be` | annotation-core: ink drawn under the pen as its points (`59864ae01`); `replaceDefaults` (`f0bb7b047`) |
| `c00f1d345` | kernel: page list read again on `stream.desynced` (`a4805a492`) |
| `2ef91a545` | plugin-annotation: tool entries applied in order, the remover puts back what it replaced (`f0bb7b047`); `whenRemeasurable`/`stageRemeasure` (`8bc6546e5`) |
| `bbd9bebf8` | kernel: a staged change an engine verb makes (`b3f1c447e`) |
| `87726eae7` | plugin-annotation, plugin-form: imports staged as one change (`b3f1c447e`) |
| `51a07da98` | kernel: `pages.setScale` prediction, `scaleBackOf`, `viewportsWithScale` (`c7f8d75ff`) |
| `dd57af5a4` | plugin-measurement: scales from the layout, a scale change as one change (`8bc6546e5`, `c7f8d75ff`) |
| `faa4cce1f`, `e9ea736b1` | measurement `reduce-page-scale` covers text, and its regeneration (finding R2) |
| `5dfee50b4` | measurement and stamp evidence re-authored; catch-up accounting layer |
| `850f4d09e`, `26227a437` | annotation-tools case for a registration over a taken id, and its regeneration |
| `ef5b91e19`, `e5e23d12b` | annotation-crud duplicate cases, and their regeneration |
| `ece09927b` | kernel: a refused copy refuses changes naming its copies (`1481e4556`, `a390171cc`) |
| `20170c360` | annotation-core: `planAnnotationCopies`, `isMovable` (`1481e4556`) |
| `f74c06c6d` | plugin-annotation: duplicate (`1481e4556`, `9eead5531`, `db8b5ffb1`, `f8ac5a351`) |
| `e4e01de11` | `coverage.json` regenerated |
| `6c5a3b51d`, `10abe03c4` | form-design duplicate case, and its regeneration |
| `d11e3e250` | plugin-form: `duplicate`, `duplicateWidgets`, `forms.copy`/`forms.addWidget` prediction (`a390171cc`) |
| `739bd3859`, `04264a481` | the duplicate case's radio rows as radio; form-design and `coverage.json` regenerated |
| `af56c7bb4` | evidence accounts the four new cases; chained pins moved |
| `ecbee4317` | clippy warnings in the two copy plans |
| `54e86be44` | report and decisions, first submission |
| `d41fddc17`, `990cdaf82` | review R1: six cases for the new lines no case reached, and their regeneration with `coverage.json` |
| `41faa9517` | review R1 and R4: evidence accounts the new cases; the scale answers' event checks restored |
| `2025ca812` | review R1 and R2: D11 lists each line left without a case; no message IDs |
| `1db5ca7c6` | typecheck fix in the restored event checks |
| this commit | report |

The four kernel commits touch only `crates/epdf-kernel` (`changes.rs`, `kernel.rs`,
`page_predict.rs`, `page_view.rs`). CU-C6 has cherry-picked all four.

## Tests

All runs are in container `embed-pdf-catchup1112`, worktree `/workspaces/wt/c5`, with the brief's
Cargo settings.

| Check | Start (`0879a6d22`, CU-C3's run at `b8ade7fe8`) | Now (`1db5ca7c6`) |
|---|---|---|
| `cargo test --workspace --no-fail-fast` | 3,269 passed, 23 failed, 1 ignored | 3,281 passed, 14 failed, 1 ignored |
| CU-C5's 10 assigned failures | 10 failing | 0 |
| plugin evidence tests | 661 of 677 | 679 of 679 |
| `vectors:check`, `coverage:check`, `test:harness`, `typecheck` | pass | pass |
| `cargo build --workspace --tests` | pass | pass |
| `cargo fmt --check` and `cargo clippy --tests` on the 5 changed crates | | pass, no warnings |
| `epdf-plugin-measurement --features uniffi --test ffi` | | 12 of 12 |

The 14 failing now, with their category:

| Test | Crate | Category |
|---|---|---|
| `attachment_matches_the_typescript` | attachment | other job's (CU-C6, `scrollChaining`) |
| `capability_matches`, `change_hooks_match`, `feeds_match`, `motion_matches`, `page_turns_match`, `pure_modules_match` | stage | other job's (CU-C6) |
| `scenarios_match_the_typescript` | link | other job's (CU-C6) |
| `outline_matches_the_typescript` | outline | other job's (CU-C6) |
| `stage_integration_matches_the_typescript` | search | other job's (CU-C6) |
| `selection_matches_the_typescript` | selection | other job's (CU-C6, `9b5125408`) |
| `the_plugin_matches_the_typescript` | commands | other job's (CU-C6, `7af98f4df`) |
| `a_measurement_scale_persists_through_the_native_engine`, `a_foreign_scale_is_read_and_kept_through_the_native_engine` | epdf-viewer-host | other job's (CU-C4): `epdf-engine` refuses `pages.setScale` and has no `PageLayout.viewports` (CU-C5-D10) |

`actions` `triggers_match_the_typescript`, on CU-C3's list for CU-C6, now passes through
`c00f1d345`.

## Vectors regenerated

Each run used `TZ=UTC pnpm run vectors -- --only <prefix>`. Each regenerated file was compared case
by case, by name, against its previous commit (`jdiff.py` compares cases by position, so an inserted
case shifts its output).

| File | Change | Upstream |
|---|---|---|
| `plugins/measurement-functions.json` | `reduce-page-scale` covers text only; `jdiff.py` reports no change | `8bc6546e5` (finding R2) |
| `plugins/annotation-tools.json` | added `register-over-a-tool-and-take-it-back` | `f0bb7b047` |
| `plugins/annotation-crud.json` | added `duplicate-copies-as-one-change`, `selection-duplicate-lands-beside` | `1481e4556` and follow-ups |
| `plugins/form-design.json` | added `duplicate-fields-and-widgets`, `duplicate-widgets-by-the-designers-rule`, `duplicate-widgets-with-nothing-to-copy-or-refused`, `selection-duplicate-hands-widgets-to-the-form` | `a390171cc`, `1481e4556` |
| `plugins/annotation-crud.json` (second) | added `import-staged-and-copy-answers` | `b3f1c447e`, `1481e4556` |
| `plugins/annotation-tools.json` (second) | `register-over-a-tool-and-take-it-back` gains two registrations with `afterCreate` | `f0bb7b047` |
| `plugins/measurement.json` | added `default-scale-change-retells-pages` | `8bc6546e5` |
| `coverage.json` | statements 72776/77093, branches 14318/15672 | the cases above |

Each new case's trace was read against its covers text.

## New upstream behaviour without a case

The cases above reach every new line a scenario can reach. CU-C5-D11 lists the rest with the
reason for each:

- measurement's no-service branches and its aborted-signal refusal;
- the copy picture source in annotation `read/render.ts`;
- `opOfStated`'s copy branch;
- an unreadable copy selected once the engine answers;
- a form copy prediction whose plan throws;
- an `addWidget` with no object number;
- a radio row with no `rect`;
- the re-indented `writeFailed` branches.

## Public API changes for the bindings (CU-C7)

- `epdf-plugin-annotation`: `remeasure_page` is gone. `when_remeasurable()` and
  `stage_remeasure(page, scale) -> Result<Value, Error>` replace it.
- `epdf-plugin-annotation` new members: `duplicate`, `can_duplicate`, `is_movable`,
  `selection_duplicate`, `selection_can_duplicate`, `DuplicateOptions`.
- `AnnotationLender` gains `copy_ops` and `may_copy`.
- `epdf-plugin-form` new members: `duplicate`, `duplicate_widgets`.
- `epdf-plugin-measurement`:
  - removed: `page_scale_of`, `PageScaleInputs`, `record_load_error`, `clear_load_error` and
    `MeasurementState::load_errors`;
  - `local_viewports` now holds `Rc<Value>`;
  - `ensure_loaded` no longer reads its options.
- `epdf-kernel`: `StagedChange` gains `send` and `op_id`, plus `StagedChange::verb`. New public
  functions `viewports_with_scale` and `scale_back_of`.
- The uniffi wrappers gained no members for the new verbs; CU-C7 decides whether to expose them.

## Decisions

CU-C5-D1 to D12, in [`../decisions/CU-C5.md`](../decisions/CU-C5.md):

| ID | Decision |
|---|---|
| D1 | kernel changes in their own commits |
| D2 | the verb's `send` without a capability handle |
| D3 | viewports told again by value |
| D4 | scale memo on the page list |
| D5 | measurement's Rust-only tests rewritten |
| D6 | catch-up accounting layer for B3 |
| D7 | evidence outcomes follow upstream |
| D8 | form duplicate ported here (manager decision) |
| D9 | duplicate fixtures answer by the plugin's rule |
| D10 | viewer-host failures are CU-C4's |
| D11 | the cases for the new lines, and the lines left without one |
| D12 | new cases listed in the live evidence, pins moved |

## Evidence

Base: `0879a6d225d16fbfff92fcc20e4140ea2faf60e3`. Tested head:
`1db5ca7c619bd7138b6e9821385629baae3bd611`. The commits after it change only this report.

Logs are in `/tmp/cu1112/logs/CU-C5/` on the host, all hashed in its `SHA256SUMS`. The container
copies are in `/workspaces/logs/` and `/workspaces/results/`. Hashes are sha256, first 8 hex digits
shown. The finishing run records `git status --porcelain` before and after its steps: 0 entries
both times.

| Check | Command | Exit | Log | sha256 |
|---|---|---|---|---|
| Prep | `heavy-build.sh CU-C5-prep -- docker exec … prep.sh /workspaces/wt/c5` | 0 | `logs/CU-C5-prep.log` | `a45af97c` |
| Mid suite at `dd57af5a4` | `suite.sh … cargo` | 101 (14 failed) | `results/CU-C5-mid/cargo.summary` | `374df976` |
| Evidence before | `node --test test/measurement.test.ts`, `stamp-placement.test.ts` | 1, 1 | `logs/CU-C5-ev-meas-before.log`, `logs/CU-C5-ev-stamp-before.log` | `57b2b04e`, `8aa34e97` |
| Pin moves | `chainpins.py` (CU-C3's, root `/workspaces/wt/c5`) | 0, 0 | `logs/CU-C5-chainpins.log`, `logs/CU-C5-chainpins-r1.log` | `fcae5ca7`, `0fe0d04e` |
| Finishing run | `heavy-build.sh CU-C5-final4 -- … CU-C5-final4.sh` | 0 (every step below) | `logs/CU-C5-final4.txt`, script `logs/CU-C5-final4.sh` | `cb939942`, `f076dc1f` |
| `coverage` | `pnpm -s run coverage` | 0 | `logs/CU-C5-final4-coverage.log` | `be29740a` |
| `vectors:check` | `pnpm -s run vectors:check` | 0 | `logs/CU-C5-final4-plug-vectors:check.log` | `3c167d99` |
| `coverage:check` | `pnpm -s run coverage:check` | 0 | `logs/CU-C5-final4-plug-coverage:check.log` | `5f6ba09a` |
| `test:harness` | `pnpm -s run test:harness` | 0 | `logs/CU-C5-final4-plug-test:harness.log` | `a4711d04` |
| `typecheck` | `pnpm -s run typecheck` | 0 | `logs/CU-C5-final4-plug-typecheck.log` | `3d7f3461` |
| fmt | `cargo fmt -p … --check` (5 crates) | 0 | `logs/CU-C5-final4-fmt.log` | `e3b0c442` (empty) |
| clippy | `cargo clippy -p … --tests` (5 crates) | 0 | `logs/CU-C5-final4-clippy.log` | `a11a1de8` |
| Build | `cargo build --workspace --tests` | 0 | `logs/CU-C5-final4-build.log` | `dda7980e` |
| Cargo | `suite.sh … cargo` | script 0; cargo 101 (3281 passed, 14 failed, 1 ignored) | `results/CU-C5-final4/cargo.log`, `cargo.summary`, `cargo.failed` | `15d7eaf3`, `cdb218c5`, `aedaaa8b` |
| Measurement ffi | `cargo test -p epdf-plugin-measurement --features uniffi --test ffi` | 0 | `logs/CU-C5-final4-measffi.log` | `e6771cfe` |
| Evidence | `CU-C5-evrun.sh final4` (26 files, 679 tests) | 0 | `logs/CU-C5-ev-final4/summary.txt` | `1821e81f` |

The 5 crates are `epdf-kernel`, `epdf-annotation-core`, `epdf-plugin-annotation`,
`epdf-plugin-form` and `epdf-plugin-measurement`. `cargo.failed` is byte-identical to the earlier
runs' list: 12 failures in CU-C6's kernel store and mirror, 2 in `epdf-viewer-host` (CU-C5-D10).

## Known gaps

- The 14 cargo failures above are CU-C6's and CU-C4's.
- The coverage lines in CU-C5-D11 have no case.
- The form plugin's JSON `planFormCopies` and `epdf-engine`'s typed one are two ports of one rule
  (CU-C5-D8).
- The uniffi wrappers don't expose `duplicate`, `duplicate_widgets` or annotation `duplicate`
  (CU-C7).

## Remaining findings

1 owned on arrival:

- `019a6653-693d-4d30-9c72-4424baa54cba/R2` (advisory, measurement `reduce-page-scale` covers
  text): fixed in `faa4cce1f`, regenerated in `e9ea736b1`. The fix awaits review.

The first review's R1 to R5 are fixed: R1 by the cases in `d41fddc17` and `990cdaf82` and the
table in CU-C5-D11, R2 by `2025ca812`, R4 by `41faa9517`, R5 by the porcelain lines in the
finishing run, and R3 by the cleanup below.

## Cleanup

Runs once the branch is on the host; the commit after this one lists what was removed. It removes
the container worktree `/workspaces/wt/c5` and its `target/` by exact path, and the scratch files
`/tmp/show.js` and `/tmp/chainpins-c5.py` in the container. The container, its volume and
`/workspaces/logs/CU-C5-*` stay for CU-C7 and the manager.

FEEDBACK: BROKEN The sync from a host checkout to the container copied every file changed against
the host's HEAD, so a stale host copy overwrote a vector freshly regenerated in the container. A
Rust replay then ran against the old file until a later check caught it. Suggest a two-way sync
helper that refuses to overwrite a container file newer than the host's. WORKS: `heavy-build.sh`
ran each finishing run's 11 steps end to end with no failure of its own.
