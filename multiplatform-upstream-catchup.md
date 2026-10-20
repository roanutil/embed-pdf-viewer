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

### Phase 0: tooling

- [ ] Rework `scripts/sync-upstream.sh` to take a source branch, so it can merge `main` into impl and impl into this branch, each with the runtime-src resolution. Test it on a throwaway worktree, as for the first version.
- [ ] Push `sync/runtime-1461b0c51` to `roanutil/embed-pdf-runtime` so the parked resolution survives this machine.
- [ ] Start the fork build of `1d23fe9e9` for wasm32-eh (`buildFork` in `perf-compare/scripts/build-arm.mjs`). Upstream changed DEPS, so expect hours; it runs while Phase 1 proceeds.

### Phase 1: impl

- [ ] Merge `main` into impl, resolving the 14 files: take upstream's side, then re-apply impl's change where it is the Rust-core routing (`WorkerHost.ts`, `InlineTransport.ts`, `PageGeometryReader.ts`, `compile.esm.sh`, `engine-runtime-build.local.json`); regenerate `pdf-functions.generated.ts` from the new headers rather than merging it. Point runtime-src at `1d23fe9e9`. Commit the merge before anything else.
- [ ] Make the 33 engine generators run against the new engine API (the "reshaped" and "does not import" rows), keeping each generator's cases. Then `wire`, `operations` and `scope`.
- [ ] Regenerate the engine vectors and read every diff.
- [ ] Re-port `crates/pdfium` to the new headers and `crates/epdf-engine` to the regenerated vectors, until every engine conformance test in `crates/` replays its file. Add the five new wire kinds.
- [ ] Build the payload with the Rust core (`scripts/publish-engine-payload.mjs --dry-run` from a tree with the fork built), run `verify-rust-core.mjs`, push impl.

### Phase 2: this branch

- [ ] Merge impl into this branch. `toolbar.tsx` and the 18 files from `main` resolve as in Phase 1, with `update.ts` taken as deleted and H3 re-applied to `update/` as in "Decisions". `pnpm-lock.yaml`: take impl's, then `pnpm install`.
- [ ] Re-run `staleness.ts` and record the table here, replacing the probe's.

### Phase 3: plugin generators and vectors

- [ ] Rewrite each "does not import" suite against the new module layout, starting with `interaction.ts` (it clears the "throws" row) and the six `annotation-core-*` suites. Keep each suite's cases; change only how it reaches the code.
- [ ] Run `vectors`, then read the diff of every vector file.

### Phase 4: re-port

- [ ] Re-port crate by crate in dependency order: kernel, core libraries, interaction and actions, stage, selection, annotation, then the plugins on top, until `cargo test` replays every file. Where the TS changed shape (the annotation model's three layers), move the Rust to match rather than adapting vectors.
- [ ] Port `core/fonts`, `core/form` and `plugin/history` before re-porting `plugin/form` and `plugin/page-edit`; port `plugin/outline` and `plugin/attachment` after the plugins they use.
- [ ] Settle M14 against upstream's print rule, as in "Decisions".

### Phase 5: close

- [ ] Regenerate `coverage.json`; re-run mutation testing on every crate whose source changed.
- [ ] Re-check the kept divergences one by one.
- [ ] Update each crate README, `multiplatform-plugins-status.md` and `multiplatform-start-here.md`.
- [ ] Publish the payload, push this branch and the fork branch, then point the perf harness's `plugins-next` arm at the tip for a run.

## What it will cost

The last catch-up took 38 first-parent commits and touched 510 files under `crates` and `tooling/plugin-conformance/src` (64,183 insertions, 23,111 deletions), with no runtime or engine-API change. This one has twice the plugin-side churn, a reshaped engine API, a runtime with removed functions, and five new packages. Expect more than that: several weeks of agent time, and a fork rebuild every time the runtime pin moves.

## Re-running the probe

`scratch/catchup-probe` (`.claude/worktrees/catchup-probe`) holds the probe merge and stays until Phase 2 is done. To measure against a newer upstream: merge into a throwaway branch with upstream's side on conflicts, check out upstream's `packages/engine/runtime/scripts` (so `fetch:payload` finds upstream's PDFium-only payload), `pnpm install`, build `@embedpdf/engine...`, then run `npx tsx --conditions=development src/staleness.ts` from `tooling/plugin-conformance` and each `:check` script from `tooling/engine-conformance`.
