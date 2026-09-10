# Web viewer end-to-end gate

Phase 0 baseline for [the port gate](../../multiplatform-port-order.md#7-web-viewer-end-to-end-gate): F1–F5 against the shipping TypeScript backend. The dedicated `examples/viewer-react/e2e.html` entry mounts the real React wrapper, built-in viewer, and PDFium worker. It does not mock the engine or invoke capabilities to perform user actions.

```sh
pnpm install --frozen-lockfile
pnpm --filter @embedpdf/engine-runtime fetch:payload
pnpm exec turbo run build --filter=@embedpdf/viewer-react...
pnpm --filter @embedpdf/tooling-viewer-e2e exec playwright install chromium firefox webkit
pnpm test:viewer:e2e
```

Use `--project=chromium-typescript` to run one browser. The runner owns port 5240 (5241 for packed fixtures) and refuses to reuse an unrelated server. Chromium and Firefox run on pull requests; WebKit runs weekly. Failures retain Playwright traces and screenshots for diagnosis; screenshots are not acceptance gates. There are no retries, skipped required flows, or arbitrary sleeps.

The fixture is a deterministic, hand-authored 12-page PDF. Regenerate it with `node tooling/viewer-e2e/fixtures/generate.mjs`. Each 600 × 400 point page has Courier text at known positions, one `needle` match, a numbered boundary line, and a red square. Text and pixels are authored independently of the engine under test. The suite checks:

- F1: open status, UI page count, and a known red pixel (a decoded blank image fails).
- F2: late-page wheel scrolling, virtualization, visible tile coverage and completion, and no duplicate render request for held tiles after a small pan.
- F3: wheel zoom in/out, page geometry, and tile coverage at the enlarged scale.
- F4: mouse selection within a line and across pages; exact text through the viewer's native copy-event handler. The test reads the event's `clipboardData`, avoiding platform clipboard permissions without replacing the handler.
- F5: exact search count, forward/backward navigation through all matches, UI page number and the active on-page highlight.

The read-only `window.viewerBaseline` bridge uses the public tokens exported by the same viewer artifact (token identity matters). Worker instrumentation records actual render requests and completions; it forwards messages and transfer lists unchanged. It does not supply render results.

`VIEWER_E2E_BACKENDS=typescript` is explicit at document open. Unknown backends, including `rust`, fail configuration and the example entry. When the Rust open selector lands, wire that actual selector here and add it to every job's backend list; do not relabel TypeScript execution as Rust. F6–F9 remain Phase 2/3 work; F10–F14 remain Phase 4 work. This baseline does not satisfy the final Rust default-switch gate.

## Packed artifacts

The release consume gate sets `VIEWER_E2E=1`, copies this same React entry into its isolated `vite-app` fixture, and runs all five flows in Chromium against the production Vite build after installing the packed tarballs. No workspace alias is used. To run it locally after building all publishable packages and runtime payloads:

```sh
VIEWER_E2E=1 pnpm verify:consume
```

`VIEWER_E2E_PACKED_DIR` is set by the consume runner; it points the Playwright web server at that installed fixture. Browser failures fail the consume gate and retain the fixture for inspection.
