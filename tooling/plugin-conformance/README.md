# Plugin conformance vectors

The TypeScript kernel and plugins are the oracle. The generators here drive them and write what they
did to `vectors/`; `crates/plugin-conformance` replays every file against the Rust port and fails on
the first difference. `multiplatform-plugins-plan.md` is why; this file is the contract both halves
implement.

```sh
pnpm --filter @embedpdf/tooling-plugin-conformance run vectors          # write
pnpm --filter @embedpdf/tooling-plugin-conformance run vectors:check    # fail if stale, orphaned or unreplayed
pnpm --filter @embedpdf/tooling-plugin-conformance run coverage         # write vectors/coverage.json
pnpm --filter @embedpdf/tooling-plugin-conformance run coverage:check
cargo test -p plugin-conformance                                        # replay the kernel
PLUGIN_CONFORMANCE_ONLY=open-all PLUGIN_CONFORMANCE_VERBOSE=1 cargo test -p plugin-conformance
```

## Files

Every module in `src/suites/` exports `suites`, and `src/generate.ts` discovers them, so a new
plugin's generator is one new file there. A suite writes one vector file:

```json
{ "schemaVersion": 1, "kind": "scenario" | "function", "describes": "...", "cases": [...] }
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
settlements, actions, engine calls and callbacks, and a reducer it drives goes unchecked. Such a case
has to carry a `reason` saying what it compares instead; the TypeScript type and the generator both
refuse one without it. Suites share one reason across the cases it is true for. The Rust runner
ignores the field.

## Scenarios

`plugins` lists `{factory, config}`. A factory is a name a plugin registers with `registerPlugin`
on the TypeScript side and returns from `HarnessPlugin::factories` on the Rust side. The runner
always prepends a spy plugin, `@@conformance/spy`, which logs the kernel's eight `@@core/*` actions;
its empty slice is in every snapshot.

`steps` run in order. After each one the runner drains every queued microtask (one real
`setImmediate` in Node, `rt::run_until_stalled` in Rust) and records a trace entry.

`awaitCalls: true` makes the TypeScript runner wait, in real time, for each `call` step's returned
promise to settle before it drains. WebCrypto answers from libuv's thread pool, which no number of
microtask turns reaches, so without it a `crypto.subtle` result would land in whichever later step
happened to be running. The Rust runner ignores the field: a Rust step already runs until nothing
is left. Only `core/signature-*` sets it, and a call that has not settled after 5 real seconds fails
the generator.

| op | fields | does |
|---|---|---|
| `call` | `target`, `method?`, `args?` | calls a function; `target` is `"kernel"`, `"documents"`, `{token, doc?}` (resolved with `kernel.capability`) or `{ref}` (a function an earlier outcome returned, called without `method`) |
| `emit` | `doc`, `event` | the fake engine delivers a document event to that handle's subscribers |
| `settle` | `label`, `value?` or `error?` | settles the oldest pending engine call or callback deferred under `label` |
| `advance` | `ms` | advances the fake clock, firing timers in order and draining after each |
| `abort` | `promise`, `reason?` | calls `abort` on a returned `AbortablePromise` |
| `drain` | | nothing; records an entry |

`meter: n` on any step but `advance` counts microtask turns. Right after the step's synchronous
action the runner logs a `@@turn` callback with argument `i` for each of `n` turns, and every tracked
promise that settles while the meter runs also logs `@@settled` with its promise number, from the
`.then` reaction the runner attached to it. `settled` only orders promises within a step, so a
promise that settles four turns early still matches; where `@@settled` lands among the `@@turn`
entries does not. `kernel/turns.json` uses it to pin the turn each document transition settles on.

A trace entry has:

| field | holds |
|---|---|
| `outcome` | `{returned}`, `{threw}` or `{promise: n}` for a `call` step |
| `settled` | `{promise, resolved}` or `{promise, rejected}` for each tracked promise that settled during the step, in settlement order |
| `actions` | `{slice, action}` for every plugin dispatch, logged before its reducer runs, and `{slice: "@@core", action}` for kernel lifecycle actions |
| `engine` | the fake engine's call log for the step: `{doc, path, pon?, args}` and `{abort, reason}` |
| `callbacks` | `{fn, args}` for every call of a scripted callback or a probe hook |
| `reports` | errors the kernel's `report` sink received |
| `status` | `kernel.status()` |
| `engineState` | per opened handle, `{subscribers}`: a leaked subscription shows here |
| `state` | the whole `getState()` tree, or the slices `snapshot` names (`"@@core"` adds `core`), or nothing with `"snapshot": "none"` |

If `createKernel` throws, the trace is a single entry with that outcome and status `unconstructed`.

### Arguments

Arguments are canonical JSON, decoded before the call. `{"$token": name}` is a token.
`{"$ref": n}` is a function an earlier outcome returned. `{"$fn": name, ...}` is a scripted
callback: every call logs `{fn: name, args}` and answers with `throws`, `defer` (a promise a
`settle` step settles), `rejects`, `resolves` or `returns`, checked in that order; `seq` gives one
answer per call and repeats the last. An interaction handler, a listener, an open thunk: each is a
`$fn`, or an object of them.

Answers are not arguments. A `$fn`'s `returns` and `resolves`, a fake-engine response `value`, a
`settle` value, an `emit` event and a document's `capabilities`, `security` and `pages` reach the
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
- Array order is compared. Object member order is compared only with
  `PLUGIN_CONFORMANCE_MEMBER_ORDER=1` set, and never for a trace entry's own members (the
  runner's envelope). It is opt-in because the Rust plugins do not match it yet: on the vectors
  of 2026-10-12, 896 cases in 59 vector files differ in member order alone, mostly sorted maps
  (state keyed by id, `model.defaults`) and struct field order.
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
      "services": ["render", "page.pieceInfo"],
      "security": { "allows": ["doc.render"], "passwordPrompt": { "state": "none" } },
      "pages": [ { "index": 0, "ref": { "kind": "objectNumber", "pageObjectNumber": 1 }, "...": "a PageLayout" } ],
      "responses": [ { "path": "metadata.read", "value": { "title": "x" } } ]
    }
  },
  "responses": [ { "path": "open", "args": [ { "kind": "bytes", "id": "a" }, {} ], "defer": "open-a" } ]
}
```

Every asynchronous call is logged and answered by the first entry whose `path`, `pon` and `args`
match; `args` compares canonically, and an entry without it matches any arguments. An entry with
`times` is consumed that many times; one without is sticky. It answers with `value`, `error`, or
`defer: label`. Pages are addressed as the engine addresses them, by `PageRef`: `page(ref)` finds
the page whose `ref.pageObjectNumber` matches, refuses any other with `EngineError(NotFound)`, and
hands out a page handle whose `ref` is that address. Page services are `page.<service>.<method>`,
logged with the page's object number as `pon`.
Optional members exist only when `services` lists them: `render`, `actions`, `attachments`,
`pieceInfo`, `redaction`, `signatures`, `fonts`, `version`, `downloadLayer`, `page.pieceInfo`,
`page.measure`, and `forms.submit` (the Rust side answers `DocumentHandle::has` from the same list). `forms.applyEffects`, `pages.flatten`,
`pages.extract` and `page.annotations.downloadFile` are optional in the contract but every shipped
engine has them, so each is present unless the document's `lacks` list names it.
Every other `pages.*` member (`pages.setName`, `pages.insertBlank`) is present unless `lacks` names
it, and every other `page.annotations.*` member (`exportAppearance`) is always present, because the
TypeScript handle answers both services through a proxy. The Rust `has` answers the same way; it
answered false for them until the stamp port asked. `forms` is a proxy as well: every `forms.*`
member but `submit` is present unless `lacks` names it, which is how a case takes
`forms.setSignatureAppearance` away (the signature port added this rule on both sides).

Four calls have fallbacks: `open` hands out the document whose key is the input's `id`,
`pages.list` answers from `pages`, `close` and `destroy` resolve. Anything else unanswered rejects
with `FakeEngineUnmatched`, and the generator refuses to write that trace unless the case sets
`allowUnmatched`: it is almost always a case that forgot a response.

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
behaviour. `allows` takes a list or `"*"`. `allowsAnnotationCreate` and `allowsAnnotationMutation`
default to `allows('doc.annotate.create')` and `allows('doc.annotate.modify')`.

Every call returns an `AbortablePromise`. An abort is logged as `{abort: <index of the call>, reason}`
and rejects with `AbortError`. An answer that arrives after an abort is still mapped (an `open`
still makes its handle) and then ignored, as the TypeScript's `resolve(map(value))` does.

## The probe

`src/probe.ts` and `crates/plugin-conformance/src/probe.rs` are a plugin configured entirely by the
vector, for testing the kernel without any real plugin's behaviour. The config is documented on
`ProbeConfig`; every hook logs as `<id>.<hook>`.

A probe with `create` builds its capability through the v3 `create` hook. Its api reaches one
`ControllerContext` member per method (the guarded `doc`, `events.source`, `waitFor`,
`serialQueue`, `latest`, `acquire`, `geometry`, `assertPageRef`, `getPage`), and the member list,
in order, is the `API` array in `src/suites/kernel.ts`. `sameAs` hands back the object an earlier
probe in the same case built, so a case can install one definition twice. The runner wraps each
plugin object once, so that stays the same object after instrumentation.

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

The other hazard is reference identity. A TypeScript reducer that returns `state` means "no change",
and the store then skips its notification: a Rust reducer returns `None` for that branch. `watch`
compares with `Object.is`, so a selector returning a fresh object fires on every change and one
returning a slice member fires when the reducer replaced that member; the Rust `watch` takes the
comparison explicitly, and a port picks the one that fires on the same changes.

## Adding a plugin

1. Add `src/suites/<id>.ts`: `registerPlugin('<id>', {factory, tokens})` and export `suites`.
2. Run `vectors`, then `coverage`, and read `vectors/coverage.json` for the plugin's files. Every
   uncovered line is either reached by a new case or explained in `multiplatform-plugins-status.md`.
   A file's `unsnapshottedOnly` ranges are lines that only `snapshot: "none"` cases reach. They
   count as covered, but no vector compares the state they leave behind, so a reducer listed there
   wants a case that snapshots its slice.
3. In the plugin's crate, implement `HarnessPlugin` behind the `harness` feature and add a test
   that calls `assert_suite` for each of its vector files.

## Fixtures

`fixtures/signature/fixtures.json` holds the keys, certificates, CMS blobs and recorded signatures
the `core/signature-*` vectors are made from. Key generation, ECDSA and RSA-PSS signatures are
random, so `fixtures/signature/generate.ts` runs once and its output is committed; the suite only
reads it, and `vectors:check` stays deterministic. Re-running the script replaces every key and
changes every vector that embeds one. The Rust test host reads the same file for the keys its
scripted WebCrypto hands out.
