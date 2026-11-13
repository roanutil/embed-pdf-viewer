# CU-C6 decisions

Rust port of upstream `074a190c9..ed064e0c8` for the kernel, stage and the remaining plugins, on
`research/catchup-2026-11-12-c6`. Each entry: decision, reason, cost, how to reverse, commit. There is
no NEW DIVERGENCE and no change under `packages/**`.

## CU-C6-D1: the host worktree is the source, the container worktree a build mirror

Edits and commits happen in the host worktree; before each build, `cu-c6-sync.sh` (copied to
`/tmp/cu1112/logs/CU-C6/`) tars every file changed since `0879a6d22` into `/workspaces/wt/c6`, and
regenerated vectors are copied back with `docker cp` and checked by sha256. Reason: the brief puts
builds in the container and the result on the host; editing Rust through `docker exec` is error-prone.
Cost: the container branch `research/catchup-2026-11-12-c6` holds no commits of its own (its tree
matches the host commits plus the synced files), so the bundle round trip of the brief was not
needed. Reverse: commit in the container and bundle as the brief describes. Commit: none (process).

## CU-C6-D2: CU-C5's four kernel commits taken, my own ports of them dropped

On the manager's instruction I reverted my uncommitted ports of `a4805a492`,
`b3f1c447e` and `c7f8d75ff` and cherry-picked CU-C5's commits unchanged: `c00f1d345` as `ad34b9960`,
`bbd9bebf8` as `9aac78c70`, `51a07da98` as `b42c7b80e`, `ece09927b` as `7d9a633b5`. My dropped diff
is kept at `/tmp/cu1112/logs/CU-C6/own-kernel-reports-superseded.diff`. Reason: CU-C7 merges one copy.
Cost: the hashes differ from CU-C5's (new committer); the patches are identical, so the merge is
clean. Reverse: none needed.

## CU-C6-D3: the wheel cases go in the stage-turns suite

The three cases for `wheelBy`, `scrollChaining` and coasting (`13305a14b`, `138c54fe7`) are added to
`turnCases` (`vectors/plugins/stage-turns.json`), not a new file. Reason: they use the same streams,
200 ms pause and helpers as the page-turn cases, and the last one is a page-turn case. Cost: that
file grows by 11,541 lines. Reverse: move the three cases to their own suite file. Commit: `157724242`.

## CU-C6-D4: the commands context does not cache capabilities per token

Upstream's `contextByDocument` keeps the capabilities a context's `tryGet` finds until the store
changes. The Rust `context_by_document` memoizes the context on the store's change count but lets
`try_get` resolve each time. Reason: capabilities change only with a store change, so the answers are
the same, and no vector observes resolution counts. Cost: a little repeated lookup work. Reverse: add
a per-token cache to `CommandCtx`. Commit: `61b12266b`.

## CU-C6-D5: `labelParams` compared as JSON, kept as values

The answers memo compares `labelParams` by its `JSON.stringify` text (`params_json`, non-finite
numbers as `null`) as upstream does, but the label is translated from the kept `Params`, not from the
JSON parsed back. Reason: `Params` cannot hold `null`. Cost: a NaN parameter interpolates as `NaN`
where upstream would interpolate `null`. Reverse: parse the text back and map `null` to text.
Commit: `61b12266b`.

## CU-C6-D6: `omittedCommandsOf`'s `menuTarget` not ported

`7af98f4df` adds a `menuTarget` resolver to `omittedCommandsOf` in `core/ui/layout-compile.ts`. The
Rust `ui-core` ports only the small helpers of that module (`mode_command_of`, `edge_of`, `modes_of`), not
`omittedCommandsOf`, so there is nothing to change. Cost: none beyond the existing gap. Reverse: port
the function. Commit: none.

## CU-C6-D7: the stage uniffi surface grows (for CU-C7)

`StageSettings` and `StageSettingsPatch` records gain `scroll_chaining`, `StageSettingKey` gains
`ScrollChaining`, and the stage object gains `async wheel_by(dx, dy, time_ms) -> bool`. Reason: the
records mirror every setting; a host that pans by wheel needs the answer. Cost: the platform wrappers'
generated bindings change (CU-C7). No rename. Reverse: drop `wheel_by` from `ffi.rs`. Commit:
`861de9764`.

## CU-C6-D8: Rust-only tests follow the new semantics

Three `epdf-plugin-commands` behavior tests expected a derivation to run again on every read; at
`7af98f4df` it runs once per store change. They now call `kernel.store().notify()` before expecting a
new answer, and one expects two `visible` calls instead of four. The kernel's M35 test (page-mirror
cell copies) is replaced by one for the write count, and the doc-hidden `page_cell_value_copies` hook
is removed with the cell type it counted. Reason: the tests pinned removed behaviour. Cost: none.
Reverse: none. Commits: `61b12266b`, `25b8b9563`.

## CU-C6-D9: coverage gaps left, with reasons

At `f0f1c06be`'s `coverage.json` the stage gaps (`camera/wheel.ts`, `camera/coasting.ts`,
`navigation/turn.ts`) are closed. Left, with no case added:

| Lines | Upstream | Reason |
|---|---|---|
| `core/main/src/changes.ts` 483-487, 490-491 | `b3f1c447e` | refusals of a verb change in a group or with a taken `opId`; the verbs' callers are CU-C5's (form, annotation import) |
| `core/main/src/kernel.ts` 1386-1387 | `a4805a492` | a failed `pages.list` after a desync; the fake engine has no way to fail it |
| `core/main/src/page-mirror.ts` 243 | `9b5125408` | `loadedPages`, read by no ported plugin case |
| `core/main/src/testing.ts` 168, 320, 330, 338-339 | `8bc6546e5` | the TypeScript test context; it has no Rust port |
| `core/ui/src/layout-compile.ts` 407, 422 | `7af98f4df` | `omittedCommandsOf` has no Rust port (D6) |
| `plugin/commands/src/controller.ts` 352, `services/availability.ts` 96, 105, 107, 113, 148, 152, 194 | `7af98f4df` | translated not-allowed, locked and turned-off sentences with i18n present, and `lockedKeyOf` throwing; the Rust code exists, unpinned by a vector |

Cost: those paths are ported but unchecked against the reference. Reverse: add commands cases with the
i18n plugin and a scripted `locked` that throws. Owner: unassigned.

## CU-C6-D10: JavaScript number edges kept

`Coasting::broken_by` takes the slowest speed with `f64::max` (it is never NaN, as upstream's
`Math.max(slowest, SLOWEST_TOLD)`), and the wheel's size with a `Math.max` that answers NaN when
either side is, since Rust's `f64::max` drops a NaN. The store counts `destroy` once and each lease it
revokes once more, as `store.ts` does. Commit: `861de9764`, `25b8b9563`.
