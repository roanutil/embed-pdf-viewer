# Viewer perf results, 2026-09-29

The Rust kernel on the plugins branch makes the viewer slower on every row we compare, in both
profiles, and by a lot. Nothing on the impl branch moves at all: `main` vs `impl-ts` and `impl-ts`
vs `impl-rust` come out "no detectable difference" on every row that passed the A/A check.

On `desktop`, `plugins` against `main` opens `large.pdf` 12.3x slower by `openedMs` (1364 ms
against 111 ms) and spends 9.7 s in long tasks during a 60-wheel scroll that `main` runs with
none. A jump to page 300 takes 871 ms longer to settle, a drag selection 146 ms longer, and a
search is 1.45x slower. The same gap holds against `impl-ts`, which runs the byte-identical engine
wasm, so the cost is the kernel and its JSON wire, not the engine. The `slow` profile widens it:
`open(large) / openedMs` is 20.3x and the scroll's long tasks reach 58.9 s. That profile throttles
the main thread only (see the README's caveats), which is where the Rust kernel runs, so it
overstates the kernel's share relative to a real slow device.

The Rust geometry reader shows nothing in the real viewer. `select / settleMs`, the flow it was
expected to move, is +1.3 ms [-3.2, +2.9] on `desktop`. Either `pages.geometry` isn't where a
selection's time goes, or the worker hop swamps the difference `tooling/engine-baseline` measures
in isolation; this harness can't tell which.

Sizes grow too, apart from JavaScript. The Rust core grows the engine wasm from 7.19 to 10.57 MiB
(2.15 to 2.83 MiB brotli) on both research branches. `plugins` adds a 10.78 MiB kernel wasm
(1.77 MiB brotli) and ships 0.75 MiB less JavaScript. It holds about 9 MiB more after open, and its
heap grows 0.56 MiB per soak cycle against 0.03 MiB on every other arm, while the kernel tables its
eval tracks (`refs`, `fns`, `hosts`) stay flat from cycle 5 to 30. The growth is somewhere those
counters don't look.

The A/A pair passed 38 of 39 rows on `desktop` and 37 of 39 on `slow`, so the noise bands are
usable. The failures are `search / mainScriptMs` (a 0.1 ms quantity) and, on `slow`, the two
`annotate` CPU rows; every other pair prints those as "unreliable". Both runs used 10 rounds on an
Apple M4 Max on AC power, with the harness at `0c56a0e24`. `harnessDirty` is set only because
`README.md` was still an untracked draft during the runs. The `desktop` report is first below,
then `slow`.

## Viewer perf comparison: 2026-09-29T16-24-58-866Z-2024mbp16black-desktop

10 rounds, profile `desktop` (CPU throttle 1x).

`workerTaskMs` is n/a: Chromium has no Performance domain on dedicated workers.

### Environment

| Field | Value |
| --- | --- |
| Harness commit | 0c56a0e2458dabad25a7bbca679287188142d551 (uncommitted changes) |
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

### Gate

- `main`: pass
- `main-aa`: pass
- `impl-ts`: pass
- `impl-rust`: pass
- `plugins`: pass

### Sizes

| Metric | main | main-aa | impl-ts | impl-rust | plugins |
| --- | --- | --- | --- | --- | --- |
| engineWasmBytes | 7.19 MiB | 7.19 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB |
| engineWasmBrotliBytes | 2.15 MiB | 2.15 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB |
| kernelWasmBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 10.78 MiB |
| kernelWasmBrotliBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 1.77 MiB |
| jsBytes | 3.37 MiB | 3.37 MiB | 3.35 MiB | 3.35 MiB | 2.62 MiB |
| jsBrotliBytes | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.80 MiB |

### A/A: `main` vs `main-aa`

A metric passes when its interval includes the null value or its point estimate is within 3% of it. Its interval then sets the noise band every other pair must clear.

38 of 39 rows pass.

| Row | main median [p95] | main-aa median [p95] | ratio or diff [95% CI] | A/A |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 43.8 [141.5] | 44.1 [138.5] | 1.005 [0.957, 1.041] | pass |
| open(ebook) / openedMs | 95.8 [201.9] | 96.4 [200.2] | 1.006 [0.959, 1.040] | pass |
| open(ebook) / firstPaintMs | 177.8 [286.8] | 177.5 [288.1] | 0.998 [0.953, 1.025] | pass |
| open(ebook) / settledMs | 265.5 [390.5] | 266.6 [389.3] | 1.004 [0.951, 1.040] | pass |
| open(ebook) / mainTaskMs | 93.6 [103.3] | 93.8 [102.7] | 1.002 [0.961, 1.040] | pass |
| open(ebook) / mainScriptMs | 15.9 [18.1] | 15.9 [17.8] | 1.003 [0.940, 1.047] | pass |
| open(large) / readyMs | 44.7 [48.3] | 43.9 [46.4] | 0.982 [0.945, 1.032] | pass |
| open(large) / openedMs | 111.2 [124.4] | 111.0 [124.1] | 0.998 [0.947, 1.054] | pass |
| open(large) / firstPaintMs | 181.8 [197.5] | 180.4 [197.6] | 0.992 [0.951, 1.042] | pass |
| open(large) / settledMs | 177.4 [197.7] | 178.6 [193.5] | 1.007 [0.951, 1.047] | pass |
| open(large) / mainTaskMs | 206.3 [231.5] | 206.2 [231.3] | 0.999 [0.943, 1.052] | pass |
| open(large) / mainScriptMs | 18.2 [20.1] | 18.1 [20.7] | 0.996 [0.958, 1.062] | pass |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / wallMs | 2003.0 [2022.5] | 2007.4 [2016.1] | 1.002 [0.993, 1.004] | pass |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | pass |
| scroll / frameP95Ms | 8.7 [9.3] | 8.5 [9.0] | 0.974 [0.927, 1.037] | pass |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / mainTaskMs | 349.4 [368.7] | 352.9 [365.9] | 1.010 [0.943, 1.052] | pass |
| scroll / mainScriptMs | 46.4 [48.0] | 46.7 [47.3] | 1.005 [0.941, 1.026] | pass |
| jump / settleMs | 79.9 [82.7] | 77.1 [83.4] | -2.9 [-10.9, +5.1] | pass |
| jump / mainTaskMs | 45.7 [50.0] | 48.6 [52.6] | 1.063 [0.943, 1.184] | pass |
| jump / mainScriptMs | 12.2 [14.7] | 12.1 [14.9] | 0.996 [0.850, 1.075] | pass |
| zoom / settleMs | 309.1 [323.6] | 314.9 [321.8] | +5.8 [-15.8, +26.2] | pass |
| zoom / zoom2SettleMs | 110.3 [115.5] | 110.0 [111.8] | -0.3 [-6.8, +11.5] | pass |
| zoom / zoom4SettleMs | 199.7 [211.5] | 206.1 [212.7] | +6.4 [-7.4, +17.7] | pass |
| zoom / backTo1SettleMs | 61.2 [70.8] | 60.4 [61.2] | -0.8 [-11.0, +9.9] | pass |
| zoom / mainTaskMs | 140.8 [149.0] | 140.7 [146.8] | 1.000 [0.947, 1.096] | pass |
| zoom / mainScriptMs | 22.0 [23.5] | 21.4 [24.9] | 0.974 [0.888, 1.107] | pass |
| select / settleMs | 145.3 [148.6] | 142.5 [147.7] | -2.8 [-6.8, +3.3] | pass |
| select / mainTaskMs | 43.5 [47.1] | 41.1 [47.6] | 0.944 [0.859, 1.145] | pass |
| select / mainScriptMs | 10.1 [11.1] | 9.6 [11.2] | 0.956 [0.838, 1.174] | pass |
| search / searchMs | 49.7 [52.3] | 46.4 [52.6] | 0.935 [0.799, 1.103] | pass |
| search / mainTaskMs | 4.0 [4.4] | 3.8 [4.6] | 0.955 [0.816, 1.084] | pass |
| search / mainScriptMs | 0.1 [0.2] | 0.1 [0.1] | 0.894 [0.749, 0.989] | A/A failed |
| annotate / settleMs | 153.0 [155.3] | 152.8 [154.8] | -0.1 [-3.0, +1.0] | pass |
| annotate / mainTaskMs | 51.3 [54.0] | 50.3 [52.8] | 0.979 [0.912, 1.066] | pass |
| annotate / mainScriptMs | 10.6 [11.0] | 10.4 [10.9] | 0.984 [0.891, 1.091] | pass |

### `main` vs `impl-ts`

| Row | main median [p95] | impl-ts median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 43.8 [141.5] | 44.6 [142.5] | 1.018 [0.957, 1.053] | no detectable difference |
| open(ebook) / openedMs | 95.8 [201.9] | 100.1 [209.3] | 1.044 [0.991, 1.084] | no detectable difference |
| open(ebook) / firstPaintMs | 177.8 [286.8] | 181.5 [299.7] | 1.021 [0.982, 1.060] | no detectable difference |
| open(ebook) / settledMs | 265.5 [390.5] | 275.2 [402.1] | 1.036 [0.985, 1.072] | no detectable difference |
| open(ebook) / mainTaskMs | 93.6 [103.3] | 93.9 [103.0] | 1.002 [0.960, 1.044] | no detectable difference |
| open(ebook) / mainScriptMs | 15.9 [18.1] | 16.0 [17.8] | 1.005 [0.940, 1.060] | no detectable difference |
| open(large) / readyMs | 44.7 [48.3] | 44.4 [46.8] | 0.993 [0.952, 1.044] | no detectable difference |
| open(large) / openedMs | 111.2 [124.4] | 113.4 [128.9] | 1.020 [0.968, 1.083] | no detectable difference |
| open(large) / firstPaintMs | 181.8 [197.5] | 183.1 [201.2] | 1.007 [0.961, 1.058] | no detectable difference |
| open(large) / settledMs | 177.4 [197.7] | 181.1 [197.1] | 1.021 [0.966, 1.069] | no detectable difference |
| open(large) / mainTaskMs | 206.3 [231.5] | 205.3 [230.9] | 0.995 [0.940, 1.059] | no detectable difference |
| open(large) / mainScriptMs | 18.2 [20.1] | 18.0 [20.8] | 0.990 [0.944, 1.058] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2003.0 [2022.5] | 2002.6 [2023.2] | 1.000 [0.992, 1.004] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [1.000, 1.000] | no detectable difference |
| scroll / frameP95Ms | 8.7 [9.3] | 8.5 [8.9] | 0.973 [0.926, 1.021] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 349.4 [368.7] | 344.2 [365.2] | 0.985 [0.935, 1.045] | no detectable difference |
| scroll / mainScriptMs | 46.4 [48.0] | 46.2 [47.3] | 0.996 [0.945, 1.020] | no detectable difference |
| jump / settleMs | 79.9 [82.7] | 76.2 [85.4] | -3.7 [-7.1, +7.0] | no detectable difference |
| jump / mainTaskMs | 45.7 [50.0] | 45.9 [49.4] | 1.004 [0.938, 1.135] | no detectable difference |
| jump / mainScriptMs | 12.2 [14.7] | 11.8 [13.9] | 0.971 [0.846, 1.025] | no detectable difference |
| zoom / settleMs | 309.1 [323.6] | 311.1 [321.3] | +1.9 [-13.4, +22.6] | no detectable difference |
| zoom / zoom2SettleMs | 110.3 [115.5] | 112.1 [120.3] | +1.8 [-1.2, +14.4] | no detectable difference |
| zoom / zoom4SettleMs | 199.7 [211.5] | 195.3 [206.5] | -4.4 [-15.9, +9.5] | no detectable difference |
| zoom / backTo1SettleMs | 61.2 [70.8] | 59.0 [71.9] | -2.2 [-8.1, +10.0] | no detectable difference |
| zoom / mainTaskMs | 140.8 [149.0] | 141.6 [144.2] | 1.006 [0.965, 1.109] | no detectable difference |
| zoom / mainScriptMs | 22.0 [23.5] | 20.2 [22.3] | 0.922 [0.875, 1.022] | no detectable difference |
| select / settleMs | 145.3 [148.6] | 141.7 [146.5] | -3.7 [-6.6, +1.0] | no detectable difference |
| select / mainTaskMs | 43.5 [47.1] | 40.8 [44.5] | 0.937 [0.834, 1.126] | no detectable difference |
| select / mainScriptMs | 10.1 [11.1] | 9.6 [10.4] | 0.950 [0.819, 1.152] | no detectable difference |
| search / searchMs | 49.7 [52.3] | 41.9 [51.6] | 0.844 [0.790, 1.041] | no detectable difference |
| search / mainTaskMs | 4.0 [4.4] | 3.6 [4.3] | 0.902 [0.847, 0.995] | no detectable difference |
| search / mainScriptMs | 0.1 [0.2] | 0.1 [0.1] | 0.879 [0.764, 1.042] | unreliable (A/A failed) |
| annotate / settleMs | 153.0 [155.3] | 153.4 [156.3] | +0.5 [-2.0, +2.6] | no detectable difference |
| annotate / mainTaskMs | 51.3 [54.0] | 50.8 [53.0] | 0.989 [0.900, 1.082] | no detectable difference |
| annotate / mainScriptMs | 10.6 [11.0] | 10.5 [11.3] | 0.996 [0.850, 1.103] | no detectable difference |

### `impl-ts` vs `impl-rust`

| Row | impl-ts median [p95] | impl-rust median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 44.6 [142.5] | 43.3 [213.6] | 0.972 [0.939, 2.882] | no detectable difference |
| open(ebook) / openedMs | 100.1 [209.3] | 98.8 [285.6] | 0.987 [0.946, 1.917] | no detectable difference |
| open(ebook) / firstPaintMs | 181.5 [299.7] | 182.9 [379.1] | 1.008 [0.962, 1.550] | no detectable difference |
| open(ebook) / settledMs | 275.2 [402.1] | 272.8 [479.2] | 0.991 [0.957, 1.366] | no detectable difference |
| open(ebook) / mainTaskMs | 93.9 [103.0] | 94.2 [103.9] | 1.003 [0.970, 1.054] | no detectable difference |
| open(ebook) / mainScriptMs | 16.0 [17.8] | 15.9 [18.0] | 0.997 [0.963, 1.075] | no detectable difference |
| open(large) / readyMs | 44.4 [46.8] | 44.9 [49.3] | 1.011 [0.969, 1.048] | no detectable difference |
| open(large) / openedMs | 113.4 [128.9] | 114.2 [128.0] | 1.007 [0.946, 1.063] | no detectable difference |
| open(large) / firstPaintMs | 183.1 [201.2] | 185.6 [200.9] | 1.013 [0.966, 1.050] | no detectable difference |
| open(large) / settledMs | 181.1 [197.1] | 181.2 [197.8] | 1.001 [0.960, 1.048] | no detectable difference |
| open(large) / mainTaskMs | 205.3 [230.9] | 205.7 [230.6] | 1.002 [0.944, 1.063] | no detectable difference |
| open(large) / mainScriptMs | 18.0 [20.8] | 18.0 [20.4] | 1.002 [0.947, 1.062] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2002.6 [2023.2] | 2010.1 [2022.5] | 1.004 [0.998, 1.008] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [0.999, 1.000] | no detectable difference |
| scroll / frameP95Ms | 8.5 [8.9] | 8.6 [9.2] | 1.013 [0.974, 1.064] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 344.2 [365.2] | 346.4 [365.7] | 1.006 [0.946, 1.046] | no detectable difference |
| scroll / mainScriptMs | 46.2 [47.3] | 46.2 [48.1] | 1.000 [0.957, 1.053] | no detectable difference |
| jump / settleMs | 76.2 [85.4] | 79.6 [82.2] | +3.3 [-7.9, +6.6] | no detectable difference |
| jump / mainTaskMs | 45.9 [49.4] | 45.7 [49.2] | 0.996 [0.910, 1.059] | no detectable difference |
| jump / mainScriptMs | 11.8 [13.9] | 11.6 [12.7] | 0.980 [0.954, 1.035] | no detectable difference |
| zoom / settleMs | 311.1 [321.3] | 304.1 [315.9] | -7.0 [-14.0, +6.2] | no detectable difference |
| zoom / zoom2SettleMs | 112.1 [120.3] | 112.7 [118.7] | +0.6 [-5.6, +5.6] | no detectable difference |
| zoom / zoom4SettleMs | 195.3 [206.5] | 191.8 [200.6] | -3.5 [-16.1, +6.5] | no detectable difference |
| zoom / backTo1SettleMs | 59.0 [71.9] | 56.8 [62.9] | -2.2 [-10.1, +5.4] | no detectable difference |
| zoom / mainTaskMs | 141.6 [144.2] | 142.1 [146.1] | 1.003 [0.937, 1.033] | no detectable difference |
| zoom / mainScriptMs | 20.2 [22.3] | 20.5 [22.8] | 1.011 [0.968, 1.106] | no detectable difference |
| select / settleMs | 141.7 [146.5] | 142.9 [146.4] | +1.3 [-3.2, +2.9] | no detectable difference |
| select / mainTaskMs | 40.8 [44.5] | 39.9 [43.1] | 0.978 [0.927, 1.105] | no detectable difference |
| select / mainScriptMs | 9.6 [10.4] | 9.2 [10.0] | 0.961 [0.904, 1.106] | no detectable difference |
| search / searchMs | 41.9 [51.6] | 46.7 [54.7] | 1.114 [0.917, 1.269] | no detectable difference |
| search / mainTaskMs | 3.6 [4.3] | 3.9 [4.5] | 1.069 [0.968, 1.174] | no detectable difference |
| search / mainScriptMs | 0.1 [0.1] | 0.1 [0.1] | 1.044 [0.921, 1.171] | unreliable (A/A failed) |
| annotate / settleMs | 153.4 [156.3] | 154.4 [157.6] | +1.0 [-2.0, +3.3] | no detectable difference |
| annotate / mainTaskMs | 50.8 [53.0] | 50.1 [53.3] | 0.988 [0.944, 1.092] | no detectable difference |
| annotate / mainScriptMs | 10.5 [11.3] | 10.3 [11.0] | 0.979 [0.914, 1.159] | no detectable difference |

### `impl-ts` vs `plugins`

| Row | impl-ts median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 44.6 [142.5] | 69.0 [146.4] | 1.547 [1.496, 2.366] | slower |
| open(ebook) / openedMs | 100.1 [209.3] | 168.8 [263.9] | 1.687 [1.626, 2.122] | slower |
| open(ebook) / firstPaintMs | 181.5 [299.7] | 328.4 [444.3] | 1.809 [1.739, 2.109] | slower |
| open(ebook) / settledMs | 275.2 [402.1] | 543.0 [693.7] | 1.973 [1.882, 2.241] | slower |
| open(ebook) / mainTaskMs | 93.9 [103.0] | 436.7 [514.7] | 4.652 [4.454, 5.043] | slower |
| open(ebook) / mainScriptMs | 16.0 [17.8] | 240.2 [287.3] | 15.044 [14.154, 16.391] | slower |
| open(large) / readyMs | 44.4 [46.8] | 68.4 [76.2] | 1.541 [1.494, 1.614] | slower |
| open(large) / openedMs | 113.4 [128.9] | 1364.0 [1610.4] | 12.029 [11.326, 13.028] | slower |
| open(large) / firstPaintMs | 183.1 [201.2] | 2240.8 [2626.1] | 12.237 [11.671, 13.092] | slower |
| open(large) / settledMs | 181.1 [197.1] | 2323.7 [2761.0] | 12.832 [12.286, 13.855] | slower |
| open(large) / mainTaskMs | 205.3 [230.9] | 2643.6 [3035.1] | 12.879 [12.127, 13.727] | slower |
| open(large) / mainScriptMs | 18.0 [20.8] | 2171.3 [2491.6] | 120.743 [112.803, 128.356] | slower |
| scroll / settleMs | 0.0 [0.0] | 141.4 [378.0] | +141.4 [+48.9, +265.7] | slower |
| scroll / wallMs | 2002.6 [2023.2] | 16466.5 [18985.0] | 8.223 [8.063, 8.924] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 45.5 [50.0] | 5.454 [3.999, 5.999] | slower |
| scroll / frameP95Ms | 8.5 [8.9] | 209.3 [263.4] | 24.711 [23.895, 28.129] | slower |
| scroll / jankFrames | 0 [0] | 102.50 [115] | +102.50 [+97, +109.50] | slower |
| scroll / longTasks | 0 [0] | 64 [123.50] | +64 [+62, +86.50] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 9749.0 [14169.3] | +9749.0 [+9462.0, +11665.5] | slower |
| scroll / mainTaskMs | 344.2 [365.2] | 16875.1 [19466.6] | 49.026 [46.074, 52.214] | slower |
| scroll / mainScriptMs | 46.2 [47.3] | 9370.1 [10892.0] | 202.745 [195.549, 220.992] | slower |
| jump / settleMs | 76.2 [85.4] | 950.5 [1117.1] | +874.3 [+802.8, +908.5] | slower |
| jump / mainTaskMs | 45.9 [49.4] | 1316.3 [1523.7] | 28.689 [25.907, 30.490] | slower |
| jump / mainScriptMs | 11.8 [13.9] | 859.3 [1019.7] | 72.750 [66.467, 75.109] | slower |
| zoom / settleMs | 311.1 [321.3] | 543.9 [577.1] | +232.8 [+222.6, +251.3] | slower |
| zoom / zoom2SettleMs | 112.1 [120.3] | 250.9 [263.0] | +138.8 [+126.0, +148.9] | slower |
| zoom / zoom4SettleMs | 195.3 [206.5] | 293.9 [316.9] | +98.5 [+85.1, +115.5] | slower |
| zoom / backTo1SettleMs | 59.0 [71.9] | 108.8 [126.5] | +49.8 [+43.1, +58.6] | slower |
| zoom / mainTaskMs | 141.6 [144.2] | 934.5 [1071.0] | 6.598 [6.324, 6.971] | slower |
| zoom / mainScriptMs | 20.2 [22.3] | 518.9 [620.8] | 25.625 [24.151, 27.753] | slower |
| select / settleMs | 141.7 [146.5] | 291.4 [314.8] | +149.8 [+144.5, +156.9] | slower |
| select / mainTaskMs | 40.8 [44.5] | 236.5 [244.9] | 5.801 [5.507, 6.515] | slower |
| select / mainScriptMs | 9.6 [10.4] | 186.1 [196.0] | 19.453 [18.345, 22.343] | slower |
| search / searchMs | 41.9 [51.6] | 71.9 [79.1] | 1.715 [1.411, 1.879] | slower |
| search / mainTaskMs | 3.6 [4.3] | 35.3 [37.1] | 9.760 [8.805, 10.321] | slower |
| search / mainScriptMs | 0.1 [0.1] | 19.2 [19.7] | 210.637 [188.470, 234.085] | unreliable (A/A failed) |
| annotate / settleMs | 153.4 [156.3] | 331.1 [423.1] | +177.7 [+175.3, +221.6] | slower |
| annotate / mainTaskMs | 50.8 [53.0] | 282.7 [328.5] | 5.570 [5.388, 6.212] | slower |
| annotate / mainScriptMs | 10.5 [11.3] | 210.2 [247.4] | 19.995 [19.122, 23.470] | slower |

### `main` vs `plugins`

| Row | main median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 43.8 [141.5] | 69.0 [146.4] | 1.574 [1.488, 2.405] | slower |
| open(ebook) / openedMs | 95.8 [201.9] | 168.8 [263.9] | 1.762 [1.681, 2.213] | slower |
| open(ebook) / firstPaintMs | 177.8 [286.8] | 328.4 [444.3] | 1.847 [1.777, 2.147] | slower |
| open(ebook) / settledMs | 265.5 [390.5] | 543.0 [693.7] | 2.045 [1.935, 2.312] | slower |
| open(ebook) / mainTaskMs | 93.6 [103.3] | 436.7 [514.7] | 4.664 [4.458, 5.055] | slower |
| open(ebook) / mainScriptMs | 15.9 [18.1] | 240.2 [287.3] | 15.124 [14.153, 16.419] | slower |
| open(large) / readyMs | 44.7 [48.3] | 68.4 [76.2] | 1.530 [1.486, 1.612] | slower |
| open(large) / openedMs | 111.2 [124.4] | 1364.0 [1610.4] | 12.270 [11.652, 13.265] | slower |
| open(large) / firstPaintMs | 181.8 [197.5] | 2240.8 [2626.1] | 12.325 [11.782, 13.217] | slower |
| open(large) / settledMs | 177.4 [197.7] | 2323.7 [2761.0] | 13.096 [12.506, 14.173] | slower |
| open(large) / mainTaskMs | 206.3 [231.5] | 2643.6 [3035.1] | 12.817 [12.094, 13.664] | slower |
| open(large) / mainScriptMs | 18.2 [20.1] | 2171.3 [2491.6] | 119.579 [113.703, 127.247] | slower |
| scroll / settleMs | 0.0 [0.0] | 141.4 [378.0] | +141.4 [+48.9, +265.7] | slower |
| scroll / wallMs | 2003.0 [2022.5] | 16466.5 [18985.0] | 8.221 [8.035, 8.901] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 45.5 [50.0] | 5.454 [3.999, 5.999] | slower |
| scroll / frameP95Ms | 8.7 [9.3] | 209.3 [263.4] | 24.050 [22.828, 27.513] | slower |
| scroll / jankFrames | 0 [0] | 102.50 [115] | +102.50 [+97, +109.50] | slower |
| scroll / longTasks | 0 [0] | 64 [123.50] | +64 [+62, +86.50] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 9749.0 [14169.3] | +9749.0 [+9462.0, +11665.5] | slower |
| scroll / mainTaskMs | 349.4 [368.7] | 16875.1 [19466.6] | 48.299 [45.839, 51.666] | slower |
| scroll / mainScriptMs | 46.4 [48.0] | 9370.1 [10892.0] | 201.863 [193.596, 219.509] | slower |
| jump / settleMs | 79.9 [82.7] | 950.5 [1117.1] | +870.6 [+802.4, +906.7] | slower |
| jump / mainTaskMs | 45.7 [50.0] | 1316.3 [1523.7] | 28.793 [26.338, 31.959] | slower |
| jump / mainScriptMs | 12.2 [14.7] | 859.3 [1019.7] | 70.651 [58.835, 74.552] | slower |
| zoom / settleMs | 309.1 [323.6] | 543.9 [577.1] | +234.8 [+221.0, +258.8] | slower |
| zoom / zoom2SettleMs | 110.3 [115.5] | 250.9 [263.0] | +140.6 [+129.0, +157.8] | slower |
| zoom / zoom4SettleMs | 199.7 [211.5] | 293.9 [316.9] | +94.2 [+84.0, +111.2] | slower |
| zoom / backTo1SettleMs | 61.2 [70.8] | 108.8 [126.5] | +47.6 [+45.0, +60.3] | slower |
| zoom / mainTaskMs | 140.8 [149.0] | 934.5 [1071.0] | 6.639 [6.345, 7.323] | slower |
| zoom / mainScriptMs | 22.0 [23.5] | 518.9 [620.8] | 23.635 [22.091, 26.378] | slower |
| select / settleMs | 145.3 [148.6] | 291.4 [314.8] | +146.1 [+139.9, +154.6] | slower |
| select / mainTaskMs | 43.5 [47.1] | 236.5 [244.9] | 5.433 [5.175, 6.481] | slower |
| select / mainScriptMs | 10.1 [11.1] | 186.1 [196.0] | 18.475 [17.060, 22.345] | slower |
| search / searchMs | 49.7 [52.3] | 71.9 [79.1] | 1.448 [1.309, 1.712] | slower |
| search / mainTaskMs | 4.0 [4.4] | 35.3 [37.1] | 8.801 [8.112, 9.443] | slower |
| search / mainScriptMs | 0.1 [0.2] | 19.2 [19.7] | 185.198 [155.359, 203.316] | unreliable (A/A failed) |
| annotate / settleMs | 153.0 [155.3] | 331.1 [423.1] | +178.1 [+176.4, +222.1] | slower |
| annotate / mainTaskMs | 51.3 [54.0] | 282.7 [328.5] | 5.507 [5.353, 6.142] | slower |
| annotate / mainScriptMs | 10.6 [11.0] | 210.2 [247.4] | 19.914 [19.440, 22.492] | slower |

### Memory

| Row | main median [p95] | main-aa median [p95] | impl-ts median [p95] | impl-rust median [p95] | plugins median [p95] |
| --- | --- | --- | --- | --- | --- |
| open(ebook) / heapAfterOpenBytes | 34.34 MiB [34.35 MiB] | 34.34 MiB [34.34 MiB] | 35.03 MiB [35.03 MiB] | 34.98 MiB [34.99 MiB] | 43.36 MiB [43.92 MiB] |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB [22.27 MiB] | 22.27 MiB [22.27 MiB] | 23.00 MiB [23.00 MiB] | 22.95 MiB [22.95 MiB] | 23.03 MiB [23.03 MiB] |
| open(large) / heapAfterOpenBytes | 48.16 MiB [48.17 MiB] | 48.15 MiB [48.16 MiB] | 48.84 MiB [48.86 MiB] | 48.81 MiB [48.82 MiB] | 58.10 MiB [58.10 MiB] |
| open(large) / workerHeapAfterOpenBytes | 22.26 MiB [22.26 MiB] | 22.26 MiB [22.26 MiB] | 22.99 MiB [23.02 MiB] | 22.96 MiB [22.96 MiB] | 22.94 MiB [22.94 MiB] |
| soak / heapCycle5Bytes | 37.41 MiB [37.42 MiB] | 37.42 MiB [37.42 MiB] | 38.19 MiB [38.20 MiB] | 38.14 MiB [38.14 MiB] | 49.05 MiB [49.07 MiB] |
| soak / heapCycle30Bytes | 38.28 MiB [38.29 MiB] | 38.27 MiB [38.28 MiB] | 39.05 MiB [39.05 MiB] | 39.00 MiB [39.01 MiB] | 63.11 MiB [63.11 MiB] |
| soak / heapSoakSlopeBytes | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.56 MiB [0.56 MiB] |

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


## Viewer perf comparison: 2026-09-29T17-23-33-918Z-2024mbp16black-slow

10 rounds, profile `slow` (CPU throttle 4x).

The throttle reached the main thread only: Chromium refuses `Emulation.setCPUThrottlingRate` on the engine worker, so worker-side work ran at full speed.

`workerTaskMs` is n/a: Chromium has no Performance domain on dedicated workers.

### Environment

| Field | Value |
| --- | --- |
| Harness commit | 0c56a0e2458dabad25a7bbca679287188142d551 (uncommitted changes) |
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

### Gate

- `main`: pass
- `main-aa`: pass
- `impl-ts`: pass
- `impl-rust`: pass
- `plugins`: pass

### Sizes

| Metric | main | main-aa | impl-ts | impl-rust | plugins |
| --- | --- | --- | --- | --- | --- |
| engineWasmBytes | 7.19 MiB | 7.19 MiB | 10.57 MiB | 10.57 MiB | 10.57 MiB |
| engineWasmBrotliBytes | 2.15 MiB | 2.15 MiB | 2.83 MiB | 2.83 MiB | 2.83 MiB |
| kernelWasmBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 10.78 MiB |
| kernelWasmBrotliBytes | 0.00 MiB | 0.00 MiB | 0.00 MiB | 0.00 MiB | 1.77 MiB |
| jsBytes | 3.37 MiB | 3.37 MiB | 3.35 MiB | 3.35 MiB | 2.62 MiB |
| jsBrotliBytes | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.96 MiB | 0.80 MiB |

### A/A: `main` vs `main-aa`

A metric passes when its interval includes the null value or its point estimate is within 3% of it. Its interval then sets the noise band every other pair must clear.

37 of 39 rows pass.

| Row | main median [p95] | main-aa median [p95] | ratio or diff [95% CI] | A/A |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 122.9 [147.2] | 122.8 [148.2] | 1.000 [0.903, 1.108] | pass |
| open(ebook) / openedMs | 216.7 [253.9] | 216.1 [254.8] | 0.997 [0.893, 1.113] | pass |
| open(ebook) / firstPaintMs | 367.2 [424.8] | 368.4 [419.9] | 1.003 [0.916, 1.101] | pass |
| open(ebook) / settledMs | 488.9 [570.2] | 491.3 [553.2] | 1.005 [0.906, 1.102] | pass |
| open(ebook) / mainTaskMs | 357.7 [434.7] | 358.6 [410.9] | 1.002 [0.900, 1.087] | pass |
| open(ebook) / mainScriptMs | 60.0 [71.9] | 61.2 [67.1] | 1.020 [0.902, 1.132] | pass |
| open(large) / readyMs | 123.4 [146.0] | 123.7 [144.7] | 1.003 [0.915, 1.137] | pass |
| open(large) / openedMs | 260.8 [305.3] | 259.4 [303.1] | 0.995 [0.907, 1.134] | pass |
| open(large) / firstPaintMs | 405.4 [467.9] | 400.8 [468.3] | 0.988 [0.896, 1.114] | pass |
| open(large) / settledMs | 417.0 [484.0] | 414.6 [486.4] | 0.994 [0.902, 1.117] | pass |
| open(large) / mainTaskMs | 617.9 [677.4] | 614.7 [676.5] | 0.995 [0.949, 1.058] | pass |
| open(large) / mainScriptMs | 55.5 [69.0] | 56.1 [70.0] | 1.011 [0.902, 1.166] | pass |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / wallMs | 2275.7 [2362.3] | 2268.5 [2356.3] | 0.997 [0.983, 1.014] | pass |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.001 [1.000, 1.003] | pass |
| scroll / frameP95Ms | 10.1 [10.2] | 10.1 [10.1] | 0.999 [0.993, 1.001] | pass |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | pass |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | pass |
| scroll / mainTaskMs | 779.8 [887.7] | 772.3 [940.0] | 0.990 [0.943, 1.089] | pass |
| scroll / mainScriptMs | 85.8 [98.2] | 83.2 [108.6] | 0.969 [0.910, 1.095] | pass |
| jump / settleMs | 107.8 [112.4] | 102.8 [118.9] | -5.0 [-9.6, +5.2] | pass |
| jump / mainTaskMs | 127.9 [135.5] | 125.5 [142.8] | 0.981 [0.929, 1.051] | pass |
| jump / mainScriptMs | 35.7 [38.7] | 34.3 [39.5] | 0.961 [0.911, 1.047] | pass |
| zoom / settleMs | 366.8 [385.5] | 373.8 [397.5] | +7.0 [-12.6, +15.9] | pass |
| zoom / zoom2SettleMs | 153.2 [161.7] | 153.0 [168.7] | -0.2 [-8.1, +9.6] | pass |
| zoom / zoom4SettleMs | 215.8 [224.6] | 218.6 [230.9] | +2.9 [-6.7, +9.0] | pass |
| zoom / backTo1SettleMs | 98.7 [106.8] | 98.8 [107.6] | +0.1 [-2.4, +2.2] | pass |
| zoom / mainTaskMs | 421.1 [456.5] | 424.9 [475.8] | 1.009 [0.980, 1.027] | pass |
| zoom / mainScriptMs | 65.0 [71.6] | 68.0 [80.8] | 1.046 [0.986, 1.136] | pass |
| select / settleMs | 138.5 [141.7] | 139.4 [143.8] | +0.9 [-2.9, +4.4] | pass |
| select / mainTaskMs | 46.8 [54.5] | 46.3 [54.7] | 0.989 [0.948, 1.029] | pass |
| select / mainScriptMs | 8.6 [9.5] | 8.7 [9.6] | 1.014 [0.891, 1.092] | pass |
| search / searchMs | 40.2 [43.4] | 40.6 [43.4] | 1.010 [0.983, 1.040] | pass |
| search / mainTaskMs | 10.7 [12.2] | 11.1 [12.9] | 1.041 [0.886, 1.133] | pass |
| search / mainScriptMs | 0.1 [0.4] | 0.1 [0.2] | 1.077 [0.562, 1.743] | pass |
| annotate / settleMs | 170.4 [186.8] | 169.2 [175.1] | -1.2 [-6.1, +1.5] | pass |
| annotate / mainTaskMs | 86.7 [113.9] | 90.1 [98.6] | 1.039 [1.009, 1.081] | A/A failed |
| annotate / mainScriptMs | 14.7 [22.7] | 17.7 [20.6] | 1.205 [1.012, 1.261] | A/A failed |

### `main` vs `impl-ts`

| Row | main median [p95] | impl-ts median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 122.9 [147.2] | 124.5 [145.0] | 1.013 [0.915, 1.158] | no detectable difference |
| open(ebook) / openedMs | 216.7 [253.9] | 222.7 [255.3] | 1.028 [0.918, 1.157] | no detectable difference |
| open(ebook) / firstPaintMs | 367.2 [424.8] | 369.7 [425.9] | 1.007 [0.920, 1.139] | no detectable difference |
| open(ebook) / settledMs | 488.9 [570.2] | 500.5 [577.7] | 1.024 [0.922, 1.148] | no detectable difference |
| open(ebook) / mainTaskMs | 357.7 [434.7] | 359.7 [432.0] | 1.005 [0.898, 1.153] | no detectable difference |
| open(ebook) / mainScriptMs | 60.0 [71.9] | 60.2 [72.5] | 1.003 [0.892, 1.139] | no detectable difference |
| open(large) / readyMs | 123.4 [146.0] | 123.5 [147.9] | 1.001 [0.913, 1.140] | no detectable difference |
| open(large) / openedMs | 260.8 [305.3] | 263.1 [310.8] | 1.009 [0.923, 1.138] | no detectable difference |
| open(large) / firstPaintMs | 405.4 [467.9] | 409.1 [472.0] | 1.009 [0.909, 1.118] | no detectable difference |
| open(large) / settledMs | 417.0 [484.0] | 420.2 [489.2] | 1.008 [0.913, 1.113] | no detectable difference |
| open(large) / mainTaskMs | 617.9 [677.4] | 618.2 [669.2] | 1.001 [0.955, 1.055] | no detectable difference |
| open(large) / mainScriptMs | 55.5 [69.0] | 55.9 [66.2] | 1.007 [0.888, 1.111] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2275.7 [2362.3] | 2271.5 [2373.3] | 0.998 [0.981, 1.015] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.001 [0.999, 1.002] | no detectable difference |
| scroll / frameP95Ms | 10.1 [10.2] | 10.1 [10.1] | 0.999 [0.993, 1.001] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 779.8 [887.7] | 783.9 [950.5] | 1.005 [0.951, 1.096] | no detectable difference |
| scroll / mainScriptMs | 85.8 [98.2] | 82.4 [107.7] | 0.960 [0.893, 1.125] | no detectable difference |
| jump / settleMs | 107.8 [112.4] | 107.5 [127.6] | -0.3 [-8.3, +13.4] | no detectable difference |
| jump / mainTaskMs | 127.9 [135.5] | 129.9 [157.2] | 1.016 [0.947, 1.170] | no detectable difference |
| jump / mainScriptMs | 35.7 [38.7] | 33.9 [42.6] | 0.949 [0.903, 1.075] | no detectable difference |
| zoom / settleMs | 366.8 [385.5] | 373.5 [387.6] | +6.7 [-3.5, +19.4] | no detectable difference |
| zoom / zoom2SettleMs | 153.2 [161.7] | 155.7 [169.2] | +2.6 [-3.6, +13.4] | no detectable difference |
| zoom / zoom4SettleMs | 215.8 [224.6] | 215.2 [221.8] | -0.6 [-3.3, +4.0] | no detectable difference |
| zoom / backTo1SettleMs | 98.7 [106.8] | 98.7 [103.5] | -0.0 [-2.9, +1.6] | no detectable difference |
| zoom / mainTaskMs | 421.1 [456.5] | 426.1 [451.6] | 1.012 [0.984, 1.035] | no detectable difference |
| zoom / mainScriptMs | 65.0 [71.6] | 68.2 [70.4] | 1.050 [0.987, 1.133] | no detectable difference |
| select / settleMs | 138.5 [141.7] | 139.8 [142.9] | +1.3 [-3.0, +5.0] | no detectable difference |
| select / mainTaskMs | 46.8 [54.5] | 48.4 [57.4] | 1.034 [0.972, 1.081] | no detectable difference |
| select / mainScriptMs | 8.6 [9.5] | 9.1 [11.9] | 1.050 [0.915, 1.241] | no detectable difference |
| search / searchMs | 40.2 [43.4] | 40.4 [43.1] | 1.004 [0.985, 1.014] | no detectable difference |
| search / mainTaskMs | 10.7 [12.2] | 11.4 [12.2] | 1.067 [0.872, 1.124] | no detectable difference |
| search / mainScriptMs | 0.1 [0.4] | 0.1 [0.6] | 1.011 [0.545, 3.467] | no detectable difference |
| annotate / settleMs | 170.4 [186.8] | 168.1 [170.9] | -2.3 [-7.1, -0.2] | no detectable difference |
| annotate / mainTaskMs | 86.7 [113.9] | 87.6 [98.0] | 1.010 [0.986, 1.042] | unreliable (A/A failed) |
| annotate / mainScriptMs | 14.7 [22.7] | 15.9 [19.0] | 1.081 [0.949, 1.145] | unreliable (A/A failed) |

### `impl-ts` vs `impl-rust`

| Row | impl-ts median [p95] | impl-rust median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 124.5 [145.0] | 124.2 [147.7] | 0.997 [0.873, 1.102] | no detectable difference |
| open(ebook) / openedMs | 222.7 [255.3] | 220.9 [262.7] | 0.992 [0.881, 1.099] | no detectable difference |
| open(ebook) / firstPaintMs | 369.7 [425.9] | 372.8 [443.6] | 1.008 [0.891, 1.094] | no detectable difference |
| open(ebook) / settledMs | 500.5 [577.7] | 501.8 [573.7] | 1.002 [0.893, 1.084] | no detectable difference |
| open(ebook) / mainTaskMs | 359.7 [432.0] | 363.3 [428.0] | 1.010 [0.877, 1.087] | no detectable difference |
| open(ebook) / mainScriptMs | 60.2 [72.5] | 62.2 [73.4] | 1.033 [0.911, 1.108] | no detectable difference |
| open(large) / readyMs | 123.5 [147.9] | 123.3 [145.2] | 0.999 [0.874, 1.126] | no detectable difference |
| open(large) / openedMs | 263.1 [310.8] | 262.1 [307.4] | 0.996 [0.882, 1.112] | no detectable difference |
| open(large) / firstPaintMs | 409.1 [472.0] | 408.4 [472.6] | 0.998 [0.897, 1.092] | no detectable difference |
| open(large) / settledMs | 420.2 [489.2] | 416.8 [486.7] | 0.992 [0.897, 1.094] | no detectable difference |
| open(large) / mainTaskMs | 618.2 [669.2] | 611.8 [666.6] | 0.990 [0.939, 1.041] | no detectable difference |
| open(large) / mainScriptMs | 55.9 [66.2] | 56.7 [69.9] | 1.015 [0.934, 1.171] | no detectable difference |
| scroll / settleMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / wallMs | 2271.5 [2373.3] | 2277.4 [2404.4] | 1.003 [0.983, 1.027] | no detectable difference |
| scroll / frameP50Ms | 8.3 [8.3] | 8.3 [8.3] | 1.000 [0.998, 1.002] | no detectable difference |
| scroll / frameP95Ms | 10.1 [10.1] | 10.0 [10.1] | 0.997 [0.992, 1.006] | no detectable difference |
| scroll / jankFrames | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTasks | 0 [0] | 0 [0] | +0 [+0, +0] | no detectable difference |
| scroll / longTaskMs | 0.0 [0.0] | 0.0 [0.0] | +0.0 [+0.0, +0.0] | no detectable difference |
| scroll / mainTaskMs | 783.9 [950.5] | 768.3 [1009.8] | 0.980 [0.894, 1.166] | no detectable difference |
| scroll / mainScriptMs | 82.4 [107.7] | 80.6 [114.5] | 0.978 [0.846, 1.226] | no detectable difference |
| jump / settleMs | 107.5 [127.6] | 108.5 [119.7] | +1.0 [-12.3, +9.5] | no detectable difference |
| jump / mainTaskMs | 129.9 [157.2] | 124.5 [142.7] | 0.959 [0.845, 1.044] | no detectable difference |
| jump / mainScriptMs | 33.9 [42.6] | 34.9 [40.9] | 1.029 [0.910, 1.111] | no detectable difference |
| zoom / settleMs | 373.5 [387.6] | 374.2 [389.8] | +0.7 [-9.8, +11.4] | no detectable difference |
| zoom / zoom2SettleMs | 155.7 [169.2] | 155.5 [159.5] | -0.3 [-10.1, +4.3] | no detectable difference |
| zoom / zoom4SettleMs | 215.2 [221.8] | 218.3 [230.3] | +3.1 [-1.8, +9.9] | no detectable difference |
| zoom / backTo1SettleMs | 98.7 [103.5] | 98.0 [103.6] | -0.8 [-2.0, +1.0] | no detectable difference |
| zoom / mainTaskMs | 426.1 [451.6] | 427.6 [453.1] | 1.004 [0.984, 1.032] | no detectable difference |
| zoom / mainScriptMs | 68.2 [70.4] | 69.6 [74.0] | 1.020 [0.940, 1.086] | no detectable difference |
| select / settleMs | 139.8 [142.9] | 139.0 [144.2] | -0.7 [-4.7, +4.7] | no detectable difference |
| select / mainTaskMs | 48.4 [57.4] | 47.0 [54.8] | 0.970 [0.916, 1.051] | no detectable difference |
| select / mainScriptMs | 9.1 [11.9] | 7.9 [11.0] | 0.871 [0.679, 1.173] | no detectable difference |
| search / searchMs | 40.4 [43.1] | 41.2 [42.1] | 1.020 [1.010, 1.036] | no detectable difference |
| search / mainTaskMs | 11.4 [12.2] | 11.0 [12.5] | 0.965 [0.920, 1.170] | no detectable difference |
| search / mainScriptMs | 0.1 [0.6] | 0.1 [0.5] | 0.946 [0.283, 1.614] | no detectable difference |
| annotate / settleMs | 168.1 [170.9] | 167.5 [173.9] | -0.6 [-6.0, +3.6] | no detectable difference |
| annotate / mainTaskMs | 87.6 [98.0] | 88.5 [96.1] | 1.011 [0.982, 1.038] | unreliable (A/A failed) |
| annotate / mainScriptMs | 15.9 [19.0] | 16.1 [19.9] | 1.015 [0.940, 1.146] | unreliable (A/A failed) |

### `impl-ts` vs `plugins`

| Row | impl-ts median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 124.5 [145.0] | 234.5 [277.8] | 1.884 [1.646, 2.092] | slower |
| open(ebook) / openedMs | 222.7 [255.3] | 512.9 [619.0] | 2.303 [2.045, 2.579] | slower |
| open(ebook) / firstPaintMs | 369.7 [425.9] | 1104.3 [1344.9] | 2.987 [2.641, 3.358] | slower |
| open(ebook) / settledMs | 500.5 [577.7] | 1401.8 [1732.3] | 2.801 [2.491, 3.169] | slower |
| open(ebook) / mainTaskMs | 359.7 [432.0] | 1659.5 [2025.5] | 4.614 [4.006, 5.140] | slower |
| open(ebook) / mainScriptMs | 60.2 [72.5] | 916.7 [1105.6] | 15.229 [13.422, 16.798] | slower |
| open(large) / readyMs | 123.5 [147.9] | 236.4 [281.8] | 1.914 [1.680, 2.112] | slower |
| open(large) / openedMs | 263.1 [310.8] | 5305.7 [6509.5] | 20.163 [17.814, 23.483] | slower |
| open(large) / firstPaintMs | 409.1 [472.0] | 8644.7 [10374.3] | 21.133 [19.069, 24.357] | slower |
| open(large) / settledMs | 420.2 [489.2] | 9373.8 [11462.3] | 22.306 [20.197, 25.697] | slower |
| open(large) / mainTaskMs | 618.2 [669.2] | 9998.9 [12166.5] | 16.174 [15.325, 18.459] | slower |
| open(large) / mainScriptMs | 55.9 [66.2] | 8172.2 [9915.7] | 146.211 [134.485, 167.158] | slower |
| scroll / settleMs | 0.0 [0.0] | 3046.4 [3383.4] | +3046.4 [+2738.8, +3153.0] | slower |
| scroll / wallMs | 2271.5 [2373.3] | 58895.0 [65455.4] | 25.927 [25.519, 26.684] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 168.3 [197.3] | 20.241 [20.197, 22.048] | slower |
| scroll / frameP95Ms | 10.1 [10.1] | 869.6 [975.6] | 86.483 [85.532, 87.938] | slower |
| scroll / jankFrames | 0 [0] | 157 [161.20] | +157 [+156.50, +159] | slower |
| scroll / longTasks | 0 [0] | 168.50 [179.40] | +168.50 [+165, +174] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 58861.0 [65450.3] | +58861.0 [+58393.0, +60399.0] | slower |
| scroll / mainTaskMs | 783.9 [950.5] | 59782.8 [66508.2] | 76.266 [69.640, 79.382] | slower |
| scroll / mainScriptMs | 82.4 [107.7] | 32154.6 [35711.0] | 390.285 [332.514, 414.813] | slower |
| jump / settleMs | 107.5 [127.6] | 4187.4 [4772.6] | +4079.9 [+3574.7, +4532.6] | slower |
| jump / mainTaskMs | 129.9 [157.2] | 4722.7 [5481.9] | 36.353 [31.433, 40.613] | slower |
| jump / mainScriptMs | 33.9 [42.6] | 2958.2 [3548.0] | 87.293 [74.582, 100.582] | slower |
| zoom / settleMs | 373.5 [387.6] | 2059.2 [2229.6] | +1685.7 [+1668.7, +1731.6] | slower |
| zoom / zoom2SettleMs | 155.7 [169.2] | 850.3 [924.6] | +694.5 [+670.0, +717.8] | slower |
| zoom / zoom4SettleMs | 215.2 [221.8] | 1231.3 [1314.2] | +1016.1 [+985.0, +1034.7] | slower |
| zoom / backTo1SettleMs | 98.7 [103.5] | 423.4 [446.7] | +324.6 [+323.3, +332.5] | slower |
| zoom / mainTaskMs | 426.1 [451.6] | 3065.8 [3288.5] | 7.195 [7.031, 7.422] | slower |
| zoom / mainScriptMs | 68.2 [70.4] | 1693.6 [1828.7] | 24.834 [23.891, 26.568] | slower |
| select / settleMs | 139.8 [142.9] | 903.5 [929.1] | +763.7 [+760.8, +768.7] | slower |
| select / mainTaskMs | 48.4 [57.4] | 817.2 [857.3] | 16.878 [16.047, 17.824] | slower |
| select / mainScriptMs | 9.1 [11.9] | 653.2 [680.5] | 72.083 [61.874, 82.220] | slower |
| search / searchMs | 40.4 [43.1] | 134.3 [144.9] | 3.324 [3.297, 3.402] | slower |
| search / mainTaskMs | 11.4 [12.2] | 112.8 [121.4] | 9.877 [9.761, 11.852] | slower |
| search / mainScriptMs | 0.1 [0.6] | 66.4 [72.2] | 718.038 [192.747, 851.519] | slower |
| annotate / settleMs | 168.1 [170.9] | 1181.6 [1270.8] | +1013.5 [+1003.3, +1032.4] | slower |
| annotate / mainTaskMs | 87.6 [98.0] | 1131.8 [1214.2] | 12.928 [12.638, 13.160] | unreliable (A/A failed) |
| annotate / mainScriptMs | 15.9 [19.0] | 848.7 [905.9] | 53.516 [50.544, 56.860] | unreliable (A/A failed) |

### `main` vs `plugins`

| Row | main median [p95] | plugins median [p95] | ratio or diff [95% CI] | Verdict |
| --- | --- | --- | --- | --- |
| open(ebook) / readyMs | 122.9 [147.2] | 234.5 [277.8] | 1.909 [1.725, 2.117] | slower |
| open(ebook) / openedMs | 216.7 [253.9] | 512.9 [619.0] | 2.367 [2.121, 2.647] | slower |
| open(ebook) / firstPaintMs | 367.2 [424.8] | 1104.3 [1344.9] | 3.008 [2.741, 3.383] | slower |
| open(ebook) / settledMs | 488.9 [570.2] | 1401.8 [1732.3] | 2.867 [2.583, 3.243] | slower |
| open(ebook) / mainTaskMs | 357.7 [434.7] | 1659.5 [2025.5] | 4.639 [4.149, 5.120] | slower |
| open(ebook) / mainScriptMs | 60.0 [71.9] | 916.7 [1105.6] | 15.268 [13.595, 16.992] | slower |
| open(large) / readyMs | 123.4 [146.0] | 236.4 [281.8] | 1.916 [1.752, 2.106] | slower |
| open(large) / openedMs | 260.8 [305.3] | 5305.7 [6509.5] | 20.344 [18.558, 23.693] | slower |
| open(large) / firstPaintMs | 405.4 [467.9] | 8644.7 [10374.3] | 21.322 [19.225, 24.515] | slower |
| open(large) / settledMs | 417.0 [484.0] | 9373.8 [11462.3] | 22.477 [20.371, 25.872] | slower |
| open(large) / mainTaskMs | 617.9 [677.4] | 9998.9 [12166.5] | 16.182 [15.363, 18.455] | slower |
| open(large) / mainScriptMs | 55.5 [69.0] | 8172.2 [9915.7] | 147.281 [131.086, 167.554] | slower |
| scroll / settleMs | 0.0 [0.0] | 3046.4 [3383.4] | +3046.4 [+2738.8, +3153.0] | slower |
| scroll / wallMs | 2275.7 [2362.3] | 58895.0 [65455.4] | 25.879 [25.432, 26.609] | slower |
| scroll / frameP50Ms | 8.3 [8.3] | 168.3 [197.3] | 20.260 [20.208, 22.068] | slower |
| scroll / frameP95Ms | 10.1 [10.2] | 869.6 [975.6] | 86.408 [85.392, 87.798] | slower |
| scroll / jankFrames | 0 [0] | 157 [161.20] | +157 [+156.50, +159] | slower |
| scroll / longTasks | 0 [0] | 168.50 [179.40] | +168.50 [+165, +174] | slower |
| scroll / longTaskMs | 0.0 [0.0] | 58861.0 [65450.3] | +58861.0 [+58393.0, +60399.0] | slower |
| scroll / mainTaskMs | 779.8 [887.7] | 59782.8 [66508.2] | 76.666 [73.073, 79.338] | slower |
| scroll / mainScriptMs | 85.8 [98.2] | 32154.6 [35711.0] | 374.559 [357.555, 394.092] | slower |
| jump / settleMs | 107.8 [112.4] | 4187.4 [4772.6] | +4079.6 [+3572.1, +4533.3] | slower |
| jump / mainTaskMs | 127.9 [135.5] | 4722.7 [5481.9] | 36.928 [33.481, 41.636] | slower |
| jump / mainScriptMs | 35.7 [38.7] | 2958.2 [3548.0] | 82.803 [73.461, 96.526] | slower |
| zoom / settleMs | 366.8 [385.5] | 2059.2 [2229.6] | +1692.5 [+1676.6, +1738.6] | slower |
| zoom / zoom2SettleMs | 153.2 [161.7] | 850.3 [924.6] | +697.1 [+674.7, +721.1] | slower |
| zoom / zoom4SettleMs | 215.8 [224.6] | 1231.3 [1314.2] | +1015.6 [+984.6, +1034.4] | slower |
| zoom / backTo1SettleMs | 98.7 [106.8] | 423.4 [446.7] | +324.6 [+322.5, +332.1] | slower |
| zoom / mainTaskMs | 421.1 [456.5] | 3065.8 [3288.5] | 7.281 [7.141, 7.454] | slower |
| zoom / mainScriptMs | 65.0 [71.6] | 1693.6 [1828.7] | 26.068 [25.456, 27.858] | slower |
| select / settleMs | 138.5 [141.7] | 903.5 [929.1] | +765.0 [+762.0, +769.6] | slower |
| select / mainTaskMs | 46.8 [54.5] | 817.2 [857.3] | 17.445 [16.666, 17.827] | slower |
| select / mainScriptMs | 8.6 [9.5] | 653.2 [680.5] | 75.691 [72.298, 82.319] | slower |
| search / searchMs | 40.2 [43.4] | 134.3 [144.9] | 3.339 [3.294, 3.405] | slower |
| search / mainTaskMs | 10.7 [12.2] | 112.8 [121.4] | 10.535 [9.572, 11.137] | slower |
| search / mainScriptMs | 0.1 [0.4] | 66.4 [72.2] | 725.885 [397.284, 984.382] | slower |
| annotate / settleMs | 170.4 [186.8] | 1181.6 [1270.8] | +1011.1 [+1000.0, +1030.1] | slower |
| annotate / mainTaskMs | 86.7 [113.9] | 1131.8 [1214.2] | 13.055 [12.792, 13.370] | unreliable (A/A failed) |
| annotate / mainScriptMs | 14.7 [22.7] | 848.7 [905.9] | 57.854 [50.936, 58.897] | unreliable (A/A failed) |

### Memory

| Row | main median [p95] | main-aa median [p95] | impl-ts median [p95] | impl-rust median [p95] | plugins median [p95] |
| --- | --- | --- | --- | --- | --- |
| open(ebook) / heapAfterOpenBytes | 35.35 MiB [35.36 MiB] | 35.35 MiB [35.36 MiB] | 36.04 MiB [36.06 MiB] | 35.99 MiB [36.00 MiB] | 47.41 MiB [47.97 MiB] |
| open(ebook) / workerHeapAfterOpenBytes | 22.27 MiB [22.27 MiB] | 22.27 MiB [22.27 MiB] | 23.00 MiB [23.00 MiB] | 22.95 MiB [22.95 MiB] | 23.49 MiB [23.49 MiB] |
| open(large) / heapAfterOpenBytes | 47.61 MiB [47.73 MiB] | 47.63 MiB [47.74 MiB] | 48.30 MiB [48.38 MiB] | 48.30 MiB [48.42 MiB] | 58.10 MiB [58.14 MiB] |
| open(large) / workerHeapAfterOpenBytes | 22.21 MiB [22.21 MiB] | 22.21 MiB [22.21 MiB] | 22.94 MiB [22.94 MiB] | 22.94 MiB [22.94 MiB] | 22.94 MiB [22.94 MiB] |
| soak / heapCycle5Bytes | 38.42 MiB [38.98 MiB] | 38.42 MiB [38.43 MiB] | 39.19 MiB [39.20 MiB] | 39.14 MiB [39.70 MiB] | 50.16 MiB [50.18 MiB] |
| soak / heapCycle30Bytes | 39.27 MiB [39.84 MiB] | 39.28 MiB [39.28 MiB] | 40.05 MiB [40.05 MiB] | 40.00 MiB [40.56 MiB] | 64.16 MiB [64.17 MiB] |
| soak / heapSoakSlopeBytes | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.03 MiB [0.03 MiB] | 0.56 MiB [0.56 MiB] |

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

