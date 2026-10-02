# Viewer perf results, 2026-10-02

The impl branch is now faster than `main`. `impl-glyphview`, the branch tip at `061a544c4`, beats
`main` on 26 of 63 rows and is slower on 3. Its main-thread JavaScript time is 10 to 56% lower
on every flow the A/A check judges (all but `coldScroll`): opening `large.pdf` runs 162 ms of it
against 284. `searchLarge` finishes in 1599 ms against 1908, and `open(ebook)` settles in 316 ms against 395. The cost is 4 to 5% on
`openedMs` for both fixtures and 13 ms on the first zoom step, plus 2.50 MiB of engine wasm
(0.70 MiB brotli). `impl-ts`, the old impl pin `c8085bd3c`, differs from `main` only on opening,
3 to 6% slower.

The plugins kernel still costs time everywhere. `plugins-sharedhits` is the plugins branch tip at
`b450a9909`, which holds every kept kernel change, the glyph view and the optimized engine link.
It is slower than `main` on 41 of 63 rows and faster on none. On the rows a user waits for, it
settles in the same time as `main` on `select`, `selectLarge`, `search`, `annotate`,
`annotateLarge` and `zoomLarge`, and scrolls with `main`'s frame times and no long tasks. Opening
is 1.44 to 1.58x (`large.pdf` opens 85 ms later and settles 152 ms later), a jump settles 51 ms
later, a zoom 58 ms later, and `searchLarge` takes 1.10x. Main-thread task and JavaScript time is
a median 1.80x `main`'s, worst on `searchLarge` at 3.64x JavaScript. The kernel is 7.94 MiB of
wasm (1.71 MiB brotli) and the branch ships 0.74 MiB less JavaScript.

`plugins-lto` is the plugins branch before any kernel fix (`bef223d9d`), built with LTO. It
failed `selectLarge` ("the drag selected no text") and `annotateLarge` ("timed out waiting for
the highlight to be added") in all 10 rounds, probably because page 200 of `large.pdf` doesn't
answer within the flows' 10 s waits. That is unchecked. Its `searchLarge` took 429 s.

This is one `desktop` run: 10 rounds on an Apple M1 (`macmini2020`, timed over SSH, alone on the
host), on AC power, harness at `da586b19f`. It took 185.0 min and recorded 840 samples; the 20
from `plugins-lto` above failed. The A/A pair passed 62 of 63 rows; `coldScroll / mainJsMs`
failed it (0.963 against a 3% band), so that row isn't judged. The 13 flows include five new
since the last run: `coldScroll`, `zoomLarge`, `selectLarge`, `searchLarge` and
`annotateLarge`. The previous results, from an Apple M2 Max, are in this file's history at
`496088649`; the two machines' numbers aren't comparable.

## All arms, median

Times are ms. The last two columns are `impl-glyphview` and `plugins-sharedhits` against `main`
from the report below: a ratio for time and CPU rows, a difference in ms for settle, jank and
long-task rows. "same" is no detectable difference; "unreliable" means the row failed the A/A
check; "failed" means every sample failed. Memory rows show the plain ratio of medians, which the
report doesn't judge. `main-aa` is left out; it is in the report's A/A section.

| Row | `main` | `impl-ts` | `impl-glyphview` | `plugins-lto` | `plugins-sharedhits` | glyphview vs main | sharedhits vs main |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| open(ebook) / readyMs | 62 | 62 | 61 | 108 | 95 | same | 1.521 slower |
| open(ebook) / openedMs | 136 | 144 | 142 | 266 | 216 | 1.040 slower | 1.582 slower |
| open(ebook) / firstPaintMs | 255 | 262 | 261 | 513 | 380 | same | 1.488 slower |
| open(ebook) / settledMs | 395 | 409 | 316 | 787 | 437 | 0.799 faster | 1.104 slower |
| open(ebook) / mainTaskMs | 144 | 144 | 131 | 609 | 294 | 0.915 faster | 2.048 slower |
| open(ebook) / mainJsMs | 111 | 111 | 99 | 555 | 244 | 0.895 faster | 2.206 slower |
| open(large) / readyMs | 63 | 63 | 62 | 103 | 91 | same | 1.440 slower |
| open(large) / openedMs | 158 | 164 | 167 | 1992 | 243 | 1.052 slower | 1.537 slower |
| open(large) / firstPaintMs | 271 | 274 | 276 | 3199 | 423 | same | 1.564 slower |
| open(large) / settledMs | 271 | 276 | 275 | 3298 | 423 | same | 1.563 slower |
| open(large) / mainTaskMs | 315 | 315 | 196 | 3607 | 579 | 0.621 faster | 1.839 slower |
| open(large) / mainJsMs | 284 | 285 | 162 | 3560 | 532 | 0.572 faster | 1.875 slower |
| scroll / settleMs | 0.0 | 0.0 | 0.0 | 184 | 0.0 | same | same |
| scroll / wallMs | 2916 | 2915 | 2887 | 21928 | 3047 | same | 1.045 slower |
| scroll / frameP50Ms | 11 | 11 | 11 | 64 | 11 | same | same |
| scroll / frameP95Ms | 26 | 25 | 25 | 287 | 26 | same | same |
| scroll / jankFrames | 0 | 0 | 0 | 159 | 0 | same | same |
| scroll / longTasks | 0 | 0 | 0 | 223 | 0 | same | same |
| scroll / longTaskMs | 0.0 | 0.0 | 0.0 | 22273 | 0.0 | same | same |
| scroll / mainTaskMs | 544 | 536 | 476 | 22469 | 907 | 0.875 faster | 1.667 slower |
| scroll / mainJsMs | 404 | 401 | 321 | 22246 | 767 | 0.794 faster | 1.898 slower |
| coldScroll / settleMs | 0.0 | 0.0 | 0.0 | 228 | 0.0 | same | same |
| coldScroll / wallMs | 2879 | 2861 | 2867 | 21076 | 3066 | same | 1.065 slower |
| coldScroll / frameP50Ms | 11 | 11 | 11 | 62 | 11 | same | same |
| coldScroll / frameP95Ms | 24 | 25 | 25 | 272 | 27 | same | same |
| coldScroll / jankFrames | 0 | 0 | 0 | 151 | 0 | same | same |
| coldScroll / longTasks | 0 | 0 | 0 | 215 | 0 | same | same |
| coldScroll / longTaskMs | 0.0 | 0.0 | 0.0 | 21158 | 0.0 | same | same |
| coldScroll / mainTaskMs | 569 | 564 | 497 | 21586 | 906 | 0.874 faster | 1.592 slower |
| coldScroll / mainJsMs | 399 | 393 | 330 | 21353 | 750 | unreliable | unreliable |
| jump / settleMs | 125 | 125 | 118 | 1510 | 177 | same | +51.2 slower |
| jump / mainTaskMs | 72 | 73 | 67 | 1857 | 195 | same | 2.695 slower |
| jump / mainJsMs | 54 | 54 | 45 | 1696 | 178 | 0.827 faster | 3.282 slower |
| zoom / settleMs | 485 | 492 | 480 | 795 | 543 | same | +57.7 slower |
| zoom / zoom2SettleMs | 165 | 166 | 178 | 358 | 195 | +13.2 slower | +30.3 slower |
| zoom / zoom4SettleMs | 319 | 326 | 303 | 436 | 354 | same | +34.4 slower |
| zoom / backTo1SettleMs | 87 | 92 | 80 | 166 | 108 | same | +21.2 slower |
| zoom / mainTaskMs | 223 | 226 | 186 | 1234 | 332 | 0.835 faster | 1.487 slower |
| zoom / mainJsMs | 161 | 163 | 126 | 1081 | 275 | 0.778 faster | 1.703 slower |
| zoomLarge / settleMs | 312 | 292 | 310 | 3379 | 316 | same | same |
| zoomLarge / zoom2SettleMs | 142 | 142 | 139 | 1670 | 151 | same | same |
| zoomLarge / zoom4SettleMs | 164 | 155 | 172 | 1725 | 172 | same | same |
| zoomLarge / backTo1SettleMs | 72 | 69 | 74 | 1027 | 87 | same | same |
| zoomLarge / mainTaskMs | 174 | 179 | 163 | 5432 | 261 | 0.937 faster | 1.498 slower |
| zoomLarge / mainJsMs | 130 | 133 | 117 | 4886 | 219 | 0.902 faster | 1.686 slower |
| select / settleMs | 220 | 215 | 227 | 500 | 237 | same | same |
| select / mainTaskMs | 62 | 62 | 52 | 367 | 82 | 0.833 faster | 1.324 slower |
| select / mainJsMs | 40 | 40 | 28 | 351 | 65 | 0.709 faster | 1.620 slower |
| selectLarge / settleMs | 212 | 213 | 205 | failed | 217 | same | same |
| selectLarge / mainTaskMs | 68 | 70 | 56 | failed | 107 | 0.818 faster | 1.566 slower |
| selectLarge / mainJsMs | 46 | 47 | 33 | failed | 83 | 0.718 faster | 1.819 slower |
| search / searchMs | 71 | 74 | 67 | 108 | 82 | same | same |
| search / mainTaskMs | 5.3 | 5.8 | 3.2 | 47 | 18 | 0.597 faster | 3.357 slower |
| search / mainJsMs | 3.7 | 3.7 | 1.6 | 34 | 8.5 | 0.440 faster | 2.315 slower |
| searchLarge / searchMs | 1908 | 1911 | 1599 | 429403 | 2095 | 0.838 faster | 1.098 slower |
| searchLarge / mainTaskMs | 347 | 348 | 298 | 429436 | 1115 | 0.860 faster | 3.215 slower |
| searchLarge / mainJsMs | 292 | 293 | 243 | 429246 | 1065 | 0.830 faster | 3.642 slower |
| annotate / settleMs | 230 | 243 | 218 | 561 | 215 | same | same |
| annotate / mainTaskMs | 80 | 80 | 70 | 442 | 102 | 0.877 faster | 1.279 slower |
| annotate / mainJsMs | 48 | 48 | 36 | 417 | 75 | 0.762 faster | 1.566 slower |
| annotateLarge / settleMs | 218 | 211 | 208 | failed | 225 | same | same |
| annotateLarge / mainTaskMs | 80 | 81 | 71 | failed | 127 | 0.888 faster | 1.591 slower |
| annotateLarge / mainJsMs | 47 | 48 | 38 | failed | 84 | 0.808 faster | 1.785 slower |
| open(ebook) / heapAfterOpenBytes | 35.35 MiB | 35.53 MiB | 35.91 MiB | 44.28 MiB | 42.34 MiB | 1.02x | 1.20x |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB | 23.00 MiB | 23.02 MiB | 23.03 MiB | 23.02 MiB | 1.03x | 1.03x |
| open(large) / heapAfterOpenBytes | 48.15 MiB | 48.86 MiB | 48.59 MiB | 58.03 MiB | 54.96 MiB | 1.01x | 1.14x |
| open(large) / workerHeapAfterOpenBytes | 22.26 MiB | 23.01 MiB | 23.01 MiB | 22.94 MiB | 22.96 MiB | 1.03x | 1.03x |
| soak / heapCycle5Bytes | 37.92 MiB | 38.70 MiB | 39.05 MiB | 50.00 MiB | 45.14 MiB | 1.03x | 1.19x |
| soak / heapCycle30Bytes | 38.76 MiB | 39.53 MiB | 39.77 MiB | 64.04 MiB | 45.68 MiB | 1.03x | 1.18x |
| soak / heapSoakSlopeBytes | 0.03 MiB | 0.03 MiB | 0.03 MiB | 0.56 MiB | 0.02 MiB | 0.84x | 0.67x |

# Viewer perf comparison: 2026-10-02T12-56-31-247Z-macmini2020-desktop

10 rounds, profile `desktop` (CPU throttle 1x).

`workerTaskMs` is n/a: Chromium has no Performance domain on dedicated workers.

## Environment

| Field | Value |
| --- | --- |
| Harness commit | da586b19f17e10619ed4a98bf4d22347f3340549 |
| Chromium / Playwright | 140.0.7339.16 / 1.55.0 |
| Node | v24.20.0 |
| OS | Darwin 27.2.0 arm64 |
| CPU | Apple M1, 8 cores |
| Memory | 16 GiB |
| Power | AC Power |
| Viewport | 1280x800 @2x |
| Fixtures | ebook 2e539f62b5fa, large 10c8668fe55c |

| Arm | Ref | SHA | Payload | Engine wasm sha256 | Kernel wasm sha256 |
| --- | --- | --- | --- | --- | --- |
| main | main | 2516e2786 | fetched | 98c2368a774d |  |
| main-aa | main | 2516e2786 | fetched | 98c2368a774d |  |
| impl-ts | research/multi-platform-rust-core-impl | c8085bd3c | built (fork d045f389d, wasm32-eh) | 0f7407599d93 |  |
| plugins-lto | research/multi-platform-rust-core-impl-plugins | bef223d9d | built (fork d045f389d, wasm32-eh) | 0f7407599d93 | ce0b35ed118c |
| impl-glyphview | research/rust-core-glyph-view | 061a544c4 | built (fork 7c7b600b1, wasm32-eh) | c888920f5d27 |  |
| plugins-sharedhits | research/rust-kernel-shared-hits | b450a9909 | built (fork 7c7b600b1, wasm32-eh) | c888920f5d27 | 5f383fbcd10b |

## Gate

- `main`: pass
- `main-aa`: pass
- `impl-ts`: pass
- `plugins-lto`: pass
- `impl-glyphview`: pass
- `plugins-sharedhits`: pass

20 samples failed and are left out:

- plugins-lto selectLarge(large): the drag selected no text (x10)
- plugins-lto annotateLarge(large): timed out waiting for the highlight to be added (x10)

## Sizes

| Metric | main | main-aa | impl-ts | plugins-lto | impl-glyphview | plugins-sharedhits |
| --- | --- | --- | --- | --- | --- | --- |
| engineWasmBytes | 7.19 MiB | 7.19 MiB | 10.57 MiB | 10.57 MiB | 9.69 MiB | 9.69 MiB |
| engineWasmBrotliBytes | 2.15 MiB | 2.15 MiB | 2.83 MiB | 2.83 MiB | 2.85 MiB | 2.85 MiB |
| kernelWasmBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 11.68 MiB | 0.00 MiB | 7.94 MiB |
| kernelWasmBrotliBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 1.88 MiB | 0.00 MiB | 1.71 MiB |
| jsBytes | 3.37 MiB | 3.37 MiB | 3.35 MiB | 2.62 MiB | 3.36 MiB | 2.63 MiB |
| jsBrotliBytes | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.80 MiB | 0.96 MiB | 0.80 MiB |

## A/A: `main` vs `main-aa`

A metric passes when its interval includes the null value or its point estimate is within 3% of it. Its interval then sets the noise band every other pair must clear.

62 of 63 rows pass.

| Row | main median [p95] | main-aa median [p95] | ratio or diff [95% CI] | A/A |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 62.2 [63.6] | 61.4 [64.6] | 0.988 [0.962, 1.035] | pass |
| open(ebook) / openedMs | 136.3 [138.9] | 136.8 [137.9] | 1.003 [0.977, 1.016] | pass |
| open(ebook) / firstPaintMs | 255.5 [269.9] | 254.3 [264.4] | 0.995 [0.976, 1.028] | pass |
| open(ebook) / settledMs | 395.5 [399.7] | 395.5 [398.3] | 1.000 [0.990, 1.010] | pass |
| open(ebook) / mainTaskMs | 143.5 [146.8] | 143.4 [146.1] | 0.999 [0.984, 1.017] | pass |
| open(ebook) / mainJsMs | 110.6 [113.8] | 110.6 [113.5] | 1.000 [0.979, 1.016] | pass |
| open(large) / readyMs | 62.9 [64.3] | 62.2 [65.8] | 0.990 [0.954, 1.041] | pass |
| open(large) / openedMs | 158.3 [162.2] | 158.4 [159.4] | 1.001 [0.979, 1.014] | pass |
| open(large) / firstPaintMs | 270.6 [285.4] | 267.1 [270.0] | 0.987 [0.966, 1.011] | pass |
| open(large) / settledMs | 270.6 [278.2] | 272.1 [283.5] | 1.006 [0.981, 1.029] | pass |
| open(large) / mainTaskMs | 314.9 [319.8] | 315.8 [319.7] | 1.003 [0.993, 1.014] | pass |
| open(large) / mainJsMs | 283.8 [289.7] | 284.4 [288.3] | 1.002 [0.993, 1.013] | pass |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / wallMs | 2916.2 [2964.4] | 2884.1 [2947.4] | 0.989 [0.968, 1.005] | pass |
| scroll / frameP50Ms | 11.1 [11.6] | 11.4 [11.8] | 1.024 [0.975, 1.040] | pass |
| scroll / frameP95Ms | 25.6 [27.2] | 26.0 [27.3] | 1.019 [0.912, 1.163] | pass |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / mainTaskMs | 544.3 [557.7] | 532.0 [559.8] | 0.977 [0.960, 1.011] | pass |
| scroll / mainJsMs | 404.2 [411.6] | 397.6 [406.6] | 0.984 [0.954, 1.007] | pass |
| coldScroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| coldScroll / wallMs | 2879.1 [2931.3] | 2840.3 [2885.3] | 0.987 [0.962, 1.009] | pass |
| coldScroll / frameP50Ms | 11.3 [11.8] | 11.2 [11.9] | 0.998 [0.948, 1.052] | pass |
| coldScroll / frameP95Ms | 24.0 [26.7] | 25.6 [27.2] | 1.067 [0.947, 1.152] | pass |
| coldScroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| coldScroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| coldScroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| coldScroll / mainTaskMs | 569.0 [582.4] | 553.9 [570.2] | 0.973 [0.954, 0.994] | pass |
| coldScroll / mainJsMs | 398.7 [406.5] | 384.0 [398.7] | 0.963 [0.953, 0.990] | A/A failed |
| jump / settleMs | 125.5 [141.0] | 123.4 [136.0] | -2.1 [-14.1, +2.7] | pass |
| jump / mainTaskMs | 72.2 [77.2] | 73.1 [77.9] | 1.013 [0.975, 1.079] | pass |
| jump / mainJsMs | 54.4 [58.3] | 56.6 [60.6] | 1.041 [0.976, 1.091] | pass |
| zoom / settleMs | 485.5 [514.0] | 495.2 [511.8] | +9.7 [-12.2, +31.4] | pass |
| zoom / zoom2SettleMs | 164.7 [169.5] | 165.0 [172.0] | +0.3 [-6.1, +4.8] | pass |
| zoom / zoom4SettleMs | 319.4 [350.2] | 329.3 [341.8] | +9.9 [-9.1, +32.1] | pass |
| zoom / backTo1SettleMs | 87.0 [108.9] | 93.0 [109.8] | +6.0 [-3.1, +19.4] | pass |
| zoom / mainTaskMs | 223.2 [230.9] | 227.8 [233.7] | 1.021 [0.986, 1.048] | pass |
| zoom / mainJsMs | 161.3 [166.8] | 165.4 [169.1] | 1.025 [0.983, 1.043] | pass |
| zoomLarge / settleMs | 311.9 [342.8] | 302.5 [311.4] | -9.4 [-31.1, +9.0] | pass |
| zoomLarge / zoom2SettleMs | 142.0 [167.6] | 139.7 [161.5] | -2.3 [-22.8, +14.4] | pass |
| zoomLarge / zoom4SettleMs | 163.9 [188.1] | 158.0 [176.5] | -6.0 [-20.9, +4.8] | pass |
| zoomLarge / backTo1SettleMs | 72.1 [92.6] | 77.4 [97.9] | +5.3 [-5.8, +15.2] | pass |
| zoomLarge / mainTaskMs | 174.4 [180.6] | 174.8 [181.5] | 1.003 [0.985, 1.049] | pass |
| zoomLarge / mainJsMs | 129.7 [134.3] | 130.8 [135.6] | 1.009 [0.983, 1.071] | pass |
| select / settleMs | 220.4 [236.8] | 221.0 [241.2] | +0.5 [-20.6, +19.3] | pass |
| select / mainTaskMs | 61.9 [65.0] | 61.4 [65.0] | 0.992 [0.941, 1.036] | pass |
| select / mainJsMs | 40.1 [42.8] | 39.9 [42.8] | 0.995 [0.941, 1.039] | pass |
| selectLarge / settleMs | 212.1 [246.1] | 208.4 [236.1] | -3.7 [-44.5, +26.4] | pass |
| selectLarge / mainTaskMs | 68.0 [77.6] | 72.1 [75.4] | 1.059 [0.935, 1.168] | pass |
| selectLarge / mainJsMs | 45.7 [53.6] | 48.8 [50.6] | 1.067 [0.929, 1.188] | pass |
| search / searchMs | 71.0 [74.7] | 72.6 [83.2] | 1.023 [0.976, 1.151] | pass |
| search / mainTaskMs | 5.3 [5.7] | 5.4 [6.7] | 1.016 [0.954, 1.122] | pass |
| search / mainJsMs | 3.7 [3.9] | 3.7 [4.5] | 1.016 [0.962, 1.086] | pass |
| searchLarge / searchMs | 1907.8 [2025.2] | 1900.2 [1945.5] | 0.996 [0.977, 1.005] | pass |
| searchLarge / mainTaskMs | 346.9 [349.4] | 346.7 [349.3] | 0.999 [0.993, 1.007] | pass |
| searchLarge / mainJsMs | 292.5 [293.9] | 292.6 [294.7] | 1.001 [0.997, 1.005] | pass |
| annotate / settleMs | 229.6 [253.6] | 226.6 [265.6] | -2.9 [-37.4, +31.5] | pass |
| annotate / mainTaskMs | 79.8 [83.2] | 77.7 [86.3] | 0.973 [0.937, 1.033] | pass |
| annotate / mainJsMs | 47.9 [50.5] | 46.4 [52.2] | 0.969 [0.926, 1.047] | pass |
| annotateLarge / settleMs | 218.5 [263.7] | 204.6 [244.6] | -13.9 [-60.5, +25.2] | pass |
| annotateLarge / mainTaskMs | 79.7 [88.5] | 78.0 [85.1] | 0.978 [0.898, 1.035] | pass |
| annotateLarge / mainJsMs | 47.2 [53.1] | 47.5 [51.1] | 1.007 [0.913, 1.038] | pass |

## `main` vs `impl-ts`

| Row | main median [p95] | impl-ts median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 62.2 [63.6] | 61.8 [69.5] | 0.994 [0.970, 1.101] | no detectable difference |
| open(ebook) / openedMs | 136.3 [138.9] | 144.3 [149.9] | 1.059 [1.028, 1.095] | slower |
| open(ebook) / firstPaintMs | 255.5 [269.9] | 262.5 [276.2] | 1.027 [1.001, 1.063] | no detectable difference |
| open(ebook) / settledMs | 395.5 [399.7] | 408.9 [413.8] | 1.034 [1.019, 1.046] | slower |
| open(ebook) / mainTaskMs | 143.5 [146.8] | 144.3 [149.6] | 1.006 [0.984, 1.035] | no detectable difference |
| open(ebook) / mainJsMs | 110.6 [113.8] | 110.6 [114.8] | 1.000 [0.976, 1.031] | no detectable difference |
| open(large) / readyMs | 62.9 [64.3] | 62.6 [74.4] | 0.995 [0.958, 1.081] | no detectable difference |
| open(large) / openedMs | 158.3 [162.2] | 164.3 [175.0] | 1.038 [1.015, 1.073] | slower |
| open(large) / firstPaintMs | 270.6 [285.4] | 273.7 [297.3] | 1.012 [0.982, 1.057] | no detectable difference |
| open(large) / settledMs | 270.6 [278.2] | 276.4 [285.4] | 1.021 [0.990, 1.048] | no detectable difference |
| open(large) / mainTaskMs | 314.9 [319.8] | 315.2 [318.3] | 1.001 [0.992, 1.014] | no detectable difference |
| open(large) / mainJsMs | 283.8 [289.7] | 284.8 [306.7] | 1.004 [0.993, 1.016] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2916.2 [2964.4] | 2915.3 [2972.1] | 1.000 [0.984, 1.022] | no detectable difference |
| scroll / frameP50Ms | 11.1 [11.6] | 11.1 [11.6] | 1.002 [0.963, 1.022] | no detectable difference |
| scroll / frameP95Ms | 25.6 [27.2] | 25.5 [27.0] | 0.997 [0.924, 1.137] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 544.3 [557.7] | 536.0 [552.3] | 0.985 [0.970, 1.018] | no detectable difference |
| scroll / mainJsMs | 404.2 [411.6] | 401.2 [408.3] | 0.992 [0.976, 1.017] | no detectable difference |
| coldScroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| coldScroll / wallMs | 2879.1 [2931.3] | 2861.4 [2951.6] | 0.994 [0.983, 1.022] | no detectable difference |
| coldScroll / frameP50Ms | 11.3 [11.8] | 11.2 [11.7] | 0.992 [0.964, 1.032] | no detectable difference |
| coldScroll / frameP95Ms | 24.0 [26.7] | 24.9 [27.2] | 1.036 [0.938, 1.121] | no detectable difference |
| coldScroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| coldScroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| coldScroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| coldScroll / mainTaskMs | 569.0 [582.4] | 564.1 [571.1] | 0.991 [0.974, 1.002] | no detectable difference |
| coldScroll / mainJsMs | 398.7 [406.5] | 392.7 [403.8] | 0.985 [0.972, 1.005] | unreliable (A/A failed) |
| jump / settleMs | 125.5 [141.0] | 125.3 [131.8] | -0.2 [-11.8, +4.4] | no detectable difference |
| jump / mainTaskMs | 72.2 [77.2] | 72.6 [77.2] | 1.006 [0.976, 1.068] | no detectable difference |
| jump / mainJsMs | 54.4 [58.3] | 54.5 [58.3] | 1.003 [0.952, 1.076] | no detectable difference |
| zoom / settleMs | 485.5 [514.0] | 492.4 [514.0] | +6.9 [-9.3, +29.2] | no detectable difference |
| zoom / zoom2SettleMs | 164.7 [169.5] | 165.8 [170.2] | +1.0 [-0.6, +3.2] | no detectable difference |
| zoom / zoom4SettleMs | 319.4 [350.2] | 326.0 [344.6] | +6.6 [-10.5, +30.5] | no detectable difference |
| zoom / backTo1SettleMs | 87.0 [108.9] | 92.2 [111.0] | +5.2 [-4.7, +19.1] | no detectable difference |
| zoom / mainTaskMs | 223.2 [230.9] | 225.5 [238.0] | 1.010 [0.959, 1.033] | no detectable difference |
| zoom / mainJsMs | 161.3 [166.8] | 163.5 [171.6] | 1.013 [0.967, 1.029] | no detectable difference |
| zoomLarge / settleMs | 311.9 [342.8] | 292.1 [352.7] | -19.8 [-40.8, +15.8] | no detectable difference |
| zoomLarge / zoom2SettleMs | 142.0 [167.6] | 141.5 [172.1] | -0.5 [-25.3, +13.9] | no detectable difference |
| zoomLarge / zoom4SettleMs | 163.9 [188.1] | 154.9 [192.5] | -9.1 [-28.3, +11.3] | no detectable difference |
| zoomLarge / backTo1SettleMs | 72.1 [92.6] | 69.1 [91.3] | -3.0 [-12.8, +5.5] | no detectable difference |
| zoomLarge / mainTaskMs | 174.4 [180.6] | 178.6 [184.9] | 1.024 [0.987, 1.087] | no detectable difference |
| zoomLarge / mainJsMs | 129.7 [134.3] | 133.5 [136.4] | 1.029 [0.980, 1.092] | no detectable difference |
| select / settleMs | 220.4 [236.8] | 214.6 [254.8] | -5.8 [-24.8, +27.4] | no detectable difference |
| select / mainTaskMs | 61.9 [65.0] | 62.2 [69.0] | 1.004 [0.956, 1.061] | no detectable difference |
| select / mainJsMs | 40.1 [42.8] | 40.4 [45.2] | 1.009 [0.945, 1.067] | no detectable difference |
| selectLarge / settleMs | 212.1 [246.1] | 213.0 [257.8] | +0.9 [-34.8, +42.2] | no detectable difference |
| selectLarge / mainTaskMs | 68.0 [77.6] | 69.5 [79.2] | 1.022 [0.902, 1.131] | no detectable difference |
| selectLarge / mainJsMs | 45.7 [53.6] | 46.7 [52.6] | 1.021 [0.880, 1.153] | no detectable difference |
| search / searchMs | 71.0 [74.7] | 74.0 [81.8] | 1.042 [0.991, 1.130] | no detectable difference |
| search / mainTaskMs | 5.3 [5.7] | 5.8 [6.3] | 1.097 [0.966, 1.177] | no detectable difference |
| search / mainJsMs | 3.7 [3.9] | 3.7 [4.0] | 0.998 [0.968, 1.053] | no detectable difference |
| searchLarge / searchMs | 1907.8 [2025.2] | 1911.2 [1926.5] | 1.002 [0.982, 1.006] | no detectable difference |
| searchLarge / mainTaskMs | 346.9 [349.4] | 348.2 [352.5] | 1.004 [0.997, 1.012] | no detectable difference |
| searchLarge / mainJsMs | 292.5 [293.9] | 293.4 [296.3] | 1.003 [0.999, 1.010] | no detectable difference |
| annotate / settleMs | 229.6 [253.6] | 243.3 [274.1] | +13.7 [-41.3, +57.9] | no detectable difference |
| annotate / mainTaskMs | 79.8 [83.2] | 80.0 [88.5] | 1.002 [0.923, 1.068] | no detectable difference |
| annotate / mainJsMs | 47.9 [50.5] | 48.0 [53.6] | 1.003 [0.905, 1.052] | no detectable difference |
| annotateLarge / settleMs | 218.5 [263.7] | 211.1 [253.1] | -7.4 [-44.6, +34.5] | no detectable difference |
| annotateLarge / mainTaskMs | 79.7 [88.5] | 80.6 [87.0] | 1.011 [0.914, 1.052] | no detectable difference |
| annotateLarge / mainJsMs | 47.2 [53.1] | 48.4 [52.6] | 1.025 [0.914, 1.073] | no detectable difference |

## `impl-ts` vs `impl-rust`

Not run: `impl-rust`.

## `impl-ts` vs `plugins`

Not run: `plugins`.

## `main` vs `plugins`

Not run: `plugins`.

## `plugins` vs `plugins-lto`

Not run: `plugins`.

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

Not run: `plugins-wire`, `plugins-getstate`.

## `plugins-lto` vs `plugins-getstate`

Not run: `plugins-getstate`.

## `main` vs `plugins-getstate`

Not run: `plugins-getstate`.

## `plugins-getstate` vs `plugins-toolbar`

Not run: `plugins-getstate`, `plugins-toolbar`.

## `main` vs `plugins-toolbar`

Not run: `plugins-toolbar`.

## `plugins-toolbar` vs `plugins-bump`

Not run: `plugins-toolbar`, `plugins-bump`.

## `main` vs `plugins-bump`

Not run: `plugins-bump`.

## `plugins-getstate` vs `plugins-item1`

Not run: `plugins-getstate`, `plugins-item1`.

## `plugins-getstate` vs `plugins-item3`

Not run: `plugins-getstate`, `plugins-item3`.

## `plugins-getstate` vs `plugins-bump`

Not run: `plugins-getstate`, `plugins-bump`.

## `plugins-item1` vs `plugins-bump`

Not run: `plugins-item1`, `plugins-bump`.

## `plugins-bump` vs `plugins-reads`

Not run: `plugins-bump`, `plugins-reads`.

## `main` vs `plugins-reads`

Not run: `plugins-reads`.

## `impl-ts` vs `impl-gate`

Not run: `impl-gate`.

## `plugins-head` vs `plugins-buildlto`

Not run: `plugins-head`, `plugins-buildlto`.

## `plugins-head` vs `plugins-wasmopt`

Not run: `plugins-head`, `plugins-wasmopt`.

## `plugins-bump` vs `plugins-buildlto`

Not run: `plugins-bump`, `plugins-buildlto`.

## `impl-head` vs `impl-enginelink`

Not run: `impl-head`, `impl-enginelink`.

## `plugins-tip` vs `plugins-enginelink`

Not run: `plugins-tip`, `plugins-enginelink`.

## `impl-head` vs `impl-fixed`

Not run: `impl-head`, `impl-fixed`.

## `plugins-tip` vs `plugins-fixed`

Not run: `plugins-tip`, `plugins-fixed`.

## `impl-fixed` vs `impl-enginelink`

Not run: `impl-fixed`, `impl-enginelink`.

## `plugins-fixed` vs `plugins-enginelink`

Not run: `plugins-fixed`, `plugins-enginelink`.

## `impl-fixed` vs `impl-o2`

Not run: `impl-fixed`, `impl-o2`.

## `impl-o2` vs `impl-enginelink`

Not run: `impl-o2`, `impl-enginelink`.

## `plugins-fixed` vs `plugins-o2`

Not run: `plugins-fixed`, `plugins-o2`.

## `plugins-o2` vs `plugins-enginelink`

Not run: `plugins-o2`, `plugins-enginelink`.

## `main` vs `main-compat`

Not run: `main-compat`.

## `impl-fixed` vs `impl-compat`

Not run: `impl-fixed`, `impl-compat`.

## `plugins-fixed` vs `plugins-compat`

Not run: `plugins-fixed`, `plugins-compat`.

## `plugins-now` vs `plugins-slicetext`

Not run: `plugins-now`, `plugins-slicetext`.

## `main` vs `plugins-slicetext`

Not run: `plugins-slicetext`.

## `impl-now` vs `impl-glyphview`

Not run: `impl-now`.

## `plugins-tip2` vs `plugins-glyphview`

Not run: `plugins-tip2`, `plugins-glyphview`.

## `main` vs `impl-glyphview`

| Row | main median [p95] | impl-glyphview median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 62.2 [63.6] | 60.8 [65.6] | 0.978 [0.964, 1.019] | no detectable difference |
| open(ebook) / openedMs | 136.3 [138.9] | 141.7 [147.4] | 1.040 [1.024, 1.068] | slower |
| open(ebook) / firstPaintMs | 255.5 [269.9] | 260.5 [273.8] | 1.020 [0.997, 1.053] | no detectable difference |
| open(ebook) / settledMs | 395.5 [399.7] | 316.1 [323.0] | 0.799 [0.788, 0.809] | faster |
| open(ebook) / mainTaskMs | 143.5 [146.8] | 131.4 [136.6] | 0.915 [0.900, 0.933] | faster |
| open(ebook) / mainJsMs | 110.6 [113.8] | 99.0 [102.0] | 0.895 [0.876, 0.907] | faster |
| open(large) / readyMs | 62.9 [64.3] | 62.5 [63.9] | 0.994 [0.966, 1.022] | no detectable difference |
| open(large) / openedMs | 158.3 [162.2] | 166.6 [171.1] | 1.052 [1.031, 1.071] | slower |
| open(large) / firstPaintMs | 270.6 [285.4] | 276.5 [281.7] | 1.022 [0.969, 1.047] | no detectable difference |
| open(large) / settledMs | 270.6 [278.2] | 275.4 [281.7] | 1.018 [0.998, 1.042] | no detectable difference |
| open(large) / mainTaskMs | 314.9 [319.8] | 195.6 [197.8] | 0.621 [0.613, 0.628] | faster |
| open(large) / mainJsMs | 283.8 [289.7] | 162.3 [163.8] | 0.572 [0.565, 0.577] | faster |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2916.2 [2964.4] | 2887.2 [2985.2] | 0.990 [0.972, 1.015] | no detectable difference |
| scroll / frameP50Ms | 11.1 [11.6] | 11.1 [11.5] | 1.000 [0.956, 1.024] | no detectable difference |
| scroll / frameP95Ms | 25.6 [27.2] | 24.8 [27.2] | 0.970 [0.838, 1.143] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 544.3 [557.7] | 476.3 [489.4] | 0.875 [0.854, 0.902] | faster |
| scroll / mainJsMs | 404.2 [411.6] | 321.0 [331.5] | 0.794 [0.776, 0.811] | faster |
| coldScroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| coldScroll / wallMs | 2879.1 [2931.3] | 2867.4 [2955.0] | 0.996 [0.984, 1.030] | no detectable difference |
| coldScroll / frameP50Ms | 11.3 [11.8] | 11.2 [11.8] | 0.996 [0.960, 1.047] | no detectable difference |
| coldScroll / frameP95Ms | 24.0 [26.7] | 25.2 [27.1] | 1.050 [0.935, 1.106] | no detectable difference |
| coldScroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| coldScroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| coldScroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| coldScroll / mainTaskMs | 569.0 [582.4] | 497.4 [510.7] | 0.874 [0.859, 0.889] | faster |
| coldScroll / mainJsMs | 398.7 [406.5] | 330.0 [337.7] | 0.828 [0.810, 0.845] | unreliable (A/A failed) |
| jump / settleMs | 125.5 [141.0] | 118.0 [131.3] | -7.5 [-21.7, -1.2] | no detectable difference |
| jump / mainTaskMs | 72.2 [77.2] | 66.8 [74.0] | 0.925 [0.893, 0.983] | no detectable difference |
| jump / mainJsMs | 54.4 [58.3] | 45.0 [49.0] | 0.827 [0.787, 0.870] | faster |
| zoom / settleMs | 485.5 [514.0] | 480.3 [517.9] | -5.2 [-24.2, +20.9] | no detectable difference |
| zoom / zoom2SettleMs | 164.7 [169.5] | 177.9 [186.9] | +13.2 [+4.7, +19.3] | slower |
| zoom / zoom4SettleMs | 319.4 [350.2] | 303.1 [337.0] | -16.3 [-32.9, +8.2] | no detectable difference |
| zoom / backTo1SettleMs | 87.0 [108.9] | 80.2 [91.7] | -6.8 [-18.0, +1.9] | no detectable difference |
| zoom / mainTaskMs | 223.2 [230.9] | 186.4 [191.5] | 0.835 [0.806, 0.857] | faster |
| zoom / mainJsMs | 161.3 [166.8] | 125.5 [134.2] | 0.778 [0.740, 0.807] | faster |
| zoomLarge / settleMs | 311.9 [342.8] | 310.3 [335.8] | -1.6 [-21.4, +24.4] | no detectable difference |
| zoomLarge / zoom2SettleMs | 142.0 [167.6] | 139.3 [153.4] | -2.7 [-17.2, +9.5] | no detectable difference |
| zoomLarge / zoom4SettleMs | 163.9 [188.1] | 172.0 [195.6] | +8.0 [-13.5, +22.0] | no detectable difference |
| zoomLarge / backTo1SettleMs | 72.1 [92.6] | 73.9 [85.5] | +1.8 [-8.7, +6.8] | no detectable difference |
| zoomLarge / mainTaskMs | 174.4 [180.6] | 163.4 [167.4] | 0.937 [0.917, 0.991] | faster |
| zoomLarge / mainJsMs | 129.7 [134.3] | 117.0 [119.9] | 0.902 [0.882, 0.962] | faster |
| select / settleMs | 220.4 [236.8] | 226.6 [267.1] | +6.1 [-15.1, +34.7] | no detectable difference |
| select / mainTaskMs | 61.9 [65.0] | 51.6 [55.9] | 0.833 [0.792, 0.895] | faster |
| select / mainJsMs | 40.1 [42.8] | 28.4 [30.9] | 0.709 [0.678, 0.765] | faster |
| selectLarge / settleMs | 212.1 [246.1] | 204.9 [239.6] | -7.2 [-57.2, +20.7] | no detectable difference |
| selectLarge / mainTaskMs | 68.0 [77.6] | 55.7 [58.7] | 0.818 [0.743, 0.909] | faster |
| selectLarge / mainJsMs | 45.7 [53.6] | 32.8 [35.4] | 0.718 [0.646, 0.814] | faster |
| search / searchMs | 71.0 [74.7] | 67.5 [77.1] | 0.950 [0.869, 1.049] | no detectable difference |
| search / mainTaskMs | 5.3 [5.7] | 3.2 [4.0] | 0.597 [0.564, 0.647] | faster |
| search / mainJsMs | 3.7 [3.9] | 1.6 [1.7] | 0.440 [0.408, 0.454] | faster |
| searchLarge / searchMs | 1907.8 [2025.2] | 1599.0 [1620.0] | 0.838 [0.821, 0.843] | faster |
| searchLarge / mainTaskMs | 346.9 [349.4] | 298.4 [299.8] | 0.860 [0.831, 0.867] | faster |
| searchLarge / mainJsMs | 292.5 [293.9] | 242.8 [243.3] | 0.830 [0.824, 0.834] | faster |
| annotate / settleMs | 229.6 [253.6] | 218.3 [242.1] | -11.3 [-33.8, +17.2] | no detectable difference |
| annotate / mainTaskMs | 79.8 [83.2] | 70.0 [73.8] | 0.877 [0.842, 0.923] | faster |
| annotate / mainJsMs | 47.9 [50.5] | 36.5 [38.7] | 0.762 [0.709, 0.804] | faster |
| annotateLarge / settleMs | 218.5 [263.7] | 208.0 [233.5] | -10.5 [-57.0, +28.6] | no detectable difference |
| annotateLarge / mainTaskMs | 79.7 [88.5] | 70.8 [73.4] | 0.888 [0.809, 0.912] | faster |
| annotateLarge / mainJsMs | 47.2 [53.1] | 38.1 [39.9] | 0.808 [0.736, 0.846] | faster |

## `plugins-tip2` vs `plugins-lazy`

Not run: `plugins-tip2`, `plugins-lazy`.

## `main` vs `plugins-lazy`

Not run: `plugins-lazy`.

## `plugins-lazy` vs `plugins-tip3`

Not run: `plugins-lazy`, `plugins-tip3`.

## `main` vs `plugins-tip3`

Not run: `plugins-tip3`.

## `plugins-tip3` vs `plugins-sharedhits`

Not run: `plugins-tip3`.

## `main` vs `plugins-sharedhits`

| Row | main median [p95] | plugins-sharedhits median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 62.2 [63.6] | 94.6 [103.8] | 1.521 [1.495, 1.606] | slower |
| open(ebook) / openedMs | 136.3 [138.9] | 215.6 [226.0] | 1.582 [1.561, 1.652] | slower |
| open(ebook) / firstPaintMs | 255.5 [269.9] | 380.2 [421.2] | 1.488 [1.438, 1.568] | slower |
| open(ebook) / settledMs | 395.5 [399.7] | 436.8 [448.6] | 1.104 [1.084, 1.125] | slower |
| open(ebook) / mainTaskMs | 143.5 [146.8] | 294.0 [305.9] | 2.048 [2.008, 2.092] | slower |
| open(ebook) / mainJsMs | 110.6 [113.8] | 244.0 [254.1] | 2.206 [2.152, 2.250] | slower |
| open(large) / readyMs | 62.9 [64.3] | 90.6 [100.8] | 1.440 [1.410, 1.508] | slower |
| open(large) / openedMs | 158.3 [162.2] | 243.3 [255.6] | 1.537 [1.503, 1.568] | slower |
| open(large) / firstPaintMs | 270.6 [285.4] | 423.1 [443.7] | 1.564 [1.525, 1.613] | slower |
| open(large) / settledMs | 270.6 [278.2] | 422.9 [434.8] | 1.563 [1.539, 1.589] | slower |
| open(large) / mainTaskMs | 314.9 [319.8] | 579.3 [585.8] | 1.839 [1.810, 1.870] | slower |
| open(large) / mainJsMs | 283.8 [289.7] | 532.1 [539.3] | 1.875 [1.836, 1.901] | slower |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2916.2 [2964.4] | 3046.6 [3102.4] | 1.045 [1.030, 1.057] | slower |
| scroll / frameP50Ms | 11.1 [11.6] | 11.1 [11.5] | 0.997 [0.953, 1.026] | no detectable difference |
| scroll / frameP95Ms | 25.6 [27.2] | 26.0 [27.2] | 1.016 [0.952, 1.184] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 544.3 [557.7] | 907.4 [921.5] | 1.667 [1.647, 1.719] | slower |
| scroll / mainJsMs | 404.2 [411.6] | 767.1 [781.4] | 1.898 [1.872, 1.939] | slower |
| coldScroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| coldScroll / wallMs | 2879.1 [2931.3] | 3065.8 [3151.2] | 1.065 [1.041, 1.098] | slower |
| coldScroll / frameP50Ms | 11.3 [11.8] | 11.1 [11.7] | 0.985 [0.948, 1.033] | no detectable difference |
| coldScroll / frameP95Ms | 24.0 [26.7] | 26.9 [27.2] | 1.121 [1.008, 1.173] | no detectable difference |
| coldScroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| coldScroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| coldScroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| coldScroll / mainTaskMs | 569.0 [582.4] | 905.6 [925.6] | 1.592 [1.567, 1.614] | slower |
| coldScroll / mainJsMs | 398.7 [406.5] | 750.4 [767.5] | 1.882 [1.850, 1.923] | unreliable (A/A failed) |
| jump / settleMs | 125.5 [141.0] | 176.7 [211.2] | +51.2 [+30.8, +78.9] | slower |
| jump / mainTaskMs | 72.2 [77.2] | 194.6 [198.7] | 2.695 [2.585, 2.772] | slower |
| jump / mainJsMs | 54.4 [58.3] | 178.4 [181.2] | 3.282 [3.106, 3.413] | slower |
| zoom / settleMs | 485.5 [514.0] | 543.2 [573.4] | +57.7 [+43.9, +84.4] | slower |
| zoom / zoom2SettleMs | 164.7 [169.5] | 195.0 [213.5] | +30.3 [+19.8, +42.4] | slower |
| zoom / zoom4SettleMs | 319.4 [350.2] | 353.8 [370.8] | +34.4 [+14.8, +55.6] | slower |
| zoom / backTo1SettleMs | 87.0 [108.9] | 108.2 [124.0] | +21.2 [+11.6, +27.6] | slower |
| zoom / mainTaskMs | 223.2 [230.9] | 332.0 [343.4] | 1.487 [1.436, 1.519] | slower |
| zoom / mainJsMs | 161.3 [166.8] | 274.8 [289.5] | 1.703 [1.658, 1.749] | slower |
| zoomLarge / settleMs | 311.9 [342.8] | 315.7 [362.5] | +3.8 [-14.0, +37.6] | no detectable difference |
| zoomLarge / zoom2SettleMs | 142.0 [167.6] | 151.0 [171.8] | +9.0 [-7.9, +22.5] | no detectable difference |
| zoomLarge / zoom4SettleMs | 163.9 [188.1] | 172.0 [192.0] | +8.1 [-12.9, +22.5] | no detectable difference |
| zoomLarge / backTo1SettleMs | 72.1 [92.6] | 86.6 [106.2] | +14.5 [+3.1, +28.6] | no detectable difference |
| zoomLarge / mainTaskMs | 174.4 [180.6] | 261.2 [269.4] | 1.498 [1.427, 1.595] | slower |
| zoomLarge / mainJsMs | 129.7 [134.3] | 218.7 [227.2] | 1.686 [1.604, 1.809] | slower |
| select / settleMs | 220.4 [236.8] | 237.3 [265.0] | +16.9 [-8.9, +41.0] | no detectable difference |
| select / mainTaskMs | 61.9 [65.0] | 82.0 [92.0] | 1.324 [1.256, 1.427] | slower |
| select / mainJsMs | 40.1 [42.8] | 64.9 [73.4] | 1.620 [1.545, 1.780] | slower |
| selectLarge / settleMs | 212.1 [246.1] | 217.1 [234.4] | +5.0 [-31.1, +35.3] | no detectable difference |
| selectLarge / mainTaskMs | 68.0 [77.6] | 106.6 [112.2] | 1.566 [1.426, 1.753] | slower |
| selectLarge / mainJsMs | 45.7 [53.6] | 83.2 [87.3] | 1.819 [1.633, 2.066] | slower |
| search / searchMs | 71.0 [74.7] | 81.7 [84.2] | 1.151 [1.092, 1.222] | no detectable difference |
| search / mainTaskMs | 5.3 [5.7] | 17.7 [21.1] | 3.357 [2.451, 4.002] | slower |
| search / mainJsMs | 3.7 [3.9] | 8.5 [8.9] | 2.315 [2.011, 2.422] | slower |
| searchLarge / searchMs | 1907.8 [2025.2] | 2095.1 [2108.0] | 1.098 [1.076, 1.102] | slower |
| searchLarge / mainTaskMs | 346.9 [349.4] | 1115.4 [1127.4] | 3.215 [3.198, 3.238] | slower |
| searchLarge / mainJsMs | 292.5 [293.9] | 1065.1 [1078.2] | 3.642 [3.629, 3.662] | slower |
| annotate / settleMs | 229.6 [253.6] | 215.5 [248.0] | -14.1 [-44.0, +24.1] | no detectable difference |
| annotate / mainTaskMs | 79.8 [83.2] | 102.1 [107.1] | 1.279 [1.243, 1.341] | slower |
| annotate / mainJsMs | 47.9 [50.5] | 74.9 [79.9] | 1.566 [1.492, 1.636] | slower |
| annotateLarge / settleMs | 218.5 [263.7] | 225.0 [255.4] | +6.5 [-35.0, +42.3] | no detectable difference |
| annotateLarge / mainTaskMs | 79.7 [88.5] | 126.8 [129.2] | 1.591 [1.448, 1.618] | slower |
| annotateLarge / mainJsMs | 47.2 [53.1] | 84.2 [86.8] | 1.785 [1.610, 1.817] | slower |

## Memory

| Row | main median [p95] | main-aa median [p95] | impl-ts median [p95] | plugins-lto median [p95] | impl-glyphview median [p95] | plugins-sharedhits median [p95] |
| --- | --- | --- | --- | --- | --- | --- |
| open(ebook) / heapAfterOpenBytes | 35.35 MiB [35.37 MiB] | 35.34 MiB [35.35 MiB] | 35.53 MiB [36.59 MiB] | 44.28 MiB [44.30 MiB] | 35.91 MiB [35.92 MiB] | 42.34 MiB [42.90 MiB] |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB [22.27 MiB] | 22.27 MiB [22.27 MiB] | 23.00 MiB [23.00 MiB] | 23.03 MiB [23.03 MiB] | 23.02 MiB [23.02 MiB] | 23.02 MiB [23.02 MiB] |
| open(large) / heapAfterOpenBytes | 48.15 MiB [48.17 MiB] | 48.15 MiB [48.18 MiB] | 48.86 MiB [48.87 MiB] | 58.03 MiB [58.04 MiB] | 48.59 MiB [48.60 MiB] | 54.96 MiB [55.02 MiB] |
| open(large) / workerHeapAfterOpenBytes | 22.26 MiB [22.28 MiB] | 22.26 MiB [22.28 MiB] | 23.01 MiB [23.01 MiB] | 22.94 MiB [22.94 MiB] | 23.01 MiB [23.01 MiB] | 22.96 MiB [22.97 MiB] |
| soak / heapCycle5Bytes | 37.92 MiB [38.43 MiB] | 38.42 MiB [38.44 MiB] | 38.70 MiB [39.20 MiB] | 50.00 MiB [50.01 MiB] | 39.05 MiB [39.07 MiB] | 45.14 MiB [46.14 MiB] |
| soak / heapCycle30Bytes | 38.76 MiB [39.27 MiB] | 39.26 MiB [39.27 MiB] | 39.53 MiB [40.04 MiB] | 64.04 MiB [64.05 MiB] | 39.77 MiB [39.79 MiB] | 45.68 MiB [46.68 MiB] |
| soak / heapSoakSlopeBytes | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.56 MiB [0.56 MiB] | 0.03 MiB [0.03 MiB] | 0.02 MiB [0.02 MiB] |

## Diagnostics

Kernel table counts from the Rust kernel at soak cycles 5 and 30, median over rounds. Not compared: no other arm has them.

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

`plugins-sharedhits`:

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

