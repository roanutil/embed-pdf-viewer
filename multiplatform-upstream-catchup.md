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
- H3's crash path: a gesture whose annotation disappears mid-drag. Upstream's store closes it with `forget`; the Rust port must do the same.
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

1. **H3** (`046d47578`): re-apply it. *Superseded in Task 10 (controller ruling, 2026-10-21): upstream's annotation store commits `forget` synchronously when a record leaves the view, which clears the draft (`packages/plugin/annotation/src/services/store.ts:487-492`, `packages/core/annotation/src/update/session.ts:65-80`), and new records keep their object-number id, so the crash H3 fixed cannot happen upstream. No TS change; Task 13's annotation port mirrors `forget`.* After the merge, `reconcile`'s temp-id rename reaches the draft in upstream's `update/edit.ts` and `update/edit-commit.ts`, and the edit gesture's reads of a missing annotation clear the draft instead of throwing. The Rust re-port carries the same guards, and the `core/annotation-update` vector case (create, pointer down, Created, move, up) stays.
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

Task 7 (redo) wrote Tasks 8.1 to 8.28 on 2026-10-21 against `research/catchup-plugins` at `253bca66b`, after Task 10 merged catchup-impl into this branch. The re-port runs here, on this branch's `crates/epdf-engine`, which already holds the 36 engine commits and 19 wire kinds catchup-impl lacked. At `253bca66b`, `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast` fails 109 tests in 32 targets. `cargo test -p epdf-viewer-host` fails 4 and `cargo test -p epdf-wasm` fails 1 (`borrowed_page`). Each of the 114 belongs to exactly one sub-task's done criteria. 8.1 recovers this branch's own `tooling/engine-conformance` work, which Task 10 dropped by taking impl's side. 8.2 to 8.6 are shared foundations, and 8.7 to 8.26 are feature families, each placed after the sub-tasks it needs. 8.27 closes the Rust side. Task 9 (the payload) runs next, and 8.28, the TS parity test, runs after Task 9 because it needs that payload.

| Sub-task | Needs |
|---|---|
| 8.1 Recover this branch's engine-conformance work and regenerate | none |
| 8.2 Wire envelope, references, kinds, replay helper | 8.1 |
| 8.3 Session kinds and fonts | 8.2 |
| 8.4 Page space, page boxes, page list, destinations, render targets | 8.2 |
| 8.5 Dates and the metadata patch types | none |
| 8.6 Change queue and metadata | 8.2, 8.4, 8.5 |
| 8.7 Security and scope | 8.1, 8.2 |
| 8.8 Attachments and piece info | 8.5, 8.6 |
| 8.9 Page geometry and text | 8.4 |
| 8.10 Text layout | 8.9 |
| 8.11 Search | 8.1, 8.4, 8.9 |
| 8.12 Annotation read, vocabulary, save round trip | 8.4, 8.5, 8.19 |
| 8.13 Annotation appearances | 8.12 |
| 8.14 Annotation create | 8.6, 8.12 |
| 8.15 Annotation update and write verdicts | 8.14 |
| 8.16 Annotation delete and reorder | 8.6, 8.12 |
| 8.17 Flatten and appearance export | 8.1, 8.13, 8.16 |
| 8.18 Page structure, named pages and extract | 8.1, 8.6, 8.17 |
| 8.19 Form read | 8.4, 8.5 |
| 8.20 Form values, reset, repair, apply | 8.6, 8.19 |
| 8.21 Form fields and widgets | 8.15, 8.20 |
| 8.22 Annotation and form transfer | 8.14, 8.21 |
| 8.23 Redaction | 8.6, 8.12 |
| 8.24 Outline | 8.4, 8.6 |
| 8.25 Measure | 8.4, 8.6, 8.12 |
| 8.26 Signatures | 8.1, 8.3, 8.21 |
| 8.29 Layer-backed fat-memory open | 8.3 |
| 8.30 Session parity | 8.6, 8.29 |
| 8.27 Close the Rust side | 8.1 to 8.26, 8.29, 8.30 |
| Task 9 | 8.27 |
| 8.28 Parity test | Task 9 |

Two sub-tasks may run at once only when neither needs the other.

#### Task 8.1: Recover this branch's engine-conformance work and regenerate

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs nothing. This task changes `tooling/engine-conformance/**` and `docs/research/catchup-engine-vector-diffs.md` only. No `crates/**`, `packages/**` or `.github/**` changes. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Task 10 resolved the 37 `tooling/engine-conformance` files both branches changed by taking catchup-impl's side, which is upstream-adapted (Task 4's generators, Task 5's vectors). That dropped this branch's own edits to those files. The ones Task 10 kept, because only this branch had them, are `src/generate-signature-protection-vectors.ts`, `vectors/signature/protection.json`, `fixtures/tiny.png`, the `signature-protection` scripts in `package.json`, and `doc.sign` / `doc.sign.certify` in `src/scope-cases.ts`. This branch's edits since its merge base with impl (`c8085bd3c`) are `git diff c8085bd3c b156f9e06^1 -- tooling/engine-conformance`: 44 files, 30 commits.

At `253bca66b` every `:check` script is current except two. `scope:check` differs on `scope/resolve.json`, and `signature-protection:check` differs on `signature/protection.json`.

Preconditions for running the generators: `pnpm install`; `pnpm exec turbo run build --filter=@embedpdf/engine...`; and a wasm payload in `packages/engine/runtime/npm/wasm32/lib`. The stock upstream payload already in `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-impl/packages/engine/runtime/npm/wasm32/lib` serves the TS oracle. Copy it into this worktree, and don't commit it.

Re-apply in Task 4's style: keep each case, adapt how it reaches the engine to the current API, and record the `request` actually sent where the case literal keeps an older spelling. Job and op ids are `"<job>"` and `"<op>"`.

| Source commit | What it did | What to do |
|---|---|---|
| `93ee01678` | 8 `pages/mutate.json` cases for `annotations.flatten` and `annotations.exportAppearance`: `flatten-annotations-subset`, `-for-print`, `-nothing-eligible`, `-empty-refused`, `-other-page-refused`, `-unknown-ref-refused`, `export-appearance`, `export-appearance-without-appearance-refused` | Re-add them to `src/generate-pages-mutate-vectors.ts`. `nm` refs are gone (bc0057f3a): resolve each `/NM` to an `objectNumber` ref through `annotations.list` before the call, and record the `request` sent. |
| `82fc580e9` | 2 `search/slice-protocol.json` cases: `regex-with-ignore-whitespace-is-refused`, `key-separates-ignore-whitespace` | Re-add them to `src/search-cases.ts` in the current form, with a recorded `request` and a cursor minted by `searchQueryKey`/`encodeSearchCursor` (our 26887941a). |
| `b7573c3a6` | `pages/extract.json` recorded `sources` (the PageRefs sent) | Record `sources` as the `PageRef`s passed to `doc.pages.extract`, in place of or beside `sourceObjectNumbers`. `crates/epdf-engine/tests/page_extract_corpus.rs` reads `sources`. |
| `da3297cf3` | `generate-signature-protection-vectors.ts` and its vector | Kept by Task 10. Find why `signature-protection:check` differs: an API change in the generator, or upstream behaviour. Adapt the generator if needed, regenerate, and write down the cause. |
| `a4ed2b228` | `doc.sign`, `doc.sign.certify` in `scope-cases.ts`; `scope/parse.json`, `scope/resolve.json` | Kept in `scope-cases.ts` (upstream's `auth/scope/types.ts` has both). Regenerate `scope/resolve.json`. |
| `dd2e1c290` | the 19 kinds' rows in `src/operations.ts` with this branch's `rustStatus`, group and notes | Impl's matrix has the 19 rows as `planned`. Set each row's `rustStatus` to what this branch's Rust host does today, and carry over the notes that still hold. |
| `221fb1477` | save mode `'rewrite'` | Already handled in impl's `generate-save-vectors.ts:152-156`. Nothing to do. Say so in the report. |
| `94836c1c4` | 84-kind wire vector | Superseded by the 89-kind vector. Nothing to do. |
| `457cdc82d`, `3b795d4ac` | vector-only regenerations (named pages, rich text) | Superseded: the upstream-adapted vectors already carry `namedPages` and `richText`. Nothing to do. |
| `3baec217b` and the PageRef adapter commits (`a35d60373`, `804511a62`, `28adb8b95`, `953ac1a76`, `b4697879a`, `b7573c3a6`, `e2bcada08`, `9a275186c`, `0b42f996e`, `78745772a`, `43feeec45`, `7b91ad1cf`, `fb1e125d0`, `672f185f6`, `cc3ea8527`, `5a3b75400`) | adapted the generators to PageRef before upstream had it | Superseded by Task 4's adaptation, except `b7573c3a6` above. Check each and list it in the report as superseded or re-applied. |

Carried items owned here, from the Task 4a ledger lines. The `abort` row note in `operations.ts` says the switch has 64 arms and the protocol 65 kinds. The `pages.move`/`pages.insert` invariant notes and the `annotations.listFullPage` cache note were not carried onto `document.apply` and `annotations.list`. `signatures.finalizeCandidate` is filed C / after-first-write though its `effect` is `read`, and `pages.workingSet` is queued-only though it never queues. Fix the notes, and decide the two rows against upstream's `effect` and WorkerHost's handling, with the reason in each row's `note`.

Rust tests that replay files this task changes may start failing. That is expected. Each one already belongs to a later sub-task: `pages_mutate` to 8.18, `page_extract_corpus` to 8.18, `search_slice_protocol` to 8.11, `scope_conformance` to 8.7, `signature_protection` to 8.26. List every Rust test whose result changed, before and after, in the report.

- [ ] Re-apply the rows of the table above, one commit per source commit re-applied.
- [ ] `operations.ts` notes and rows (carried items); re-render with `pnpm --filter @embedpdf/tooling-engine-conformance run operations`.
- [ ] Regenerate: `pnpm --filter @embedpdf/tooling-engine-conformance run vectors`, `run scope`, `run signature-protection`, `run pages-mutate`, `run search`, `run extract`. Run each twice; the files come out byte-identical.
- [ ] Every script in `tooling/engine-conformance/package.json` ending `:check` passes, `signature-protection:check` and `scope:check` included.
- [ ] Add a section "Recovered from research/catchup-plugins" to `docs/research/catchup-engine-vector-diffs.md`: per file, what this task changed and why, with the source commit.
- [ ] Run `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --no-fail-fast 2>&1 | grep -E '^test .* FAILED'` before and after, and put the difference in the report.
- [ ] Commit with explicit paths.

#### Task 8.2: Wire envelope, references, kinds and the request replay helper

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.1. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Wire (`host.rs`)".

Upstream's `WorkerRequest` union has 89 kinds. This branch's host routes 84: the old 65, minus nothing, plus the 19 it ported (`dd2e1c290`). Against the 89, it lacks 20: `annotations.list`, `annotations.export`, `annotations.import`, `annotations.readAppearance`, `annotations.reorder`, `forms.addWidget`, `forms.updateWidget`, `forms.deleteWidget`, `forms.reorderWidgets`, `forms.reorderCalculations`, `forms.importValues`, `document.apply`, `document.applyChanges`, `metadata.readCustom`, `metadata.updateCustom`, `objectNumbers.reserve`, `outline.read`, `pages.workingSet`, `pieceInfo.delete`, `signatures.cancel`. It still routes 14 that upstream removed: `annotations.listFullPage`, `annotations.listRawAll`, `annotations.listRawPage`, `annotations.move`, `attachments.create`, `attachments.delete`, `forms.applyEffects`, `forms.attachWidget`, `pages.delete`, `pages.insert`, `pages.insertBlank`, `pages.move`, `pages.rotate`, `pieceInfo.clear`, and `signatures.abort`, which upstream renamed `signatures.cancel` (7bccc4f35). This task makes the host read the new envelope and references, refuses the removed kinds on the wire, and gives every later sub-task one way to replay a recorded `request`. It ports no kind's behaviour.

Transition rules, which hold from this task until 8.27:
- The envelope fields are optional. A request without `effect`, `opId` or `authority` is accepted and treated as WorkerHost treats the default (`authority` absent means the session's own). This keeps the kernel's calls through `translate.rs` and the existing tests working. 8.27 makes them required where upstream's schema requires them.
- The removed kinds leave `Host::call`, so the wire answers them as unknown kinds. Their arms move, unchanged apart from the ref spelling, to a doc-hidden `Host::call_legacy(request, payloads)` that only `crates/epdf-viewer-host/src/translate.rs` and the tests listed below call. `call_legacy` routes only those 15 kinds and has a doc comment saying it exists for the Task 8 transition. Each owner sub-task moves its callers to the new kind and deletes its legacy arms: `annotations.listFullPage`, `listRawAll` and `listRawPage` go in 8.12, `annotations.move` in 8.16, `attachments.create`, `attachments.delete` and `pieceInfo.clear` in 8.8, the five `pages.*` kinds in 8.18, `forms.applyEffects` in 8.20, `forms.attachWidget` in 8.21, and `signatures.abort` in 8.26. 8.27 deletes `call_legacy`.

This branch already speaks PageRef on the wire (`71775a1e4`, `3baec217b`), but spells it `{kind: "objectNumber", pageObjectNumber}`. Upstream spells it `{kind: "objectNumber", objectNumber}` (595bdc6c7), and every ref of kind `objectNumber` names its number `objectNumber`. It also refuses an unknown kind as WorkerHost does (`991031edd`), and answers `name: "EngineError"` (`c1e6330a6`). Keep both.

Files: `crates/epdf-engine/src/host.rs`, `src/dispatch.rs`, `src/identity.rs`, `src/ids.rs`, `src/error.rs`, a new `crates/epdf-engine/tests/common/replay.rs` (declared from `tests/common/mod.rs`), a new `tests/replay_helper.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`, `crates/epdf-viewer-host/tests/translate.rs`, `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (row states), and these passing tests, which call removed kinds or spell the old ref:

| Test file | What it uses | What to do |
|---|---|---|
| `crates/epdf-engine/tests/common/mod.rs` (lines 21, 27, 89) | `pageObjectNumber` in refs | Respell to `objectNumber`. Add a `call_legacy` helper beside `call`. |
| `tests/document_events.rs` (13 tests call removed kinds) | `annotations.move`, `pages.move`/`rotate`/`delete`/`insertBlank`, `pieceInfo.clear`, `attachments.create`/`delete`, `forms.applyEffects`, `signatures.abort`, `forms.attachWidget`, `annotations.listRawPage` | Repoint each to `call_legacy`, with a comment naming the kind's owner (8.16, 8.18, 8.8, 8.20, 8.26, 8.21, 8.12). |
| `tests/signature_sign.rs:96-117` | `signatures.abort` | Repoint to `call_legacy`; owner 8.26. |
| `tests/signature_analyze.rs:128` | `pages.rotate` | Repoint to `call_legacy`; owner 8.18. |
| `tests/annotation_flatten.rs:56,72` | `annotations.listRawPage` | Repoint to `call_legacy`; owner 8.12. |
| `tests/host_sessions.rs:215-233` | `pages.insertBlank`, `pageObjectNumber` | Repoint to `call_legacy` and respell; owner 8.18. |
| `tests/measure.rs:182` | `pageObjectNumber` | Respell. |
| `crates/epdf-viewer-host/tests/asset_engine.rs:29` | `pageObjectNumber` | Respell. |
| `tests/wire_contract.rs`, `tests/form_effects.rs`, `tests/search_slice_protocol.rs` | removed kinds, old spelling | `wire_contract` is rewritten below. The other two already fail and belong to 8.20 and 8.11; respell only. |

No test is `#[ignore]`d. A test that can't be repointed or respelled stops the task: report it.

Vector: `wire/requests.json`.

Upstream reference: `packages/engine/core/src/wire/worker-protocol.ts` (the union, `WriteJobFields`, the `effect` tag), `packages/engine/core/src/wire/schemas.ts`, `packages/engine/core/src/identity/PageRef.ts`, `PageRef.schema.ts`, `AnnotationRef.ts`, `FormFieldRef.ts`, and `packages/engine/services/src/worker-host/WorkerHost.ts`. Commits: f550e9e5f (`effect`), b32fdf584 and 630b77eab (`opId`, `objectNumberFloor`), 91404cfcb and 2445caba7 (`authority`), 595bdc6c7 (refs), bc0057f3a (`nm` and `index` refs gone, `baseIndex`), and for the removals 83f6598d2, 1233d1349, 98e5c270c, d7bd2d277, 119685ea4, b69c40377, c2d7a5194, a9434c0e2, 7bccc4f35.

Kind owners. Every kind is routed by exactly one sub-task:

| Owner | Kinds |
|---|---|
| 8.2 | `abort`, `shutdown` |
| 8.3 | `open.fatMem`, `open.layerMemBase`, `open.layerFileBase`, `layer.close`, `close`, `objectNumbers.reserve`, `pages.workingSet`, `document.version`, `document.saveBuffer`, `document.saveLayerBuffer`, `document.saveFile`, `fonts.register`, `fonts.addFallback`, `fonts.clearFallbacks`, `fonts.clear`, `fonts.authorizeEditing`, `document.setFontSettings` |
| 8.4 | `pages.list`, `actions.read`, `pages.render`, `pages.renderEncoded`, `document.renderPageFile`, `document.renderPageFileEncoded` |
| 8.6 | `document.apply`, `document.applyChanges`, `metadata.read`, `metadata.update`, `metadata.readCustom`, `metadata.updateCustom` |
| 8.7 | `document.probeSecurityFile`, `document.checkPasswordPermissions` |
| 8.8 | `attachments.list`, `attachments.readFile`, `pieceInfo.read`, `pieceInfo.applications`, `pieceInfo.update`, `pieceInfo.delete` |
| 8.9 | `pages.geometry`, `pages.text` |
| 8.11 | `search.query` |
| 8.12 | `annotations.list`, `annotations.readFile` |
| 8.13 | `annotations.renderAppearances`, `annotations.renderAppearancesEncoded`, `annotations.readAppearance` |
| 8.14 | `annotations.create` |
| 8.15 | `annotations.update` |
| 8.16 | `annotations.delete`, `annotations.reorder` |
| 8.17 | `pages.flatten`, `annotations.flatten`, `annotations.exportAppearance` |
| 8.18 | `pages.extract`, `pages.setName`, `pages.removeName` |
| 8.19 | `forms.list` |
| 8.20 | `forms.setValue`, `forms.reset`, `forms.repair` |
| 8.21 | `forms.createField`, `forms.updateField`, `forms.deleteField`, `forms.detachWidget`, `forms.addWidget`, `forms.updateWidget`, `forms.deleteWidget`, `forms.reorderWidgets`, `forms.reorderCalculations` |
| 8.22 | `annotations.export`, `annotations.import`, `forms.export`, `forms.import`, `forms.importValues` |
| 8.23 | `redaction.apply` |
| 8.24 | `outline.read` |
| 8.25 | `measure.viewports`, `measure.setScale` |
| 8.26 | `signatures.list`, `signatures.analyze`, `signatures.contents`, `signatures.digest`, `signatures.revisionBytes`, `signatures.prepare`, `signatures.complete`, `signatures.cancel`, `signatures.finalizeCandidate`, `forms.setSignatureAppearance` |

Kept divergence to re-check: a malformed PageRef (unknown `kind`, a number that is not a positive integer) is refused as `InvalidArg`, where the TS worker does not validate (`3baec217b`, kept on 2026-10-15). Keep it under the new spelling, with a comment at the check.

- [ ] Parse the envelope every job request carries: `effect`, `jobId`, `opId` and optional `objectNumberFloor` on writes, `authority` where the kind takes one. Answers keep their shape here; their owners change them.
- [ ] `PageRef` and `AnnotationRef` in upstream's spelling (`objectNumber`; `AnnotationRef` is `objectNumber` or `baseIndex`, each with `page`). Remove the `nm` and `index` ref kinds at the wire. The internal page object number type stays.
- [ ] Move the arms of the 14 removed kinds and of `signatures.abort` from `Host::call` to `Host::call_legacy` (transition rules above). On the wire each gets the unknown-kind answer.
- [ ] `crates/epdf-engine/tests/common/replay.rs` (`common::replay`): open a fixture in a `Host`, take a case's recorded `request`, put the test's `docId` in place of the recorded one and real ids in place of `"<job>"` and `"<op>"`, send it through `Host::call` with its payloads, and return the parsed result or error. It lives under `tests/common/`, not in `crates/test-support`, because `test-support` depending on `epdf-engine` would make a dependency cycle. Unit-test the substitution in `tests/replay_helper.rs`.
- [ ] `wire_contract.rs`: expect 89 kinds. Add a `NOT_YET_ROUTED` list of the 20 kinds above, each with its owner from the table. The test fails if a listed kind routes (so the list can't rot) or an unlisted kind answers `NotImplemented`. Each later sub-task removes its kinds. Also assert that each removed kind, `signatures.abort` included, gets the unknown-kind answer.
- [ ] `translate.rs`: the kernel's calls keep working against the new ref spelling. A kernel call that mapped onto a removed kind now goes through `Host::call_legacy`, with a comment naming the kind's owner.
- [ ] Repoint or respell the test files in the table above.
- [ ] `operations.ts`: rows match what the host now does; re-render with `pnpm --filter @embedpdf/tooling-engine-conformance run operations`.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test wire_contract --test host_sessions --test replay_helper --test document_events --test signature_sign --test signature_analyze --test annotation_flatten --test measure` passes, except `document_events::an_effects_batch_publishes_form_effects_applied`, `document_events::every_form_write_carries_meta` (8.20) and `annotation_flatten::a_flatten_that_applies_nothing_is_no_mutation` (8.17), which failed before. This task owns `every_wire_kind_routes_to_an_implementation`. `CARGO_BUILD_JOBS=6 cargo test -p epdf-viewer-host --test asset_engine --test translate` passes.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.3: Session kinds and fonts

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.2. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Wire (`host.rs`)".

The kinds, from 8.2's table: `open.fatMem`, `open.layerMemBase`, `open.layerFileBase`, `layer.close`, `close`, `objectNumbers.reserve`, `pages.workingSet`, `document.version`, `document.saveBuffer`, `document.saveLayerBuffer`, `document.saveFile`, `fonts.register`, `fonts.addFallback`, `fonts.clearFallbacks`, `fonts.clear`, `fonts.authorizeEditing`, `document.setFontSettings`. This branch already routes all of them except `objectNumbers.reserve` and `pages.workingSet`. The font kinds and `document.version` were ported here against the pre-catch-up TS (`86e20c356`, `7566552a2`). Check each against upstream's fields and answers, and adapt rather than rewrite.

Files: `crates/epdf-engine/src/host.rs`, `src/session/` (`registry.rs`, `document.rs`, `pages.rs`, `save.rs`, `authority.rs`), `src/features/save.rs`, `src/features/fonts.rs`, `src/mutation.rs`, `crates/epdf-engine/tests/wire_contract.rs`, `tests/host_sessions.rs`, `tests/document_session.rs`, `tests/session_registry.rs`, `tests/mutation_finalization.rs`, `tests/font_registry.rs`, `tests/font_settings.rs`, `tests/document_events.rs` (the save and version events only), `crates/epdf-viewer-host/src/translate.rs`, `src/native_engine.rs`.

Vectors: none beyond `wire/requests.json`. Unit tests ported from upstream's pin the answers.

Upstream reference: `packages/engine/core/src/wire/worker-protocol.ts` (`ObjectNumbersReserveWorkerRequest`, `PagesWorkingSetWorkerRequest`, the open kinds' `objectNumbers`, `reserveObjectNumbers`, `signedDocumentPolicy`, and the open answer's `objectNumbers`/`lastObjectNumber`), `packages/engine/core/src/identity/ObjectNumbers.ts`, `packages/engine/services/src/worker-host/WorkerHost.ts`, `packages/engine/services/src/features/save/`, `packages/engine/services/src/features/fonts/FontRegistrar.ts`, `packages/engine/core/src/dto/PdfSaveMode.ts`, `FontSpec.ts`. Tests to port: `packages/engine/main/test/object-numbers.test.ts`, `page-residency.test.ts`, `page-residency-host.test.ts`, `page-retention.test.ts`, `opening.test.ts`, `worker-files.test.ts`, `fonts.test.ts`. Commits: b32fdf584 (`objectNumbers.reserve`, `OBJECT_NUMBER_ISSUE_LIMIT`, `LayerFull`), 630b77eab (`objectNumberFloor`), f550e9e5f (`pages.workingSet`).

`signedDocumentPolicy`: parse it and keep it on the session. 8.26 enforces it with the signature engine.

Kept divergences to re-check (all kept by the user; keep each unless upstream now does the same, and record which in the report): font bytes as payload 0 (missing is `WireFormat`), fractional or boolean `weight`/`italic` refused as `WireFormat`, NUL in `familyName` refused as `InvalidArg` (`7566552a2`); `document.setFontSettings` refuses an unknown `embeddingPolicy` as `InvalidArg` (`dd2e1c290`); `document.versioned` is added in `crates/epdf-viewer-host/src/native_engine.rs`, not the host (`517d0123d`).

- [ ] Open kinds take `objectNumbers`, `reserveObjectNumbers` and `signedDocumentPolicy`, and the open answer carries `objectNumbers` and `lastObjectNumber` as upstream's does.
- [ ] `objectNumbers.reserve`: hand the session `count` more numbers; refuse a session that does not issue its own; refuse past `OBJECT_NUMBER_ISSUE_LIMIT` with `LayerFull`. Writes honour `objectNumberFloor`.
- [ ] `pages.workingSet`: take it on arrival, never answer, and set the order parsed pages close in, as WorkerHost does.
- [ ] Save, version and font kinds answer with upstream's fields; check each against `worker-protocol.ts`.
- [ ] Port the listed upstream tests that exercise these kinds where Rust has no equivalent.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test wire_contract --test host_sessions --test document_session --test session_registry --test mutation_finalization --test font_registry --test font_settings` passes, with your new tests. This task owns none of the tests failing at `253bca66b`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.4: Page space, page boxes, the page list, destinations and render targets

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.2. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Cross-cutting changes", "Pages", "Actions", "Render".

Ruling (ledger, Task 4b): the vectors record page space, as the TS engine API now answers, and the Rust host ports upstream's `pageSpaceBoundary` so its wire answers are page space too. In page space, x runs right from the visible box's left edge and y runs down from its top edge, in PDF units, ignoring `/Rotate`. Readers and writers stay in PDF space. The host converts on the way out and on the way in, in one module. This task builds that module and its first consumers. Each later family adds its own DTO's conversion there, never inside a feature.

Files: a new `crates/epdf-engine/src/page_space.rs` (or `src/page_space/`), `src/geom.rs` (page-space `PageRect {x, y, width, height}`, `PagePoint`, `PageQuad` with `upperLeft`/`upperRight`/`lowerLeft`/`lowerRight`), `src/host.rs`, `src/features/pages.rs`, `src/features/actions/destination.rs`, `src/features/actions/document.rs`, `src/features/render/geometry.rs`, `src/features/render/raster.rs`, `crates/pdfium/src/page_list.rs`, `crates/pdfium/src/page_write.rs`, `crates/pdfium/tests/documents.rs`, `crates/epdf-engine/tests/pages_corpus.rs`, `tests/action_reader.rs`, `tests/submit_flags.rs`, `tests/render_geometry.rs`, `tests/render_pixels.rs`, `tests/rotation_normalization.rs`, `tests/geometry.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: `metadata/pages.json`, `metadata/actions.json`, `actions/submit-flags.json`, `actions/types.json`, `render/geometry.json`.

Upstream reference: `packages/engine/core/src/geometry/pageSpace.ts`, `pageBoxes.ts` (`pageBoxesOf`, `pageRotationOf`, `DEFAULT_MEDIA_BOX`), `pageTransform.ts`, `convert.ts`, `primitives.ts`, `packages/engine/core/src/pageSpace/` (`coordinates.ts`, `pages.ts`, `destinations.ts`, `rendering.ts`, `helpers.ts`), `packages/engine/services/src/worker-host/pageSpaceBoundary.ts` (`resultInPageSpace`, `requestInFileSpace`, `pdfRenderTargetOf`), `packages/engine/core/src/dto/PageLayout.ts`, `PageListSnapshot.ts`, `NamedPage.ts`, `PdfAction.ts`, `PdfDestination.ts`, `packages/engine/services/src/features/pages/PagesReader.ts`, `features/actions/`, `features/destinations/`, `features/render/`. Tests to port: `packages/engine/core/test/pageSpace/` (`primitives.test.ts`, `destinations.test.ts`, `helpers.test.ts`), `packages/engine/main/test/page-space.test.ts`. Commits: 60283448c (codec, `pdfCropBox`, action key order), d64920820 and 8b6a2d0e8 (boxes), f5d70b67e (destinations), 7f107fd7c (render targets), 771342379 (`renderMatrix`), bf7d4558e (`tabOrder`), bd4976e4e (`namedPages`, fields never left out), 595bdc6c7 (`ref`).

Carried item owned here (Task 6): the page box defaults and the `/Rotate` reduction move into the engine as upstream's `pageBoxesOf` and `pageRotationOf`. `pageRotationOf` turns a value that is not a multiple of 90 into 0, where the wrapper's `quarter_turns_of` truncates. Read raw boxes through `Document::written_page_box_by_index`. Remove the wrapper defaults in `page_box_by_index`, `page_size_by_index` and `page_rotation_quarter_turns`, and rewrite their unit tests in `crates/pdfium/src/page_list.rs` to the raw contract.

`namedPages` in `pages.list` reads through this branch's named pages code (`457cdc82d`, `crates/pdfium/src/named_pages.rs`); reuse it.

Kept divergences to re-check: none recorded in this area.

- [ ] Page-space types and the transform, with upstream's `primitives.test.ts` and `helpers.test.ts` cases as Rust unit tests.
- [ ] `pageBoxesOf`/`pageRotationOf` in the engine; wrapper defaults removed (carried item).
- [ ] The boundary module: `result_in_page_space` and `request_in_file_space`, called once in `Host::call`. Port the arms for this task's kinds, and leave a match that fails to compile for a result type with no conversion, as upstream's does.
- [ ] `pages.list`: `ref`, `tabOrder`, all five boxes in page space, `pdfCropBox` in PDF space, `namedPages`. A 90-degree page still reads 612 by 792.
- [ ] Actions and destinations: page-space destinations, and `mapDocumentActions` key order (the triggers before `nameTreeScripts`, `openAction`, `openDestination`).
- [ ] Render kinds take a page-space target and convert with `pdfRenderTargetOf`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED` if listed; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test pages_corpus --test action_reader --test submit_flags --test render_geometry --test render_pixels --test rotation_normalization --test geometry --test wire_contract` passes (owns `pages_corpus::the_listing_agrees_with_the_typescript_engine`), and `CARGO_BUILD_JOBS=6 cargo test -p pdfium` passes.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.5: Dates and the metadata patch types

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs nothing. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Cross-cutting changes" (Dates) and "Security, scope and schema". Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except the ts-rs output named below. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Upstream has one `IsoDateTime` type and the `createdAt`/`modifiedAt` names everywhere (fd175ba50). A PDF date reads as ISO 8601 with the offset the file wrote: `+01:00` stays `+01:00`, `Z` or a zero offset reads `Z`, no offset stays none, and there are no milliseconds. A write takes ISO with or without an offset (`IsoDateTimeSchema` is `datetime({ offset: true, local: true })`) and stores the offset given; UTC is written `+00'00'`. This task ports the formatter and the patch types. The readers adopt it in 8.6, 8.8, 8.12 and 8.19.

Files: `crates/epdf-engine/src/clock.rs` (`format_pdf_date`, a new `pdf_date_to_iso`), `src/dto.rs` (`IsoDateTime`, `MetadataPatch`, a new `CustomMetadataPatch`), `crates/epdf-engine/tests/schema_parity.rs`, and the ts-rs output `packages/engine/core/src/dto/generated/MetadataPatch.ts`, plus any new file ts-rs writes beside it. That `packages/**` change is required, for three reasons. ts-rs writes those files from `dto.rs` during `cargo test`. `packages/engine/core/src/dto/DocumentMetadata.ts` imports `DocumentMetadataTrapped` from that directory. And `.github/workflows/engine-rust.yml:97` fails CI when they differ from the Rust types.

Vectors: `schema/MetadataPatch.json`, `schema/CustomMetadataPatch.json`.

Upstream reference: `packages/engine/services/src/shared/pdf-date.ts` (`pdfDateToIso`, `formatPdfDate`), `packages/engine/core/src/dto/IsoDateTime.ts`, `IsoDateTime.schema.ts`, `MetadataPatch.ts`, `CustomMetadataPatch.ts`, `CustomMetadata.ts`, `DocumentMetadata.ts`, `packages/engine/core/src/wire/schemas.ts` (`MetadataPatchSchema`, `CustomMetadataPatchSchema`). Tests to port: `packages/engine/main/test/pdf-date.test.ts`, `dates.test.ts`. Commits: fd175ba50, a4b8259f7.

Carried items owned here. From Task 2: `dto/generated/MetadataPatch.ts` is stale against upstream (it still has `created`, `modified` and `custom`). From Task 5: `schema_parity.rs` needs `CustomMetadataPatch`, and dates follow fd175ba50 (offset kept, no `.000`, PDF dates to ISO).

Kept divergences to re-check: none recorded in this area.

- [ ] `pdf_date_to_iso` and `format_pdf_date`, with every case of `pdf-date.test.ts` and `dates.test.ts` as a Rust unit test. Nothing normalizes to UTC.
- [ ] `MetadataPatch`: `createdAt` and `modifiedAt`, no `custom`, strict. `CustomMetadataPatch`: a map of string to string or null; a null map is refused.
- [ ] `schema_parity.rs`: `check::<MetadataPatch>` on the rewritten cases, and a new `check::<CustomMetadataPatch>` replaying `schema/CustomMetadataPatch.json`.
- [ ] Regenerate the ts-rs files (`cargo test -p epdf-engine --lib` writes them) and commit them on their own.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test schema_parity --lib` passes (owns `metadata_patch_agrees_with_its_zod_schema`). If callers of the renamed fields fail to compile, rename at the call site only and leave the behaviour to 8.6.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.6: The change queue and metadata

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.2, 8.4 and 8.5. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Cross-cutting changes" (`doc.apply`) and "Metadata".

Form effects, page structure edits, attachment writes and metadata are changes on one queue, applied by `document.apply` (2445caba7, 98e5c270c, 119685ea4, c2d7a5194). A change is all or none: one refused op refuses the whole change, and the ops before it are reversed. `document.applyChanges` runs a server request's changes, each in its own layer transaction, with one outcome per change. Each op's places convert from page space right before it runs, and its answer converts back right after (8.4's boundary). This task builds the queue and the mutation meta every write answers with, and ports metadata on it. The family sub-tasks register their own ops.

Files: a new `crates/epdf-engine/src/features/changes/` (applier, ledger, op table, reverse steps), `src/mutation.rs` (upstream's `MutationMeta`: changed refs with their page; `weakRefsInvalidated` and `shouldRefetch` gone), `src/revision.rs`, `src/host.rs`, `src/page_space.rs` (the `document.apply` arms), `src/features/metadata.rs`, `src/features/metadata_write.rs`, `crates/epdf-engine/tests/metadata_corpus.rs`, `tests/metadata_write.rs`, a new `tests/document_apply.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: `metadata/read.json`, `metadata/update.json`.

Upstream reference: `packages/engine/core/src/mutation/Change.ts`, `changeOutcome.ts`, `MutationMeta.ts`, `WriteOptions.ts`, `MetadataUpdateResult.ts`, `CustomMetadataUpdateResult.ts`, `packages/engine/core/src/wire/changeSchemas.ts`, `packages/engine/services/src/features/changes/` (`ChangeApplier.ts`, `ChangeLedger.ts`, `ChangeRecord.ts`, `recordCodec.ts`, `internal/changeContext.ts`, `internal/metadataChanges.ts`), `packages/engine/services/src/features/metadata/`, `packages/engine/core/src/dto/DocumentMetadata.ts`, `CustomMetadata.ts`, and `worker-protocol.ts` (`DocumentApplyWorkerRequest`, `DocumentApplyChangesWorkerRequest`). Tests to port: `packages/engine/core/test/mutation/change.test.ts`, `packages/engine/main/test/changes.test.ts`, `write-transactions.test.ts`, `metadata.test.ts`. Commits: 2445caba7, b69c40377, 119685ea4, a4b8259f7, fd175ba50, bc0057f3a.

Op table. Register only `metadata.update` and `metadata.updateCustom` here. Every other op type in `Change.ts` answers `NotImplemented` naming its owner until that owner registers it:

| Owner | Op types |
|---|---|
| 8.8 | `attachments.*` |
| 8.14 to 8.16 | the annotation ops |
| 8.18 | `pages.*` |
| 8.20 | `forms.setValue`, `forms.setDisplay`, `forms.setAppearanceText`, `forms.reset` |
| 8.21 | the other `forms.*` ops |
| 8.22 | `annotations.import`, `forms.import`, `forms.importValues` |
| 8.24 | `outline.*` |
| 8.26 | `forms.setSignatureAppearance` |

Keep that list in one place in the op table so the owners find it.

Kept divergence to re-check: no-op flatten and redaction answers carry `mutationSequence` where TS sends only `{tag, result}` (`87b61b533`, kept 2026-10-15). Make sure `MutationMeta` and the answer envelope leave room for it; 8.17 and 8.23 keep it.

- [ ] The queue: ops in order, page-space conversion per op, all or none with reverse steps, and `document.applyChanges` with one outcome per change. Port the `change.test.ts` and `changes.test.ts` cases that need only metadata ops, as `tests/document_apply.rs`.
- [ ] `MutationMeta` as upstream's, shared by every write answer.
- [ ] `metadata.read` with `createdAt`/`modifiedAt` through `pdf_date_to_iso`, keeping the file's offset (`rich` reads `2024-01-15T09:30:00+01:00`); custom keys through `metadata.readCustom`.
- [ ] `metadata.update` and `metadata.updateCustom` as kinds and as ops. Refuse `custom-key-shadowing-a-standard-one`, `custom-key-with-a-slash` and `custom-key-outside-ascii` with `InvalidArg`.
- [ ] `metadata_corpus.rs` and `metadata_write.rs` replay the new shapes, and `metadata_write.rs` replays `request`. The generator folds `metadata.custom.get()` back into the read (our 1b8c92a3f), so the replay does the same.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test metadata_corpus --test metadata_write --test document_apply --test wire_contract` passes (owns `metadata_agrees_with_the_typescript_engine`, `an_updated_document_reads_back_as_the_typescript_engines_did`, `a_clear_and_an_empty_value_are_the_same_edit`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.7: Security and scope

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.1 and 8.2. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Security, scope and schema", and the section 8.1 added.

Files: `crates/epdf-engine/src/features/security/` (`state.rs`, `reader.rs`), `src/auth/scope/` (`resolver.rs`, `pdf_bits.rs`, `types.rs`, `parser.rs`), `src/host.rs`, `crates/epdf-engine/tests/security_state.rs`, `tests/security_reader.rs`, `tests/security_scope.rs`, `tests/scope_conformance.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/native_engine.rs` and `tests/security.rs` where scope enforcement lives.

Vectors: `security/state.json`, `scope/resolve.json`, `scope/parse.json` (as 8.1 regenerated them).

Upstream reference: `packages/engine/core/src/auth/scope/` (`resolver.ts` with `filterMatches`, `builders.ts` with `materializePdfPermissions`, `pdf-bits.ts`, `types.ts`), `packages/engine/services/src/features/security/`, `packages/engine/services/src/shared/securityPermissions.ts`. Tests to port: `packages/engine/core/test/auth/`, `packages/engine/main/test/permissions.test.ts`, `security-collab.test.ts`. Commits: 5443a5649 (`canAssemble` with bit 4 or 11; `doc.forms.fill` and `doc.pages.assemble` on more bit sets), a10ed2659 (`doc.text.select`, `doc.text.search`), 4b2cdb395 (`doc.sign`; `filterMatches` drops the membership test), 67ccb76dc (`doc.outline.modify`).

The `group-filter-needs-membership` case keeps its name, but upstream now allows it, and its `covers` text says the rule changed (our 016af6547). Follow the vector.

Kept divergence: `a4ed2b228` enforces document scopes natively and keeps a locked document open until it is unlocked. Keep it as is: the user decides it at Task 14 (controller ruling).

- [ ] `materializePdfPermissions` and the advisory: `annotate-and-modify` reads `canAssemble: true`.
- [ ] The resolver: the new capabilities in every expansion, `doc.sign` and `doc.sign.certify` in the closed set, and `filterMatches` without the membership test.
- [ ] Both kinds answer with upstream's envelope; remove them from `NOT_YET_ROUTED` if listed; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test security_state --test security_reader --test security_scope --test scope_conformance --test wire_contract` passes (owns `the_advisory_agrees_with_typescript`, `resolution_agrees_with_typescript`, `the_closed_capability_set_is_the_same_set`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.8: Attachments and piece info

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.5 and 8.6. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Files".

Files: `crates/epdf-engine/src/features/files/` (`attachments.rs`, `piece_info.rs`, `mod.rs`), `src/features/changes/` (register `attachments.create` and `attachments.delete`), `src/host.rs`, `crates/epdf-engine/tests/attachment_reader.rs`, `tests/piece_info_reader.rs`, `tests/files_write.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: `files/attachments.json`, `files/piece-info.json`, `files/write.json`.

Upstream reference: `packages/engine/core/src/dto/Attachment.ts`, `Attachment.schema.ts`, `PieceInfo.ts`, `packages/engine/core/src/mutation/AttachmentMutationResults.ts`, `packages/engine/services/src/features/attachments/`, `packages/engine/services/src/features/changes/internal/attachmentChanges.ts`, `packages/engine/services/src/features/pieceinfo/PieceInfoAccessor.ts`. Tests to port: `packages/engine/main/test/attachments.test.ts`, `pieceinfo.test.ts`. Commits: a9434c0e2 (attachment `ref`, `{attachments: [...]}` listing, `pieceInfo.delete`), c2d7a5194 (attachment writes as change ops), fd175ba50 (dates), bd4976e4e (absent fields read `null`).

This branch made pieceInfo writes resolve void (`1c82e6d0c`); check that against upstream's answer and keep or change it to match.

Kept divergences to re-check: none recorded in this area.

- [ ] Attachments: `ref` for `key`; `createdAt` through `pdf_date_to_iso` (`D:20240401090000Z` reads `2024-04-01T09:00:00Z`); `modifiedAt`; `null` for absent fields; `mimeType: null` for a created file with no type; `description` on created entries.
- [ ] `attachments.create` and `attachments.delete` as change ops.
- [ ] Piece info: `modifiedAt` without milliseconds, and `pieceInfo.delete` in place of `pieceInfo.clear`.
- [ ] The three tests replay the new shapes; `files_write.rs` reads `{attachments: [...]}`. `files/write.json` records no `request`: its cases drive the engine through the TS API, not one wire request. `files_write.rs` keeps calling the Rust feature functions with the case's inputs and compares the recorded outcome. For a case whose TS call became a `document.apply` change (attachment create and delete), build that change from the case and send it through `Host::call`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test attachment_reader --test piece_info_reader --test files_write --test wire_contract` passes (owns `attachment_reader::the_listing_agrees_with_the_typescript_engine`, `piece_info_reader::the_reads_agree_with_the_typescript_engine`, `a_files_write_leaves_what_the_typescript_engine_left`).
- [ ] Legacy path (8.2): delete the `Host::call_legacy` arms of `attachments.create`, `attachments.delete` and `pieceInfo.clear`. Move every caller to the new kind or op: the `translate.rs` arm, and the tests 8.2 repointed (`grep -rn call_legacy crates/epdf-engine/tests crates/epdf-viewer-host`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.9: Page geometry and text

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.4. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Text and geometry". Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows), and `RustPageGeometryReader.ts` only if the step below finds it must change. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

The geometry snapshot was renamed and moved to page space. Runs use `start` for `charStart` and a page-space `rect`. Glyphs use `loose`/`tight` boxes or quads for `looseBox`/`looseQuad`/`tightBox`/`tightQuad`, and `space`/`empty` booleans for the `flags` bits. A rotated run gives `rotation` in clockwise degrees (0.5236 rad is now 330). The PDFium runtime `1461b0c51` shortens loose boxes (bounded by `/FontBBox`, runtime-src `2558d5d97`, `ff5e04acc`) and drops non-finite text (`06fbf69b3`, probable). Rust links that runtime since Task 6, so those values should come out the same with no Rust change.

The Rust reader returns PDF-space values (upstream's `PageGeometrySnapshot<PdfCoordinates>` with the new names), and the host boundary turns them into page space. Two other callers read the Rust reader directly: the C ABI `epdf_page_geometry` in `crates/epdf-wasm`, and, through it, `packages/engine/services/src/features/geometry/RustPageGeometryReader.ts`, which the TS worker host calls and so converts with the TS boundary.

Files: `crates/epdf-engine/src/features/geometry.rs`, `src/features/text/reader.rs`, `src/page_space.rs` (the `pages.geometry` arm), `src/host.rs`, `crates/epdf-engine/tests/geometry_corpus.rs`, `tests/text_corpus.rs`, `tests/wire_contract.rs`, `crates/test-support/src/corpus.rs` (the comparator, if it names the old fields), `crates/epdf-wasm/src/` (the geometry export, if it reshapes JSON), `crates/epdf-wasm/tests/borrowed_page.rs`, `crates/epdf-viewer-host/src/translate.rs`, and `crates/epdf-ffi/src/page.rs` if it reads the geometry JSON.

Vectors: `pages.geometry/*.json` (6 files), `pages.text/*.json` (6 files).

Upstream reference: `packages/engine/core/src/dto/PageGeometrySnapshot.ts`, `PageTextSnapshot.ts`, `packages/engine/core/src/pageSpace/text.ts`, `packages/engine/services/src/features/geometry/PageGeometryReader.ts`, `RustPageGeometryReader.ts`, `packages/engine/services/src/features/text/PageTextReader.ts`, `pageSpaceBoundary.ts` (`pageGeometryOf`). Tests to port: `packages/engine/main/test/page-geometry-reader.test.ts`, `geometry-orientation.test.ts`, `page-text.test.ts`. Commits: a10ed2659, 00d2c8760, 3bad82c8d (names and page space), 6153c181e (clockwise degrees). PDFium: the `runtime-src` submodule at `1461b0c51`.

Carried item owned here (Task 2): `RustPageGeometryReader.ts:58-68` casts the Rust JSON to `PageGeometrySnapshot<PdfCoordinates>`, but the Rust JSON was the old shape. Once the Rust reader emits upstream's PDF-space shape, the cast is true and the TS file needs no change. Check it field by field against `PageGeometrySnapshot.ts`. Change the TS file only if a field can't be emitted from Rust, and give the reason in the commit message.

Kept divergences to re-check: none recorded in this area.

- [ ] The reader emits upstream's names in PDF space, and the boundary turns them into page space.
- [ ] `geometry_corpus.rs` replays the page-space vectors through the boundary (`flags` is now `space`/`empty`).
- [ ] `borrowed_page.rs`: the C ABI answer is PDF space, so convert it with the engine's page-space transform before comparing it with the page-space vectors.
- [ ] `RustPageGeometryReader.ts` checked (carried item).
- [ ] Remove `pages.geometry` and `pages.text` from `NOT_YET_ROUTED` if listed; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test geometry_corpus --test text_corpus --test wire_contract` passes (owns `the_corpus_exercises_both_run_variants_and_empty_glyphs`, `every_corpus_page_matches_the_typescript_reader`), and `CARGO_BUILD_JOBS=6 cargo test -p epdf-wasm --test borrowed_page` passes (owns `the_borrowed_page_export_matches_the_frozen_vectors`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p epdf-wasm -p test-support --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host -p epdf-wasm -p test-support`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.10: Text layout in page space

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.9. Rust matches upstream's TypeScript at `b456ee4fd`; the per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Text and geometry" (the `text/layout.json` paragraphs) and "Generator fixes in this round". Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**`, `tooling/**` or `.github/**` changes. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

`text/layout.json` is now page space, with envelope `schemaVersion: 2` (generator fixed in our ea6b5a115). Quads are `PageQuad`s with named corners, segment rects are page boxes, and hit probes are page-space points. The generator runs upstream's `createTextLayout`, which mirrors the page-space runs with `mirroredRun`, runs the PDF layout, and flips each answer back. Frames are keyed on the y-up runs, so `frameCount` stays 4. Some answers moved with the geometry, not with the layout code (for example `report` page 0, where glyph 211's line is `[197, 211]` and word range `(207, 6)` draws as 2 segments). The diff report lists them. They are not layout bugs to chase.

Files: `crates/epdf-engine/src/features/text/layout.rs`, `src/features/text/mod.rs`, `crates/epdf-engine/tests/text_layout.rs`, `tests/charmap.rs` (unchanged vector, keep passing).

Vectors: `text/layout.json`, `text/charmap.json`.

Upstream reference: `packages/engine/core/src/text/layout.ts` (`createPdfTextLayout`), `packages/engine/core/src/pageSpace/text.ts` (`createTextLayout`, `mirroredRun`), `packages/engine/core/src/text/TextRange.ts`, `charmap.ts`. Tests to port: `packages/engine/core/test/pageSpace/text.test.ts`, `packages/engine/core/test/text/`. Commits: a10ed2659, 00d2c8760, 3bad82c8d; our ea6b5a115.

Carried item owned here (Task 5): `text_layout.rs` moves to page space and `schemaVersion` 2.

Kept divergences to re-check: none recorded in this area.

- [ ] A page-space layout entry that mirrors runs as `createTextLayout` does, over the existing PDF-space layout.
- [ ] `text_layout.rs` reads `schemaVersion` 2, named-corner quads, page-box segments and page-space probes.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test text_layout --test charmap` passes. This owns all 13 `text_layout` tests: `a_degenerate_glyph_has_no_quad_and_a_real_one_does`, `glyph_quads_and_ranges_agree_with_typescript`, `an_empty_range_produces_no_segments`, `a_hit_at_a_glyphs_own_centre_finds_a_glyph`, `the_layout_shape_agrees_with_typescript`, `every_segment_rect_bounds_its_own_quad`, `the_run_and_glyph_counts_are_consistent`, `the_corpus_covers_more_than_one_orientation_frame`, `a_hit_far_outside_the_page_finds_nothing`, `segments_agree_with_typescript`, `a_word_can_cross_a_line_boundary_between_abutting_runs`, `word_and_line_ranges_contain_their_glyph_and_stay_on_the_page`, `hit_tests_agree_with_typescript`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.11: Search

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.1, 8.4 and 8.9. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each slice-protocol case's recorded `request`, not its `case`. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Search", and the section 8.1 added.

A range's `length` is `count`, and a snippet is `{before, match, after}`. Slice keys spell the mode `snippets` or `matches` and carry `ignoreWhitespace`. `search.query` takes a flattened `SearchScanRequest`: query fields at the top level, `snippets`, `from` as a `PageRef`, and the cursor string. The cursor key is upstream's `searchQueryKey`, encoded with `encodeSearchCursor`. A conflicting start page is refused with "`from` conflicts with the cursor". Match rects are page space (3bad82c8d). This branch already ports `ignoreWhitespace` (`29fe12ddb`); adapt it to the new key and request.

Files: `crates/epdf-engine/src/features/search/` (`mod.rs`, `cursor.rs`, `fold.rs`, `literal.rs`, `dialect.rs`, `snippet.rs`, `matcher.rs`, `reader.rs`), `src/page_space.rs` (`pageSearchSliceOf`), `src/host.rs`, `crates/epdf-engine/tests/search_dialect.rs`, `tests/search_slice_protocol.rs`, `tests/search_reader.rs`, `tests/search_ignore_whitespace.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: `search/fold.json`, `search/literal.json`, `search/regex-dialect.json`, `search/snippet.json`, `search/slice-protocol.json` (with the two `ignoreWhitespace` cases 8.1 re-added).

Upstream reference: `packages/engine/core/src/search/` (`fold.ts`, `literal.ts`, `regex.ts`, `snippet.ts`, `matcher.ts`, `types.ts`, `epoch.ts`), `packages/engine/services/src/features/search/` (`SearchReader.ts`, `internal/searchCursor.ts`). Tests to port: `packages/engine/core/test/search/`, `packages/engine/core/test/wire/searchToken.test.ts`, `packages/engine/main/test/search.test.ts`, `search-ignore-whitespace.test.ts`. Commits: a10ed2659, f082bf3e4, 3bad82c8d; our 0963cacd3 and 26887941a.

Carried items owned here (Task 5): `search_slice_protocol.rs` reads the flattened `request`. No slice-protocol case mints a cursor with mode `matches` (the `snippets: false` key path), so add a Rust unit test for that key.

Kept divergences to re-check: none recorded in this area. (S4 and S6 in the review's Lows are plugin-side and shared with TS.)

- [ ] `count`, the snippet shape and the key spelling in the dialect, fold and literal code.
- [ ] The flattened request, `from` as a `PageRef`, upstream's cursor key and its refusals.
- [ ] Page-space match rects at the boundary.
- [ ] Remove `search.query` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test search_dialect --test search_slice_protocol --test search_reader --test search_ignore_whitespace --test wire_contract` passes. This owns `snippets_agree_with_typescript`, `the_fold_vectors_keep_the_properties_the_anchor_stage_rests_on`, `folded_ranges_translate_back_the_same_way`, `the_fold_agrees_with_typescript`, `the_rust_engine_reproduces_the_javascript_dialect`, `the_dialect_corpus_covers_what_the_entry_gate_asks_for`, `literal_search_agrees_with_typescript`, `a_snippet_locates_its_match_inside_itself` and `every_slice_protocol_case_agrees_with_the_oracle`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.12: Annotation read, vocabulary and the save round trip

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.4, 8.5 and 8.19 (the `forms.list` widget shape the reader test compares against). Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Annotation" (the shared record table, `read.json`, `vocabulary.json`) and "Extract and save".

Every annotation record changed shape. From the diff report's table:
- `rect` and every point are in page space (1a243431a).
- Colours are `"#rrggbb"` strings (c053c3d33).
- Quad corners are `upperLeft/upperRight/lowerLeft/lowerRight` (00d2c8760).
- `rotation` is in clockwise degrees, so a stored 30 reads 330 (2bdc74fbf). It is taken from the appearance through `appearanceTurnOf`, not from `/EMBD_Metadata /Rotation` (2bdc74fbf, 6153c181e).
- Box kinds carry `box`; `unrotatedRect` and `rectDifferences` are gone (2bdc74fbf).
- `ref` is an `AnnotationRef` (595bdc6c7, bc0057f3a).
- Flags are flattened to top-level booleans (c78997346).
- Dates are `createdAt`/`modifiedAt` (fd175ba50).
- `reply`, `popup`, `importedBy` and `modifiedBy` are new, and an empty `createdBy`/`userId`/`groupId` reads `null` (c78997346).
- `hasAppearance` and `appearanceState` are new (25dd21f38, 17350fb4c).
- `actions` is always present and no field is left out (bd4976e4e). Opacity reads as written (bd4976e4e).

`annotations.list` replaces the three `listRaw*`/`listFullPage` kinds and returns no widgets (83f6598d2, 1233d1349). `/Popup` reads as `popup`, with `parent` and `open`. A read has no `pageState`. The `stampName` codec is gone (0427e3dc9), and `/Name` passes verbatim. `FreeTextTypeWriter` decodes as `free-text-typewriter` (404b79b3e). FreeText records carry `richText` and records carry `measure`; this branch already reads both (`347f806ae`, `3b795d4ac`, `818eba296`, `635f7d303`), so check them against the vector and adapt.

Files: `crates/epdf-engine/src/features/annotation/read/` (all of it), `annotation/rich_text.rs`, `annotation/vocabulary.rs`, `annotation/flags.rs`, `annotation/intent.rs`, `annotation/state.rs`, `annotation/subtype.rs`, `src/revision.rs` (drop the read's `PageState`/weak state if nothing else needs it), `src/identity.rs`, `src/page_space.rs` (`pageAnnotationOf`), `src/host.rs`, `crates/epdf-engine/tests/annotation_reader.rs`, `tests/annotation_vocabulary.rs`, `tests/document_save.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: `annotation/read.json`, `annotation/vocabulary.json`, `save/roundtrip.json`.

Upstream reference: `packages/engine/core/src/annotation/` (`base.ts`, `defaults.ts`, `appearanceTurn.ts`, `drawnPoints.ts`, `kinds/`, `color.ts`, `relationships.ts`), `packages/engine/core/src/pageSpace/annotations.ts`, `pageSpace/helpers.ts` (`appearanceTurnOf`), `packages/engine/core/src/dto/RichText.ts`, `Measure.ts`, `packages/engine/core/src/identity/AnnotationRef.ts`, `annotationKey.ts`, `packages/engine/services/src/features/annotations/` (`AnnotationReader.ts`, `RawAnnotationReader.ts`, `internal/`), `packages/engine/services/src/features/save/`. Tests to port: `packages/engine/core/test/annotation/`, `packages/engine/core/test/pageSpace/annotations.test.ts`, `packages/engine/main/test/annotations.test.ts`, `annotation-rotation.test.ts`, `rich-text.test.ts`, `color.test.ts`. Commits: as listed above.

Kept divergences to re-check:
- From the FreeText rich-text port (`818eba296`, kept 2026-10-14): the reader resolves registered fonts through a thread-local copy of the font registry; rich-text input styles stay untyped JSON; a font identity falls back to its registration name when PDFium has none. Keep all three, and say so in the report.
- Part of N6 (`343fc063e`): a ref of unknown kind fails rather than keying `"undefined"`. That lives in `crates/plugins/`; check only that the engine's ref decoding refuses an unknown kind too.

- [ ] The record in upstream's shape, read in PDF space and converted at the boundary.
- [ ] `annotations.list` (no widgets) and `annotations.readFile`; the three removed list kinds' feature code deleted or folded in.
- [ ] Vocabulary: drop `stampName`, add `free-text-typewriter`. `annotation_vocabulary.rs` stops asking for the `stampName` map.
- [ ] `annotation_reader.rs` replays the new record. Widgets are compared through `forms.list`, as the generator reads them (our 0044ea9b9). A test whose subject is gone (`the_page_state_agrees_including_the_weak_flag`) is rewritten to what replaced it, or deleted with the reason in the commit message.
- [ ] `document_save.rs`: `annotationCount` on `annotation_kinds.pdf` is 37.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_reader --test annotation_vocabulary --test document_save --test wire_contract` passes. This owns `the_corpus_covers_every_subtype_that_has_a_reader`, `the_page_state_agrees_including_the_weak_flag`, `a_widget_is_joined_to_the_field_that_owns_it`, `a_rect_is_reported_in_the_corner_order_the_file_wrote`, `a_reply_edge_carries_a_type_exactly_when_it_carries_a_parent`, `annotation_reader::the_reads_agree_with_the_typescript_engine`, `the_subtypes_with_no_reader_read_as_unsupported_on_both_sides`, `the_it_names_decode_what_typescript_does`, `the_name_maps_decode_and_encode_what_typescript_does` and `a_saved_document_reopens_as_the_typescript_engines_did`.
- [ ] Legacy path (8.2): delete the `Host::call_legacy` arms of `annotations.listFullPage`, `annotations.listRawAll` and `annotations.listRawPage`. Move every caller to the new kind or op: the `translate.rs` arm, and the tests 8.2 repointed (`grep -rn call_legacy crates/epdf-engine/tests crates/epdf-viewer-host`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.


**Added by controller ruling after 8.2:** resolve `baseIndex` refs to promoted annotations as upstream does (the `EPDFLayer` birth-name branch), binding what `crates/pdfium` lacks. Until this lands, `crates/epdf-engine` resolves only the non-promoted branch, and `redaction.rs` refuses several `baseIndex` refs on one page. Also owns removing `translate::call_host`'s `nm`/`index` to `baseIndex` rewrite once the kernel stops sending them (Task 13 may own the kernel side).

#### Task 8.13: Annotation appearances

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.12. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Annotation" (`appearances.json`).

Entries are addressed by `baseIndex` refs and carry page-space rects. The renderer returns every mode and state (17350fb4c), so `normal` and `scaled` gain rollover and down entries (7 become 11). Annotations without `/AP` are drawn in memory (25dd21f38, probable), so `no-appearances` goes from 0 entries to 34. Each case's eleventh entry is a widget, reached through `page.forms.renderAppearancesRaw` (`tooling/engine-conformance/src/generate-annotation-appearance-vectors.ts:118`). Find the wire kind that sends in upstream's `packages/engine/main`, and route it if it is one of this task's kinds. `annotations.readAppearance` is new (7f0b0a183).

Files: `crates/epdf-engine/src/features/annotation/appearance.rs`, `src/features/render/raster.rs`, `src/page_space.rs` (`pageAppearancesOf`), `src/host.rs`, `crates/pdfium/src/page.rs` (only if the render call needs it), `crates/epdf-engine/tests/annotation_appearance.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vector: `annotation/appearances.json`.

Upstream reference: `packages/engine/services/src/features/annotations/AnnotationAppearanceReader.ts`, `packages/engine/core/src/dto/AnnotationRender.ts`, `packages/engine/core/src/pageSpace/rendering.ts`, `packages/engine/core/src/annotation/appearanceTurn.ts`, `pageSpaceBoundary.ts` (`pageAppearancesOf`). Tests to port: `packages/engine/main/test/annotations-appearance.test.ts`, `appearance-states.test.ts`, `appearance-clamp.test.ts`. Commits: 17350fb4c, 25dd21f38, 7f107fd7c, 7f0b0a183, 1233d1349; runtime 5e9241945 (the turn comes from the caller).

Stub removed: `crates/epdf-engine/src/features/annotation/appearance.rs:258` (`// re-port (Task 8): EPDF_RenderAnnotBitmapUnrotated now takes the turn`). Pass the turn and unrotated box from `appearanceTurnOf` through `PageView::render_annotation_rgba_with_matrix(state, Some(AppearanceTurn { degrees, unrotated_box }))`.

Carried item owned here (Task 6): `render_one` skips a turned box annotation silently, with no per-annotation skip record. Removing the stub ends that skip. If any other annotation is still skipped, record it the way upstream's reader does, or say in the report why upstream also skips silently.

Kept divergences to re-check: none recorded in this area.

- [ ] Every mode and state, in-memory appearances for annotations without `/AP`, `baseIndex` refs, page-space rects.
- [ ] Turned box annotations render (stub removed).
- [ ] `annotations.readAppearance` routed.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_appearance --test wire_contract` passes. This owns all 10 `annotation_appearance` tests: `a_stripped_appearance_is_drawn_flat_and_fills_its_logical_box`, `a_rotated_box_kind_is_placed_by_its_logical_box_and_the_near_misses_are_not`, `a_scale_multiplies_the_raster_and_leaves_the_placement_alone`, `an_annotation_with_no_appearance_stream_is_absent_rather_than_empty`, `each_mode_draws_its_own_appearance_stream`, `the_appearances_land_where_the_typescript_engine_put_them`, `generating_a_form_fields_appearance_never_reaches_the_reader`, `the_per_appearance_pixel_clamp_reduces_the_scale_rather_than_refusing`, `an_appearance_is_drawn_from_the_annotations_own_place_on_the_page`, `only_the_modes_an_annotation_has_are_emitted`. A test whose premise upstream reversed (for example "absent rather than empty", now that a missing `/AP` is drawn) is rewritten to upstream's rule, with the reason in the commit message.
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/annotation/appearance.rs` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.14: Annotation create

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.6 and 8.12. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Annotation" (`create.json`).

Ruling (ledger, Task 4b): the create vector records the v3 page-space draft actually sent, not the old spelling, and Rust replays it as the wire request. 63 cases became 57. The six `widget-*` cases are gone, because the annotation verbs refuse widgets (1233d1349). `square-degenerate-cloudy` and `polygon-zero-cloud` are refused with `InvalidArg` (c78997346). A created annotation reads `print: true` from the defaults table (69627beb9, probable), and gets a UUID `/NM` at birth, recorded as `"<stamped>"`. Byte inputs moved from `resources.r0` to `resources.appearance` / `resources.file` (11a1259d9). The answer's meta is 8.6's `MutationMeta`.

Files: `crates/epdf-engine/src/features/annotation/write/` (`create.rs`, `draft.rs`, `resolve.rs`, `transform.rs`, `context.rs`, `appearance.rs`, `mutator.rs`), `annotation/rich_text.rs`, `src/features/changes/` (register `annotations.create`), `src/page_space.rs` (`pdfAnnotationDraftOf`), `src/host.rs`, `crates/pdfium/src/annot_write.rs` (`/RD` and opacity), `crates/epdf-engine/tests/annotation_create.rs`, `tests/not_ported_refusals.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vector: `annotation/create.json`.

Upstream reference: `packages/engine/core/src/pageSpace/helpers.ts` (`resolveAnnotationDraft`, `resolveRectCommand`), `packages/engine/core/src/annotation/` (`defaults.ts`, `shapeForRect.ts`, `drawnPoints.ts`, `resolve/`, `kinds/`), `packages/engine/core/src/mutation/AnnotationMutationResults.ts`, `packages/engine/services/src/features/annotations/AnnotationMutator.ts`, `AnnotationBatchApplier.ts`, `internal/`, `packages/engine/services/src/features/changes/internal/annotationChanges.ts`. Tests to port: `packages/engine/main/test/annotations-mutations.test.ts`, `annotation-rect.test.ts`, `annotation-resources.test.ts`, `annotation-write-checks.test.ts`, `annotations-stamp.test.ts`. Commits: c78997346, 69627beb9, 11a1259d9, 1233d1349, 1a243431a, 5f4686dc8 and bc0057f3a (the `/NM` candidates); runtime 5e9241945 (the engine owns `/RD`).

Stubs removed: `crates/epdf-engine/src/features/annotation/write/create.rs:192` (the preflight refusal), `:647` (`apply_caret`), `:806` (`apply_free_text`), `:1682` (`apply_shape`), `:1834` (helper `rect_differences_not_ported`). The runtime no longer has `EPDFAnnot_SetRectangleDifferences` (5e9241945: "the engine owns /RD"), so write `/RD` the way upstream's TS now does. Delete `a_create_with_rect_differences_is_refused_before_the_annotation_exists` from `tests/not_ported_refusals.rs`. 8.15 and 8.21 take the rest of that file.

Carried item owned here (Task 6): `pdfium::Annotation::set_opacity` clamps to 0..1, while upstream passes the value through and lets native refuse it. Drop the clamp and give the error upstream gives.

Kept divergences to re-check: the three rich-text choices listed in 8.12 (`818eba296`).

- [ ] Draft resolution from page space, the defaults table, `/NM` at birth, `resources.appearance`/`resources.file`, the widget refusal, and the two degenerate refusals.
- [ ] `/RD` written by the engine (stubs removed); opacity unclamped.
- [ ] `annotations.create` as a kind and as a change op.
- [ ] `annotation_create.rs` replays the v3 drafts and compares `created` and `meta`.
- [ ] Remove `annotations.create` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_create --test not_ported_refusals --test wire_contract` and `CARGO_BUILD_JOBS=6 cargo test -p pdfium` pass. This owns all 11 `annotation_create` tests: `a_created_annotation_survives_a_save_and_reopen`, `a_create_is_append_only_and_says_so`, `a_review_state_is_written_in_the_spelling_the_pdf_uses`, `a_rotated_stamp_is_authored_in_the_frame_it_is_turned_from`, `a_redaction_gets_a_default_appearance_only_when_it_has_a_label`, `a_stamp_fit_policy_decides_where_the_picture_lands`, `a_created_annotation_reads_back_as_the_typescript_engines_did`, `a_draft_that_cannot_be_written_is_refused_before_anything_is_written`, `a_created_annotation_ships_with_an_appearance`, `the_transform_keys_land_in_the_dictionary_and_not_only_in_the_dto`, `a_new_annotation_is_addressable`.
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/annotation/write/create.rs` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.15: Annotation update and the write verdicts

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.14. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Annotation" (`update.json` and `write.json`).

`update.json`: 44 cases became 41. `caret-clear-rect-differences` went with the inset (2bdc74fbf), and `widget-font-size-only` and `widget-restyle` went with widgets (1233d1349). `polygon-degenerate` is refused with `InvalidArg` (404b79b3e). Two cases swap their appearance verdict. Free-text updates read `contents: ""` (69627beb9). The vector records the v3 patch actually sent (ruling, Task 4b).

`write.json`: the 8 `mutations` cases are gone, because upstream removed weak refs and `computeMutationImpact` (bc0057f3a). Of the 29 `appearance` cases, four change verdict. Three move from `regenerate` to `translation`, because the update resolves a rect patch into the shape before classifying (9d8ebf857); those cases record `resolvedPatch`. `ink-advisory-rotation` moves from `inert` to `regenerate`, because rotation now turns a vertex kind's upright points (6153c181e).

Files: `crates/epdf-engine/src/features/annotation/write/` (`update.rs`, `patch.rs`, `resolve.rs`, `transform.rs`, `appearance.rs`, `impact.rs` (delete), `mod.rs`), `annotation/rich_text.rs`, `src/features/changes/` (register `annotations.update`), `src/page_space.rs` (`pdfAnnotationPatchOf`), `src/host.rs`, `crates/epdf-engine/tests/annotation_update.rs`, `tests/annotation_write.rs`, `tests/not_ported_refusals.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: `annotation/update.json`, `annotation/write.json`.

Upstream reference: `packages/engine/core/src/pageSpace/helpers.ts` (`resolveAnnotationPatch`, `resolveRectCommand`), `packages/engine/core/src/annotation/` (`shapeForRect.ts`, `resolve/`, `appearanceTurn.ts`, `drawnPoints.ts`), `packages/engine/services/src/features/annotations/AnnotationMutator.ts`, `internal/`. Tests to port: `packages/engine/main/test/annotations-rotation.test.ts`, `annotation-rotation.test.ts`, `annotations-stamp-resize.test.ts`, `annotations-text-preservation.test.ts`. Commits: 9d8ebf857, 6153c181e, 2bdc74fbf, 404b79b3e, 69627beb9, bc0057f3a; runtime 5e9241945.

Stubs removed: `crates/epdf-engine/src/features/annotation/write/update.rs:250` (the preflight refusal), `:851` (`apply_shape_patch`), `:1002` (`patch_rect_differences`), `:1464` (helper). Upstream no longer has `rectDifferences` as a patch field (2bdc74fbf). Port what upstream's update does to `/RD` when a box kind's rect or shape changes. Delete `an_update_with_rect_differences_is_refused_before_the_identity_stamp` from `tests/not_ported_refusals.rs`.

Kept divergences to re-check: the three rich-text choices listed in 8.12 (`818eba296`).

- [ ] Patch resolution from page space into the shape, then the verdict. `annotations.update` as a kind and as a change op.
- [ ] `/RD` stubs removed. Delete the weak-ref impact code and its tests (`the_mutation_impacts_agree_with_the_typescript_engine`, `an_index_shifting_mutation_needs_a_scanned_page`): upstream removed their cases, and the commit message says so.
- [ ] `annotation_update.rs` replays the v3 patches; `annotation_write.rs` replays the `appearance` cases with `resolvedPatch`.
- [ ] Remove `annotations.update` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_update --test annotation_write --test not_ported_refusals --test wire_contract` passes. This owns `an_update_is_not_structural`, `an_updated_annotation_reads_back_as_the_typescript_engines_did`, `a_moved_stamp_refits_the_picture_it_already_had`, `the_appearance_verdict_agrees_with_the_typescript_engine`, `a_patch_for_the_wrong_subtype_is_refused`, `a_patched_review_state_is_written_in_the_spelling_the_pdf_uses`, `the_appearance_verdicts_agree_with_the_typescript_engine`, and the two deleted `annotation_write` tests above.
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/annotation/write/update.rs` prints nothing.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.16: Annotation delete and reorder

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.6 and 8.12. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Annotation" (`delete.json` and `move.json`).

Deleting an annotation deletes its thread and popups in one change (bd4976e4e). `by-object-number` and `by-index-onto-a-durable-annotation` also remove `a-reply`, `a-group-member` and `stamped-by-embedpdf`, so `remaining` drops from 8 to 5 and `meta.changed` lists 4 refs. The `deleted` field is gone. `annotations.reorder` replaces `annotations.move` (1233d1349): `movedIndices`/`movedOrder` become `order`. The reorder no longer stamps `/NM` (bc0057f3a), so the `afterOrder` entries the old engine named are `null` or `""`.

Files: `crates/epdf-engine/src/features/annotation/write/delete.rs`, `write/move_.rs` (rename to reorder), `write/mutator.rs`, `src/features/annotation/read/relationship.rs`, `src/features/changes/` (register `annotations.delete` and `annotations.reorder`), `src/revision.rs`, `src/identity.rs`, `src/host.rs`, `crates/epdf-engine/tests/annotation_delete.rs`, `tests/annotation_move.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: `annotation/delete.json`, `annotation/move.json`.

Upstream reference: `packages/engine/core/src/annotation/relationships.ts`, `packages/engine/core/src/mutation/AnnotationMutationResults.ts`, `AnnotationListMutationMeta.ts`, `ListPosition.ts`, `packages/engine/services/src/features/annotations/AnnotationMutator.ts`, `packages/engine/services/src/features/changes/internal/annotationChanges.ts` (`deleteAnnotation`, `reorderAnnotations`, `reorderBack`, `restoreAnnotations`). Tests to port: `packages/engine/main/test/annotations-mutations.test.ts` (the delete and reorder cases), `packages/engine/core/test/mutation/listPosition.test.ts`. Commits: bd4976e4e, 1233d1349, bc0057f3a, 595bdc6c7.

Kept divergences to re-check: none recorded in this area.

- [ ] Delete with its thread and popups in one change, with the reverse step for all or none.
- [ ] `annotations.reorder` with `ListPosition`; `move_.rs` renamed; no `/NM` stamping.
- [ ] Both as kinds and as change ops; both tests replay `request`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_delete --test annotation_move --test wire_contract` passes. This owns `a_reference_that_names_nothing_is_an_invalid_reference`, `a_delete_removes_the_annotation_the_typescript_engine_removed`, `a_move_is_structural_and_says_so`, `the_block_lands_where_it_was_asked_to_in_the_order_it_was_given`, `a_move_that_changes_nothing_is_still_structural`, `a_move_refuses_what_it_cannot_do_and_changes_nothing`, `a_move_leaves_the_order_the_typescript_engine_left`.
- [ ] Legacy path (8.2): delete the `Host::call_legacy` arms of `annotations.move`. Move every caller to the new kind or op: the `translate.rs` arm, and the tests 8.2 repointed (`grep -rn call_legacy crates/epdf-engine/tests crates/epdf-viewer-host`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.17: Flatten and appearance export

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.1, 8.13 and 8.16. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields.

The kinds: `pages.flatten`, `annotations.flatten` and `annotations.exportAppearance`. This branch ported the last two against the pre-catch-up TS (`93ee01678`), naming annotations by `nm` refs, which upstream removed (bc0057f3a). 8.1 re-added their eight `pages/mutate.json` cases in the current API, with recorded requests. This task adapts the Rust to upstream's refs, results and meta. 8.18 owns `pages_mutate.rs`, which replays these cases with the rest of the file. Here, add a test that replays just the flatten and export cases, so this task's done criteria don't wait on 8.18.

Files: `crates/epdf-engine/src/features/pages_write.rs` (`FlattenUsage`, page flatten), `src/features/annotation/` (flatten and appearance export), `src/host.rs`, `src/page_space.rs`, `crates/epdf-engine/tests/annotation_flatten.rs`, a new `tests/flatten_corpus.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: the `flatten-*`, `flatten-annotations-*` and `export-appearance*` cases of `pages/mutate.json`.

Upstream reference: `packages/engine/services/src/features/annotations/AnnotationFlattener.ts`, `packages/engine/services/src/features/pages/PagesFlattener.ts`, `packages/engine/core/src/mutation/AnnotationFlattenResult.ts`, `PageFlattenResult.ts`, `packages/engine/core/src/wire/flatten.ts`, and `worker-protocol.ts` for the three kinds. Tests to port: `packages/engine/main/test/annotation-flatten.test.ts`, `annotation-appearance-export.test.ts`, `page-flatten.test.ts`, `packages/engine/core/test/wire/flatten-schema.test.ts`. Commits: `git log 2516e2786..b456ee4fd -- <each reference file>`.

Kept divergences to re-check (kept by the user; keep each unless upstream now does the same, and record which in the report): `annotations.flatten` bumps the sequence twice, copying the TS (`dd2e1c290`); a no-op flatten answer carries `mutationSequence` where TS sends only `{tag, result}` (`87b61b533`).

- [ ] The three kinds take upstream's refs and answer upstream's results and meta.
- [ ] `annotation_flatten.rs` uses the new refs; `flatten_corpus.rs` replays the flatten and export cases' `request`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED` if listed; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_flatten --test flatten_corpus --test wire_contract` passes (owns `a_flatten_that_applies_nothing_is_no_mutation`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.18: Page structure, named pages and extract

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.1, 8.6 and 8.17. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Pages" and "Extract and save", and the section 8.1 added.

Page structure goes through `document.apply`. `pages.rotate`, `pages.reorder` (was `pages.move`), `pages.delete`, `pages.insertBlank` and `pages.insert` are ops, positions are neighbour positions (`ListPosition`), and `inserted` holds refs. `pages.setName` and `pages.removeName` are kinds; this branch ported them (`457cdc82d`), so adapt them to upstream's `PageNameInput`/`PageNameResult`. `pages.extract` stays a kind. Its vector now records `sources`, the `PageRef`s sent (8.1). `extract.json` also changed through the PDFium runtime (trailing spaces).

Files: `crates/epdf-engine/src/features/pages_write.rs`, `src/features/pages.rs`, `src/features/assembly.rs`, `src/features/extract.rs`, `src/features/changes/` (register the five page ops), `src/session/pages.rs`, `src/host.rs`, `crates/epdf-engine/tests/pages_mutate.rs`, `tests/named_pages.rs`, `tests/page_extract_corpus.rs`, `tests/page_extract.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs` (the kernel's page-edit calls now build `document.apply` requests).

Vectors: `pages/mutate.json`, `pages/extract.json`.

Upstream reference: `packages/engine/core/src/mutation/` (`PageDeleteInput.ts`, `PageDeleteResult.ts`, `PageInsertBlankInput.ts`, `PageInsertResult.ts`, `PageReorderInput.ts`, `PageReorderResult.ts`, `PageRotateInput.ts`, `PageRotateResult.ts`, `PageRestoreResult.ts`, `PageNameInput.ts`, `PageNameResult.ts`, `ListPosition.ts`), `packages/engine/core/src/dto/NamedPage.ts`, `PageExtract.ts`, `packages/engine/services/src/features/pages/` (`PagesMutator.ts`, `PagesInserter.ts`, `PagesExtractor.ts`, `internal/`), `packages/engine/services/src/features/changes/internal/pageChanges.ts`. Tests to port: `packages/engine/core/test/mutation/listPosition.test.ts`, `packages/engine/main/test/page-delete.test.ts`, `page-insert.test.ts`, `page-insert-blank.test.ts`, `page-reorder.test.ts`, `page-rotate.test.ts`, `named-pages.test.ts`, `page-extract.test.ts`. Commits: 2445caba7, 119685ea4, b69c40377, 1233d1349 (neighbour positions; `insert-blank-past-the-end-refused` and `move-past-the-end-refused` are gone).

Kept divergences to re-check: an absent `destIndex` is omitted on the page event (`517d0123d`), and Swift's `movePages(to: .end)` computes the index in the bindings layer (`17e8ff483`). With `ListPosition`, `destIndex` may be gone; record in the report whether each still applies.

- [ ] The five page ops in the queue, each converting places as it runs, all or none.
- [ ] `pages.setName`/`pages.removeName` and `pages.extract` in upstream's shapes.
- [ ] `pages_mutate.rs` replays `request` for every case (flatten and export included, through 8.17's code) and compares `inserted`, `flattened` and `layout` in the new shapes. `page_extract_corpus.rs` reads `sources`.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED` if listed; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test pages_mutate --test named_pages --test page_extract_corpus --test page_extract --test wire_contract` passes. This owns `a_destination_past_the_end_says_which_end`, `a_page_mutation_leaves_the_layout_the_typescript_engine_left`, `a_refused_mutation_changes_nothing`, `a_refused_page_mutation_is_refused_for_the_same_reason`, `an_extract_reopens_as_the_document_the_typescript_engine_produced`, `rotation_survives_the_copy` and `the_callers_order_reaches_the_output`. A test whose cases upstream removed is deleted, with the reason in the commit message.
- [ ] Legacy path (8.2): delete the `Host::call_legacy` arms of `pages.delete`, `pages.insert`, `pages.insertBlank`, `pages.move` and `pages.rotate`. Move every caller to the new kind or op: the `translate.rs` arm, and the tests 8.2 repointed (`grep -rn call_legacy crates/epdf-engine/tests crates/epdf-viewer-host`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.19: Form read

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.4 and 8.5. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Forms" (`snapshot.json`, and the field record notes under `field.json`).

Field records changed:
- `fieldObjectNumber` folds into `ref.objectNumber` (595bdc6c7).
- `flags` flattens to `readOnly`/`required`/`noExport` (c3b90c3a4).
- `createdBy`, `createdAt`, `filledBy`, `filledByName`, `filledAt`, `importedBy` and `groupId` are new (b43963d12), with dates through `pdf_date_to_iso` (8.5).
- Widgets carry `ref`, `page` and `objectNumber` in place of `annotObjectNumber`/`pageObjectNumber` (595bdc6c7).

The snapshot gains a top-level `widgets` list (1233d1349), `topIndex` and `defaultValue` where they apply (bf7d4558e), and `calculationOrder[].objectNumber`. Places are page space (c053c3d33).

Files: `crates/epdf-engine/src/features/forms/snapshot.rs`, `forms/mod.rs`, `src/page_space.rs` (`pageFormSnapshotOf`, `pageFormFieldOf`, `pageWidgetOf`), `src/host.rs`, `crates/epdf-engine/tests/form_reader.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vector: `forms/snapshot.json`.

Upstream reference: `packages/engine/core/src/forms/` (`field.ts`, `snapshot.ts`, `widgets.ts`, `value-entry.ts`), `packages/engine/core/src/identity/FormFieldRef.ts`, `packages/engine/core/src/pageSpace/forms.ts`, `packages/engine/services/src/features/forms/FormReader.ts`, `internal/`. Tests to port: `packages/engine/core/test/forms/`, `packages/engine/main/test/forms.test.ts`, `field-groups.test.ts`, `form-attribution.test.ts`, `widget-finding.test.ts`. Commits: 595bdc6c7, c3b90c3a4, b43963d12, 1233d1349, bf7d4558e, c053c3d33.

Kept divergences to re-check: none recorded in this area.

- [ ] The field, widget and snapshot records in upstream's shape, converted at the boundary.
- [ ] `form_reader.rs` replays them.
- [ ] Remove `forms.list` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test form_reader --test wire_contract` passes (owns `the_snapshot_agrees_with_the_typescript_engine`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.20: Form values, reset, repair and apply

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.6 and 8.19. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Forms" (`effects.json`, `apply.json`, `write.json`).

Ruling (ledger, Task 4b, form-apply): upstream removed `forms.applyEffects` (98e5c270c). `forms/apply.json` drives the same 23 script-effect cases through `doc.apply` ops (`forms.setValue`, `forms.reset`, `forms.setDisplay`), all or none, with no per-effect status. A case that used to report a rejected effect is now a whole-change refusal. `a-push-button-cannot-be-reset` and `reset-with-no-fields-is-rejected` now apply as an item with no changed fields. `a-rejection-does-not-stop-the-batch`, `an-all-no-op-batch-produces-no-meta` and `a-no-op-in-the-middle-does-not-stop-the-batch` refuse whole (`InvalidArg`, `NotFound`, `NotFound`), and their `covers` text says so (our 016af6547).

`forms/write.json`: `documentCases` went from 10 to 2, because the FDF/XFDF cases went with form bundles (7f0b73a3c). The repair report says `needsAppearancesCleared` (7bccc4f35). `direct-field-refused` is `NotFound`, and `text-reset-no-default` reports no changed widget.

The runtime `1461b0c51` removed `EPDFForm_SetTextValue`, `SetToggle`, `SetChoiceValues`, `ResetField`, `ImportFDF` and `ImportXFDF` (runtime 69e229992). Task 6 bound the replacements: `Document::set_field_value(objnum, FieldValueWrite)`, `redraw_field_widgets`, `set_field_widgets_checked`, `set_field_selected_indices` and `set_field_default_value`. Upstream resets by writing `/DV` back through the value writers.

Files: `crates/epdf-engine/src/features/forms/write.rs`, `forms/effects.rs` (fold into the change ops or delete), `src/features/changes/` (register `forms.setValue`, `forms.setDisplay`, `forms.setAppearanceText`, `forms.reset`), `src/host.rs`, `crates/epdf-engine/tests/form_write.rs`, `tests/form_effects.rs` (replace with a new `tests/form_apply.rs`), `tests/document_events.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`, `src/native_engine.rs`, `tests/viewer_host.rs`, `tests/translate.rs`.

Vectors: `forms/apply.json`, `forms/write.json`.

Upstream reference: `packages/engine/core/src/forms/` (`write.ts`, `effects.ts`, `predict.ts`, `value-entry.ts`), `packages/engine/core/src/mutation/FormMutationResults.ts`, `Change.ts`, `packages/engine/services/src/features/forms/FormMutator.ts`, `internal/`, `packages/engine/services/src/features/changes/ChangeApplier.ts`, `internal/formChanges.ts` (`setValue`, `setDisplay`, `setAppearanceText`, `reset`), `packages/engine/core/src/events/` for the form change events. Tests to port: `packages/engine/main/test/forms.test.ts` (value and reset cases), `form-prediction.test.ts`, `form-scripts.test.ts`, `widget-appearance-refresh.test.ts`, `document-events.test.ts` (form events). Commits: 98e5c270c, 7f0b73a3c, 7bccc4f35, 2445caba7; runtime 69e229992.

Stubs removed:
- `crates/epdf-engine/src/features/forms/write.rs:226` (`set_value`) and `:251` (`reset`).
- `write.rs:297` (`read_back`, `#[allow(dead_code)]`).
- `write.rs:372` (`import_data`). FDF and XFDF are gone upstream, so delete it; bundle import is 8.22's.
- `crates/epdf-engine/src/features/forms/effects.rs:327` (the `preflight_inner` refusal) and `:501` (the unreachable `apply_native` arm).

This branch's form write answers (`06993dcf7`: the TS's meta, sequence and cascade) and its document events (`6cbcaa8a5`) were written against `forms.applyEffects`. Move them to upstream's change answers and events.

Some tests lose their cases upstream:
- `an_import_applies_what_the_typescript_engine_applied` and `an_unreadable_payload_is_refused` replayed the FDF/XFDF `documentCases`, and none is left. Delete them, with the reason in the commit message.
- `every_effects_batch_agrees_with_the_oracle` read the deleted `forms/effects.json`. A new `tests/form_apply.rs` replaces it and replays `forms/apply.json`.
- `an_effects_batch_publishes_form_effects_applied` in `document_events.rs` becomes a test of the event upstream publishes for a form change, renamed.

Kept divergences to re-check: the no-op `applyEffects` answer carrying `mutationSequence` (`87b61b533`, kept 2026-10-15) has no kind left to live on, since `forms.applyEffects` is gone. Record it in the report as obsolete.

- [ ] `set_value` and `reset` on the new runtime writers; `read_back` used again.
- [ ] The four form value ops in the queue, all or none. `forms.setValue` and `forms.reset` as kinds take `refs` and answer with `MutationMeta`.
- [ ] `forms.repair` reports `needsAppearancesCleared`.
- [ ] `tests/form_apply.rs` replays `forms/apply.json`; `tests/form_effects.rs` is deleted; `form_write.rs` replays `request`. `forms/apply.json` records `ops`, not a whole request: wrap each case's `ops` as `{kind: "document.apply", docId, change: {ops}}` with the envelope fields 8.2 defaults, send it through `Host::call`, and compare `result` or `refusedWith`.
- [ ] `epdf-viewer-host`: the kernel's form value and reset calls map onto the new kinds and events.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test form_write --test form_apply --test document_events --test wire_contract` and `CARGO_BUILD_JOBS=6 cargo test -p epdf-viewer-host --test translate` pass, and so do the three `viewer_host` tests it owns: `CARGO_BUILD_JOBS=6 cargo test -p epdf-viewer-host --test viewer_host -- a_checkbox_is_checked_by_its_on_state a_reset_restores_the_default_through_the_native_engine a_text_field_fills_through_the_native_engine`. (`an_fdf_export_imports_back` is 8.22's.) This owns:
  - in `form_write`: `a_written_field_reads_back_as_the_typescript_engines_did`, `a_form_model_is_a_snapshot_and_a_write_makes_it_stale`, `a_repair_reports_what_the_typescript_engine_reported`, `a_refused_write_is_refused_for_the_same_reason`, and the two deleted import tests;
  - `every_effects_batch_agrees_with_the_oracle`, through its replacement;
  - in `document_events`: `an_effects_batch_publishes_form_effects_applied` and `every_form_write_carries_meta`;
  - in `viewer_host`: `a_checkbox_is_checked_by_its_on_state`, `a_reset_restores_the_default_through_the_native_engine` and `a_text_field_fills_through_the_native_engine`.
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/forms/write.rs crates/epdf-engine/src/features/forms/effects.rs` prints nothing.
- [ ] Legacy path (8.2): delete the `Host::call_legacy` arms of `forms.applyEffects`. Move every caller to the new kind or op: the `translate.rs` arm, and the tests 8.2 repointed (`grep -rn call_legacy crates/epdf-engine/tests crates/epdf-viewer-host`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.21: Form fields and widgets

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.15 and 8.20. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Forms" (`field.json`).

`forms/field.json`: 25 cases became 23. `attach-already-attached-refused` and `attach-off-state-refused` went with `attachWidget` (d7bd2d277). `delete-merged-field-refused` now succeeds: a merged field and its widget delete in one change (60602f579). New kinds and ops: `forms.addWidget` (2445caba7), `forms.updateWidget`, `forms.deleteWidget` and `forms.reorderWidgets` (1233d1349), `forms.reorderCalculations` (98718b780). There are also op-only types: `forms.create`, `forms.update`, `forms.delete`, `forms.removeWidget`.

Files: `crates/epdf-engine/src/features/forms/field.rs`, `forms/snapshot.rs` (widget placement), `src/features/changes/` (register the field and widget ops), `src/page_space.rs` (`pdfFormFieldDraftOf`, `pdfWidgetPlacementOf`), `src/host.rs`, `crates/pdfium/src/form_write.rs` (only if a binding is missing), `crates/epdf-engine/tests/form_field.rs`, `tests/not_ported_refusals.rs` (delete), `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vector: `forms/field.json`.

Upstream reference: `packages/engine/core/src/forms/field.ts`, `widgets.ts`, `packages/engine/core/src/pageSpace/forms.ts`, `packages/engine/core/src/mutation/FormMutationResults.ts`, `packages/engine/services/src/features/forms/FormMutator.ts`, `internal/`, `packages/engine/services/src/features/changes/internal/formChanges.ts` (`createField`, `updateField`, `deleteField`, `addWidget`, `removeWidget`, `deleteWidget`, `updateWidget`, `reorderWidgets`, `reorderCalculations`, and their reverse steps). Tests to port: `packages/engine/main/test/forms.test.ts` (the field and widget cases), `widget-rotation.test.ts`, `widget-painter-parity.test.ts`. Commits: d7bd2d277, 60602f579, 2445caba7, 1233d1349, 98718b780; runtime 69e229992 (`EPDFForm_SetFieldDefaultValue`).

Stubs removed: `crates/epdf-engine/src/features/forms/field.rs:275` (a create with a default value), `:364` (an update touching `/DV`) and `:676` (helper `default_value_not_ported`). Write `/DV` through `Document::set_field_default_value(objnum, FieldValueWrite)`. A Clear is `FieldValueWrite::None`, which shadows an inherited `/DV` with an empty value, as upstream's does. Then delete `crates/epdf-engine/tests/not_ported_refusals.rs`: its last two tests (`a_field_create_with_a_default_value_is_refused_before_the_field_exists`, `a_field_update_touching_the_default_value_is_refused_before_the_rename`) test the stubs. 8.14 and 8.15 removed the other two; if either is still there, stop and report.

This branch's widget-link refs (`e3d45fc3a`) and field-delete cascade count (`a2b884473`) were written against the pre-catch-up TS. Check each against upstream, and keep or change it to match.

Kept divergences to re-check: none recorded in this area.

- [ ] `/DV` written; the three stubs and `not_ported_refusals.rs` gone.
- [ ] Merged-field delete in one change; the `attachWidget` code deleted.
- [ ] The nine kinds and the field and widget ops, all or none, with places from page space.
- [ ] `form_field.rs` replays `request`, and the two removed attach cases are gone from it.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test form_field --test wire_contract` passes. This owns `a_widget_lands_where_the_placement_put_it`, `a_draft_refusal_says_what_was_wrong_with_the_draft`, `a_structural_write_leaves_what_the_typescript_engine_left`, `a_radio_without_a_state_is_refused_before_the_field_exists` and `a_refused_structural_write_is_refused_for_the_same_reason`.
- [ ] `git grep -n "re-port (Task 8)" -- crates/epdf-engine/src/features/forms/field.rs` prints nothing, and `crates/epdf-engine/tests/not_ported_refusals.rs` is gone.
- [ ] Legacy path (8.2): delete the `Host::call_legacy` arms of `forms.attachWidget`. Move every caller to the new kind or op: the `translate.rs` arm, and the tests 8.2 repointed (`grep -rn call_legacy crates/epdf-engine/tests crates/epdf-viewer-host`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.22: Annotation and form transfer

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.14 and 8.21. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Form bundles replace FDF/XFDF (7f0b73a3c), and annotations gained export and import (83f6598d2, 843deedb4, 73f05157f). The kinds are `annotations.export`, `annotations.import`, `forms.export` (takes a `selection`), `forms.import` (takes a `bundle`) and `forms.importValues`. The imports are also change ops (`annotations.import`, `forms.import`, `forms.importValues`). No engine vector covers them beyond `wire/requests.json`, so unit tests ported from upstream's pin their behaviour.

Files: a new `crates/epdf-engine/src/features/transfer/` (the bundle codec, limits, planning), `src/features/annotation/` (export, import), `src/features/forms/` (export, import), `src/features/changes/` (register the three import ops), `src/page_space.rs` (the import arms), `src/host.rs`, new `crates/epdf-engine/tests/annotation_transfer.rs` and `tests/form_transfer.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`, `tests/viewer_host.rs`.

Vectors: none beyond `wire/requests.json`.

Upstream reference: `packages/engine/core/src/transfer/` (`AnnotationBundle.ts`, `annotationImport.ts`, `AnnotationTransfer.ts`, `bundle.ts`, `bundleLimits.ts`, `codec.ts`, `exportSelection.ts`, `FormBundle.ts`, `formExport.ts`, `formImport.ts`, `FormTransfer.ts`, `pageRefs.ts`, `planAnnotationImport.ts`, `planFormImport.ts`), `packages/engine/services/src/features/transfer/` (`bundlePages.ts`, `BundleResources.ts`, `checkWireBundle.ts`), `packages/engine/services/src/features/annotations/AnnotationExporter.ts`, `AnnotationImporter.ts`, `packages/engine/services/src/features/forms/FormExporter.ts`, `FormImporter.ts`. Tests to port: `packages/engine/core/test/transfer/` (all six), `packages/engine/core/test/wire/annotationsExportToken.test.ts`, `packages/engine/main/test/annotation-export.test.ts`, `annotation-import.test.ts`, `annotation-transfer.test.ts`, `annotations-stamp-transfer.test.ts`, `form-transfer.test.ts`. Commits: 7f0b73a3c, 83f6598d2, 843deedb4, 73f05157f.

`an_fdf_export_imports_back` in `crates/epdf-viewer-host/tests/viewer_host.rs` round-trips FDF, which is gone. Rewrite it as a form bundle round trip through the kernel's export and import calls, and rename it.

Kept divergences to re-check: none recorded in this area.

- [ ] The bundle codec and limits, with upstream's `core/test/transfer` cases as Rust unit tests.
- [ ] The five kinds and the three import ops, with places converted at the boundary.
- [ ] Remove this task's kinds from `NOT_YET_ROUTED`; set their `operations.ts` rows to `routed` and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test annotation_transfer --test form_transfer --test wire_contract` and `CARGO_BUILD_JOBS=6 cargo test -p epdf-viewer-host --test viewer_host` pass (owns `an_fdf_export_imports_back`, through its rewrite).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.23: Redaction

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.6 (`MutationMeta`) and 8.12. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. Replay each case's recorded `request` through `Host` with `common::replay` (8.2); don't rebuild it from the case fields. The per-file notes are in `docs/research/catchup-engine-vector-diffs.md`, "Redaction".

10 cases became 9: `batched-index-refs-refused` went with index refs (bc0057f3a). Each case carries its `request` with the v3 scope, `{pages}` or `{annotations}` of refs (a9434c0e2). Results name pages by a `page` ref. `remaining` loses the two widgets (1233d1349), so its order shifts.

Files: `crates/epdf-engine/src/features/redaction.rs`, `src/host.rs`, `crates/epdf-engine/tests/redaction_apply.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vector: `redaction/apply.json`.

Upstream reference: `packages/engine/services/src/features/redaction/RedactionApplier.ts`, `packages/engine/core/src/mutation/RedactionApplyResult.ts`. Tests to port: `packages/engine/main/test/redaction-apply.test.ts`. Commits: a9434c0e2, bc0057f3a, 1233d1349; runtime 532ecf80e (`EPDFPage_ApplyRedactions`, bound in Task 6 as `apply_redaction_marks`).

Kept divergences to re-check: a no-op redaction answer carries `mutationSequence` (`87b61b533`, kept 2026-10-15). Keep it, and record it in the report.

- [ ] The v3 scope, page refs in results, and widgets out of `remaining`.
- [ ] `redaction_apply.rs` replays `request`.
- [ ] Remove `redaction.apply` from `NOT_YET_ROUTED` if listed; set its `operations.ts` row and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test redaction_apply --test wire_contract` passes (owns `an_applied_redaction_removes_what_the_typescript_engine_removed`, `a_refused_apply_is_refused_for_the_same_reason`, `a_refused_apply_redacts_nothing`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.24: Outline

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.4 and 8.6. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

`outline.read` is new (67ccb76dc). The outline is written through the `document.apply` ops `outline.create`, `outline.update`, `outline.move` and `outline.delete`. Task 13 ports `plugin/outline`, which needs them. Destinations are page space (8.4). No engine vector covers the outline beyond `wire/requests.json`.

Files: a new `crates/epdf-engine/src/features/outline.rs` (or `features/outline/`), `src/features/changes/` (register the four outline ops), `src/page_space.rs` (`pageOutlineOf`, `pageOutlineItemOf`, `pdfOutlineDraftOf`, `pdfOutlinePatchOf`), `src/host.rs`, `crates/pdfium/src/` (an outline wrapper over `epdf_outline.h`, which Task 6 bound in `pdfium-sys`), a new `crates/epdf-engine/tests/outline.rs`, `tests/wire_contract.rs`.

Vectors: none beyond `wire/requests.json`.

Upstream reference: `packages/engine/core/src/dto/DocumentOutline.ts`, `DocumentOutline.schema.ts`, `packages/engine/core/src/identity/OutlineItemRef.ts`, `packages/engine/core/src/mutation/OutlineMutationResults.ts`, `packages/engine/core/src/outline/predict.ts`, `packages/engine/core/src/pageSpace/outline.ts`, `packages/engine/services/src/features/outline/` (`OutlineReader.ts`, `OutlineMutator.ts`, `internal/`), `packages/engine/services/src/features/changes/internal/outlineChanges.ts`. Tests to port: `packages/engine/main/test/outline.test.ts`. Commit: 67ccb76dc.

Kept divergences to re-check: none recorded in this area.

- [ ] The outline wrapper in `crates/pdfium`, with tests in `crates/pdfium/tests/`.
- [ ] `outline.read` and the four ops, all or none, with places converted at the boundary.
- [ ] Remove `outline.read` from `NOT_YET_ROUTED`; set its `operations.ts` row to `routed` and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test outline --test wire_contract` and `CARGO_BUILD_JOBS=6 cargo test -p pdfium` pass. This task owns none of the tests failing at `253bca66b`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.25: Measure

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.4, 8.6 (`MutationMeta`) and 8.12. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

`measure.viewports` and `measure.setScale` were ported here against the pre-catch-up TS (`635f7d303`, `d8027c034`). Upstream moved measurement to page space (c053c3d33: `pageViewportsOf`, `pdfMeasureOf`), and annotation records carry `measure` (8.12). Adapt the kinds' requests and answers to upstream's.

Files: `crates/epdf-engine/src/features/measure.rs`, `crates/pdfium/src/measure.rs`, `crates/epdf-engine/src/page_space.rs` (`pageViewportsOf`, `pdfMeasureOf`), `crates/epdf-engine/src/host.rs`, `crates/epdf-engine/tests/measure.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`.

Vectors: none beyond `wire/requests.json` and the `measure` fields of the annotation vectors (8.12 to 8.15).

Upstream reference: `packages/engine/services/src/features/measure/` (`MeasureReader.ts`, `MeasureMutator.ts`, `internal/`), `packages/engine/core/src/dto/Measure.ts`, `Measure.schema.ts`, `packages/engine/core/src/pageSpace/measure.ts`, `packages/engine/core/src/measure/`. Tests to port: `packages/engine/core/test/measure.test.ts`, `packages/engine/main/test/measurements.test.ts`, `measurement-authoring.test.ts`, `measurement-conformance.test.ts`, `measurement-shape-authoring.test.ts`. Commits: c053c3d33, and `git log 2516e2786..b456ee4fd -- packages/engine/services/src/features/measure` for the rest.

Kept divergence: `d8027c034` refuses a scale write with no measure instead of clearing. Keep it as is: the user decides it at Task 14 (controller ruling).

- [ ] Both kinds in upstream's shapes, with page-space viewports.
- [ ] Remove nothing from `NOT_YET_ROUTED` (both were already routed); set their `operations.ts` rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test measure --test wire_contract` and `CARGO_BUILD_JOBS=6 cargo test -p pdfium` pass, with the ported upstream cases. This task owns none of the tests failing at `253bca66b`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.26: Signatures

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.1, 8.3 and 8.21. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push. The section 8.1 added to `docs/research/catchup-engine-vector-diffs.md` says why `signature/protection.json` changed.

This branch's signature engine (`da3297cf3`, `4c3fa2ecb`, `8480e5c91`, `138d6e38e`, `5efaf80c3`) follows the pre-catch-up TS. Upstream renamed `signatures.abort` to `signatures.cancel` (7bccc4f35), and its signature requests and answers moved with the rest of the wire (`PageRef`, `effect`, `opId`, `authority`). The kinds are `signatures.list`, `signatures.analyze`, `signatures.contents`, `signatures.digest`, `signatures.revisionBytes`, `signatures.prepare`, `signatures.complete`, `signatures.cancel`, `signatures.finalizeCandidate` and `forms.setSignatureAppearance` (also a change op). The open kinds' `signedDocumentPolicy`, which 8.3 parses and stores, is enforced here.

Files: `crates/epdf-engine/src/features/signature/` (all of it), `src/features/forms/` (`forms.setSignatureAppearance`), `src/features/changes/` (register `forms.setSignatureAppearance`), `src/session/`, `src/host.rs`, `crates/epdf-engine/tests/signature_analyze.rs`, `signature_finalize.rs`, `signature_protection.rs`, `signature_read.rs`, `signature_sign.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/src/translate.rs`, `src/native_engine.rs`.

Vectors: `signature/protection.json` (as 8.1 regenerated it), `wire/requests.json`.

Upstream reference: `packages/engine/services/src/features/signature/` (`SignatureReader.ts`, `SignatureAnalyzer.ts`, `SignatureMutator.ts`, `CandidateFinalizer.ts`, `internal/`), `packages/engine/core/src/signature/`, and `worker-protocol.ts` for each kind. Tests to port where Rust has none: `packages/engine/main/test/signatures.test.ts`, `signature-file-candidate.test.ts`, `signature-finalize-candidate.test.ts`, `signature-server-memory.test.ts`, `signature-visual-fill.test.ts`, `packages/engine/core/test/signature-protection.test.ts`, `packages/engine/core/test/signature/`. Commits: 7bccc4f35, and `git log 2516e2786..b456ee4fd -- packages/engine/services/src/features/signature packages/engine/core/src/signature` for the rest.

Kept divergences to re-check (kept by the user; keep each unless upstream now does the same, and record which): `signatures.finalizeCandidate` opens a plain file document, not a layer (`dd2e1c290`); signing notices travel in an envelope-level `event` (`517d0123d`, in `epdf-viewer-host`).

- [ ] `signatures.cancel` in place of `signatures.abort`; every kind in upstream's request and answer shape.
- [ ] `signedDocumentPolicy` enforced as upstream's WorkerHost does.
- [ ] `forms.setSignatureAppearance` as a kind and as a change op.
- [ ] `signature_protection.rs` replays the regenerated vector.
- [ ] Remove `signatures.cancel` from `NOT_YET_ROUTED`; set the rows and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test signature_analyze --test signature_finalize --test signature_protection --test signature_read --test signature_sign --test wire_contract` passes, and `pnpm --filter @embedpdf/tooling-engine-conformance run signature-protection:check` is current. This task owns none of the tests failing at `253bca66b`; if 8.1's regeneration made `signature_protection` fail, this task owns that failure.
- [ ] Legacy path (8.2): delete the `Host::call_legacy` arms of `signatures.abort`. Move every caller to the new kind or op: the `translate.rs` arm, and the tests 8.2 repointed (`grep -rn call_legacy crates/epdf-engine/tests crates/epdf-viewer-host`).
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.29: Layer-backed fat-memory open

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.3. Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Upstream opens `open.fatMem` bytes as a fresh layer over a private memory base (`WorkerHost.openBytesAsLayer`, `packages/engine/services/src/worker-host/WorkerHost.ts:953-967`): the base is acquired under `bytes-open:<uuid>`, the layer opens `{ kind: 'fresh' }` through `openLayerDocument` (`packages/engine/services/src/document-session/lifecycle/PdfDocumentOpener.ts:115`), and `session.persistLayerArtifact = false`, so the session's kind is `'layer'` but a write answers no artifact (`WorkerHost.ts:2758`). This engine opens a plain `FPDF_DOCUMENT` (`DocumentSession::open_with`, `crates/epdf-engine/src/session/document.rs:213`), and `EPDFLayer_GetLastObjectNumber` answers 0 on one. 8.3 (`61fb035b3`) therefore left four things working on layer sessions only: `objectNumbers.reserve` and the open's `reserveObjectNumbers` answer `NotImplemented` on a fat-memory session (`DocumentSession::reserve_object_numbers`, `document.rs:790`), the open answer has no `lastObjectNumber` (`DocumentSession::last_object_number`, `document.rs:756`, read by `Host::open_answer`, `crates/epdf-engine/src/host.rs:2100`), and `objectNumberFloor` does nothing (`DocumentSession::raise_last_object_number`, `document.rs:769`, called from `host.rs:529`). `objectNumbers.reserve` sits in `wire_contract.rs`'s `STUBBED` list for that reason (controller ruling after 8.3); this task removes the row.

The crash. 8.3 tried the upstream open: `open_with` built `BaseDocument::from_bytes`, opened `open_layer()`, split it with `into_parts()` and parked the base with `retain_until_close`, keeping `kind: OpenKind::FatMemory`. Every test in `cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast` kept its result except `viewer_host`, whose binary died with SIGSEGV. Run serially (`CARGO_BUILD_JOBS=6 cargo test -p epdf-viewer-host --test viewer_host -- --test-threads=1`), it dies in `a_plain_pdf_imports_one_asset_per_page` (`crates/epdf-viewer-host/tests/viewer_host.rs:1591`), the stamp plugin's library import (`crates/plugins/stamp/src/write/libraries.rs`, through `with_stamp()` at `viewer_host.rs:1489`). `lldb --batch -o run -o "thread backtrace all" -- <viewer_host test binary> a_plain_pdf_imports_one_asset_per_page --exact --test-threads=1` stops on thread `epdf-owner` with `EXC_BAD_ACCESS (code=1, address=0x18)` in `libembedpdf.dylib`'s `CPDF_PageImageCache::DetachCurBitmap() + 16`, and the backtrace shows only that frame. A second attempt leaked the base (`std::mem::forget`) instead of releasing it at close, and the crash stayed, so the base's lifetime is not the cause. Not yet ruled out: a render of a page of a layer document whose page was loaded through `EPDFDoc_LoadPageByObjectNumberNormalized` (`PagePtrPool::acquire`); a page shared between two layer sessions over one base; `EPDFPage_ResetRenderCache`, which upstream calls when a page's last job releases it (`packages/engine/services/src/document-session/pages/PageResidency.ts:184-200`) and Rust never does; the stamp import rendering the pages of a document it opened itself (`open` with `bytes`, `crates/epdf-viewer-host/src/native_engine.rs:212`). Build `libembedpdf` with symbols or rerun under `MallocScribble` / ASan to get the rest of the stack before guessing.

Files: `crates/epdf-engine/src/session/document.rs` (`open_with`, `unlock_with`, `install`, a `persist_layer_artifact` flag), `src/session/registry.rs` (`open_memory`), `src/session/save.rs` (`finish_mutation` returns no artifact when the session does not persist one), `src/session/pages.rs`, `src/features/save.rs`, `src/features/signature/reader.rs:341` and `analyzer.rs:149` (they branch on `OpenKind::Layer`), `src/host.rs`, `crates/pdfium/src/` (whatever the crash fix needs), `crates/epdf-engine/tests/host_sessions.rs`, `tests/document_session.rs`, `tests/session_registry.rs`, `tests/wire_contract.rs`, `crates/epdf-viewer-host/tests/viewer_host.rs` (reading only, unless the fix belongs there).

Vectors: none. Every vector replay must keep its result; a fat-memory session that becomes a layer changes what a save writes only if `save_to_buffer` differs on a layer, which the full suite shows.

Upstream reference: `WorkerHost.ts:849-943` (`handleOpen`), `:953-967` (`openBytesAsLayer`), `:2732-2775` (`finishMutation`, `saveLayerArtifact`), `PdfDocumentOpener.ts:78-140`, `DocumentSession.ts:126` (`persistLayerArtifact`), `:173-186` (`openFromHandle`), `:218-260` (`install`), `:514-575` (object numbers), `packages/engine/services/src/features/signature/SignatureMutator.ts:470-488` (an installed signing keeps `persistLayerArtifact` false outside a layer), `features/save/DocumentSaver.ts:40-125` (a save of an unchanged layer returns the loaded bytes). Tests to port: `packages/engine/main/test/object-numbers.test.ts` through `packages/engine/core/src/conformance/runObjectNumberConformance.ts` (the cases that need only the session: "the pool hands out numbers, each once"; the create cases stay with 8.14, 8.18 and 8.21, which by then can run them on a fat-memory session). Commits: b32fdf584, 630b77eab.

- [ ] Reproduce the crash with the open changed as above, find its cause, and fix it in the layer that owns it (`pdfium`, the page pool, or the engine). Record the cause and the evidence in the report.
- [ ] `open.fatMem` opens bytes as a fresh layer over a private memory base, as `openBytesAsLayer` does, with the session's kind `Layer` and no artifact persisted per write. A locked file still parks and unlocks (`unlock_with` loads the same way), and `install` keeps what `SignatureMutator.ts:470-488` keeps.
- [ ] `objectNumbers.reserve` and `reserveObjectNumbers` hand out numbers on a fat-memory session, the open answer carries `lastObjectNumber`, and `objectNumberFloor` raises it. Replace `host_sessions.rs`'s `a_fat_memory_session_has_no_numbers_to_hand_out` and `document_session.rs`'s `a_layer_session_reserves_numbers_and_a_plain_one_has_none` with tests of the new behaviour.
- [ ] Remove the `objectNumbers.reserve` row from `STUBBED` in `wire_contract.rs`; update its `operations.ts` note and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test wire_contract --test host_sessions --test document_session --test session_registry --test mutation_finalization` and `CARGO_BUILD_JOBS=6 cargo test -p epdf-viewer-host --test viewer_host -- --test-threads=1` pass, the second with no SIGSEGV. This task owns none of the tests failing at `253bca66b`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not, and no test binary exits on a signal.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.30: Session parity

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`), or the branch off its tip the controller names. Needs 8.6 (the write path and `write-transactions.test.ts`) and 8.29 (every session is then a layer, so transactions and residency have one document kind to serve). Rust matches upstream's TypeScript at `b456ee4fd`. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. `crates/epdf-viewer-host/src/translate.rs` maps the kernel's engine calls onto this host's wire: when you change a kind's request or answer, change its arm there and in `crates/epdf-viewer-host/tests/translate.rs`, so the kernel side keeps working until Task 13 ports it. No `packages/**`, `tooling/**` or `.github/**` changes except `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` (this task's rows). Commit with explicit paths; no `git add -A`, no `git stash`, no push.

8.3 (`61fb035b3`) left four session behaviours Rust-only, none with an owner until this task (controller ruling after 8.3).

Page residency. Rust's `PagePtrPool` (`crates/epdf-engine/src/session/pages.rs`) keeps no page past its last holder, which is upstream's pool without a residency (`packages/engine/services/src/document-session/pages/PagePtrPool.ts:19-31`, `budgetBytes: 0`). 8.3 records each view's working set on the pool (`PagePtrPool::set_working_set`, `pages.rs:181`) and ranks pages by it (`close_order`, `pages.rs:217`), but with nothing kept the order applies to no page. Upstream keeps parsed pages within a byte budget across every document on the runtime (`PageResidency.ts`, whole file; `DEFAULT_PAGE_RESIDENCY_POLICY` 384 MB and 30 s idle, `:60-63`), measures each page with `EPDFPage_GetParsedSize`, loads renders in slices, closes a page whose content changed (`EPDFPage_IsContentCurrent`, `:289-298`), empties a released page's image cache (`EPDFPage_ResetRenderCache`, `:184-200`), and closes the pages a content write may change around it (`WorkerHost.ts:818-845`, `pageEffectOf` `:2814`). The worker owns one residency (`WorkerHost.ts:352`, `:385`, set on each session at `:872`).

Close answers. Upstream answers `close` and `layer.close` with the tag alone (`WorkerHost.ts:2326-2351`). Rust answers `{closedSessions}` and `{closed}` (`host.rs:2184`, `:2192`), which `crates/epdf-engine/tests/host_sessions.rs:123` and `:214`, `crates/epdf-wasm/tests/host_bridge.rs:105`, `crates/epdf-uniffi/tests/host_surface.rs:69` and `crates/epdf-viewer-host/tests/translate.rs:307` assert.

Locked-open protection. A locked open answers `protection: null` in Rust (`Host::protection_of` reads no session for a parked one); upstream's locked answer has no `protection` field at all (`WorkerHost.ts:922-927`). Match upstream when the parked opens are reworked below.

Layer transactions. Upstream runs every write as one layer transaction (`WorkerHost.ts:538-553`, `transactedSession` `:808`, `writesDocument` `:2897`): `DocumentSession.beginTransaction` (`DocumentSession.ts:428`), `commitTransaction` (`:458-489`), which refuses a document past `OBJECT_NUMBER_CEILING` with `LayerFull` and `details.lastObjectNumber`, and `abortTransaction` (`:491-515`), which leaves the document as it was and returns the numbers a create claimed to the held pool. A write's answer carries the layer artifact with the layer's `lastObjectNumber` (`LayerArtifactWorkerPayload`, `packages/engine/core/src/wire/worker-protocol.ts:1459-1464`; `WorkerHost.saveLayerArtifact`, `:2765-2775`). Rust has no transaction: `finish_mutation` (`crates/epdf-engine/src/session/save.rs:68`) saves the artifact as payload 0 and the JSON names no artifact, and 8.3's floor raise runs before the arm (`host.rs:529`), where upstream's runs after `assertNoPendingSigning`. If 8.6 built transactions for `document.apply`, extend them to every write kind; otherwise build them here, binding `EPDFLayer_BeginTransaction`, `EPDFLayer_CommitTransaction`, `EPDFLayer_AbortTransaction` and `EPDFLayer_IsInTransaction` in `crates/pdfium/src/base.rs` beside 8.3's `layer_last_object_number`.

Parked opens. Upstream parks a session whose open failed on a password, wrong or missing, for every open kind (`WorkerHost.ts:913-930`, `isPasswordOpenError` `:2943`), and answers `passwordRejected: true` when a wrong password was given; `document.checkPasswordPermissions` then loads it (`DocumentSession.parkLocked`, `DocumentSession.ts:266`). Rust parks only a fat-memory open with no password (`SessionRegistry::open_memory`, `crates/epdf-engine/src/session/registry.rs`) and refuses a wrong password with `DocPasswordIncorrect`; a layer open on a locked base fails. The kernel depends on the refusal: `crates/epdf-viewer-host/tests/viewer_host.rs:79` (`a_failed_open_lists_its_tab_as_failed_with_the_reason`) expects a failed tab, and `native_engine.rs` reads a locked open from `security` alone. This task changes the Rust host and engine only. Keep the kernel's behaviour by having `translate.rs` or `native_engine.rs` turn a `passwordRejected` open back into the `DocPasswordIncorrect` failure it expects, marked as temporary; moving the kernel to upstream's locked-tab prompt is Task 13's, and this task's report says so.

Files: `crates/epdf-engine/src/session/` (`pages.rs`, `document.rs`, `registry.rs`, `save.rs`, `base_registry.rs`), `src/host.rs`, `src/mutation.rs`, `crates/pdfium/src/` (the transaction and parsed-size bindings), `crates/epdf-engine/tests/host_sessions.rs`, `tests/document_session.rs`, `tests/session_registry.rs`, `tests/mutation_finalization.rs`, `tests/wire_contract.rs`, a new `tests/page_residency.rs`, a new `tests/write_transactions.rs` unless 8.6 made one, `crates/epdf-wasm/tests/host_bridge.rs`, `crates/epdf-uniffi/tests/host_surface.rs`, `crates/epdf-viewer-host/src/translate.rs`, `src/native_engine.rs`, `tests/translate.rs`.

Vectors: none.

Upstream reference: `PageResidency.ts:1-474`, `PagePtrPool.ts:1-85`, `packages/engine/core/src/scheduling/place.ts:15-51`, `WorkerHost.ts:411-443`, `:472-500` (`takeHeld`, `pageIsKept`), `:538-553`, `:788-801`, `:808-845`, `:849-943`, `:2326-2366`, `:2732-2775`, `:2943-2950`, `DocumentSession.ts:266-300`, `:418-515`, `worker-protocol.ts:1598-1614`, `:1913`. Tests to port: `packages/engine/main/test/page-residency.test.ts`, `page-residency-host.test.ts`, `page-retention.test.ts`, `write-transactions.test.ts` (the cases 8.6 did not), `opening.test.ts` (the locked and wrong-password cases for `bytes`, `layerBytes` and `layerFile`). Commits: f550e9e5f (working sets), b32fdf584 (`LayerFull`, artifact `lastObjectNumber`).

- [ ] A page residency behind the pool, as `PageResidency` keeps pages: a byte budget, parsed sizes, the working-set close order 8.3 recorded, content-current checks, the render cache emptied on release, and the pages a content write may change closed around it. One residency per `Host`, shared by its sessions. Port `page-residency.test.ts` against the Rust pool and the `page-residency-host.test.ts` and `page-retention.test.ts` cases a synchronous host can express; name the ones it cannot in the report.
- [ ] `close` and `layer.close` answer `null`, as the tag-only answers of the other kinds do on this wire. Update every test above that asserts `closedSessions` or `closed`, and `translate.rs`'s `close` arm.
- [ ] Every write kind runs in one layer transaction: committed before the artifact is saved, aborted on any refusal, the floor raised inside it after the pending-signing check. A commit past `OBJECT_NUMBER_CEILING` is `LayerFull` with `details.lastObjectNumber`, and the document is as it was. A write's answer names its artifact as upstream's does, `lastObjectNumber` included.
- [ ] Wrong-password and locked-layer opens park and answer `passwordRejected` as `WorkerHost.handleOpen` does, and unlock through `document.checkPasswordPermissions`. The kernel keeps its current behaviour through a marked shim, left for Task 13.
- [ ] Set the `operations.ts` notes for `pages.workingSet`, `close` and `layer.close`, and re-render.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine --test host_sessions --test document_session --test session_registry --test mutation_finalization --test page_residency --test write_transactions --test wire_contract`, `CARGO_BUILD_JOBS=6 cargo test -p epdf-viewer-host --test translate --test viewer_host`, `CARGO_BUILD_JOBS=6 cargo test -p epdf-wasm --test host_bridge` and `CARGO_BUILD_JOBS=6 cargo test -p epdf-uniffi --test host_surface` pass. This task owns none of the tests failing at `253bca66b`.
- [ ] Record the baseline before you start: `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm --no-fail-fast 2>&1 | grep -E '^test .* FAILED'`. At the end the same command lists no test the baseline did not.
- [ ] `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p pdfium --all-targets` prints no warnings. `cargo fmt -p epdf-engine -p epdf-viewer-host -p pdfium`, then `git checkout --` every file it changed that you did not otherwise touch, and drop its hunks outside lines you wrote.
- [ ] Commit with explicit paths.

#### Task 8.27: Close the Rust side

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`). Needs 8.1 to 8.26, 8.29 and 8.30. Run cargo as `CARGO_BUILD_JOBS=6 cargo ...`. No `packages/**` or `.github/**` changes; `tooling/engine-conformance/src/operations.ts` and `OPERATIONS.md` only. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Task 9 (the Rust-core payload) runs after this task, and 8.28 after Task 9.

Kept divergences to re-check: walk the list in `docs/research/rust-plugins-branch-review.md`, "Status", and the engine ones the user kept on 2026-10-14 and 2026-10-15 (`dd2e1c290`, `3baec217b`, `7566552a2`, `517d0123d`, `87b61b533`, `818eba296`, plus `a4ed2b228`, `d8027c034`, `17e8ff483`). For each, state in the report whether it is kept as it was, obsolete, or now matches upstream, citing the sub-task that decided.

- [ ] `wire_contract.rs`: `NOT_YET_ROUTED` is empty; remove it. All 89 kinds route.
- [ ] `Host::call_legacy` has no arms left; delete it. `grep -rn call_legacy crates` prints nothing.
- [ ] The envelope fields 8.2 made optional for the transition are required where upstream's schema requires them; update the tests and `translate.rs` that omit them.
- [ ] `operations.ts`: every row is `ported` or `routed`; `pnpm --filter @embedpdf/tooling-engine-conformance run operations:check` passes.
- [ ] `git grep -n "re-port (Task 8)" -- crates` prints nothing.
- [ ] Every `:check` script in `tooling/engine-conformance/package.json` is current.
- [ ] Done when `CARGO_BUILD_JOBS=6 cargo test -p epdf-engine -p epdf-viewer-host -p epdf-wasm -p epdf-uniffi -p epdf-node -p epdf-ffi -p pdfium -p pdfium-sys -p test-support` passes with no failures, and `CARGO_BUILD_JOBS=6 cargo clippy -p epdf-engine -p epdf-viewer-host -p epdf-wasm -p epdf-uniffi -p epdf-node -p epdf-ffi -p pdfium -p test-support --all-targets` prints no warnings. Plugin crates (`crates/plugins/`, `epdf-kernel`, `plugin-conformance`) are Task 13's.
- [ ] Commit with explicit paths.


**Added by controller ruling after 8.2:** bring `crates/epdf-engine/src/error.rs` to upstream's error-code set (6 added, 2 removed since the merge base), and decide whether `jobId` keeps accepting strings.

#### Task 8.28: The Rust geometry parity test, after Task 9

Branch `research/catchup-plugins` (worktree `/Users/andrew/repos/embed-pdf-viewer/.claude/worktrees/catchup-plugins`). Needs Task 9: the test runs only against a wasm payload with the Rust core linked in, which Task 9 builds. Commit with explicit paths; no `git add -A`, no `git stash`, no push.

Ruling (ledger, Task 2): the typecheck gate was waived for `packages/engine/main/test/geometry-rust-parity.test.ts` only, by marking it `describe.skip` with a comment pointing at Task 8. Upstream removed `page.geometry.read()`, so the test is rewritten against upstream's text layout. This task changes that one `packages/**` file, because the ruling assigns it here.

Files: `packages/engine/main/test/geometry-rust-parity.test.ts`.

Upstream reference: `packages/engine/core/src/pageSpace/text.ts` (`createTextLayout`), `packages/engine/main/src/document/LocalPageTextService.ts` (`handle.text.layout()`, which sends `pages.geometry`), `packages/engine/services/src/features/geometry/RustPageGeometryReader.ts`.

Stub removed: `packages/engine/main/test/geometry-rust-parity.test.ts:82` (`// Waits for the engine re-port (Task 8)`).

Carried item owned here (Task 2): `hasRustCore` is unused until now. Remove the `describe.skip` and the `@ts-expect-error` together.

- [ ] Rewrite the test to compare both readers through `handle.text.layout()` (or the lowest upstream call that still sends `pages.geometry`), exactly as before, with no tolerance. Gate it with `describe.skipIf(!hasRustCore)` in place of `describe.skip`, and drop the `@ts-expect-error`.
- [ ] `pnpm --filter @embedpdf/engine typecheck` passes.
- [ ] `EMBEDPDF_WASM_URL=<Task 9's Rust-core payload> pnpm --filter @embedpdf/engine test -- geometry-rust-parity` runs (not skipped) and passes. On the stock payload it skips.
- [ ] `git grep -n "re-port (Task 8)" -- crates packages` prints nothing.
- [ ] Commit with explicit paths.

### Task 9: Payload with the Rust core

- [ ] Once Task 3's fork build is done: from `.claude/worktrees/catchup-plugins` (after the 2026-10-21 reorder the Rust re-port lives there), `PERF_WORK_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work node scripts/publish-engine-payload.mjs --dry-run`, `verify-rust-core.mjs` passes, and the 33 engine `:check` scripts are current.

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
