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

## Local performance measurement

Performance measurement is opt-in and local only; CI and ordinary `test:viewer:e2e` runs do not enable it. After the build/browser setup above:

```sh
# Five fresh-context repetitions of each flow in one browser, serially.
VIEWER_E2E_PERF_OUTPUT="$PWD/tooling/viewer-e2e/performance-results/before.json" \
  pnpm test:viewer:perf --project=chromium-typescript
# Make the port change, rebuild the affected packages, then repeat with after.json.
node tooling/viewer-e2e/compare-performance.mjs \
  tooling/viewer-e2e/performance-results/before.json \
  tooling/viewer-e2e/performance-results/after.json
```

Without `VIEWER_E2E_PERF_OUTPUT`, each run writes a timestamped JSON report and adjacent `.json.md` summary under `performance-results/` (gitignored). Explicit output paths are resolved from the tooling package's working directory; use absolute paths as above. `--repeat-each=10` overrides the five-repeat default. Browser/backend projects and `VIEWER_E2E_PACKED_DIR` work as in the correctness suite. Tracing and failure screenshots are disabled in performance mode to reduce recording overhead; all correctness assertions still run.

Metrics are milliseconds, with raw samples and per-flow/browser min, median, nearest-rank p95, and max:

- `navigationToFirstPageFrameMs`: browser navigation time origin to the animation frame after the first visible page image is decoded. Includes app load and engine startup. This measures a frame opportunity, not a compositor presentation timestamp or completion of all sharp tiles. The F1 pixel assertion still verifies content separately.
- `flowWallMs`: Node's monotonic clock from the end of viewer setup to entry into teardown. Includes the entire flow's Playwright input, assertions, polling, and IPC. It is an end-to-end scenario cost, not isolated engine latency. F1's startup is measured separately by the navigation metric.
- `renderRoundTripMedianMs` / `renderRoundTripP95Ms`: browser-clock render dispatch to worker response, including queueing, transport and worker work. Successful render requests across startup and the flow contribute; rejected requests remain in raw data. These are per-run request distributions, then summarized across repeats—not a sum of overlapping work or paint latency.

The report records commit/worktree status, fixture digest, browser and Playwright versions, CPU/OS, viewport, repeat index, delivery mode, and test status. Failed/retried samples are preserved but excluded from summaries. Comparison rejects failed reports and mismatched environments, browsers, fixtures, delivery modes, or flow sets. Positive median changes mean slower; there are no thresholds or CI gates.

Use the same machine, browser, power settings and background load before/after. These small-fixture results are not a large-document benchmark. Contexts are fresh, but the browser process may be reused and OS/Vite caches persist; workspace mode also includes Vite development overhead. The server restarts for each command, so inspect raw repeat indices for first-run compilation or warmup effects. Rebuild between implementations and use matching repeat counts. With five samples, p95 is simply the maximum; small deltas need more evidence.
