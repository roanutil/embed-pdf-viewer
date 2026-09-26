# Plugin layer port status

Where `multiplatform-plugins-plan.md` got to. Every claim points at a test you can run:

```sh
pnpm --filter @embedpdf/tooling-plugin-conformance run vectors:check    # the vectors still describe the TypeScript
pnpm --filter @embedpdf/tooling-plugin-conformance run coverage:check   # and reach what they reached
cargo test -p <crate>                                                   # the Rust replays them
```

`.github/workflows/plugin-rust.yml` runs all three. None of these crates link PDFium, so the replay
needs no fork artifacts.

Groups K0 through P5 are done: the kernel, every core library a plugin imports, and all 18 plugin
packages have a Rust crate that replays vectors generated from the shipping TypeScript. P6, the
adapter onto `crates/epdf-engine` and the uniffi exports, is not started.

## What is ported

| TypeScript | Crate | Vector cases | Statements reached | Branches reached | Mutants caught of viable |
|---|---|---|---|---|---|
| `core/main` | `epdf-kernel` | 83 | 1,672 / 1,674 | 411 / 418 | 282 of 355 on the first run; not rerun (below) |
| `core/geometry` | `epdf-geometry` | 609 | 801 / 801 | 94 / 94 | 438 of 447 |
| `core/stage` | `epdf-stage-core` | 300 | 919 / 919 | 196 / 196 | 529 of 534 |
| `core/ui` | `epdf-ui-core` | 1,299 | 1,207 / 1,207 | 336 / 337 | 282 of 284 |
| `core/annotation` | `epdf-annotation-core` | 7,022 | 6,628 / 6,637 | 2,030 / 2,048 | 3,460 of 3,528 |
| `core/acrojs` | `epdf-acrojs` | 92 | 797 / 797 | 117 / 118 | 177 of 185 |
| `plugin/metadata` | `epdf-plugin-metadata` | 7 | 159 / 159 | 20 / 24 | 13 of 14 |
| `plugin/i18n` | `epdf-plugin-i18n` | 626 | 424 / 424 | 121 / 122 | 280 of 284 |
| `plugin/shell` | `epdf-plugin-shell` | 29 | 215 / 215 | 57 / 57 | 60 of 62 |
| `plugin/page-edit` | `epdf-plugin-page-edit` | 12 | 231 / 231 | 37 / 40 | 45 of 45 |
| `plugin/view-manager` | `epdf-plugin-view-manager` | 69 | 405 / 405 | 102 / 102 | 97 of 99 |
| `plugin/actions` | `epdf-plugin-actions` | 150 | 2,364 / 2,376 | 641 / 670 | 274 of 283 |
| `plugin/commands` | `epdf-plugin-commands` | 71 | 425 / 425 | 131 / 131 | 111 of 113 |
| `plugin/render` | `epdf-plugin-render` | 1,653 | 2,003 / 2,013 | 421 / 438 | 506 of 521 |
| `plugin/interaction` | `epdf-plugin-interaction` | 38 | 582 / 583 | 105 / 106 | 83 of 90 |
| `plugin/selection` | `epdf-plugin-selection` | 333 | 1,324 / 1,325 | 260 / 277 | 684 of 722 |
| `plugin/stage` | `epdf-plugin-stage` | 182 | 2,984 / 2,990 | 723 / 751 | 863 of 895 |
| `plugin/search` | `epdf-plugin-search` | 67 | 631 / 634 | 138 / 147 | 103 of 106 |
| `plugin/annotation` | `epdf-plugin-annotation` | 724 | 6,261 / 6,275 | 1,432 / 1,489 | 1,018 of 1,065 |
| `plugin/redaction` | `epdf-plugin-redaction` | 45 | 404 / 404 | 107 / 110 | 91 of 92 |
| `plugin/link` | `epdf-plugin-link` | 23 | 352 / 352 | 81 / 84 | 62 of 63 |
| `plugin/form` | `epdf-plugin-form` | 191 | 2,746 / 2,762 | 756 / 799 | 454 of 466 |
| `plugin/stamp` | `epdf-plugin-stamp` | 74 | 1,014 / 1,014 | 325 / 327 | 131 of 133 |

Timeouts count as caught: each is a mutant that loops forever. The two annotation crates' counts
include the misses that new cases killed after the run (30 in the core, 118 in the plugin); each
of those was confirmed by applying the mutant by hand, not by a second full run. Each crate's `README.md` lists every
unreached line with its reason and every surviving mutant with its argument; almost all of both are
guards the public surface cannot trigger (a document-scoped plugin's "no document" arm, a reducer's
`default` for an action outside its union) or mutants that build an equal value.

The whole corpus is 94 vector files and 13,699 cases. The generators reach 34,548 of the 34,622
statements and 8,642 of the 8,886 branches in the ported TypeScript (`vectors/coverage.json`).

## How it is checked

The generators drive the real TypeScript and write what it did. A function case is a call and its
result. A scenario is a kernel, its plugins and a fake engine driven through ordered steps, with a
trace entry after each step: the call's outcome, the promises that settled, every dispatched action,
every engine call, every callback, every reported error, and the state tree.
`tooling/plugin-conformance/README.md` is the format.

A Rust crate passes when it reproduces every trace entry. That is stricter than it sounds: the
trace records the order things happen in, so a port that did the right things one microtask turn
early fails. The kernel's first replay failed 13 of 43 cases that way, and `rt::returned` in
`crates/epdf-kernel/src/rt.rs` is the rule that fixed them. Later ports found two more turn rules
(adopting an `AbortablePromise`, and `finally` adopting its callback's result), recorded in
`crates/plugins/annotation/src/promise.rs`.

Coverage is the completeness gate: `coverage:check` fails when a line a vector reached stops being
reached, or a new one is reached and nobody recorded it. Mutation testing is the gate on the Rust
side.

## Host inputs

Where the TypeScript reads an ambient value, the Rust takes it from the host:

- Script execution. `ScriptSandbox` (`crates/plugins/acrojs`) is a trait; there is no default.
  A host without one behaves as the TypeScript does with scripting disabled. The vectors use a
  scripted sandbox that answers from a queue, and the sandbox's own JavaScript prelude ships as
  data in `crates/plugins/acrojs/prelude.js`.
- Animation frames. The stage plugin's `Scheduler` (`raf`, `caf`) and the render plugin's
  `RenderHost`. Without one, stage runs frameless and render waits 16 ms on the kernel clock, as the
  TypeScript does under Node.
- The clock and time zone. `rt::now()` and an injected UTC offset. The generators run under
  `@sinonjs/fake-timers` 15.4.0 with `TZ=UTC`, and the Rust clock follows that library's rules.
- Console output. Each plugin that logs (`actions`, `annotation`, `redaction`, `i18n`) takes a
  sink; the vectors record what reaches it.
- `crypto.randomUUID()`, in the annotation plugin's `AnnotationHost`.

## Known gaps

These are open, each with where it is recorded:

- `epdf-kernel` was mutation-tested once (282 of 355 viable caught, 73 missed). The misses that were real got
  vector cases and `crates/epdf-kernel/tests/runtime.rs`; the rest were the typed plugin API, which
  the plugin crates now exercise. The kernel has not been rerun with the plugin crates' tests.
- `epdf-plugin-selection`: seven sign mutants in `frame_for_run` survive, and a rotated fixture near
  the handedness boundary would kill them (`crates/plugins/selection/README.md`).
- `epdf-plugin-search`: its tick counts mirror the TypeScript but no vector can see them, since only
  one chain resumes per step (`crates/plugins/search/README.md`).
- `epdf-plugin-annotation`: `downloadFile` missing from a page service has no vector. The form port
  added a fake-engine `lacks` list that could now describe it.
- `epdf-plugin-selection` carries a second copy of engine-core's text layout, because
  `crates/epdf-engine` links PDFium and the plugin crates must not. Both are pinned, separately.
- `epdf-plugin-i18n` carries `Intl.PluralRules` as data generated from Node 24.20.0 (ICU 78.3) and
  CLDR 48. The generating script was not committed, so a Node upgrade that changes the rules means
  writing it again. `vectors:check` flags the day that happens.
- Inputs outside the TypeScript types are not ported: fractional or `NaN` page indexes, unknown
  enum strings, `Object.prototype` member names as ids. Each crate's README lists its own.

## TypeScript behaviour the vectors pin

The TypeScript is the oracle, so these are recorded rather than fixed:

- `packages/plugin/annotation`: `armStamp` activates the built-in `stamp` tool, whose tag makes the
  tool-change callback disarm the payload immediately. And `rehydrate()` without read authority
  never re-checks after `/access`.
- `packages/plugin/render`: a render whose signal is already aborted still starts an engine fetch
  nobody owns, and a tile manager keeps running after its document closes.
- `packages/plugin/commands`: the doc comment on `run` in `types.ts` describes routing the code
  does not do.

## Running mutation tests

`scripts/mutants.sh` is the only way to run them. It queues every run on the machine first come,
first served, runs one mutant at a time with four compile jobs, at `nice 19`. Two unthrottled runs
from parallel worktrees crashed the development machine twice before it existed.

## Not started

P6: an `epdf-viewer-host` crate that puts the kernel and plugins on `crates/epdf-engine` through
its `host.rs` wire, and the uniffi exports for the native facades. Its gate is an integration test
over a fixture, not vectors, because the engine is pinned on its own.
