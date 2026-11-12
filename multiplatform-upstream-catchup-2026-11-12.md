# Upstream catch-up, 2026-11-12

This is the ledger for bringing `research/multi-platform-rust-core-impl` ("impl") and
`research/multi-platform-rust-core-impl-plugins` ("plugins") up to upstream `main` again. Upstream is
canonical. The Rust port, the conformance tooling and the `platforms/` wrappers follow it. The method
is the catch-up guide at `docs/research/upstream-catchup/CATCHUP-GUIDE.md` on fork branch
`research/catchup-2026-11-06`, and the 2026-11-07 catch-up (`multiplatform-upstream-catchup-2026-11-07.md`
and `docs/research/upstream-catchup-2026-11-07/` on plugins `7990560d6`).

## Pinned targets

| What | Commit |
|---|---|
| Upstream viewer `embedpdf/embed-pdf-viewer` `main` | `ed064e0c867e7688234ad36c0350377544ef93f9` (PR #1079) |
| Upstream runtime `embedpdf/runtime`, pinned by that `main` | `aa59ce554aab55879b728dbb86714e13e5cb327f` |

These stay fixed for this catch-up. The previous catch-up's base with upstream `main` is
`074a190c9`; the range brings 132 commits.

## Starting points

| Branch | Commit | Runtime pin (`runtime-src`) |
|---|---|---|
| `research/multi-platform-rust-core-impl` | `e4c17f48d7e7216b51193e48497056f900e720e9` | `786765b80` |
| `research/multi-platform-rust-core-impl-plugins` | `7990560d6634298ae6767d9e30626102e124e981` | `3c6e02297` |

Impl isn't an ancestor of plugins: impl has two commits plugins lacks (`bc2b75c83` docs,
`e4c17f48d` gitignore).

## Working branches

All local; nothing is pushed.

| Branch | Repository | Purpose |
|---|---|---|
| `catchup/2026-11-12-runtime` | `~/repos/embed-pdf-runtime` | Runtime fork `3c6e02297` merged with upstream runtime `aa59ce554` (`17d8073d4`) |
| `research/catchup-2026-11-12-c1` | viewer | Container setup record and helper scripts |
| `research/catchup-2026-11-12-impl` | viewer | Upstream `main` merged into impl |
| `research/catchup-2026-11-12-plugins` | viewer | The impl catch-up merged into plugins, plus the port |

The canonical branches move only by fast-forward at the end.

## Where the work runs

Everything runs in dev container `embed-pdf-catchup1112` (image `embedpdf-dev:latest`), volume
`embedpdf-catchup1112` at `/workspaces`. Use `docker exec embed-pdf-catchup1112 bash -c '...'`.
Heavy steps go through `heavy-build.sh`. Each job has its own worktree `/workspaces/wt/<job>` of
the clone `/workspaces/embed-pdf-viewer` and brings its branch to the host as a git bundle. The
helper scripts are in `docs/research/upstream-catchup-2026-11-12/scripts/` and at `/workspaces/`.
The container layout and the PDFium libraries are in the CU-C1 report.

## Progress

### Runtime (CU-C1)

`catchup/2026-11-12-runtime` at `17d8073d4` merges upstream runtime `aa59ce554` into `3c6e02297`
with no conflicts. Upstream adds 5 `EPDF*` exports (the raw viewport API) and removes none.
linux-arm64 PDFium comes from upstream's `runtime-aa59ce554` release; `wasm32-eh` was built from
`17d8073d4`. See [`reports/CU-C1.md`](docs/research/upstream-catchup-2026-11-12/reports/CU-C1.md).

### Impl (CU-C2)

`2714d5957` merges upstream `ed064e0c8` into impl. Only `runtime-src` conflicted; it's set to
`17d8073d4`. The engine-conformance generators typecheck unchanged, and every regenerable engine
vector group is byte-identical to impl's committed vectors (two runs). `wire:check` and
`operations:check` fail as at the start; plugins already has their fixes. On impl,
`operations:check` reports that `worker-protocol.ts` has 94 request kinds where the plan documents
89, and that 5 font kinds have no row: `document.fonts.list`, `document.fonts.program`,
`document.fonts.resolve`, `fonts.list` and `fonts.program` (CU-C2-D5 names 4; corrected here, CU-C3-D8).

| Check | Start | After the merge |
|---|---|---|
| `cargo test --workspace` | 550 passed, 117 failed | 550 passed, 117 failed (same names) |
| engine `:check` scripts | 30 of 32 | 30 of 32 |
| TS package tests | 98 of 98 | 96 of 98; the 2 pass alone at lower concurrency (CU-C2-D7) |

As in the earlier catch-ups (manager decision D-03), impl's Rust stays red on vector shape and the
port happens on plugins. Report, decisions and the grouped failure list:
[`reports/CU-C2.md`](docs/research/upstream-catchup-2026-11-12/reports/CU-C2.md).

### Plugins (CU-C3)

`3aa47e708` merges the impl catch-up into plugins `7990560d6`. `runtime-src` conflicted (set to
`17d8073d4`) and so did `AGENT-BRIEF.md` (plugins' copy, which adds the Phase 2 section). The engine
vectors regenerate twice to the same bytes with one change, `limits` on `document.applyChanges` in
`wire/requests.json` (`1a4f373eb`, upstream `5496eb084`). The plugin-conformance generator had 7
type errors and stopped on the measurement suite; `c74ec3e9d` follows upstream's layout-borne page
scales (`c7f8d75ff`, `8bc6546e5`), `stageRemeasure`, in-order tool entries (`f0bb7b047`) and
once-per-change command derivations (`7af98f4df`). `3b8cf99eb` regenerates every plugin vector
(deterministic over two runs), and `b8ade7fe8` re-baselines the evidence archives.

| Check | Start `7990560d6` | Now `b8ade7fe8` |
|---|---|---|
| `cargo test --workspace` | 3,292 passed, 0 failed | 3,269 passed, 23 failed (CU-C5 10, CU-C6 13, `epdf-engine` none) |
| engine `:check` scripts | 34 of 34 | 34 of 34 |
| plugin `vectors:check`, `coverage:check`, `test:harness`, `typecheck` | all pass | all pass |
| plugin evidence tests | | 661 of 677 (the measurement B3 slice, left to CU-C5) |
| TS package tests | 98 of 98 | 98 of 98 |

The failures by crate and cause, the coverage gaps in upstream's new code, and the new engine
behaviour that still needs engine cases (CU-C4) are in
[`reports/CU-C3.md`](docs/research/upstream-catchup-2026-11-12/reports/CU-C3.md). The port jobs
CU-C4 to CU-C6 start from this branch.
