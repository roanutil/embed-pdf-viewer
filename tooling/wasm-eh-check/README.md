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

```bash
EH_MODULE_DIR=/tmp/eh-rust node serve.mjs &
node safari.mjs
```

## Results, 2026-09-23

Module: the `wasm32-eh` build at fork commit `7f0fa5dbe` with the Rust core
linked in, sha256 `35eb2b3e2ba3…`, 272 `try_table` / 200 `throw_ref` / 202
`catch_all_ref` and no legacy `try` or `rethrow`.

| browser                 | version                 | module  | Rust core |
| ----------------------- | ----------------------- | ------- | --------- |
| Chrome, release channel | 153.0.8010.53           | pass    | pass      |
| Playwright Chromium     | 153.0.8010.12           | pass    | pass      |
| Playwright Firefox      | 155.0                   | pass    | pass      |
| Playwright WebKit       | 26.6                    | pass    | pass      |
| Safari                  | 27.0 (22625.1.29.11.27) | not run | not run   |

## What this does and does not settle

Chrome is a release-channel pass and counts toward the floor.

The three Playwright projects are Playwright's own engine builds, and a pass
there is not a pass in the browser a user has. That is the exact mistake the
plan warns against: its Firefox evidence was a Nightly, and recording it as
"Firefox passes" is how a floor gets announced that nobody verified. Playwright
Firefox 155.0 and WebKit 26.6 passing says the module is not rejected by those
engines, which is genuinely useful and is not the same claim.

Safari is still unverified. `safaridriver` is installed and `safari.mjs` is
ready, but Safari refuses WebDriver sessions until somebody enables remote
automation, and a script should not do that to a person's browser. Until it is
run, Section 14's risk stands as written: Safari is untested.

Firefox release is not installed on the machine these results came from, so the
release-Firefox half of the gate is also open.
