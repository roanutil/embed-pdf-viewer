# Plugin layer port status

Where `multiplatform-plugins-plan.md` got to. Every claim points at a test you can run:

```sh
pnpm --filter @embedpdf/tooling-plugin-conformance run vectors:check    # the vectors still describe the TypeScript
pnpm --filter @embedpdf/tooling-plugin-conformance run coverage:check   # and reach what they reached
cargo test -p <crate>                                                   # the Rust replays them
```

`.github/workflows/plugin-rust.yml` runs all three. None of these crates link PDFium, so the replay
needs no fork artifacts.

Groups K0 through P5 are done, against the v3 TypeScript that `research/multi-platform-rust-core-impl`
carries at `278019267`. The kernel, every core library a plugin imports, and all 19 plugin packages
have a Rust crate that replays vectors generated from that TypeScript. P6, the adapter onto
`crates/epdf-engine` and the uniffi exports, ships as `crates/epdf-viewer-host` (added in
`57617919c`); its gate is `crates/epdf-viewer-host/tests/viewer_host.rs`. How the port caught up with the v3
restructure is in `multiplatform-plugins-rebase.md`.

## What is ported

| TypeScript | Crate | Vector cases | Statements reached | Branches reached | Mutants caught of viable |
|---|---|---|---|---|---|
| `core/main` | `epdf-kernel` | 166 | 2,808 / 2,826 | 689 / 709 | 613 of 644 |
| `core/geometry` | `epdf-geometry` | 652 | 927 / 927 | 112 / 112 | 507 of 514 |
| `core/stage` | `epdf-stage-core` | 300 | 919 / 919 | 196 / 196 | 529 of 534 (pre-v3; source unchanged) |
| `core/ui` | `epdf-ui-core` | 1,299 | 1,207 / 1,207 | 336 / 337 | 282 of 284 (pre-v3; source unchanged) |
| `core/annotation` | `epdf-annotation-core` | 7,689 | 8,319 / 8,328 | 2,588 / 2,608 | 4,577 of 4,669 |
| `core/acrojs` | `epdf-acrojs` | 92 | 797 / 797 | 118 / 119 | 177 of 185 (pre-v3; source unchanged) |
| `core/signature` | `epdf-signature-core` | 456 | 1,264 / 1,314 | 303 / 309 | 1,781 of 1,824 |
| `plugin/metadata` | `epdf-plugin-metadata` | 14 | 233 / 234 | 39 / 41 | 35 of 37 |
| `plugin/i18n` | `epdf-plugin-i18n` | 635 | 618 / 625 | 153 / 154 | 298 of 303 |
| `plugin/shell` | `epdf-plugin-shell` | 38 | 341 / 341 | 103 / 103 | 77 of 79 |
| `plugin/page-edit` | `epdf-plugin-page-edit` | 19 | 294 / 294 | 84 / 87 | 58 of 66 |
| `plugin/view-manager` | `epdf-plugin-view-manager` | 76 | 530 / 530 | 155 / 155 | 137 of 140 |
| `plugin/actions` | `epdf-plugin-actions` | 159 | 3,184 / 3,195 | 759 / 785 | 307 of 314 |
| `plugin/commands` | `epdf-plugin-commands` | 76 | 575 / 575 | 164 / 164 | 107 of 108 |
| `plugin/render` | `epdf-plugin-render` | 1,450 | 2,283 / 2,293 | 469 / 483 | 527 of 543 |
| `plugin/interaction` | `epdf-plugin-interaction` | 46 | 589 / 590 | 131 / 132 | 87 of 90 |
| `plugin/selection` | `epdf-plugin-selection` | 367 | 1,446 / 1,447 | 287 / 296 | 699 of 716 |
| `plugin/stage` | `epdf-plugin-stage` | 194 | 3,536 / 3,542 | 774 / 804 | 874 of 899 |
| `plugin/search` | `epdf-plugin-search` | 77 | 678 / 678 | 160 / 163 | 89 of 90 |
| `plugin/annotation` | `epdf-plugin-annotation` | 956 | 9,238 / 9,254 | 2,186 / 2,280 | 1,412 of 1,449 |
| `plugin/redaction` | `epdf-plugin-redaction` | 50 | 731 / 738 | 167 / 172 | 67 of 70 |
| `plugin/link` | `epdf-plugin-link` | 29 | 460 / 461 | 128 / 132 | 95 of 96 |
| `plugin/form` | `epdf-plugin-form` | 219 | 3,433 / 3,460 | 882 / 931 | 521 of 530 |
| `plugin/stamp` | `epdf-plugin-stamp` | 100 | 2,286 / 2,305 | 635 / 653 | 239 of 251 |
| `plugin/measurement` | `epdf-plugin-measurement` | 61 | 1,126 / 1,135 | 248 / 260 | 245 of 248 |
| `plugin/signature` | `epdf-plugin-signature` | 58 | 1,375 / 1,375 | 306 / 310 | 148 of 150 |

Timeouts count as caught: each is a mutant that loops forever. A mutant counted as caught because a
case added after the run kills it was confirmed by applying it by hand and watching the replay fail,
not by a second full run; each README says how many there are. Every crate's `README.md` lists every
unreached line with its reason and every surviving mutant with its argument. Almost all of both are
guards the public surface cannot trigger (a document-scoped plugin's "no document" arm, a reducer's
`default` for an action outside its union) or mutants that build an equal value.

The whole corpus is 109 vector files and 15,278 cases. The generators reach 49,197 of the 49,390
statements and 12,172 of the 12,495 branches in the ported TypeScript (`vectors/coverage.json`).

## How it is checked

The generators drive the real TypeScript and write what it did. A function case is a call and its
result. A scenario is a kernel, its plugins and a fake engine driven through ordered steps, with a
trace entry after each step: the call's outcome, the promises that settled, every dispatched action,
every engine call, every callback, every reported error, and the state tree.
`tooling/plugin-conformance/README.md` is the format.

A Rust crate passes when it reproduces every trace entry. That is stricter than it sounds: the
trace records the order things happen in, so a port that did the right things one microtask turn
early fails. The kernel's first replay failed 13 of 43 cases that way, and `rt::returned` in
`crates/epdf-kernel/src/rt.rs` is the rule that fixed them. Later ports found more turn rules
(adopting an `AbortablePromise`, `finally` adopting its callback's result, the extra turns
`Promise::race` spends on an abortable input), recorded in `crates/plugins/annotation/src/promise.rs`
and `crates/epdf-kernel/README.md`.

Coverage is the completeness gate: `coverage:check` fails when a line a vector reached stops being
reached, or a new one is reached and nobody recorded it. Mutation testing is the gate on the Rust
side. It earned its keep in the v3 catch-up: triaging survivors found real port bugs in
`epdf-signature-core` (the RSA-PSS hash, a stack overflow on cross-issuing CAs, four DER and date
divergences, `signed` compared by value instead of truthiness), `epdf-plugin-stamp` (a dropped
`mark`), and number formatting in `epdf-plugin-page-edit` and `epdf-kernel`. Each is fixed and
pinned by a case.

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
- Console output. Each plugin that logs (`actions`, `annotation`, `redaction`, `link`, `i18n`,
  `measurement`, `stamp`) takes a sink; the vectors record what reaches it.
- `crypto.randomUUID()`, in the annotation plugin's `AnnotationHost`, which also carries the font
  list the TypeScript reads from the engine's fonts service.
- Cryptography. `epdf-signature-core` takes `WebCrypto` (key generation, signing, key export, random
  bytes), `TrustPort` and `PersonalKeyStore` as traits. The browser's IndexedDB key store does not
  port; a native host writes its own over its keychain.
- `Math.random`, in the stamp plugin's name generator. The vectors inject it; the default is a
  clock-seeded xorshift.

## Known gaps

These are open, each with where it is recorded:

- `epdf-plugin-search`: its tick counts mirror the TypeScript, and two close-race cases now pin the
  difference between the find-all and session chains, but not each count
  (`crates/plugins/search/README.md`).
- `epdf-plugin-selection` carries a second copy of engine-core's text layout, because
  `crates/epdf-engine` links PDFium and the plugin crates must not. Both are pinned, separately.
- `epdf-annotation-core` carries the engine-core measurement readout in `src/measure.rs` for the
  same reason. The coverage gate does not count engine-core lines, so 30 `/Measure` dictionaries in
  the vectors are all that check it.
- `epdf-plugin-i18n` carries `Intl.PluralRules` as data generated from Node 24.20.0 (ICU 78.3) and
  CLDR 48. The generating script was not committed, so a Node upgrade that changes the rules means
  writing it again. `vectors:check` flags the day that happens.
- `epdf-signature-core` departs from the TypeScript on purpose in four places, each listed under
  "Deliberate deviations" in its README. The one with a cost: two CA certificates that issue each
  other hang pkijs, and the Rust refuses the chain as a cycle instead, a verdict the TypeScript can
  never confirm.
- `epdf-stage-core`, `epdf-ui-core` and `epdf-acrojs` keep their pre-v3 mutation numbers. Their
  TypeScript source did not change, and neither did the crates.
- Inputs outside the TypeScript types are not ported: fractional or `NaN` page indexes, unknown
  enum strings, `Object.prototype` member names as ids. Each crate's README lists its own.

## TypeScript behaviour the vectors pin

The TypeScript is the oracle, so these are recorded rather than fixed. Each crate's README has the
case that pins it.

- `packages/core/main`: `documents.retry` reopens with the original options, so a rename made on the
  failed tab is lost. A superseded or cancelled lane run doesn't reject until its body finishes. A
  document controller listening to `onActiveChanged` hears the change its own close causes.
- `packages/plugin/view-manager`: `viewManagerPlugin()` cannot be installed in a kernel. Its
  controller spreads its context, which reads the `doc` getter, and that getter throws for a
  workspace plugin.
- The guarded handle: after close, a captured capability's verbs still reach the engine before they
  reject `instance-closed`. `page-edit`'s `duplicate` of an unknown page calls `extract` first.
- `packages/plugin/annotation`: `refresh()` without read authority never re-checks after `/access`.
- `packages/plugin/render`: a render whose signal is already aborted still starts an engine fetch,
  and `renderPages` sorts a page listed twice at its last index.
- `packages/plugin/link`: a rejected `ensureLoaded` stays in the in-flight map, so that page never
  retries.
- `packages/plugin/redaction`: the pending-marks memo keys on an array the annotation plane rebuilds
  on every call, so it never hits.
- `packages/plugin/measurement`: `setScale('all')` re-measures nothing; every page reports
  `Annotation hydration is incomplete`.
- `packages/plugin/form`: `importValues` with scripting on addresses fields by name, and never finds
  a field that has an object number.
- `packages/plugin/stamp`: `moveAsset` and `duplicateAsset` mint a new `/Name`.
- `packages/plugin/stage`: `applyViewState` doesn't clamp the cursor to the document it lands on.
- `packages/core/signature`, through pkijs: chain validation starts from the last certificate the
  CMS carries and never checks the signer itself, and verification ignores the signature
  algorithm's own hash (an MD5-with-RSA label over a SHA-256 digest verifies).

## Running mutation tests

`scripts/mutants.sh` is the only way to run them. It queues every run on the machine first come,
first served, runs one mutant at a time with four compile jobs, at `nice 19`. Two unthrottled runs
from parallel worktrees crashed the development machine twice before it existed. The v3 numbers
above came from running it file by file over every crate, from a worktree kept apart from the ones
agents were editing.

## P6

`crates/epdf-viewer-host` puts the kernel and plugins on `crates/epdf-engine` and exports them
through uniffi for the native facades. Its gate is an integration test over a fixture
(`crates/epdf-viewer-host/tests/viewer_host.rs`), not vectors, because the engine is pinned on its
own.
