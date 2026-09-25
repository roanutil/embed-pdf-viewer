# Porting the plugin layer to Rust

`multiplatform-plan.md` section 2 keeps the web kernel, the plugins and the viewer in TypeScript.
This plan reverses that for the kernel and the plugins, and keeps it for everything that touches a
DOM or a framework. It follows the engine port's method: the shipping TypeScript is the oracle,
vectors generated from it are committed, and a Rust runner replays every vector.
`multiplatform-port-status.md` is where the engine port got to. This file is the plan for the layer
above it, and `multiplatform-plugins-status.md` will say where this one got to.

The reason to port at all is the native reader products. Without a JavaScript runtime, an iOS or
Android viewer today has to re-derive tiling, camera math, selection, hit testing and annotation
editing in Swift and Kotlin, twice, with no oracle. With the plugins in Rust, the facades carry
platform types and gestures, and the behaviour is shared.

The cost is a second implementation of 22,665 lines of plugin source plus the parts of `core/*`
they import, kept in step with the TypeScript until one of them is retired. The web keeps running
the TypeScript. Nothing here changes what ships on the web.

## 1. What ports, and what does not

| Source | Lines | Ports to | Stays TypeScript |
|---|---|---|---|
| `packages/core/main` | 1,674 | `crates/epdf-kernel` | nothing: store, plan, scope, event hook, serial queue, context, document lifecycle all port |
| `packages/core/geometry` | 801 | `crates/epdf-geometry` | nothing |
| `packages/core/stage` | 919 | `crates/epdf-stage-core` | nothing |
| `packages/core/ui` | 1,207 | `crates/epdf-ui-core` | nothing |
| `packages/core/annotation` | 7,527 | `crates/epdf-annotation-core` | nothing that a plugin imports |
| `packages/core/acrojs` | 2,014 | host bridge (`host.ts`, `input.ts`, `color.ts`, `actions.ts`) into the form and actions crates | `prelude.ts` is JavaScript that runs inside the sandbox; Rust ships the same text as data |
| `packages/core/js-sandbox` | 305 | a `ScriptSandbox` trait | the QuickJS implementation. A host supplies one (QuickJS-wasm on web, JavaScriptCore on iOS, QuickJS on Android) or none |
| `packages/plugin/*` | 22,665 | one crate per plugin, `crates/epdf-plugin-<id>` | the seven lines that read `window` or a DOM type (in `actions`, `annotation` and `stage`), which become host inputs |
| `packages/framework/*`, `packages/viewer/*` | | | all of it |

The form scripting boundary is the one the engine port already drew for `forms.applyEffects`
(`multiplatform-port-hazards.md` 3.5): a script runs in a sandbox, and the decisions it makes come
back as data. On a host with no sandbox the plugin behaves as the TypeScript does when scripting is
disabled, and that configuration is itself a vector case.

## 2. The Rust shape

### 2.1 Kernel

`epdf-kernel` is `core/main` in Rust. The store holds one state tree, `core` plus keyed plugin
slices, with the same two channels: state changed and action dispatched. Slice keys are the same
strings (`pluginId`, `pluginId::docId`). Action dispatch order, listener re-entry (finish the pass,
then run another) and listener isolation are the same, because the vectors record them.

A plugin is a `PluginDef` trait object with the same members as the TypeScript `PluginDef`: id,
token, scope, requires, optional, initial state, reducer, capability, init, effects. State and
actions are concrete types per plugin, serialized with serde into exactly the JSON the TypeScript
state would print, so the vectors can compare them.

Three JavaScript semantics need a Rust answer, and each is a hazard for every plugin port:

1. Reference identity. A TypeScript reducer that returns the same `state` object means "no change":
   the store skips the change notification. A Rust reducer returns `Option<State>`, and `None` is
   that branch. `watch` compares with `Object.is`, so a selector returning a fresh object fires on
   every change; a Rust watch has to reproduce which selectors fire, not what their values are
   equal to. Slices live in `Rc`, and a watch that compared references in TypeScript compares
   `Rc::ptr_eq` in Rust.
2. Microtask order. Every `await` in the TypeScript is a turn of the microtask queue. The Rust
   runtime is a single-threaded executor with a FIFO ready queue, so one `.await` on a resolved
   future is one turn. Two concurrent chains that dispatch in an order the vectors record have to
   interleave the same way, and where they cannot, the vector says which order it pinned and why.
3. Time. Timers go through an injected clock on both sides. The TypeScript generator runs under
   `@sinonjs/fake-timers`; the Rust runtime owns a virtual clock that fires timers in deadline
   order and drains the ready queue between each.

### 2.2 Engine boundary

The plugins' engine is `engine-core`'s `Engine` and `DocumentHandle` contract, which three things
already implement (local WASM, cloud, the test fake). The Rust kernel defines the same contract as a
trait whose calls are one JSON envelope per operation, `service.method(args)`, returning a future
and an abort handle. Each plugin wraps the calls it makes in typed functions over serde DTOs.

That makes the fake engine one implementation in each language that answers from a table, and it
means the kernel does not link PDFium. An adapter onto `crates/epdf-engine` is a separate crate and
the last phase (section 5). Its gate is an integration test over a fixture, not a vector, because
the engine is already pinned on its own.

### 2.3 The harness surface

Every plugin crate has a `harness` cargo feature exposing one function: given a method name and JSON
arguments, call the capability and return a JSON outcome. This is the same move `host.rs` made for
the engine wire. The conformance runner needs it, and a Swift or Kotlin facade that wants a dynamic
entry point can use it too. Typed Rust callers use the capability directly.

## 3. Conformance

Vectors live under `tooling/plugin-conformance/vectors/`, generated by
`tooling/plugin-conformance/src/`, one generator per crate. Each generator has a `:check` script
that regenerates in memory and fails on a diff. Two families:

Function vectors cover the pure modules: geometry, stage math, the UI solver, annotation geometry,
tile planning, paint plans, reducers called directly. Each case is a function name, JSON arguments
and the JSON result or thrown error. This family is where most of the arithmetic gets pinned, and
it is cheap to grow.

Scenario vectors cover everything with a lifecycle. A case names the plugins and their config, the
fake engine's documents and response table, and an ordered list of steps: start, open, unlock,
close, destroy, call a capability method, emit an engine document event, resolve a deferred engine
call, advance the clock, invoke a returned function. After each step the runner drains the queue and
records a trace entry: the call's outcome (returned, threw, resolved, rejected, or still pending),
the actions dispatched in order, the engine calls made with their arguments, any capability events,
calls made on harness-supplied callbacks, and the full state tree. A Rust run has to reproduce the
trace exactly.

Callbacks cross as data. An argument `{"$fn": "name", "returns": ...}` becomes a function that
records its calls in the trace and returns the scripted value; an interaction handler is an object
whose members are such functions. A function returned by the plugin (an unsubscribe, a teardown) is
recorded as `{"$ref": n}` and a later step may invoke it.

Canonical JSON is the comparison format. `undefined` members are dropped, `Map` becomes an object
and `Set` an array in insertion order, byte arrays become `{"$bytes": base64}`, errors become
`{"$error": {name, message, code}}`, and numbers compare as `f64`. Error messages are compared
exactly: a message is behaviour a UI can show.

### 3.1 Coverage is the completeness gate

"All behaviour possible" needs a measure, and the measure is the TypeScript's own coverage under the
generators. Every generator runs under `c8`; the summary for each ported source file is committed
next to its vectors, and `:check` fails if a line that was covered stops being covered. A line the
vectors do not reach is listed in the status file with its reason: unreachable from the public
surface, DOM-only, or a gap still to close. The target is every reachable branch.

On the Rust side, each runner is mutation-tested with `cargo-mutants`, as every engine runner was.
A surviving mutant either gets a case or is documented at the line it survives on.

## 4. Order

Groups follow the plugin dependency graph, so each lands with its prerequisites already pinned.

| Group | Crates | Depends on |
|---|---|---|
| K0 | `epdf-kernel`, `plugin-conformance` (runner, canonical JSON, fake engine, executor), `tooling/plugin-conformance` | nothing |
| K1 | `epdf-geometry`, `epdf-stage-core`, `epdf-ui-core` | K0 |
| P1 | `i18n`, `shell`, `metadata`, `page-edit`, `view-manager`, `interaction` | K0, K1 |
| P2 | `render`, `commands`, `actions` (with the `ScriptSandbox` trait) | P1 |
| P3 | `stage`, `selection`, `search` | P2 |
| K2 | `epdf-annotation-core` | K1 |
| P4 | `annotation` | P3, K2 |
| P5 | `form`, `link`, `redaction`, `stamp` | P4 |
| P6 | `epdf-viewer-host`: the kernel and plugins over `epdf-engine`, plus uniffi exports | P5 |

Within a group the crates are independent and land in parallel. A group is done when its vectors
are committed, the Rust runner is green, coverage is recorded and the mutation run is documented.

## 5. What this plan does not do

It does not move the web to the Rust plugins. The viewer keeps the TypeScript kernel, and the
performance evidence in `multiplatform-plan.md` section 3 (1,000 anchored shapes: TypeScript
121.96 µs against Rust 200.76 µs) argues against routing web projection through WASM without a new
measurement.

It does not write the Swift or Kotlin facades. P6 exports the kernel through uniffi; the facades are
the native reader products' own work.

It does not make form scripting available where no sandbox exists. That is a capability, not a port.
