# CU-C2: impl merge of upstream `ed064e0c8`

`research/catchup-2026-11-12-impl` merges upstream main `ed064e0c8` into impl `e4c17f48d`, with
`runtime-src` at the runtime fork merge `17d8073d4`. Only `runtime-src` conflicted. The
engine-conformance generators typecheck against the new TS without changes, and every regenerable
vector group came out byte-identical to the committed vectors, twice. The test results match the
baseline by name: cargo 550 passed and 117 failed before and after, all in `epdf-engine` on vectors
the Rust doesn't read yet (manager decision D-03). Decisions are in
[`../decisions/CU-C2.md`](../decisions/CU-C2.md).

## Commits

| Commit | Summary |
|---|---|
| `12256e882` | Merge `research/catchup-2026-11-12-c1` (CU-C1's report, decisions and scripts) |
| `2714d5957` | Merge upstream main `ed064e0c8`; `runtime-src` set to `17d8073d4` |
| this commit | CU-C2 report, decisions, cargo failure list and the 2026-11-12 ledger |

There's no generator commit and no vector commit, because neither had any content (CU-C2-D6).

## Merge

`git merge --no-ff ed064e0c8` on `12256e882`. One conflict:

| Path | Resolution |
|---|---|
| `packages/engine/runtime/runtime-src` | `17d8073d49bab5c29d956568345652bf893b459c` (CU-C1's merge of upstream runtime `aa59ce554` into fork `3c6e02297`) |

`.gitmodules` merged cleanly and keeps the fork's URL and branch. `engine-runtime-build.json` is
upstream's, release `runtime-aa59ce554…` (CU-C2-D4). Five files merged cleanly with changes from
both sides: `.agents/skills/embedpdf-conventions/SKILL.md`, `.prettierignore`, `package.json`,
`packages/engine/main/src/index.ts` and `packages/engine/services/src/worker-host/WorkerHost.ts`.
For each, the added and removed lines of `git diff ed064e0c8` after the merge are the same set as
`git diff 074a190c9 e4c17f48d`, so each carries upstream's file plus impl's own hunks and nothing
else.

Checks on `2714d5957`: `git merge-base --is-ancestor ed064e0c8 HEAD` and
`git merge-base --is-ancestor e4c17f48d HEAD` both exit 0, and `git ls-tree HEAD
packages/engine/runtime/runtime-src` prints `17d8073d4…`.

## Engine conformance

`pnpm -s run typecheck` in `tooling/engine-conformance` exits 0 at `2714d5957`. The guide's
"Expected API that no longer exists" steps had nothing to act on: no generator imports a name that
upstream renamed or removed in `074a190c9..ed064e0c8`.

`regen-engine.sh` ran twice after the `wasm32` payload was rebuilt at runtime `aa59ce554`
(CU-C2-D3). Both runs printed the same log, and the sha256 lists of all 51 files under `vectors/`
are identical (`deterministic=yes`). 30 of 31 groups ran; `wire` fails before writing (CU-C2-D5).
No vector changed, so there's nothing to trace to an upstream commit.

| Groups | Result |
|---|---|
| `vectors`, `schema`, `search`, `scope`, `security`, `render`, `charmap`, `metadata`, `annotation`, `annotation-read`, `annotation-appearance`, `attachments`, `forms`, `piece-info`, `extract`, `save`, `annotation-write`, `annotation-create`, `annotation-delete`, `annotation-update`, `annotation-move`, `actions`, `layout`, `form-write`, `form-field`, `pages-mutate`, `redaction`, `metadata-write`, `files-write`, `form-apply` | regenerated, byte-identical to `e4c17f48d` |
| `wire` | not regenerated: "expected 89 request kinds, the union has 94", as at the baseline |

The upstream engine changes in the range add behaviour that impl's cases don't exercise (raw
viewports, page scale as a change, duplicate annotations and form fields, rename through an
update, change-kind versions; commits in CU-C2-D6). That's a coverage gap for the port jobs, not a
regeneration result.

Engine `:check` scripts: 30 of 32 pass before and after. `operations:check` and `wire:check` fail
on both, for reasons older than this catch-up (CU-C2-D5).

## Tests

All in container `embed-pdf-catchup1112`, `suite.sh` with the brief's Cargo settings. The baseline
is `12256e882`, which has the same code as `e4c17f48d`.

| Check | Baseline `12256e882` | After `2714d5957` |
|---|---|---|
| `cargo build --workspace --tests` | builds | builds (exit 0) |
| `cargo test --workspace --no-fail-fast` | 550 passed, 117 failed | 550 passed, 117 failed |
| engine `:check` scripts | 30 of 32 | 30 of 32 |
| TS `turbo run test` over `packages/**` | 98 of 98 tasks | 96 of 98; the 2 pass alone (CU-C2-D7) |

The two `cargo.failed` lists have the same sha256 (`898127a7…`): the merge added and removed no
failure. Every failure is in `crates/epdf-engine/tests`. By cause:

| Cause | Count |
|---|---|
| Vector field the Rust doesn't read: `changed` 24, `p1` 13, `left` 10, `advisory` 8, `pageState` 7, `length` 6, `family` 5, `mutations` 3, `bits` 3, `text` 2, `raw` 2 | 83 |
| Vector type change: `invalid type: map` 4, `unknown field createdAt` 1 | 5 |
| Value differs from the vector (assertion) | 16 |
| Behaviour not ported or different (form value writes `NotImplemented`, `forms/effects.json` missing, text layout `loose` glyphs, vocabulary stamp names, catalog action flags and others) | 13 |

The full list with names, grouped the same way, is
[`../results/CU-C2-cargo-failures.md`](../results/CU-C2-cargo-failures.md). These are the
starting list for CU-C4 to CU-C6 on plugins.

## Evidence

Base `e4c17f48d7e7216b51193e48497056f900e720e9`; upstream `ed064e0c867e7688234ad36c0350377544ef93f9`;
merge `2714d5957936389a88e8b821cd9a14da908b0ab2`; head is the commit that adds this file. Logs are in
`/tmp/cu1112/logs/CU-C2/` on the host, all hashed in its `SHA256SUMS`; container copies are in
`/workspaces/logs/` and `/workspaces/results/`.

| Check | Command | Exit | Log | Log sha256 |
|---|---|---|---|---|
| Baseline prep | `heavy-build.sh CU-C2-baseline -- docker exec … bash /workspaces/logs/CU-C2-prep.sh /workspaces/wt/impl` | 0 | `CU-C2-prep-base2.log` | `877c32af…` |
| Baseline suite | `suite.sh /workspaces/wt/impl /workspaces/results/CU-C2-base cargo engconf ts` | 0 (script) | `CU-C2-suite-base2.log`, `CU-C2-base/cargo.log` | `3b9b3487…`, `24adbb9c…` |
| Baseline prep-extra | `bash /workspaces/prep-extra.sh /workspaces/wt/impl` | 0 | `CU-C2-prep-extra-base.log` | `66b839da…` |
| Baseline TS | `suite.sh … /workspaces/results/CU-C2-base-ts ts` | 0 (turbo exit 0) | `CU-C2-base-ts/ts.log` | `42857d62…` |
| Merge prep (rebuilt `wasm32`) | `CU-C2-prep.sh` after deleting the stale `wasm32` outputs | 0 | `CU-C2-prep-merge2.log` | `362731b3…` |
| Generator typecheck | `pnpm -s run typecheck` in `tooling/engine-conformance` | 0 | `CU-C2-typecheck-merge.log` | `49555e0d…` |
| Workspace build | `cargo build --workspace --tests` | 0 | `CU-C2-cargo-build-merge.log` | `a1ac438e…` |
| Regeneration, run 1 and 2 | `bash /workspaces/regen-engine.sh /workspaces/wt/impl` (twice, `CU-C2-regen2x.sh`) | 0 | `CU-C2-regen-run1.log`, `CU-C2-regen-run2.log` | both `7fe7523e…` |
| Determinism | `sha256sum` of every vector file after each run, compared | 0 | `CU-C2-regen-determinism.log` | `0c94355b…` |
| After suite | `suite.sh /workspaces/wt/impl /workspaces/results/CU-C2-after cargo engconf ts` | 0 (script) | `CU-C2-suite-after.log`, `CU-C2-after/cargo.log`, `CU-C2-after/ts.log` | `0c587ed7…`, `fd3e7af1…`, `d9a96fb1…` |
| TS rerun, viewer and vue | `turbo run test --concurrency=1 --filter=@embedpdf/vue --filter=@embedpdf/viewer --only` | 0 | `CU-C2-ts-rerun-viewer-vue.log` | `643f97aa…` |
| Failing names | `cargo.failed` before and after | | `CU-C2-base/cargo.failed`, `CU-C2-after/cargo.failed` | both `898127a7…` |

The regeneration logs and determinism log are from the second attempt. The first attempt failed in
13 groups on the stale `wasm32` payload and was overwritten (CU-C2-D3); its outcome is in this
job's channel.

## Known gaps

- `wire:check` and `operations:check` still fail on impl (CU-C2-D5).
- New upstream engine behaviour has no engine vector cases (CU-C2-D6).
- impl's Rust stays red on vector shape (D-03); the port is CU-C4 to CU-C6 on plugins.
- `runtime-src` names `17d8073d4`, which exists only in the local runtime fork until the human
  pushes `catchup/2026-11-12-runtime` (D-04).
- The plugin-conformance part of `suite.sh` wasn't run: impl has no such package.

## Cleanup

See the ledger's progress section and this job's final message: `target/` and the container
worktree `/workspaces/wt/impl` are removed after the branch is on the host. The container, the
volume and `/workspaces/results/CU-C2-*` stay for later jobs.

FEEDBACK: BROKEN `prep.sh` kept a `wasm32` payload built from the previous runtime because
`fetch-payload.sh` never replaces a local build, which cost one failed regeneration with
"Cannot call unknown function EPDFPage_CountViewportsRaw". Suggest `prep.sh` delete
`npm/wasm32/lib` when `build/libpdfium/wasm32/BUILD-METADATA.json` names a different runtime sha
than `engine-runtime-build.json`. WORKS heavy-build plus `work_wait` ran eight steps with no model
polling.
