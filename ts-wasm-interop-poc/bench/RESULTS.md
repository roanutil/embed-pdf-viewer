# Interop benchmark results

5 interleaved rounds. Node v24.20.0, darwin 25.6.0, Apple M4 Max.

Medians across rounds. "worst spread" is the widest (max-min)/median of any single implementation for that cell: read a large value as "this row drifted, do not trust two significant figures".

### Per-frame projection

| cell | poc-1 | poc-2 | poc-3 | poc-2/poc-1 | poc-3/poc-1 | worst spread |
|---|---|---|---|---|---|---|
| n=1 plain flat | 23 ns | 150 ns | 239 ns | 6.37x | 10.17x | 8% |
| n=1 plain projecting | 23 ns | 151 ns | 242 ns | 6.58x | 10.55x | 8% |
| n=1 anchored flat | 27 ns | 175 ns | 249 ns | 6.39x | 9.06x | 24% |
| n=1 anchored projecting | 147 ns | 972 ns | 394 ns | 6.60x | 2.68x | 12% |
| n=10 plain flat | 276 ns | 1.39 µs | 795 ns | 5.04x | 2.88x | 9% |
| n=10 plain projecting | 240 ns | 1.38 µs | 783 ns | 5.76x | 3.26x | 22% |
| n=10 anchored flat | 225 ns | 1.61 µs | 821 ns | 7.15x | 3.65x | 40% |
| n=10 anchored projecting | 1.34 µs | 9.47 µs | 2.24 µs | 7.08x | 1.68x | 7% |
| n=100 plain flat | 2.22 µs | 13.59 µs | 6.27 µs | 6.11x | 2.82x | 21% |
| n=100 plain projecting | 2.22 µs | 13.62 µs | 6.33 µs | 6.13x | 2.85x | 22% |
| n=100 anchored flat | 2.36 µs | 15.95 µs | 6.62 µs | 6.77x | 2.81x | 24% |
| n=100 anchored projecting | 12.70 µs | 94.61 µs | 20.43 µs | 7.45x | 1.61x | 6% |
| n=1000 plain flat | 22.32 µs | 134.91 µs | 64.66 µs | 6.04x | 2.90x | 18% |
| n=1000 plain projecting | 21.37 µs | 135.52 µs | 63.08 µs | 6.34x | 2.95x | 17% |
| n=1000 anchored flat | 24.71 µs | 155.94 µs | 66.92 µs | 6.31x | 2.71x | 18% |
| n=1000 anchored projecting | 126.96 µs | 929.54 µs | 204.69 µs | 7.32x | 1.61x | 12% |
| n=10000 plain flat | 215.16 µs | 1.35 ms | 681.15 µs | 6.27x | 3.17x | 32% |
| n=10000 plain projecting | 215.63 µs | 1.34 ms | 684.06 µs | 6.19x | 3.17x | 28% |
| n=10000 anchored flat | 233.52 µs | 1.55 ms | 707.90 µs | 6.63x | 3.03x | 30% |
| n=10000 anchored projecting | 1.30 ms | 9.41 ms | 2.10 ms | 7.25x | 1.62x | 12% |

### poc-3: what the object decode costs

poc-3 crosses the boundary once per frame either way. The only difference between these two columns is whether the returned `Float64Array` is turned into `DisplayList` objects, which is the same allocation poc-1 performs. If the decode dominates, moving the math to Rust cannot pay off until the renderer reads floats directly.

| cell | poc-1 (objects) | poc-3 (objects) | poc-3 (raw buffer) | decode share | raw vs poc-1 |
|---|---|---|---|---|---|
| n=1 plain flat | 23 ns | 239 ns | 200 ns | 16% | 8.53x |
| n=1 plain projecting | 23 ns | 242 ns | 201 ns | 17% | 8.76x |
| n=1 anchored flat | 27 ns | 249 ns | 212 ns | 15% | 7.72x |
| n=1 anchored projecting | 147 ns | 394 ns | 352 ns | 11% | 2.39x |
| n=10 plain flat | 276 ns | 795 ns | 496 ns | 38% | 1.80x |
| n=10 plain projecting | 240 ns | 783 ns | 500 ns | 36% | 2.08x |
| n=10 anchored flat | 225 ns | 821 ns | 536 ns | 35% | 2.38x |
| n=10 anchored projecting | 1.34 µs | 2.24 µs | 1.94 µs | 14% | 1.45x |
| n=100 plain flat | 2.22 µs | 6.27 µs | 3.66 µs | 42% | 1.65x |
| n=100 plain projecting | 2.22 µs | 6.33 µs | 3.68 µs | 42% | 1.66x |
| n=100 anchored flat | 2.36 µs | 6.62 µs | 4.06 µs | 39% | 1.72x |
| n=100 anchored projecting | 12.70 µs | 20.43 µs | 17.87 µs | 13% | 1.41x |
| n=1000 plain flat | 22.32 µs | 64.66 µs | 33.73 µs | 48% | 1.51x |
| n=1000 plain projecting | 21.37 µs | 63.08 µs | 33.50 µs | 47% | 1.57x |
| n=1000 anchored flat | 24.71 µs | 66.92 µs | 37.23 µs | 44% | 1.51x |
| n=1000 anchored projecting | 126.96 µs | 204.69 µs | 174.95 µs | 15% | 1.38x |
| n=10000 plain flat | 215.16 µs | 681.15 µs | 329.31 µs | 52% | 1.53x |
| n=10000 plain projecting | 215.63 µs | 684.06 µs | 330.81 µs | 52% | 1.53x |
| n=10000 anchored flat | 233.52 µs | 707.90 µs | 367.24 µs | 48% | 1.57x |
| n=10000 anchored projecting | 1.30 ms | 2.10 ms | 1.74 ms | 17% | 1.34x |

### Hit-test

| cell | poc-1 | poc-2 | poc-3 | poc-2/poc-1 | poc-3/poc-1 | worst spread |
|---|---|---|---|---|---|---|
| n=10 hit | 130 ns | 1.03 µs | 178 ns | 7.96x | 1.37x | 6% |
| n=10 miss | 1.25 µs | 10.22 µs | 1.55 µs | 8.19x | 1.24x | 5% |
| n=100 hit | 126 ns | 1.03 µs | 183 ns | 8.12x | 1.44x | 6% |
| n=100 miss | 12.03 µs | 101.85 µs | 15.15 µs | 8.47x | 1.26x | 6% |
| n=1000 hit | 127 ns | 1.02 µs | 183 ns | 8.07x | 1.44x | 5% |
| n=1000 miss | 119.84 µs | 1.02 ms | 152.48 µs | 8.51x | 1.27x | 6% |

### Dispatch (ns per pointerMove)

| shapes | poc-1 | poc-2 | poc-3 | poc-2/poc-1 | poc-3/poc-1 | worst spread |
|---|---|---|---|---|---|---|
| n=10 | 90 ns | 92 ns | 1.68 µs | 1.03x | 18.78x | 9% |
| n=100 | 493 ns | 496 ns | 1.70 µs | 1.01x | 3.45x | 8% |
| n=1000 | 5.89 µs | 5.81 µs | 1.95 µs | 0.99x | 0.33x | 23% |

### Allocation (100 anchored shapes, GC-free window)

These are `heapUsed` deltas, not an allocation counter. Calibration: a `{x, y}` object measures 73 bytes here against a ~24 byte theoretical minimum, so treat the absolute figures as good to about 3x. The comparison between implementations is sound, since all three are measured the same way.

| | bytes/frame | bytes/shape | window | vs poc-1 |
|---|---|---|---|---|
| poc-1 | 145326 | 1453.3 | 16 frames | 1.00x |
| poc-2 | 502981 | 5029.8 | 8 frames | 3.46x |
| poc-3 | 48527 | 485.3 | 64 frames | 0.33x |

### Cold start

Chrome DevTools MCP, 5 same-origin iframe loads per POC against `vite preview` production builds.

> localhost, and the hashed JS/wasm assets come from the memory cache. The wasm leg here is fetch-from-cache plus compile plus instantiate, NOT download. Over a real network add the transfer: 45.2 kB gzipped for poc-2, 66.6 kB for poc-3.

| | wasm shipped | instantiate leg | first frame | first frame range |
|---|---|---|---|---|
| poc-1 | none | 1.2 ms | 19.0 ms | 13.5 to 34.4 ms |
| poc-2 | 130.7 kB | 6.3 ms | 21.2 ms | 12.7 to 43.7 ms |
| poc-3 | 277.2 kB | 7.8 ms | 24.8 ms | 21.5 to 41.6 ms |

### Crossover

poc-3 first beats poc-1 at n=never anchored, n=never plain, at the projecting view.

poc-2 wins 0 of 20 frame cells, as predicted.
