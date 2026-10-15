# Swift bridge benchmarks: uniffi against a hand-written C bridge

We keep uniffi as the bridge for every plugin, and we hand-write exactly two kinds of path: page
rasters (B5) and large record arrays (the search hit list, and whatever B1 and B2 add of the same
shape). Pointer samples in B1 stay on uniffi. On a call that moves a few bytes, the two bridges
are equal within noise: both pay the same hop to the kernel thread (medians of 11.96 µs and
10.75 µs in run 2, lower in run 1), and the `noop_hop` ratio between them swung from 0.98x to 1.11x
across runs (`hit_count`'s from 0.84x to 1.07x). On a call that moves a lot, the
difference is transfer cost alone. Handing over 8,610 search hits costs 33.6 ms through uniffi and
1.24 ms by hand. A page raster costs about 1 ms (1x) to 8 ms (3x) through uniffi and about 10 µs by
hand. Those rasters were raster-store cache hits, so neither number includes PDFium rendering; a
cold render adds the same render time to both.

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
| Code | `7e9491152` (on `459bbe779`), the commit that adds the bridge, the benchmarks and this document |

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
6.25 ns against 1.25 ns). The hop to the kernel thread had medians of 11.96 µs (uniffi) and
10.75 µs (manual) in run 2. Run 1 was lower, and had the ratio the other way round (6.42 µs against
6.58 µs). The 4 ns doesn't show up next to the hop.

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
| ABI tests | 0 | 566 (`epdf-viewer-host/tests/manual_bridge_abi.rs` 220, `manual_bridge_support/mod.rs` 244, `epdf-manual-bridge/tests/layout.rs` 102) |

Archive size (`libEmbedPDFFFI.a`, simulator slice): 112,839,896 bytes shipping, 113,733,664 bytes
with the experimental features, so 893,768 bytes (0.79%) for the manual bridge plus the two uniffi
benchmark exports. That's an archive, not a linked app; the linked cost is smaller and unmeasured.

## Why the gap is where it is

I went in expecting the per-call overhead to be the story. uniffi does a lot on every call: a
`RustBuffer` for arguments, a future the foreign side polls through a callback, a `catch_unwind`. A
hand-written call does none of that.

And it doesn't matter. Not here.

Every capability call in this design hops to the kernel thread, because the kernel is `Rc` all the
way down and lives on one thread. That hop is a channel send, a thread wake, a job, and a reply back
across threads into a Swift continuation. Its median on this machine was 11.96 µs (uniffi) and
10.75 µs (manual) in run 2, and about 6.5 µs on both in run 1. Within run 2 alone, the five
per-run means of `noop_hop` span about 4 µs on each bridge (15.53 µs to 19.75 µs for uniffi,
10.92 µs to 14.91 µs by hand). uniffi adds about 4 ns at the entry, far below that noise, and
both `noop_hop` and `hit_count` say so: the two bridges trade places between runs.

So the hop sets the floor, and the floor is the same for both. Which means the only place a
hand-written bridge can win is in what crosses after the hop. Two things cross badly.

The first is a big array of nested records. uniffi converts each `SearchHit` into its mirror record
(cloning the snippet, collecting the segments), writes all 8,610 into one buffer, and Swift reads them
back field by field into structs, strings and arrays. That's about 3.8 µs a hit. The manual path
writes flat C structs in one pass, and Swift decodes them in 1.24 ms, or reads them in place for
nothing at all. I haven't split uniffi's 33.6 ms between the Rust half and the Swift half. My guess is
most of it is the Swift lift, but it is a guess.

The second is bytes. Reading the code, a raster crosses uniffi as a `Vec<u8>` copied out of the host
table, then written into a `RustBuffer`, then lifted into `Data`, and bridging that `Data` to
`CFData` for `CGDataProvider` may copy it once more. I counted those passes from the source, not
with a profiler. What I did measure is the total: 8.19 ms for 17 MB at 3x, which is half a frame
budget gone on a cache hit. The manual path doesn't copy at all: `CGDataProvider` reads
the raster store's own buffer and frees its `Arc` in the release callback.

(That zero-copy has a side effect worth knowing about. A `CGImage` from the manual path keeps the
raster store's buffer alive after the store evicts it. The memory doesn't grow, it just stops being
the store's to give back. For a viewer that's probably what you want. For the footprint accounting,
it means "zero growth" is shared memory, not free memory.)

Pointer samples, the thing B1 worried about, are `fire` calls: an entry plus a post to the kernel
thread, with no reply for anyone to wait on. So they cost less than `noop_hop`, and the only part a
hand-written path could shave is the entry, where uniffi's extra is about 4 ns. A 120 Hz pointer
stream is one call every 8.3 ms.

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
lines of their uniffi twins (1,332 against 534), and that's before counting 566 lines of ABI tests,
102 of them there only because a person keeps the header's struct layouts in sync by hand.
Every hand-written member is `unsafe` code with an ownership contract (`epdf_hits_free`, `epdf_pixels_free`, a context
released exactly once) that the compiler doesn't check on either side. A field added to `SearchHit`
lands in uniffi for free and in the manual bridge as a header change, a Rust change, a Swift change
and a layout test. So the hand-written paths have to stay few, and each has to keep earning its
place against a uniffi version that keeps getting cheaper to maintain.

One thing has to change before any hand-written path ships. The bridge parks each pending promise's
abort in a kernel-thread-local table (`ABORTS` in `crates/epdf-manual-bridge/src/lib.rs`), and
nothing drains it when the kernel thread exits. A call still in flight at shutdown would drop its
promise during thread-local teardown, after the runtime it reaches is gone: the same abort hazard
`bound::drop_all` fixed for the `KernelBound` slab in `f4a9a0844`. The table needs the same drain,
in the same place, before the thread exits.

Update (2026-10-15): `9b1a52698` added that drain. The first parked abort registers an
`epdf_ffi::at_teardown` hook that empties `ABORTS` while the kernel thread's locals are alive
(`crates/epdf-manual-bridge/src/lib.rs:190-201`), so this no longer blocks a hand-written path.

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
  user@remote-host:~/bench/EmbedPDF/
ssh user@remote-host 'cd ~/bench/EmbedPDF \
  && EMBEDPDF_EXPERIMENTAL_BRIDGE=1 xcodebuild test -scheme EmbedPDF-Package \
       -configuration Release -destination "platform=iOS Simulator,name=iPhone 17" \
       -only-testing:EmbedPDFBridgeBenchmarks 2>&1 | grep "^BENCH "' > bench-simulator.jsonl
```

The rsync carries the XCFramework, the generated `EmbedPDFBindings` sources and the fixtures, all of
which are ignored by git. Off the home network, reach the same machine by its Tailscale name.

## Appendix: run 2's raw output

Every table above comes from these lines, as `BridgeBenchmarks.swift` printed them (times in
nanoseconds). `progress_events`' `median_ns` and `p95_ns` are the first-event latency.

```
BENCH {"bridge":"uniffi","extra":{},"median_ns":6.25,"n":100000,"p95_ns":7.09,"run_means_ns":[7.32125,6.8975,6.70125,7.47,6.68916],"runs":5,"workload":"noop"}
BENCH {"bridge":"manual","extra":{},"median_ns":2.5,"n":100000,"p95_ns":2.92,"run_means_ns":[1.55916,3.66542,3.13125,3.10167,2.54917],"runs":5,"workload":"noop"}
BENCH {"bridge":"uniffi","extra":{},"median_ns":11958,"n":10000,"p95_ns":38917,"run_means_ns":[18336.325,17202.2833,15528.5667,19754.5833,16252.4459],"runs":5,"workload":"noop_hop"}
BENCH {"bridge":"manual","extra":{},"median_ns":10750,"n":10000,"p95_ns":25250,"run_means_ns":[13447.8833,12537.5625,10921.7708,13058.9375,14906.4416],"runs":5,"workload":"noop_hop"}
BENCH {"bridge":"uniffi","extra":{},"median_ns":8209,"n":10000,"p95_ns":22333.049999999996,"run_means_ns":[10688.1958,11745.1375,11058.9,11120.6166,10656.2125],"runs":5,"workload":"hit_count"}
BENCH {"bridge":"manual","extra":{},"median_ns":9750,"n":10000,"p95_ns":26708,"run_means_ns":[11098.2334,13817.0917,15730.9583,12137.9875,11487.075],"runs":5,"workload":"hit_count"}
BENCH {"bridge":"uniffi","extra":{"hits":8610,"segments":8610},"median_ns":33622916.5,"n":200,"p95_ns":35702497.3,"run_means_ns":[34571620.625,33755313.125,33524538.125,33944122.5,33581212.5],"runs":5,"workload":"hits_lazy"}
BENCH {"bridge":"manual","extra":{"hits":8610,"segments":8610},"median_ns":194563,"n":200,"p95_ns":239908.34999999998,"run_means_ns":[194978.54,205819.375,196413.125,196185.42,202711.045],"runs":5,"workload":"hits_lazy"}
BENCH {"bridge":"manual","extra":{"hits":8610,"segments":8610},"median_ns":1241104,"n":200,"p95_ns":1356776.45,"run_means_ns":[1244721.46,1290936.665,1268102.915,1254355.205,1245313.75],"runs":5,"workload":"hits_eager"}
BENCH {"bridge":"uniffi","extra":{},"median_ns":151922437.5,"n":50,"p95_ns":160792795.79999998,"run_means_ns":[152451651.66,154052203.34,152109902.5,155119269.16,152170680.82],"runs":5,"workload":"search"}
BENCH {"bridge":"manual","extra":{},"median_ns":151939187.5,"n":50,"p95_ns":159354243.9,"run_means_ns":[152147064.16,152153400.84,152278564.16,152080085,155257201.66],"runs":5,"workload":"search"}
BENCH {"bridge":"uniffi","extra":{},"median_ns":971936270.5,"n":20,"p95_ns":1008239776.8,"run_means_ns":[962254297.95,964931950,984555541.65,980382531.25,985806350],"runs":5,"workload":"find_all"}
BENCH {"bridge":"manual","extra":{},"median_ns":936518312.5,"n":20,"p95_ns":965331809.75,"run_means_ns":[944843350,952321014.55,939723350,938491991.65,930808566.7],"runs":5,"workload":"find_all"}
BENCH {"bridge":"uniffi","extra":{"events_per_s":12.88655345557427,"events_per_scan":14,"scan_median_ns":1086566042},"median_ns":49625583.5,"n":20,"p95_ns":50724050,"run_means_ns":[1086403750.1],"runs":1,"workload":"progress_events"}
BENCH {"bridge":"manual","extra":{"events_per_s":12.850069180497526,"events_per_scan":14,"scan_median_ns":1087573146},"median_ns":49778354,"n":20,"p95_ns":52511820.85,"run_means_ns":[1089488297.95],"runs":1,"workload":"progress_events"}
BENCH {"bridge":"manual","extra":{"footprint_growth_bytes":0,"height":792,"peak_growth_bytes":0,"width":612},"median_ns":10125,"n":50,"p95_ns":12689.55,"run_means_ns":[10084.18,10395.84,9950.82,10424.16,10029.16],"runs":5,"workload":"raster_1x"}
BENCH {"bridge":"uniffi","extra":{"footprint_growth_bytes":1949696,"height":792,"peak_growth_bytes":1949696,"width":612},"median_ns":969167,"n":50,"p95_ns":1079904.65,"run_means_ns":[975375.82,980464.18,975068.34,975625,998130],"runs":5,"workload":"raster_1x"}
BENCH {"bridge":"manual","extra":{"footprint_growth_bytes":0,"height":1584,"peak_growth_bytes":0,"width":1224},"median_ns":8541.5,"n":50,"p95_ns":12542,"run_means_ns":[9765,9632.5,9054.16,9309.18,8873.34],"runs":5,"workload":"raster_2x"}
BENCH {"bridge":"uniffi","extra":{"footprint_growth_bytes":7782400,"height":1584,"peak_growth_bytes":7798784,"width":1224},"median_ns":3661562.5,"n":50,"p95_ns":4225377.05,"run_means_ns":[3792615,3695825,3703570,3825911.66,3804017.5],"runs":5,"workload":"raster_2x"}
BENCH {"bridge":"manual","extra":{"footprint_growth_bytes":0,"height":2376,"peak_growth_bytes":0,"width":1836},"median_ns":10416.5,"n":50,"p95_ns":13314.55,"run_means_ns":[10499.16,10166.66,10110,10848.34,10533.34],"runs":5,"workload":"raster_3x"}
BENCH {"bridge":"uniffi","extra":{"footprint_growth_bytes":17465344,"height":2376,"peak_growth_bytes":17465344,"width":1836},"median_ns":8189396,"n":50,"p95_ns":9497444.05,"run_means_ns":[8496027.5,8576345.82,8415294.16,8449338.34,8336925.84],"runs":5,"workload":"raster_3x"}
```
