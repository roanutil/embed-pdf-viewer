# wasm32-eh browser check

Phase 1's exit gate asks for the "real linked EH module tested in release
Chrome, Firefox, Safari". Section 14 says why it is worth its own harness:

> The historical audit counted 441 `try`, 367 `catch_all`, 285 `rethrow`, 0
> `try_table`. Recorded passes are Chrome 152.0.7977.83 and Playwright Firefox
> 132.0 Nightly. A Nightly pass says nothing about release Firefox, and Safari
> is untested. There is no fallback if a release browser we support refuses the
> module.

The page under test is deliberately plain: no bundler, no framework, no viewer.
It loads `embedpdf.browser.js`, opens `report.pdf`, counts pages and characters,
renders a bitmap and checks a wrong password is refused with
`FPDF_ERR_PASSWORD`. Two of those are there for the EH build specifically. The
render reaches `fxcodec`, whose libpng and libjpeg error handling is the
`setjmp`/`longjmp` this build lowers through wasm EH; the wrong password is a
C++ throw and catch inside PDFium. When the module was linked with the Rust
core, it also reads page geometry through it and checks the run and glyph counts
against the frozen corpus vectors.

## Running it

Build a module first, per `tooling/engine-conformance/README.md`, then:

```bash
EH_MODULE_DIR=/tmp/eh-rust pnpm --filter @embedpdf/tooling-wasm-eh-check run check
```

Safari needs its own runner and one manual step, once per machine, because it
grants remote control of a browser:

```
Safari > Settings > Advanced > Show features for web developers
Safari > Settings > Developer > Allow remote automation
```

Then quit and reopen Safari. The setting is written immediately but only takes
effect from Safari's next launch, and until then `safaridriver` reports a
connection timeout that says nothing about the setting. `safari.mjs` now names
that case rather than letting you go looking at a checkbox that is already on.

```bash
EH_MODULE_DIR=/tmp/eh-rust node serve.mjs &
node safari.mjs
```

## Results, 2026-09-23

Module: the `wasm32-eh` build at fork commit `7f0fa5dbe` with the Rust core
linked in, sha256 `35eb2b3e2ba3…`, 272 `try_table` / 200 `throw_ref` / 202
`catch_all_ref` and no legacy `try` or `rethrow`.

| browser                  | version                 | module | Rust core |
| ------------------------ | ----------------------- | ------ | --------- |
| Chrome, release channel  | 154.0.8037.58           | pass   | pass      |
| Firefox, release channel | 156.0.1                 | pass   | pass      |
| Safari                   | 27.0 (22625.1.29.11.27) | pass   | pass      |
| Playwright Chromium      | 153.0.8010.12           | pass   | pass      |
| Playwright Firefox       | 155.0                   | pass   | pass      |
| Playwright WebKit        | 26.6                    | pass   | pass      |

Every version in that table is printed by the run itself. The `records the
browser it ran in` test logs the build each project drove, because "Firefox
passes" is worth nothing without the string next to it, and a README is the
easiest place for that string to go stale.

## What this does and does not settle

All three release browsers pass, so Phase 1's browser floor is met and the
Section 14 risk it was raised against is closed. The `firefox-release` project
drives the Firefox in `/Applications` over WebDriver BiDi through Playwright's
`moz-firefox` channel, not Playwright's patched build.

The three Playwright projects are engine builds, and a pass there is not a pass
in the browser a user has. They are kept because they are cheap and they catch a
module no engine will load, not because they are evidence for the floor. The
plan's original Firefox evidence was a Nightly recorded as "Firefox passes",
which is the mistake this split exists to prevent.

What none of this covers is Windows, Linux, or any mobile browser. The floor
these results support is macOS desktop.
