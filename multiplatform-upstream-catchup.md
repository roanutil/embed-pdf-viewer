# Catching the Rust port up to upstream, October 2026

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan phase by phase. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** bring `research/multi-platform-rust-core-impl` and this branch up to upstream `main`, runtime included, with every engine and plugin vector regenerated from the new TypeScript and replayed by Rust.

**Architecture:** the same route as the last catch-up (`multiplatform-plugins-rebase.md`): `main` into impl, impl into this branch, then make the generators run, regenerate, and re-port crate by crate until `cargo test` replays every file. The runtime fork moves in the same step as the TypeScript, because they are coupled (see "The runtime").

**Tech stack:** Rust workspace under `crates/`, the TypeScript oracle under `packages/`, `tooling/engine-conformance` and `tooling/plugin-conformance`, the PDFium fork at `roanutil/embed-pdf-runtime`.

**Spec:** `multiplatform-plugins-rebase.md` (method), `multiplatform-plan.md` (the port's goals).

## Global constraints

- Push research branches to `private-fork` only. The runtime fork branch goes to `roanutil/embed-pdf-runtime`.
- Rust matches the TypeScript reference. The kept divergences in `docs/research/rust-plugins-branch-review.md`, "Status", stay divergent.
- No TypeScript behavior changes under `packages/**` beyond what upstream brings, except H3 and M14 (see "Decisions").
- No `.github/**` changes.
- Mutation tests only through `scripts/mutants.sh`, one `--file` at a time. Other cargo commands as `CARGO_BUILD_JOBS=6 cargo ...`.
- At most two agents at once, each on its own branch off the integration tip, merged back one at a time.

## Review focus

- A vector file that regenerates byte-identical is not proof its crate is right: check the suite still exercises the code that moved (the annotation model is the likely case).
- H3's crash path: a gesture whose annotation disappears mid-drag. Upstream's new model has no guard there yet.
- Payload hash drift: every Rust change that reaches `epdf-wasm` needs a published payload before a fresh clone works.
- Kept divergences can silently become convergent or get lost in a re-port; re-check each against the list after its crate is ported.
- Engine wire kinds: upstream grew the request union from 84 to 89 kinds, and a Rust engine that answers an unknown kind with a generic error passes vectors that never send it.

---

## Where it starts

Measured on 2026-10-20 against upstream `main` at `b456ee4fd`. This branch and impl share their merge base with `main`, `2516e2786` (2026-09-22), and pin the same runtime commit, `7c7b600b1`.

Upstream has 241 non-merge commits since then. Under `packages/core` and `packages/plugin` they change 955 files (89,867 insertions, 37,240 deletions), about twice what the last catch-up absorbed (43,658 and 19,678). Under `packages/engine/{core,main,services}` they change 1,145 files (154,088 insertions, 20,710 deletions). Five packages are new and have no port: `core/fonts`, `core/form`, `plugin/attachment`, `plugin/history` and `plugin/outline`.

Impl has 11 commits this branch lacks (`547c6b5a2` to `061a544c4`: the toolbar gate, the `-O2` link, bindings for the current pin, and three read fixes).

### Merges, probed

`git merge-tree` of `main` into impl conflicts in 14 files plus the runtime-src pointer: the conventions skill and `.prettierignore` (upstream moved `references/` to `docs/conventions/` and made the skill index a table), `engine-lifecycle.md` (carried along by that move), and 11 under `packages/engine` and `packages/framework/react`, the largest being `WorkerHost.ts` (upstream: 1,518 insertions, 628 deletions; impl: 115 insertions).

Merging impl into this branch conflicts in `packages/framework/react/src/toolbar.tsx` only. Merging `main` into the result adds 18 more files, including `pnpm-lock.yaml` and the deletion of `packages/core/annotation/src/update.ts` (upstream's `67180c5d8` split it into `update/` and a three-layer state model), which holds H3.

### Plugin vectors

`tooling/plugin-conformance/src/staleness.ts`, fixed in this commit (it silenced the console that scenario suites record, and checked each suite against every vector file), reports 110 unchanged on this branch's tip, matching `vectors:check`. On the probe merge (`scratch/catchup-probe`, `db5997aa6`, upstream's side on every conflict):

| State | Count | Vector files |
|---|---|---|
| Unchanged | 5 | `core/signature-parse`, `core/stage-anchor`, `core/stage-camera`, `core/stage-layout`, `plugins/actions` |
| Stale | 24 | `kernel/lifecycle`, `kernel/turns`; `core/acrojs`, `core/acrojs-host`; `core/signature-` `document`, `signers`, `trust`, `verify`; `plugins/actions-` `dispatcher`, `hover`, `lifecycle`, `script`, `submit`, `triggers`; `plugins/` `metadata`, `page-edit`, `render-capability`, `render-tile-plane`, `search`, `search-stage`, `search-find-all`, `stage-capability`, `stage-events`, `stage-motion` |
| Throws | 13 | the eight annotation scenarios, the four form scenarios and `stage-feeds`, all `no plugin factory "interaction"` |
| Does not import | 31 suite files | `annotation-core-*` (6, `initialStyle` gone from `update`), `annotation-pure`, `annotation-repository`, `commands`, `form-core`, `geometry`, `i18n`, `interaction`, `kernel-utils`, `link`, `measurement`, `redaction`, `render-pure`, `search-reducer`, `selection`, `shell`, `signature-plugin`, `stage-pure`, `stamp-*` (7), `ui-core`, `view-manager` |

As last time, most of "throws" is `interaction.ts` not importing.

### Engine vectors

With upstream's PDFium-only payload (`runtime-payloads-da4b62329b0e9769`) and engine-main built, none of the 33 `tooling/engine-conformance` checks passes:

| State | Count | Checks |
|---|---|---|
| Runs, differs | 6 | `schema`, `search`, `signature-protection`, `security`, `forms`, `metadata-write` |
| Engine API reshaped | 18 | `vectors`, `metadata`, `save` (`doc.metadata.read`), `annotation-read`, `-delete`, `-move` (`listRaw`, `listRawAll`), `annotation-create`, `-update`, `-appearance`, `attachments`, `extract` (`handle.text.read`), `form-effects`, `form-field`, `form-write`, `files-write`, `pages-mutate` (`doc.pages.move`), `piece-info`, `redaction` |
| Does not import | 6 | `render`, `charmap`, `layout`, `annotation`, `annotation-write`, `actions` |
| Other | 3 | `scope` (`rawScope.map is not a function`), `wire` (84 kinds expected, the union has 89), `operations` (new protocol operations such as `annotations.list`, `annotations.reorder` have no row) |

## The runtime

Upstream viewer `main` pins runtime `1461b0c51`. The fork merge of it is resolved and parked on the local runtime-src branch `sync/runtime-1461b0c51` (`1d23fe9e9`) in `.claude/worktrees/plugins-share`: upstream's two `build-target.sh` edits are ported into `target-args.sh` (all nine targets' gn args match upstream's byte for byte), and `skia/BUILD.gn` takes upstream's structure with the `pdf_use_skia` gate on expat.

It can't land alone. Between `7c7b600b1` and `1d23fe9e9` the public headers lose 27 functions and gain 106, and 12 of the lost ones are called by this branch's TS and Rust: `EPDF_GetPageRotationByIndex`, `EPDF_GetPageSizeByIndexNormalized`, `EPDFAnnot_ApplyRedaction`, `EPDFAnnot_SetRectangleDifferences`, and eight `EPDFForm_*` writers (`SetTextValue`, `SetToggle`, `SetChoiceValues`, `ResetField`, `ImportFDF`, `ImportXFDF`, `SetFieldDefaultValues`, `RemoveFieldDefaultValue`). Upstream's TS moved to replacements such as `EPDF_GetPageRotateByIndex` and `EPDFForm_LoadModel`. The Rust side (`crates/pdfium/src/page_list.rs`, `page_write.rs`, `annot_write.rs`, `form_write.rs`) follows the TS in the engine re-port.

## Decisions

Made on 2026-10-20.

1. **H3** (`046d47578`): re-apply it. After the merge, `reconcile`'s temp-id rename reaches the draft in upstream's `update/edit.ts` and `update/edit-commit.ts`, and the edit gesture's reads of a missing annotation clear the draft instead of throwing. The Rust re-port carries the same guards, and the `core/annotation-update` vector case (create, pointer down, Created, move, up) stays.
2. **M14** (`0bef57fe5`): follow upstream where it is stricter, drop M14 where upstream makes it unneeded. Upstream's `document-events.ts` no longer reads the print policy there; it blocks every print whose origin isn't `user`. Check that rule against M14's case (a script `print()` under a blocking policy); if upstream's rule already refuses it, drop M14's code and port upstream's rule.
3. **The five new packages**: port all of them, each as a crate under `crates/plugins/`. `core/fonts`, `core/form` and `plugin/history` first, because the ported `plugin/form` and `plugin/page-edit` now depend on them; then `plugin/outline` and `plugin/attachment`.

## Order of work

All work lands on two integration branches, so `research/multi-platform-rust-core-impl` and this branch move only when the user approves the final merge: `research/catchup-impl` (off impl, worktree `.claude/worktrees/catchup-impl`) and `research/catchup-plugins` (off this branch at the plan's commit, worktree `.claude/worktrees/catchup-plugins`). Pushes wait for the user's go-ahead; nothing in a task pushes.

The re-port tasks (8, 13) are too large to write out before the vectors exist. Tasks 7 and 12 end by appending numbered tasks for them to this file, one per crate or feature family, each with the vector files it must replay.

### Task 1: Sync script takes a source branch

Branch `research/catchup-plugins`. `scripts/sync-upstream.sh` merges `main` only. Give it `--from <url-or-ref> <branch>` (default: upstream viewer `main`), so the same script merges `main` into impl and impl into this branch. The runtime-src step stays: read the pointer the source pins, merge it into the fork branch, resolve this side's pointer to the fork merge. When the source pins the same runtime commit as the fork branch already contains, the runtime step is a no-op.

- [ ] Add the option and keep the default behaviour byte-for-byte for `main`.
- [ ] Test both paths on a throwaway worktree (`git worktree add --detach`), as the first version was tested: `--from` a local branch, and the default. Delete the worktree.
- [ ] `shellcheck scripts/sync-upstream.sh` clean; commit.

### Task 2: Merge upstream main into impl

Branch `research/catchup-impl`, created from `research/multi-platform-rust-core-impl`. Runtime-src must point at `1d23fe9e9` (local branch `sync/runtime-1461b0c51` in `.claude/worktrees/plugins-share/packages/engine/runtime/runtime-src`; the new worktree's submodule borrows objects from there with `git submodule update --init --reference`).

- [ ] Merge upstream `main` at `b456ee4fd` (`git -c core.commitGraph=false merge`). Resolve the 14 files: upstream's side, then re-apply impl's own change where it is Rust-core routing or build (`WorkerHost.ts`, `InlineTransport.ts`, `PageGeometryReader.ts`, `index.ts` exports, `compile.esm.sh`, `engine-runtime-build.local.json`). The conventions skill: upstream's table plus a row for `docs/conventions/engine-lifecycle.md`; `.prettierignore`: both sides; repoint `.agents/skills/embedpdf-conventions/references/` mentions in `crates/` and `multiplatform-plan.md` to `docs/conventions/`. `pdf-functions.generated.ts`: take upstream's, then regenerate from the fork's headers once Task 3's build has them (`node build/generate-functions.mjs`), and commit that separately.
- [ ] Runtime-src to `1d23fe9e9`; `.gitmodules` to the fork URL and branch, as on this branch.
- [ ] `pnpm install`, `pnpm exec turbo run build --filter=@embedpdf/engine...`, `pnpm exec turbo run typecheck --filter=@embedpdf/engine...`: no errors. Commit the merge alone, before anything else.

### Task 3: Fork build and payload inputs for the new pin

Not a subagent task: the controller starts `buildFork` for `1d23fe9e9` (wasm32-eh) in the background with `PERF_WORK_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work`, and fetches upstream's native `darwin-arm64` artifact for `1461b0c51` into `packages/engine/runtime/build/libpdfium/` for host Rust tests (`bash packages/engine/runtime/scripts/fetch-libpdfium.sh darwin-arm64`). Upstream DEPS changed, so the fork build takes hours; Tasks 4 to 7 do not need it.

### Task 4: Engine generators import and boot

Branch `research/catchup-impl`, in `tooling/engine-conformance`. Make every generator load and run against the new engine API, keeping each generator's cases and its output format; only how it reaches the engine changes. First the six that do not import (`render`, `charmap`, `layout`, `annotation`, `annotation-write`, `actions`), then `scope` (`rawScope.map is not a function`), `wire` (the request union is 89 kinds; list the five new ones), `operations` (add rows for the new protocol operations, with the same columns), then the 18 whose engine calls were reshaped (`doc.metadata.read`, `annotations.listRaw`, `listRawAll`, `handle.text.read`, `doc.pages.move`, `forms.applyEffects`, `forms.createField`, `holder.applications`, `service.read`, and the rest the checks name).

- [ ] Each script in `package.json` ending `:check` runs to a verdict (current, or "differ"), none throws. Record per script: current or differs.
- [ ] Where a case's input no longer exists in the new API (a removed operation), keep the case in a comment with the reason, and list it in the report.
- [ ] Commit. No vector file changes in this task.

### Task 5: Regenerate engine vectors and write the diff report

- [ ] Run every regenerate script (the `:check` names without the suffix) and commit the vectors.
- [ ] Write `docs/research/catchup-engine-vector-diffs.md`: per vector file, unchanged or what changed (new fields, renamed fields, different values, new or removed cases), with the upstream commit that caused it where `git log -S` finds it. This report is the input for the engine re-port.

### Task 6: PDFium bindings for the new pin

`crates/pdfium-sys` regenerates from `wrapper.h` and the include directory. Add any new headers to `wrapper.h`. In `crates/pdfium`, replace the 12 removed functions with what upstream's TS now calls (for example `EPDF_GetPageRotateByIndex` for `EPDF_GetPageRotationByIndex`; the form writers through `EPDFForm_LoadModel`): read upstream's `packages/engine/services/src/features/` for each.

- [ ] `CARGO_BUILD_JOBS=6 cargo build --workspace` and `cargo test -p pdfium -p pdfium-sys` pass against the `darwin-arm64` artifact from Task 3.
- [ ] Commit.

### Task 7: Engine re-port tasks

- [ ] From Task 5's report, append Tasks 8.1 to 8.N to this file: one per feature family under `crates/epdf-engine/src/features/` (and the wire kinds), each naming the vector files it must replay and the upstream TS files that are its reference. Order them so shared types come first. Commit the plan change.

### Task 8: Engine re-port (expanded by Task 7)

Each sub-task ends with `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine` replaying its vector files, and the kept divergences in its area re-checked against `docs/research/rust-plugins-branch-review.md`, "Status".

### Task 9: Payload with the Rust core

- [ ] Once Task 3's fork build is done: from `.claude/worktrees/catchup-impl`, `PERF_WORK_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work node scripts/publish-engine-payload.mjs --dry-run` (bring the script over from this branch if impl lacks it), `verify-rust-core.mjs` passes, and the 33 engine `:check` scripts are current.

### Task 10: Merge impl into this branch, with H3

Branch `research/catchup-plugins`.

- [ ] Merge `research/catchup-impl` (with Task 1's script, `--from`). `toolbar.tsx` and the files that came from `main` resolve as in Task 2; `packages/core/annotation/src/update.ts` taken as deleted; `pnpm-lock.yaml` impl's, then `pnpm install`.
- [ ] Re-apply H3 to `packages/core/annotation/src/update/edit.ts` and `update/edit-commit.ts`, as in "Decisions": the temp-id rename reaches the draft, and the gesture's reads of a missing annotation clear the draft. Add a TS unit test for create, pointer down, Created, move, up.
- [ ] Commit the merge, then the H3 change separately.

### Task 11: Plugin generators import

`tooling/plugin-conformance/src/suites`. Rewrite each of the 31 suite files that do not import against the new module layout, starting with `interaction.ts` (it clears the 13 "throws") and the six `annotation-core-*`. Keep each suite's cases; change only how it reaches the code. Add suites for the five new packages only in Task 13's sub-tasks.

- [ ] `npx tsx --conditions=development src/staleness.ts` reports no `no-import` and no `throws`. Replace the probe table under "Plugin vectors" with this result. Commit.

### Task 12: Regenerate plugin vectors and plan the re-port

- [ ] `pnpm --filter @embedpdf/tooling-plugin-conformance run vectors`; commit.
- [ ] Write `docs/research/catchup-plugin-vector-diffs.md` as in Task 5.
- [ ] Append Tasks 13.1 to 13.N: one per crate in dependency order (kernel, core libraries, `core/fonts`, `core/form`, interaction and actions, `plugin/history`, stage, selection, annotation with H3, form, page-edit, the other plugins, `plugin/outline`, `plugin/attachment`), each naming its vector files and TS reference; the M14 check goes with actions. Commit.

### Task 13: Plugin re-port (expanded by Task 12)

Each sub-task ends with its crate's `cargo test` replaying its vector files, and its kept divergences re-checked.

### Task 14: Close

- [ ] `coverage` regenerated; `vectors:check` and `coverage:check` clean; every engine `:check` current.
- [ ] `scripts/mutants.sh` on every crate whose source changed, one `--file` at a time; triage survivors.
- [ ] Update each touched crate README, `multiplatform-plugins-status.md`, `multiplatform-port-status.md` and `multiplatform-start-here.md`.
- [ ] Report to the user for the merges and pushes: `research/catchup-impl` into impl, `research/catchup-plugins` into this branch, `sync/runtime-1461b0c51` to the fork, and the payload publish.

## What it will cost

The last catch-up took 38 first-parent commits and touched 510 files under `crates` and `tooling/plugin-conformance/src` (64,183 insertions, 23,111 deletions), with no runtime or engine-API change. This one has twice the plugin-side churn, a reshaped engine API, a runtime with removed functions, and five new packages. Expect more than that: several weeks of agent time, and a fork rebuild every time the runtime pin moves.

## Re-running the probe

`scratch/catchup-probe` (`.claude/worktrees/catchup-probe`) holds the probe merge and stays until Phase 2 is done. To measure against a newer upstream: merge into a throwaway branch with upstream's side on conflicts, check out upstream's `packages/engine/runtime/scripts` (so `fetch:payload` finds upstream's PDFium-only payload), `pnpm install`, build `@embedpdf/engine...`, then run `npx tsx --conditions=development src/staleness.ts` from `tooling/plugin-conformance` and each `:check` script from `tooling/engine-conformance`.
