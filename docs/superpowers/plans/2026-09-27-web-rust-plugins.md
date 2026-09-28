# Wiring the web viewer to the Rust plugin crates for evaluation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A dev-only build of the full viewer chrome that runs on `crates/epdf-kernel` and
`crates/plugins/*` compiled to WASM instead of `packages/core/main` and `packages/plugin/*`, so a
person can open a PDF in a browser and try every feature by hand.

**Architecture:** A new crate, `crates/epdf-web`, builds the kernel and every plugin for
`wasm32-unknown-unknown` with `wasm-bindgen` and runs on the main thread. PDFium stays where it is,
in the worker, reached through the existing `@embedpdf/engine`. Each plugin crate's conformance
`harness` adapter already calls any capability method by name with JSON arguments, so we reuse it
as the dynamic binding instead of writing bindings per plugin. A private TypeScript package,
`@embedpdf/core-wasm`, implements the `Kernel` interface over that module. A private Vite app
redirects `@embedpdf/core` and `@embedpdf/plugin-*` imports to it, so no framework or viewer source
changes.

**Tech Stack:** Rust 2024 (`rust-toolchain.toml`), `wasm-bindgen`, `js-sys`, `wasm-bindgen-futures`,
Vite 6, TypeScript, the existing `@embedpdf/engine` worker.

## Global Constraints

- Not for production. Nothing new is published: `@embedpdf/core-wasm` and the eval app are
  `"private": true`, `crates/epdf-web` inherits `publish = false`. No release flag, no size or speed
  gate, no changes to what `@embedpdf/viewer` ships.
- No source changes in `packages/framework/*`, `packages/viewer/*`, `packages/core/*` or
  `packages/plugin/*`. The redirect happens in the eval app's Vite config. If a published package
  does end up changing, it needs its own changeset file (`AGENTS.md`).
- The vectors keep passing. Changes to `crates/plugin-conformance`, `crates/epdf-kernel` and
  `crates/plugins/actions/src/harness.rs` are additions only, and after each task
  `cargo test -p plugin-conformance -p epdf-kernel -p epdf-plugin-commands` still passes with the
  same counts.
- Known leaks are fine. The harness keeps every returned function and promise in a table forever
  (`crates/plugin-conformance/src/harness.rs:92-101`). That is acceptable for a session of manual
  testing and is recorded, not fixed.

## What we are reusing and what is missing

What we reuse. `HarnessPlugin::build(factory, config, h)` builds any plugin from JSON config, and
`HarnessPlugin::call(capability, method, args, h)` returns an `Outcome` (`Returned`, `Threw`,
`Promise`) for any capability method (`crates/plugin-conformance/src/harness.rs:16,47`). Returned
functions come back as `{"$ref": n}` and promises as `{"$promise": n}` (`harness.rs:147,159`).
`Harness::describe` already prints a whole capability object as a table of those refs
(`harness.rs:192-219`). Every plugin has these adapters, and 15,278 vector cases cover them.

What is missing, one task each:

1. Callbacks are scripted. `Callback::call` replays an answer from the vector spec
   (`harness.rs:267-297`). The viewer passes live closures: `CommandDef.run`, `enabled` and
   `active` (`packages/plugin/commands/src/contract.ts:63-88`), `InteractionHandler`s
   (`packages/plugin/interaction/src/contract.ts:82-97`), i18n loaders, `setUiAdapter`,
   `setFilePickerProvider`.
2. The engine is a fake. The only `DocumentHandle` is
   `crates/plugin-conformance/src/fake_engine.rs`.
3. The clock is virtual. `rt.rs` only moves when someone calls `advance(ms)` (`rt.rs:198`), and
   the queue only drains on `run_until_stalled()` (`rt.rs:121`).
4. Nothing speaks JavaScript. There is no `wasm-bindgen` in `crates/`.
5. The UI expects some return values to be live objects, not JSON: `VisiblePage.transform` is a
   `PageTransform` with methods (`packages/core/geometry/src/index.ts:188-231`), and render paint
   plans carry a `PageImageHandle` with `objectUrl()` (`packages/plugin/render/src/paint-plan.ts:28-48`).

Plugins written in TypeScript get partial support. The chrome registers one, `demoToolsPlugin`
(`packages/viewer/chrome/src/config/demo-tools.plugin.ts`, 40 lines), and all it does is call
`interaction.registerTool(...)` from `init`. The shim runs a JS plugin's `init` after the Rust
kernel starts, with a context whose `get(token)` returns the Rust capability proxies. A JS plugin
with its own state, reducer, capability or effects is still unsupported and fails loudly.

Form scripting is required. The viewer turns it on (`actionsPlugin({ javascript: { enabled: true } })`
in `packages/viewer/chrome/src/viewer.tsx`), and the TypeScript falls back to a QuickJS sandbox
when no `sandboxFactory` is given (`packages/plugin/actions/src/scripting/environment.ts:40-45`).
The Rust crate has the `ScriptSandbox` trait (`crates/plugins/acrojs/src/host.rs:27-46`) and no
implementation, so Task 8 implements it over the same `@embedpdf/core-js-sandbox`. Its `boot` and
`run` are synchronous in both languages, so the Rust side calls straight into QuickJS with no
promise in between.

## Tasks and estimates

| Task | Deliverable | Estimate |
|---|---|---|
| 1 | `epdf-web` links every plugin for `wasm32-unknown-unknown` | half a day |
| 2 | Live callbacks and a harness with no fake engine | 1 to 2 days |
| 3 | Wall-clock driver and JS promise conversion | 1 to 2 days |
| 4 | `JsDocumentHandle`: the engine bridge | 3 to 5 days |
| 5 | `wasm-bindgen` exports: build kernel, call, subscribe | 2 to 3 days |
| 6 | `@embedpdf/core-wasm`: `Kernel` shim, capability proxies, cached getters, rehydration | 5 to 8 days |
| 7 | Eval app with the import redirect, and host inputs | 2 to 3 days |
| 8 | Form scripting over the QuickJS sandbox | 2 to 4 days |
| 9 | Manual test pass and the findings file | 2 to 3 days |

About five to seven weeks for one person. Tasks 2 and 3 can run in parallel with Task 4.

---

### Task 1: Link every plugin crate for `wasm32-unknown-unknown`

**Files:**
- Create: `crates/epdf-web/Cargo.toml`, `crates/epdf-web/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members`), `rust-toolchain.toml` (`targets`)

**Interfaces:**
- Produces: crate `epdf-web` (`crate-type = ["cdylib", "rlib"]`) depending on every plugin crate
  with the `harness` feature on. Later tasks add modules to it.

The `harness` feature stays on here, unlike a shipping build, because Tasks 2 and 5 use it as the
binding. It pulls in `plugin-conformance`, whose `std::fs` use (`plugin-conformance/src/lib.rs:54,92`)
compiles on wasm32 and only fails if called. We never call it.

- [ ] **Step 1: Add the target.** Add `"wasm32-unknown-unknown"` to `targets` in
  `rust-toolchain.toml`, then check:

```sh
rustup target list --installed | grep wasm32-unknown-unknown
```

- [ ] **Step 2: Create the crate.** `crates/epdf-web/Cargo.toml`:

```toml
[package]
name = "epdf-web"
version.workspace = true
edition.workspace = true
publish.workspace = true
description = "Evaluation only: the kernel and plugins on the browser main thread, over wasm-bindgen."

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
epdf-kernel = { path = "../epdf-kernel" }
plugin-conformance = { path = "../plugin-conformance" }
epdf-plugin-actions = { path = "../plugins/actions" }
epdf-plugin-annotation = { path = "../plugins/annotation" }
epdf-plugin-commands = { path = "../plugins/commands" }
epdf-plugin-form = { path = "../plugins/form" }
epdf-plugin-i18n = { path = "../plugins/i18n" }
epdf-plugin-interaction = { path = "../plugins/interaction" }
epdf-plugin-link = { path = "../plugins/link" }
epdf-plugin-measurement = { path = "../plugins/measurement" }
epdf-plugin-metadata = { path = "../plugins/metadata" }
epdf-plugin-page-edit = { path = "../plugins/page-edit" }
epdf-plugin-redaction = { path = "../plugins/redaction" }
epdf-plugin-render = { path = "../plugins/render" }
epdf-plugin-search = { path = "../plugins/search" }
epdf-plugin-selection = { path = "../plugins/selection" }
epdf-plugin-shell = { path = "../plugins/shell" }
epdf-plugin-signature = { path = "../plugins/signature" }
epdf-plugin-stage = { path = "../plugins/stage" }
epdf-plugin-stamp = { path = "../plugins/stamp" }
epdf-plugin-view-manager = { path = "../plugins/view-manager" }
serde_json = { version = "1", features = ["float_roundtrip"] }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
```

`crates/epdf-web/src/lib.rs`:

```rust
//! Evaluation only. The kernel and every plugin on the browser main thread,
//! driven by name through the conformance harness adapters.
//!
//! docs/superpowers/plans/2026-09-27-web-rust-plugins.md is the plan.
```

Add `"crates/epdf-web"` to `members` in the root `Cargo.toml`, after `crates/epdf-wasm`.

- [ ] **Step 3: Check it.**

```sh
cargo check -p epdf-web --target wasm32-unknown-unknown
```

The likely failure is `getrandom` 0.2 refusing the target: the lock has
`rsa → num-bigint-dig → rand → rand_core 0.6.4 → getrandom 0.2.17` without the `js` feature. Fix:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
getrandom = { version = "0.2", features = ["js"] }
```

Any other failure: fix it in `epdf-web` if possible. If a plugin crate itself has to change, keep
the change behind `#[cfg(target_arch = "wasm32")]` and note it in the findings file (Task 9).

- [ ] **Step 4: Host tests unchanged.** `cargo test -p epdf-kernel -p plugin-conformance` passes.
- [ ] **Step 5: Commit** `crates/epdf-web`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`:
  "Link every plugin crate for wasm32-unknown-unknown".

### Task 2: Live callbacks and a harness without the fake

**Files:**
- Modify: `crates/plugin-conformance/src/harness.rs`
- Test: `crates/plugin-conformance/tests/live_harness.rs`

**Interfaces:**
- Produces:
  - `pub type LiveFn = Rc<dyn Fn(&Value, Vec<Value>) -> Answer>;` The first argument is the `$fn`
    spec, so the host can find its function by id.
  - `impl Harness { pub fn live(adapters: Vec<Rc<dyn HarnessPlugin>>, tokens: HashMap<String, AnyToken>, live: LiveFn) -> Harness }`
  - `Callback::call` forwards to the harness's `LiveFn` when there is one, and skips `log`.
  - `Harness::track` and `Harness::log` record nothing on a live harness, so the trace does not
    grow for the whole session.

The adapters already turn every `{"$fn": ...}` in a config or argument into `h.callback(spec)`
(for example `crates/plugins/commands/src/harness.rs:168,343`). Making `callback` live is therefore
enough to make every plugin's callbacks live, with no adapter changes.

- [ ] **Step 1: Write the failing test.** `crates/plugin-conformance/tests/live_harness.rs` builds
  a live harness whose `LiveFn` records the spec and args and returns
  `Answer::Returns(json!(42))`, makes a callback from `json!({"$fn": "js:7"})`, calls it with
  `vec![json!("a")]`, and asserts the answer is `42`, the recorded spec's `$fn` is `"js:7"`, and
  the args are `["a"]`.
- [ ] **Step 2: Run it.** `cargo test -p plugin-conformance --test live_harness`. Expected: FAIL,
  `no function or associated item named live`.
- [ ] **Step 3: Implement.** Add `live: Option<LiveFn>` to `Inner` (`harness.rs:92`), set it to
  `None` in `new` and `Some` in `live`. At the top of `Callback::call`:

```rust
if let Some(live) = self.harness.0.live.as_ref() {
    return live(&self.spec, args);
}
```

At the top of `log` and inside `track`'s `on_settle`, return early when `live` is set. `track`
still pushes the promise into the table so `{"$promise": n}` resolves.

- [ ] **Step 4: Run it and the vectors.**
  `cargo test -p plugin-conformance && cargo test -p epdf-plugin-commands -p epdf-plugin-stage`.
  Expected: PASS, vector counts unchanged.
- [ ] **Step 5: Commit** "Let the conformance harness call live callbacks".

### Task 3: Drive the runtime from the browser

**Files:**
- Create: `crates/epdf-web/src/runtime.rs`
- Modify: `crates/epdf-kernel/src/rt.rs` (add one function)
- Test: `crates/epdf-kernel/tests/next_deadline.rs`, `crates/epdf-web/tests/runtime.rs`
  (`wasm-bindgen-test`)

**Interfaces:**
- Produces:
  - `epdf_kernel::rt::next_deadline() -> Option<f64>`: the earliest scheduled timer's deadline on
    the virtual clock, `None` when no timer is pending. `pending_timers()` (`rt.rs:249`) only gives
    a count.
  - `epdf_web::runtime::after_entry()`: call at the end of every export. Runs
    `rt::run_until_stalled()`, then if `next_deadline()` is `Some(d)`, arms one JS `setTimeout` for
    `d - rt::now()` ms that calls `rt::advance(elapsed)` with the real elapsed time and then
    `after_entry()` again. At most one timeout is armed at a time.
  - `epdf_web::runtime::to_js(p: Promise<Value>) -> js_sys::Promise` and
    `epdf_web::runtime::from_js(p: js_sys::Promise) -> Promise<Value>`. A settlement coming in from
    JS calls `after_entry()`.

The virtual clock then trails wall time by at most one timer's lateness, which is fine for manual
testing. Microtask order across the boundary is not the order the vectors pin: a JS `await` on a
converted promise runs on the JS microtask queue, not the Rust ready queue. Anything that looks
like an ordering bug during Task 9 goes in the findings file with that in mind.

- [ ] **Step 1: Failing test for `next_deadline`.** Spawn a task that sleeps 30 ms and one that
  sleeps 10 ms (use the kernel's existing timer helper in `rt.rs`), assert `next_deadline()` is
  `Some(10.0)`, `advance(10.0)`, assert `Some(30.0)`, `advance(20.0)`, assert `None`.
- [ ] **Step 2: Run it.** `cargo test -p epdf-kernel --test next_deadline`. Expected: FAIL, not
  found.
- [ ] **Step 3: Implement** `next_deadline` next to `pending_timers`, reading the minimum deadline
  from `rt.timers`.
- [ ] **Step 4: Implement `runtime.rs`** as described, with `wasm_bindgen_futures::JsFuture` behind
  `from_js` and `js_sys::Promise::new` behind `to_js`. Errors cross as
  `{"$error": {name, message, code}}`, the canonical shape the kernel already prints.
- [ ] **Step 5: Test in a browser.** A `wasm-bindgen-test` that spawns a 20 ms sleep, calls
  `after_entry()`, awaits a 50 ms JS timeout, and asserts the task finished.
  `wasm-pack test --headless --chrome crates/epdf-web`. Expected: PASS.
- [ ] **Step 6: Commit** "Drive the kernel runtime from browser timers".

### Task 4: The engine bridge

**Files:**
- Create: `crates/epdf-web/src/engine.rs`
- Create: `packages/core-wasm/src/engine-bridge.ts` (package created in Task 6; create the
  directory now)
- Test: `packages/core-wasm/test/engine-bridge.test.ts`

**Interfaces:**
- Consumes: `Engine`, `DocumentHandle` traits (`crates/epdf-kernel/src/engine.rs:27,46`), in
  particular `DocumentHandle::call(path, pon: Option<u32>, args: Vec<Value>) -> Promise<Value>`
  (`engine.rs:60`); `runtime::from_js` (Task 3).
- Produces:
  - Rust `JsEngine` and `JsDocumentHandle` implementing those traits by calling a JS object with
    methods `open(source) -> Promise<docId>`, `call(docId, path, pon, argsJson) -> Promise<json>`,
    `has(docId, member) -> bool`, `subscribe(docId, listener) -> unsubscribe`, `destroy(docId)`.
  - TS `createEngineBridge(engine: Engine): EngineBridge` in `engine-bridge.ts`, implementing that
    object over `@embedpdf/engine`'s `Engine` and `DocumentHandle`
    (`packages/engine/core/src/engine/DocumentHandle.ts:26-140`). `path` is `service.method`
    (`"pages.list"`), resolved with a property walk. `pon` selects `doc.page(ref)`.
  - A host-object table: any value in an engine result that is not plain JSON (a
    `PageImageHandle`, a `Blob`) is replaced by `{"$host": n}` on the way in, and
    `bridge.host(n)` returns the original. `Blob` and `File` arguments are read to bytes and sent
    as `{"$bytes": base64}`, the kernel's byte spelling.

- [ ] **Step 1: Failing test.** In `engine-bridge.test.ts`, create an engine with
  `createLocalEngine` under Node (the `InlineTransport` path, `packages/engine/main/src/index.ts:101-140`),
  open a fixture PDF from `tooling/engine-conformance`, call
  `bridge.call(docId, 'pages.list', null, '[]')`, and assert the parsed result has the fixture's
  page count. Call a render method and assert the result contains a `$host` key and
  `bridge.host(n)` has `objectUrl`.
- [ ] **Step 2: Run it.** `pnpm --filter @embedpdf/core-wasm exec vitest run engine-bridge`.
  Expected: FAIL, module not found.
- [ ] **Step 3: Implement `engine-bridge.ts`**, then `engine.rs` over it.
- [ ] **Step 4: Run the test.** Expected: PASS. Then
  `cargo check -p epdf-web --target wasm32-unknown-unknown`.
- [ ] **Step 5: Commit** "Bridge the Rust kernel's engine contract to @embedpdf/engine".

### Task 5: `wasm-bindgen` exports

**Files:**
- Create: `crates/epdf-web/src/exports.rs`, `crates/epdf-web/src/adapters.rs`
- Create: `crates/epdf-web/scripts/build.sh`

**Interfaces:**
- Consumes: `Harness::live` (Task 2), `runtime` (Task 3), `JsEngine` (Task 4), every crate's
  `HarnessPlugin` adapter, built the way `crates/plugin-conformance/src/scenario.rs:406-480` builds
  a kernel.
- Produces a `#[wasm_bindgen]` class `WasmKernel`:
  - `new(plugins_json: String, engine: JsValue, call_js: js_sys::Function) -> WasmKernel`.
    `plugins_json` is `[{"factory": "stage", "config": {...}}, ...]` with JS functions already
    replaced by `{"$fn": "js:<id>"}`. `call_js(id, argsJson)` is how Rust calls them back; its
    return (a value or a Promise) becomes an `Answer`.
  - `start() -> js_sys::Promise`, `destroy()`.
  - `capability(token_name: String, doc_id: Option<String>) -> String`: `Harness::describe` of the
    capability, a JSON object of member name to `{"$ref": n}` or plain value.
  - `invoke_ref(id: u32, args_json: String) -> String`: calls a returned function. The result is
    `{"returned": v}`, `{"threw": e}` or `{"promise": n}`.
  - `promise(id: u32) -> js_sys::Promise`.
  - `state_json() -> String`, `subscribe(listener: js_sys::Function) -> u32`, `unsubscribe(id: u32)`.
  - `documents()`, which returns the documents capability as `capability` does.

Every export ends with `runtime::after_entry()`.

- [ ] **Step 1: Write `adapters.rs`,** a `fn adapters() -> Vec<Rc<dyn HarnessPlugin>>` listing every
  plugin crate's adapter, the same list `crates/plugin-conformance` uses to run all suites.
- [ ] **Step 2: Write `exports.rs`.**
- [ ] **Step 3: Test in a browser.** A `wasm-bindgen-test` that builds a `WasmKernel` with
  `metadata` and `shell` over a stub engine object, starts it, reads `capability("shell", None)`,
  invokes one getter ref, and asserts `"returned"`.
  `wasm-pack test --headless --chrome crates/epdf-web`. Expected: PASS.
- [ ] **Step 4: Build script.** `crates/epdf-web/scripts/build.sh`:

```sh
#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
cargo build -p epdf-web --release --target wasm32-unknown-unknown --manifest-path "$root/Cargo.toml"
wasm-bindgen --target web --out-dir "$root/packages/core-wasm/wasm" \
  "$root/target/wasm32-unknown-unknown/release/epdf_web.wasm"
```

Run it. Expected: `packages/core-wasm/wasm/epdf_web.js` and `epdf_web_bg.wasm` exist. Add
`packages/core-wasm/wasm/` to `.gitignore`.

- [ ] **Step 5: Commit** "Export the Rust kernel to JavaScript through the harness adapters".

### Task 6: `@embedpdf/core-wasm`, the `Kernel` shim

**Files:**
- Create: `packages/core-wasm/package.json` (`"private": true`, `"build:wasm": "bash ../../crates/epdf-web/scripts/build.sh"`)
- Create: `packages/core-wasm/src/index.ts`, `src/kernel.ts`, `src/proxy.ts`, `src/plugins.ts`,
  `src/rehydrate.ts`
- Test: `packages/core-wasm/test/kernel.test.ts`

**Interfaces:**
- Consumes: `WasmKernel` (Task 5), `createEngineBridge` (Task 4), the `Kernel` interface in
  `packages/core/main/src/kernel.ts:84-117`.
- Produces:
  - `createKernel({ engine, plugins }): Kernel`, same signature as `@embedpdf/core`.
  - One factory per plugin with the TS name and config type (`stagePlugin`, `renderPlugin`, ...,
    `feedbackPlugin`), each returning a descriptor `{ $rust: 'stage', config }` typed as the TS
    `PluginDef`. Tokens are re-exported from the TS plugin packages unchanged, so
    `kernel.capability(StageToken)` maps by `token.name`.
  - An entry module per redirected package (`src/redirect/core.ts`,
    `src/redirect/plugin-<id>.ts`) that does `export * from` the real package and then overrides
    `createKernel` or the plugin factory. That keeps every token, type and pure helper
    (`createScrollHandler`, `selectionHandleGeom`, `pageTransform`, `solve`, `scene`) the framework
    imports as a value.

Proxies. `capability(token)` returns one JS object per (plugin, document), created once. Each
`$ref` member becomes a function that serializes its arguments (functions become
`{"$fn": "js:<id>"}` in a function table, `AbortSignal` is dropped and wired to the promise's
abort), calls `invoke_ref`, and maps the outcome: `returned` is parsed and rehydrated, `threw` is
thrown as an `Error` with `name` and `code` set, `promise` becomes `kernel.promise(n)`, and a
nested `$ref` becomes a function.

Reference stability. React selectors re-run on every store change and compare by reference
(`packages/framework/react/src/runtime.tsx:53-253`); a getter that returns a fresh object each
time makes `useSyncExternalStore` loop. The shim keeps a state version that `subscribe` bumps, and
caches every call's rehydrated result by (member, argument JSON, version). A call at the same
version returns the same object. This invalidates more than it needs to, which is fine here.

Rehydration. `src/rehydrate.ts` holds a table from capability member to a function fixing its
result. It starts with two entries: `listVisiblePages` wraps each `transform` with
`pageTransform(...)` from `@embedpdf/core-geometry`, and render's `getPlan` swaps each
`{"$host": n}` for `bridge.host(n)`. Task 9 adds entries as the UI hits more.

JS-authored plugins. An entry in `plugins` without `$rust` is a TS `PluginDef`. If it has only
`id`, `scope`, `requires`, `optional` and `init`, the shim keeps it and, once `start()` resolves,
calls `init(ctx)` once (workspace scope) or once per opened document (document scope, on the
documents capability's open event). `ctx.get(token)` and `ctx.tryGet(token)` return the same
capability proxies `kernel.capability` does, and `ctx.doc` is the document id. Any other member
(`initialState`, `reduce`, `capability`, `create`, `effects`) makes `createKernel` throw
`[core-wasm] JS plugin "<id>" has <member>; only init-only JS plugins run in the eval build`.

- [ ] **Step 1: Failing test.** `kernel.test.ts`: `createKernel` with `stagePlugin()`,
  `interactionPlugin()`, `renderPlugin()` over the Node engine and the fixture PDF, `start()`,
  `documents.openAll()`, then assert `kernel.capability(StageToken).listVisiblePages()` returns
  the same array object twice in a row, and a different one after `panBy(0, 100)`. Assert
  `visible[0].transform.toPixels` is a function. A second test adds the real `demoToolsPlugin`
  from `packages/viewer/chrome/src/config/demo-tools.plugin.ts`, opens the fixture, calls
  `kernel.capability(InteractionToken).activateTool('signature')` and asserts
  `getActiveTool()` is `'signature'`. A third passes a JS plugin with a `reduce` member and asserts
  `createKernel` throws the message above.
- [ ] **Step 2: Run it.** `pnpm --filter @embedpdf/core-wasm exec vitest run kernel`. Expected:
  FAIL.
- [ ] **Step 3: Implement** `kernel.ts`, `proxy.ts`, `plugins.ts`, `rehydrate.ts`, the redirect
  entries.
- [ ] **Step 4: Run the test.** Expected: PASS.
- [ ] **Step 5: Commit** "Add the private Kernel shim over the Rust plugin crates".

### Task 7: The eval app

**Files:**
- Create: `examples/rust-kernel-eval/package.json` (`"private": true`, `"dev": "pnpm --filter @embedpdf/core-wasm build:wasm && vite"`)
- Create: `examples/rust-kernel-eval/vite.config.ts`, `index.html`, `src/main.ts`
- Create: `examples/rust-kernel-eval/src/hosts.ts`

**Interfaces:**
- Consumes: `@embedpdf/core-wasm` redirect entries (Task 6), the viewer chrome from source the way
  `packages/viewer/main` mounts it in dev (`packages/viewer/main/src/dev.ts`, `index.html`, and the
  React-to-Preact aliases in `packages/viewer/main/vite.config.ts`).

The Vite config copies viewer/main's aliases and adds one `resolveId` plugin: an import of
`@embedpdf/core` or `@embedpdf/plugin-<id>` resolves to
`packages/core-wasm/src/redirect/<name>.ts`, unless the importer is inside `packages/core-wasm`
(the redirect modules import the real packages). Everything else resolves normally.

Host inputs, in `src/hosts.ts`, passed through plugin config where the Rust crate takes them:
stage's `Scheduler` (`crates/plugins/stage/src/contract.rs:315`) and render's `RenderHost`
(`crates/plugins/render/src/host_contract.rs:67`) over `requestAnimationFrame`;
`AnnotationHost.random_uuid` (`crates/plugins/annotation/src/contract.rs:227`) over
`crypto.randomUUID`; console sinks over `console`. Signatures run with no signer, so a mark is
drawn without sealing. `WebCrypto` and `PersonalKeyStore` are not wired. The script sandbox is
Task 8.

- [ ] **Step 1: Write the app.**
- [ ] **Step 2: Run it.** `pnpm --filter @embedpdf/example-rust-kernel-eval dev`, open the printed
  URL. Expected: the chrome loads, a sample PDF opens, the console shows no errors, and the
  Insert tab's Signature button toggles active when clicked (the demo tools plugin ran).
- [ ] **Step 3: Prove it is the Rust kernel.** In the browser console,
  `document.querySelector('embedpdf-viewer').viewer` (or the handle the chrome exposes,
  `packages/viewer/chrome/src/handle.ts:84-115`) reaches a kernel whose constructor is the shim's.
  Add a visible "Rust kernel" badge in `index.html` so screenshots are unambiguous.
- [ ] **Step 4: Commit** "Add the Rust kernel eval app".

### Task 8: Form scripting over the QuickJS sandbox

**Files:**
- Modify: `crates/plugin-conformance/src/harness.rs` (a host-object registry on the live harness)
- Modify: `crates/plugins/actions/src/harness.rs:90-96` (`javascript_config`)
- Create: `crates/epdf-web/src/sandbox.rs`
- Create: `packages/core-wasm/src/sandbox.ts`
- Modify: `packages/core-wasm/src/plugins.ts` (`actionsPlugin`)
- Test: `packages/core-wasm/test/scripting.test.ts`

**Interfaces:**
- Consumes: `ScriptSandbox` and `ScriptSandboxFactory` (`crates/plugins/acrojs/src/host.rs:27-50`),
  `createQuickJsSandbox` from `@embedpdf/core-js-sandbox`, `runtime::from_js` (Task 3),
  `Harness::live` (Task 2).
- Produces:
  - `impl Harness { pub fn set_host(&self, name: &str, value: Rc<dyn Any>); pub fn host(&self, name: &str) -> Option<Rc<dyn Any>> }`.
  - In the actions adapter, a `sandbox` spec of `{"$host": "<name>"}` resolves to
    `h.host(name)` downcast to `ScriptSandboxFactory`. Every other spec still goes to
    `scripted_sandbox_factory`, so the vectors are untouched.
  - Rust `JsScriptSandbox` implementing `ScriptSandbox` over a JS object with
    `disposed(): boolean`, `boot(sourcesJson, inputJson, budgetJson): string`,
    `run(source, inputJson, budgetJson): string`, `dispose(): void`. A JS throw becomes
    `Err(Error)` with the thrown `name` and `message`.
  - TS `createSandboxBridge(): () => Promise<JsonSandbox>` in `sandbox.ts`: calls
    `createQuickJsSandbox()` and wraps the result as that JSON object.
  - The shim's `actionsPlugin(config)`: when `config.javascript?.enabled` and no
    `sandboxFactory` is given, it registers `createSandboxBridge()` as host `"quickjs"` and sends
    `sandbox: {"$host": "quickjs"}`. A caller-supplied `sandboxFactory` is wrapped the same way.
    This matches the TypeScript default in `environment.ts:40-45`.

`ScriptInput`, `ScriptOutput` and `ScriptBudget` are serde types in `crates/plugins/acrojs/src/types.rs`
whose JSON is pinned by the acrojs vectors, so the QuickJS sandbox receives exactly the shapes the
TypeScript host sends it. The QuickJS sandbox installs its own prelude (`QuickJsSandbox.ts:143`),
so the Rust side sends none.

- [ ] **Step 1: Failing test.** `scripting.test.ts`: `createKernel` with `interactionPlugin()`,
  `actionsPlugin({ javascript: { enabled: true } })`, `formPlugin()` over the Node engine, a UI
  adapter set through `setUiAdapter` whose `alert` records its message, then open
  `tooling/engine-conformance/fixtures/document_actions.pdf`. Its open action is
  `app.alert('opened')` (`tooling/engine-conformance/fixtures/generate.mjs:611-613`). Assert the
  recorded alerts include `'opened'`.
- [ ] **Step 2: Run it.** `pnpm --filter @embedpdf/core-wasm exec vitest run scripting`.
  Expected: FAIL, no alert (no sandbox, so scripting is off).
- [ ] **Step 3: Implement** the registry, the actions adapter branch, `sandbox.rs`, `sandbox.ts`
  and the `actionsPlugin` change.
- [ ] **Step 4: Run the test and the vectors.** The vitest passes, and
  `cargo test -p plugin-conformance -p epdf-plugin-actions -p epdf-plugin-form -p epdf-acrojs`
  passes with unchanged counts.
- [ ] **Step 5: Try it in the eval app.** Open a PDF form with format and calculate scripts (any
  form using `AFNumber_Format` and a sum field). Type into a line item. Expected: the value
  reformats on blur and the total updates.
- [ ] **Step 6: Commit** "Run form scripts through QuickJS in the Rust eval build".

### Task 9: Manual test pass

**Files:**
- Create: `docs/research/rust-plugins-web-eval.md`

- [ ] **Step 1: Walk the features,** each on the eval app and on `packages/viewer/main` dev side
  by side, with the same PDF: open and close tabs; scroll, zoom, fit, spreads, thumbnails; text
  selection and copy; search; each annotation tool, edit and delete; stamps and images; form fill,
  including format, validate and calculate scripts and a document open action; links; redact
  and apply; measurement; page rotate, delete, move; signatures without a signer; locale switch;
  toolbar overflow on a narrow window.
- [ ] **Step 2: Record each** as works, differs (what the TypeScript did instead), or broken (the
  console error), with the plugin it lands in and whether the cause is the port, the binding
  (rehydration, caching, ordering, the sandbox bridge), or the eval scope (unsigned signatures).
- [ ] **Step 3: Fix binding bugs as you go** (a missing rehydrate entry, a cache miss) and commit
  each separately. Port bugs get a vector case in the crate, per
  `multiplatform-plugins-status.md`, and a commit of their own.
- [ ] **Step 4: Commit** the findings file: "Record the Rust plugin eval pass".
