# Swift versus Rust benchmark results

Measured on 2026-09-18.

## Summary

- Swift's PDFium implementations perform approximately the same as Rust for
  text extraction, search, and rendering.
- The native Swift artifacts are 75.7% to 76.4% smaller than the corresponding
  Rust artifacts, excluding the shared PDFium dynamic library.
- The complete Swift PDFium Wasm product is 2,581 bytes (0.055%) larger than
  Rust because both products are dominated by the same 4.46 MiB PDFium module.
- The current Swift viewer geometry Wasm is 45.4% smaller than the matching
  Rust Wasm, but its frame projection and hit-test boundaries are approximately
  1.8 to 2.1 times slower and allocate 1.73 times as many JavaScript-observed
  bytes per frame.

## Test environment

| Component | Version |
|---|---|
| Hardware | Apple M4 Max |
| Operating system | Darwin 27.0.0, arm64 |
| Swift | Apple Swift 6.4 (`swiftlang-6.4.0.34.1`) |
| Rust | `rustc 1.98.0 (88d9e12ae 2026-08-18)` |
| Node.js | 26.9.0 |
| Emscripten | 3.1.72 |

## PDFium binding benchmark

The comparable arms are Rust coarse and Swift coarse. All arms passed the
fairness gate for three fixtures and five workloads. Results are medians from
seven interleaved rounds with a 500 ms workload budget.

### Wasm performance

| Workload | Rust coarse | Swift coarse | Swift relative to Rust |
|---|---:|---:|---:|
| Geometry | 823.23 us | 800.21 us | 2.8% faster |
| Geometry copy-out | 19.20 us | 18.52 us | 3.5% faster |
| Text extraction | 119.92 us | 115.04 us | 4.1% faster |
| Render 1x | 2.356 ms | 2.310 ms | 2.0% faster |
| Render 4x | 14.813 ms | 14.194 ms | 4.2% faster |
| Render copy-out 4x | 571.95 us | 562.93 us | 1.6% faster |
| Search | 32.22 us | 35.05 us | 8.8% slower |

### Wasm product size

These totals include the common PDFium Wasm payload.

| Product | Total bytes | Increase over direct baseline |
|---|---:|---:|
| Direct PDFium baseline | 4,676,714 | - |
| Rust coarse | 4,682,786 | 6,072 bytes (5.9 KiB) |
| Swift coarse | 4,685,367 | 8,653 bytes (8.5 KiB) |

The complete Swift product is 2,581 bytes, or 0.055%, larger than Rust. Looking
only at the language wrapper contribution, Swift is 42.5% larger.

### Native performance

| Workload | Rust coarse | Swift coarse | Swift relative to Rust |
|---|---:|---:|---:|
| Geometry | 572.81 us | 586.73 us | 2.4% slower |
| Geometry copy-out | 49.34 us | 49.21 us | 0.3% faster |
| Text extraction | 123.00 us | 122.95 us | effectively tied |
| Render 1x | 1.435 ms | 1.440 ms | 0.3% slower |
| Render 4x | 8.035 ms | 8.114 ms | 1.0% slower |
| Render copy-out 4x | 1.602 ms | 1.594 ms | 0.5% faster |
| Search | 40.31 us | 40.31 us | tied |

### Native artifact size

The common PDFium dynamic library is excluded. The Swift value includes its
addon shim and Swift dynamic library.

| Artifact | Bytes | Difference |
|---|---:|---:|
| Rust coarse | 476,464 | - |
| Swift coarse | 112,624 | 363,840 bytes (76.4%) smaller |

## Direct native PDFium prototype

Results are medians from seven interleaved rounds of 30 iterations. Both
executables dynamically use the same PDFium library.

| Operation | Rust | Swift | Swift relative to Rust |
|---|---:|---:|---:|
| Page count | 5.453 us | 0.286 us | 94.8% faster |
| Page size | 420.779 us | 1.065 us | 99.7% faster |
| Text extraction | 1.085 ms | 1.191 ms | 9.8% slower |
| Search | 1.091 ms | 1.077 ms | 1.3% faster |
| Render | 944.857 us | 939.057 us | 0.6% faster |

The page-count and page-size difference is not a language-only comparison: the
Rust API dispatches through its worker while Swift makes synchronous direct
calls. Search and rendering, where PDFium performs most of the work, are within
1.3%.

| Executable | Bytes | Difference |
|---|---:|---:|
| Rust | 563,424 | - |
| Swift | 137,024 | 426,400 bytes (75.7%) smaller |

These executable sizes exclude the common PDFium dynamic library. They also do
not represent the cost of Swift system libraries already supplied by Apple
platforms.

## Viewer geometry Wasm benchmark

The matching comparison is `rust-geometry-only` versus
`swift-geometry-only`. All four benchmark implementations produced the same 10
display items in the fairness probe. Timings are medians from five interleaved
rounds with a 500 ms budget; allocation results use three rounds.

### Wasm size

| Implementation | Bytes | Difference |
|---|---:|---:|
| Rust geometry-only | 134,411 | - |
| Swift geometry-only | 73,400 | 61,011 bytes (45.4%) smaller |

### Representative performance

| Workload | Rust geometry-only | Swift geometry-only | Swift/Rust |
|---|---:|---:|---:|
| Frame: 1,000 plain, flat shapes | 124.62 us | 264.62 us | 2.12x |
| Frame: 1,000 anchored, projected shapes | 863.91 us | 1.59 ms | 1.84x |
| Hit-test: 1,000 shapes, hit | 0.961 us | 1.82 us | 1.89x |
| Hit-test: 1,000 shapes, miss | 951.35 us | 1.81 ms | 1.90x |
| Pointer move: 1,000 shapes | 3.65 us | 3.70 us | 1.01x |

### JavaScript-observed allocation

For 100 anchored shapes, the benchmark observes `heapUsed + arrayBuffers`
during a window with no observed garbage collection. It is not a native
allocation counter and excludes Rust and Swift internal allocations.

| Implementation | Bytes per frame | Relative |
|---|---:|---:|
| Rust geometry-only | 469,130 | 1.00x |
| Swift geometry-only | 813,804 | 1.73x |

## Interpretation

For the PDFium boundary, neither language has a material throughput advantage:
PDFium dominates the substantial work, and most native results differ by less
than 1%. Swift currently produces much smaller native language-side artifacts.

For the viewer geometry boundary, Swift produces the smaller Wasm module but
pays more for calls and copied results crossing into JavaScript. The pointer
state calculation itself is tied, which points to the geometry result boundary
and allocation behavior as the first optimization target rather than Swift's
arithmetic performance.

## Reproduction

```bash
cd pdfium-binding-benchmarks
node bench/compare.mjs --out /tmp/epdf-swift-rust-binding-wasm
node bench/compare.mjs --native --out /tmp/epdf-swift-rust-binding-native

cd ../viewer-rust-port-benchmarks
node bench/compare.mjs --stdout-only
```

The viewer's generated per-round files are under
`viewer-rust-port-benchmarks/bench/results/`. The PDFium comparison inputs from
this run are under `/tmp/epdf-swift-rust-binding-wasm/` and
`/tmp/epdf-swift-rust-binding-native/`.
