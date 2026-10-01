# Viewer perf results, 2026-09-30

The Rust plugins kernel now settles within 11% of `main` on zoom, select, search and annotate,
and scrolls `large.pdf` with no long tasks, no jank and `main`'s 8.3 ms median frame. Drag-select,
search and zoom-back settle in the same time as `main`. It still does twice the main-thread work:
against `main`, `plugins-bump` is 1.8x main-thread task time and 2x JavaScript time across a
60-wheel scroll. Opening is 1.5x (`large.pdf` opens 74 ms later and settles 115 ms later), and a
jump settles 26 ms later.

`plugins-bump` carries every kernel change kept from `FINDINGS.md`. It was measured at
`dd1b86fcc`, which is in `research/multi-platform-rust-core-impl-plugins` through merge
`416ee81b8`. Against `plugins-getstate` (`776f0c265`, section 5's wire fixes and the incremental
`getState`), the toolbar gate and the wire's revision-keyed answer cache cut scroll task
time 14%, scroll wall time 8%, jump settle 37 ms, and `zoom`, `select` and `annotate` task time 10
to 13%. `plugins-lto` is the plugins branch before any of this, built with LTO: opening
`large.pdf` there takes 1589 ms against `plugins-bump`'s 216, and a scroll 18.9 s of main-thread
task time against 0.76 s.

The impl branch hasn't moved. `impl-rust` shows no detectable difference from `impl-ts` on any row,
and `impl-ts` differs from `main` only on `open(ebook) / settledMs`, 3% slower. The Rust core costs
3.4 MiB of engine wasm (0.7 MiB brotli) on both research branches, and the plugins kernel adds
11.7 MiB (1.9 MiB brotli) while shipping 0.75 MiB less JavaScript.

`mainJsMs` replaces `mainScriptMs` from this run on: it counts microtask JavaScript, which
`ScriptDuration` left out (FINDINGS.md, section 6), so its numbers aren't comparable with the
`mainScriptMs` rows in this file's history. The trace behind it costs 3% or less on most timing
rows and 11 to 16% on `zoom` settle.

This is one `desktop` run: 10 rounds on an Apple M2 Max (`2023MBP16Grey`, timed over SSH so the
build machine's load stays out), on AC power, harness at `364990a39`. It took 123.1 min and
recorded 720 samples, none failed. The A/A pair passed 38 of 39 rows; `jump / mainTaskMs` failed
it (1.034 against a 3% band), so that row isn't judged. The previous results, from an Apple M4 Max,
are in this file's history at `04ff546ab`; the two machines' numbers aren't comparable.

## All arms, median

Times are ms. The last column is `plugins-bump` against `main` from the report below: a ratio for
time and CPU rows, a difference in ms for settle, jank and long-task rows. "same" is no detectable
difference; "unreliable" means the row failed the A/A check. Memory rows show the plain ratio of
medians, which the report doesn't judge.

| Row | `main` | `impl-ts` | `impl-rust` | `plugins` | `plugins-lto` | `plugins-getstate` | `plugins-bump` | bump vs main |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| open(ebook) / readyMs | 60 | 63 | 59 | 93 | 94 | 89 | 93 | 1.536 slower |
| open(ebook) / openedMs | 124 | 131 | 128 | 221 | 221 | 189 | 191 | 1.544 slower |
| open(ebook) / firstPaintMs | 221 | 228 | 227 | 428 | 415 | 318 | 315 | 1.428 slower |
| open(ebook) / settledMs | 340 | 351 | 349 | 693 | 667 | 368 | 362 | 1.065 slower |
| open(ebook) / mainTaskMs | 123 | 124 | 124 | 572 | 542 | 260 | 244 | 1.991 slower |
| open(ebook) / mainJsMs | 129 | 166 | 132 | 589 | 557 | 249 | 270 | 2.094 slower |
| open(large) / readyMs | 60 | 61 | 58 | 92 | 92 | 91 | 92 | 1.545 slower |
| open(large) / openedMs | 142 | 149 | 146 | 1702 | 1589 | 217 | 216 | 1.526 slower |
| open(large) / firstPaintMs | 229 | 237 | 238 | 2791 | 2541 | 348 | 341 | 1.488 slower |
| open(large) / settledMs | 228 | 234 | 235 | 2886 | 2660 | 358 | 343 | 1.504 slower |
| open(large) / mainTaskMs | 265 | 266 | 264 | 3204 | 2933 | 531 | 528 | 1.992 slower |
| open(large) / mainJsMs | 277 | 312 | 275 | 3216 | 2937 | 519 | 550 | 1.985 slower |
| scroll / settleMs | 0.0 | 0.0 | 0.0 | 222 | 202 | 0.0 | 0.0 | same |
| scroll / wallMs | 2020 | 2022 | 2026 | 20879 | 18458 | 2431 | 2246 | 1.112 slower |
| scroll / frameP50Ms | 8.3 | 8.3 | 8.3 | 58 | 50 | 8.3 | 8.3 | same |
| scroll / frameP95Ms | 8.6 | 8.6 | 8.6 | 280 | 238 | 8.9 | 9.3 | 1.088 slower |
| scroll / jankFrames | 0 | 0 | 0 | 153 | 108 | 0 | 0 | same |
| scroll / longTasks | 0 | 0 | 0 | 226 | 86 | 0 | 0 | same |
| scroll / longTaskMs | 0.0 | 0.0 | 0.0 | 20810 | 11896 | 0.0 | 0.0 | same |
| scroll / mainTaskMs | 416 | 420 | 410 | 21377 | 18884 | 882 | 756 | 1.819 slower |
| scroll / mainJsMs | 319 | 322 | 312 | 21216 | 18716 | 781 | 647 | 2.027 slower |
| jump / settleMs | 99 | 100 | 100 | 1234 | 1159 | 162 | 125 | +26.5 slower |
| jump / mainTaskMs | 59 | 60 | 60 | 1589 | 1506 | 177 | 147 | unreliable |
| jump / mainJsMs | 43 | 45 | 43 | 1442 | 1381 | 168 | 129 | 3.006 slower |
| zoom / settleMs | 394 | 397 | 398 | 674 | 611 | 438 | 436 | +41.7 slower |
| zoom / zoom2SettleMs | 133 | 135 | 137 | 312 | 279 | 156 | 157 | +24.2 slower |
| zoom / zoom4SettleMs | 261 | 260 | 263 | 366 | 334 | 282 | 283 | +21.4 slower |
| zoom / backTo1SettleMs | 78 | 82 | 87 | 143 | 136 | 94 | 91 | same |
| zoom / mainTaskMs | 185 | 185 | 189 | 1214 | 1125 | 321 | 280 | 1.510 slower |
| zoom / mainJsMs | 134 | 132 | 135 | 1097 | 1014 | 273 | 233 | 1.748 slower |
| select / settleMs | 146 | 146 | 148 | 401 | 324 | 144 | 148 | same |
| select / mainTaskMs | 49 | 49 | 48 | 303 | 274 | 77 | 70 | 1.438 slower |
| select / mainJsMs | 28 | 29 | 29 | 290 | 261 | 62 | 53 | 1.879 slower |
| search / searchMs | 60 | 61 | 62 | 91 | 91 | 68 | 66 | same |
| search / mainTaskMs | 5.2 | 4.7 | 5.0 | 43 | 41 | 20 | 15 | 2.851 slower |
| search / mainJsMs | 3.2 | 3.3 | 3.3 | 30 | 28 | 10 | 7.8 | 2.419 slower |
| annotate / settleMs | 156 | 156 | 160 | 459 | 428 | 166 | 166 | +9.5 slower |
| annotate / mainTaskMs | 59 | 59 | 59 | 364 | 338 | 96 | 84 | 1.421 slower |
| annotate / mainJsMs | 35 | 34 | 35 | 346 | 319 | 76 | 63 | 1.815 slower |
| open(ebook) / heapAfterOpenBytes | 34.35 MiB | 35.03 MiB | 34.98 MiB | 43.36 MiB | 43.30 MiB | 41.50 MiB | 41.32 MiB | 1.20x |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB | 23.00 MiB | 22.95 MiB | 23.03 MiB | 23.03 MiB | 23.02 MiB | 23.02 MiB | 1.03x |
| open(large) / heapAfterOpenBytes | 48.15 MiB | 48.83 MiB | 48.81 MiB | 58.10 MiB | 58.03 MiB | 56.56 MiB | 56.12 MiB | 1.17x |
| open(large) / workerHeapAfterOpenBytes | 22.26 MiB | 22.99 MiB | 22.96 MiB | 22.94 MiB | 22.94 MiB | 22.95 MiB | 22.95 MiB | 1.03x |
| soak / heapCycle5Bytes | 37.41 MiB | 38.19 MiB | 38.13 MiB | 49.05 MiB | 48.98 MiB | 44.29 MiB | 44.12 MiB | 1.18x |
| soak / heapCycle30Bytes | 38.27 MiB | 39.03 MiB | 38.99 MiB | 63.11 MiB | 63.04 MiB | 44.96 MiB | 44.67 MiB | 1.17x |
| soak / heapSoakSlopeBytes | 0.03 MiB | 0.03 MiB | 0.03 MiB | 0.56 MiB | 0.56 MiB | 0.03 MiB | 0.02 MiB | 0.67x |

# Viewer perf comparison: 2026-09-30T23-17-54-004Z-2023MBP16Grey-desktop

10 rounds, profile `desktop` (CPU throttle 1x).

`workerTaskMs` is n/a: Chromium has no Performance domain on dedicated workers.

## Environment

| Field | Value |
| --- | --- |
| Harness commit | 364990a39c8fca3afabbd61b77b46374449c8435 |
| Chromium / Playwright | 140.0.7339.16 / 1.55.0 |
| Node | v24.20.0 |
| OS | Darwin 27.0.0 arm64 |
| CPU | Apple M2 Max, 12 cores |
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
| plugins-getstate | research/rust-kernel-wire | 776f0c265 | built (fork d045f389d, wasm32-eh) | 0f7407599d93 | 764d1ef49e62 |
| plugins-bump | research/rust-kernel-bump | dd1b86fcc | built (fork d045f389d, wasm32-eh) | 0f7407599d93 | 764d1ef49e62 |

## Gate

- `main`: pass
- `main-aa`: pass
- `impl-ts`: pass
- `impl-rust`: pass
- `plugins`: pass
- `plugins-lto`: pass
- `plugins-getstate`: pass
- `plugins-bump`: pass

## Sizes

| Metric | main | main-aa | impl-ts | impl-rust | plugins | plugins-lto | plugins-getstate | plugins-bump |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| engineWasmBytes | 7.19 MiB | 7.19 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB |
| engineWasmBrotliBytes | 2.15 MiB | 2.15 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB |
| kernelWasmBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 10.78 MiB | 11.68 MiB | 11.69 MiB | 11.69 MiB |
| kernelWasmBrotliBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 1.77 MiB | 1.88 MiB | 1.88 MiB | 1.88 MiB |
| jsBytes | 3.37 MiB | 3.37 MiB | 3.35 MiB | 3.35 MiB | 2.62 MiB | 2.62 MiB | 2.62 MiB | 2.62 MiB |
| jsBrotliBytes | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.80 MiB | 0.80 MiB | 0.80 MiB | 0.80 MiB |

## A/A: `main` vs `main-aa`

A metric passes when its interval includes the null value or its point estimate is within 3% of it. Its interval then sets the noise band every other pair must clear.

38 of 39 rows pass.

| Row | main median [p95] | main-aa median [p95] | ratio or diff [95% CI] | A/A |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 60.4 [63.4] | 62.9 [65.9] | 1.041 [0.889, 1.188] | pass |
| open(ebook) / openedMs | 123.7 [129.5] | 127.1 [130.0] | 1.028 [0.955, 1.097] | pass |
| open(ebook) / firstPaintMs | 220.6 [228.1] | 222.9 [227.3] | 1.011 [0.962, 1.053] | pass |
| open(ebook) / settledMs | 340.3 [348.0] | 341.9 [346.6] | 1.005 [0.981, 1.026] | pass |
| open(ebook) / mainTaskMs | 122.8 [127.4] | 124.1 [126.8] | 1.011 [0.981, 1.037] | pass |
| open(ebook) / mainJsMs | 128.9 [170.7] | 167.5 [170.1] | 1.300 [0.726, 1.782] | pass |
| open(large) / readyMs | 59.7 [63.6] | 61.7 [65.3] | 1.033 [0.916, 1.126] | pass |
| open(large) / openedMs | 141.6 [146.0] | 143.5 [148.2] | 1.013 [0.954, 1.052] | pass |
| open(large) / firstPaintMs | 229.2 [237.7] | 232.1 [241.4] | 1.012 [0.975, 1.035] | pass |
| open(large) / settledMs | 228.0 [234.5] | 229.1 [238.5] | 1.005 [0.967, 1.038] | pass |
| open(large) / mainTaskMs | 264.9 [267.8] | 264.6 [269.2] | 0.999 [0.993, 1.010] | pass |
| open(large) / mainJsMs | 277.1 [313.6] | 311.1 [315.3] | 1.123 [0.888, 1.309] | pass |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / wallMs | 2020.3 [2030.5] | 2018.7 [2032.6] | 0.999 [0.992, 1.006] | pass |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | pass |
| scroll / frameP95Ms | 8.6 [9.0] | 8.6 [9.4] | 1.009 [0.964, 1.056] | pass |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / mainTaskMs | 415.5 [426.0] | 414.6 [442.7] | 0.998 [0.962, 1.040] | pass |
| scroll / mainJsMs | 319.3 [325.1] | 317.3 [332.4] | 0.994 [0.961, 1.045] | pass |
| jump / settleMs | 98.8 [104.1] | 101.9 [105.0] | +3.2 [-0.7, +6.9] | pass |
| jump / mainTaskMs | 59.4 [60.6] | 61.4 [62.8] | 1.034 [1.022, 1.079] | A/A failed |
| jump / mainJsMs | 42.9 [47.4] | 45.8 [48.1] | 1.066 [0.993, 1.133] | pass |
| zoom / settleMs | 394.1 [412.2] | 394.5 [417.2] | +0.4 [-10.4, +17.3] | pass |
| zoom / zoom2SettleMs | 133.0 [137.5] | 130.0 [138.2] | -3.0 [-7.3, +4.8] | pass |
| zoom / zoom4SettleMs | 261.2 [274.9] | 266.6 [283.2] | +5.4 [-5.5, +12.9] | pass |
| zoom / backTo1SettleMs | 77.7 [91.6] | 82.1 [94.2] | +4.4 [-11.6, +14.6] | pass |
| zoom / mainTaskMs | 185.3 [193.3] | 187.6 [194.6] | 1.013 [0.966, 1.052] | pass |
| zoom / mainJsMs | 133.6 [139.0] | 132.4 [143.0] | 0.991 [0.948, 1.044] | pass |
| select / settleMs | 146.4 [149.1] | 148.4 [157.9] | +1.9 [-1.5, +6.5] | pass |
| select / mainTaskMs | 48.6 [54.2] | 49.1 [51.0] | 1.012 [0.960, 1.060] | pass |
| select / mainJsMs | 28.4 [31.2] | 28.7 [30.0] | 1.010 [0.973, 1.068] | pass |
| search / searchMs | 59.5 [64.4] | 63.5 [68.9] | 1.066 [0.992, 1.149] | pass |
| search / mainTaskMs | 5.2 [6.2] | 5.2 [8.3] | 1.002 [0.849, 1.124] | pass |
| search / mainJsMs | 3.2 [3.9] | 3.3 [4.9] | 1.012 [0.940, 1.159] | pass |
| annotate / settleMs | 156.3 [159.8] | 156.5 [162.8] | +0.2 [-2.5, +5.0] | pass |
| annotate / mainTaskMs | 59.4 [63.4] | 59.5 [62.9] | 1.001 [0.964, 1.041] | pass |
| annotate / mainJsMs | 34.6 [38.4] | 35.0 [36.6] | 1.012 [0.954, 1.076] | pass |

## `main` vs `impl-ts`

| Row | main median [p95] | impl-ts median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 60.4 [63.4] | 63.0 [63.9] | 1.042 [0.892, 1.187] | no detectable difference |
| open(ebook) / openedMs | 123.7 [129.5] | 131.4 [133.3] | 1.062 [0.980, 1.134] | no detectable difference |
| open(ebook) / firstPaintMs | 220.6 [228.1] | 228.2 [236.3] | 1.034 [0.998, 1.077] | no detectable difference |
| open(ebook) / settledMs | 340.3 [348.0] | 350.6 [358.7] | 1.030 [1.006, 1.055] | slower |
| open(ebook) / mainTaskMs | 122.8 [127.4] | 124.1 [131.8] | 1.011 [0.976, 1.039] | no detectable difference |
| open(ebook) / mainJsMs | 128.9 [170.7] | 166.3 [170.2] | 1.290 [0.720, 1.795] | no detectable difference |
| open(large) / readyMs | 59.7 [63.6] | 61.4 [64.1] | 1.028 [0.905, 1.112] | no detectable difference |
| open(large) / openedMs | 141.6 [146.0] | 148.9 [152.0] | 1.051 [0.970, 1.091] | no detectable difference |
| open(large) / firstPaintMs | 229.2 [237.7] | 236.7 [247.5] | 1.033 [0.992, 1.057] | no detectable difference |
| open(large) / settledMs | 228.0 [234.5] | 233.6 [243.9] | 1.024 [0.983, 1.072] | no detectable difference |
| open(large) / mainTaskMs | 264.9 [267.8] | 266.1 [273.0] | 1.005 [0.988, 1.017] | no detectable difference |
| open(large) / mainJsMs | 277.1 [313.6] | 311.7 [316.4] | 1.125 [0.766, 1.309] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2020.3 [2030.5] | 2021.5 [2054.9] | 1.001 [0.995, 1.011] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | no detectable difference |
| scroll / frameP95Ms | 8.6 [9.0] | 8.6 [9.2] | 1.001 [0.979, 1.025] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 415.5 [426.0] | 420.2 [449.9] | 1.011 [0.977, 1.044] | no detectable difference |
| scroll / mainJsMs | 319.3 [325.1] | 322.2 [338.1] | 1.009 [0.982, 1.044] | no detectable difference |
| jump / settleMs | 98.8 [104.1] | 100.5 [103.9] | +1.8 [-1.5, +6.0] | no detectable difference |
| jump / mainTaskMs | 59.4 [60.6] | 59.6 [62.6] | 1.004 [0.982, 1.059] | unreliable (A/A failed) |
| jump / mainJsMs | 42.9 [47.4] | 45.3 [46.0] | 1.054 [0.979, 1.094] | no detectable difference |
| zoom / settleMs | 394.1 [412.2] | 397.3 [409.4] | +3.2 [-11.6, +13.5] | no detectable difference |
| zoom / zoom2SettleMs | 133.0 [137.5] | 135.3 [138.7] | +2.2 [-1.7, +6.2] | no detectable difference |
| zoom / zoom4SettleMs | 261.2 [274.9] | 260.5 [272.6] | -0.7 [-11.0, +7.1] | no detectable difference |
| zoom / backTo1SettleMs | 77.7 [91.6] | 81.6 [93.1] | +3.9 [-11.8, +12.7] | no detectable difference |
| zoom / mainTaskMs | 185.3 [193.3] | 184.9 [195.1] | 0.998 [0.958, 1.054] | no detectable difference |
| zoom / mainJsMs | 133.6 [139.0] | 132.4 [138.5] | 0.991 [0.950, 1.048] | no detectable difference |
| select / settleMs | 146.4 [149.1] | 145.5 [153.3] | -0.9 [-5.8, +4.2] | no detectable difference |
| select / mainTaskMs | 48.6 [54.2] | 49.1 [56.7] | 1.011 [0.957, 1.105] | no detectable difference |
| select / mainJsMs | 28.4 [31.2] | 28.7 [33.2] | 1.011 [0.967, 1.119] | no detectable difference |
| search / searchMs | 59.5 [64.4] | 60.7 [63.9] | 1.020 [0.950, 1.081] | no detectable difference |
| search / mainTaskMs | 5.2 [6.2] | 4.7 [4.9] | 0.907 [0.867, 0.987] | no detectable difference |
| search / mainJsMs | 3.2 [3.9] | 3.3 [3.6] | 1.041 [0.973, 1.086] | no detectable difference |
| annotate / settleMs | 156.3 [159.8] | 156.5 [165.1] | +0.2 [-3.0, +7.3] | no detectable difference |
| annotate / mainTaskMs | 59.4 [63.4] | 59.2 [64.4] | 0.996 [0.945, 1.083] | no detectable difference |
| annotate / mainJsMs | 34.6 [38.4] | 34.5 [39.1] | 0.997 [0.944, 1.099] | no detectable difference |

## `impl-ts` vs `impl-rust`

| Row | impl-ts median [p95] | impl-rust median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 63.0 [63.9] | 59.2 [64.3] | 0.941 [0.840, 1.119] | no detectable difference |
| open(ebook) / openedMs | 131.4 [133.3] | 127.5 [134.5] | 0.970 [0.920, 1.060] | no detectable difference |
| open(ebook) / firstPaintMs | 228.2 [236.3] | 227.0 [239.7] | 0.995 [0.967, 1.034] | no detectable difference |
| open(ebook) / settledMs | 350.6 [358.7] | 348.7 [358.1] | 0.994 [0.976, 1.017] | no detectable difference |
| open(ebook) / mainTaskMs | 124.1 [131.8] | 123.5 [128.4] | 0.995 [0.956, 1.038] | no detectable difference |
| open(ebook) / mainJsMs | 166.3 [170.2] | 131.9 [171.9] | 0.793 [0.547, 1.423] | no detectable difference |
| open(large) / readyMs | 61.4 [64.1] | 58.5 [64.9] | 0.953 [0.869, 1.107] | no detectable difference |
| open(large) / openedMs | 148.9 [152.0] | 146.1 [150.8] | 0.982 [0.929, 1.069] | no detectable difference |
| open(large) / firstPaintMs | 236.7 [247.5] | 237.8 [245.2] | 1.005 [0.975, 1.045] | no detectable difference |
| open(large) / settledMs | 233.6 [243.9] | 234.9 [241.9] | 1.006 [0.963, 1.043] | no detectable difference |
| open(large) / mainTaskMs | 266.1 [273.0] | 263.9 [273.4] | 0.991 [0.978, 1.024] | no detectable difference |
| open(large) / mainJsMs | 311.7 [316.4] | 275.4 [317.8] | 0.883 [0.759, 1.304] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2021.5 [2054.9] | 2026.0 [2039.6] | 1.002 [0.988, 1.007] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | no detectable difference |
| scroll / frameP95Ms | 8.6 [9.2] | 8.6 [9.1] | 1.005 [0.976, 1.035] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 420.2 [449.9] | 409.6 [424.7] | 0.975 [0.958, 1.006] | no detectable difference |
| scroll / mainJsMs | 322.2 [338.1] | 311.9 [327.1] | 0.968 [0.953, 1.002] | no detectable difference |
| jump / settleMs | 100.5 [103.9] | 100.0 [104.2] | -0.6 [-5.4, +3.8] | no detectable difference |
| jump / mainTaskMs | 59.6 [62.6] | 60.2 [63.3] | 1.010 [0.947, 1.054] | unreliable (A/A failed) |
| jump / mainJsMs | 45.3 [46.0] | 43.3 [47.6] | 0.956 [0.910, 1.019] | no detectable difference |
| zoom / settleMs | 397.3 [409.4] | 397.6 [414.2] | +0.4 [-10.7, +10.2] | no detectable difference |
| zoom / zoom2SettleMs | 135.3 [138.7] | 136.8 [141.3] | +1.5 [-4.2, +5.5] | no detectable difference |
| zoom / zoom4SettleMs | 260.5 [272.6] | 263.1 [274.4] | +2.6 [-11.5, +9.9] | no detectable difference |
| zoom / backTo1SettleMs | 81.6 [93.1] | 87.0 [89.0] | +5.4 [-7.9, +11.3] | no detectable difference |
| zoom / mainTaskMs | 184.9 [195.1] | 188.6 [190.0] | 1.020 [0.978, 1.044] | no detectable difference |
| zoom / mainJsMs | 132.4 [138.5] | 134.8 [138.0] | 1.018 [0.973, 1.051] | no detectable difference |
| select / settleMs | 145.5 [153.3] | 148.1 [155.2] | +2.6 [-3.1, +7.8] | no detectable difference |
| select / mainTaskMs | 49.1 [56.7] | 48.5 [51.8] | 0.988 [0.904, 1.039] | no detectable difference |
| select / mainJsMs | 28.7 [33.2] | 28.6 [30.7] | 0.998 [0.900, 1.050] | no detectable difference |
| search / searchMs | 60.7 [63.9] | 62.0 [65.9] | 1.021 [0.996, 1.096] | no detectable difference |
| search / mainTaskMs | 4.7 [4.9] | 5.0 [5.3] | 1.049 [0.983, 1.090] | no detectable difference |
| search / mainJsMs | 3.3 [3.6] | 3.3 [3.5] | 0.986 [0.952, 1.045] | no detectable difference |
| annotate / settleMs | 156.5 [165.1] | 160.1 [166.3] | +3.6 [-4.4, +7.4] | no detectable difference |
| annotate / mainTaskMs | 59.2 [64.4] | 59.3 [62.9] | 1.002 [0.922, 1.071] | no detectable difference |
| annotate / mainJsMs | 34.5 [39.1] | 35.2 [36.6] | 1.023 [0.926, 1.070] | no detectable difference |

## `impl-ts` vs `plugins`

| Row | impl-ts median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 63.0 [63.9] | 93.2 [96.1] | 1.480 [1.355, 1.738] | slower |
| open(ebook) / openedMs | 131.4 [133.3] | 221.4 [223.9] | 1.684 [1.649, 1.817] | slower |
| open(ebook) / firstPaintMs | 228.2 [236.3] | 427.9 [434.8] | 1.875 [1.845, 1.926] | slower |
| open(ebook) / settledMs | 350.6 [358.7] | 693.1 [710.0] | 1.977 [1.948, 2.017] | slower |
| open(ebook) / mainTaskMs | 124.1 [131.8] | 572.3 [591.8] | 4.611 [4.511, 4.788] | slower |
| open(ebook) / mainJsMs | 166.3 [170.2] | 588.7 [611.7] | 3.540 [3.246, 6.386] | slower |
| open(large) / readyMs | 61.4 [64.1] | 92.1 [95.1] | 1.500 [1.449, 1.668] | slower |
| open(large) / openedMs | 148.9 [152.0] | 1701.8 [1736.2] | 11.431 [11.215, 12.232] | slower |
| open(large) / firstPaintMs | 236.7 [247.5] | 2790.8 [2902.3] | 11.792 [11.507, 12.316] | slower |
| open(large) / settledMs | 233.6 [243.9] | 2886.1 [2952.8] | 12.356 [11.884, 12.797] | slower |
| open(large) / mainTaskMs | 266.1 [273.0] | 3203.6 [3278.4] | 12.038 [11.912, 12.300] | slower |
| open(large) / mainJsMs | 311.7 [316.4] | 3215.6 [3267.0] | 10.317 [10.201, 13.552] | slower |
| scroll / settleMs | 0.0 [0.0] | 222.4 [284.7] | +222.4 [+115.3, +280.2] | slower |
| scroll / wallMs | 2021.5 [2054.9] | 20879.1 [21336.9] | 10.329 [10.216, 10.464] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 58.3 [58.3] | 6.999 [6.999, 6.999] | slower |
| scroll / frameP95Ms | 8.6 [9.2] | 280.0 [290.5] | 32.631 [30.932, 33.579] | slower |
| scroll / jankFrames | 0 [0] | 153 [178.25] | +153 [+149, +159] | slower |
| scroll / longTasks | 0 [0] | 225.50 [257.45] | +225.50 [+221.50, +233] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 20810.5 [22715.9] | +20810.5 [+20553.5, +21309.0] | slower |
| scroll / mainTaskMs | 420.2 [449.9] | 21377.3 [21815.0] | 50.877 [50.161, 52.579] | slower |
| scroll / mainJsMs | 322.2 [338.1] | 21216.2 [21641.2] | 65.844 [64.897, 67.944] | slower |
| jump / settleMs | 100.5 [103.9] | 1233.8 [1342.1] | +1133.3 [+1102.8, +1193.3] | slower |
| jump / mainTaskMs | 59.6 [62.6] | 1589.3 [1674.9] | 26.655 [25.270, 27.899] | unreliable (A/A failed) |
| jump / mainJsMs | 45.3 [46.0] | 1442.4 [1551.4] | 31.869 [30.863, 34.228] | slower |
| zoom / settleMs | 397.3 [409.4] | 674.1 [684.2] | +276.8 [+262.6, +286.8] | slower |
| zoom / zoom2SettleMs | 135.3 [138.7] | 311.8 [316.9] | +176.5 [+169.7, +180.6] | slower |
| zoom / zoom4SettleMs | 260.5 [272.6] | 365.7 [375.1] | +105.2 [+88.7, +110.2] | slower |
| zoom / backTo1SettleMs | 81.6 [93.1] | 142.6 [146.7] | +60.9 [+52.4, +67.7] | slower |
| zoom / mainTaskMs | 184.9 [195.1] | 1214.2 [1246.2] | 6.567 [6.322, 6.738] | slower |
| zoom / mainJsMs | 132.4 [138.5] | 1096.6 [1124.7] | 8.281 [7.974, 8.513] | slower |
| select / settleMs | 145.5 [153.3] | 400.8 [406.2] | +255.3 [+248.4, +261.1] | slower |
| select / mainTaskMs | 49.1 [56.7] | 302.7 [306.2] | 6.166 [5.658, 6.397] | slower |
| select / mainJsMs | 28.7 [33.2] | 290.5 [292.3] | 10.118 [9.208, 10.441] | slower |
| search / searchMs | 60.7 [63.9] | 90.9 [95.8] | 1.498 [1.439, 1.610] | slower |
| search / mainTaskMs | 4.7 [4.9] | 43.2 [47.8] | 9.123 [8.815, 9.637] | slower |
| search / mainJsMs | 3.3 [3.6] | 30.2 [31.3] | 9.009 [8.703, 9.307] | slower |
| annotate / settleMs | 156.5 [165.1] | 459.2 [472.0] | +302.7 [+292.6, +309.0] | slower |
| annotate / mainTaskMs | 59.2 [64.4] | 363.9 [369.0] | 6.150 [5.664, 6.449] | slower |
| annotate / mainJsMs | 34.5 [39.1] | 346.2 [350.7] | 10.047 [9.158, 10.487] | slower |

## `main` vs `plugins`

| Row | main median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 60.4 [63.4] | 93.2 [96.1] | 1.542 [1.407, 1.760] | slower |
| open(ebook) / openedMs | 123.7 [129.5] | 221.4 [223.9] | 1.789 [1.705, 1.910] | slower |
| open(ebook) / firstPaintMs | 220.6 [228.1] | 427.9 [434.8] | 1.940 [1.881, 2.012] | slower |
| open(ebook) / settledMs | 340.3 [348.0] | 693.1 [710.0] | 2.037 [1.995, 2.079] | slower |
| open(ebook) / mainTaskMs | 122.8 [127.4] | 572.3 [591.8] | 4.661 [4.576, 4.791] | slower |
| open(ebook) / mainJsMs | 128.9 [170.7] | 588.7 [611.7] | 4.567 [3.402, 6.330] | slower |
| open(large) / readyMs | 59.7 [63.6] | 92.1 [95.1] | 1.542 [1.466, 1.669] | slower |
| open(large) / openedMs | 141.6 [146.0] | 1701.8 [1736.2] | 12.019 [11.673, 12.456] | slower |
| open(large) / firstPaintMs | 229.2 [237.7] | 2790.8 [2902.3] | 12.176 [11.816, 12.573] | slower |
| open(large) / settledMs | 228.0 [234.5] | 2886.1 [2952.8] | 12.657 [12.350, 13.033] | slower |
| open(large) / mainTaskMs | 264.9 [267.8] | 3203.6 [3278.4] | 12.095 [11.996, 12.271] | slower |
| open(large) / mainJsMs | 277.1 [313.6] | 3215.6 [3267.0] | 11.604 [10.285, 13.508] | slower |
| scroll / settleMs | 0.0 [0.0] | 222.4 [284.7] | +222.4 [+115.3, +280.2] | slower |
| scroll / wallMs | 2020.3 [2030.5] | 20879.1 [21336.9] | 10.335 [10.279, 10.465] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 58.3 [58.3] | 6.999 [6.999, 6.999] | slower |
| scroll / frameP95Ms | 8.6 [9.0] | 280.0 [290.5] | 32.665 [30.705, 33.832] | slower |
| scroll / jankFrames | 0 [0] | 153 [178.25] | +153 [+149, +159] | slower |
| scroll / longTasks | 0 [0] | 225.50 [257.45] | +225.50 [+221.50, +233] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 20810.5 [22715.9] | +20810.5 [+20553.5, +21309.0] | slower |
| scroll / mainTaskMs | 415.5 [426.0] | 21377.3 [21815.0] | 51.446 [50.268, 53.037] | slower |
| scroll / mainJsMs | 319.3 [325.1] | 21216.2 [21641.2] | 66.456 [65.286, 68.696] | slower |
| jump / settleMs | 98.8 [104.1] | 1233.8 [1342.1] | +1135.0 [+1104.4, +1194.7] | slower |
| jump / mainTaskMs | 59.4 [60.6] | 1589.3 [1674.9] | 26.765 [25.908, 28.309] | unreliable (A/A failed) |
| jump / mainJsMs | 42.9 [47.4] | 1442.4 [1551.4] | 33.593 [31.633, 35.646] | slower |
| zoom / settleMs | 394.1 [412.2] | 674.1 [684.2] | +280.0 [+261.5, +291.0] | slower |
| zoom / zoom2SettleMs | 133.0 [137.5] | 311.8 [316.9] | +178.8 [+171.9, +182.9] | slower |
| zoom / zoom4SettleMs | 261.2 [274.9] | 365.7 [375.1] | +104.5 [+86.9, +109.8] | slower |
| zoom / backTo1SettleMs | 77.7 [91.6] | 142.6 [146.7] | +64.9 [+51.5, +68.7] | slower |
| zoom / mainTaskMs | 185.3 [193.3] | 1214.2 [1246.2] | 6.554 [6.358, 6.745] | slower |
| zoom / mainJsMs | 133.6 [139.0] | 1096.6 [1124.7] | 8.210 [7.961, 8.474] | slower |
| select / settleMs | 146.4 [149.1] | 400.8 [406.2] | +254.4 [+249.5, +258.5] | slower |
| select / mainTaskMs | 48.6 [54.2] | 302.7 [306.2] | 6.231 [5.897, 6.490] | slower |
| select / mainJsMs | 28.4 [31.2] | 290.5 [292.3] | 10.231 [9.800, 10.690] | slower |
| search / searchMs | 59.5 [64.4] | 90.9 [95.8] | 1.527 [1.466, 1.619] | slower |
| search / mainTaskMs | 5.2 [6.2] | 43.2 [47.8] | 8.273 [7.941, 9.104] | slower |
| search / mainJsMs | 3.2 [3.9] | 30.2 [31.3] | 9.375 [8.788, 9.746] | slower |
| annotate / settleMs | 156.3 [159.8] | 459.2 [472.0] | +302.9 [+295.0, +309.3] | slower |
| annotate / mainTaskMs | 59.4 [63.4] | 363.9 [369.0] | 6.128 [5.981, 6.303] | slower |
| annotate / mainJsMs | 34.6 [38.4] | 346.2 [350.7] | 10.017 [9.669, 10.450] | slower |

## `plugins` vs `plugins-lto`

| Row | plugins median [p95] | plugins-lto median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 93.2 [96.1] | 93.9 [96.8] | 1.008 [0.931, 1.103] | no detectable difference |
| open(ebook) / openedMs | 221.4 [223.9] | 220.9 [226.0] | 0.998 [0.984, 1.023] | no detectable difference |
| open(ebook) / firstPaintMs | 427.9 [434.8] | 415.4 [444.7] | 0.971 [0.952, 1.014] | no detectable difference |
| open(ebook) / settledMs | 693.1 [710.0] | 666.9 [685.4] | 0.962 [0.950, 0.985] | faster |
| open(ebook) / mainTaskMs | 572.3 [591.8] | 542.5 [574.0] | 0.948 [0.918, 0.984] | faster |
| open(ebook) / mainJsMs | 588.7 [611.7] | 557.2 [597.6] | 0.947 [0.850, 1.037] | no detectable difference |
| open(large) / readyMs | 92.1 [95.1] | 92.2 [97.4] | 1.001 [0.941, 1.051] | no detectable difference |
| open(large) / openedMs | 1701.8 [1736.2] | 1589.0 [1620.6] | 0.934 [0.915, 0.946] | faster |
| open(large) / firstPaintMs | 2790.8 [2902.3] | 2541.3 [2636.7] | 0.911 [0.885, 0.928] | faster |
| open(large) / settledMs | 2886.1 [2952.8] | 2660.1 [2707.0] | 0.922 [0.903, 0.929] | faster |
| open(large) / mainTaskMs | 3203.6 [3278.4] | 2933.3 [2994.1] | 0.916 [0.898, 0.924] | faster |
| open(large) / mainJsMs | 3215.6 [3267.0] | 2937.2 [3007.0] | 0.913 [0.898, 0.926] | no detectable difference |
| scroll / settleMs | 222.4 [284.7] | 202.3 [352.5] | -20.1 [-223.8, +129.2] | no detectable difference |
| scroll / wallMs | 20879.1 [21336.9] | 18458.4 [23048.2] | 0.884 [0.854, 0.907] | faster |
| scroll / frameP50Ms | 58.3 [58.3] | 50.0 [63.8] | 0.857 [0.857, 0.857] | faster |
| scroll / frameP95Ms | 280.0 [290.5] | 238.5 [307.9] | 0.852 [0.815, 0.909] | faster |
| scroll / jankFrames | 153 [178.25] | 108.50 [151.50] | -44.50 [-51.50, -38.50] | faster |
| scroll / longTasks | 225.50 [257.45] | 85.50 [178.45] | -140 [-148.50, -127] | faster |
| scroll / longTaskMs | 20810.5 [22715.9] | 11896.0 [20679.5] | -8914.5 [-9510.0, -8264.9] | faster |
| scroll / mainTaskMs | 21377.3 [21815.0] | 18883.9 [23594.9] | 0.883 [0.855, 0.908] | faster |
| scroll / mainJsMs | 21216.2 [21641.2] | 18715.5 [23427.7] | 0.882 [0.854, 0.908] | faster |
| jump / settleMs | 1233.8 [1342.1] | 1159.0 [1346.0] | -74.8 [-166.8, +22.2] | no detectable difference |
| jump / mainTaskMs | 1589.3 [1674.9] | 1506.1 [1706.4] | 0.948 [0.897, 1.021] | unreliable (A/A failed) |
| jump / mainJsMs | 1442.4 [1551.4] | 1380.7 [1570.7] | 0.957 [0.899, 1.025] | no detectable difference |
| zoom / settleMs | 674.1 [684.2] | 610.9 [636.5] | -63.2 [-75.8, -43.5] | faster |
| zoom / zoom2SettleMs | 311.8 [316.9] | 278.6 [296.6] | -33.2 [-40.2, -21.9] | faster |
| zoom / zoom4SettleMs | 365.7 [375.1] | 334.2 [344.3] | -31.5 [-42.5, -13.7] | faster |
| zoom / backTo1SettleMs | 142.6 [146.7] | 135.6 [139.1] | -6.9 [-9.7, -5.1] | no detectable difference |
| zoom / mainTaskMs | 1214.2 [1246.2] | 1124.8 [1144.8] | 0.926 [0.909, 0.942] | faster |
| zoom / mainJsMs | 1096.6 [1124.7] | 1014.0 [1030.1] | 0.925 [0.908, 0.937] | faster |
| select / settleMs | 400.8 [406.2] | 324.3 [372.2] | -76.5 [-90.9, -55.6] | faster |
| select / mainTaskMs | 302.7 [306.2] | 273.6 [281.7] | 0.904 [0.892, 0.925] | faster |
| select / mainJsMs | 290.5 [292.3] | 261.1 [268.1] | 0.899 [0.886, 0.922] | faster |
| search / searchMs | 90.9 [95.8] | 91.0 [96.0] | 1.001 [0.960, 1.056] | no detectable difference |
| search / mainTaskMs | 43.2 [47.8] | 41.2 [47.2] | 0.955 [0.899, 1.033] | no detectable difference |
| search / mainJsMs | 30.2 [31.3] | 28.1 [28.8] | 0.932 [0.907, 0.955] | no detectable difference |
| annotate / settleMs | 459.2 [472.0] | 428.5 [441.1] | -30.8 [-38.1, -18.2] | faster |
| annotate / mainTaskMs | 363.9 [369.0] | 338.3 [340.6] | 0.930 [0.923, 0.934] | faster |
| annotate / mainJsMs | 346.2 [350.7] | 319.3 [321.3] | 0.922 [0.915, 0.929] | faster |

## `plugins-lto` vs `plugins-fix`

Not run: `plugins-fix`.

## `plugins-fix` vs `plugins-fix34`

Not run: `plugins-fix`, `plugins-fix34`.

## `plugins-fix34` vs `plugins-fix-leak`

Not run: `plugins-fix34`, `plugins-fix-leak`.

## `plugins-fix-leak` vs `plugins-fix-zoom`

Not run: `plugins-fix-leak`, `plugins-fix-zoom`.

## `plugins-lto` vs `plugins-fix-zoom`

Not run: `plugins-fix-zoom`.

## `main` vs `plugins-fix-zoom`

Not run: `plugins-fix-zoom`.

## `plugins-fix-zoom` vs `plugins-wire`

Not run: `plugins-fix-zoom`, `plugins-wire`.

## `plugins-wire` vs `plugins-getstate`

Not run: `plugins-wire`.

## `plugins-lto` vs `plugins-getstate`

| Row | plugins-lto median [p95] | plugins-getstate median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 93.9 [96.8] | 89.2 [96.1] | 0.949 [0.900, 1.032] | no detectable difference |
| open(ebook) / openedMs | 220.9 [226.0] | 189.4 [196.1] | 0.857 [0.832, 0.875] | faster |
| open(ebook) / firstPaintMs | 415.4 [444.7] | 317.9 [326.5] | 0.765 [0.735, 0.785] | faster |
| open(ebook) / settledMs | 666.9 [685.4] | 368.2 [377.4] | 0.552 [0.539, 0.560] | faster |
| open(ebook) / mainTaskMs | 542.5 [574.0] | 259.8 [262.4] | 0.479 [0.462, 0.492] | faster |
| open(ebook) / mainJsMs | 557.2 [597.6] | 249.1 [285.4] | 0.447 [0.367, 0.544] | no detectable difference |
| open(large) / readyMs | 92.2 [97.4] | 91.4 [94.8] | 0.991 [0.913, 1.063] | no detectable difference |
| open(large) / openedMs | 1589.0 [1620.6] | 217.2 [220.0] | 0.137 [0.133, 0.140] | faster |
| open(large) / firstPaintMs | 2541.3 [2636.7] | 348.3 [366.7] | 0.137 [0.135, 0.140] | faster |
| open(large) / settledMs | 2660.1 [2707.0] | 357.7 [363.1] | 0.134 [0.131, 0.137] | faster |
| open(large) / mainTaskMs | 2933.3 [2994.1] | 531.2 [542.9] | 0.181 [0.178, 0.184] | faster |
| open(large) / mainJsMs | 2937.2 [3007.0] | 518.9 [566.2] | 0.177 [0.163, 0.192] | faster |
| scroll / settleMs | 202.3 [352.5] | 0.0 [0.0] | -202.3 [-300.1, -3.2] | faster |
| scroll / wallMs | 18458.4 [23048.2] | 2431.3 [2449.9] | 0.132 [0.128, 0.136] | faster |
| scroll / frameP50Ms | 50.0 [63.8] | 8.3 [8.3] | 0.167 [0.167, 0.167] | faster |
| scroll / frameP95Ms | 238.5 [307.9] | 8.9 [9.5] | 0.037 [0.036, 0.040] | faster |
| scroll / jankFrames | 108.50 [151.50] | 0 [0] | -108.50 [-112, -103] | faster |
| scroll / longTasks | 85.50 [178.45] | 0 [0] | -85.50 [-101.50, -81] | faster |
| scroll / longTaskMs | 11896.0 [20679.5] | 0.0 [0.0] | -11896.0 [-12633.0, -11543.0] | faster |
| scroll / mainTaskMs | 18883.9 [23594.9] | 882.1 [903.0] | 0.047 [0.045, 0.048] | faster |
| scroll / mainJsMs | 18715.5 [23427.7] | 781.4 [799.9] | 0.042 [0.040, 0.043] | faster |
| jump / settleMs | 1159.0 [1346.0] | 162.0 [180.2] | -997.0 [-1096.5, -925.5] | faster |
| jump / mainTaskMs | 1506.1 [1706.4] | 177.4 [187.1] | 0.118 [0.101, 0.125] | unreliable (A/A failed) |
| jump / mainJsMs | 1380.7 [1570.7] | 167.5 [176.8] | 0.121 [0.100, 0.129] | faster |
| zoom / settleMs | 610.9 [636.5] | 437.9 [454.6] | -173.0 [-191.6, -153.3] | faster |
| zoom / zoom2SettleMs | 278.6 [296.6] | 156.3 [166.7] | -122.3 [-131.5, -112.9] | faster |
| zoom / zoom4SettleMs | 334.2 [344.3] | 281.6 [288.9] | -52.6 [-65.9, -38.9] | faster |
| zoom / backTo1SettleMs | 135.6 [139.1] | 94.2 [106.1] | -41.4 [-45.6, -32.0] | faster |
| zoom / mainTaskMs | 1124.8 [1144.8] | 320.8 [344.9] | 0.285 [0.278, 0.295] | faster |
| zoom / mainJsMs | 1014.0 [1030.1] | 272.7 [298.6] | 0.269 [0.265, 0.278] | faster |
| select / settleMs | 324.3 [372.2] | 143.8 [153.7] | -180.5 [-201.8, -164.4] | faster |
| select / mainTaskMs | 273.6 [281.7] | 77.3 [83.9] | 0.282 [0.275, 0.294] | faster |
| select / mainJsMs | 261.1 [268.1] | 61.7 [66.3] | 0.236 [0.229, 0.244] | faster |
| search / searchMs | 91.0 [96.0] | 67.6 [71.5] | 0.743 [0.688, 0.780] | faster |
| search / mainTaskMs | 41.2 [47.2] | 19.7 [22.6] | 0.477 [0.340, 0.527] | faster |
| search / mainJsMs | 28.1 [28.8] | 10.5 [11.4] | 0.374 [0.341, 0.396] | faster |
| annotate / settleMs | 428.5 [441.1] | 165.5 [171.1] | -263.0 [-272.1, -257.6] | faster |
| annotate / mainTaskMs | 338.3 [340.6] | 96.1 [100.2] | 0.284 [0.279, 0.292] | faster |
| annotate / mainJsMs | 319.3 [321.3] | 75.7 [78.1] | 0.237 [0.231, 0.241] | faster |

## `main` vs `plugins-getstate`

| Row | main median [p95] | plugins-getstate median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 60.4 [63.4] | 89.2 [96.1] | 1.475 [1.371, 1.708] | slower |
| open(ebook) / openedMs | 123.7 [129.5] | 189.4 [196.1] | 1.531 [1.449, 1.641] | slower |
| open(ebook) / firstPaintMs | 220.6 [228.1] | 317.9 [326.5] | 1.441 [1.395, 1.503] | slower |
| open(ebook) / settledMs | 340.3 [348.0] | 368.2 [377.4] | 1.082 [1.060, 1.106] | slower |
| open(ebook) / mainTaskMs | 122.8 [127.4] | 259.8 [262.4] | 2.116 [2.080, 2.165] | slower |
| open(ebook) / mainJsMs | 128.9 [170.7] | 249.1 [285.4] | 1.932 [1.290, 3.019] | slower |
| open(large) / readyMs | 59.7 [63.6] | 91.4 [94.8] | 1.530 [1.402, 1.666] | slower |
| open(large) / openedMs | 141.6 [146.0] | 217.2 [220.0] | 1.534 [1.470, 1.586] | slower |
| open(large) / firstPaintMs | 229.2 [237.7] | 348.3 [366.7] | 1.519 [1.476, 1.556] | slower |
| open(large) / settledMs | 228.0 [234.5] | 357.7 [363.1] | 1.569 [1.517, 1.617] | slower |
| open(large) / mainTaskMs | 264.9 [267.8] | 531.2 [542.9] | 2.006 [1.975, 2.029] | slower |
| open(large) / mainJsMs | 277.1 [313.6] | 518.9 [566.2] | 1.873 [1.554, 2.313] | slower |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2020.3 [2030.5] | 2431.3 [2449.9] | 1.203 [1.184, 1.209] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | no detectable difference |
| scroll / frameP95Ms | 8.6 [9.0] | 8.9 [9.5] | 1.041 [1.005, 1.088] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 415.5 [426.0] | 882.1 [903.0] | 2.123 [2.061, 2.194] | slower |
| scroll / mainJsMs | 319.3 [325.1] | 781.4 [799.9] | 2.448 [2.382, 2.537] | slower |
| jump / settleMs | 98.8 [104.1] | 162.0 [180.2] | +63.2 [+40.9, +72.8] | slower |
| jump / mainTaskMs | 59.4 [60.6] | 177.4 [187.1] | 2.988 [2.600, 3.146] | unreliable (A/A failed) |
| jump / mainJsMs | 42.9 [47.4] | 167.5 [176.8] | 3.900 [3.203, 4.121] | slower |
| zoom / settleMs | 394.1 [412.2] | 437.9 [454.6] | +43.8 [+27.7, +62.8] | slower |
| zoom / zoom2SettleMs | 133.0 [137.5] | 156.3 [166.7] | +23.3 [+19.9, +31.2] | slower |
| zoom / zoom4SettleMs | 261.2 [274.9] | 281.6 [288.9] | +20.4 [+8.3, +28.0] | slower |
| zoom / backTo1SettleMs | 77.7 [91.6] | 94.2 [106.1] | +16.5 [+2.8, +25.9] | slower |
| zoom / mainTaskMs | 185.3 [193.3] | 320.8 [344.9] | 1.732 [1.665, 1.817] | slower |
| zoom / mainJsMs | 133.6 [139.0] | 272.7 [298.6] | 2.042 [1.973, 2.136] | slower |
| select / settleMs | 146.4 [149.1] | 143.8 [153.7] | -2.6 [-6.8, +4.6] | no detectable difference |
| select / mainTaskMs | 48.6 [54.2] | 77.3 [83.9] | 1.591 [1.506, 1.677] | slower |
| select / mainJsMs | 28.4 [31.2] | 61.7 [66.3] | 2.174 [2.088, 2.288] | slower |
| search / searchMs | 59.5 [64.4] | 67.6 [71.5] | 1.136 [1.063, 1.212] | no detectable difference |
| search / mainTaskMs | 5.2 [6.2] | 19.7 [22.6] | 3.771 [2.715, 4.266] | slower |
| search / mainJsMs | 3.2 [3.9] | 10.5 [11.4] | 3.264 [2.917, 3.475] | slower |
| annotate / settleMs | 156.3 [159.8] | 165.5 [171.1] | +9.2 [+6.0, +12.8] | slower |
| annotate / mainTaskMs | 59.4 [63.4] | 96.1 [100.2] | 1.617 [1.569, 1.674] | slower |
| annotate / mainJsMs | 34.6 [38.4] | 75.7 [78.1] | 2.190 [2.094, 2.286] | slower |

## `plugins-getstate` vs `plugins-toolbar`

Not run: `plugins-toolbar`.

## `main` vs `plugins-toolbar`

Not run: `plugins-toolbar`.

## `plugins-toolbar` vs `plugins-bump`

Not run: `plugins-toolbar`.

## `main` vs `plugins-bump`

| Row | main median [p95] | plugins-bump median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 60.4 [63.4] | 92.9 [94.9] | 1.536 [1.392, 1.754] | slower |
| open(ebook) / openedMs | 123.7 [129.5] | 191.0 [196.1] | 1.544 [1.459, 1.657] | slower |
| open(ebook) / firstPaintMs | 220.6 [228.1] | 315.1 [322.4] | 1.428 [1.384, 1.483] | slower |
| open(ebook) / settledMs | 340.3 [348.0] | 362.5 [369.7] | 1.065 [1.044, 1.089] | slower |
| open(ebook) / mainTaskMs | 122.8 [127.4] | 244.5 [246.8] | 1.991 [1.954, 2.037] | slower |
| open(ebook) / mainJsMs | 128.9 [170.7] | 269.9 [274.0] | 2.094 [1.409, 2.883] | slower |
| open(large) / readyMs | 59.7 [63.6] | 92.3 [93.6] | 1.545 [1.378, 1.669] | slower |
| open(large) / openedMs | 141.6 [146.0] | 216.0 [221.0] | 1.526 [1.459, 1.587] | slower |
| open(large) / firstPaintMs | 229.2 [237.7] | 341.0 [348.6] | 1.488 [1.446, 1.518] | slower |
| open(large) / settledMs | 228.0 [234.5] | 343.1 [351.8] | 1.504 [1.458, 1.548] | slower |
| open(large) / mainTaskMs | 264.9 [267.8] | 527.7 [533.2] | 1.992 [1.967, 2.015] | slower |
| open(large) / mainJsMs | 277.1 [313.6] | 550.1 [561.3] | 1.985 [1.548, 2.316] | slower |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2020.3 [2030.5] | 2246.3 [2283.5] | 1.112 [1.087, 1.126] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | no detectable difference |
| scroll / frameP95Ms | 8.6 [9.0] | 9.3 [9.6] | 1.088 [1.061, 1.114] | slower |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 415.5 [426.0] | 755.9 [764.5] | 1.819 [1.729, 1.872] | slower |
| scroll / mainJsMs | 319.3 [325.1] | 647.0 [657.5] | 2.027 [1.936, 2.095] | slower |
| jump / settleMs | 98.8 [104.1] | 125.3 [155.7] | +26.5 [+13.8, +37.2] | slower |
| jump / mainTaskMs | 59.4 [60.6] | 146.6 [152.3] | 2.468 [2.422, 2.577] | unreliable (A/A failed) |
| jump / mainJsMs | 42.9 [47.4] | 129.1 [135.1] | 3.006 [2.848, 3.128] | slower |
| zoom / settleMs | 394.1 [412.2] | 435.8 [454.6] | +41.7 [+30.2, +54.2] | slower |
| zoom / zoom2SettleMs | 133.0 [137.5] | 157.2 [160.4] | +24.2 [+19.2, +28.8] | slower |
| zoom / zoom4SettleMs | 261.2 [274.9] | 282.6 [294.4] | +21.4 [+8.6, +28.0] | slower |
| zoom / backTo1SettleMs | 77.7 [91.6] | 90.8 [107.1] | +13.1 [-0.6, +24.3] | no detectable difference |
| zoom / mainTaskMs | 185.3 [193.3] | 279.7 [290.5] | 1.510 [1.464, 1.559] | slower |
| zoom / mainJsMs | 133.6 [139.0] | 233.4 [240.4] | 1.748 [1.692, 1.813] | slower |
| select / settleMs | 146.4 [149.1] | 147.5 [155.4] | +1.0 [-3.9, +4.6] | no detectable difference |
| select / mainTaskMs | 48.6 [54.2] | 69.8 [73.5] | 1.438 [1.340, 1.508] | slower |
| select / mainJsMs | 28.4 [31.2] | 53.3 [55.6] | 1.879 [1.776, 1.971] | slower |
| search / searchMs | 59.5 [64.4] | 65.8 [69.4] | 1.105 [1.066, 1.185] | no detectable difference |
| search / mainTaskMs | 5.2 [6.2] | 14.9 [18.5] | 2.851 [1.963, 3.473] | slower |
| search / mainJsMs | 3.2 [3.9] | 7.8 [8.4] | 2.419 [2.131, 2.559] | slower |
| annotate / settleMs | 156.3 [159.8] | 165.8 [169.1] | +9.5 [+6.7, +11.8] | slower |
| annotate / mainTaskMs | 59.4 [63.4] | 84.4 [86.0] | 1.421 [1.376, 1.457] | slower |
| annotate / mainJsMs | 34.6 [38.4] | 62.7 [64.6] | 1.815 [1.732, 1.901] | slower |

## `plugins-getstate` vs `plugins-item1`

Not run: `plugins-item1`.

## `plugins-getstate` vs `plugins-item3`

Not run: `plugins-item3`.

## `plugins-getstate` vs `plugins-bump`

| Row | plugins-getstate median [p95] | plugins-bump median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 89.2 [96.1] | 92.9 [94.9] | 1.041 [0.937, 1.088] | no detectable difference |
| open(ebook) / openedMs | 189.4 [196.1] | 191.0 [196.1] | 1.008 [0.973, 1.041] | no detectable difference |
| open(ebook) / firstPaintMs | 317.9 [326.5] | 315.1 [322.4] | 0.991 [0.972, 1.010] | no detectable difference |
| open(ebook) / settledMs | 368.2 [377.4] | 362.5 [369.7] | 0.984 [0.972, 0.994] | no detectable difference |
| open(ebook) / mainTaskMs | 259.8 [262.4] | 244.5 [246.8] | 0.941 [0.934, 0.950] | faster |
| open(ebook) / mainJsMs | 249.1 [285.4] | 269.9 [274.0] | 1.083 [0.799, 1.274] | no detectable difference |
| open(large) / readyMs | 91.4 [94.8] | 92.3 [93.6] | 1.010 [0.913, 1.072] | no detectable difference |
| open(large) / openedMs | 217.2 [220.0] | 216.0 [221.0] | 0.995 [0.964, 1.030] | no detectable difference |
| open(large) / firstPaintMs | 348.3 [366.7] | 341.0 [348.6] | 0.979 [0.955, 1.000] | no detectable difference |
| open(large) / settledMs | 357.7 [363.1] | 343.1 [351.8] | 0.959 [0.934, 0.985] | faster |
| open(large) / mainTaskMs | 531.2 [542.9] | 527.7 [533.2] | 0.993 [0.979, 1.011] | no detectable difference |
| open(large) / mainJsMs | 518.9 [566.2] | 550.1 [561.3] | 1.060 [0.867, 1.147] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2431.3 [2449.9] | 2246.3 [2283.5] | 0.924 [0.904, 0.942] | faster |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | no detectable difference |
| scroll / frameP95Ms | 8.9 [9.5] | 9.3 [9.6] | 1.046 [1.001, 1.081] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 882.1 [903.0] | 755.9 [764.5] | 0.857 [0.822, 0.869] | faster |
| scroll / mainJsMs | 781.4 [799.9] | 647.0 [657.5] | 0.828 [0.796, 0.844] | faster |
| jump / settleMs | 162.0 [180.2] | 125.3 [155.7] | -36.7 [-50.5, -10.5] | faster |
| jump / mainTaskMs | 177.4 [187.1] | 146.6 [152.3] | 0.826 [0.798, 0.949] | unreliable (A/A failed) |
| jump / mainJsMs | 167.5 [176.8] | 129.1 [135.1] | 0.771 [0.737, 0.935] | faster |
| zoom / settleMs | 437.9 [454.6] | 435.8 [454.6] | -2.1 [-16.6, +11.9] | no detectable difference |
| zoom / zoom2SettleMs | 156.3 [166.7] | 157.2 [160.4] | +0.9 [-8.1, +4.1] | no detectable difference |
| zoom / zoom4SettleMs | 281.6 [288.9] | 282.6 [294.4] | +1.0 [-9.3, +9.3] | no detectable difference |
| zoom / backTo1SettleMs | 94.2 [106.1] | 90.8 [107.1] | -3.4 [-12.8, +8.3] | no detectable difference |
| zoom / mainTaskMs | 320.8 [344.9] | 279.7 [290.5] | 0.872 [0.846, 0.894] | faster |
| zoom / mainJsMs | 272.7 [298.6] | 233.4 [240.4] | 0.856 [0.833, 0.875] | faster |
| select / settleMs | 143.8 [153.7] | 147.5 [155.4] | +3.7 [-4.9, +8.6] | no detectable difference |
| select / mainTaskMs | 77.3 [83.9] | 69.8 [73.5] | 0.904 [0.845, 0.934] | faster |
| select / mainJsMs | 61.7 [66.3] | 53.3 [55.6] | 0.864 [0.819, 0.892] | faster |
| search / searchMs | 67.6 [71.5] | 65.8 [69.4] | 0.972 [0.928, 1.055] | no detectable difference |
| search / mainTaskMs | 19.7 [22.6] | 14.9 [18.5] | 0.756 [0.498, 1.110] | no detectable difference |
| search / mainJsMs | 10.5 [11.4] | 7.8 [8.4] | 0.741 [0.654, 0.818] | faster |
| annotate / settleMs | 165.5 [171.1] | 165.8 [169.1] | +0.3 [-3.7, +3.5] | no detectable difference |
| annotate / mainTaskMs | 96.1 [100.2] | 84.4 [86.0] | 0.878 [0.849, 0.896] | faster |
| annotate / mainJsMs | 75.7 [78.1] | 62.7 [64.6] | 0.829 [0.802, 0.858] | faster |

## `plugins-item1` vs `plugins-bump`

Not run: `plugins-item1`.

## `plugins-bump` vs `plugins-reads`

Not run: `plugins-reads`.

## `main` vs `plugins-reads`

Not run: `plugins-reads`.

## `impl-ts` vs `impl-gate`

Not run: `impl-gate`.

## Memory

| Row | main median [p95] | main-aa median [p95] | impl-ts median [p95] | impl-rust median [p95] | plugins median [p95] | plugins-lto median [p95] | plugins-getstate median [p95] | plugins-bump median [p95] |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| open(ebook) / heapAfterOpenBytes | 34.35 MiB [34.90 MiB] | 34.35 MiB [34.92 MiB] | 35.03 MiB [35.59 MiB] | 34.98 MiB [34.99 MiB] | 43.36 MiB [43.36 MiB] | 43.30 MiB [43.31 MiB] | 41.50 MiB [41.51 MiB] | 41.32 MiB [41.33 MiB] |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB [22.27 MiB] | 22.27 MiB [22.27 MiB] | 23.00 MiB [23.00 MiB] | 22.95 MiB [22.95 MiB] | 23.03 MiB [23.03 MiB] | 23.03 MiB [23.03 MiB] | 23.02 MiB [23.02 MiB] | 23.02 MiB [23.02 MiB] |
| open(large) / heapAfterOpenBytes | 48.15 MiB [48.16 MiB] | 48.15 MiB [48.16 MiB] | 48.83 MiB [48.85 MiB] | 48.81 MiB [48.82 MiB] | 58.10 MiB [58.14 MiB] | 58.03 MiB [58.04 MiB] | 56.56 MiB [56.56 MiB] | 56.12 MiB [56.14 MiB] |
| open(large) / workerHeapAfterOpenBytes | 22.26 MiB [22.26 MiB] | 22.26 MiB [22.26 MiB] | 22.99 MiB [22.99 MiB] | 22.96 MiB [22.96 MiB] | 22.94 MiB [22.94 MiB] | 22.94 MiB [22.94 MiB] | 22.95 MiB [22.95 MiB] | 22.95 MiB [22.95 MiB] |
| soak / heapCycle5Bytes | 37.41 MiB [37.98 MiB] | 37.42 MiB [38.97 MiB] | 38.19 MiB [38.20 MiB] | 38.13 MiB [38.69 MiB] | 49.05 MiB [49.06 MiB] | 48.98 MiB [48.99 MiB] | 44.29 MiB [44.30 MiB] | 44.12 MiB [44.13 MiB] |
| soak / heapCycle30Bytes | 38.27 MiB [38.82 MiB] | 38.27 MiB [39.82 MiB] | 39.03 MiB [39.04 MiB] | 38.99 MiB [39.55 MiB] | 63.11 MiB [63.12 MiB] | 63.04 MiB [63.05 MiB] | 44.96 MiB [44.97 MiB] | 44.67 MiB [44.69 MiB] |
| soak / heapSoakSlopeBytes | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.56 MiB [0.56 MiB] | 0.56 MiB [0.56 MiB] | 0.03 MiB [0.03 MiB] | 0.02 MiB [0.02 MiB] |

## Diagnostics

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

`plugins-getstate`:

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

`plugins-bump`:

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

