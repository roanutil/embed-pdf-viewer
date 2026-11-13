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

### Port (CU-C4 to CU-C6)

Three jobs ported upstream `074a190c9..ed064e0c8` from CU-C3's tip, each on its own branch.

| Job | Branch tip | Result |
|---|---|---|
| CU-C4 | `4fffe696b` | New engine group `changes` (41 kinds, viewports, scale, rename, widget rename, annotation and form copy) and the `epdf-engine`/`pdfium` port that replays it. [Report](docs/research/upstream-catchup-2026-11-12/reports/CU-C4.md) |
| CU-C5 | `756b8cf16` | Annotation, form, measurement, stamp and signature crates, annotation and form-field duplicate, four kernel changes. Evidence 679 of 679. [Report](docs/research/upstream-catchup-2026-11-12/reports/CU-C5.md) |
| CU-C6 | `063eb2285` | Kernel, stage (wheel, scroll chaining, coasting) and the remaining plugins; CU-C5's kernel commits taken as cherry-picks. [Report](docs/research/upstream-catchup-2026-11-12/reports/CU-C6.md) |

None made a NEW DIVERGENCE or changed `packages/**`.

### Integration (CU-C7)

`c339af05d`, `c4b25c066` and `87b752368` merge c4, c5 and c6 into this branch; only `coverage.json`
conflicted, and it was regenerated. CU-C7 then fixed the one cargo failure and the eight evidence
failures the merge exposed, matched upstream for `labelParams` that aren't finite, added a commands
case for the translated refusal sentences, fixed the payload script to use emsdk 4's own toolchain,
built the Rust-core `wasm32` payload and ran `core-wasm`, `epdf-web` and the `platforms/` wrappers
against the starting plugins tip. [Report](docs/research/upstream-catchup-2026-11-12/reports/CU-C7.md).
Every decision of the catch-up, NEW DIVERGENCE first (there are none), is in
[`decisions.md`](docs/research/upstream-catchup-2026-11-12/decisions.md).

## Final state

On `research/catchup-2026-11-12-plugins` at `4fd6375ee` (code; the docs commit after it changes no
code), in the container. "Start" is plugins `7990560d6`, except where the cell says otherwise.

| Check | Start | Now |
|---|---|---|
| `cargo test --workspace` | 3,292 passed, 0 failed | 3,305 passed, 0 failed (1 ignored) |
| engine `:check` scripts | 34 of 34 | 35 of 35 (new `changes:check`) |
| plugin `vectors:check`, `coverage:check`, `test:harness`, `typecheck` | all pass | all pass; statements 72,853 of 77,093 |
| plugin evidence tests | 677 tests at CU-C3 (661 passing after the regeneration) | 679 of 679 |
| TS package tests (`packages/**`) | 98 of 98 | 98 of 98 |
| `core-wasm` vitest, `test:gc` (Rust-core payload) | 92 of 92, 13 of 13 (2026-11-07 final) | 102 of 102, 13 of 13 |
| `epdf-web` wasm tests | | all pass (9 binaries) |
| `.NET` `dotnet test`, parity, packaging | 33 of 33, 16 of 16, OK | 33 of 33, 16 of 16, OK |
| `.NET` `pack.sh` + `validate.sh --rid linux-arm64` | PASS | PASS |
| Kotlin `test-command-context.sh` on a desktop JVM | pass | pass |
| Swift on Linux (smoke; Swift Testing) | 8 of 8; 466 of 504 | 8 of 8; 466 of 504, the same 38 failing |

The Swift failures are the harness's stubs (keychain, JavaScriptCore, image decoding) and three
Linux differences, listed in the CU-C7 report. The `@mfb/embedpdf-rust-viewer` typecheck still fails
on `#inline-transport`, as in 2026-11-07.

Impl (`research/catchup-2026-11-12-impl`, `a7c75f5ad`) has the upstream merge; its Rust stays red on
vector shape as at its start (D-03). The local `research/multi-platform-rust-core-impl` is
fast-forwarded to it (`e4c17f48d..a7c75f5ad`).

## Moving the plugins branch

`research/multi-platform-rust-core-impl-plugins` is checked out in `/Users/andrew/repos/embed-pdf-viewer`
with uncommitted work that isn't part of this catch-up, so it wasn't moved (D-02). It's a
fast-forward: `7990560d6` is an ancestor of `research/catchup-2026-11-12-plugins`. Once that checkout
is clean or its work is committed, from that checkout:

```sh
git merge --ff-only research/catchup-2026-11-12-plugins
```

From any other worktree while it isn't checked out anywhere, `git fetch . research/catchup-2026-11-12-plugins:research/multi-platform-rust-core-impl-plugins`
does the same and refuses anything but a fast-forward.

## Not done

- No push of any branch, viewer or runtime (D-04). The runtime-src pointer names `17d8073d4`, which
  exists only in the local `~/repos/embed-pdf-runtime` on `catchup/2026-11-12-runtime`.
- No releases: no `runtime-payloads-*` payload, no `apple-engine-*` framework and no `Package.swift`
  pin. Each is published from the Mac with `scripts/publish-engine-payload.mjs`.
- No Mac work (D-06): Xcode builds, iOS and macOS tests, the SwiftUI layer, demos, the iOS and macOS
  fork slices for `17d8073d4`.
- No Android work (D-06): the AAR, its 16 KB checks and the emulator smoke test.
- The four other `.NET` RIDs.
- The plugins canonical fast-forward (above).
- Remaining findings and gaps are in the reports' "Known gaps" sections and the manager's roll-up.
