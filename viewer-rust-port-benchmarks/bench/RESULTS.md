# Interop benchmark results

Historical report: 5 interleaved rounds on Node v24.20.0, darwin 25.6.0,
Apple M4 Max, according to the original write-up. The raw timing and allocation
samples are not committed, so those measurements and the reported environment
cannot be independently verified from this checkout. The tables are retained
as a historical record, not as verified current results.

Only the [cold-start summary](results/cold-start.json) is committed. It contains
summary values, not the individual browser samples. See [README.md](README.md)
for a new run; `compare.mjs` overwrites this report.

Medians across rounds. "worst spread" is the widest (max-min)/median of any single implementation for that cell: a large value indicates unstable timings, without identifying the cause.

### Per-frame projection

| cell | typescript-baseline | rust-geometry-only | rust-geometry-and-state | rust-geometry-only/typescript-baseline | rust-geometry-and-state/typescript-baseline | worst spread |
|---|---|---|---|---|---|---|
| n=1 plain flat | 23 ns | 147 ns | 239 ns | 6.33x | 10.27x | 2% |
| n=1 plain projecting | 23 ns | 148 ns | 241 ns | 6.50x | 10.57x | 2% |
| n=1 anchored flat | 27 ns | 172 ns | 246 ns | 6.37x | 9.08x | 3% |
| n=1 anchored projecting | 147 ns | 958 ns | 387 ns | 6.51x | 2.63x | 8% |
| n=10 plain flat | 271 ns | 1.37 µs | 787 ns | 5.06x | 2.90x | 3% |
| n=10 plain projecting | 234 ns | 1.36 µs | 774 ns | 5.80x | 3.31x | 17% |
| n=10 anchored flat | 250 ns | 1.59 µs | 813 ns | 6.36x | 3.25x | 31% |
| n=10 anchored projecting | 1.28 µs | 9.34 µs | 2.20 µs | 7.31x | 1.72x | 4% |
| n=100 plain flat | 2.20 µs | 13.55 µs | 6.21 µs | 6.17x | 2.83x | 17% |
| n=100 plain projecting | 2.19 µs | 13.40 µs | 6.21 µs | 6.13x | 2.84x | 17% |
| n=100 anchored flat | 2.33 µs | 15.63 µs | 6.55 µs | 6.70x | 2.81x | 19% |
| n=100 anchored projecting | 12.57 µs | 93.23 µs | 20.14 µs | 7.41x | 1.60x | 4% |
| n=1000 plain flat | 21.98 µs | 131.89 µs | 62.66 µs | 6.00x | 2.85x | 12% |
| n=1000 plain projecting | 20.98 µs | 132.37 µs | 62.27 µs | 6.31x | 2.97x | 10% |
| n=1000 anchored flat | 22.30 µs | 153.36 µs | 66.22 µs | 6.88x | 2.97x | 16% |
| n=1000 anchored projecting | 121.96 µs | 913.60 µs | 200.76 µs | 7.49x | 1.65x | 4% |
| n=10000 plain flat | 211.16 µs | 1.32 ms | 663.84 µs | 6.23x | 3.14x | 15% |
| n=10000 plain projecting | 210.89 µs | 1.31 ms | 662.15 µs | 6.21x | 3.14x | 15% |
| n=10000 anchored flat | 198.93 µs | 1.54 ms | 699.43 µs | 7.72x | 3.52x | 15% |
| n=10000 anchored projecting | 1.26 ms | 9.23 ms | 2.04 ms | 7.35x | 1.63x | 7% |

### rust-geometry-and-state: what the object decode costs

The two Rust columns compare returning a numeric buffer with converting it
into JavaScript display objects. “Decode share” is the relative difference
between the two recorded medians, not a separately timed conversion step.
Both paths use one Wasm call once the metadata cache is current.

| cell | typescript-baseline (objects) | rust-geometry-and-state (objects) | rust-geometry-and-state (raw buffer) | decode share | raw vs typescript-baseline |
|---|---|---|---|---|---|
| n=1 plain flat | 23 ns | 239 ns | 199 ns | 17% | 8.56x |
| n=1 plain projecting | 23 ns | 241 ns | 200 ns | 17% | 8.78x |
| n=1 anchored flat | 27 ns | 246 ns | 207 ns | 16% | 7.66x |
| n=1 anchored projecting | 147 ns | 387 ns | 346 ns | 11% | 2.35x |
| n=10 plain flat | 271 ns | 787 ns | 491 ns | 38% | 1.81x |
| n=10 plain projecting | 234 ns | 774 ns | 494 ns | 36% | 2.11x |
| n=10 anchored flat | 250 ns | 813 ns | 529 ns | 35% | 2.11x |
| n=10 anchored projecting | 1.28 µs | 2.20 µs | 1.91 µs | 13% | 1.50x |
| n=100 plain flat | 2.20 µs | 6.21 µs | 3.59 µs | 42% | 1.63x |
| n=100 plain projecting | 2.19 µs | 6.21 µs | 3.59 µs | 42% | 1.64x |
| n=100 anchored flat | 2.33 µs | 6.55 µs | 3.99 µs | 39% | 1.71x |
| n=100 anchored projecting | 12.57 µs | 20.14 µs | 17.57 µs | 13% | 1.40x |
| n=1000 plain flat | 21.98 µs | 62.66 µs | 33.03 µs | 47% | 1.50x |
| n=1000 plain projecting | 20.98 µs | 62.27 µs | 32.95 µs | 47% | 1.57x |
| n=1000 anchored flat | 22.30 µs | 66.22 µs | 36.68 µs | 45% | 1.64x |
| n=1000 anchored projecting | 121.96 µs | 200.76 µs | 171.43 µs | 15% | 1.41x |
| n=10000 plain flat | 211.16 µs | 663.84 µs | 323.85 µs | 51% | 1.53x |
| n=10000 plain projecting | 210.89 µs | 662.15 µs | 323.74 µs | 51% | 1.54x |
| n=10000 anchored flat | 198.93 µs | 699.43 µs | 361.97 µs | 48% | 1.82x |
| n=10000 anchored projecting | 1.26 ms | 2.04 ms | 1.71 ms | 17% | 1.36x |

### Hit-test

| cell | typescript-baseline | rust-geometry-only | rust-geometry-and-state | rust-geometry-only/typescript-baseline | rust-geometry-and-state/typescript-baseline | worst spread |
|---|---|---|---|---|---|---|
| n=10 hit | 128 ns | 1.01 µs | 177 ns | 7.89x | 1.38x | 2% |
| n=10 miss | 1.24 µs | 10.13 µs | 1.54 µs | 8.17x | 1.24x | 2% |
| n=100 hit | 126 ns | 1.02 µs | 181 ns | 8.06x | 1.43x | 3% |
| n=100 miss | 11.95 µs | 100.82 µs | 15.10 µs | 8.44x | 1.26x | 4% |
| n=1000 hit | 126 ns | 1.02 µs | 182 ns | 8.06x | 1.44x | 2% |
| n=1000 miss | 119.60 µs | 1.01 ms | 150.76 µs | 8.46x | 1.26x | 2% |

### State updates (time per pointerMove)

| shapes | typescript-baseline | rust-geometry-only | rust-geometry-and-state | rust-geometry-only/typescript-baseline | rust-geometry-and-state/typescript-baseline | worst spread |
|---|---|---|---|---|---|---|
| n=10 | 76 ns | 78 ns | 1.66 µs | 1.02x | 21.94x | 2% |
| n=100 | 348 ns | 352 ns | 1.68 µs | 1.01x | 4.82x | 2% |
| n=1000 | 3.70 µs | 3.66 µs | 1.92 µs | 0.99x | 0.52x | 15% |

### Allocation (100 anchored shapes, historical heap-only estimates)

These historical samples use the obsolete heap-only metric and undercount the
Rust-owned-state variant’s copied typed-array backing stores. Rerun allocation
rounds with the current harness before using this table to compare variants.

The table uses changes in `heapUsed`, excluding typed-array backing storage
and Rust allocations. The original report gave a calibration value of 73 bytes
per `{x, y}` object, but its samples are unavailable. No accuracy bound can be
established from that summary.

| | bytes/frame | bytes/shape | window | vs typescript-baseline |
|---|---|---|---|---|
| typescript-baseline | 145326 | 1453.3 | 16 frames | 1.00x |
| rust-geometry-only | 502981 | 5029.8 | 8 frames | 3.46x |
| rust-geometry-and-state | 48527 | 485.3 | 64 frames | 0.33x |

### Cold start

The committed summary describes five same-origin iframe loads per variant
against Vite production previews, collected through Chrome DevTools MCP.
The asset byte counts below are recorded build sizes, not current build outputs.
The “ready interval” is `wasmReady - boot`, including in the TypeScript baseline.
“First frame” is recorded by the mounted viewer’s React effect after its
initial render, relative to navigation start. It does not time browser painting. Cache conditions and individual samples
were not retained in a form that lets these timings be independently checked.

| | wasm shipped | ready interval | first frame | first frame range |
|---|---|---|---|---|
| typescript-baseline | none | 1.2 ms | 19.0 ms | 13.5 to 34.4 ms |
| rust-geometry-only | 130.7 KiB | 6.3 ms | 21.2 ms | 12.7 to 43.7 ms |
| rust-geometry-and-state | 277.2 KiB | 7.8 ms | 24.8 ms | 21.5 to 41.6 ms |

### Crossover

In these recorded frame tables, neither Rust variant beats the TypeScript
baseline at any tested size (1, 10, 100, 1,000, or 10,000 shapes). This does
not establish that no crossover exists outside the tested cases. The state-update
table does report a Rust geometry-and-state advantage at 1,000 shapes.
