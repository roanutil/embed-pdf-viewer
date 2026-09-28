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
  this app fetches or builds them for you. `pnpm setup` (below) checks for them and prints exact
  commands if they are missing.

## Setup

Run once per checkout, from this directory:

1. `pnpm install` at the repo root, if you have not already.
2. `pnpm setup`. This builds `packages/engine/main/workers/` and
   `packages/default-stamps/locales/`, two gitignored outputs this app's chrome needs that are
   not built by anything else here. It takes about 20 seconds, most of it the engine's own
   dependency build. If the PDFium runtime artifacts above are missing, it fails immediately with
   the commands to fetch or build them instead of leaving that failure for the browser to surface
   later as a missing-file error against a running dev server.

`setup` is not chained onto `dev`: rebuilding the engine costs those same ~20 seconds every time,
and `dev` should stay fast once `setup` has run. Re-run `pnpm setup` after pulling changes to
`packages/engine/main`, `packages/default-stamps`, or `packages/engine/runtime`.

## Running it

`pnpm dev` builds the Rust kernel (`pnpm --filter @embedpdf/core-wasm build:wasm`) and starts
Vite. Open the printed URL. The chrome loads, a sample PDF opens, and the console shows no
errors.

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

Signatures run with no signer: neither `WebCrypto` nor a `PersonalKeyStore` is wired into this
app's hosts, so the signatures rung is dropped from its config entirely. Clicking Insert →
Signature draws a mark, but it is never sealed.
