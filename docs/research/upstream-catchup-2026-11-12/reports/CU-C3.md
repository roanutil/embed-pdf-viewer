# CU-C3: plugins merge, plugin generator, all vectors

`research/catchup-2026-11-12-plugins` merges the impl catch-up (`a7c75f5ad`) into plugins
`7990560d6`, which is still the host's plugins tip. The engine vectors regenerate with one change,
in `wire/requests.json`. The plugin-conformance generator follows upstream again (7 type errors to
0) and every plugin vector is regenerated, deterministically, with each change traced to an upstream
commit. `cargo test --workspace` goes from 3,292 passed and 0 failed to 3,269 passed and 23 failed:
all 23 are plugin conformance replays of the new vectors, none in `epdf-engine`, split between CU-C5
(10) and CU-C6 (13). No Rust changed. Decisions are in [`../decisions/CU-C3.md`](../decisions/CU-C3.md).

## Commits

| Commit | Summary |
|---|---|
| `3aa47e708` | Merge `research/catchup-2026-11-12-impl` into plugins |
| `1a4f373eb` | Engine `wire` request vectors regenerated at `ed064e0c8` |
| `c74ec3e9d` | Plugin-conformance generator follows upstream (typecheck 7 errors to 0) |
| `3b8cf99eb` | Every plugin vector regenerated, and `coverage.json` |
| `b8ade7fe8` | Evidence archives re-baselined (661 of 677 evidence tests pass) |
| this commit | Report, decisions, cargo failures, coverage gaps, helper scripts, ledger |

## Merge

`git merge --no-ff --no-commit research/catchup-2026-11-12-impl` on `7990560d6`; the merge base is
`10dc98b0a`. Two conflicts:

| Path | Resolution |
|---|---|
| `packages/engine/runtime/runtime-src` | `17d8073d49bab5c29d956568345652bf893b459c`, CU-C1's runtime merge |
| `docs/research/upstream-catchup-2026-11-07/AGENT-BRIEF.md` | plugins' copy (CU-C3-D1) |

Besides those, only `.gitignore` and `multiplatform-upstream-catchup-2026-11-07.md` changed on
both sides. The ledger merged to plugins' text; `.gitignore` keeps plugins' copy (CU-C3-D2).
Plugins changed no file under `packages/**` that upstream changed in the range. The workspace
builds on the merge with no Rust change (`cargo build --workspace --tests`, exit 0).

## Engine vectors

`regen-engine.sh` ran twice on `3aa47e708`: all 33 groups wrote, and the sha256 lists over
`tooling/engine-conformance/vectors` are identical (`deterministic=yes`). The generators typecheck.
One file changed:

| File | Change | Upstream |
|---|---|---|
| `vectors/wire/requests.json` | `document.applyChanges` gains optional `limits: BundleLimits` | `5496eb084` |

Unlike impl, `wire` and `operations` regenerate and check on plugins: all 34 engine `:check`
scripts pass before and after. That closes CU-C2's R1 for the wire group. The rest of R1 stands: no
engine case exercises the new engine behaviour, so CU-C4 has to add cases for it:

| Behaviour | Upstream |
|---|---|
| Raw viewports on `PageLayout.viewports` (no fixture has `/VP`) | `726ec2dd6`, `c7f8d75ff` |
| `pages.setScale` as a change op, its undo and `viewportsVersion` | `c7f8d75ff`, `65b0f137e` |
| `annotations.copy` and `forms.copy`, held to the bundle limits | `1481e4556`, `a390171cc`, `5496eb084`, `2612ae1e1` |
| Rename through an annotation update (`patch.nm`) | `756f8e56b` |
| Change kinds with the version they arrived in | `a4805a492` |

## Plugin generator and vectors

After the merge `tsc --noEmit` reported 7 errors, and the generator stopped at
`plugins/measurement.json` with "unanswered engine calls". `c74ec3e9d` fixes them (CU-C3-D3, D4):
the fake engine lists `pages.setScale`, `annotations.copy` and `forms.copy`; the measurement suite
is rewritten for scales on the page layout and a scale change as one `doc.apply` change; the
`annotation-api` remeasure cases use `stageRemeasure`; one stamp-tool case and the commands covers
text follow `f0bb7b047` and `7af98f4df`. Every changed case's trace was read against its covers text.

`TZ=UTC pnpm run vectors` twice: identical sha256 lists over `vectors/`. `vectors:check` (119 files
current and replayed), `typecheck` (0 errors), `test:harness` (14 of 14) and `coverage:check`
pass. Each changed group, reviewed with `jdiff.py` and a per-case diff:

| Vector files | Upstream |
|---|---|
| `stage-capability`, `stage-events`, `stage-feeds`, `stage-motion`, `stage-pure`, `stage-turns`, `search-stage`, `link`, `outline`, `attachment`: `scrollChaining` | `13305a14b` |
| `core/annotation-update`, `core/annotation-view`, `annotation-tools`: `drawing` on drafts, an ink stroke under the pen is its points | `59864ae01` |
| `annotation-hydration`, `form-fill`, `signature`, `actions-triggers`: `stream.desynced` reads `pages.list` | `a4805a492` |
| `form-fill`: `forms.import` carries `opId` | `b3f1c447e` |
| `annotation-pure`, `annotation-tools`: tool entries apply in order | `f0bb7b047` |
| `commands/plugin`: derivations asked once per store change | `7af98f4df` |
| `selection`: the page mirror's cell holds a version | `9b5125408` |
| `annotation-api`, `measurement`, `measurement-functions` | `8bc6546e5`, `c7f8d75ff` |
| `coverage.json` | regenerated with `pnpm run coverage` |

## Evidence archives

`node --test` over `test/*.test.ts` and `test/*.test.mjs`: 17 of 26 files failed after the
regeneration, 2 fail now, 661 of 677 tests pass. `b8ade7fe8` re-captures the c2 to c11 archives with
their capture scripts, re-pins source rows whose owner changed upstream, and moves every chained
integrity, preservation and family pin (CU-C3-D5). The 16 failures left are the measurement B3 slice:
`measurement.test.ts` (15) and `stamp-placement`'s aggregate (1), left to CU-C5 (CU-C3-D6).

## Coverage gaps

[`../results/CU-C3-coverage-gaps.md`](../results/CU-C3-coverage-gaps.md) lists, per package, the
lines upstream added that no plugin vector reaches, with their commits. CU-C5 gets most of them:
annotation duplicate and form field duplicate (`1481e4556`, `a390171cc` and follow-ups) have no case
at all. CU-C6 gets stage wheel and coasting (`13305a14b`, `138c54fe7`), command availability
(`7af98f4df`), and a few kernel lines. No cases were added (CU-C3-D7).

## Tests

All in container `embed-pdf-catchup1112`, `suite.sh` with the brief's Cargo settings.

| Check | Start `7990560d6` | Now `b8ade7fe8` |
|---|---|---|
| `cargo test --workspace --no-fail-fast` | 3,292 passed, 0 failed, 1 ignored | 3,269 passed, 23 failed, 1 ignored |
| engine `:check` scripts | 34 of 34 | 34 of 34 |
| plugin `vectors:check`, `coverage:check`, `test:harness`, `typecheck` | all pass | all pass |
| plugin evidence tests | not run | 661 of 677 (2 files fail, CU-C3-D6) |
| TS `turbo run test` over `packages/**` | 98 of 98 | 98 of 98 |

The 23 failures, by crate, cause and port job, are in
[`../results/CU-C3-cargo-failures.md`](../results/CU-C3-cargo-failures.md): CU-C4 none, CU-C5 10,
CU-C6 13.

## CU-C2 findings routed here

| Finding | Disposition |
|---|---|
| `47eb2091-…/R1` | wire group regenerated and checked on plugins; the new engine behaviour above still needs cases (CU-C4) |
| `47eb2091-…/R2` | ledger's impl TS row now reads 96 of 98, citing CU-C2-D7 (CU-C3-D8) |
| `47eb2091-…/R3` | ledger names the 5 font kinds `operations:check` reported on impl (CU-C3-D8) |

## Evidence

Base `7990560d6634298ae6767d9e30626102e124e981` and `a7c75f5adf101c096cba894074991c989a9d4ffc`;
merge `3aa47e7081e38931532516721a2a606d1cdae8b6`; tested head
`b8ade7fe8bef1bc3c1120524f7ab5f9403f399f7`; the head is the commit that adds this file. Logs are
in `/tmp/cu1112/logs/CU-C3/` on the host, all hashed in its `SHA256SUMS`; the container copies are
in `/workspaces/logs/` and `/workspaces/results/`. Hashes are sha256, first 8 hex digits shown.

| Check | Command | Exit | Log | sha256 |
|---|---|---|---|---|
| Baseline prep and suite | `heavy-build.sh CU-C3-baseline -- docker exec … bash /workspaces/logs/CU-C3-baseline.sh` | 0 | `CU-C3-baseline.log`, `CU-C3-suite-base.log` | `0fc7bd06`, `7b32003c` |
| Baseline cargo | `suite.sh … cargo` | 0 | `results/CU-C3-base/cargo.log`, `cargo.summary` | `5fec9c55`, `2e9e1041` |
| Baseline TS | `suite.sh … ts` | 0 | `results/CU-C3-base/ts.log` | `4f39a40a` |
| Merge prep, typechecks, build, engine regen x2 | `heavy-build.sh CU-C3-merge1 -- … CU-C3-merge1.sh` | 0 | `CU-C3-merge1.log` | `ea63ddf3` |
| Engine generator typecheck | `pnpm -s run typecheck` (engine-conformance) | 0 | `CU-C3-engconf-typecheck-merge.log` | `39288f5c` |
| Plugin typecheck after the merge | `pnpm -s run typecheck` (plugin-conformance) | 2 (7 errors) | `CU-C3-plugconf-typecheck-merge.log` | `bc4ff2a3` |
| Workspace build | `cargo build --workspace --tests` | 0 | `CU-C3-cargo-build-merge.log` | `5c414233` |
| Engine regen, runs 1 and 2 | `regen-engine.sh /workspaces/wt/plugins` | 0 | `CU-C3-engregen-merge-run{1,2}.log`, `.sha` | logs `c82c35fe`, sha lists `0e3e9ec0` (both runs) |
| Plugin regen x2 and checks | `heavy-build.sh CU-C3-plugregen -- … CU-C3-plugregen.sh` | 0 | `CU-C3-plugregen.log` | `da111c27` |
| Plugin regen, runs 1 and 2 | `TZ=UTC pnpm -s run vectors` | 0 | `CU-C3-plugregen-run{1,2}.log`, `.sha` | logs `0213f504`, sha lists `a48403ef` (both runs) |
| `vectors:check` | `pnpm -s run vectors:check` | 0 | `CU-C3-plug-vectors:check.log` | `5e68b506` |
| `typecheck` | `pnpm -s run typecheck` | 0 | `CU-C3-plug-typecheck.log` | `39288f5c` |
| `test:harness` | `pnpm -s run test:harness` | 0 | `CU-C3-plug-test:harness.log` | `eff6a62a` |
| `coverage.json` rewrite (`coverage:check` then passes in the after suite) | `pnpm -s run coverage` | 0 | `CU-C3-plug-coverage-write.log` | `077daf2e` |
| Evidence re-capture | `heavy-build.sh CU-C3-recapture -- … CU-C3-recapture.sh` | 0 | `CU-C3-recapture.log` | `6f88aa8c` |
| Evidence tests, final | `heavy-build.sh CU-C3-evtests-final -- … CU-C3-evrun.sh final` | 0 (script) | `CU-C3-evtests-final.log` | `2ea88500` |
| After suite | `heavy-build.sh CU-C3-after -- … CU-C3-after.sh` | 0 (script) | `CU-C3-after.log`, `CU-C3-suite-after.log` | `22967abc`, `4bfd1aa3` |
| After cargo | `suite.sh … cargo` | 101 | `results/CU-C3-after/cargo.log`, `cargo.failed` | `6818406a`, `4858f9e6` |
| After engine and plugin checks | `suite.sh … engconf plugconf` | 0 | `results/CU-C3-after/engconf.all`, `plugconf.all` | `507d337b`, `0778cdb1` |
| After TS | `suite.sh … ts` | 0 | `results/CU-C3-after/ts.log` | `d3134bd2` |
| Coverage gaps | `python3 scripts/cu-c3/gaps.py` | 0 | `CU-C3-coverage-gaps.txt` | `4240412b` |

The helper scripts used for the generator review and the re-baseline are committed in
`../scripts/cu-c3/`: `casediff.py`, `casepatch.py`, `trace.py` (per-case vector review),
`srcpins.py`, `owners.py`, `chainpins.py`, `fixrow.py` (pins), `gaps.py` (coverage).

## Known gaps

- 16 evidence tests in the measurement B3 slice fail (CU-C3-D6, owner CU-C5).
- New engine behaviour has no engine vector cases (CU-C4).
- Upstream's new plugin code has the coverage gaps listed above (CU-C5, CU-C6).
- 52 of the 98 TS tasks came from turbo's cache, before and after, since the same package inputs
  were built by CU-C2.
- `runtime-src` names `17d8073d4`, which is only in the local runtime fork until the human pushes
  it (D-04).

## Cleanup

After the branch is on the host: `/workspaces/wt/plugins/target` and the container worktree
`/workspaces/wt/plugins` are removed. The container, its volume, `/workspaces/logs/CU-C3-*` and
`/workspaces/results/CU-C3-*` stay for later jobs.

FEEDBACK: BROKEN the plugin-conformance evidence archives pin source hashes, sibling archive
hashes and test-file hashes in chains across 17 archives with no re-baseline tool in the repo, so one
upstream change to a shared file fails 6 test files, and a naive re-pin script corrupted the chain once
before it was fixed. Suggest committing a re-pin tool (like `scripts/cu-c3/chainpins.py`) beside the
archives. WORKS heavy-build plus `work_wait` ran 7 heavy steps with no model polling.
