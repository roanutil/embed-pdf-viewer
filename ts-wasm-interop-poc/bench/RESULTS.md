# Interop benchmark results

5 interleaved rounds. Node v24.20.0, darwin 25.6.0, Apple M4 Max.

Medians across rounds. "worst spread" is the widest (max-min)/median of any single implementation for that cell: read a large value as "this row drifted, do not trust two significant figures".

### Per-frame projection

| cell | poc-1 | poc-2 | poc-3 | poc-2/poc-1 | poc-3/poc-1 | worst spread |
|---|---|---|---|---|---|---|
| n=1 plain flat | 33 ns | 156 ns | 246 ns | 4.78x | 7.52x | 15% |
| n=1 plain projecting | 35 ns | 155 ns | 251 ns | 4.47x | 7.24x | 5% |
| n=1 anchored flat | 40 ns | 180 ns | 255 ns | 4.46x | 6.31x | 6% |
| n=1 anchored projecting | 151 ns | 999 ns | 400 ns | 6.60x | 2.64x | 8% |
| n=10 plain flat | 251 ns | 1.42 µs | 807 ns | 5.66x | 3.21x | 13% |
| n=10 plain projecting | 243 ns | 1.39 µs | 791 ns | 5.71x | 3.25x | 13% |
| n=10 anchored flat | 257 ns | 1.67 µs | 837 ns | 6.51x | 3.26x | 17% |
| n=10 anchored projecting | 1.30 µs | 9.63 µs | 2.26 µs | 7.41x | 1.74x | 4% |
| n=100 plain flat | 2.25 µs | 13.96 µs | 6.46 µs | 6.19x | 2.86x | 4% |
| n=100 plain projecting | 2.31 µs | 13.79 µs | 6.36 µs | 5.97x | 2.75x | 5% |
| n=100 anchored flat | 2.44 µs | 16.69 µs | 6.77 µs | 6.85x | 2.78x | 6% |
| n=100 anchored projecting | 12.90 µs | 95.01 µs | 21.23 µs | 7.37x | 1.65x | 4% |
| n=1000 plain flat | 22.51 µs | 134.51 µs | 65.50 µs | 5.98x | 2.91x | 7% |
| n=1000 plain projecting | 22.42 µs | 136.78 µs | 64.95 µs | 6.10x | 2.90x | 15% |
| n=1000 anchored flat | 23.82 µs | 161.92 µs | 68.32 µs | 6.80x | 2.87x | 14% |
| n=1000 anchored projecting | 125.55 µs | 948.99 µs | 205.55 µs | 7.56x | 1.64x | 6% |
| n=10000 plain flat | 224.63 µs | 1.35 ms | 714.83 µs | 6.02x | 3.18x | 13% |
| n=10000 plain projecting | 222.02 µs | 1.34 ms | 693.05 µs | 6.04x | 3.12x | 11% |
| n=10000 anchored flat | 220.14 µs | 1.66 ms | 738.95 µs | 7.56x | 3.36x | 15% |
| n=10000 anchored projecting | 1.30 ms | 9.70 ms | 2.11 ms | 7.46x | 1.62x | 6% |

### poc-3: what the object decode costs

poc-3 crosses the boundary once per frame either way. The only difference between these two columns is whether the returned `Float64Array` is turned into `DisplayList` objects, which is the same allocation poc-1 performs. If the decode dominates, moving the math to Rust cannot pay off until the renderer reads floats directly.

| cell | poc-1 (objects) | poc-3 (objects) | poc-3 (raw buffer) | decode share | raw vs poc-1 |
|---|---|---|---|---|---|
| n=1 plain flat | 33 ns | 246 ns | 204 ns | 17% | 6.26x |
| n=1 plain projecting | 35 ns | 251 ns | 210 ns | 16% | 6.06x |
| n=1 anchored flat | 40 ns | 255 ns | 216 ns | 15% | 5.35x |
| n=1 anchored projecting | 151 ns | 400 ns | 358 ns | 10% | 2.37x |
| n=10 plain flat | 251 ns | 807 ns | 511 ns | 37% | 2.03x |
| n=10 plain projecting | 243 ns | 791 ns | 517 ns | 35% | 2.13x |
| n=10 anchored flat | 257 ns | 837 ns | 550 ns | 34% | 2.14x |
| n=10 anchored projecting | 1.30 µs | 2.26 µs | 1.98 µs | 13% | 1.52x |
| n=100 plain flat | 2.25 µs | 6.46 µs | 3.71 µs | 43% | 1.64x |
| n=100 plain projecting | 2.31 µs | 6.36 µs | 3.74 µs | 41% | 1.62x |
| n=100 anchored flat | 2.44 µs | 6.77 µs | 4.13 µs | 39% | 1.70x |
| n=100 anchored projecting | 12.90 µs | 21.23 µs | 18.21 µs | 14% | 1.41x |
| n=1000 plain flat | 22.51 µs | 65.50 µs | 34.63 µs | 47% | 1.54x |
| n=1000 plain projecting | 22.42 µs | 64.95 µs | 34.84 µs | 46% | 1.55x |
| n=1000 anchored flat | 23.82 µs | 68.32 µs | 38.59 µs | 44% | 1.62x |
| n=1000 anchored projecting | 125.55 µs | 205.55 µs | 176.98 µs | 14% | 1.41x |
| n=10000 plain flat | 224.63 µs | 714.83 µs | 338.52 µs | 53% | 1.51x |
| n=10000 plain projecting | 222.02 µs | 693.05 µs | 339.23 µs | 51% | 1.53x |
| n=10000 anchored flat | 220.14 µs | 738.95 µs | 384.69 µs | 48% | 1.75x |
| n=10000 anchored projecting | 1.30 ms | 2.11 ms | 1.76 ms | 17% | 1.35x |

### Hit-test

| cell | poc-1 | poc-2 | poc-3 | poc-2/poc-1 | poc-3/poc-1 | worst spread |
|---|---|---|---|---|---|---|
| n=10 hit | 132 ns | 1.04 µs | 180 ns | 7.89x | 1.37x | 3% |
| n=10 miss | 1.26 µs | 10.39 µs | 1.60 µs | 8.25x | 1.27x | 3% |
| n=100 hit | 134 ns | 1.04 µs | 187 ns | 7.78x | 1.39x | 3% |
| n=100 miss | 12.74 µs | 104.04 µs | 15.42 µs | 8.17x | 1.21x | 4% |
| n=1000 hit | 134 ns | 1.05 µs | 186 ns | 7.89x | 1.39x | 3% |
| n=1000 miss | 126.71 µs | 1.04 ms | 154.37 µs | 8.20x | 1.22x | 3% |

### Dispatch (ns per pointerMove)

| shapes | poc-1 | poc-2 | poc-3 | poc-2/poc-1 | poc-3/poc-1 | worst spread |
|---|---|---|---|---|---|---|
| n=10 | 92 ns | 95 ns | 1.73 µs | 1.03x | 18.71x | 3% |
| n=100 | 501 ns | 510 ns | 1.73 µs | 1.02x | 3.46x | 6% |
| n=1000 | 6.06 µs | 6.04 µs | 1.98 µs | 1.00x | 0.33x | 22% |

### Allocation (100 anchored shapes, GC-free window)

These are `heapUsed` deltas, not an allocation counter. Calibration: a `{x, y}` object measures 73 bytes here against a ~24 byte theoretical minimum, so treat the absolute figures as good to about 3x. The comparison between implementations is sound, since all three are measured the same way.

| | bytes/frame | bytes/shape | window | vs poc-1 |
|---|---|---|---|---|
| poc-1 | 4376 | 43.8 | 512 frames | 1.00x |
| poc-2 | 4351 | 43.5 | 512 frames | 0.99x |
| poc-3 | 7774 | 77.7 | 512 frames | 1.78x |

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
