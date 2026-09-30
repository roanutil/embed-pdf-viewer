# Viewer perf results, 2026-09-30

The fixed Rust kernel is now close to `main` on everything a user would notice. It isn't at parity
on startup or CPU. Scrolling a 400-page document has no long tasks and `main`'s 8.3 ms median
frame, drag-select settles in the same time as `main`, and the soak heap no longer grows. What's
left: opening is 1.5 to 1.75x `main` (51 to 134 ms later), jump and zoom settle 33 and 49 ms
later, and main-thread task time is still 2 to 8x `main`, with script time around 20x on a large
document's open and scroll. That CPU hides in idle time on this machine and would show on a slower
one.

`plugins-fix-zoom` is `research/rust-kernel-state-cache` at `8cef92c76`: the plugins branch tip
(`bef223d9d`) plus six commits. `e0f33b1c4` caches the kernel's state JSON per change and notifies
subscribers once per call. `2b07fb371` shares the page registry through an `Rc` and makes the
measurement plugin's per-page reconcile linear. `2ed312446` fixes the heap leak: a settled promise
now drops its abort closure, which had pinned each engine call's task and answer. `e13b09f75` and
`be26ed588` trim per-render work in the toolbar, the wire and the tile keys. `8cef92c76` renders
baked annotation appearances at the same HIGH engine priority as page tiles; at MEDIUM they queued
behind every tile after a zoom. `FINDINGS.md` has the analysis behind each one. None of this is
merged into `research/multi-platform-rust-core-impl-plugins` or pushed yet.

Against `plugins-lto`, the fair baseline because `plugins-fix-zoom` builds its kernel with the
same LTO recipe, the fixes cut opening `large.pdf` from 1239 to 167 ms (7.4x). A 60-wheel scroll
goes from 8.4 s of long tasks to none, and jump settle from 817 to 114 ms. `plugins-lto` itself is
about 10% faster than `plugins` on kernel-bound rows. That's LTO, which Cargo skips for the branch
as committed because `crates/epdf-web/Cargo.toml` declares `crate-type = ["cdylib", "rlib"]`.

The impl branch hasn't moved: `impl-ts` and `impl-rust` show no detectable difference from `main`
on any row. The Rust `pages.geometry` reader can't show in this viewer, because geometry is about
2% of the engine worker's time during a selection. The Rust core costs 3.4 MiB of engine wasm
(0.7 MiB brotli) on both research branches, and the plugins kernel adds 11.7 MiB (1.9 MiB brotli)
while shipping 0.75 MiB less JavaScript.

This is one `desktop` run: 10 rounds on an Apple M4 Max, on AC power, harness at `61e6fd907`. It
took 84.5 min and recorded 630 samples, none failed. The A/A pair passed 38 of 39 rows;
`search / searchMs` failed it, so that row isn't judged. The `slow` profile wasn't re-run for
these arms. The earlier five-arm results, `slow` included, are in this file's history at
`90e783cab`.

## All arms, median

Times are ms. The last column is `plugins-fix-zoom` against `main` from the report below: a ratio
for time and CPU rows, a difference in ms for settle, jank and long-task rows. "same" is no
detectable difference; "unreliable" means the row failed the A/A check. Memory rows show the plain
ratio of medians, which the report doesn't judge.

| Row | `main` | `impl-ts` | `impl-rust` | `plugins` | `plugins-lto` | `plugins-fix-zoom` | fix-zoom vs main |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| open(ebook) / readyMs | 44 | 43 | 45 | 68 | 69 | 68 | 1.545 slower |
| open(ebook) / openedMs | 96 | 97 | 99 | 169 | 168 | 147 | 1.534 slower |
| open(ebook) / firstPaintMs | 174 | 181 | 181 | 324 | 321 | 253 | 1.448 slower |
| open(ebook) / settledMs | 266 | 273 | 272 | 526 | 486 | 294 | 1.106 slower |
| open(ebook) / mainTaskMs | 93 | 93 | 93 | 407 | 367 | 215 | 2.316 slower |
| open(ebook) / mainScriptMs | 16 | 16 | 16 | 219 | 190 | 96 | 6.094 slower |
| open(ebook) / heapAfterOpenBytes | 34.34 MiB | 35.02 MiB | 34.97 MiB | 43.35 MiB | 43.28 MiB | 41.55 MiB | 1.21x |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB | 23.00 MiB | 22.95 MiB | 23.03 MiB | 23.03 MiB | 23.02 MiB | 1.03x |
| open(large) / readyMs | 43 | 43 | 44 | 68 | 69 | 68 | 1.593 slower |
| open(large) / openedMs | 110 | 113 | 114 | 1346 | 1239 | 167 | 1.527 slower |
| open(large) / firstPaintMs | 182 | 184 | 183 | 2213 | 2006 | 299 | 1.643 slower |
| open(large) / settledMs | 177 | 180 | 180 | 2287 | 2056 | 311 | 1.754 slower |
| open(large) / mainTaskMs | 205 | 205 | 205 | 2598 | 2324 | 505 | 2.464 slower |
| open(large) / mainScriptMs | 18 | 18 | 18 | 2134 | 1908 | 354 | 19.611 slower |
| open(large) / heapAfterOpenBytes | 48.15 MiB | 48.84 MiB | 48.81 MiB | 58.10 MiB | 58.01 MiB | 57.28 MiB | 1.19x |
| open(large) / workerHeapAfterOpenBytes | 22.26 MiB | 22.99 MiB | 22.96 MiB | 22.94 MiB | 22.94 MiB | 22.95 MiB | 1.03x |
| scroll / settleMs | 0.0 | 0.0 | 0.0 | 140 | 161 | 0.0 | +0.0 same |
| scroll / wallMs | 2005 | 2003 | 2005 | 16079 | 14221 | 2619 | 1.306 slower |
| scroll / frameP50Ms | 8.3 | 8.3 | 8.3 | 42 | 37 | 8.3 | 1.000 same |
| scroll / frameP95Ms | 8.4 | 8.6 | 8.4 | 207 | 191 | 9.7 | 1.149 slower |
| scroll / jankFrames | 0 | 0 | 0 | 99 | 94.5 | 0 | +0 same |
| scroll / longTasks | 0 | 0 | 0 | 64.5 | 63.5 | 0 | +0 same |
| scroll / longTaskMs | 0.0 | 0.0 | 0.0 | 9613 | 8404 | 0.0 | +0.0 same |
| scroll / mainTaskMs | 346 | 341 | 335 | 16483 | 14634 | 1354 | 3.912 slower |
| scroll / mainScriptMs | 46 | 46 | 46 | 9095 | 8067 | 910 | 19.641 slower |
| jump / settleMs | 81 | 82 | 81 | 992 | 817 | 114 | +32.7 slower |
| jump / mainTaskMs | 48 | 48 | 47 | 1334 | 1163 | 374 | 7.785 slower |
| jump / mainScriptMs | 12 | 12 | 12 | 878 | 770 | 324 | 26.842 slower |
| zoom / settleMs | 306 | 303 | 300 | 537 | 511 | 355 | +48.7 slower |
| zoom / zoom2SettleMs | 111 | 114 | 115 | 246 | 235 | 139 | +28.0 slower |
| zoom / zoom4SettleMs | 196 | 188 | 185 | 290 | 276 | 214 | +17.8 slower |
| zoom / backTo1SettleMs | 62 | 58 | 58 | 107 | 99 | 77 | +14.8 slower |
| zoom / mainTaskMs | 143 | 140 | 140 | 901 | 798 | 287 | 2.005 slower |
| zoom / mainScriptMs | 22 | 20 | 20 | 496 | 426 | 134 | 6.059 slower |
| select / settleMs | 143 | 141 | 142 | 294 | 291 | 145 | +2.7 same |
| select / mainTaskMs | 41 | 40 | 41 | 237 | 221 | 78 | 1.874 slower |
| select / mainScriptMs | 9.7 | 9.3 | 9.4 | 187 | 173 | 42 | 4.359 slower |
| search / searchMs | 51 | 46 | 48 | 76 | 73 | 61 | 1.192 unreliable |
| search / mainTaskMs | 4.0 | 4.0 | 3.7 | 36 | 34 | 20 | 5.047 slower |
| search / mainScriptMs | 0.1 | 0.1 | 0.1 | 19 | 17 | 8.5 | 90.904 slower |
| annotate / settleMs | 153 | 153 | 153 | 330 | 326 | 165 | +12.3 slower |
| annotate / mainTaskMs | 50 | 49 | 49 | 282 | 260 | 93 | 1.864 slower |
| annotate / mainScriptMs | 10 | 10 | 10 | 210 | 192 | 48 | 4.621 slower |
| soak / heapCycle5Bytes | 37.42 MiB | 38.18 MiB | 38.13 MiB | 49.05 MiB | 48.96 MiB | 44.36 MiB | 1.19x |
| soak / heapCycle30Bytes | 38.27 MiB | 39.04 MiB | 39.00 MiB | 63.11 MiB | 63.02 MiB | 45.05 MiB | 1.18x |
| soak / heapSoakSlopeBytes | 0.03 MiB | 0.03 MiB | 0.03 MiB | 0.56 MiB | 0.56 MiB | 0.03 MiB |  |

| Size | `main` | `impl-ts` | `impl-rust` | `plugins` | `plugins-lto` | `plugins-fix-zoom` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| engineWasmBytes | 7.19 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB |
| engineWasmBrotliBytes | 2.15 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB |
| kernelWasmBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 10.78 MiB | 11.68 MiB | 11.68 MiB |
| kernelWasmBrotliBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 1.77 MiB | 1.88 MiB | 1.88 MiB |
| jsBytes | 3.37 MiB | 3.35 MiB | 3.35 MiB | 2.62 MiB | 2.62 MiB | 2.62 MiB |
| jsBrotliBytes | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.80 MiB | 0.80 MiB | 0.80 MiB |

## Viewer perf comparison: 2026-09-30T01-07-37-279Z-mac-desktop

10 rounds, profile `desktop` (CPU throttle 1x).

`workerTaskMs` is n/a: Chromium has no Performance domain on dedicated workers.

### Environment

| Field | Value |
| --- | --- |
| Harness commit | 61e6fd907dc96d80a93a542d82d0a4c38233f184 |
| Chromium / Playwright | 140.0.7339.16 / 1.55.0 |
| Node | v24.20.0 |
| OS | Darwin 27.0.0 arm64 |
| CPU | Apple M4 Max, 16 cores |
| Memory | 64 GiB |
| Power | AC Power |
| Viewport | 1280x800 @2x |
| Fixtures | ebook 2e539f62b5fa, large 10c8668fe55c |

| Arm | Ref | SHA | Payload | Engine wasm sha256 | Kernel wasm sha256 |
| --- | --- | --- | --- | --- | --- |
| main | main | 2516e2786 | fetched | 98c2368a774d |  |
| main-aa | main | 2516e2786 | fetched | 98c2368a774d |  |
| impl-ts | research/multi-platform-rust-core-impl | c8085bd3c | built (fork d045f389d, wasm32-eh) | 0f7407599d93 |  |
| impl-rust | research/multi-platform-rust-core-impl | c8085bd3c | built (fork d045f389d, wasm32-eh) | 0f7407599d93 |  |
| plugins | research/multi-platform-rust-core-impl-plugins | bef223d9d | built (fork d045f389d, wasm32-eh) | 0f7407599d93 | 992f38174937 |
| plugins-lto | research/multi-platform-rust-core-impl-plugins | bef223d9d | built (fork d045f389d, wasm32-eh) | 0f7407599d93 | ce0b35ed118c |
| plugins-fix-zoom | research/rust-kernel-state-cache | 8cef92c76 | built (fork d045f389d, wasm32-eh) | 0f7407599d93 | df47d777780e |

### Gate

- `main`: pass
- `main-aa`: pass
- `impl-ts`: pass
- `impl-rust`: pass
- `plugins`: pass
- `plugins-lto`: pass
- `plugins-fix-zoom`: pass

### Sizes

| Metric | main | main-aa | impl-ts | impl-rust | plugins | plugins-lto | plugins-fix-zoom |
| --- | --- | --- | --- | --- | --- | --- | --- |
| engineWasmBytes | 7.19 MiB | 7.19 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB |
| engineWasmBrotliBytes | 2.15 MiB | 2.15 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB |
| kernelWasmBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 10.78 MiB | 11.68 MiB | 11.68 MiB |
| kernelWasmBrotliBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 1.77 MiB | 1.88 MiB | 1.88 MiB |
| jsBytes | 3.37 MiB | 3.37 MiB | 3.35 MiB | 3.35 MiB | 2.62 MiB | 2.62 MiB | 2.62 MiB |
| jsBrotliBytes | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.80 MiB | 0.80 MiB | 0.80 MiB |

### A/A: `main` vs `main-aa`

A metric passes when its interval includes the null value or its point estimate is within 3% of it. Its interval then sets the noise band every other pair must clear.

38 of 39 rows pass.

| Row | main median [p95] | main-aa median [p95] | ratio or diff [95% CI] | A/A |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 44.3 [45.6] | 43.4 [44.7] | 0.979 [0.947, 1.038] | pass |
| open(ebook) / openedMs | 95.7 [98.7] | 95.6 [97.4] | 0.999 [0.980, 1.025] | pass |
| open(ebook) / firstPaintMs | 174.4 [182.3] | 174.0 [180.5] | 0.998 [0.972, 1.023] | pass |
| open(ebook) / settledMs | 265.9 [270.4] | 265.0 [268.4] | 0.997 [0.984, 1.009] | pass |
| open(ebook) / mainTaskMs | 92.8 [94.5] | 92.7 [94.4] | 0.999 [0.981, 1.011] | pass |
| open(ebook) / mainScriptMs | 15.7 [16.1] | 15.6 [16.2] | 0.992 [0.972, 1.020] | pass |
| open(large) / readyMs | 42.7 [45.5] | 42.9 [44.7] | 1.003 [0.956, 1.024] | pass |
| open(large) / openedMs | 109.5 [111.8] | 109.7 [111.6] | 1.001 [0.984, 1.015] | pass |
| open(large) / firstPaintMs | 181.7 [185.1] | 181.0 [185.6] | 0.996 [0.972, 1.036] | pass |
| open(large) / settledMs | 177.4 [181.3] | 177.6 [181.3] | 1.001 [0.973, 1.038] | pass |
| open(large) / mainTaskMs | 205.1 [207.5] | 205.0 [208.2] | 1.000 [0.997, 1.010] | pass |
| open(large) / mainScriptMs | 18.1 [18.4] | 18.0 [18.5] | 0.997 [0.982, 1.023] | pass |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / wallMs | 2004.9 [2016.9] | 2005.4 [2022.4] | 1.000 [0.996, 1.007] | pass |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.001] | pass |
| scroll / frameP95Ms | 8.4 [8.8] | 8.4 [9.1] | 1.000 [0.978, 1.066] | pass |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / mainTaskMs | 346.2 [366.6] | 350.0 [372.3] | 1.011 [0.941, 1.076] | pass |
| scroll / mainScriptMs | 46.4 [47.8] | 46.7 [48.1] | 1.008 [0.975, 1.028] | pass |
| jump / settleMs | 81.3 [83.1] | 81.5 [83.3] | +0.2 [-4.6, +5.3] | pass |
| jump / mainTaskMs | 48.1 [50.2] | 47.6 [49.9] | 0.990 [0.966, 1.039] | pass |
| jump / mainScriptMs | 12.1 [13.5] | 11.9 [13.9] | 0.986 [0.939, 1.029] | pass |
| zoom / settleMs | 305.9 [319.4] | 302.6 [315.5] | -3.4 [-14.7, +17.2] | pass |
| zoom / zoom2SettleMs | 111.3 [115.7] | 111.1 [114.1] | -0.2 [-8.3, +7.3] | pass |
| zoom / zoom4SettleMs | 195.8 [206.2] | 196.8 [203.9] | +1.0 [-9.2, +17.3] | pass |
| zoom / backTo1SettleMs | 62.0 [72.1] | 59.7 [72.6] | -2.3 [-7.0, +4.8] | pass |
| zoom / mainTaskMs | 143.1 [145.0] | 142.5 [145.7] | 0.996 [0.961, 1.036] | pass |
| zoom / mainScriptMs | 22.1 [23.4] | 21.5 [23.4] | 0.971 [0.884, 1.118] | pass |
| select / settleMs | 142.5 [148.1] | 143.3 [148.5] | +0.8 [-5.3, +5.3] | pass |
| select / mainTaskMs | 41.5 [43.1] | 42.2 [46.5] | 1.019 [0.947, 1.107] | pass |
| select / mainScriptMs | 9.7 [10.1] | 10.2 [11.0] | 1.058 [0.962, 1.107] | pass |
| search / searchMs | 51.5 [53.1] | 46.0 [50.9] | 0.894 [0.845, 0.969] | A/A failed |
| search / mainTaskMs | 4.0 [4.6] | 3.8 [4.2] | 0.958 [0.868, 1.009] | pass |
| search / mainScriptMs | 0.1 [0.1] | 0.1 [0.1] | 1.037 [0.860, 1.078] | pass |
| annotate / settleMs | 152.6 [154.3] | 152.9 [162.5] | +0.3 [-1.2, +3.3] | pass |
| annotate / mainTaskMs | 50.0 [52.1] | 51.7 [55.1] | 1.034 [0.992, 1.082] | pass |
| annotate / mainScriptMs | 10.3 [10.9] | 10.4 [11.1] | 1.002 [0.963, 1.051] | pass |

### `main` vs `impl-ts`

| Row | main median [p95] | impl-ts median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 44.3 [45.6] | 42.9 [44.9] | 0.968 [0.945, 1.036] | no detectable difference |
| open(ebook) / openedMs | 95.7 [98.7] | 97.5 [100.2] | 1.018 [1.000, 1.041] | no detectable difference |
| open(ebook) / firstPaintMs | 174.4 [182.3] | 180.9 [184.1] | 1.037 [1.009, 1.059] | slower |
| open(ebook) / settledMs | 265.9 [270.4] | 272.6 [274.8] | 1.025 [1.015, 1.035] | no detectable difference |
| open(ebook) / mainTaskMs | 92.8 [94.5] | 93.1 [94.8] | 1.003 [0.985, 1.016] | no detectable difference |
| open(ebook) / mainScriptMs | 15.7 [16.1] | 15.9 [16.3] | 1.013 [0.986, 1.029] | no detectable difference |
| open(large) / readyMs | 42.7 [45.5] | 43.0 [45.3] | 1.007 [0.961, 1.048] | no detectable difference |
| open(large) / openedMs | 109.5 [111.8] | 113.4 [115.1] | 1.035 [1.015, 1.044] | slower |
| open(large) / firstPaintMs | 181.7 [185.1] | 184.4 [188.0] | 1.015 [1.002, 1.048] | no detectable difference |
| open(large) / settledMs | 177.4 [181.3] | 180.3 [183.8] | 1.016 [1.004, 1.051] | no detectable difference |
| open(large) / mainTaskMs | 205.1 [207.5] | 205.4 [207.7] | 1.002 [0.998, 1.005] | no detectable difference |
| open(large) / mainScriptMs | 18.1 [18.4] | 17.9 [18.3] | 0.992 [0.978, 1.014] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2004.9 [2016.9] | 2002.7 [2021.3] | 0.999 [0.995, 1.004] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [0.999, 1.001] | no detectable difference |
| scroll / frameP95Ms | 8.4 [8.8] | 8.6 [8.9] | 1.018 [0.982, 1.047] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 346.2 [366.6] | 340.6 [360.0] | 0.984 [0.936, 1.034] | no detectable difference |
| scroll / mainScriptMs | 46.4 [47.8] | 46.1 [47.6] | 0.995 [0.971, 1.020] | no detectable difference |
| jump / settleMs | 81.3 [83.1] | 82.4 [87.3] | +1.1 [-0.4, +6.4] | no detectable difference |
| jump / mainTaskMs | 48.1 [50.2] | 48.1 [52.4] | 1.000 [0.945, 1.048] | no detectable difference |
| jump / mainScriptMs | 12.1 [13.5] | 12.1 [15.2] | 0.999 [0.937, 1.115] | no detectable difference |
| zoom / settleMs | 305.9 [319.4] | 303.4 [322.0] | -2.5 [-16.9, +19.1] | no detectable difference |
| zoom / zoom2SettleMs | 111.3 [115.7] | 114.2 [120.1] | +2.9 [-0.3, +11.4] | no detectable difference |
| zoom / zoom4SettleMs | 195.8 [206.2] | 188.0 [204.7] | -7.8 [-17.5, +10.8] | no detectable difference |
| zoom / backTo1SettleMs | 62.0 [72.1] | 58.3 [65.2] | -3.8 [-9.0, +2.5] | no detectable difference |
| zoom / mainTaskMs | 143.1 [145.0] | 140.2 [143.7] | 0.980 [0.969, 1.021] | no detectable difference |
| zoom / mainScriptMs | 22.1 [23.4] | 20.0 [23.0] | 0.906 [0.874, 1.021] | no detectable difference |
| select / settleMs | 142.5 [148.1] | 141.3 [147.4] | -1.2 [-5.9, +4.9] | no detectable difference |
| select / mainTaskMs | 41.5 [43.1] | 39.6 [43.9] | 0.956 [0.932, 1.035] | no detectable difference |
| select / mainScriptMs | 9.7 [10.1] | 9.3 [10.3] | 0.966 [0.932, 1.016] | no detectable difference |
| search / searchMs | 51.5 [53.1] | 46.5 [51.7] | 0.903 [0.843, 0.985] | unreliable (A/A failed) |
| search / mainTaskMs | 4.0 [4.6] | 4.0 [4.6] | 1.005 [0.903, 1.083] | no detectable difference |
| search / mainScriptMs | 0.1 [0.1] | 0.1 [0.1] | 0.995 [0.918, 1.065] | no detectable difference |
| annotate / settleMs | 152.6 [154.3] | 153.3 [153.8] | +0.7 [-1.1, +2.2] | no detectable difference |
| annotate / mainTaskMs | 50.0 [52.1] | 49.3 [51.8] | 0.985 [0.946, 1.033] | no detectable difference |
| annotate / mainScriptMs | 10.3 [10.9] | 10.1 [10.6] | 0.979 [0.894, 1.019] | no detectable difference |

### `impl-ts` vs `impl-rust`

| Row | impl-ts median [p95] | impl-rust median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 42.9 [44.9] | 44.7 [45.8] | 1.043 [0.996, 1.063] | no detectable difference |
| open(ebook) / openedMs | 97.5 [100.2] | 99.4 [102.1] | 1.020 [1.003, 1.035] | no detectable difference |
| open(ebook) / firstPaintMs | 180.9 [184.1] | 180.8 [182.4] | 0.999 [0.985, 1.015] | no detectable difference |
| open(ebook) / settledMs | 272.6 [274.8] | 271.6 [278.5] | 0.996 [0.991, 1.005] | no detectable difference |
| open(ebook) / mainTaskMs | 93.1 [94.8] | 93.0 [96.9] | 0.999 [0.986, 1.013] | no detectable difference |
| open(ebook) / mainScriptMs | 15.9 [16.3] | 15.7 [16.5] | 0.989 [0.968, 1.020] | no detectable difference |
| open(large) / readyMs | 43.0 [45.3] | 44.2 [45.2] | 1.028 [0.974, 1.058] | no detectable difference |
| open(large) / openedMs | 113.4 [115.1] | 113.6 [116.4] | 1.002 [0.990, 1.022] | no detectable difference |
| open(large) / firstPaintMs | 184.4 [188.0] | 182.8 [189.2] | 0.991 [0.979, 1.018] | no detectable difference |
| open(large) / settledMs | 180.3 [183.8] | 179.9 [186.0] | 0.998 [0.977, 1.025] | no detectable difference |
| open(large) / mainTaskMs | 205.4 [207.7] | 205.0 [207.9] | 0.998 [0.993, 1.003] | no detectable difference |
| open(large) / mainScriptMs | 17.9 [18.3] | 17.8 [18.5] | 0.996 [0.974, 1.024] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2002.7 [2021.3] | 2005.3 [2018.1] | 1.001 [0.996, 1.007] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.001] | no detectable difference |
| scroll / frameP95Ms | 8.6 [8.9] | 8.4 [9.2] | 0.982 [0.953, 1.038] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 340.6 [360.0] | 335.2 [341.3] | 0.984 [0.944, 1.009] | no detectable difference |
| scroll / mainScriptMs | 46.1 [47.6] | 45.6 [46.8] | 0.988 [0.968, 1.015] | no detectable difference |
| jump / settleMs | 82.4 [87.3] | 80.8 [83.4] | -1.6 [-7.4, +0.1] | no detectable difference |
| jump / mainTaskMs | 48.1 [52.4] | 47.1 [51.4] | 0.979 [0.922, 1.052] | no detectable difference |
| jump / mainScriptMs | 12.1 [15.2] | 11.8 [12.5] | 0.980 [0.877, 1.034] | no detectable difference |
| zoom / settleMs | 303.4 [322.0] | 299.6 [319.3] | -3.8 [-19.8, +12.0] | no detectable difference |
| zoom / zoom2SettleMs | 114.2 [120.1] | 115.0 [120.1] | +0.8 [-4.6, +5.8] | no detectable difference |
| zoom / zoom4SettleMs | 188.0 [204.7] | 185.2 [201.1] | -2.8 [-16.7, +12.5] | no detectable difference |
| zoom / backTo1SettleMs | 58.3 [65.2] | 58.0 [71.8] | -0.3 [-5.5, +11.8] | no detectable difference |
| zoom / mainTaskMs | 140.2 [143.7] | 140.3 [146.0] | 1.000 [0.980, 1.027] | no detectable difference |
| zoom / mainScriptMs | 20.0 [23.0] | 20.2 [23.1] | 1.007 [0.938, 1.113] | no detectable difference |
| select / settleMs | 141.3 [147.4] | 141.6 [147.5] | +0.3 [-5.5, +1.7] | no detectable difference |
| select / mainTaskMs | 39.6 [43.9] | 40.7 [43.9] | 1.027 [0.980, 1.064] | no detectable difference |
| select / mainScriptMs | 9.3 [10.3] | 9.4 [10.2] | 1.011 [0.984, 1.070] | no detectable difference |
| search / searchMs | 46.5 [51.7] | 48.3 [55.1] | 1.040 [0.895, 1.142] | unreliable (A/A failed) |
| search / mainTaskMs | 4.0 [4.6] | 3.7 [4.5] | 0.932 [0.848, 1.054] | no detectable difference |
| search / mainScriptMs | 0.1 [0.1] | 0.1 [0.1] | 1.054 [0.930, 1.180] | no detectable difference |
| annotate / settleMs | 153.3 [153.8] | 153.0 [154.4] | -0.2 [-2.2, +1.2] | no detectable difference |
| annotate / mainTaskMs | 49.3 [51.8] | 49.5 [51.5] | 1.004 [0.963, 1.040] | no detectable difference |
| annotate / mainScriptMs | 10.1 [10.6] | 10.1 [10.7] | 1.000 [0.972, 1.090] | no detectable difference |

### `impl-ts` vs `plugins`

| Row | impl-ts median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 42.9 [44.9] | 67.7 [69.2] | 1.579 [1.511, 1.625] | slower |
| open(ebook) / openedMs | 97.5 [100.2] | 168.5 [170.2] | 1.729 [1.711, 1.750] | slower |
| open(ebook) / firstPaintMs | 180.9 [184.1] | 323.9 [334.6] | 1.790 [1.758, 1.841] | slower |
| open(ebook) / settledMs | 272.6 [274.8] | 526.1 [541.9] | 1.930 [1.846, 1.983] | slower |
| open(ebook) / mainTaskMs | 93.1 [94.8] | 406.6 [444.1] | 4.369 [4.243, 4.638] | slower |
| open(ebook) / mainScriptMs | 15.9 [16.3] | 219.4 [245.9] | 13.793 [13.260, 14.943] | slower |
| open(large) / readyMs | 43.0 [45.3] | 67.7 [70.0] | 1.574 [1.509, 1.620] | slower |
| open(large) / openedMs | 113.4 [115.1] | 1346.3 [1385.3] | 11.875 [11.786, 12.111] | slower |
| open(large) / firstPaintMs | 184.4 [188.0] | 2213.5 [2287.6] | 12.002 [11.861, 12.195] | slower |
| open(large) / settledMs | 180.3 [183.8] | 2287.3 [2340.0] | 12.687 [12.465, 12.839] | slower |
| open(large) / mainTaskMs | 205.4 [207.7] | 2598.4 [2630.1] | 12.649 [12.433, 12.764] | slower |
| open(large) / mainScriptMs | 17.9 [18.3] | 2133.6 [2156.6] | 119.025 [116.568, 120.756] | slower |
| scroll / settleMs | 0.0 [0.0] | 139.9 [365.9] | +139.9 [+93.5, +320.5] | slower |
| scroll / wallMs | 2002.7 [2021.3] | 16079.1 [16924.2] | 8.029 [7.853, 8.224] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 41.7 [50.0] | 5.000 [3.999, 5.569] | slower |
| scroll / frameP95Ms | 8.6 [8.9] | 207.3 [234.2] | 24.147 [22.874, 26.206] | slower |
| scroll / jankFrames | 0 [0] | 99 [101.65] | +99 [+98, +99.50] | slower |
| scroll / longTasks | 0 [0] | 64.50 [66.55] | +64.50 [+62.50, +65.50] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 9613.0 [9952.5] | +9613.0 [+9189.5, +9808.5] | slower |
| scroll / mainTaskMs | 340.6 [360.0] | 16483.5 [17383.0] | 48.399 [46.377, 50.062] | slower |
| scroll / mainScriptMs | 46.1 [47.6] | 9095.1 [9774.2] | 197.136 [190.587, 207.413] | slower |
| jump / settleMs | 82.4 [87.3] | 992.3 [1097.1] | +909.8 [+768.6, +1000.9] | slower |
| jump / mainTaskMs | 48.1 [52.4] | 1334.4 [1463.4] | 27.758 [25.017, 30.731] | slower |
| jump / mainScriptMs | 12.1 [15.2] | 878.0 [985.7] | 72.862 [61.577, 81.603] | slower |
| zoom / settleMs | 303.4 [322.0] | 537.2 [554.2] | +233.8 [+212.2, +250.5] | slower |
| zoom / zoom2SettleMs | 114.2 [120.1] | 245.6 [254.6] | +131.4 [+125.8, +138.3] | slower |
| zoom / zoom4SettleMs | 188.0 [204.7] | 289.9 [302.7] | +101.9 [+84.7, +115.1] | slower |
| zoom / backTo1SettleMs | 58.3 [65.2] | 106.7 [108.7] | +48.5 [+46.1, +54.1] | slower |
| zoom / mainTaskMs | 140.2 [143.7] | 900.8 [948.9] | 6.425 [6.271, 6.700] | slower |
| zoom / mainScriptMs | 20.0 [23.0] | 495.8 [529.3] | 24.747 [23.058, 26.189] | slower |
| select / settleMs | 141.3 [147.4] | 293.9 [308.6] | +152.6 [+145.8, +163.1] | slower |
| select / mainTaskMs | 39.6 [43.9] | 236.5 [240.8] | 5.966 [5.800, 6.050] | slower |
| select / mainScriptMs | 9.3 [10.3] | 186.8 [189.5] | 20.021 [19.584, 20.578] | slower |
| search / searchMs | 46.5 [51.7] | 76.2 [77.8] | 1.641 [1.455, 1.738] | unreliable (A/A failed) |
| search / mainTaskMs | 4.0 [4.6] | 35.7 [37.2] | 8.907 [8.271, 9.579] | slower |
| search / mainScriptMs | 0.1 [0.1] | 18.8 [19.3] | 201.935 [188.318, 214.674] | slower |
| annotate / settleMs | 153.3 [153.8] | 330.3 [332.5] | +177.0 [+174.8, +178.7] | slower |
| annotate / mainTaskMs | 49.3 [51.8] | 281.6 [285.8] | 5.715 [5.548, 5.922] | slower |
| annotate / mainScriptMs | 10.1 [10.6] | 209.7 [211.7] | 20.707 [20.283, 22.644] | slower |

### `main` vs `plugins`

| Row | main median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 44.3 [45.6] | 67.7 [69.2] | 1.528 [1.489, 1.617] | slower |
| open(ebook) / openedMs | 95.7 [98.7] | 168.5 [170.2] | 1.760 [1.737, 1.799] | slower |
| open(ebook) / firstPaintMs | 174.4 [182.3] | 323.9 [334.6] | 1.857 [1.807, 1.913] | slower |
| open(ebook) / settledMs | 265.9 [270.4] | 526.1 [541.9] | 1.979 [1.891, 2.032] | slower |
| open(ebook) / mainTaskMs | 92.8 [94.5] | 406.6 [444.1] | 4.383 [4.253, 4.645] | slower |
| open(ebook) / mainScriptMs | 15.7 [16.1] | 219.4 [245.9] | 13.966 [13.400, 15.059] | slower |
| open(large) / readyMs | 42.7 [45.5] | 67.7 [70.0] | 1.585 [1.514, 1.611] | slower |
| open(large) / openedMs | 109.5 [111.8] | 1346.3 [1385.3] | 12.290 [12.094, 12.409] | slower |
| open(large) / firstPaintMs | 181.7 [185.1] | 2213.5 [2287.6] | 12.179 [12.020, 12.623] | slower |
| open(large) / settledMs | 177.4 [181.3] | 2287.3 [2340.0] | 12.891 [12.681, 13.316] | slower |
| open(large) / mainTaskMs | 205.1 [207.5] | 2598.4 [2630.1] | 12.671 [12.457, 12.776] | slower |
| open(large) / mainScriptMs | 18.1 [18.4] | 2133.6 [2156.6] | 118.044 [115.734, 120.630] | slower |
| scroll / settleMs | 0.0 [0.0] | 139.9 [365.9] | +139.9 [+93.5, +320.5] | slower |
| scroll / wallMs | 2004.9 [2016.9] | 16079.1 [16924.2] | 8.020 [7.847, 8.222] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 41.7 [50.0] | 4.999 [3.999, 5.569] | slower |
| scroll / frameP95Ms | 8.4 [8.8] | 207.3 [234.2] | 24.579 [23.406, 26.675] | slower |
| scroll / jankFrames | 0 [0] | 99 [101.65] | +99 [+98, +99.50] | slower |
| scroll / longTasks | 0 [0] | 64.50 [66.55] | +64.50 [+62.50, +65.50] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 9613.0 [9952.5] | +9613.0 [+9189.5, +9808.5] | slower |
| scroll / mainTaskMs | 346.2 [366.6] | 16483.5 [17383.0] | 47.610 [45.404, 49.375] | slower |
| scroll / mainScriptMs | 46.4 [47.8] | 9095.1 [9774.2] | 196.201 [190.697, 205.870] | slower |
| jump / settleMs | 81.3 [83.1] | 992.3 [1097.1] | +911.0 [+769.3, +1002.3] | slower |
| jump / mainTaskMs | 48.1 [50.2] | 1334.4 [1463.4] | 27.749 [25.125, 30.151] | slower |
| jump / mainScriptMs | 12.1 [13.5] | 878.0 [985.7] | 72.790 [63.255, 80.525] | slower |
| zoom / settleMs | 305.9 [319.4] | 537.2 [554.2] | +231.3 [+212.9, +252.5] | slower |
| zoom / zoom2SettleMs | 111.3 [115.7] | 245.6 [254.6] | +134.3 [+129.6, +144.3] | slower |
| zoom / zoom4SettleMs | 195.8 [206.2] | 289.9 [302.7] | +94.0 [+80.8, +112.0] | slower |
| zoom / backTo1SettleMs | 62.0 [72.1] | 106.7 [108.7] | +44.7 [+40.6, +51.0] | slower |
| zoom / mainTaskMs | 143.1 [145.0] | 900.8 [948.9] | 6.296 [6.170, 6.650] | slower |
| zoom / mainScriptMs | 22.1 [23.4] | 495.8 [529.3] | 22.411 [21.490, 25.330] | slower |
| select / settleMs | 142.5 [148.1] | 293.9 [308.6] | +151.4 [+145.5, +162.4] | slower |
| select / mainTaskMs | 41.5 [43.1] | 236.5 [240.8] | 5.704 [5.561, 6.169] | slower |
| select / mainScriptMs | 9.7 [10.1] | 186.8 [189.5] | 19.348 [18.892, 20.213] | slower |
| search / searchMs | 51.5 [53.1] | 76.2 [77.8] | 1.481 [1.369, 1.554] | unreliable (A/A failed) |
| search / mainTaskMs | 4.0 [4.6] | 35.7 [37.2] | 8.952 [8.177, 9.193] | slower |
| search / mainScriptMs | 0.1 [0.1] | 18.8 [19.3] | 200.856 [188.318, 209.204] | slower |
| annotate / settleMs | 152.6 [154.3] | 330.3 [332.5] | +177.7 [+175.2, +179.4] | slower |
| annotate / mainTaskMs | 50.0 [52.1] | 281.6 [285.8] | 5.627 [5.444, 5.855] | slower |
| annotate / mainScriptMs | 10.3 [10.9] | 209.7 [211.7] | 20.278 [19.498, 21.075] | slower |

### `plugins` vs `plugins-lto`

| Row | plugins median [p95] | plugins-lto median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 67.7 [69.2] | 69.3 [70.6] | 1.024 [0.995, 1.053] | no detectable difference |
| open(ebook) / openedMs | 168.5 [170.2] | 168.1 [170.9] | 0.998 [0.989, 1.011] | no detectable difference |
| open(ebook) / firstPaintMs | 323.9 [334.6] | 320.9 [329.0] | 0.991 [0.964, 1.010] | no detectable difference |
| open(ebook) / settledMs | 526.1 [541.9] | 485.7 [491.0] | 0.923 [0.899, 0.965] | faster |
| open(ebook) / mainTaskMs | 406.6 [444.1] | 366.8 [377.4] | 0.902 [0.850, 0.928] | faster |
| open(ebook) / mainScriptMs | 219.4 [245.9] | 189.8 [197.1] | 0.865 [0.803, 0.902] | faster |
| open(large) / readyMs | 67.7 [70.0] | 68.5 [69.1] | 1.012 [0.993, 1.023] | no detectable difference |
| open(large) / openedMs | 1346.3 [1385.3] | 1239.2 [1274.1] | 0.920 [0.913, 0.927] | faster |
| open(large) / firstPaintMs | 2213.5 [2287.6] | 2006.0 [2060.5] | 0.906 [0.889, 0.914] | faster |
| open(large) / settledMs | 2287.3 [2340.0] | 2055.6 [2119.5] | 0.899 [0.890, 0.917] | faster |
| open(large) / mainTaskMs | 2598.4 [2630.1] | 2324.2 [2395.6] | 0.895 [0.886, 0.914] | faster |
| open(large) / mainScriptMs | 2133.6 [2156.6] | 1907.8 [1965.9] | 0.894 [0.885, 0.915] | faster |
| scroll / settleMs | 139.9 [365.9] | 161.3 [307.8] | +21.4 [-175.1, +65.6] | no detectable difference |
| scroll / wallMs | 16079.1 [16924.2] | 14221.0 [14816.7] | 0.884 [0.859, 0.908] | faster |
| scroll / frameP50Ms | 41.7 [50.0] | 37.2 [41.7] | 0.892 [0.672, 1.250] | no detectable difference |
| scroll / frameP95Ms | 207.3 [234.2] | 191.5 [196.2] | 0.924 [0.836, 0.957] | faster |
| scroll / jankFrames | 99 [101.65] | 94.50 [100.65] | -4.50 [-8, +1] | no detectable difference |
| scroll / longTasks | 64.50 [66.55] | 63.50 [66.10] | -1 [-2.50, +1.50] | no detectable difference |
| scroll / longTaskMs | 9613.0 [9952.5] | 8404.0 [8919.2] | -1209.0 [-1512.1, -759.0] | faster |
| scroll / mainTaskMs | 16483.5 [17383.0] | 14633.7 [15254.2] | 0.888 [0.863, 0.911] | faster |
| scroll / mainScriptMs | 9095.1 [9774.2] | 8066.9 [8541.7] | 0.887 [0.844, 0.917] | faster |
| jump / settleMs | 992.3 [1097.1] | 816.6 [912.0] | -175.6 [-305.2, -21.6] | faster |
| jump / mainTaskMs | 1334.4 [1463.4] | 1163.4 [1258.6] | 0.872 [0.778, 0.975] | faster |
| jump / mainScriptMs | 878.0 [985.7] | 769.6 [852.8] | 0.877 [0.756, 1.017] | no detectable difference |
| zoom / settleMs | 537.2 [554.2] | 511.0 [518.6] | -26.2 [-39.1, -8.7] | faster |
| zoom / zoom2SettleMs | 245.6 [254.6] | 234.8 [239.3] | -10.8 [-20.8, -7.3] | faster |
| zoom / zoom4SettleMs | 289.9 [302.7] | 276.2 [283.4] | -13.7 [-25.9, -0.9] | no detectable difference |
| zoom / backTo1SettleMs | 106.7 [108.7] | 98.8 [104.3] | -7.9 [-11.4, -4.1] | faster |
| zoom / mainTaskMs | 900.8 [948.9] | 798.1 [821.4] | 0.886 [0.845, 0.913] | faster |
| zoom / mainScriptMs | 495.8 [529.3] | 426.5 [444.0] | 0.860 [0.805, 0.893] | faster |
| select / settleMs | 293.9 [308.6] | 291.1 [298.6] | -2.8 [-18.6, +1.3] | no detectable difference |
| select / mainTaskMs | 236.5 [240.8] | 221.5 [224.7] | 0.936 [0.915, 0.948] | no detectable difference |
| select / mainScriptMs | 186.8 [189.5] | 173.4 [177.5] | 0.928 [0.908, 0.944] | no detectable difference |
| search / searchMs | 76.2 [77.8] | 73.3 [76.0] | 0.962 [0.946, 1.045] | unreliable (A/A failed) |
| search / mainTaskMs | 35.7 [37.2] | 33.9 [34.7] | 0.950 [0.911, 0.971] | no detectable difference |
| search / mainScriptMs | 18.8 [19.3] | 17.0 [17.3] | 0.903 [0.882, 0.947] | no detectable difference |
| annotate / settleMs | 330.3 [332.5] | 325.6 [327.1] | -4.7 [-7.0, -2.3] | no detectable difference |
| annotate / mainTaskMs | 281.6 [285.8] | 260.4 [263.0] | 0.925 [0.914, 0.936] | no detectable difference |
| annotate / mainScriptMs | 209.7 [211.7] | 191.9 [194.0] | 0.915 [0.903, 0.926] | faster |

### `plugins-lto` vs `plugins-fix`

Not run: `plugins-fix`.

### `plugins-fix` vs `plugins-fix34`

Not run: `plugins-fix`, `plugins-fix34`.

### `plugins-fix34` vs `plugins-fix-leak`

Not run: `plugins-fix34`, `plugins-fix-leak`.

### `plugins-fix-leak` vs `plugins-fix-zoom`

Not run: `plugins-fix-leak`.

### `plugins-lto` vs `plugins-fix-zoom`

| Row | plugins-lto median [p95] | plugins-fix-zoom median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 69.3 [70.6] | 68.4 [71.3] | 0.987 [0.966, 1.011] | no detectable difference |
| open(ebook) / openedMs | 168.1 [170.9] | 146.8 [150.4] | 0.873 [0.860, 0.889] | faster |
| open(ebook) / firstPaintMs | 320.9 [329.0] | 252.5 [258.0] | 0.787 [0.774, 0.801] | faster |
| open(ebook) / settledMs | 485.7 [491.0] | 294.2 [299.5] | 0.606 [0.599, 0.614] | faster |
| open(ebook) / mainTaskMs | 366.8 [377.4] | 214.9 [221.0] | 0.586 [0.577, 0.593] | faster |
| open(ebook) / mainScriptMs | 189.8 [197.1] | 95.7 [99.1] | 0.504 [0.494, 0.513] | faster |
| open(large) / readyMs | 68.5 [69.1] | 68.1 [69.6] | 0.994 [0.986, 1.005] | no detectable difference |
| open(large) / openedMs | 1239.2 [1274.1] | 167.2 [173.4] | 0.135 [0.134, 0.137] | faster |
| open(large) / firstPaintMs | 2006.0 [2060.5] | 298.6 [315.1] | 0.149 [0.146, 0.151] | faster |
| open(large) / settledMs | 2055.6 [2119.5] | 311.2 [318.2] | 0.151 [0.149, 0.153] | faster |
| open(large) / mainTaskMs | 2324.2 [2395.6] | 505.3 [525.4] | 0.217 [0.215, 0.225] | faster |
| open(large) / mainScriptMs | 1907.8 [1965.9] | 354.5 [371.2] | 0.186 [0.183, 0.193] | faster |
| scroll / settleMs | 161.3 [307.8] | 0.0 [0.0] | -161.3 [-202.3, -103.1] | faster |
| scroll / wallMs | 14221.0 [14816.7] | 2619.0 [2630.6] | 0.184 [0.180, 0.186] | faster |
| scroll / frameP50Ms | 37.2 [41.7] | 8.3 [8.3] | 0.224 [0.200, 0.286] | faster |
| scroll / frameP95Ms | 191.5 [196.2] | 9.7 [10.2] | 0.051 [0.049, 0.054] | faster |
| scroll / jankFrames | 94.50 [100.65] | 0 [0] | -94.50 [-99, -91] | faster |
| scroll / longTasks | 63.50 [66.10] | 0 [0] | -63.50 [-65, -62] | faster |
| scroll / longTaskMs | 8404.0 [8919.2] | 0.0 [0.0] | -8404.0 [-8582.5, -8180.5] | faster |
| scroll / mainTaskMs | 14633.7 [15254.2] | 1354.4 [1408.3] | 0.093 [0.088, 0.096] | faster |
| scroll / mainScriptMs | 8066.9 [8541.7] | 910.5 [955.0] | 0.113 [0.105, 0.119] | faster |
| jump / settleMs | 816.6 [912.0] | 114.0 [130.2] | -702.6 [-767.6, -623.3] | faster |
| jump / mainTaskMs | 1163.4 [1258.6] | 374.4 [391.8] | 0.322 [0.304, 0.348] | faster |
| jump / mainScriptMs | 769.6 [852.8] | 323.8 [338.7] | 0.421 [0.392, 0.461] | faster |
| zoom / settleMs | 511.0 [518.6] | 354.7 [363.1] | -156.4 [-165.7, -148.2] | faster |
| zoom / zoom2SettleMs | 234.8 [239.3] | 139.3 [143.4] | -95.5 [-97.8, -86.9] | faster |
| zoom / zoom4SettleMs | 276.2 [283.4] | 213.6 [225.8] | -62.5 [-70.9, -51.9] | faster |
| zoom / backTo1SettleMs | 98.8 [104.3] | 76.8 [84.2] | -22.0 [-27.1, -16.5] | faster |
| zoom / mainTaskMs | 798.1 [821.4] | 286.8 [294.0] | 0.359 [0.351, 0.368] | faster |
| zoom / mainScriptMs | 426.5 [444.0] | 134.1 [136.6] | 0.314 [0.304, 0.324] | faster |
| select / settleMs | 291.1 [298.6] | 145.2 [150.0] | -145.9 [-151.9, -137.1] | faster |
| select / mainTaskMs | 221.5 [224.7] | 77.7 [81.3] | 0.351 [0.330, 0.360] | faster |
| select / mainScriptMs | 173.4 [177.5] | 42.1 [44.7] | 0.243 [0.234, 0.251] | faster |
| search / searchMs | 73.3 [76.0] | 61.3 [62.8] | 0.837 [0.769, 0.858] | unreliable (A/A failed) |
| search / mainTaskMs | 33.9 [34.7] | 20.1 [21.3] | 0.593 [0.529, 0.633] | faster |
| search / mainScriptMs | 17.0 [17.3] | 8.5 [9.3] | 0.501 [0.426, 0.548] | faster |
| annotate / settleMs | 325.6 [327.1] | 164.9 [166.8] | -160.8 [-161.5, -157.7] | faster |
| annotate / mainTaskMs | 260.4 [263.0] | 93.3 [94.7] | 0.358 [0.355, 0.364] | faster |
| annotate / mainScriptMs | 191.9 [194.0] | 47.8 [48.6] | 0.249 [0.246, 0.252] | faster |

### `main` vs `plugins-fix-zoom`

| Row | main median [p95] | plugins-fix-zoom median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 44.3 [45.6] | 68.4 [71.3] | 1.545 [1.512, 1.630] | slower |
| open(ebook) / openedMs | 95.7 [98.7] | 146.8 [150.4] | 1.534 [1.511, 1.573] | slower |
| open(ebook) / firstPaintMs | 174.4 [182.3] | 252.5 [258.0] | 1.448 [1.414, 1.478] | slower |
| open(ebook) / settledMs | 265.9 [270.4] | 294.2 [299.5] | 1.106 [1.096, 1.122] | slower |
| open(ebook) / mainTaskMs | 92.8 [94.5] | 214.9 [221.0] | 2.316 [2.275, 2.334] | slower |
| open(ebook) / mainScriptMs | 15.7 [16.1] | 95.7 [99.1] | 6.094 [5.956, 6.216] | slower |
| open(large) / readyMs | 42.7 [45.5] | 68.1 [69.6] | 1.593 [1.522, 1.609] | slower |
| open(large) / openedMs | 109.5 [111.8] | 167.2 [173.4] | 1.527 [1.502, 1.549] | slower |
| open(large) / firstPaintMs | 181.7 [185.1] | 298.6 [315.1] | 1.643 [1.613, 1.695] | slower |
| open(large) / settledMs | 177.4 [181.3] | 311.2 [318.2] | 1.754 [1.732, 1.816] | slower |
| open(large) / mainTaskMs | 205.1 [207.5] | 505.3 [525.4] | 2.464 [2.456, 2.554] | slower |
| open(large) / mainScriptMs | 18.1 [18.4] | 354.5 [371.2] | 19.611 [19.425, 20.451] | slower |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2004.9 [2016.9] | 2619.0 [2630.6] | 1.306 [1.290, 1.310] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.001] | no detectable difference |
| scroll / frameP95Ms | 8.4 [8.8] | 9.7 [10.2] | 1.149 [1.101, 1.187] | slower |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 346.2 [366.6] | 1354.4 [1408.3] | 3.912 [3.656, 4.105] | slower |
| scroll / mainScriptMs | 46.4 [47.8] | 910.5 [955.0] | 19.641 [18.397, 20.645] | slower |
| jump / settleMs | 81.3 [83.1] | 114.0 [130.2] | +32.7 [+22.2, +46.5] | slower |
| jump / mainTaskMs | 48.1 [50.2] | 374.4 [391.8] | 7.785 [7.486, 8.205] | slower |
| jump / mainScriptMs | 12.1 [13.5] | 323.8 [338.7] | 26.842 [25.334, 28.091] | slower |
| zoom / settleMs | 305.9 [319.4] | 354.7 [363.1] | +48.7 [+36.7, +67.3] | slower |
| zoom / zoom2SettleMs | 111.3 [115.7] | 139.3 [143.4] | +28.0 [+24.9, +35.7] | slower |
| zoom / zoom4SettleMs | 195.8 [206.2] | 213.6 [225.8] | +17.8 [+9.3, +35.5] | slower |
| zoom / backTo1SettleMs | 62.0 [72.1] | 76.8 [84.2] | +14.8 [+10.8, +22.3] | slower |
| zoom / mainTaskMs | 143.1 [145.0] | 286.8 [294.0] | 2.005 [1.984, 2.092] | slower |
| zoom / mainScriptMs | 22.1 [23.4] | 134.1 [136.6] | 6.059 [5.832, 6.643] | slower |
| select / settleMs | 142.5 [148.1] | 145.2 [150.0] | +2.7 [-4.7, +5.1] | no detectable difference |
| select / mainTaskMs | 41.5 [43.1] | 77.7 [81.3] | 1.874 [1.777, 2.026] | slower |
| select / mainScriptMs | 9.7 [10.1] | 42.1 [44.7] | 4.359 [4.194, 4.577] | slower |
| search / searchMs | 51.5 [53.1] | 61.3 [62.8] | 1.192 [1.096, 1.252] | unreliable (A/A failed) |
| search / mainTaskMs | 4.0 [4.6] | 20.1 [21.3] | 5.047 [4.486, 5.300] | slower |
| search / mainScriptMs | 0.1 [0.1] | 8.5 [9.3] | 90.904 [77.422, 100.486] | slower |
| annotate / settleMs | 152.6 [154.3] | 164.9 [166.8] | +12.3 [+11.0, +14.8] | slower |
| annotate / mainTaskMs | 50.0 [52.1] | 93.3 [94.7] | 1.864 [1.804, 1.941] | slower |
| annotate / mainScriptMs | 10.3 [10.9] | 47.8 [48.6] | 4.621 [4.441, 4.802] | slower |

### Memory

| Row | main median [p95] | main-aa median [p95] | impl-ts median [p95] | impl-rust median [p95] | plugins median [p95] | plugins-lto median [p95] | plugins-fix-zoom median [p95] |
| --- | --- | --- | --- | --- | --- | --- | --- |
| open(ebook) / heapAfterOpenBytes | 34.34 MiB [34.35 MiB] | 34.34 MiB [34.35 MiB] | 35.02 MiB [35.04 MiB] | 34.97 MiB [34.98 MiB] | 43.35 MiB [43.36 MiB] | 43.28 MiB [44.28 MiB] | 41.55 MiB [41.57 MiB] |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB [22.27 MiB] | 22.27 MiB [22.27 MiB] | 23.00 MiB [23.00 MiB] | 22.95 MiB [22.95 MiB] | 23.03 MiB [23.03 MiB] | 23.03 MiB [23.03 MiB] | 23.02 MiB [23.02 MiB] |
| open(large) / heapAfterOpenBytes | 48.15 MiB [48.16 MiB] | 48.16 MiB [48.18 MiB] | 48.84 MiB [48.85 MiB] | 48.81 MiB [48.82 MiB] | 58.10 MiB [58.10 MiB] | 58.01 MiB [58.02 MiB] | 57.28 MiB [57.28 MiB] |
| open(large) / workerHeapAfterOpenBytes | 22.26 MiB [22.26 MiB] | 22.26 MiB [22.28 MiB] | 22.99 MiB [22.99 MiB] | 22.96 MiB [22.96 MiB] | 22.94 MiB [22.94 MiB] | 22.94 MiB [22.94 MiB] | 22.95 MiB [22.95 MiB] |
| soak / heapCycle5Bytes | 37.42 MiB [37.43 MiB] | 37.42 MiB [37.42 MiB] | 38.18 MiB [38.19 MiB] | 38.13 MiB [38.14 MiB] | 49.05 MiB [49.61 MiB] | 48.96 MiB [48.98 MiB] | 44.36 MiB [44.37 MiB] |
| soak / heapCycle30Bytes | 38.27 MiB [38.28 MiB] | 38.27 MiB [38.28 MiB] | 39.04 MiB [39.05 MiB] | 39.00 MiB [39.01 MiB] | 63.11 MiB [63.67 MiB] | 63.02 MiB [63.05 MiB] | 45.05 MiB [45.05 MiB] |
| soak / heapSoakSlopeBytes | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.56 MiB [0.56 MiB] | 0.56 MiB [0.56 MiB] | 0.03 MiB [0.03 MiB] |

### Diagnostics

Kernel table counts from the Rust kernel at soak cycles 5 and 30, median over rounds. Not compared: no other arm has them.

`plugins`:

| Table | Cycle 5 | Cycle 30 |
| --- | --- | --- |
| aborts | 0 | 0 |
| allHosts | 296 | 296 |
| described | 18 | 18 |
| documents | 1 | 1 |
| fns | 259 | 259 |
| heldFns | 259 | 259 |
| hosts | 296 | 296 |
| promises | 0 | 0 |
| proxies | 18 | 18 |
| refs | 645 | 645 |
| wasmBytes | 9.00 MiB | 9.00 MiB |
| wireRefs | 645 | 645 |

`plugins-lto`:

| Table | Cycle 5 | Cycle 30 |
| --- | --- | --- |
| aborts | 0 | 0 |
| allHosts | 296 | 296 |
| described | 18 | 18 |
| documents | 1 | 1 |
| fns | 259 | 259 |
| heldFns | 259 | 259 |
| hosts | 296 | 296 |
| promises | 0 | 0 |
| proxies | 18 | 18 |
| refs | 645 | 645 |
| wasmBytes | 8.94 MiB | 8.94 MiB |
| wireRefs | 645 | 645 |

`plugins-fix-zoom`:

| Table | Cycle 5 | Cycle 30 |
| --- | --- | --- |
| aborts | 0 | 0 |
| allHosts | 296 | 296 |
| described | 18 | 18 |
| documents | 1 | 1 |
| fns | 259 | 259 |
| heldFns | 259 | 259 |
| hosts | 296 | 296 |
| promises | 0 | 0 |
| proxies | 18 | 18 |
| refs | 645 | 645 |
| wasmBytes | 8.94 MiB | 8.94 MiB |
| wireRefs | 645 | 645 |

