# PDFium binding benchmark results

These tables summarize 56 committed measurements: seven rounds for four variants
in Wasm and native modes. The numbers can be recalculated from
[`bench/results`](bench/results/README.md). They describe the saved builds, not
current performance.

**The native measurements predate the readback-copy fix in `bench/arm.mjs`.**
Variants b/c/d made an extra JavaScript copy in `readBytes` and `readU16`.
Rerun before comparing current text extraction or byte-copy performance.
The original write-up reported agreement on two correctness fixtures; the current
check adds `flipped-p0` to cover text whose transformation reverses orientation.
The saved timing JSON does not contain the original correctness-check log.

## Method and environment

The saved files identify Node v24.20.0, Apple M4 Max, ARM64 macOS (`darwin 25.6.0`),
and Rust 1.98.0. The Wasm files identify Emscripten 3.1.72.
They use page index 4 (the fifth page) of
[`report.pdf`](../examples/snippet-react/public/report.pdf), with 8,106 glyphs.

`bench/run.mjs` runs one process per variant per round. Starting order rotates:
a/b/c/d, b/c/d/a, c/d/a/b, d/a/b/c, then repeats. The runner defaults to a 500 ms
sampling budget per timing cell; the JSON does not record that setting directly. Rotation distributes ordering effects; it does
not establish that CPU load or temperature had no effect.

The shared [timing harness](../viewer-rust-port-benchmarks/bench/harness.mjs)
times batches of calls and divides by the batch size. The first table reports
medians across rounds. The p95 table reports the median of each round's
95th percentile of batch averages. It does not measure individual-call tail
latency or worst-case frame time.

The variants are a: direct calls, b: a Rust forwarding wrapper, c: whole
operations in Rust, and d: whole operations in C++.
See [README.md](README.md) for build commands and comparison limits.

## Wasm

### Median nanoseconds per call, by workload and arm

| workload | a direct | b rust thin | c rust coarse | d c++ coarse | b/a | c/a | d/a |
|---|---|---|---|---|---|---|---|
| `null-call` | 351.2 ns | 329.4 ns | n/a | n/a | 0.94x | n/a | n/a |
| `null-peek` | 4.0 ns | 3.7 ns | n/a | n/a | 0.91x | n/a | n/a |
| `null-readbytes` | 30.6 ns | 30.4 ns | n/a | n/a | 0.99x | n/a | n/a |
| `geometry` | 2.788 ms | 2.838 ms | 1.070 ms | 1.057 ms | 1.02x | 0.38x | 0.38x |
| `geometry-copyout` | n/a | n/a | 22.39 µs | 21.72 µs | n/a | n/a | n/a |
| `text` | 120.29 µs | 122.01 µs | 122.52 µs | 120.07 µs | 1.01x | 1.02x | 1.00x |
| `render-1x` | 2.877 ms | 2.909 ms | 2.924 ms | 2.859 ms | 1.01x | 1.02x | 0.99x |
| `render-4x` | 16.095 ms | 16.689 ms | 17.089 ms | 16.109 ms | 1.04x | 1.06x | 1.00x |
| `render-copyout-4x` | 640.22 µs | 651.36 µs | 637.49 µs | 648.81 µs | 1.02x | 1.00x | 1.01x |
| `search` | 102.36 µs | 104.28 µs | 30.61 µs | 31.92 µs | 1.02x | 0.30x | 0.31x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 378.1 ns | 360.5 ns | n/a | n/a |
| `null-peek` | 4.2 ns | 4.1 ns | n/a | n/a |
| `null-readbytes` | 32.1 ns | 31.9 ns | n/a | n/a |
| `geometry` | 3.195 ms | 3.225 ms | 1.218 ms | 1.150 ms |
| `geometry-copyout` | n/a | n/a | 27.04 µs | 26.04 µs |
| `text` | 126.44 µs | 130.05 µs | 129.73 µs | 126.01 µs |
| `render-1x` | 3.002 ms | 3.111 ms | 3.009 ms | 3.011 ms |
| `render-4x` | 16.526 ms | 17.348 ms | 17.681 ms | 16.714 ms |
| `render-copyout-4x` | 700.59 µs | 716.84 µs | 698.48 µs | 706.24 µs |
| `search` | 106.08 µs | 108.44 µs | 32.44 µs | 33.33 µs |

### Module size

| arm | wasm bytes | delta vs a |
|---|---|---|
| a direct | 4,676,714 | n/a |
| b rust thin | 4,680,516 | +3.7 KiB |
| c rust coarse | 4,682,622 | +5.8 KiB |
| d c++ coarse | 4,685,996 | +9.1 KiB |

## What the Wasm results show

Geometry took 2.788 ms with direct calls, 1.070 ms with whole Rust operations,
and 1.057 ms with whole C++ operations: about a 2.6× speedup in both cases.
Search improved by about 3.3× with Rust and 3.2× with C++.

Text and rendering changed much less. For example, the Rust operation took
1.02× the direct-call time for text and 1.06× for rendering at scale 4.
These measurements support reducing repeated JavaScript calls for geometry and
search on this fixture. They do not establish a general language advantage.

The Rust forwarding variant was faster on `null-call` in all seven rounds.
The cause is unresolved. Separately linked binaries can differ in more than
wrapper call count, so this result does not isolate the wrapper's cost.
Likewise, the render differences do not establish a compiler or code-layout
mechanism without a controlled experiment.

## Native

### Median nanoseconds per call, by workload and arm

| workload | a direct | b rust thin | c rust coarse | d c++ coarse | b/a | c/a | d/a |
|---|---|---|---|---|---|---|---|
| `null-call` | 200.3 ns | 192.6 ns | n/a | n/a | 0.96x | n/a | n/a |
| `null-peek` | 39.2 ns | 15.0 ns | n/a | n/a | 0.38x | n/a | n/a |
| `null-readbytes` | 276.1 ns | 288.9 ns | n/a | n/a | 1.05x | n/a | n/a |
| `geometry` | 8.642 ms | 4.134 ms | 577.37 µs | 581.16 µs | 0.48x | 0.07x | 0.07x |
| `geometry-copyout` | n/a | n/a | 71.31 µs | 72.90 µs | n/a | n/a | n/a |
| `text` | 126.94 µs | 127.80 µs | 127.59 µs | 128.30 µs | 1.01x | 1.01x | 1.01x |
| `render-1x` | 1.556 ms | 1.555 ms | 1.556 ms | 1.575 ms | 1.00x | 1.00x | 1.01x |
| `render-4x` | 8.555 ms | 8.556 ms | 8.564 ms | 8.721 ms | 1.00x | 1.00x | 1.02x |
| `render-copyout-4x` | 1.706 ms | 2.398 ms | 2.498 ms | 2.647 ms | 1.41x | 1.46x | 1.55x |
| `search` | 82.19 µs | 64.48 µs | 39.84 µs | 39.99 µs | 0.78x | 0.48x | 0.49x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 213.4 ns | 205.1 ns | n/a | n/a |
| `null-peek` | 41.3 ns | 15.4 ns | n/a | n/a |
| `null-readbytes` | 289.9 ns | 301.9 ns | n/a | n/a |
| `geometry` | 10.006 ms | 4.532 ms | 627.10 µs | 634.90 µs |
| `geometry-copyout` | n/a | n/a | 89.84 µs | 92.81 µs |
| `text` | 134.02 µs | 135.18 µs | 135.58 µs | 137.11 µs |
| `render-1x` | 1.617 ms | 1.609 ms | 1.617 ms | 1.652 ms |
| `render-4x` | 8.818 ms | 8.931 ms | 8.864 ms | 9.109 ms |
| `render-copyout-4x` | 1.919 ms | 3.421 ms | 3.743 ms | 3.559 ms |
| `search` | 84.92 µs | 67.17 µs | 43.12 µs | 43.09 µs |

## What the native results show

Within the Rust addon, geometry took 4.134 ms with individual calls and
577.37 µs with a whole-operation call, about a 7.16× speedup. The C++ comparison
was about 14.87×, but also changes pointer representation and memory-reading
helpers. It cannot isolate the effect of grouping calls by itself.

Search improved by about 1.62× within the Rust addon and 2.06× between the C++
variants. Those gains were smaller than the Wasm search gains on this fixture.
Rendering times were within about 2% across native variants.

The byte-copy results require particular care: they include the extra copy
removed from b/c/d since this run. The largest native round-to-round variation
was c's `render-copyout-4x`, at 1.75× between its fastest and slowest rounds.
Variation alone cannot identify thermal throttling or cache effects.

## Limits of the comparison

1. The timed workload uses one report page. Passing the correctness check on
   other fixtures does not measure their performance or prove correctness for
   every PDF.
2. `bench/workloads.mjs` implements a simplified geometry reader. It does not
   run the full [PageGeometryReader](../packages/engine/services/src/features/geometry/PageGeometryReader.ts),
   including cancellation checks and rectangle normalization.
3. Thin geometry reads use memory helpers; whole-operation geometry reads use
   `DataView`. The comparison includes that decoding difference as well as
   different call counts. The benchmark also bypasses the runtime's general
   [Wasm argument conversion](../packages/engine/runtime/src/wasm/wasm-runtime.ts).
4. Native a uses `node-addon-api` with BigInt pointers, b/c use `napi-rs` with
   numeric pointers, and d uses `node-addon-api` with numeric pointers. Native
   buffer views also require addon work that Wasm heap views do not.
5. Build settings differ: the Rust release profile enables full link-time
   optimization, while CMake requests its toolchain's interprocedural
   optimization. The saved measurements do not isolate those choices.

For current results, rebuild and run both modes into fresh output directories
using the [README commands](README.md). Keep the new raw measurements with any
new performance claims.
