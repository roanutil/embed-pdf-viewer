# Interop benchmark results

5 interleaved rounds. Node v24.20.0, darwin 27.0.0, Apple M4 Max.

### Wasm size

| implementation | bytes |
|---|---:|
| typescript-baseline | 0 |
| rust-geometry-only | 134,411 |
| rust-geometry-and-state | 284,382 |
| swift-geometry-only | 73,400 |

Medians across rounds. "worst spread" is the widest (max-min)/median of any single implementation for that cell. A large value indicates unstable timings without identifying the cause.

### Per-frame projection

| cell | typescript-baseline | rust-geometry-only | rust-geometry-and-state | swift-geometry-only | rust-geometry-only/typescript-baseline | rust-geometry-and-state/typescript-baseline | swift-geometry-only/typescript-baseline | worst spread |
|---|---|---|---|---|---|---|---|---|
| n=1 plain flat | 24 ns | 152 ns | 234 ns | 300 ns | 6.33x | 9.73x | 12.47x | 10% |
| n=1 plain projecting | 23 ns | 154 ns | 239 ns | 299 ns | 6.59x | 10.26x | 12.83x | 10% |
| n=1 anchored flat | 28 ns | 182 ns | 248 ns | 329 ns | 6.60x | 9.01x | 11.92x | 19% |
| n=1 anchored projecting | 149 ns | 984 ns | 396 ns | 1.76 µs | 6.62x | 2.66x | 11.80x | 5% |
| n=10 plain flat | 277 ns | 1.42 µs | 814 ns | 3.05 µs | 5.15x | 2.94x | 11.04x | 9% |
| n=10 plain projecting | 240 ns | 1.41 µs | 813 ns | 3.03 µs | 5.87x | 3.39x | 12.64x | 21% |
| n=10 anchored flat | 224 ns | 1.68 µs | 876 ns | 3.14 µs | 7.48x | 3.91x | 14.03x | 44% |
| n=10 anchored projecting | 1.33 µs | 9.61 µs | 2.34 µs | 17.31 µs | 7.25x | 1.77x | 13.06x | 11% |
| n=100 plain flat | 2.26 µs | 13.96 µs | 6.56 µs | 29.98 µs | 6.17x | 2.90x | 13.25x | 24% |
| n=100 plain projecting | 2.26 µs | 13.90 µs | 6.57 µs | 30.18 µs | 6.14x | 2.90x | 13.34x | 24% |
| n=100 anchored flat | 2.39 µs | 16.58 µs | 7.41 µs | 31.35 µs | 6.93x | 3.10x | 13.11x | 29% |
| n=100 anchored projecting | 12.77 µs | 95.38 µs | 21.74 µs | 171.96 µs | 7.47x | 1.70x | 13.47x | 11% |
| n=1000 plain flat | 22.79 µs | 138.70 µs | 65.22 µs | 296.42 µs | 6.09x | 2.86x | 13.01x | 27% |
| n=1000 plain projecting | 22.11 µs | 136.75 µs | 65.24 µs | 302.08 µs | 6.19x | 2.95x | 13.66x | 27% |
| n=1000 anchored flat | 22.93 µs | 162.65 µs | 74.00 µs | 310.85 µs | 7.09x | 3.23x | 13.56x | 38% |
| n=1000 anchored projecting | 124.86 µs | 941.83 µs | 216.69 µs | 1.71 ms | 7.54x | 1.74x | 13.68x | 15% |
| n=10000 plain flat | 221.89 µs | 1.38 ms | 702.03 µs | 2.99 ms | 6.22x | 3.16x | 13.49x | 35% |
| n=10000 plain projecting | 221.90 µs | 1.37 ms | 711.36 µs | 2.95 ms | 6.15x | 3.21x | 13.31x | 31% |
| n=10000 anchored flat | 233.19 µs | 1.61 ms | 794.51 µs | 3.09 ms | 6.91x | 3.41x | 13.24x | 15% |
| n=10000 anchored projecting | 1.27 ms | 9.62 ms | 2.20 ms | 17.54 ms | 7.59x | 1.74x | 13.84x | 11% |

### rust-geometry-and-state: what the object decode costs

The Rust columns compare returning a numeric buffer with converting it into JavaScript display objects. Decode share is the relative difference between recorded medians, not a separately timed conversion. Both paths use one Wasm call once the metadata cache is current.

| cell | typescript-baseline (objects) | rust-geometry-and-state (objects) | rust-geometry-and-state (raw buffer) | decode share | raw vs typescript-baseline |
|---|---|---|---|---|---|
| n=1 plain flat | 24 ns | 234 ns | 194 ns | 17% | 8.08x |
| n=1 plain projecting | 23 ns | 239 ns | 197 ns | 18% | 8.45x |
| n=1 anchored flat | 28 ns | 248 ns | 210 ns | 16% | 7.61x |
| n=1 anchored projecting | 149 ns | 396 ns | 354 ns | 11% | 2.38x |
| n=10 plain flat | 277 ns | 814 ns | 520 ns | 36% | 1.88x |
| n=10 plain projecting | 240 ns | 813 ns | 519 ns | 36% | 2.16x |
| n=10 anchored flat | 224 ns | 876 ns | 600 ns | 31% | 2.68x |
| n=10 anchored projecting | 1.33 µs | 2.34 µs | 2.05 µs | 13% | 1.54x |
| n=100 plain flat | 2.26 µs | 6.56 µs | 3.84 µs | 41% | 1.70x |
| n=100 plain projecting | 2.26 µs | 6.57 µs | 3.86 µs | 41% | 1.71x |
| n=100 anchored flat | 2.39 µs | 7.41 µs | 4.76 µs | 36% | 1.99x |
| n=100 anchored projecting | 12.77 µs | 21.74 µs | 18.99 µs | 13% | 1.49x |
| n=1000 plain flat | 22.79 µs | 65.22 µs | 35.51 µs | 46% | 1.56x |
| n=1000 plain projecting | 22.11 µs | 65.24 µs | 35.40 µs | 46% | 1.60x |
| n=1000 anchored flat | 22.93 µs | 74.00 µs | 44.07 µs | 40% | 1.92x |
| n=1000 anchored projecting | 124.86 µs | 216.69 µs | 186.76 µs | 14% | 1.50x |
| n=10000 plain flat | 221.89 µs | 702.03 µs | 349.94 µs | 50% | 1.58x |
| n=10000 plain projecting | 221.90 µs | 711.36 µs | 351.94 µs | 51% | 1.59x |
| n=10000 anchored flat | 233.19 µs | 794.51 µs | 435.49 µs | 45% | 1.87x |
| n=10000 anchored projecting | 1.27 ms | 2.20 ms | 1.86 ms | 16% | 1.46x |

### Hit-test

| cell | typescript-baseline | rust-geometry-only | rust-geometry-and-state | swift-geometry-only | rust-geometry-only/typescript-baseline | rust-geometry-and-state/typescript-baseline | swift-geometry-only/typescript-baseline | worst spread |
|---|---|---|---|---|---|---|---|---|
| n=10 hit | 130 ns | 1.04 µs | 185 ns | 1.91 µs | 7.98x | 1.42x | 14.66x | 8% |
| n=10 miss | 1.25 µs | 10.29 µs | 1.65 µs | 19.12 µs | 8.22x | 1.32x | 15.28x | 7% |
| n=100 hit | 128 ns | 1.04 µs | 190 ns | 1.93 µs | 8.12x | 1.48x | 15.05x | 6% |
| n=100 miss | 12.18 µs | 103.95 µs | 16.30 µs | 191.79 µs | 8.54x | 1.34x | 15.75x | 6% |
| n=1000 hit | 127 ns | 1.05 µs | 189 ns | 1.92 µs | 8.23x | 1.48x | 15.05x | 6% |
| n=1000 miss | 121.71 µs | 1.04 ms | 162.72 µs | 1.92 ms | 8.53x | 1.34x | 15.75x | 5% |

### State updates (time per pointerMove)

| shapes | typescript-baseline | rust-geometry-only | rust-geometry-and-state | swift-geometry-only | rust-geometry-only/typescript-baseline | rust-geometry-and-state/typescript-baseline | swift-geometry-only/typescript-baseline | worst spread |
|---|---|---|---|---|---|---|---|---|
| n=10 | 76 ns | 79 ns | 1.69 µs | 62 ns | 1.04x | 22.17x | 0.82x | 10% |
| n=100 | 357 ns | 356 ns | 1.72 µs | 371 ns | 1.00x | 4.80x | 1.04x | 9% |
| n=1000 | 3.77 µs | 3.71 µs | 1.97 µs | 3.94 µs | 0.99x | 0.52x | 1.05x | 14% |

### Allocation (100 anchored shapes, window with no observed GC)

These are `heapUsed + arrayBuffers` deltas over a window with no observed GC, not an allocation counter. They include copied typed-array backing stores but exclude Rust's internal allocations. Heap calibration: a `{x, y}` object measures 73 bytes in this run. This is a calibration observation, not an accuracy bound. Backing-store bytes are accounted separately.

| | bytes/frame | heap/frame | backing stores/frame | bytes/shape | window | vs typescript-baseline |
|---|---|---|---|---|---|---|
| typescript-baseline | 145166 | 145166 | 0 | 1451.7 | 16 frames | 1.00x |
| rust-geometry-only | 502849 | 502849 | 0 | 5028.5 | 8 frames | 3.46x |
| rust-geometry-and-state | 56421 | 48413 | 8008 | 564.2 | 64 frames | 0.39x |
| swift-geometry-only | n/a | n/a | n/a | n/a | 4 frames | n/a |

An `n/a` means every allocation round for that implementation saw a GC inside its measurement window, so there is no delta to report. It is a missing measurement, not a zero.


### Cold start

Chrome DevTools MCP, 5 same-origin iframe loads per POC against `vite preview` production builds.

> These figures come from `results/cold-start.json`, a committed summary recorded separately in a browser. Unlike every other table here they were NOT produced by this run, and the procedure that produces them is manual (see bench/README.md).

> Historical localhost measurements using repeated iframe loads. Assets and compiled Wasm may be reused from cache; these are not uncached network or guaranteed fresh-compilation timings. Individual samples and build hashes were not retained. The ready interval and first-frame marker are defined in bench/README.md.

The ready interval is wasmReady minus boot. First frame is recorded by a React effect after the initial viewer render, relative to navigation start; it is not a paint measurement.

| | wasm shipped | ready interval | first frame | first frame range |
|---|---|---|---|---|
| typescript-baseline | none | 1.2 ms | 19.0 ms | 13.5 to 34.4 ms |
| rust-geometry-only | 130.7 KiB | 6.3 ms | 21.2 ms | 12.7 to 43.7 ms |
| rust-geometry-and-state | 277.2 KiB | 7.8 ms | 24.8 ms | 21.5 to 41.6 ms |

### Crossover

At the projecting view, the first tested shape count where rust-geometry-and-state beats typescript-baseline is none of the tested counts for anchored shapes and none of the tested counts for plain shapes. This does not establish behavior outside those cases.

rust-geometry-only has the lowest recorded median in 0 of 20 frame cells.
