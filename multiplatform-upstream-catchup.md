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

**Reordered on 2026-10-21 (controller ruling, see the ledger):** this branch carries 36 `crates/epdf-engine` commits (10,813 insertions, 19 wire kinds) that impl lacks, so the engine re-port runs here, not on impl. Order: Tasks 1 to 6 on `research/catchup-impl`, then Task 10, then Task 7 again against this branch, then Tasks 8 and 9 here, then 11 onwards. `research/catchup-impl` stops at Task 6.

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

Task 7 wrote Tasks 8.1 to 8.24 on 2026-10-20 from `docs/research/catchup-engine-vector-diffs.md` at catchup-impl `52f10e027`. At that tip `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast` fails 98 tests in 29 targets and `cargo test -p epdf-wasm` fails 1 (`borrowed_page`). Each of the 99 belongs to exactly one sub-task's done criteria. 8.1 to 8.5 are shared foundations, and 8.6 to 8.22 are feature families, each placed after the sub-tasks it needs. 8.23 closes the re-port on catchup-impl. 8.24 runs on `research/catchup-plugins` after Task 10, because the Rust engine for 19 of the 89 wire kinds exists only on that branch (see 8.24).

| Sub-task | Needs |
|---|---|
| 8.1 Wire envelope, references, replay helper | none |
| 8.2 Session kinds | 8.1 |
| 8.3 Page space, page boxes, page list, destinations, render targets | 8.1 |
| 8.4 Dates and the metadata patch types | none |
| 8.5 Change queue and metadata | 8.1, 8.3, 8.4 |
| 8.6 Security and scope | none |
| 8.7 Attachments and piece info | 8.4, 8.5 |
| 8.8 Page structure writes | 8.3, 8.5 |
| 8.9 Page geometry and text | 8.3 |
| 8.10 Text layout | 8.9 |
| 8.11 Search | 8.3, 8.9 |
| 8.12 Annotation read | 8.3, 8.4 |
| 8.13 Annotation appearances | 8.12 |
| 8.14 Annotation create | 8.5, 8.12 |
| 8.15 Annotation update and write verdicts | 8.14 |
| 8.16 Annotation delete and reorder | 8.5, 8.12 |
| 8.17 Form read | 8.3, 8.4 |
| 8.18 Form values, reset and apply | 8.5, 8.17 |
| 8.19 Form fields and widgets | 8.15, 8.18 |
| 8.20 Annotation and form transfer | 8.14, 8.19 |
| 8.21 Redaction | 8.12 |
| 8.22 Outline | 8.3, 8.5 |
| 8.23 Close on catchup-impl | 8.1 to 8.22 |
| 8.24 Plugins-branch engine features | 8.23 and Task 10 |

Two sub-tasks may run at once only when neither needs the other.

#### Task 8.1: Wire envelope, references and the request replay helper

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Rust matches upstream's TypeScript at `b456ee4fd`. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md` on that branch; this task's section is "Wire (`host.rs`)". Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except those named below. Commit with explicit paths (`git commit -- <paths>`); no `git add -A`, no `git stash`, no push.

Upstream's `WorkerRequest` union has 89 kinds. Against the old 65-kind vector, 38 are added, 14 are removed, 49 are kept with changed fields and 2 are unchanged. This task makes the Rust host read the new envelope and the new references, refuse the removed kinds, and gives every later sub-task one way to replay a recorded `request`. It ports no new kind's behaviour.

Files: `crates/epdf-engine/src/host.rs`, `src/dispatch.rs`, `src/identity.rs`, `src/ids.rs`, `src/error.rs`, `crates/test-support/src/lib.rs`, a new `crates/test-support/src/replay.rs`, `crates/epdf-engine/tests/wire_contract.rs`, `crates/epdf-engine/tests/host_sessions.rs` where the envelope reaches it. Also `tooling/engine-conformance/src/operations.ts` and its rendering `tooling/engine-conformance/OPERATIONS.md`: two carried ledger items live there, and its `rustStatus` column is the routing record each later sub-task updates.

Vector: `wire/requests.json`.

Upstream reference: `packages/engine/core/src/wire/worker-protocol.ts` (the union, `WriteJobFields`, the `effect` tag), `packages/engine/core/src/wire/schemas.ts`, `packages/engine/core/src/identity/PageRef.ts`, `PageRef.schema.ts`, `AnnotationRef.ts`, `FormFieldRef.ts`, and `packages/engine/services/src/worker-host/WorkerHost.ts` (its switch and its refusal of an unknown kind). Commits: f550e9e5f (`effect`), b32fdf584 and 630b77eab (`opId`, `objectNumberFloor`), 91404cfcb and 2445caba7 (`authority`), 595bdc6c7 (`page: PageRef`, `pages: PageRef[]`, refs name their number `objectNumber`), bc0057f3a (`nm` and `index` refs gone, `baseIndex` added), 83f6598d2, 1233d1349, 98e5c270c, d7bd2d277, 119685ea4, b69c40377, c2d7a5194, a9434c0e2 (the 14 removals).

Prior art on `research/catchup-plugins`, absent on impl: `71775a1e4` (one PageRef type), `3baec217b` (PageRef on the Rust wire), `991031edd` (an unknown kind refused as WorkerHost refuses it), `c1e6330a6` (a failed request answers `name: "EngineError"`). Read each with `git show <sha> -- crates/epdf-engine`. Where a hunk fits upstream's current shape, apply it (`git show <sha> -- <path> | git apply -3`) and name the sha in the commit message, so Task 10's merge meets the same lines on both sides.

Kind owners. Every kind is routed by exactly one sub-task:

| Owner | Kinds |
|---|---|
| 8.1 | `abort`, `shutdown` |
| 8.2 | `open.fatMem`, `open.layerMemBase`, `open.layerFileBase`, `layer.close`, `close`, `objectNumbers.reserve`, `pages.workingSet`, `document.saveBuffer`, `document.saveLayerBuffer`, `document.saveFile`, `fonts.register`, `fonts.addFallback`, `fonts.clearFallbacks`, `fonts.clear` |
| 8.3 | `pages.list`, `actions.read`, `pages.render`, `pages.renderEncoded`, `document.renderPageFile`, `document.renderPageFileEncoded` |
| 8.5 | `document.apply`, `document.applyChanges`, `metadata.read`, `metadata.update`, `metadata.readCustom`, `metadata.updateCustom` |
| 8.6 | `document.probeSecurityFile`, `document.checkPasswordPermissions` |
| 8.7 | `attachments.list`, `attachments.readFile`, `pieceInfo.read`, `pieceInfo.applications`, `pieceInfo.update`, `pieceInfo.delete` |
| 8.8 | `pages.flatten`, `pages.extract` |
| 8.9 | `pages.geometry`, `pages.text` |
| 8.11 | `search.query` |
| 8.12 | `annotations.list`, `annotations.readFile` |
| 8.13 | `annotations.renderAppearances`, `annotations.renderAppearancesEncoded`, `annotations.readAppearance` |
| 8.14 | `annotations.create` |
| 8.15 | `annotations.update` |
| 8.16 | `annotations.delete`, `annotations.reorder` |
| 8.17 | `forms.list` |
| 8.18 | `forms.setValue`, `forms.reset`, `forms.repair` |
| 8.19 | `forms.createField`, `forms.updateField`, `forms.deleteField`, `forms.detachWidget`, `forms.addWidget`, `forms.updateWidget`, `forms.deleteWidget`, `forms.reorderWidgets`, `forms.reorderCalculations` |
| 8.20 | `annotations.export`, `annotations.import`, `forms.export`, `forms.import`, `forms.importValues` |
| 8.21 | `redaction.apply` |
| 8.22 | `outline.read` |
| 8.24 | `signatures.analyze`, `signatures.cancel`, `signatures.complete`, `signatures.contents`, `signatures.digest`, `signatures.finalizeCandidate`, `signatures.list`, `signatures.prepare`, `signatures.revisionBytes`, `measure.setScale`, `measure.viewports`, `pages.setName`, `pages.removeName`, `document.version`, `document.setFontSettings`, `fonts.authorizeEditing`, `annotations.flatten`, `annotations.exportAppearance`, `forms.setSignatureAppearance` |

Carried items owned here:
- Task 4a: the `abort` row note in `operations.ts` still says the switch has 64 arms and the protocol 65 kinds; the `pages.move`/`pages.insert` invariant notes and the `annotations.listFullPage` cache note were not carried onto `document.apply` and `annotations.list`.
- Task 4a: `signatures.finalizeCandidate` is filed C / after-first-write though its `effect` is `read`; `pages.workingSet` is queued-only though it never queues. Decide each against upstream's `effect` and WorkerHost's handling, and record the reason in the row's `note`.

Kept divergence to carry over: a malformed PageRef (unknown `kind`, a number that is not a positive integer) is refused as `InvalidArg`, where the TS worker does not validate (`3baec217b`, kept on 2026-10-15). Keep it, and say so in a comment at the check.

- [ ] Parse the envelope every job request carries: `effect`, `jobId`, `opId` and optional `objectNumberFloor` on writes, `authority` where the kind takes one. Answers keep their shape here; their owners change them.
- [ ] One `PageRef` (`{kind: "objectNumber", objectNumber}`) and one `AnnotationRef` (`objectNumber` or `baseIndex`, each with `page`) in `identity.rs`. Remove the `nm` and `index` ref kinds and `pageObjectNumber`/`pageObjectNumbers` request fields at the wire. Keep the internal page object number type; only the wire spelling moves.
- [ ] Remove the arms of the 14 removed kinds (`annotations.listFullPage`, `annotations.listRawAll`, `annotations.listRawPage`, `annotations.move`, `attachments.create`, `attachments.delete`, `forms.applyEffects`, `forms.attachWidget`, `pages.delete`, `pages.insert`, `pages.insertBlank`, `pages.move`, `pages.rotate`, `pieceInfo.clear`). Each now gets the answer WorkerHost gives an unknown kind. The feature code behind them stays until its owner reuses or deletes it.
- [ ] `crates/test-support/src/replay.rs`: open a fixture in a `Host`, take a case's recorded `request`, put the test's `docId` in place of the recorded one and real ids in place of `"<job>"` and `"<op>"`, send it through `Host::call` with its payloads, and return the parsed result or error. Unit-test the substitution.
- [ ] `wire_contract.rs`: expect 89 kinds and `schemaVersion` as the vector records it. Add a `NOT_YET_ROUTED` list naming each kind this task does not route, each with its owner sub-task from the table. The test fails if a listed kind routes (so the list cannot rot) or an unlisted kind answers `NotImplemented`. Each later sub-task removes its kinds from the list. Also assert that each of the 14 removed kinds gets the unknown-kind answer.
- [ ] `operations.ts`: fix the carried notes and the two rows above, and set `rustStatus` to what the host now does for every row (`ported`, `routed`, `planned`). Re-render with `pnpm --filter @embedpdf/tooling-engine-conformance run operations`; `run operations:check` passes.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test wire_contract` passes (owns `every_wire_kind_routes_to_an_implementation`), `cargo test -p test-support` passes, and `cargo test -p epdf-engine --test host_sessions` still passes.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p test-support --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p test-support`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.2: Session kinds: open options, object numbers, working set, save, fonts

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.1. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Wire (`host.rs`)"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

The session-level kinds, owned here by 8.1's table: `open.fatMem`, `open.layerMemBase`, `open.layerFileBase`, `layer.close`, `close`, `objectNumbers.reserve`, `pages.workingSet`, `document.saveBuffer`, `document.saveLayerBuffer`, `document.saveFile`, `fonts.register`, `fonts.addFallback`, `fonts.clearFallbacks`, `fonts.clear`.

Files: `crates/epdf-engine/src/host.rs`, `src/session/` (`registry.rs`, `document.rs`, `pages.rs`, `save.rs`, `authority.rs`), `src/features/save.rs`, `src/features/fonts.rs`, `src/mutation.rs`, `crates/epdf-engine/tests/wire_contract.rs` (remove this task's kinds from `NOT_YET_ROUTED`), `tests/host_sessions.rs`, `tests/document_session.rs`, `tests/session_registry.rs`, `tests/mutation_finalization.rs`, `tests/font_registry.rs`.

Vectors: none beyond `wire/requests.json`. The answers are pinned by unit tests ported from upstream's.

Upstream reference: `packages/engine/core/src/wire/worker-protocol.ts` (`ObjectNumbersReserveWorkerRequest`, `PagesWorkingSetWorkerRequest`, the open kinds' `objectNumbers`, `reserveObjectNumbers`, `signedDocumentPolicy`, and the open answer's `objectNumbers`/`lastObjectNumber`), `packages/engine/core/src/identity/ObjectNumbers.ts`, `packages/engine/services/src/worker-host/WorkerHost.ts`, `packages/engine/services/src/features/save/`, `packages/engine/core/src/dto/PdfSaveMode.ts`. Tests to port: `packages/engine/main/test/object-numbers.test.ts`, `page-residency.test.ts`, `page-residency-host.test.ts`, `page-retention.test.ts`, `opening.test.ts`, `worker-files.test.ts`. Commits: b32fdf584 (`objectNumbers.reserve`, `OBJECT_NUMBER_ISSUE_LIMIT`, `LayerFull`), 630b77eab (`objectNumberFloor`), f550e9e5f (`pages.workingSet`, `effect`).

Prior art on `research/catchup-plugins`: `221fb1477` (the rewrite save mode spelled as `PdfSaveMode`), `6a9c17b60`. Apply hunks that fit with `git show <sha> -- <path> | git apply -3` and name the sha.

`signedDocumentPolicy`: parse it and keep it on the session. Enforcing it needs the signature engine, which is on `research/catchup-plugins` only; 8.24 wires it. Say so in a comment.

Kept divergences to re-check: none in this area on impl. The fonts divergences (`7566552a2`: font bytes as payload 0, fractional or boolean `weight`/`italic` refused as `WireFormat`, NUL in `familyName` refused as `InvalidArg`) live on the plugins branch and are 8.24's.

- [ ] Open kinds take `objectNumbers`, `reserveObjectNumbers` and `signedDocumentPolicy`, and the open answer carries `objectNumbers` and `lastObjectNumber` as upstream's does.
- [ ] `objectNumbers.reserve`: hand the session `count` more numbers; refuse a session that does not issue its own; refuse past `OBJECT_NUMBER_ISSUE_LIMIT` with `LayerFull`. Writes honour `objectNumberFloor`.
- [ ] `pages.workingSet`: take it on arrival, never answer, and set the order parsed pages close in, as WorkerHost does.
- [ ] Save and font kinds: answer with upstream's envelope (`effect`, `opId`); check each answer field against `worker-protocol.ts`.
- [ ] Port the listed upstream tests that exercise these kinds as Rust unit or integration tests.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render (`pnpm --filter @embedpdf/tooling-engine-conformance run operations`).
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test wire_contract --test host_sessions --test document_session --test session_registry --test mutation_finalization --test font_registry` passes, with your new tests.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.3: Page space, page boxes, the page list, destinations and render targets

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.1. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Cross-cutting changes", "Pages", "Actions", "Render"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Ruling (ledger, Task 4b): the vectors record page space, as the TS engine API now answers, and the Rust host ports upstream's `pageSpaceBoundary` so its wire answers are page space too. Page space is x right from the visible box's left edge, y down from its top edge, in PDF units, ignoring `/Rotate`. Readers and writers stay in PDF space; the host converts on the way out and on the way in, in one module. This task builds that module and its first consumers. Each later family adds its own DTO's conversion there, never inside a feature.

Files: a new `crates/epdf-engine/src/page_space.rs` (or `src/page_space/`), `src/geom.rs` (page-space `PageRect {x, y, width, height}`, `PagePoint`, `PageQuad` with `upperLeft`/`upperRight`/`lowerLeft`/`lowerRight`), `src/host.rs` (call the boundary on results and requests), `src/features/pages.rs`, `src/features/actions/destination.rs`, `src/features/actions/document.rs`, `src/features/render/geometry.rs`, `src/features/render/raster.rs`, `crates/pdfium/src/page_list.rs` and `crates/pdfium/src/page_write.rs` (drop the defaults Task 6 kept), `crates/pdfium/tests/documents.rs`, `crates/epdf-engine/tests/pages_corpus.rs`, `tests/action_reader.rs`, `tests/submit_flags.rs`, `tests/render_geometry.rs`, `tests/render_pixels.rs`, `tests/rotation_normalization.rs`, `tests/geometry.rs`, `tests/wire_contract.rs`.

Vectors: `metadata/pages.json`, `metadata/actions.json`, `actions/submit-flags.json`, `actions/types.json`, `render/geometry.json`.

Upstream reference: `packages/engine/core/src/geometry/pageSpace.ts`, `pageBoxes.ts` (`pageBoxesOf`, `pageRotationOf`, `DEFAULT_MEDIA_BOX`), `pageTransform.ts`, `convert.ts`, `primitives.ts`, `packages/engine/core/src/pageSpace/` (`coordinates.ts`, `pages.ts`, `destinations.ts`, `rendering.ts`, `helpers.ts`), `packages/engine/services/src/worker-host/pageSpaceBoundary.ts` (`resultInPageSpace`, `requestInFileSpace`, `pdfRenderTargetOf`), `packages/engine/core/src/dto/PageLayout.ts`, `PageListSnapshot.ts`, `NamedPage.ts`, `PdfAction.ts`, `PdfDestination.ts`, `packages/engine/services/src/features/pages/PagesReader.ts`, `features/actions/`, `features/destinations/`, `features/render/`. Tests to port: `packages/engine/core/test/pageSpace/` (`primitives.test.ts`, `destinations.test.ts`, `helpers.test.ts`), `packages/engine/main/test/page-space.test.ts`. Commits: 60283448c (codec, `pdfCropBox`, action key order), d64920820 and 8b6a2d0e8 (boxes), f5d70b67e (destinations), 7f107fd7c (render targets), 771342379 (`renderMatrix`), bf7d4558e (`tabOrder`), bd4976e4e (`namedPages`, fields never left out), 595bdc6c7 (`ref`).

Carried item owned here (Task 6): the page box defaults and the `/Rotate` reduction move into the engine as upstream's `pageBoxesOf` and `pageRotationOf`. `pageRotationOf` turns a value that is not a multiple of 90 into 0, where the wrapper's `quarter_turns_of` truncates. Read raw boxes through `Document::written_page_box_by_index`, and remove the wrapper defaults in `page_box_by_index`, `page_size_by_index` and `page_rotation_quarter_turns`, with their unit tests in `crates/pdfium/src/page_list.rs` rewritten to the raw contract.

Prior art on `research/catchup-plugins`: `457cdc82d` (named pages read and write, with `crates/pdfium/src/named_pages.rs`). This task needs only the read for `namedPages`. Apply hunks that fit with `git show <sha> -- <path> | git apply -3` and name the sha.

Kept divergences to re-check: none recorded in this area.

- [ ] Page-space types and the transform, ported with upstream's `primitives.test.ts` and `helpers.test.ts` cases as Rust unit tests.
- [ ] `pageBoxesOf`/`pageRotationOf` in the engine, and the wrapper defaults removed (carried item).
- [ ] The boundary module: `result_in_page_space` and `request_in_file_space`, called once in `Host::call`. Port the arms for this task's kinds; leave a match that fails to compile for a result type with no conversion, as upstream's does.
- [ ] `pages.list`: `ref`, `tabOrder`, all five boxes in page space, `pdfCropBox` in PDF space, `namedPages`. A 90-degree page still reads 612 by 792.
- [ ] Actions and destinations: page-space destinations; `mapDocumentActions` key order (triggers before `nameTreeScripts`, `openAction`, `openDestination`).
- [ ] Render kinds take a page-space target and convert with `pdfRenderTargetOf`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test pages_corpus --test action_reader --test submit_flags --test render_geometry --test render_pixels --test rotation_normalization --test geometry --test wire_contract` passes (owns `pages_corpus::the_listing_agrees_with_the_typescript_engine`) and `CARGO_BUILD_JOBS=6 cargo test -p pdfium` passes.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.4: Dates and the metadata patch types

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs nothing. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Cross-cutting changes", Dates; "Security, scope and schema"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except the ts-rs output named below. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Upstream has one `IsoDateTime` type and the `createdAt`/`modifiedAt` names everywhere (fd175ba50). A PDF date reads as ISO 8601 with the offset the file wrote: `+01:00` stays `+01:00`, `Z` or a zero offset reads `Z`, no offset stays none, and there are no milliseconds. A write takes ISO with or without an offset (`IsoDateTimeSchema` is `datetime({ offset: true, local: true })`) and stores the offset given; UTC is written `+00'00'`. Before, metadata normalized to UTC with `.000`, piece info carried `.000`, and attachments passed the raw `D:` string. This task ports the formatter and the patch types; the readers adopt it in 8.5, 8.7, 8.12 and 8.17.

Files: `crates/epdf-engine/src/clock.rs` (`format_pdf_date`, a new `pdf_date_to_iso`), `src/dto.rs` (`IsoDateTime`, `MetadataPatch`, a new `CustomMetadataPatch`), `crates/epdf-engine/tests/schema_parity.rs`, and the ts-rs output `packages/engine/core/src/dto/generated/MetadataPatch.ts` with any new file ts-rs writes beside it. That `packages/**` change is required: ts-rs writes those files from `dto.rs` during `cargo test`, `packages/engine/core/src/dto/DocumentMetadata.ts` imports `DocumentMetadataTrapped` from that directory, and `.github/workflows/engine-rust.yml:97` fails CI when they differ from the Rust types.

Vectors: `schema/MetadataPatch.json`, `schema/CustomMetadataPatch.json`.

Upstream reference: `packages/engine/services/src/shared/pdf-date.ts` (`pdfDateToIso`, `formatPdfDate`), `packages/engine/core/src/dto/IsoDateTime.ts`, `IsoDateTime.schema.ts`, `MetadataPatch.ts`, `CustomMetadataPatch.ts`, `CustomMetadata.ts`, `DocumentMetadata.ts`, `packages/engine/core/src/wire/schemas.ts` (`MetadataPatchSchema`, `CustomMetadataPatchSchema`). Tests to port: `packages/engine/main/test/pdf-date.test.ts`, `dates.test.ts`. Commits: fd175ba50 (dates), a4b8259f7 (custom metadata leaves `MetadataPatch` for its own patch).

Carried items owned here: from Task 2, `dto/generated/MetadataPatch.ts` is stale against upstream (it still has `created`, `modified` and `custom`); from Task 5, `schema_parity.rs` needs `CustomMetadataPatch`, and the date formats per fd175ba50 (offset kept, no `.000`, PDF dates to ISO).

Kept divergences to re-check: none recorded in this area.

- [ ] `pdf_date_to_iso` and `format_pdf_date` ported, with every case of `pdf-date.test.ts` and `dates.test.ts` as a Rust unit test. `IsoDateTime` accepts what `IsoDateTimeSchema` accepts and keeps the offset; nothing normalizes to UTC.
- [ ] `MetadataPatch`: `createdAt` and `modifiedAt`, no `custom`, strict (unknown keys refused). `CustomMetadataPatch`: a map of string to string or null, refused when the map itself is null.
- [ ] `schema_parity.rs`: `check::<MetadataPatch>` against the rewritten cases, and a new `check::<CustomMetadataPatch>` replaying `schema/CustomMetadataPatch.json`.
- [ ] Regenerate the ts-rs files (they are written by `cargo test -p epdf-engine --lib`) and commit them in their own commit.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test schema_parity` passes (owns `metadata_patch_agrees_with_its_zod_schema`) and `cargo test -p epdf-engine --lib` passes. Callers of the renamed fields that 8.5 owns may fail to compile; if they do, rename at the call site only and leave the behaviour to 8.5.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.5: The change queue and metadata

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.1, 8.3 and 8.4. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Cross-cutting changes", `doc.apply`; "Metadata"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay each case's recorded `request` through `Host` with `test_support::replay` (8.1); don't rebuild it from the case fields. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Form effects, page structure edits, attachment writes and metadata are changes on one queue, applied by `document.apply` (2445caba7, 98e5c270c, 119685ea4, c2d7a5194). A change is all or none: one refused op refuses the whole change, and the ops before it are reversed. `document.applyChanges` runs a server request's changes, each in its own layer transaction, with one outcome per change. Each op's places convert from page space right before it runs and its answer back right after (8.3's boundary). This task builds the queue and the mutation meta every write answers with, and ports metadata on it. The family sub-tasks register their own ops.

Files: a new `crates/epdf-engine/src/features/changes/` (applier, ledger, op table, reverse steps), `src/mutation.rs` (upstream's `MutationMeta`: changed refs with their page; `weakRefsInvalidated` and `shouldRefetch` gone), `src/revision.rs`, `src/host.rs`, `src/page_space.rs` (the `document.apply` arms), `src/features/metadata.rs`, `src/features/metadata_write.rs`, `crates/epdf-engine/tests/metadata_corpus.rs`, `tests/metadata_write.rs`, a new `tests/document_apply.rs`, `tests/wire_contract.rs`.

Vectors: `metadata/read.json`, `metadata/update.json`.

Upstream reference: `packages/engine/core/src/mutation/Change.ts`, `changeOutcome.ts`, `MutationMeta.ts`, `WriteOptions.ts`, `MetadataUpdateResult.ts`, `CustomMetadataUpdateResult.ts`, `packages/engine/core/src/wire/changeSchemas.ts`, `packages/engine/services/src/features/changes/` (`ChangeApplier.ts`, `ChangeLedger.ts`, `ChangeRecord.ts`, `recordCodec.ts`, `internal/changeContext.ts`, `internal/metadataChanges.ts`), `packages/engine/services/src/features/metadata/`, `packages/engine/core/src/dto/DocumentMetadata.ts`, `CustomMetadata.ts`, and `worker-protocol.ts` (`DocumentApplyWorkerRequest`, `DocumentApplyChangesWorkerRequest`). Tests to port: `packages/engine/core/test/mutation/change.test.ts`, `packages/engine/main/test/changes.test.ts`, `write-transactions.test.ts`, `metadata.test.ts`. Commits: 2445caba7 (`document.apply`, `authority`), b69c40377, 119685ea4, a4b8259f7 (`metadata.readCustom`, `metadata.updateCustom`, custom key rules), fd175ba50 (dates), bc0057f3a (meta without weak refs).

Op table. Register here only `metadata.update` and `metadata.updateCustom`; every other op type in `Change.ts` answers `NotImplemented` naming its owner until registered: annotation ops 8.14 to 8.16, `annotations.import` and `forms.import`/`forms.importValues` 8.20, `forms.setValue`/`setDisplay`/`setAppearanceText`/`reset` 8.18, the other `forms.*` ops 8.19 (`forms.setSignatureAppearance` 8.24), `outline.*` 8.22, `pages.*` 8.8, `attachments.*` 8.7. Keep that list in one place in the op table so the owners can find it.

Kept divergence to re-check: no-op flatten, redaction and `applyEffects` answers carry `mutationSequence` where TS sends only `{tag, result}` (kept 2026-10-15, plugins `87b61b533`). It lives in the plugins branch's `epdf-viewer-host`, not on impl; note in your report whether the meta you port leaves room for it, and don't add it here.

- [ ] The queue: ops in order, page-space conversion per op, all or none with reverse steps, and `document.applyChanges` with one outcome per change. Port `change.test.ts` and `changes.test.ts` cases that need only metadata ops, as `tests/document_apply.rs`.
- [ ] `MutationMeta` as upstream's, shared by every write answer.
- [ ] `metadata.read` with `createdAt`/`modifiedAt` through `pdf_date_to_iso`, keeping the file's offset (the `rich` case reads `2024-01-15T09:30:00+01:00`); custom keys through `metadata.readCustom`.
- [ ] `metadata.update` and `metadata.updateCustom` as kinds and as ops. Refuse `custom-key-shadowing-a-standard-one`, `custom-key-with-a-slash` and `custom-key-outside-ascii` with `InvalidArg`.
- [ ] `metadata_corpus.rs` and `metadata_write.rs` replay the new shapes; `metadata_write.rs` replays `request`. Upstream's `metadata.read` no longer returns `custom`; the generator folds `metadata.custom.get()` back in (our 1b8c92a3f), so the replay does the same.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test metadata_corpus --test metadata_write --test document_apply --test wire_contract` passes (owns `metadata_agrees_with_the_typescript_engine`, `an_updated_document_reads_back_as_the_typescript_engines_did`, `a_clear_and_an_empty_value_are_the_same_edit`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.6: Security and scope

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs nothing. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Security, scope and schema"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Files: `crates/epdf-engine/src/features/security/state.rs`, `security/reader.rs`, `src/auth/scope/resolver.rs`, `pdf_bits.rs`, `types.rs`, `parser.rs`, `src/host.rs` (envelope of `document.probeSecurityFile`, `document.checkPasswordPermissions`), `crates/epdf-engine/tests/security_state.rs`, `tests/security_reader.rs`, `tests/scope_conformance.rs`, `tests/wire_contract.rs`.

Vectors: `security/state.json`, `scope/resolve.json`, `scope/parse.json`.

Upstream reference: `packages/engine/core/src/auth/scope/` (`resolver.ts` with `filterMatches`, `builders.ts` with `materializePdfPermissions`, `pdf-bits.ts`, `types.ts`), `packages/engine/services/src/features/security/`, `packages/engine/services/src/shared/securityPermissions.ts`. Tests to port: `packages/engine/core/test/auth/`, `packages/engine/main/test/permissions.test.ts`, `security-collab.test.ts`. Commits: 5443a5649 (`canAssemble` with bit 4 or 11; `doc.forms.fill` and `doc.pages.assemble` on more bit sets), a10ed2659 (`doc.text.select`, `doc.text.search`), 4b2cdb395 (`doc.sign`; `filterMatches` drops the membership test), 67ccb76dc (`doc.outline.modify`).

The `group-filter-needs-membership` case keeps its name, but upstream now allows it; its `covers` text says the rule changed (our 016af6547). Follow the vector.

Kept divergences to re-check: none recorded in this area on impl. Plugins `a4ed2b228` (scopes enforced natively, a locked document kept open until unlocked) is on `research/catchup-plugins` and is 8.24's.

- [ ] `materializePdfPermissions` and the advisory: `annotate-and-modify` reads `canAssemble: true`.
- [ ] The resolver: the new capabilities in every expansion, and `filterMatches` without the membership test.
- [ ] Both kinds answer with upstream's envelope; remove them from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test security_state --test security_reader --test scope_conformance --test wire_contract` passes (owns `the_advisory_agrees_with_typescript`, `resolution_agrees_with_typescript`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.7: Attachments and piece info

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.4 and 8.5. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Files"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay recorded requests through `Host` with `test_support::replay` (8.1). No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Files: `crates/epdf-engine/src/features/files/attachments.rs`, `files/piece_info.rs`, `files/mod.rs`, `src/features/changes/` (register `attachments.create` and `attachments.delete`), `src/host.rs`, `crates/epdf-engine/tests/attachment_reader.rs`, `tests/piece_info_reader.rs`, `tests/files_write.rs`, `tests/wire_contract.rs`.

Vectors: `files/attachments.json`, `files/piece-info.json`, `files/write.json`.

Upstream reference: `packages/engine/core/src/dto/Attachment.ts`, `Attachment.schema.ts`, `PieceInfo.ts`, `packages/engine/core/src/mutation/AttachmentMutationResults.ts`, `packages/engine/services/src/features/attachments/`, `packages/engine/services/src/features/changes/internal/attachmentChanges.ts`, `packages/engine/services/src/features/pieceinfo/PieceInfoAccessor.ts`. Tests to port: `packages/engine/main/test/attachments.test.ts`, `pieceinfo.test.ts`. Commits: a9434c0e2 (attachment `ref`, `{attachments: [...]}` listing, `pieceInfo.delete`), c2d7a5194 (attachment writes as change ops), fd175ba50 (`createdAt`/`modifiedAt` as ISO; piece info `modifiedAt` without milliseconds), bd4976e4e (absent fields read `null`).

Prior art on `research/catchup-plugins`: `1c82e6d0c` (pieceInfo writes resolve void). Apply hunks that fit with `git show <sha> -- <path> | git apply -3` and name the sha.

Kept divergences to re-check: none recorded in this area.

- [ ] Attachments: `ref` for `key`, `createdAt` through `pdf_date_to_iso` (`D:20240401090000Z` reads `2024-04-01T09:00:00Z`), `modifiedAt`, `null` for absent fields, `mimeType: null` for a created file with no type, `description` on created entries.
- [ ] `attachments.create` and `attachments.delete` as change ops in the queue.
- [ ] Piece info: `modifiedAt` without milliseconds; `pieceInfo.delete` in place of `pieceInfo.clear`.
- [ ] The three tests replay the new shapes; `files_write.rs` reads `{attachments: [...]}`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test attachment_reader --test piece_info_reader --test files_write --test wire_contract` passes (owns `attachment_reader::the_listing_agrees_with_the_typescript_engine`, `piece_info_reader::the_reads_agree_with_the_typescript_engine`, `a_files_write_leaves_what_the_typescript_engine_left`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.8: Page structure writes

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.3 and 8.5. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Pages", "Extract and save"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay each case's recorded `request` through `Host` with `test_support::replay` (8.1). No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows and the carried notes). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Page structure goes through `document.apply`: `pages.rotate`, `pages.reorder` (was `pages.move`), `pages.delete`, `pages.insertBlank`, `pages.insert` are ops, positions are neighbour positions (`ListPosition`), and `inserted` holds refs. `pages.flatten` stays a kind.

Files: `crates/epdf-engine/src/features/pages_write.rs`, `src/features/assembly.rs`, `src/features/extract.rs`, `src/features/changes/` (register the five page ops), `src/session/pages.rs`, `src/host.rs`, `crates/epdf-engine/tests/pages_mutate.rs`, `tests/page_extract_corpus.rs`, `tests/page_extract.rs`, `tests/wire_contract.rs`.

Vectors: `pages/mutate.json`, `pages/extract.json`.

Upstream reference: `packages/engine/core/src/mutation/` (`PageDeleteInput.ts`, `PageDeleteResult.ts`, `PageInsertBlankInput.ts`, `PageInsertResult.ts`, `PageReorderInput.ts`, `PageReorderResult.ts`, `PageRotateInput.ts`, `PageRotateResult.ts`, `PageFlattenResult.ts`, `PageRestoreResult.ts`, `ListPosition.ts`), `packages/engine/services/src/features/pages/` (`PagesMutator.ts`, `PagesInserter.ts`, `PagesFlattener.ts`, `PagesExtractor.ts`, `internal/`), `packages/engine/services/src/features/changes/internal/pageChanges.ts`. Tests to port: `packages/engine/core/test/mutation/listPosition.test.ts`, `packages/engine/main/test/page-delete.test.ts`, `page-insert.test.ts`, `page-insert-blank.test.ts`, `page-reorder.test.ts`, `page-rotate.test.ts`, `page-flatten.test.ts`. Commits: 2445caba7, 119685ea4, b69c40377 (page structure through `document.apply`), 1233d1349 (neighbour positions; `insert-blank-past-the-end-refused` and `move-past-the-end-refused` gone).

`pages/extract.json` changed only through the PDFium runtime (trailing spaces); `page_extract_corpus` passes at `52f10e027` and must keep passing.

Carried item owned here (Task 4a): the `pages.move`/`pages.insert` invariant notes in `tooling/engine-conformance/src/operations.ts` were not carried onto `document.apply`. Carry them onto the `document.apply` row, or into the op table's doc comment if the row is the wrong place, and say which in the commit message. (8.1 owns the rest of the operations notes.)

Kept divergences to re-check: Swift's `movePages(to: .end)` computes the index in the bindings layer (plugins `17e8ff483`). That is in the Swift bindings on `research/catchup-plugins`, not in this crate; note in your report that `ListPosition`'s `end` makes it moot, or why not.

- [ ] The five page ops in the queue, each converting places as it runs, all or none.
- [ ] `pages.flatten` with page refs in the answer.
- [ ] `pages_mutate.rs` replays `request` and compares `inserted`, `flattened` and `layout` in the new shapes.
- [ ] Remove `pages.flatten` and `pages.extract` from `NOT_YET_ROUTED` if listed; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test pages_mutate --test page_extract_corpus --test page_extract --test wire_contract` passes (owns `a_destination_past_the_end_says_which_end`, `a_page_mutation_leaves_the_layout_the_typescript_engine_left`, `a_refused_mutation_changes_nothing`, `a_refused_page_mutation_is_refused_for_the_same_reason`). A test whose cases upstream removed is deleted, with the reason in the commit message.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.9: Page geometry and text

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.3. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Text and geometry"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows), and `RustPageGeometryReader.ts` only if the step below finds it must change. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

The geometry snapshot was renamed and moved to page space: runs use `start` for `charStart` and a page-space `rect`; glyphs use `loose`/`tight` boxes or quads for `looseBox`/`looseQuad`/`tightBox`/`tightQuad`, and `space`/`empty` booleans for the `flags` bits; a rotated run gives `rotation` in clockwise degrees (0.5236 rad is now 330). The PDFium runtime `1461b0c51` shortens loose boxes (bounded by `/FontBBox`, runtime-src `2558d5d97`, `ff5e04acc`) and drops non-finite text (`06fbf69b3`, probable); Rust links that runtime since Task 6, so those values should come out the same with no Rust change.

The Rust reader returns PDF-space values (upstream's `PageGeometrySnapshot<PdfCoordinates>` with the new names); the host boundary turns them into page space. Two other callers read the Rust reader directly: the C ABI `epdf_page_geometry` in `crates/epdf-wasm`, and through it `packages/engine/services/src/features/geometry/RustPageGeometryReader.ts`, which is called from the TS worker host and so gets converted by the TS boundary.

Files: `crates/epdf-engine/src/features/geometry.rs`, `src/features/text/reader.rs`, `src/page_space.rs` (the `pages.geometry` arm), `src/host.rs`, `crates/epdf-engine/tests/geometry_corpus.rs`, `tests/text_corpus.rs`, `tests/wire_contract.rs`, `crates/test-support/src/corpus.rs` (the comparator, if it names the old fields), `crates/epdf-wasm/src/` (the geometry export, if it reshapes JSON), `crates/epdf-wasm/tests/borrowed_page.rs`.

Vectors: `pages.geometry/*.json` (6 files), `pages.text/*.json` (6 files).

Upstream reference: `packages/engine/core/src/dto/PageGeometrySnapshot.ts`, `PageTextSnapshot.ts`, `packages/engine/core/src/pageSpace/text.ts`, `packages/engine/services/src/features/geometry/PageGeometryReader.ts`, `RustPageGeometryReader.ts`, `packages/engine/services/src/features/text/PageTextReader.ts`, `pageSpaceBoundary.ts` (`pageGeometryOf`). Tests to port: `packages/engine/main/test/page-geometry-reader.test.ts`, `geometry-orientation.test.ts`, `page-text.test.ts`. Commits: a10ed2659, 00d2c8760, 3bad82c8d (names and page space), 6153c181e (clockwise degrees). PDFium: the `runtime-src` submodule at `1461b0c51`.

Carried item owned here (Task 2): `RustPageGeometryReader.ts:58-68` casts the Rust JSON to `PageGeometrySnapshot<PdfCoordinates>`, but the Rust JSON was the old shape. Once the Rust reader emits upstream's PDF-space shape the cast is true and the TS file needs no change. Check it field by field against `PageGeometrySnapshot.ts`; change the TS file only if a field cannot be emitted from Rust, and give the reason in the commit message.

Kept divergences to re-check: none recorded in this area.

- [ ] The reader emits upstream's names in PDF space; the boundary turns them into page space.
- [ ] `geometry_corpus.rs` replays the page-space vectors through the boundary (`flags` is now `space`/`empty`).
- [ ] `borrowed_page.rs`: the C ABI answer is PDF space, so convert it with the engine's page-space transform before comparing with the page-space vectors.
- [ ] `RustPageGeometryReader.ts` checked (carried item).
- [ ] Remove `pages.geometry` and `pages.text` from `NOT_YET_ROUTED` if listed; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test geometry_corpus --test text_corpus --test wire_contract` passes (owns `the_corpus_exercises_both_run_variants_and_empty_glyphs`, `every_corpus_page_matches_the_typescript_reader`) and `CARGO_BUILD_JOBS=6 cargo test -p epdf-wasm --test borrowed_page` passes (owns `the_borrowed_page_export_matches_the_frozen_vectors`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-wasm -p test-support --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-wasm -p test-support`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.10: Text layout in page space

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.9. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Text and geometry", the `text/layout.json` paragraphs, and "Generator fixes in this round"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

`text/layout.json` is now page space, envelope `schemaVersion: 2` (generator fixed in our ea6b5a115). Quads are `PageQuad`s with named corners, segment rects are page boxes, hit probes are page-space points. The generator runs upstream's `createTextLayout`, which mirrors the page-space runs with `mirroredRun`, runs the PDF layout, and flips each answer back; frames are keyed on the y-up runs, so `frameCount` stays 4. Some answers moved with the geometry, not with layout code (for example `report` page 0, glyph 211's line `[197, 211]`, and word range `(207, 6)` drawn as 2 segments); the diff report lists them. They are not layout bugs to chase.

Files: `crates/epdf-engine/src/features/text/layout.rs`, `src/features/text/mod.rs`, `crates/epdf-engine/tests/text_layout.rs`, `tests/charmap.rs` (unchanged vector, keep passing).

Vectors: `text/layout.json`, `text/charmap.json`.

Upstream reference: `packages/engine/core/src/text/layout.ts` (`createPdfTextLayout`), `packages/engine/core/src/pageSpace/text.ts` (`createTextLayout`, `mirroredRun`), `packages/engine/core/src/text/TextRange.ts`, `charmap.ts`. Tests to port: `packages/engine/core/test/pageSpace/text.test.ts`, `packages/engine/core/test/text/`. Commits: a10ed2659, 00d2c8760, 3bad82c8d; our ea6b5a115.

Carried item owned here (Task 5): `text_layout.rs` moves to page space and `schemaVersion` 2.

Kept divergences to re-check: none recorded in this area.

- [ ] A page-space layout entry that mirrors runs as `createTextLayout` does, over the existing PDF-space layout.
- [ ] `text_layout.rs` reads `schemaVersion` 2, named-corner quads, page-box segments and page-space probes.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test text_layout --test charmap` passes (owns all 13 `text_layout` tests: `a_degenerate_glyph_has_no_quad_and_a_real_one_does`, `glyph_quads_and_ranges_agree_with_typescript`, `an_empty_range_produces_no_segments`, `a_hit_at_a_glyphs_own_centre_finds_a_glyph`, `the_layout_shape_agrees_with_typescript`, `every_segment_rect_bounds_its_own_quad`, `the_run_and_glyph_counts_are_consistent`, `the_corpus_covers_more_than_one_orientation_frame`, `a_hit_far_outside_the_page_finds_nothing`, `segments_agree_with_typescript`, `a_word_can_cross_a_line_boundary_between_abutting_runs`, `word_and_line_ranges_contain_their_glyph_and_stay_on_the_page`, `hit_tests_agree_with_typescript`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.11: Search

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.3 and 8.9. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Search"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay each slice-protocol case's recorded `request`; don't rebuild it from `case`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's row). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

A range's `length` is `count`; a snippet is `{before, match, after}`; slice keys spell the mode `snippets` or `matches` and carry `ignoreWhitespace`; `search.query` takes a flattened `SearchScanRequest` (query fields at the top level, `snippets`, `from` as a `PageRef`, the cursor string); the cursor key is upstream's `searchQueryKey`, encoded with `encodeSearchCursor`; a conflicting start page is refused with "`from` conflicts with the cursor". Match rects are page space (3bad82c8d).

Files: `crates/epdf-engine/src/features/search/` (`mod.rs`, `cursor.rs`, `fold.rs`, `literal.rs`, `dialect.rs`, `snippet.rs`, `matcher.rs`, `reader.rs`), `src/page_space.rs` (the `search.query` arm, `pageSearchSliceOf`), `src/host.rs`, `crates/epdf-engine/tests/search_dialect.rs`, `tests/search_slice_protocol.rs`, `tests/search_reader.rs`, `tests/wire_contract.rs`.

Vectors: `search/fold.json`, `search/literal.json`, `search/regex-dialect.json`, `search/snippet.json`, `search/slice-protocol.json`.

Upstream reference: `packages/engine/core/src/search/` (`fold.ts`, `literal.ts`, `regex.ts`, `snippet.ts`, `matcher.ts`, `types.ts`, `epoch.ts`), `packages/engine/services/src/features/search/` (`SearchReader.ts`, `internal/searchCursor.ts` with `searchQueryKey` and `encodeSearchCursor`). Tests to port: `packages/engine/core/test/search/`, `packages/engine/core/test/wire/searchToken.test.ts`, `packages/engine/main/test/search.test.ts`, `search-ignore-whitespace.test.ts`. Commits: a10ed2659 (the reshape), f082bf3e4 (`ignoreWhitespace`, before the merge base), 3bad82c8d (page-space rects); our 0963cacd3 and 26887941a (recorded `request`, minted cursors).

Prior art on `research/catchup-plugins`: `29fe12ddb` (the `ignoreWhitespace` flag in the Rust engine). Apply hunks that fit with `git show <sha> -- crates/epdf-engine | git apply -3` and name the sha.

Carried item owned here (Task 5): `search_slice_protocol.rs` reads the flattened `request`. Task 5 also noted that no slice-protocol case mints a cursor with mode `matches` (the `snippets: false` key path); add a Rust unit test for that key.

Kept divergences to re-check: none recorded in this area. (S4 and S6 in the review's Lows are shared with TS and are plugin-side.)

- [ ] `count`, the snippet shape and the key spelling in the dialect, fold and literal code.
- [ ] The flattened request, `from` as a `PageRef`, upstream's cursor key and its refusals.
- [ ] Page-space match rects at the boundary.
- [ ] Remove `search.query` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test search_dialect --test search_slice_protocol --test search_reader --test wire_contract` passes (owns `snippets_agree_with_typescript`, `the_fold_vectors_keep_the_properties_the_anchor_stage_rests_on`, `folded_ranges_translate_back_the_same_way`, `the_fold_agrees_with_typescript`, `the_rust_engine_reproduces_the_javascript_dialect`, `the_dialect_corpus_covers_what_the_entry_gate_asks_for`, `literal_search_agrees_with_typescript`, `a_snippet_locates_its_match_inside_itself`, `every_slice_protocol_case_agrees_with_the_oracle`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.12: Annotation read, vocabulary and the save round trip

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.3 and 8.4. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Annotation": the shared record table, `read.json`, `vocabulary.json`; "Extract and save"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Every annotation record changed shape. From the diff report's table: `rect` and every point in page space (1a243431a); colours as `"#rrggbb"` (c053c3d33); quad corners `upperLeft/upperRight/lowerLeft/lowerRight` (00d2c8760); `rotation` in clockwise degrees, a stored 30 reads 330 (2bdc74fbf), and taken from the appearance through `appearanceTurnOf`, not from `/EMBD_Metadata /Rotation` (2bdc74fbf, 6153c181e); box kinds carry `box`, and `unrotatedRect` and `rectDifferences` are gone (2bdc74fbf); `ref` is an `AnnotationRef` (595bdc6c7, bc0057f3a); flags flattened to top-level booleans (c78997346); `createdAt`/`modifiedAt` (fd175ba50); `reply`, `popup`, `importedBy`, `modifiedBy`, and `null` for empty `createdBy`/`userId`/`groupId` (c78997346); `hasAppearance` and `appearanceState` (25dd21f38, 17350fb4c); `actions` always present and no field left out (bd4976e4e); opacity as written (bd4976e4e). `annotations.list` replaces the three `listRaw*`/`listFullPage` kinds and returns no widgets (83f6598d2, 1233d1349). `/Popup` reads as `popup` with `parent` and `open`. There is no `pageState` in a read. The `stampName` codec is gone (0427e3dc9) and `/Name` passes verbatim; `FreeTextTypeWriter` decodes as `free-text-typewriter` (404b79b3e). FreeText records carry `richText`, and records carry `measure`.

Files: `crates/epdf-engine/src/features/annotation/read/` (all of it), `annotation/vocabulary.rs`, `annotation/flags.rs`, `annotation/intent.rs`, `annotation/state.rs`, `annotation/subtype.rs`, `src/revision.rs` (drop the read's `PageState`/weak state if nothing else needs it), `src/identity.rs`, `src/page_space.rs` (`pageAnnotationOf`), `src/host.rs`, `crates/epdf-engine/tests/annotation_reader.rs`, `tests/annotation_vocabulary.rs`, `tests/document_save.rs`, `tests/wire_contract.rs`.

Vectors: `annotation/read.json`, `annotation/vocabulary.json`, `save/roundtrip.json`.

Upstream reference: `packages/engine/core/src/annotation/` (`base.ts`, `defaults.ts`, `appearanceTurn.ts`, `drawnPoints.ts`, `kinds/`, `color.ts`, `relationships.ts`), `packages/engine/core/src/pageSpace/annotations.ts`, `pageSpace/helpers.ts` (`appearanceTurnOf`), `packages/engine/core/src/dto/RichText.ts`, `Measure.ts`, `packages/engine/core/src/identity/AnnotationRef.ts`, `annotationKey.ts`, `packages/engine/services/src/features/annotations/` (`AnnotationReader.ts`, `RawAnnotationReader.ts`, `internal/`), `packages/engine/services/src/features/save/`. Tests to port: `packages/engine/core/test/annotation/`, `packages/engine/core/test/pageSpace/annotations.test.ts`, `packages/engine/main/test/annotations.test.ts`, `annotation-rotation.test.ts`, `rich-text.test.ts`, `color.test.ts`. Commits: as listed above, plus 83f6598d2 and 1233d1349 for `annotations.list`.

Prior art on `research/catchup-plugins`: `347f806ae`, `3b795d4ac`, `818eba296` (FreeText rich text read and write, `crates/pdfium/src/fonts.rs`), `635f7d303` (measure dictionaries, `crates/pdfium/src/measure.rs`). Apply the read hunks that fit with `git show <sha> -- <path> | git apply -3` and name the sha; 8.14 and 8.15 take the write hunks.

Kept divergences to re-check, from the FreeText rich-text port (`818eba296`, kept 2026-10-14): the reader resolves registered fonts through a thread-local copy of the font registry, rich-text input styles stay untyped JSON, and a font identity falls back to its registration name when PDFium has none. If you take that code, keep those three, and say so in your report. Also part of N6 (`343fc063e`): a ref of unknown kind fails rather than keying `"undefined"`; that is in `crates/plugins/` on the plugins branch, so only check that the engine's ref decoding refuses an unknown kind too.

- [ ] The record in upstream's shape, read in PDF space and converted at the boundary.
- [ ] `annotations.list` (no widgets) and `annotations.readFile`; the three removed list kinds' feature code deleted or folded in.
- [ ] Vocabulary: drop `stampName`, add `free-text-typewriter`. `annotation_vocabulary.rs` stops asking for the `stampName` map.
- [ ] `annotation_reader.rs` replays the new record; widgets are compared through `forms.list`, as the generator reads them (our 0044ea9b9). Tests whose subject is gone (`the_page_state_agrees_including_the_weak_flag`) are rewritten to what replaced it, or deleted with the reason in the commit message.
- [ ] `document_save.rs`: `annotationCount` on `annotation_kinds.pdf` is 37.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_reader --test annotation_vocabulary --test document_save --test wire_contract` passes (owns `the_corpus_covers_every_subtype_that_has_a_reader`, `the_page_state_agrees_including_the_weak_flag`, `a_widget_is_joined_to_the_field_that_owns_it`, `a_rect_is_reported_in_the_corner_order_the_file_wrote`, `a_reply_edge_carries_a_type_exactly_when_it_carries_a_parent`, `annotation_reader::the_reads_agree_with_the_typescript_engine`, `the_subtypes_with_no_reader_read_as_unsupported_on_both_sides`, `the_it_names_decode_what_typescript_does`, `the_name_maps_decode_and_encode_what_typescript_does`, `a_saved_document_reopens_as_the_typescript_engines_did`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.13: Annotation appearances

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.12. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Annotation", `appearances.json`). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Entries are addressed by `baseIndex` refs and carry page-space rects. The renderer returns every mode and state (17350fb4c), so `normal` and `scaled` gain rollover and down entries (7 become 11). Annotations without `/AP` are drawn in memory (25dd21f38, probable), so `no-appearances` goes from 0 entries to 34. Each case's eleventh entry is a widget, reached through `page.forms.renderAppearancesRaw` (`tooling/engine-conformance/src/generate-annotation-appearance-vectors.ts:118`); find the wire kind that sends in upstream's `packages/engine/main` and route it if it is one of this task's kinds. `annotations.readAppearance` is new (7f0b0a183).

Files: `crates/epdf-engine/src/features/annotation/appearance.rs`, `src/features/render/raster.rs`, `src/page_space.rs` (`pageAppearancesOf`), `src/host.rs`, `crates/pdfium/src/page.rs` (only if the render call needs it), `crates/epdf-engine/tests/annotation_appearance.rs`, `tests/wire_contract.rs`.

Vector: `annotation/appearances.json`.

Upstream reference: `packages/engine/services/src/features/annotations/AnnotationAppearanceReader.ts`, `packages/engine/core/src/dto/AnnotationRender.ts`, `packages/engine/core/src/pageSpace/rendering.ts`, `packages/engine/core/src/annotation/appearanceTurn.ts`, `packages/engine/services/src/worker-host/pageSpaceBoundary.ts` (`pageAppearancesOf`). Tests to port: `packages/engine/main/test/annotations-appearance.test.ts`, `appearance-states.test.ts`, `appearance-clamp.test.ts`. Commits: 17350fb4c, 25dd21f38, 7f107fd7c, 7f0b0a183, 1233d1349, 5e9241945 (runtime: the turn comes from the caller).

Stub removed: `crates/epdf-engine/src/features/annotation/appearance.rs:258` (`// re-port (Task 8): EPDF_RenderAnnotBitmapUnrotated now takes the turn`). Pass the turn and unrotated box from `appearanceTurnOf` through `PageView::render_annotation_rgba_with_matrix(state, Some(AppearanceTurn { degrees, unrotated_box }))`.

Carried item owned here (Task 6): a turned box annotation is skipped silently in `render_one` with no per-annotation skip record. Removing the stub ends the skip; if any other annotation is still skipped, record it the way upstream's reader does, or say in your report why upstream also skips silently.

Kept divergences to re-check: none recorded in this area.

- [ ] Every mode and state, in-memory appearances for annotations without `/AP`, `baseIndex` refs, page-space rects.
- [ ] Turned box annotations render (stub removed).
- [ ] `annotations.readAppearance` routed.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_appearance --test wire_contract` passes (owns all 10 `annotation_appearance` tests: `a_stripped_appearance_is_drawn_flat_and_fills_its_logical_box`, `a_rotated_box_kind_is_placed_by_its_logical_box_and_the_near_misses_are_not`, `a_scale_multiplies_the_raster_and_leaves_the_placement_alone`, `an_annotation_with_no_appearance_stream_is_absent_rather_than_empty`, `each_mode_draws_its_own_appearance_stream`, `the_appearances_land_where_the_typescript_engine_put_them`, `generating_a_form_fields_appearance_never_reaches_the_reader`, `the_per_appearance_pixel_clamp_reduces_the_scale_rather_than_refusing`, `an_appearance_is_drawn_from_the_annotations_own_place_on_the_page`, `only_the_modes_an_annotation_has_are_emitted`). A test whose premise upstream reversed (for example "absent rather than empty" now that missing `/AP` is drawn) is rewritten to upstream's rule, with the reason in the commit message.
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/annotation/appearance.rs` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.14: Annotation create

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.5 and 8.12. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Annotation", `create.json`). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's row). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Ruling (ledger, Task 4b): the create vector records the v3 page-space draft actually sent, not the old spelling, and Rust replays it as the wire request. 63 cases became 57: the six `widget-*` cases are gone because the annotation verbs refuse widgets (1233d1349); `square-degenerate-cloudy` and `polygon-zero-cloud` are refused with `InvalidArg` (c78997346). A created annotation reads `print: true` from the defaults table (69627beb9, probable) and gets a UUID `/NM` at birth, recorded as `"<stamped>"`. Byte inputs moved from `resources.r0` to `resources.appearance` / `resources.file` (11a1259d9). The answer's meta is 8.5's `MutationMeta`.

Files: `crates/epdf-engine/src/features/annotation/write/create.rs`, `write/draft.rs`, `write/resolve.rs`, `write/transform.rs`, `write/context.rs`, `write/appearance.rs`, `write/mutator.rs`, `src/features/changes/` (register `annotations.create`), `src/page_space.rs` (`pdfAnnotationDraftOf`), `src/host.rs`, `crates/pdfium/src/annot_write.rs` (only for `/RD` and opacity), `crates/epdf-engine/tests/annotation_create.rs`, `tests/not_ported_refusals.rs`, `tests/wire_contract.rs`.

Vector: `annotation/create.json`.

Upstream reference: `packages/engine/core/src/pageSpace/helpers.ts` (`resolveAnnotationDraft`, `resolveRectCommand`), `packages/engine/core/src/annotation/` (`defaults.ts`, `shapeForRect.ts`, `drawnPoints.ts`, `resolve/`, `kinds/`), `packages/engine/core/src/mutation/AnnotationMutationResults.ts`, `packages/engine/services/src/features/annotations/AnnotationMutator.ts`, `AnnotationBatchApplier.ts`, `internal/`, `packages/engine/services/src/features/changes/internal/annotationChanges.ts`. Tests to port: `packages/engine/main/test/annotations-mutations.test.ts`, `annotation-rect.test.ts`, `annotation-resources.test.ts`, `annotation-write-checks.test.ts`, `annotations-stamp.test.ts`. Commits: c78997346, 69627beb9, 11a1259d9, 1233d1349, 1a243431a, 5f4686dc8 and bc0057f3a (the `/NM` stamp candidates), 5e9241945 (runtime: the engine owns `/RD`).

Stubs removed: `crates/epdf-engine/src/features/annotation/write/create.rs:192` (preflight refusal), `:641` (`apply_caret`), `:799` (`apply_free_text`), `:1652` (`apply_shape`), `:1804` (helper `rect_differences_not_ported`). Upstream's runtime no longer has `EPDFAnnot_SetRectangleDifferences` (5e9241945: "the engine owns /RD"); write `/RD` the way upstream's TS does now. Also delete `a_create_with_rect_differences_is_refused_before_the_annotation_exists` from `tests/not_ported_refusals.rs` (its subject is gone); leave the rest of the file to 8.15 and 8.19.

Carried item owned here (Task 6): `pdfium::Annotation::set_opacity` clamps to 0..1, while upstream passes the value through and lets native refuse it. Match upstream: drop the clamp and surface the native refusal as the error upstream gives.

Prior art on `research/catchup-plugins`: the write hunks of `3b795d4ac` (FreeText rich text on create). Apply with `git show <sha> -- <path> | git apply -3` where they fit, and name the sha.

Kept divergences to re-check: the rich-text ones listed in 8.12 (`818eba296`), if you take that code.

- [ ] Draft resolution from page space, the defaults table, `/NM` at birth, `resources.appearance`/`resources.file`, widget refusal, the two degenerate refusals.
- [ ] `/RD` written by the engine (stubs removed), opacity unclamped.
- [ ] `annotations.create` as a kind and as a change op.
- [ ] `annotation_create.rs` replays the v3 drafts and compares `created` and `meta`.
- [ ] Remove `annotations.create` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_create --test not_ported_refusals --test wire_contract` passes (owns all 11 `annotation_create` tests: `a_created_annotation_survives_a_save_and_reopen`, `a_create_is_append_only_and_says_so`, `a_review_state_is_written_in_the_spelling_the_pdf_uses`, `a_rotated_stamp_is_authored_in_the_frame_it_is_turned_from`, `a_redaction_gets_a_default_appearance_only_when_it_has_a_label`, `a_stamp_fit_policy_decides_where_the_picture_lands`, `a_created_annotation_reads_back_as_the_typescript_engines_did`, `a_draft_that_cannot_be_written_is_refused_before_anything_is_written`, `a_created_annotation_ships_with_an_appearance`, `the_transform_keys_land_in_the_dictionary_and_not_only_in_the_dto`, `a_new_annotation_is_addressable`), and `CARGO_BUILD_JOBS=6 cargo test -p pdfium` passes.
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/annotation/write/create.rs` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.15: Annotation update and the write verdicts

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.14. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Annotation", `update.json` and `write.json`). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's row). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

`update.json`: 44 cases became 41 (`caret-clear-rect-differences` went with the inset, 2bdc74fbf; `widget-font-size-only` and `widget-restyle` with widgets, 1233d1349). `polygon-degenerate` is refused with `InvalidArg` (404b79b3e). Two cases swap their appearance verdict. Free-text updates read `contents: ""` (69627beb9). The vector records the v3 patch actually sent (ruling, Task 4b). `write.json`: the 8 `mutations` cases are gone, because upstream removed weak refs and `computeMutationImpact` (bc0057f3a). Of the 29 `appearance` cases, three move from `regenerate` to `translation`, because the update resolves a rect patch into the shape before classifying (9d8ebf857; those cases record `resolvedPatch`), and `ink-advisory-rotation` moves from `inert` to `regenerate`, because rotation now turns a vertex kind's upright points (6153c181e).

Files: `crates/epdf-engine/src/features/annotation/write/update.rs`, `write/patch.rs`, `write/resolve.rs`, `write/transform.rs`, `write/appearance.rs`, `write/impact.rs` (delete), `write/mod.rs`, `src/features/changes/` (register `annotations.update`), `src/page_space.rs` (`pdfAnnotationPatchOf`), `src/host.rs`, `crates/epdf-engine/tests/annotation_update.rs`, `tests/annotation_write.rs`, `tests/not_ported_refusals.rs`, `tests/wire_contract.rs`.

Vectors: `annotation/update.json`, `annotation/write.json`.

Upstream reference: `packages/engine/core/src/pageSpace/helpers.ts` (`resolveAnnotationPatch`, `resolveRectCommand`), `packages/engine/core/src/annotation/shapeForRect.ts`, `resolve/`, `appearanceTurn.ts`, `drawnPoints.ts`, `packages/engine/services/src/features/annotations/AnnotationMutator.ts`, `internal/`. Tests to port: `packages/engine/main/test/annotations-rotation.test.ts`, `annotation-rotation.test.ts`, `annotations-stamp-resize.test.ts`, `annotations-text-preservation.test.ts`. Commits: 9d8ebf857, 6153c181e, 2bdc74fbf, 404b79b3e, 69627beb9, bc0057f3a, 5e9241945.

Stubs removed: `crates/epdf-engine/src/features/annotation/write/update.rs:250` (preflight refusal), `:847` (`apply_shape_patch`), `:998` (`patch_rect_differences`), `:1413` (helper). `rectDifferences` is no longer a patch field upstream (2bdc74fbf); port what upstream's update does to `/RD` when a box kind's rect or shape changes. Delete `an_update_with_rect_differences_is_refused_before_the_identity_stamp` from `tests/not_ported_refusals.rs`.

Prior art on `research/catchup-plugins`: the update hunks of `3b795d4ac` and `818eba296` (FreeText rich text on update). Apply where they fit, and name the sha.

Kept divergences to re-check: the rich-text ones listed in 8.12 (`818eba296`), if you take that code.

- [ ] Patch resolution from page space into the shape, then the verdict; `annotations.update` as a kind and as a change op.
- [ ] `/RD` stubs removed; weak-ref impact code and its tests deleted (`the_mutation_impacts_agree_with_the_typescript_engine`, `an_index_shifting_mutation_needs_a_scanned_page`: their cases are gone upstream; say so in the commit message).
- [ ] `annotation_update.rs` replays the v3 patches; `annotation_write.rs` replays the `appearance` cases, including `resolvedPatch`.
- [ ] Remove `annotations.update` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_update --test annotation_write --test not_ported_refusals --test wire_contract` passes (owns `an_update_is_not_structural`, `an_updated_annotation_reads_back_as_the_typescript_engines_did`, `a_moved_stamp_refits_the_picture_it_already_had`, `the_appearance_verdict_agrees_with_the_typescript_engine`, `a_patch_for_the_wrong_subtype_is_refused`, `a_patched_review_state_is_written_in_the_spelling_the_pdf_uses`, `the_appearance_verdicts_agree_with_the_typescript_engine`, and the two deleted `annotation_write` tests above).
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/annotation/write/update.rs` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.16: Annotation delete and reorder

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.5 and 8.12. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Annotation", `delete.json` and `move.json`). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay each case's recorded `request` through `Host` with `test_support::replay` (8.1). No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Deleting an annotation deletes its thread and popups in one change (bd4976e4e): `by-object-number` and `by-index-onto-a-durable-annotation` also remove `a-reply`, `a-group-member` and `stamped-by-embedpdf`, so `remaining` drops from 8 to 5 and `meta.changed` lists 4 refs. The `deleted` field is gone. `annotations.reorder` replaces `annotations.move` (1233d1349): `movedIndices`/`movedOrder` become `order`, and the move no longer stamps `/NM` (bc0057f3a), so `afterOrder` entries the old engine named are `null` or `""`.

Files: `crates/epdf-engine/src/features/annotation/write/delete.rs`, `write/move_.rs` (rename to reorder), `write/mutator.rs`, `src/features/annotation/read/relationship.rs`, `src/features/changes/` (register `annotations.delete` and `annotations.reorder`), `src/revision.rs`, `src/identity.rs`, `src/host.rs`, `crates/epdf-engine/tests/annotation_delete.rs`, `tests/annotation_move.rs`, `tests/wire_contract.rs`.

Vectors: `annotation/delete.json`, `annotation/move.json`.

Upstream reference: `packages/engine/core/src/annotation/relationships.ts`, `packages/engine/core/src/mutation/AnnotationMutationResults.ts`, `AnnotationListMutationMeta.ts`, `ListPosition.ts`, `packages/engine/services/src/features/annotations/AnnotationMutator.ts`, `packages/engine/services/src/features/changes/internal/annotationChanges.ts` (`deleteAnnotation`, `reorderAnnotations`, `reorderBack`, `restoreAnnotations`). Tests to port: `packages/engine/main/test/annotations-mutations.test.ts` (delete and reorder cases), `packages/engine/core/test/mutation/listPosition.test.ts` if 8.8 has not. Commits: bd4976e4e, 1233d1349, bc0057f3a, 595bdc6c7.

Kept divergences to re-check: none recorded in this area.

- [ ] Delete with its thread and popups, in one change, with the reverse step for all or none.
- [ ] `annotations.reorder` with `ListPosition`; `move_.rs` renamed; no `/NM` stamping.
- [ ] Both as kinds and as change ops; both tests replay `request`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_delete --test annotation_move --test wire_contract` passes (owns `a_reference_that_names_nothing_is_an_invalid_reference`, `a_delete_removes_the_annotation_the_typescript_engine_removed`, `a_move_is_structural_and_says_so`, `the_block_lands_where_it_was_asked_to_in_the_order_it_was_given`, `a_move_that_changes_nothing_is_still_structural`, `a_move_refuses_what_it_cannot_do_and_changes_nothing`, `a_move_leaves_the_order_the_typescript_engine_left`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.17: Form read

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.3 and 8.4. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Forms", `snapshot.json` and the field record notes under `field.json`). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's row). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Field records: `fieldObjectNumber` folds into `ref.objectNumber` (595bdc6c7); `flags` flattens to `readOnly`/`required`/`noExport` (c3b90c3a4); `createdBy`, `createdAt`, `filledBy`, `filledByName`, `filledAt`, `importedBy`, `groupId` are new (b43963d12), with dates through `pdf_date_to_iso` (8.4); widgets carry `ref`, `page` and `objectNumber` for `annotObjectNumber`/`pageObjectNumber` (595bdc6c7). The snapshot gains a top-level `widgets` list (1233d1349), `topIndex` and `defaultValue` where they apply (bf7d4558e), and `calculationOrder[].objectNumber`. Places are page space (c053c3d33).

Files: `crates/epdf-engine/src/features/forms/snapshot.rs`, `forms/mod.rs`, `src/page_space.rs` (`pageFormSnapshotOf`, `pageFormFieldOf`, `pageWidgetOf`), `src/host.rs`, `crates/epdf-engine/tests/form_reader.rs`, `tests/wire_contract.rs`.

Vector: `forms/snapshot.json`.

Upstream reference: `packages/engine/core/src/forms/` (`field.ts`, `snapshot.ts`, `widgets.ts`, `value-entry.ts`), `packages/engine/core/src/identity/FormFieldRef.ts`, `packages/engine/core/src/pageSpace/forms.ts`, `packages/engine/services/src/features/forms/FormReader.ts`, `internal/`. Tests to port: `packages/engine/core/test/forms/`, `packages/engine/main/test/forms.test.ts`, `field-groups.test.ts`, `form-attribution.test.ts`, `widget-finding.test.ts`. Commits: 595bdc6c7, c3b90c3a4, b43963d12, 1233d1349, bf7d4558e, c053c3d33.

Kept divergences to re-check: none recorded in this area.

- [ ] The field, widget and snapshot records in upstream's shape, converted at the boundary.
- [ ] `form_reader.rs` replays them.
- [ ] Remove `forms.list` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test form_reader --test wire_contract` passes (owns `the_snapshot_agrees_with_the_typescript_engine`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.18: Form values, reset, repair and apply

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.5 and 8.17. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Forms", `effects.json`, `apply.json`, `write.json`). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay each case's recorded `request` through `Host` with `test_support::replay` (8.1). No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Ruling (ledger, Task 4b, form-apply): upstream removed `forms.applyEffects` (98e5c270c). `forms/apply.json` drives the same 23 script-effect cases through `doc.apply` ops (`forms.setValue`, `forms.reset`, `forms.setDisplay`), all or none, with no per-effect status. A case that used to report a rejected effect is now a whole-change refusal; `a-push-button-cannot-be-reset` and `reset-with-no-fields-is-rejected` now apply as an item with no changed fields; `a-rejection-does-not-stop-the-batch`, `an-all-no-op-batch-produces-no-meta` and `a-no-op-in-the-middle-does-not-stop-the-batch` refuse whole (`InvalidArg`, `NotFound`, `NotFound`), and their `covers` text says so (our 016af6547). `forms/write.json`: `documentCases` went from 10 to 2 (the FDF/XFDF cases went with form bundles, 7f0b73a3c); the repair report says `needsAppearancesCleared` (7bccc4f35); `direct-field-refused` is `NotFound`; `text-reset-no-default` reports no changed widget.

The runtime `1461b0c51` removed `EPDFForm_SetTextValue`, `SetToggle`, `SetChoiceValues`, `ResetField`, `ImportFDF` and `ImportXFDF` (runtime 69e229992). Task 6 bound the replacements: `Document::set_field_value(objnum, FieldValueWrite)`, `redraw_field_widgets`, `set_field_widgets_checked`, `set_field_selected_indices`, `set_field_default_value`. Upstream resets by writing `/DV` back through the value writers.

Files: `crates/epdf-engine/src/features/forms/write.rs`, `forms/effects.rs` (fold into the change ops or delete), `src/features/changes/` (register `forms.setValue`, `forms.setDisplay`, `forms.setAppearanceText`, `forms.reset`), `src/host.rs`, `crates/epdf-engine/tests/form_write.rs`, `tests/form_effects.rs` (replace with a new `tests/form_apply.rs`), `tests/wire_contract.rs`.

Vectors: `forms/apply.json`, `forms/write.json`.

Upstream reference: `packages/engine/core/src/forms/` (`write.ts`, `effects.ts`, `predict.ts`, `value-entry.ts`), `packages/engine/core/src/mutation/FormMutationResults.ts`, `Change.ts`, `packages/engine/services/src/features/forms/FormMutator.ts`, `internal/`, `packages/engine/services/src/features/changes/ChangeApplier.ts`, `internal/formChanges.ts` (`setValue`, `setDisplay`, `setAppearanceText`, `reset`). Tests to port: `packages/engine/main/test/forms.test.ts` (value and reset cases), `form-prediction.test.ts`, `form-scripts.test.ts`, `widget-appearance-refresh.test.ts`. Commits: 98e5c270c, 7f0b73a3c, 7bccc4f35, 69e229992 (runtime), 2445caba7.

Stubs removed: `crates/epdf-engine/src/features/forms/write.rs:175` (`set_value`), `:199` (`reset`), `:245` (`read_back`, `#[allow(dead_code)]`), `:320` (`import_data`: FDF and XFDF are gone upstream, so delete it; bundle import is 8.20's), `crates/epdf-engine/src/features/forms/effects.rs:327` (`preflight_inner` refusal) and `:501` (the unreachable `apply_native` arm).

Tests whose cases upstream removed: `an_import_applies_what_the_typescript_engine_applied` and `an_unreadable_payload_is_refused` replayed the FDF/XFDF `documentCases`, and none is left; delete them with the reason in the commit message. `every_effects_batch_agrees_with_the_oracle` read the deleted `forms/effects.json`; `tests/form_apply.rs` replaces it and replays `forms/apply.json`.

Kept divergences to re-check: the no-op `applyEffects` answer carrying `mutationSequence` (plugins `87b61b533`, kept 2026-10-15) has no kind left to live on, since `forms.applyEffects` is gone. Record that in your report so 8.24 drops it rather than ports it.

- [ ] `set_value` and `reset` on the new runtime writers; `read_back` used again.
- [ ] The four form value ops in the queue, all or none; `forms.setValue` and `forms.reset` as kinds take `refs` and answer with `MutationMeta`.
- [ ] `forms.repair` reports `needsAppearancesCleared`.
- [ ] `tests/form_apply.rs` replays `forms/apply.json`; `tests/form_effects.rs` deleted; `form_write.rs` replays `request`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test form_write --test form_apply --test wire_contract` passes (owns `a_written_field_reads_back_as_the_typescript_engines_did`, `a_form_model_is_a_snapshot_and_a_write_makes_it_stale`, `a_repair_reports_what_the_typescript_engine_reported`, `a_refused_write_is_refused_for_the_same_reason`, the two deleted import tests, and `every_effects_batch_agrees_with_the_oracle` through its replacement).
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/forms/write.rs crates/epdf-engine/src/features/forms/effects.rs` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.19: Form fields and widgets

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.15 and 8.18. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Forms", `field.json`). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay each case's recorded `request` through `Host` with `test_support::replay` (8.1). No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

`forms/field.json`: 25 cases became 23 (`attach-already-attached-refused` and `attach-off-state-refused` went with `attachWidget`, d7bd2d277). `delete-merged-field-refused` now succeeds: a merged field and its widget delete in one change (60602f579). New kinds and ops: `forms.addWidget` (2445caba7), `forms.updateWidget`, `forms.deleteWidget`, `forms.reorderWidgets` (1233d1349), `forms.reorderCalculations` (98718b780), and the op-only `forms.create`, `forms.update`, `forms.delete`, `forms.removeWidget`.

Files: `crates/epdf-engine/src/features/forms/field.rs`, `forms/snapshot.rs` (widget placement), `src/features/changes/` (register the field and widget ops), `src/page_space.rs` (`pdfFormFieldDraftOf`, `pdfWidgetPlacementOf`), `src/host.rs`, `crates/pdfium/src/form_write.rs` (only if a binding is missing), `crates/epdf-engine/tests/form_field.rs`, `tests/not_ported_refusals.rs` (delete), `tests/wire_contract.rs`.

Vector: `forms/field.json`.

Upstream reference: `packages/engine/core/src/forms/field.ts`, `widgets.ts`, `packages/engine/core/src/pageSpace/forms.ts`, `packages/engine/core/src/mutation/FormMutationResults.ts`, `packages/engine/services/src/features/forms/FormMutator.ts`, `internal/`, `packages/engine/services/src/features/changes/internal/formChanges.ts` (`createField`, `updateField`, `deleteField`, `addWidget`, `removeWidget`, `deleteWidget`, `updateWidget`, `reorderWidgets`, `reorderCalculations` and their reverse steps). Tests to port: `packages/engine/main/test/forms.test.ts` (field and widget cases), `widget-rotation.test.ts`, `widget-painter-parity.test.ts`. Commits: d7bd2d277, 60602f579, 2445caba7, 1233d1349, 98718b780, 69e229992 (runtime `EPDFForm_SetFieldDefaultValue`).

Stubs removed: `crates/epdf-engine/src/features/forms/field.rs:274` (create with a default value), `:362` (update touching `/DV`), `:671` (helper `default_value_not_ported`). Write `/DV` through `Document::set_field_default_value(objnum, FieldValueWrite)`; a Clear is `FieldValueWrite::None`, which shadows an inherited `/DV` with an empty value, as upstream's does. Then delete `crates/epdf-engine/tests/not_ported_refusals.rs`: its last two tests (`a_field_create_with_a_default_value_is_refused_before_the_field_exists`, `a_field_update_touching_the_default_value_is_refused_before_the_rename`) test the stubs. 8.14 and 8.15 removed the other two; if either is still there, stop and report.

Prior art on `research/catchup-plugins`: `e3d45fc3a` (a widget link takes its widget as its `AnnotationRef`), `a2b884473` (a field delete's widget cascade counts as no extra mutation), `06993dcf7` (form writes answer with TS's meta, sequence and cascade). Apply the `crates/epdf-engine` hunks that fit with `git show <sha> -- crates/epdf-engine | git apply -3`, and name the sha.

Kept divergences to re-check: none recorded in this area.

- [ ] `/DV` written; the three stubs and `not_ported_refusals.rs` gone.
- [ ] Merged-field delete in one change; `attachWidget` code deleted.
- [ ] The nine kinds and the field and widget ops, all or none, with places from page space.
- [ ] `form_field.rs` replays `request`; the two removed attach cases are gone from it.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test form_field --test wire_contract` passes (owns `a_widget_lands_where_the_placement_put_it`, `a_draft_refusal_says_what_was_wrong_with_the_draft`, `a_structural_write_leaves_what_the_typescript_engine_left`).
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.20: Annotation and form transfer

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.14 and 8.19. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Form bundles replace FDF/XFDF (7f0b73a3c), and annotations gained export and import (83f6598d2, 843deedb4, 73f05157f). The kinds: `annotations.export`, `annotations.import`, `forms.export` (takes a `selection`), `forms.import` (takes a `bundle`), `forms.importValues`; the imports are also change ops (`annotations.import`, `forms.import`, `forms.importValues`). No engine vector covers them beyond `wire/requests.json`, so their behaviour is pinned by unit tests ported from upstream's.

Files: a new `crates/epdf-engine/src/features/transfer/` (bundle codec, limits, planning), `src/features/annotation/` (export, import), `src/features/forms/` (export, import), `src/features/changes/` (register the three import ops), `src/page_space.rs` (the import arms), `src/host.rs`, new `crates/epdf-engine/tests/annotation_transfer.rs` and `tests/form_transfer.rs`, `tests/wire_contract.rs`.

Vectors: none beyond `wire/requests.json`.

Upstream reference: `packages/engine/core/src/transfer/` (`AnnotationBundle.ts`, `annotationImport.ts`, `AnnotationTransfer.ts`, `bundle.ts`, `bundleLimits.ts`, `codec.ts`, `exportSelection.ts`, `FormBundle.ts`, `formExport.ts`, `formImport.ts`, `FormTransfer.ts`, `pageRefs.ts`, `planAnnotationImport.ts`, `planFormImport.ts`), `packages/engine/services/src/features/transfer/` (`bundlePages.ts`, `BundleResources.ts`, `checkWireBundle.ts`), `packages/engine/services/src/features/annotations/AnnotationExporter.ts`, `AnnotationImporter.ts`, `packages/engine/services/src/features/forms/FormExporter.ts`, `FormImporter.ts`, `packages/engine/core/src/wire/annotationsExportToken` (see `packages/engine/core/test/wire/annotationsExportToken.test.ts`). Tests to port: `packages/engine/core/test/transfer/` (all six), `packages/engine/main/test/annotation-export.test.ts`, `annotation-import.test.ts`, `annotation-transfer.test.ts`, `annotations-stamp-transfer.test.ts`, `form-transfer.test.ts`. Commits: 7f0b73a3c, 83f6598d2, 843deedb4, 73f05157f.

Kept divergences to re-check: none recorded in this area.

- [ ] Bundle codec and limits, with upstream's `core/test/transfer` cases as Rust unit tests.
- [ ] The five kinds and the three import ops, places converted at the boundary.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows to `routed` and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_transfer --test form_transfer --test wire_contract` passes. This task owns none of the 98 tests failing at `52f10e027`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.21: Redaction

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.12. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md` ("Redaction"). Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Replay each case's recorded `request` through `Host` with `test_support::replay` (8.1). No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's row). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

10 cases became 9 (`batched-index-refs-refused` went with index refs, bc0057f3a). Each case carries its `request` with the v3 scope, `{pages}` or `{annotations}` of refs (a9434c0e2). Results name pages by `page` ref. `remaining` loses the two widgets (1233d1349), so its order shifts.

Files: `crates/epdf-engine/src/features/redaction.rs`, `src/host.rs`, `crates/epdf-engine/tests/redaction_apply.rs`, `tests/wire_contract.rs`.

Vector: `redaction/apply.json`.

Upstream reference: `packages/engine/services/src/features/redaction/RedactionApplier.ts`, `packages/engine/core/src/mutation/RedactionApplyResult.ts`. Tests to port: `packages/engine/main/test/redaction-apply.test.ts`. Commits: a9434c0e2, bc0057f3a, 1233d1349; runtime 532ecf80e (`EPDFPage_ApplyRedactions`, bound in Task 6 as `apply_redaction_marks`).

Kept divergences to re-check: the no-op redaction answer carrying `mutationSequence` (plugins `87b61b533`, kept 2026-10-15) lives in the plugins branch's `epdf-viewer-host`. Check that the answer you port still lets that host add it, and note it in your report for 8.24.

- [ ] The v3 scope, page refs in results, widgets out of `remaining`.
- [ ] `redaction_apply.rs` replays `request`.
- [ ] Remove `redaction.apply` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test redaction_apply --test wire_contract` passes (owns `an_applied_redaction_removes_what_the_typescript_engine_removed`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine --all-targets` prints no warnings. `cargo fmt -p epdf-engine`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.22: Outline

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`), or the branch off its tip the controller names. Needs 8.3 and 8.5. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's row). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

`outline.read` is new (67ccb76dc), and the outline is written through `document.apply` ops `outline.create`, `outline.update`, `outline.move`, `outline.delete`. Task 13 ports `plugin/outline`, which needs these. Destinations are page space (8.3). No engine vector covers the outline beyond `wire/requests.json`.

Files: a new `crates/epdf-engine/src/features/outline.rs` (or `features/outline/`), `src/features/changes/` (register the four outline ops), `src/page_space.rs` (`pageOutlineOf`, `pageOutlineItemOf`, `pdfOutlineDraftOf`, `pdfOutlinePatchOf`), `src/host.rs`, `crates/pdfium/src/` (an outline wrapper over `epdf_outline.h`, which Task 6 bound in `pdfium-sys`), a new `crates/epdf-engine/tests/outline.rs`, `tests/wire_contract.rs`.

Vectors: none beyond `wire/requests.json`.

Upstream reference: `packages/engine/core/src/dto/DocumentOutline.ts`, `DocumentOutline.schema.ts`, `packages/engine/core/src/identity/OutlineItemRef.ts`, `packages/engine/core/src/mutation/OutlineMutationResults.ts`, `packages/engine/core/src/outline/predict.ts`, `packages/engine/core/src/pageSpace/outline.ts`, `packages/engine/services/src/features/outline/` (`OutlineReader.ts`, `OutlineMutator.ts`, `internal/`), `packages/engine/services/src/features/changes/internal/outlineChanges.ts`. Tests to port: `packages/engine/main/test/outline.test.ts`. Commit: 67ccb76dc.

Kept divergences to re-check: none recorded in this area.

- [ ] The outline wrapper in `crates/pdfium`, with tests in `crates/pdfium/tests/`.
- [ ] `outline.read` and the four ops, all or none, places converted at the boundary.
- [ ] Remove `outline.read` from `NOT_YET_ROUTED`; set its `operations.ts` row to `routed` and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test outline --test wire_contract` and `CARGO_BUILD_JOBS=6 cargo test -p pdfium` pass. This task owns none of the 98 tests failing at `52f10e027`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.23: Close the re-port on catchup-impl

Branch `research/catchup-impl` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl`). Needs 8.1 to 8.22. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Ruling (ledger, Task 2): the typecheck gate was waived for `packages/engine/main/test/geometry-rust-parity.test.ts` only, by marking it `describe.skip` with a comment pointing here. Upstream removed `page.geometry.read()`, so the test is rewritten against upstream's text layout. This task changes that one `packages/**` file, because the ruling assigns it here.

Files: `packages/engine/main/test/geometry-rust-parity.test.ts`, `crates/epdf-engine/tests/wire_contract.rs`, `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (final row states).

Upstream reference: `packages/engine/core/src/pageSpace/text.ts` (`createTextLayout`), `packages/engine/main/src/document/LocalPageTextService.ts` (`handle.text.layout()`, which sends `pages.geometry`), `packages/engine/services/src/features/geometry/RustPageGeometryReader.ts`.

Carried items owned here (Task 2): the parity test's `hasRustCore` is unused until now; remove the `describe.skip` and the `@ts-expect-error` together.

Kept divergences to re-check: walk the list in `docs/research/rust-plugins-branch-review.md`, "Status" (on `research/catchup-plugins`), and the engine-wire ones recorded on 2026-10-14 and 2026-10-15 (`dd2e1c290`, `3baec217b`, `7566552a2`, `517d0123d`, `87b61b533`, `818eba296`). For each, state in the report whether it is on impl now, kept as it was, or waits for 8.24.

- [ ] Rewrite the parity test to compare both readers through `handle.text.layout()` (or the lowest upstream call that still sends `pages.geometry`), exactly as before (no tolerance). Gate it with `describe.skipIf(!hasRustCore)` in place of `describe.skip`, and drop the `@ts-expect-error`.
- [ ] `pnpm --filter @embedpdf/engine typecheck` passes. `pnpm --filter @embedpdf/engine test -- geometry-rust-parity` runs and skips on the stock payload; if Task 9's Rust-core payload exists, run it with `EMBEDPDF_WASM_URL` pointing at it and it passes. If it does not exist yet, say so in the report: Task 9 runs it.
- [ ] `wire_contract.rs`: `NOT_YET_ROUTED` holds exactly the 19 kinds 8.24 owns (`signatures.analyze`, `signatures.cancel`, `signatures.complete`, `signatures.contents`, `signatures.digest`, `signatures.finalizeCandidate`, `signatures.list`, `signatures.prepare`, `signatures.revisionBytes`, `measure.setScale`, `measure.viewports`, `pages.setName`, `pages.removeName`, `document.version`, `document.setFontSettings`, `fonts.authorizeEditing`, `annotations.flatten`, `annotations.exportAppearance`, `forms.setSignatureAppearance`). Rename it `PORTED_ON_PLUGINS` with a comment pointing at 8.24.
- [ ] `pnpm --filter @embedpdf/tooling-engine-conformance run operations:check` passes, and every row not owned by 8.24 is `ported` or `routed`.
- [ ] `git grep -n "re-port (Task 8)"` prints nothing.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine` passes with no failures, `CARGO_BUILD_JOBS=6 cargo test -p epdf-wasm -p epdf-uniffi -p epdf-node -p pdfium -p pdfium-sys -p test-support` passes, and `CARGO_BUILD_JOBS=6 cargo clippy --workspace --all-targets` prints no warnings.
- [ ] Commit with explicit paths.

#### Task 8.24: Plugins-branch engine features, after Task 10

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), after Task 10 has merged `research/catchup-impl` into it. Needs 8.23 and Task 10. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**` or `.github/**` changes; `tooling/engine-conformance/**` only as named below. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Why this is separate: 36 commits on this branch change `crates/epdf-engine` (80 files, 10,813 insertions against the merge base with catchup-impl) and are not on impl. They port, against the pre-catch-up TypeScript, signatures (`da3297cf3`, `4c3fa2ecb`, `8480e5c91`, `138d6e38e`, `5efaf80c3`), measurement viewports (`635f7d303`, `d8027c034`), named pages (`457cdc82d`), font settings (`86e20c356`, `7566552a2`), flatten and appearance export (`93ee01678`), scopes enforced natively (`a4ed2b228`), document events (`6cbcaa8a5`), and the PageRef and error-envelope wire changes 8.1 drew on. Their 19 wire kinds are the ones 8.23 left in `PORTED_ON_PLUGINS`. Task 10's merge is expected to resolve `crates/epdf-engine` toward impl's side; this task brings these features onto upstream's current shapes on top of that.

Files: `crates/epdf-engine/src/features/signature/`, `features/measure.rs`, `features/fonts.rs`, `features/pages.rs` and `pages_write.rs` (named pages), `features/annotation/` (flatten, appearance export), `features/forms/` (`forms.setSignatureAppearance`), `src/host.rs`, `src/page_space.rs`, `crates/epdf-viewer-host/` where it translates these kinds, `crates/epdf-engine/tests/` (`annotation_flatten.rs`, `document_events.rs`, `font_settings.rs`, `measure.rs`, `named_pages.rs`, `search_ignore_whitespace.rs`, `security_scope.rs`, `signature_analyze.rs`, `signature_finalize.rs`, `signature_protection.rs`, `signature_read.rs`, `signature_sign.rs`, `wire_contract.rs`), and `tooling/engine-conformance/vectors/signature/protection.json`, regenerated (the only tooling change; it is this branch's vector and was cut against the old API).

Vectors: `signature/protection.json` (regenerate with `pnpm --filter @embedpdf/tooling-engine-conformance run signature-protection` after fixing its generator to the new API if needed, then commit it separately) and `wire/requests.json`.

Upstream reference: `packages/engine/services/src/features/signature/` (`SignatureReader.ts`, `SignatureAnalyzer.ts`, `SignatureMutator.ts`, `CandidateFinalizer.ts`, `internal/`), `packages/engine/core/src/signature/`, `packages/engine/services/src/features/measure/`, `packages/engine/core/src/dto/Measure.ts`, `packages/engine/core/src/pageSpace/measure.ts`, `packages/engine/core/src/dto/NamedPage.ts`, `packages/engine/core/src/mutation/PageNameInput.ts`, `PageNameResult.ts`, `packages/engine/services/src/features/fonts/FontRegistrar.ts`, `packages/engine/services/src/features/annotations/AnnotationFlattener.ts`, `packages/engine/core/src/mutation/AnnotationFlattenResult.ts`, `PageFlattenResult.ts`, and `worker-protocol.ts` for each kind. Tests to port where Rust has none: `packages/engine/main/test/signatures.test.ts`, `signature-*.test.ts`, `measurements.test.ts`, `measurement-*.test.ts`, `named-pages.test.ts`, `fonts.test.ts`, `annotation-flatten.test.ts`, `annotation-appearance-export.test.ts`, `document-events.test.ts`, `packages/engine/core/test/signature-protection.test.ts`. Commits: 7bccc4f35 (`signatures.cancel` replaces `signatures.abort`); for the rest, `git log 2516e2786..b456ee4fd -- <reference file>`.

Kept divergences to re-check (all kept by the user; keep each unless upstream now does the same, and record which): `document.setFontSettings` refuses an unknown `embeddingPolicy` as `InvalidArg`, `signatures.finalizeCandidate` opens a plain file document, `annotations.flatten` bumps the sequence twice (`dd2e1c290`); font bytes as payload 0, fractional or boolean `weight`/`italic` as `WireFormat`, NUL in `familyName` as `InvalidArg` (`7566552a2`); `document.versioned` added in `epdf-viewer-host/native_engine.rs`, signing notices in an envelope-level `event`, absent `destIndex` omitted (`517d0123d`); no-op flatten and redaction answers carry `mutationSequence` (`87b61b533`; the `applyEffects` part has no kind left, see 8.18). Also wire `signedDocumentPolicy`, which 8.2 parses and stores.

- [ ] List each of the 36 commits (`git log --oneline <merge>^2..<merge>^1 -- crates/epdf-engine`, where `<merge>` is Task 10's merge commit) and say for each: merged as is, re-ported here, or obsolete upstream.
- [ ] Port the 19 kinds against upstream's shapes, places through the page-space boundary, writes through the change queue where upstream's are change ops.
- [ ] `wire_contract.rs`: `PORTED_ON_PLUGINS` is empty and removed; every one of the 89 kinds routes.
- [ ] `operations.ts`: the 19 rows set; `pnpm --filter @embedpdf/tooling-engine-conformance run operations:check` passes.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine` passes with no failures, `cargo test -p epdf-viewer-host` passes, `pnpm --filter @embedpdf/tooling-engine-conformance run signature-protection:check` is current, and `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings.
- [ ] `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

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
