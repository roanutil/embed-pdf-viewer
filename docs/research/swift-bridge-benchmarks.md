# Swift bridge benchmarks: uniffi against a hand-written C bridge

We keep uniffi as the bridge for every plugin, and we hand-write exactly two kinds of path: page
rasters (B5) and large record arrays (the search hit list, and whatever B1 and B2 add of the same
shape). Pointer samples in B1 stay on uniffi. On a call that moves a few bytes, the two bridges
cost the same, because the hop to the kernel thread is 99.9% of the call. On a call that moves
megabytes or thousands of records, uniffi is 27x to 786x slower, and the difference is all copying
and serialization.

These are provisional numbers from a local dev machine, which is noisy. Numbers from the dedicated
perf Mac are pending (it didn't resolve on the day of the run), and so are device numbers, which need
B5's signing. Don't make the final call on rasters until the device numbers exist.

## Setup

| | |
|---|---|
| Machine | Apple M4 Max, 16 cores, 64 GB. The local dev machine, not the perf Mac. Microsoft Defender and Screens 5 were running. |
| macOS | 27.0.1 |
| Xcode | 27.0 (27A266a) |
| Simulator | iPhone 17, iOS 26.5 (`5FA32A5D-6186-4C16-BE25-C87829DD1BFA`) |
| Rust | 1.98.0, `PROFILE=release` (`lto = true`, `codegen-units = 1`) |
| Swift | `-configuration Release` |
| Code | `459bbe779` plus the commit that adds this document |

The manual bridge is `crates/epdf-manual-bridge`, a C ABI declared by hand in
[`include/epdf_manual.h`](../../crates/epdf-manual-bridge/include/epdf_manual.h), with its Swift side in
`platforms/apple/EmbedPDF/Sources/EmbedPDFManualBridge`. It links into `epdf-viewer-host`'s own
staticlib behind the `manual-bridge` feature, so both bridges share one kernel thread and one
`ViewerHost`. Both start the same plugin work: the uniffi exports and the C exports call the same
`start_search`, `start_find_all` and `start_render_page` helpers in the plugins' `ffi` modules.

Every workload ran through both bridges against the same `PDFViewer`, in
[`BridgeBenchmarks.swift`](../../platforms/apple/EmbedPDF/Tests/EmbedPDFBridgeBenchmarks/BridgeBenchmarks.swift):
200 warm-up iterations (or N, when N is smaller), then 5 runs of N, timed with `ContinuousClock`.
Median and p95 are over every sample of every run. A sample is one call, except `noop`, where it is
the mean of a batch of 100 (one call is shorter than the clock's 41.7 ns tick). "Runs" is the range
of the 5 per-run means.

The tables are run 2. Run 1 agrees within the noise shown below, except that it lost
`progressEvents` to an xctest kill ("Test crashed with signal kill") after `findAll`. Run alone, and
again in run 2, that test passed, so its numbers come from run 2 and a solo run. I don't know what
killed it.

## Results

Ratio is uniffi's median over the manual median: above 1 means manual is faster.

### Control calls

| Workload | N | uniffi median / p95 | manual median / p95 | Ratio |
|---|---|---|---|---|
| `noop` | 100,000 | 6.25 ns / 7.09 ns | 2.50 ns / 2.92 ns | 2.5 |
| `noop_hop` | 10,000 | 11.96 µs / 38.92 µs | 10.75 µs / 25.25 µs | 1.11 |

Verdict: equal where it matters. uniffi's entry costs about 4 ns more than a bare C call (run 1:
6.25 ns against 1.25 ns). The hop to the kernel thread costs 6 µs to 12 µs on either bridge, and
run 1 had the ratio the other way round (6.42 µs against 6.58 µs). The 4 ns doesn't show up next to
the hop.

### Search

| Workload | N | uniffi median / p95 | manual median / p95 | Ratio |
|---|---|---|---|---|
| `hit_count` | 10,000 | 8.21 µs / 22.33 µs | 9.75 µs / 26.71 µs | 0.84 |
| `hits`, lazy | 200 | 33.62 ms / 35.70 ms | 194.56 µs / 239.91 µs | 173 |
| `hits`, eager | 200 | 33.62 ms / 35.70 ms | 1.24 ms / 1.36 ms | 27.1 |
| `search("the")` | 50 | 151.92 ms / 160.79 ms | 151.94 ms / 159.35 ms | 1.00 |
| `findAll("e")` | 20 | 971.94 ms / 1.008 s | 936.52 ms / 965.33 ms | 1.04 |

`hits` is the session after `search("e")` on `report.pdf`: 8,610 hits, 8,610 segments, a snippet on
every hit. "Lazy" is `ManualHits`, a `RandomAccessCollection` over the C arrays with nothing decoded.
"Eager" is `Array(hits.map(SearchHit.init))`, the same `[SearchHit]` uniffi returns.

Verdicts. `hit_count`: equal (run 1 had it 1.07 the other way). `hits`: manual wins by 27x even
after it builds the same Swift values, which is about 3.8 µs a hit that uniffi spends and the manual
path doesn't. `search`: equal, engine-bound. `findAll`: manual wins by 35 ms on a 950 ms call, and
35 ms is what the `hits` row predicts for 8,610 hits, so it's the same cost riding on a promise.

### Progress events

| Workload | Scans | uniffi | manual |
|---|---|---|---|
| first progress event, median / p95 | 20 | 49.63 ms / 50.72 ms | 49.78 ms / 52.51 ms |
| scan, median | 20 | 1.087 s | 1.088 s |
| events per scan | 20 | 14 | 14 |
| progress events a second | 20 | 12.9 | 12.9 |

Each scan is a session `search("e")` with a fresh subscription, read through an `AsyncStream` on
both bridges. The brief asked for events during `findAll("e")`, but `find_all` touches no session
state and emits no events, so this uses `search`.

Verdict: equal. Fourteen events a scan is one per page; the engine paces them, and either bridge
delivers each in microseconds.

### Rasters

| Workload | Pixels | uniffi median / p95 | manual median / p95 | Ratio | uniffi footprint growth | manual footprint growth |
|---|---|---|---|---|---|---|
| `renderPage` @1x | 612 x 792 | 969.17 µs / 1.08 ms | 10.12 µs / 12.69 µs | 96 | 1,949,696 B | 0 B |
| `renderPage` @2x | 1224 x 1584 | 3.66 ms / 4.23 ms | 8.54 µs / 12.54 µs | 429 | 7,782,400 B | 0 B |
| `renderPage` @3x | 1836 x 2376 | 8.19 ms / 9.50 ms | 10.42 µs / 13.31 µs | 786 | 17,465,344 B | 0 B |

N is 50. The page is `report.pdf`'s first, and the call ends with a `CGImage` on both bridges.
Footprint growth is the highest `phys_footprint` seen after a call, minus the footprint before the
workload.

Read this table with one caveat. The render plugin's raster store caches by page and size, so every
timed call after the first is a cache hit, and neither column includes PDFium rendering. That makes
it a clean measure of the transfer, but it's not what a cold render costs: add the render time to
both columns.

Verdict: manual wins, and the gap grows with the raster. The manual path costs a hop and nothing
else at every size, because it hands Swift a pointer into the `Arc<Vec<u8>>` the raster store already
holds. uniffi's cost grows at about 2.1 GB/s of pixels. `colors.pdf` drawn through Core Graphics gave
the same dimensions and the same pixels at (5, 5) and (35, 5) on both bridges, so the byte order
(RGBA, straight alpha, `CGImageAlphaInfo.last`) agrees.

## Code size

Non-blank, non-comment lines. The uniffi column counts only the members both bridges have (`hit_count`,
`hits`, `search`, `find_all`, `subscribe`, `render_page`, the records they return, and their shared
helpers), not uniffi's generated Swift.

| | uniffi | manual |
|---|---|---|
| Rust | 421 (`search/src/ffi.rs` 225, `render/src/ffi.rs` 196) | 792 (`epdf-manual-bridge/src` 764, `epdf-viewer-host/src/experimental.rs` 28) |
| C | 0 | 204 (`epdf_manual.h`) |
| Swift | 113 (`Search.swift` 54, `Render.swift` 59) | 336 (`EmbedPDFManualBridge` 322, `EmbedPDF/Experimental.swift` 14) |
| Total | 534 | 1,332 |
| ABI tests | 0 | 566 (`tests/abi.rs` 220, `tests/support/mod.rs` 244, `tests/layout.rs` 102) |

Archive size (`libEmbedPDFFFI.a`, simulator slice): 112,839,896 bytes shipping, 113,733,664 bytes
with the manual bridge, so 893,768 bytes (0.79%) for both bridges' exports side by side. That's an
archive, not a linked app; the linked cost is smaller and unmeasured.

## Why the gap is where it is

I went in expecting the per-call overhead to be the story. uniffi does a lot on every call: a
`RustBuffer` for arguments, a future the foreign side polls through a callback, a `catch_unwind`. A
hand-written call does none of that.

And it doesn't matter. Not here.

Every capability call in this design hops to the kernel thread, because the kernel is `Rc` all the
way down and lives on one thread. That hop is a channel send, a thread wake, a job, and a reply back
across threads into a Swift continuation. It costs 6 µs to 12 µs on this machine. uniffi adds about
4 ns to a call that already costs 10,000 ns, and both `noop_hop` and `hit_count` say exactly that:
the two bridges trade places between runs.

So the hop sets the floor, and the floor is the same for both. Which means the only place a
hand-written bridge can win is in what crosses after the hop. Two things cross badly.

The first is a big array of nested records. uniffi converts each `SearchHit` into its mirror record
(cloning the snippet, collecting the segments), writes all 8,610 into one buffer, and Swift reads them
back field by field into structs, strings and arrays. That's about 3.8 µs a hit. The manual path
writes flat C structs in one pass, and Swift decodes them in 1.24 ms, or reads them in place for
nothing at all. I haven't split uniffi's 33.6 ms between the Rust half and the Swift half. My guess is
most of it is the Swift lift, but it is a guess.

The second is bytes. A raster crosses uniffi as a `Vec<u8>` copied out of the host table, then
written into a `RustBuffer`, then lifted into `Data`. Three passes over 17 MB at 3x is 8 ms, which is
half a frame budget gone on a cache hit. The manual path doesn't copy at all: `CGDataProvider` reads
the raster store's own buffer and frees its `Arc` in the release callback.

(That zero-copy has a side effect worth knowing about. A `CGImage` from the manual path keeps the
raster store's buffer alive after the store evicts it. The memory doesn't grow, it just stops being
the store's to give back. For a viewer that's probably what you want. For the footprint accounting,
it means "zero growth" is shared memory, not free memory.)

Pointer samples, the thing B1 worried about, are the `noop_hop` case: small, frequent, and nobody
waits on them. A 120 Hz pointer stream is one call every 8.3 ms, and the hop costs a thousandth of
that. A hand-written path would save 4 ns of it.

## Recommendation

For B5, hand-write the raster path: `render_page`, `render_thumbnail`, `render_pages`, and the tile
renders that land with the stage plugin. That's where the difference is a frame, not a rounding error,
and it grows with every scale and every tile.

For the hit list, and any later member that returns thousands of records (annotation lists in B2 look
like the next one), hand-write it or change its shape. A paged or page-scoped `hits` on uniffi might
recover most of the 27x without any C. I haven't measured that, and it's worth an afternoon before
anyone writes a second hand-kept header.

For B1, keep pointer samples on uniffi. There's nothing to win.

The cost is real, and it's mostly not the lines. The manual search and render paths are 2.5x the
lines of their uniffi twins (1,332 against 534), and that's before counting 566 lines of ABI tests, 102 of them there only
because a person keeps the header's struct layouts in sync by hand. Every hand-written
member is `unsafe` code with an ownership contract (`epdf_hits_free`, `epdf_pixels_free`, a context
released exactly once) that the compiler doesn't check on either side. A field added to `SearchHit`
lands in uniffi for free and in the manual bridge as a header change, a Rust change, a Swift change
and a layout test. So the hand-written paths have to stay few, and each has to keep earning its
place against a uniffi version that keeps getting cheaper to maintain.

## Running it again

Build the experimental XCFramework, then run only the benchmarks, in Release:

```bash
bash platforms/apple/build-xcframework.sh --experimental-manual-bridge
cd platforms/apple/EmbedPDF
EMBEDPDF_EXPERIMENTAL_BRIDGE=1 xcodebuild test -scheme EmbedPDF-Package -configuration Release \
  -destination "platform=iOS Simulator,name=iPhone 17" -only-testing:EmbedPDFBridgeBenchmarks \
  2>&1 | grep "^BENCH "
```

With `EMBEDPDF_EXPERIMENTAL_BRIDGE=1`, `Package.swift` adds the `EmbedPDFManualBridge` target and the
benchmark test target, and leaves out the other test targets (`EmbedPDFBindingsSmokeTests` uses
`@testable import`, which can't build in Release). Run the script again without the flag to get the
shipping XCFramework back.

For the perf Mac, build locally and ship the built package, rather than fetching the branch there:

```bash
bash platforms/apple/build-xcframework.sh --experimental-manual-bridge
rsync -a --delete platforms/apple/EmbedPDF/ \
  andrew@2023mbp16grey.local:~/bench/EmbedPDF/
ssh andrew@2023mbp16grey.local 'cd ~/bench/EmbedPDF \
  && EMBEDPDF_EXPERIMENTAL_BRIDGE=1 xcodebuild test -scheme EmbedPDF-Package \
       -configuration Release -destination "platform=iOS Simulator,name=iPhone 17" \
       -only-testing:EmbedPDFBridgeBenchmarks 2>&1 | grep "^BENCH "' > bench-simulator.jsonl
```

The rsync carries the XCFramework, the generated `EmbedPDFBindings` sources and the fixtures, all of
which are ignored by git. Off the home network, the same machine is
`andrew@2023mbp16grey.squeaker-algol.ts.net` over Tailscale.
