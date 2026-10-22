# Plugin conformance vectors

The TypeScript kernel and plugins are the oracle. The generators here drive them and write what they
did to `vectors/`; `crates/plugin-conformance` replays the frozen files against the Rust port and fails on
the first difference. The generator emits schema 2; the checked-in vectors and Rust replay still use
schema 1 until the vector regeneration and Rust harness migration. `multiplatform-plugins-plan.md` is why; this file is the contract both halves
implement.

```sh
pnpm --filter @embedpdf/tooling-plugin-conformance run vectors          # write
pnpm --filter @embedpdf/tooling-plugin-conformance run vectors:check    # fail if stale, orphaned or unreplayed
pnpm --filter @embedpdf/tooling-plugin-conformance run coverage         # write vectors/coverage.json
pnpm --filter @embedpdf/tooling-plugin-conformance run coverage:check
pnpm --filter @embedpdf/tooling-plugin-conformance run test:harness    # runtime invariants
pnpm --filter @embedpdf/tooling-plugin-conformance run audit:kernel    # every kernel scenario and refusal
cargo test -p plugin-conformance                                        # replay the kernel
PLUGIN_CONFORMANCE_ONLY=open-all PLUGIN_CONFORMANCE_VERBOSE=1 cargo test -p plugin-conformance
```

## Files

Every module in `src/suites/` exports `suites`, and `src/generate.ts` discovers them, so a new
plugin's generator is one new file there. A suite writes one vector file:

```json
{ "schemaVersion": 2, "kind": "scenario" | "function", "describes": "...", "cases": [...] }
```

`vectors:check` also fails on a file under `vectors/` that no suite writes, which is what a deleted
or renamed suite leaves behind, and on a file no Rust test replays. It reads the second from the
source: some `.rs` file under `crates/` must hold the path as a string literal
(`"plugins/actions.json"`), as every `assert_suite` call does. A literal in a test that never runs
would still pass.

`vectors/environment.json` records the ICU and Unicode versions of the Node that wrote the vectors
(`process.versions.icu`). ICU decides `Intl.PluralRules` (i18n plurals) and `localeCompare` (render
tiles, sorting), so another Node can write other vectors from the same TypeScript. A full write
updates the file and says when the version changed. `vectors:check` compares it with the running
Node: when every file still matches it only warns, and when some file is stale it says the ICU
differs, so a stale plural is not mistaken for a TypeScript change.

A function case is `{name, fn, args, outcome}`: a pure function called with canonical arguments.
The Rust side looks `fn` up in the table its test passes to `assert_suite`. Use this family for
everything that has no lifecycle: geometry, reducers called directly, tile planning, layout. It is
cheaper to grow than scenarios and it is where most arithmetic should be pinned.

A scenario case is `{name, covers, plugins, engine, steps, snapshot?, reason?, allowUnmatched?, awaitCalls?, trace}`.

`"snapshot": "none"` records no state, so the case is compared only through its outcomes,
settlements, committed state changes, document events, engine calls and callbacks. Final state is not compared. Such a case
has to carry a `reason` saying what it compares instead; the TypeScript type and the generator both
refuse one without it. Suites share one reason across the cases it is true for. The Rust runner
ignores the field.

## Scenarios

`plugins` lists `{factory, config}`. A factory is a name a plugin registers with `registerPlugin`
on the TypeScript side and returns from `HarnessPlugin::factories` on the Rust side. The runner
always prepends a stateless spy plugin, `@@conformance/spy`, which subscribes to the documents
capability during `connect`. It records opened, openFailed, locked, closed, activeChanged,
pagesChanged and unsavedChangesChanged events. A stateless spy adds no state slice.

`steps` run in order. After each one the runner drains every queued microtask (one real
`setImmediate` in Node, `rt::run_until_stalled` in Rust) and records a trace entry.

`awaitCalls: true` makes the TypeScript runner wait, in real time, for each `call` step's returned
promise to settle before it drains. WebCrypto answers from libuv's thread pool, which no number of
microtask turns reaches, so without it a `crypto.subtle` result would land in whichever later step
happened to be running. The Rust runner ignores the field: a Rust step already runs until nothing
is left. The `core/signature-*` suites and `plugins/signature.json` set it, and a call that has not
settled after 5 real seconds fails the generator.

| op        | fields                        | does                                                                                                                                                                                      |
| --------- | ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `call`    | `target`, `method?`, `args?`  | calls a function; `target` is `"kernel"`, `"documents"`, `{token, doc?}` (resolved with `kernel.capability`) or `{ref}` (a function an earlier outcome returned, called without `method`) |
| `emit`    | `doc`, `event`                | the fake engine delivers a document event to that handle's subscribers                                                                                                                    |
| `settle`  | `label`, `value?` or `error?` | settles the oldest pending engine call or callback deferred under `label`                                                                                                                 |
| `advance` | `ms`                          | advances the fake clock, firing timers in order and draining after each                                                                                                                   |
| `abort`   | `promise`, `reason?`          | calls `abort` on a returned `AbortablePromise`                                                                                                                                            |
| `drain`   |                               | nothing; records an entry                                                                                                                                                                 |

Object methods can use a dotted path (`selection.list`). Resolve each member once
at invocation time and call with the final containing object as `this`. Flat calls
and the function-ref restriction remain unchanged.

`meter: n` on any step but `advance` counts microtask turns. Right after the step's synchronous
action the runner logs a `@@turn` callback with argument `i` for each of `n` turns, and every tracked
promise that settles while the meter runs also logs `@@settled` with its promise number, from the
`.then` reaction the runner attached to it. `settled` only orders promises within a step, so a
promise that settles four turns early still matches; where `@@settled` lands among the `@@turn`
entries does not. `kernel/turns.json` uses it to pin the turn each document transition settles on.

A trace entry has:

| field            | holds                                                                                                                             |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `outcome`        | `{returned}`, `{threw}` or `{promise: n}` for a `call` step                                                                       |
| `settled`        | `{promise, resolved}` or `{promise, rejected}` for each tracked promise that settled during the step, in settlement order         |
| `stateChanges`   | `{slice, previous, next}` from every instrumented plugin state cell's `onChange`, after commit; identity transitions emit nothing |
| `documentEvents` | `{event, value}` from the documents capability's public lifecycle event sources                                                   |
| `engine`         | the fake engine's call log for the step: `{doc, path, pon?, args}` and `{abort, reason}`                                          |
| `callbacks`      | `{fn, args}` for every call of a scripted callback or a probe hook                                                                |
| `reports`        | errors the kernel's `report` sink received                                                                                        |
| `status`         | `kernel.status()`                                                                                                                 |
| `engineState`    | per opened handle, `{subscribers}`: a leaked subscription shows here                                                              |
| `state`          | the whole `getState()` tree, or the slices `snapshot` names (`"@@core"` adds `core`), or nothing with `"snapshot": "none"`        |

If `createKernel` throws, the trace is a single entry with that outcome and status `unconstructed`.
Construction callbacks, state changes and reports are retained in the first trace entry. State-change
subscriptions use the real context and are owned by its cleanup scope; the runner never wraps a
context with a replacement dispatcher.

### Arguments

Arguments are canonical JSON, decoded before the call. `{"$token": name}` is a token.
`{"$ref": n}` is a function an earlier outcome returned. `{"$fn": name, ...}` is a scripted
callback: every call logs `{fn: name, args}` and answers with `throws`, `defer` (a promise a
`settle` step settles), `rejects`, `resolves` or `returns`, checked in that order; `seq` gives one
answer per call and repeats the last. An interaction handler, a listener, an open thunk: each is a
`$fn`, or an object of them.

Answers are not arguments. A `$fn`'s `returns` and `resolves`, a fake-engine response `value`, a
`settle` value, an `emit` event and a document's `security`, `pages` and `namedPages` reach the
Rust plugin as the JSON itself (`Callback::call`, `fake_engine.rs`), while the TypeScript decodes
them. The two agree on `$undefined`, `$number` and `$bytes`, which are how the Rust kernel holds
those values, and on a nested `$fn`, which the adapter that reads it turns into a function. They
would not agree on `$error`, `$map`, `$set`, `$token` or `$ref`, so the generator refuses a case
that puts one in an answer.

### Canonical JSON

`src/canonical.ts` defines it and `crates/plugin-conformance/src/canonical.rs` compares it.

- `undefined` is omitted as an object member and `{"$undefined":true}` anywhere else.
- `NaN` and the infinities are `{"$number": "NaN"}` and so on. `-0` is `0`.
- Bytes are `{"$bytes": base64}`, a `Map` is `{"$map": [[k, v], ...]}`, a `Set` is `{"$set": [...]}`.
- An error is `{"$error": {name, message, ...own enumerable members}}`, so `EngineError.code` and
  `PermissionDenied.required` are compared. Messages are compared exactly.
  Only an `Error` (from any realm) is written this way. A thrown or rejected value that is not
  one, `throw 'text'` or a rejection with a plain object, is written as its own canonical form, so
  it never compares equal to an `Error` with the same message.
- A function in an outcome is `{"$ref": n}`, numbered in the order the runner first sees it; one
  in state or in callback arguments is `{"$function": true}`. A promise is `{"$promise": n}`.
- Array order is compared, and so is object member order, except for a trace entry's own members
  (the runner's envelope) and the paths `crates/plugin-conformance/member-order-allow.json` names.
  `PLUGIN_CONFORMANCE_MEMBER_ORDER=0` turns the order check off. The allow-list holds the places a
  typed Rust value can't follow the TypeScript: where it passes a caller's object through (stage
  cameras, pointer samples, rich text and patch inputs), or where one type takes its order from
  whichever function built it (a page scale, a number format, an annotation's paint). Each entry
  is a vector file, a case and a path with its array indexes written `[]`; on 2026-10-13 it holds
  375 paths in 186 cases of 25 files. A new mismatch fails the run: fix the order, or, only if it
  is one of those two kinds, add the path. `pnpm run member-order:allow` adds every path the tree
  currently fails on, rerunning until none is left (`--fresh` rebuilds the list from empty).
- Numbers compare as `f64`. Two integers below 2^53 in magnitude compare exactly, so an epoch in
  milliseconds off by one fails. Any other pair compares within one part in 10^12, which absorbs a
  last-bit difference in a transcendental and nothing a coordinate could carry.

A Rust capability printed as an outcome lists its members in the order the TypeScript object
literal defines them (`HarnessPlugin::members`), because that order numbers the refs.
A member that is not a method prints through `HarnessPlugin::describe_member`, which the runner
calls in the same order, so any ref it registers numbers in place. The actions plugin's
`scriptRealm` (`{transaction, budget}`) is the one such member so far. The TypeScript needs no
counterpart: canonical JSON already prints a nested object.

## The fake engine

`src/fake-engine.ts` and `crates/plugin-conformance/src/fake_engine.rs` answer `engine-core`'s
`Engine` and `DocumentHandle` contract from `engine`:

```json
{
  "documents": {
    "a": {
      "id": "a",
      "local": true,
      "security": {
        "allows": ["doc.render"],
        "passwordPrompt": { "state": "none" }
      },
      "pages": [
        {
          "index": 0,
          "ref": { "kind": "objectNumber", "objectNumber": 1 },
          "...": "a PageLayout"
        }
      ],
      "responses": [{ "path": "metadata.get", "value": { "title": "x" } }]
    }
  },
  "responses": [
    {
      "path": "open",
      "args": [{ "kind": "bytes", "id": "a" }, {}],
      "defer": "open-a"
    }
  ]
}
```

Every asynchronous call is logged and answered by the first entry whose `path`, `pon` and `args`
match; `args` compares canonically, and an entry without it matches any arguments. An entry with
`times` is consumed that many times; one without is sticky. It answers with `value`, `error`, or
`defer: label`. A successful response may also carry `events`: they publish synchronously, in order,
before its promise resolves, including when a later `settle` completes a deferred response.
Publication stays bound to the originating open handle; a same-id reopen owns a distinct stream
and page registry, even when old responses settle or retained old methods run afterwards. An
error publishes none. Event listeners are isolated through the runner's report sink, and the
cursor is the highest observed `origin.serverId`.

Pages are addressed by `PageRef.objectNumber`. `page(ref)` is a synchronous address without data;
its service calls check the current page registry and reject `EngineError(NotFound)` for a missing
page. A layout event replaces that registry before listeners run. `pages.list` answers a current
`{pageCount, pages, namedPages}` snapshot. Each `PageLayout` uses page-space boxes, `pdfCropBox`,
`tabOrder` and the current ref spelling. Page services log `page.<service>.<method>` with `pon`.

All shared document and page services exist, including render policy, actions, attachments,
redaction, signatures, outline, page forms and page measure. Only the current contract's service
members exist: `metadata.get` and `metadata.custom.get` are methods, while `metadata.read`,
`forms.applyEffects`, `pages.move` and `page.geometry` are absent. The exhaustive service-member
tables are checked against the upstream interfaces. `forms.submit` is optional and requires
`services: ["forms.submit"]`.

`local: true` applies the local-engine brand to the document and pages and exposes fonts, pieceInfo,
version, downloadLayer, downloadToFile and raw page render/appearance methods. `services` can expose
individual local extensions for targeted fixtures; real local-engine checks require the brand.
`lacks` deliberately produces a malformed handle by hiding a named member; it does not make a
required upstream member optional.

`objectNumbers` seeds held numbers; `take()` removes one and `held` counts those remaining. A
successful `reserve(count)` fills the fake pool to that count with deterministic increasing numbers,
refusing nonpositive or noninteger counts. This is a table fake, without a background refill or
remote editing session; it does not reproduce local scheduling or object-number loss transports.
`with(facts)` returns the same table handle and `setWorkingSet` accepts hints without changing answers.

Fallbacks are `open` (document keyed by input id), `pages.list` (the registry), `render.getPolicy`
(`continuous`), `objectNumbers.reserve` (the deterministic fake pool), `close` and `destroy` (void).
Every other unanswered call rejects `FakeEngineUnmatched`, and generation refuses the trace unless
`allowUnmatched` is set. Named responses always take precedence over fallbacks.

An `open` that no entry answers and whose input `id` keys no document falls back to
`defaultDocument`, when the engine spec names one: `{"defaultDocument": "workshop", "documents":
{"workshop": {...}}}`. It is for a plugin that opens scratch documents under ids it mints (the
stamp asset engine), where the case cannot know the id in advance. Without it that open rejects
with `FakeEngineUnmatched`.

An error the engine rejects with, from an entry's `error` or from a `settle` step that settles an
engine call's `defer`, is the class a real engine throws. One named `PermissionDenied` is built as
`engine-core`'s `PermissionDenied` (`makeEngineError`; `engine_error` on the Rust side, which sets
`ErrorClass::PermissionDenied`), because the kernel's `toPluginError` maps refusals by class and
not by name. Every other error, and every error a `settle` step hands to a scripted callback or a
vector hands in as an argument, stays a plain `Error` with the name and members it spells.

Synchronous reads come from `security` and are not logged, because how often a selector runs is not
behaviour. `allows` takes a list or `"*"`. `allowsAnnotation` takes create/update/delete/set-group overrides; create defaults to
`allows('doc.annotate.create')`, update/delete to `allows('doc.annotate.modify')`, and set-group to
false. `allowsField` similarly takes fill/sign/set-group overrides; the defaults use
`doc.forms.fill`, `doc.sign` and false. `fieldRights` exposes those coarse defaults. These are
fixture decisions, not a second implementation of the engine's collaboration resolver. Security
reads are `state`, `scope`, `identity` and `passwordPrompt`. Successful `security.unlock` updates
`state` from its `DocumentUnlockResult.security` and derives `passwordPrompt` with the shipping
helper before settling. A `security.rawScope` fixture delegates capability, collaboration and
field authorization to the shipping scope resolvers with the current state's PDF bits; its
expanded `scope` and authorization getters therefore reflect an unlock. Explicit decision
fixtures (`allows`, `allowsAnnotation`, `allowsField`) remain available for targeted cases.

Every call returns an `AbortablePromise`. An abort is logged as `{abort: <index of the call>, reason}`
and rejects with `AbortError`. An answer that arrives after an abort is still mapped (an `open`
still makes its handle) and then ignored, as the TypeScript's `resolve(map(value))` does.

## The probe

`src/probe.ts` and `crates/plugin-conformance/src/probe.rs` are a plugin configured entirely by the
vector, for testing the kernel without any real plugin's behaviour. The config is documented on
`ProbeConfig`; every hook logs as `<id>.<hook>`.

Every probe uses the upstream `create(ctx) -> { api, connect? }` contract. `state` is cloned per
instance; `createThrows` tests construction failure. `connect` configures a synchronous connection:
state/active watches, a replacement state, staged-page reads, declared sibling `tryGet`, document
and capability listeners, and owned cleanup (which may throw, reject or defer). `sameAs` returns an
earlier probe definition; instrumentation preserves its object identity.

The API reaches `PluginContext` directly: `replaceState`, `mergeState` and `sameState` use named pure
transitions through `ctx.state.update`; there is no action adapter. `getPage(pon)` reads an object
number and `pageOf(refOrIndex)` resolves a ref or display index. Events, waitFor, serialQueue,
latest, acquire, permissions and guarded document calls use their public context members. The `API`
array in `src/suites/kernel.ts` defines capability function-reference order and must match `probe.ts`.

### Schema 2 and Rust replay migration

`SCHEMA_VERSION` is 2 for every envelope; function-case payloads are unchanged. Rust must reject
unknown versions and explicitly migrate its trace type, parser, comparison, probe member order,
fake handle contracts and scenario inputs. Schema 1 action records cannot be translated losslessly:
`stateChanges` reports committed state, and `documentEvents` reports public lifecycle events. There
is no record for a same-identity transition, and no fabricated core actions. State snapshots still
record request order, pending tabs, leases' results and committed pages. Construction logs now land
in the first trace entry. The checked-in vectors are intentionally not regenerated by this change.

The kernel scenarios change from 110 to 106: 97 names survive, nine cases are renamed/replaced, and
four are retired. The replacements preserve surviving behavior through the current API:

| Former case                            | Current case                             | Coverage                                                           |
| -------------------------------------- | ---------------------------------------- | ------------------------------------------------------------------ |
| close-during-init                      | close-during-policy                      | Close joins the real final async bring-up read, then cancels       |
| close-during-init-then-init-fails      | close-during-policy-then-policy-fails    | Late failure still cancels after close                             |
| close-joins-a-pending-init-on-its-turn | close-joins-a-pending-policy-on-its-turn | Open and close settlement timing at that await boundary            |
| effect-setup-failure-rolls-back        | connection-setup-failure-rolls-back      | Throwing synchronous connect rolls back                            |
| dispatch-and-watch                     | state-transitions-and-watch              | Replacement/merge/identity transitions and committed notifications |
| a-listener-dispatching-mid-pass        | a-listener-updating-state                | A state listener applies another transition                        |
| connect-dispatches-and-listens         | connect-updates-and-listens              | State write, staged pages and lifetime-owned listeners             |
| controller-geometry-and-pages          | controller-pages                         | Live getPage/pageOf/assertPageRef and scope resolution             |
| page-space-at-the-origin               | invalid-page-identities                  | Ref misses and valid zero-top pdfCropBox metadata                  |

Retired definitions: `destroy-during-start` deferred a workspace `init` before another plugin;
`destroy-during-the-last-init` deferred the final `init` before effects; `destroy-during-a-start-that-fails`
rejected that pending init after destroy; `a-plugin-with-no-reducer` dispatched SET into a definition
with `reduce: null`. Upstream commit `4fc1a32af` deletes init/effects/reducers/dispatch and makes
construction and connect synchronous (`packages/core/main/src/types.ts` PluginDef; `kernel.ts` start
and bringUp). Those hook boundaries cannot be deferred in a conforming probe. The surviving start
failure, destroy, rollback, state identity and pending engine-read cases remain active.

Only the coordinate-conversion portions of the two page cases are retired: upstream `c053c3d33`
deletes `ctx.geometry` and its PageSpace cache. Current getPage/pageOf preserve layouts, including
nonzero `pdfCropBox` and zero-top PDF crops; those reads are active. No local geometry bridge replaces
it. `audit:kernel` executes all 106 cases and checks every refusal's step, error name, code and message;
success cases must have none. `test:harness` checks lifecycle outcomes, cleanup, page reads and exact
callback turns. Other suite-input migrations and regenerated vectors follow separately.

## Instance ids

The kernel numbers every document instance `<id>#<n>` from one counter shared by every kernel in
the process, so a raw trace would depend on how many cases ran before it. Before each case the
runner reads the counter (it opens a document in a throwaway kernel) and afterwards rewrites every
`<id>#<n>` with `n` above that base to `n - base`. That is what a kernel whose counter starts at
zero writes, and the Rust kernel counts per kernel. The first reading also pushes the counter past
1,000, so a latest-lane run id (`<lane>#<n>`, counted per lane from one) is never mistaken for an
instance id. Slice lease ids of workspace plugins (`<plugin>#<n>`) are counted per store and are not
rewritten either.

## Module state

Two generator-side rules keep module-level state from leaking between suites and cases. Neither
has a Rust counterpart, because the Rust side builds a fresh kernel and fresh plugin state for
every case. `src/staleness.ts` imports every suite module before it emits any, as `generate.ts`
does, because a scenario suite may name a factory another suite's module registers. And a stamp
case loads its own copy of the stamp source tree: `src/loader/instance-hook.mjs` is a resolve hook
that carries a `?stamp-instance=N` query from the importing module down to every module under
`packages/plugin/stamp/src`, so the asset engine's module-level id counter starts over per case.

## Time and ordering

The generator runs under `@sinonjs/fake-timers` 15.4.0 with `now: 0`, faking `setTimeout`,
`setInterval`, `Date` and `performance`, and nothing that would change microtask order. The Rust
runtime's clock (`crates/epdf-kernel/src/rt.rs`) reproduces its rules: delays truncate to whole
milliseconds, a zero delay scheduled while the clock advances becomes one, and timers fire by
(deadline, insertion order).

Microtask order is the hazard every port meets first. Two chains that the TypeScript interleaves
must interleave the same way in Rust, and the rules that get them there are in `rt.rs`: a spawned
future's first poll is inline, awaiting any promise costs a turn, and `rt::returned(future)` adds the
turn a JavaScript caller spends resuming after an `async` function it awaited. The kernel's first
replay had 13 of 43 cases out of order until those rules were in place, and every one of them was a
missing turn.

The other hazard is reference identity. A pure session transition that returns `state` means
"no change": the state cell skips its notification. `watch` compares with `Object.is`, so a selector
returning a fresh object fires on every change, while one returning a retained member fires when
that member is replaced. Rust needs an explicit comparison with the same observable behavior.

## Adding a plugin

1. Add `src/suites/<id>.ts`: `registerPlugin('<id>', {factory, tokens})` and export `suites`.
2. Run `vectors`, then `coverage`, and read `vectors/coverage.json` for the plugin's files. Every
   uncovered line is either reached by a new case or explained in `multiplatform-plugins-status.md`.
   A file's `unsnapshottedOnly` ranges are lines that only `snapshot: "none"` cases reach. They
   count as covered, but no vector compares the state they leave behind, so a reducer listed there
   wants a case that snapshots its slice. The top-level `unloaded` list names ported files no
   generator imports at all, which c8 would otherwise leave out of the report; a file of the new
   plugin that shows up there is either type-only or untested.
3. In the plugin's crate, implement `HarnessPlugin` behind the `harness` feature and add a test
   that calls `assert_suite` for each of its vector files.

## Fixtures

`fixtures/signature/fixtures.json` holds the keys, certificates, CMS blobs and recorded signatures
the `core/signature-*` vectors are made from. Key generation, ECDSA and RSA-PSS signatures are
random, so `fixtures/signature/generate.ts` runs once and its output is committed; the suite only
reads it, and `vectors:check` stays deterministic. Re-running the script replaces every key and
changes every vector that embeds one. The Rust test host reads the same file for the keys its
scripted WebCrypto hands out.

Current text-layout and frame fixtures (Task12b2a):

- A `page.text.layout` response/settlement stores `{ "$textLayout": snapshot }`, where
  `snapshot` is a current page-space `PageGeometrySnapshot`: runs use `start`,
  glyphs use `loose`/`tight` with `space`/`empty`, and quads have semantic corner
  names. The fake reconstructs **shipping** `createTextLayout(snapshot)` in its
  existing result mapper, for both immediate and deferred success. Only this
  method interprets the marker. Rust Task13 must reconstruct its text-layout
  implementation from the same recorded snapshot; JSON never embeds functions.
- A scenario's `clock: { frames: true }` passes the current kernel HostClock:
  monotonic fake `performance.now()`, fake timer `after`, and 16 ms `nextFrame`.
  Without it the host is frameless. The shipping instance clock owns lifetime
  cancellation. The retired stage `config.scheduler` is never passed to a plugin.
- Pure cases record named shipping transitions and their positional arguments.
  Selection layout probes record page-space geometry, including converted points
  and grids. Geometry normalizer cases call engine `normalizePdfQuad` with four
  recorded PDF-space points; page-transform cases carry current page size/scale.
- Seven historical definitions are retired: four `pdfToContentMatrix` cases
  (`origin`, `offset`, `negative`, `extra-members-ignored`, removed in c053c3d33),
  selection `reduceSelection/page-loaded{,-again}` (4fc1a32af removes loaded-page
  session state; current page-mirror scenarios retain loading coverage), and
  search `unknown-action` (4fc1a32af removes action dispatch; no shipping unknown
  action transition). Exact historical definitions: `git show
6bba125f9:tooling/plugin-conformance/src/suites/{geometry,selection,search-reducer}.ts`.
  Other old geometry targets have current shipping replacements, including
  engine page/PDF conversions and quad normalization; they remain active.

Run `node --import=tsx --conditions=development --test test/current-suites.test.ts`
from this package to audit all 1,287 owned definitions and the text-layout/frame
seams. The explicit refusal table includes deliberately malformed grid inputs
and the independently reproduced shipping paged-empty camera refusal
(`stage/src/services/scene.ts:itemRect`); Task13 must preserve the observation
without an accidental Rust panic, and Task14 owns any upstream follow-up.

### Current live annotation fixture port

`annotation-live` registers the shipping annotation plugin under its real `annotation`
id/token. It is a distinct fixture factory from the legacy `annotation` factory while
unmigrated suites remain. Its helpers use the reviewed `annotation-current-fixtures`
records, page-space layouts, immutable object/base-index refs, explicit object-number
pools, flat `annotations.list` answers and real `apply` ChangeOps. Typed confirmation
builders construct complete result/event envelopes before execution. They never inspect
or reinterpret calls. Confirmed events publish before their call settles, and a local
`origin.tx.id` equals the actual queue request's `opId`; deferred dependent writes must
be confirmed in document order. No old annotation service method is restored.

This factory supplies deterministic host entropy only to the shipping UUID generator's
16-byte `Uint8Array` requests. At case start its counter is zero. Each request gets
zero bytes with a big-endian uint32 counter in bytes 12–15, starting at 1. Other crypto
requests delegate unchanged. The shipping UUIDv7 generator writes the fake clock's
48-bit millisecond time and UUID version/variant bits into those bytes. A hold gets its
ID when staged, before its timer sends it. One sequence serves every document in that
case; it resets for the next case. Case-end cleanup restores the exact prior own-property
descriptor (or deletes the temporary own property when the method was inherited), without
changing the prototype. The same cleanup applies to runner/construction errors and
refusals. Only `[annotation]` console errors are routed to the scenario callback log;
other errors still reach the host console and the independent live audit.

Task13 must reconstruct this explicit fixture port, the current nested capability nouns
and dotted method traversal with the final containing object as receiver. Resolve getters
at invocation time, once per segment; retain ordinary getter errors and the rule that
function refs accept no method. Keep the schema 2 trace envelope: this addition changes
method paths, records, session state, pending/confirmed sequencing and result payloads,
not the envelope version. `plugins/annotation-reconciliation.json` is a new suite target
for the later vector/replay task; no vector is written by this migration. All 115 original
live definitions map to 149 current cases: nine renames, no retirements and 34 additions.
The first 49 definitions and all 25+2 historical reconciliation mappings are in
`historical/task-12b2b1b-i/`; the remaining 66 definitions and whole-family accounting are
in `historical/task-12b2b1b-ii/`. Current comments/grouping/hover/markup/tools use direct
nouns and explicit typed answers; the hover fixture lends widgets through `lend`, never
through an annotation engine list. `born` uses the shipping drawing bounds for confirmed
creates. Named malformed BinarySource cases preserve exact errors plus valid recovery;
ordinary cases never accept accidental missing methods, stale refs or callback failures.

Run `node --import=tsx --conditions=development --test test/annotation-live.test.ts
test/annotation-live-ii.test.ts` from this package for the current live boundary. The tests
assert actual successful state, current ops/results/events, complete per-kind read schemas,
exact intended refusals/diagnostics and global cleanup. Legacy annotation helpers/UUID
shims remain solely for the eight B2/B3/C consumer files listed in `family.json`; those
owners migrate them, then global closure removes the bridge. Its three compiler diagnostics
remain deferred and its obsolete surface is never a current replay target.

The direct live tests additionally pin a narrow failure-callback reentry edge. A refused
create first emits its write-failed occurrence, then processes dependent refusals and
notifies the changed view. Ordinary disappearance/refusal synchronously forgets its
selection/draft. Re-entering move/up from the failure callback is safe, and its dependent
update is aborted. This special reentry can leave an internal selected ID for a missing
record; public selection/get/anchor/chrome/render reads are empty or disabled, and the
draft is null. Preserve that exact current ordering/observation in Task13; it is separate
from the ordinary H3 cleanup assertions and is not evidence of a public H3 crash.

### Current form core and shared UUID host input

`form-core.ts` calls the current shipping index, prediction, projection and selection
functions. Its 101 original cases map to 99 current pure cases and two actual-kernel
status cases in `form-core-lifecycle.ts`; a third lifecycle case independently checks
pending values, successful confirmation, rollback and field permission refusal without
an annotation plugin. Complete archived inputs, exact mappings, intentional malformed
inputs and reconstruction details are in [the form history](historical/task-12b2b2-i/README.md).

`form-current` is an isolated registration of the actual form plugin, with its real id,
token and settings/create/state/connect. It takes complete current records from
`form-current-fixtures.ts`, with explicit `FormSnapshot.widgets`, flat field properties
and current refs. Its typed static value-set answers carry a full ChangeResult,
FormMutationMeta and `forms.valueSet` event before settlement. It adds no old method
aliases or console wrapper. The original `form-fixtures.ts` remains for the 59 deferred
live form cases and the stamp/signature consumers; it is not the current replay API.

`case-entropy.ts` now owns the existing annotation fixture's UUID host input. Annotation
and form factories join one sequence per case, across both factory orders and every
document. The byte algorithm, delegation and exact descriptor/absence cleanup are
unchanged. Annotation keeps its scoped console owner; form installs no console hook.
Run `node --import=tsx --conditions=development --test test/form-current.test.ts` for
current form semantics, shared entropy, exact diagnostic expectations and archival
accounting. No vectors or Rust replay code are updated by these input migrations.

### Current form fill and design inputs

`form-fill.ts` and `form-design.ts` retain all 60 original names with current
shipping behavior. [The fill/design history](historical/task-12b2b2-ii/README.md)
links exact BASE source/inputs, the exhaustive before/after digest manifest,
removed-door replacements and the verified upstream no-annotation handler
refusal limitation. Current forms now comprise core102 + fill31 + design29;
only scripting46/actions13 remain for the later forms slice.

`form-live-fixtures.ts` builds complete typed static apply answers and current
page-space rows over the reviewed factory. Its distinct diagnostic host records
only `[form]` console errors, installed before annotation and restored after it;
that fixed order, current bundle/ref-map schemas and the form-owned widget lender
are Task13 replay seams. Shared entropy and the reviewed current factories are
unchanged. Run `node --import=tsx --conditions=development --test test/form-live.test.ts`
for independent operations/results/permission/event/schema/lifecycle assertions.
