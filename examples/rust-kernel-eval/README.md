# Rust kernel eval

This is the viewer chrome, from source, running on the Rust kernel (`@embedpdf/core-wasm`)
instead of the TypeScript one. It exists to evaluate the Rust plugin crates against the real
chrome, not to ship anything: it is dev-only, not published, and not a template for production
use. See `docs/superpowers/plans/2026-09-27-web-rust-plugins.md` for the plan this app checks
work against.

## Prerequisites

- `wasm-bindgen-cli` 0.2.127 exactly, on `PATH`. `crates/epdf-web/Cargo.toml` pins
  `wasm-bindgen =0.2.127`; a different CLI version fails at build time with a version-mismatch
  error. Install with `cargo install wasm-bindgen-cli --version 0.2.127`.
- The `wasm32-unknown-unknown` Rust target. `rust-toolchain.toml` lists it already, so `rustup`
  installs it on the first `cargo` command anywhere in the repo.
- The PDFium runtime artifacts under `packages/engine/runtime/dist/` and
  `packages/engine/runtime/npm/wasm32/lib/`. These are gitignored build output, and nothing in
  this app fetches or builds them for you. `pnpm bootstrap` (below) checks for them and prints exact
  commands if they are missing.

## Setup

Run once per checkout, from this directory:

1. `pnpm install` at the repo root, if you have not already.
2. `pnpm bootstrap`. This builds `packages/engine/main/workers/` and
   `packages/default-stamps/locales/`, two gitignored outputs this app's chrome needs that are
   not built by anything else here. It takes about 20 seconds, most of it the engine's own
   dependency build. If the PDFium runtime artifacts above are missing, it fails immediately with
   the commands to fetch or build them instead of leaving that failure for the browser to surface
   later as a missing-file error against a running dev server.

`bootstrap` is not chained onto `eval`: rebuilding the engine costs those same ~20 seconds every
time, and `eval` should stay fast once `bootstrap` has run. Re-run `pnpm bootstrap` after pulling
changes to `packages/engine/main`, `packages/default-stamps`, or `packages/engine/runtime`.

## Running it

`pnpm eval` builds the Rust kernel (`pnpm --filter @embedpdf/core-wasm build:wasm`) and starts
Vite. Open the printed URL. The chrome loads, a sample PDF opens, and the console shows no
errors until the kernel is destroyed. Destroying it logs 16
`[kernel] capability("stage") on a destroying kernel` reports, where the TypeScript logs 17 (see
"Stage reports at destroy" in `docs/research/rust-plugins-web-eval.md`).

The script is `eval`, not `dev`, so the root `pnpm dev` (`turbo run dev --parallel`) does not
start a cargo build. The scripts are named `bootstrap` and `eval` rather than `setup` and `dev`
because `pnpm setup` is pnpm's own command, which edits your shell profile and never runs a
package script.

A Rust panic reaches the console as `[epdf-web] panic:` with its message and location, followed
by the `RuntimeError: unreachable` it aborts with. After that, or after a `recursive use of an
object detected` error, the kernel's state is broken: reload the page.

## Confirming it is the Rust kernel

The orange "Rust kernel" badge in the top bar is there so screenshots are unambiguous. To confirm
it from the console: `document.querySelector('embedpdf-viewer').viewer` reaches the handle
described in `packages/viewer/chrome/src/handle.ts:84-115`; its members are the shim's `Wire` ref
functions rather than plain TypeScript closures, for example
`viewer.documents.list.toString()` reads `(...args) => this.call(id, member, args)`.

## Known behaviors

A first load right after Vite has re-optimized dependencies sometimes does not boot. Vite reloads
the page itself when this happens; a second load works normally. This is not specific to the Rust
kernel.

Form scripting runs. The chrome's `actionsPlugin({ javascript: { enabled: true } })` gets the
QuickJS sandbox from `@embedpdf/core-js-sandbox` by default, as the TypeScript does, and the Rust
actions plugin calls it synchronously through the shim (`packages/core-wasm/src/sandbox.ts`).
Format, keystroke, validate and calculate scripts all go through it. To try it, open
`packages/plugin/form/test/fixtures/action_form_fixture.pdf`: typing `1234.5` into Venue and
tabbing out shows `1,234.50`, and the calculated total follows.

What the eval build leaves out (JS plugins beyond `init`, silent haptics, partial cancellation,
memory that only grows) is listed under "Known gaps" in `docs/research/rust-plugins-web-eval.md`.

Signatures run with no signer: neither `WebCrypto` nor a `PersonalKeyStore` is wired into this
app's hosts, so the signatures rung is dropped from its config entirely. Clicking Insert →
Signature draws a mark, but it is never sealed.

## Tests

`pnpm --filter @embedpdf/core-wasm test:wasm` runs the shim's vitest suite against the Node build
(`packages/core-wasm/wasm-node/`, from `bash crates/epdf-web/scripts/build.sh`). It is not named
`test`, so the repo's `turbo run test` skips it on a checkout with no Rust build.

The `crates/epdf-web` tests are `wasm-bindgen-test`s and run under Node with
`wasm-bindgen-test-runner` from `wasm-bindgen-cli` 0.2.127:

```sh
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test -p epdf-web --target wasm32-unknown-unknown -j 4
```

On the host target the same crate compiles to 0 tests, so `cargo test -p epdf-web` passing there
says nothing.
