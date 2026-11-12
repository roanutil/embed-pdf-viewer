# CU-C2 decisions

Decisions made while merging upstream `ed064e0c8` into impl and regenerating the engine vectors on
`research/catchup-2026-11-12-impl`. They're committed with the report, in the commit that adds this
file, unless a different commit is named.

## CU-C2-D1. Merge the CU-C1 record with a merge commit

Decision: `12256e882` merges `research/catchup-2026-11-12-c1` with `--no-ff`, though a fast-forward
was possible (`eabb14bc8` sits directly on `e4c17f48d`).
Reason: the brief says "merge", and a merge commit keeps the CU-C1 branch visible as its own step in
the history CU-C3 will merge again.
Cost: one extra merge commit with no content of its own.
Reverse: recreate the branch from `eabb14bc8` and replay the later commits.

## CU-C2-D2. Prepare impl with a copy of `prep.sh` that doesn't name the rust-viewer package

Decision: impl was prepared with `/workspaces/logs/CU-C2-prep.sh`, the shared `/workspaces/prep.sh`
with `--filter='!@mfb/embedpdf-rust-viewer'` removed. The shared script and the committed
`scripts/prep.sh` are unchanged.
Reason: impl has no package by that name, so `turbo run build` stopped with "No package found with
name '@mfb/embedpdf-rust-viewer' in workspace" and prep failed (first baseline attempt,
`CU-C2-prep-base.log`). Plugins has the package and needs the filter, so the shared script stays as
it is for CU-C3. This is the case CU-C1's finding R4 points at.
Cost: impl's prep differs from the committed script by that one filter.
Reverse: none needed. A later edit could make the filter conditional on the package existing.

## CU-C2-D3. Rebuild the wasm32 payload after the merge

Decision: before regenerating vectors, delete the ignored build outputs
`packages/engine/runtime/npm/wasm32/lib` and `packages/engine/runtime/build/libpdfium/wasm32` in
`/workspaces/wt/impl`, and run prep again. With no published payload for runtime state
`69b0e491e9586b32`, prep built `wasm32` locally from upstream's `runtime-aa59ce554` library (1,061
bound functions).
Reason: the baseline prep built `wasm32` from runtime `4346c5131`, and `fetch-payload.sh` leaves any
local build in place ("locally built payload present, leaving it untouched"). The first regeneration
then failed in 13 groups with "Cannot call unknown function EPDFPage_CountViewportsRaw", one of the
5 functions upstream runtime `55cc10b20` added.
Cost: one extra prep (about 3 minutes). Nothing tracked changed.
Reverse: not applicable.

## CU-C2-D4. Take upstream's `engine-runtime-build.json`

Decision: the merge keeps upstream's file unchanged, pinned to release
`runtime-aa59ce554aab55879b728dbb86714e13e5cb327f`.
Reason: the brief says to take upstream's unless the linux-arm64 release is missing. It's there:
prep fetched it, and its sha256 `da0ec9f9…` matches the file (CU-C1 report).
Cost: none.
Reverse: not applicable.

## CU-C2-D5. Leave `wire:check` and `operations:check` failing on impl

Decision: no change to the wire generator or `operations.ts` on impl. Both checks fail after the
merge exactly as they did at `12256e882`.
Reason: both failures are older than this catch-up. `wire` expects 89 request kinds and the union has
94; `operations` lacks rows for `document.fonts.program`, `document.fonts.resolve`, `fonts.list` and
`fonts.program`. Plugins already carries the fix (`cc2d9490f` and the fonts generator work there),
and CU-C3 merges this branch into plugins. The 2026-11-07 catch-up made the same call ("`wire:check`
still fails as at the start").
Cost: impl's engine `:check` scripts stay at 30 of 32.
Reverse: cherry-pick plugins' `operations.ts`, `worker-kinds.ts` and wire generator changes onto
impl.

## CU-C2-D6. No generator commit and no vector commit

Decision: the brief's two separate commits (generator fixes, vectors) don't exist, because neither
had any content.
Reason: `tsc --noEmit` in `tooling/engine-conformance` exits 0 at `2714d5957`. The generators use
nothing upstream renamed or removed in `074a190c9..ed064e0c8`. All 30 regenerable groups wrote output
byte-identical to the committed vectors in both runs (`git status` clean). The 19 non-merge upstream
commits under `packages/engine` in that range add behaviour that no existing case exercises: raw
viewports (`726ec2dd6`, `c7f8d75ff`), page scale as a change (`65b0f137e`, `8bc6546e5`), duplicate
annotations and form fields (`1481e4556`, `a390171cc`), rename through an annotation update
(`756f8e56b`), and change-kind versions (`a4805a492`).
Cost: impl's engine vectors don't cover that new behaviour. Adding cases is new coverage, not
regeneration, so it's left for the port jobs (CU-C3 to CU-C6) to decide.
Reverse: not applicable.

## CU-C2-D7. Count the TS viewer and vue timeouts as load, not regressions

Decision: the after-merge TS run's 8 failures (4 in `@embedpdf/viewer`, 4 in `@embedpdf/vue`, each
a browser test timing out after about 45 s) are recorded as load timeouts.
Reason: the suite runs turbo at `--concurrency=6`. Run alone at `--concurrency=1` on the same tree,
with a turbo cache miss, `@embedpdf/viewer` passed 585 of 585 and `@embedpdf/vue` 184 of 184
(`CU-C2-ts-rerun-viewer-vue.log`).
Cost: the full-suite TS number reads 96 of 98 tasks, not 98.
Reverse: rerun the suite at lower concurrency.
