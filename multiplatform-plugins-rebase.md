# Bringing the plugin port up to the current TypeScript

This branch, `research/multi-platform-rust-core-impl-plugins`, ports the plugin layer as it stood at
`dd99f4a7e`, its merge base with `research/multi-platform-rust-core-impl`. That branch has since
gained 101 commits, including a merge of `main` that restructures every plugin package. This file
is the plan for catching the port up, and the measurements it starts from.
`multiplatform-plugins-status.md` records the port itself.

## The answer so far

The merge is textually clean and semantically expensive. A probe merge on 2026-09-26 (branch tip
`9022e3d61` against `research/multi-platform-rust-core-impl` at `278019267`) conflicted in two
files only:

- `.gitignore`: keep both sides. This branch adds `mutants.out*/`; the other adds `tmp/` and the
  `tooling/bundler-matrix` outputs.
- `pnpm-lock.yaml`: take theirs, then `pnpm install` to add `@embedpdf/tooling-plugin-conformance`
  back.

What breaks is the oracle. The incoming changes touch 617 files under `packages/core` and
`packages/plugin` (43,658 insertions, 19,678 deletions), and every plugin changed shape: the
`reducer.ts` / `capability.ts` / `effects.ts` / `types.ts` split this port mirrors is gone, replaced
by `model.ts`, `controller.ts` and `token.ts` (`packages/plugin/metadata/src` is the smallest
example; `packages/plugin/stamp/src` also grew `read/`, `write/` and `services/`). Three packages are
new and have no port: `core/signature`, `plugin/signature` and `plugin/measurement`.

`packages/core/main/src` is unchanged, so the kernel port is not affected by the restructure. Its
lifecycle vectors still changed, through `engine-core`; which part is not yet known.

## Suite by suite

After the probe merge, `tsc` reported 148 errors across 26 generator suites, and `vectors:check`
could not start at all, because `generate.ts` imports every suite and the first missing module
stops it. Importing and checking each suite on its own gave:

| State | Count | Vector files |
|---|---|---|
| Unchanged | 26 | `kernel/utils`; `core/acrojs`, `core/acrojs-host`; all four `core/geometry-*`; all three `core/stage-*`; all six `core/ui-*`; `core/annotation-` `geometry-basics`, `geometry-hit`, `geometry-text`, `endings`, `cloudy`, `flags`, `groups`, `props`, `anchor`, `placement` |
| Stale (runs, different output) | 9 | `kernel/lifecycle`; `core/annotation-` `geometry-shapes`, `kinds`, `snap`, `hit`, `scene`, `update`; `plugins/stage-capability`, `plugins/stage-motion` |
| Runs and throws | 15 | the seven annotation scenarios (`no plugin factory "interaction"`), `stage-feeds` (`no plugin factory "actions"`), `metadata` (reads `kind` of `undefined`), `page-edit` (`nothing pending as "rotate"`), `render-capability` and `render-tile-plane` (`nothing pending as "r0"`), `search`, `search-stage`, `search-find-all` |
| Does not import | 26 suite files | `actions`, `annotation-core-view`, `annotation-pure`, `annotation-repository`, `commands`, the six `form-*`, `i18n`, `interaction`, `link`, `redaction`, `render-pure`, `search-reducer`, `selection`, `shell`, `stage-pure`, the five `stamp-*`, `view-manager` |

Most of the "throws" row is a consequence of the "does not import" row: a scenario that registers
the actions or interaction plugin fails once that suite's `registerPlugin` never ran. The
`nothing pending` failures are different: the plugin no longer made the engine call the case
deferred, which is a behaviour or call-order change.

## Order of work

1. Merge `research/multi-platform-rust-core-impl` into this branch, resolving the two files as
   above. Commit the merge before touching anything else, so every later diff is the port's.
2. Make the generators run. Rewrite each "does not import" suite against the new module layout,
   starting with `interaction.ts` and `actions.ts`: the annotation, stage and form scenarios
   register those plugins, so fixing them first turns most of the "throws" row into real results.
   Keep each suite's cases; change only how it reaches the code.
3. Run `vectors`, then read the diff of every vector file. An unchanged file means its Rust crate
   is still right. A changed file is a behaviour change to port, and the diff says what changed.
4. Re-port crate by crate in dependency order (kernel, core libraries, interaction and actions,
   then stage, selection, annotation, and the plugins on top), until `cargo test` replays every
   file. Where the TypeScript's new shape is a real API change (the controller and model split),
   move the Rust crate to match rather than adapting the vectors.
5. Port the three new packages with the same method, each as a new crate under `crates/plugins/`.
6. Regenerate `coverage.json`, re-run mutation testing on every crate whose source changed, and
   update each crate README and `multiplatform-plugins-status.md`.

## Re-running the probe

To measure again after the other branch moves: merge into a throwaway branch, `pnpm install`, then
run a script that imports each file in `tooling/plugin-conformance/src/suites/` inside a
`try`/`catch` and calls `emit([suite], true)` from `src/vectors.ts` for each suite, printing
whether it failed to import, threw, came out stale or came out unchanged. The one used for the
table above was not committed; it is about forty lines.

## Running agents on this work

Parallel `cargo-mutants` runs crashed the development machine twice. The rules that stopped it:

- Mutation tests only through `scripts/mutants.sh`, one `--file` at a time. It queues every run on
  the machine first come, first served, and runs one mutant at a time with four build jobs at
  `nice 19`.
- Every other cargo command as `CARGO_BUILD_JOBS=6 cargo ...`.
- At most two agents at once, each on its own branch from the integration tip, merged back one at
  a time with `vectors/coverage.json` regenerated after each merge.
