# Results: four PDFium wrapper arms, wasm, seven rounds

The fairness gate ran before any timing, over both gate fixtures (`report-p4` and `rotated-p0`),
and passed:

```
fairness gate: a, b, c, d agree on 8106 glyphs, page 4
```

All four arms agree on every canonical answer they are declared to produce, on both fixtures. The
numbers below are real per-operation cost, not an artifact of one arm quietly computing the wrong
thing.

## This run corrects a bug in the previous one

The first version of this file reported `geometry` at 1.57x (arm C) and 1.64x (arm D) slower than
arm A, and concluded coarsening lost on this workload. That conclusion was wrong. `geometryCoarse`
in `bench/workloads.mjs`, the decode arms C and D use to turn the packed record buffer back into JS
glyph objects, allocated four throwaway arrays and four throwaway closures per non-empty glyph:
`[16, 20, 24, 28].map(...)` and `[32, 36, 40, 44].map(...)` for the two rects, and
`Array.from({ length: 8 }, ...)` for each of the two quads. `geometryThin`, the arm A/B path,
builds the same shapes through `rectAt` and `quadAt`, both closure-free: a direct 4-element array
literal for a rect, a preallocated `Array(8)` filled by a plain indexed loop for a quad. Across the
fixture's 6,648 non-empty glyphs that is roughly 26,000 extra allocations paid only by arms C and
D, on a workload where JS-side object materialization is the dominant cost for every arm. The
`geometry` numbers in the first run measured that allocation bug, not the cost of coarsening.

The fix rewrites `geometryCoarse` to build rects and quads the same way `geometryThin` does:
`dvRectAt` and `dvQuadAt`, added next to `rectAt`/`quadAt` in `bench/workloads.mjs`, read through
the same `DataView` the coarse path already used, but with no `.map`, no `Array.from`, and no
closure per glyph. Nothing else about the decode changed: same fields, same order, same branch
conditions, same values, confirmed by the fairness gate still passing with all four arms agreeing.
The other three coarse decode paths in the file (`searchCoarse`, `textCoarse`, `renderCoarse`) were
checked against the same class of asymmetry and found already symmetric with their thin
counterparts — `textCoarse` and `renderCoarse` make one call and no per-item JS loop at all, and
`searchCoarse`'s per-hit `[0, 4, 8, 12].map(...)` allocates exactly the same shape per hit that
`searchThin`'s per-rect `rectPtrs.map(...)` allocates per rect. Those three were left unchanged.

Every number in this file after this section is from the run made with the fix in place. Only the
`geometry` column moved; everything else is unchanged from the first run within round-to-round
noise, which is what you would expect from a bug confined to one decode path.

## Environment

```
Node v24.20.0 · Apple M4 Max · darwin 25.6.0
arch arm64 · 16 cores · loadavg 3.08 at the start of the run
emsdk unknown · rustc 1.98.0 (88d9e12ae, 2026-08-18)
fixture: examples/snippet-react/public/report.pdf, page 4, 8106 glyphs
rounds: 7, interleaved a/b/c/d, budget 500ms per cell
```

`emsdk unknown` is a shell artifact of this measurement session, not a change to the wasm module:
the session's PATH did not have `em++` on it when `bench/run.mjs` ran, so `bench/node.mjs` fell
back to its `'unknown'` default for that one metadata field. The wasm modules were not rebuilt for
this run; module sizes below are byte-identical to the first run, which is what you would expect
from a JS-only fix. `bench/run.test.mjs` independently fails on this same detection gap on a clean
checkout of this branch, before any change in this task, so it is a pre-existing environment
quirk, not something this task introduced or needs to fix.

macOS thermal throttling is not controllable from here. Rounds are interleaved for exactly that
reason, so drift lands on all four arms rather than on whoever runs last. Worst observed
round-to-round spread (max/min across the 7 rounds, same arm and workload):

```
b/null-peek 1.23x, a/null-peek 1.21x, a/geometry 1.16x
```

`null-peek` measures a single `getValue` call, around 4 nanoseconds; a 1.2x spread there is a
sub-nanosecond absolute wobble that a 500ms sampling budget cannot fully average out at that scale.
`bench/compare.mjs` treats spread past about 1.3x as a thermal-drift signal; nothing here reaches
that line, so nothing suggests throttling distorted the comparisons that matter.

A note on what the numbers mean: `harness.mjs` measures in batches (auto-tuned so one batch takes at
least 2ms), and reports the median and p95 of those batch means, not of individual calls. The p95
column below is the p95 of batch means: it shows run-to-run variance within a workload, not
per-call tail latency, and should not be read as a worst-case frame cost.

## Wasm

### Median nanoseconds per call, by workload and arm

| workload | a direct | b rust thin | c rust coarse | d c++ coarse | b/a | c/a | d/a |
|---|---|---|---|---|---|---|---|
| `null-call` | 322.5 ns | 335.3 ns | — | — | 1.04x | — | — |
| `null-peek` | 4.0 ns | 3.3 ns | — | — | 0.83x | — | — |
| `null-readbytes` | 30.3 ns | 30.2 ns | — | — | 1.00x | — | — |
| `geometry` | 3.569 ms | 3.576 ms | 976.83 µs | 1.294 ms | 1.00x | 0.27x | 0.36x |
| `geometry-copyout` | — | — | 20.52 µs | 20.38 µs | — | — | — |
| `text` | 120.43 µs | 122.55 µs | 122.33 µs | 121.55 µs | 1.02x | 1.02x | 1.01x |
| `render-1x` | 2.804 ms | 2.945 ms | 2.932 ms | 2.792 ms | 1.05x | 1.05x | 1.00x |
| `render-4x` | 15.715 ms | 16.439 ms | 16.324 ms | 15.602 ms | 1.05x | 1.04x | 0.99x |
| `render-copyout-4x` | 576.51 µs | 578.45 µs | 573.42 µs | 573.50 µs | 1.00x | 0.99x | 0.99x |
| `search` | 110.50 µs | 111.27 µs | 30.46 µs | 31.32 µs | 1.01x | 0.28x | 0.28x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 329.8 ns | 344.6 ns | — | — |
| `null-peek` | 4.0 ns | 4.0 ns | — | — |
| `null-readbytes` | 30.9 ns | 30.7 ns | — | — |
| `geometry` | 4.473 ms | 3.955 ms | 1.025 ms | 1.328 ms |
| `geometry-copyout` | — | — | 22.47 µs | 22.40 µs |
| `text` | 124.81 µs | 126.87 µs | 126.91 µs | 126.11 µs |
| `render-1x` | 2.848 ms | 3.010 ms | 2.992 ms | 2.847 ms |
| `render-4x` | 15.915 ms | 16.824 ms | 16.519 ms | 15.778 ms |
| `render-copyout-4x` | 592.11 µs | 592.99 µs | 594.91 µs | 592.41 µs |
| `search` | 112.55 µs | 115.92 µs | 35.12 µs | 37.12 µs |

### Module size

| arm | wasm bytes | delta vs a |
|---|---|---|
| a direct | 5,180,492 | — |
| b rust thin | 5,183,471 | +2.9 kB |
| c rust coarse | 5,184,644 | +4.1 kB |
| d c++ coarse | 5,188,608 | +7.9 kB |

Byte-identical to the first run, as expected: this task changed only `bench/workloads.mjs`, a
harness file, not any producer crate or the C++ side.

## What the numbers say

**The wrapper tax is real and small.** Arm B costs 0 to 5 percent more than arm A on every
workload that runs PDFium work (`geometry` 1.00x, `text` 1.02x, `render-1x` 1.05x, `render-4x`
1.05x, `search` 1.01x), and 4 percent on the pure `null-call` crossing with nothing behind it. That
is inside the pre-registered 15 percent band in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md),
and well inside the 25 percent line past which the prediction said to suspect the harness rather
than the result. `null-peek` at 0.83x and `null-readbytes` at 1.00x are noise: both are
single-digit-nanosecond operations where a 500ms sampling budget cannot resolve better than about a
nanosecond, and the round-to-round spread above confirms `null-peek` is the noisiest cell in the
whole table.

**Coarsening the crossing wins on `geometry`, once the decode is fairness-matched.** The prediction
was 3x to 10x faster for arm C against arm A. With the allocation bug fixed, arm C lands at 0.27x
(about 3.7x faster) and arm D at 0.36x (about 2.8x faster), both inside or just under that band.
The mechanism is what the prediction expected: `geometryThin` makes two wasm calls per glyph
(`FPDFText_GetTextObject` and `EPDFText_GetCharGeometry`) plus a third on every object-key change,
so 8,106 glyphs cost on the order of sixteen thousand crossings. `geometryCoarse` makes exactly one
crossing, decodes PDFium's per-glyph struct into a packed 112-byte record on the native side, and
then walks that one contiguous buffer from JS. What the first run's bug did was add roughly 26,000
extra JS-side allocations on top of that one-crossing win, allocations big enough to erase it and
invert the result. `search` shows the same mechanism at a different ratio (0.28x, about 3.5x
faster) because it was never near the bug: `searchCoarse`'s per-hit decode already matched
`searchThin`'s allocation shape, so its number did not move between runs. `render` and `text` land
within 1 to 5 percent everywhere in both runs, matching the prediction that render is dominated by
PDFium's own bitmap work and unaffected by any wrapper.

**Arm C and arm D are no longer indistinguishable on `geometry`.** On every other shared workload
they still are: `text` 122.33µs vs 121.55µs (d 1 percent faster), `render-1x` 2.932ms vs 2.792ms
(d 5 percent faster), `render-4x` 16.324ms vs 15.602ms (d 5 percent faster), `search` 30.46µs vs
31.32µs (c 3 percent faster) — all inside or close to the round-to-round spread measured above, so
none of them is a result. `geometry` is different: arm C at 976.83µs against arm D at 1.294ms is
arm C running about 25 percent faster, and that gap holds in every one of the 7 rounds individually
(ratio of d over c from 1.27x to 1.37x, no overlap with the 1.16x worst same-arm spread measured
elsewhere in this run). The JS decode is identical for both arms after the fix, and both make one
crossing into an identically-shaped native record buffer, so the gap is in the native geometry
producers themselves: `rs_read_page_geometry` in Rust is doing the per-glyph PDFium-to-record work
measurably faster than `cc_read_page_geometry` in C++ on this fixture. The first run's allocation
bug added roughly the same amount of JS garbage to both arms, which was large enough relative to
this ~300-microsecond native gap to bury it entirely; correcting the bug is what makes it visible.
This is the one place in the whole table where Rust buys something C++ does not, and it is worth
treating cautiously rather than as a general verdict: it is one native routine on one fixture, and
the reason for the gap (register allocation, bounds-check elision, something about how emscripten
compiled the two implementations) has not been investigated here.

**These are lower bounds on real per-call cost.** `bench/arm.mjs` cwraps every arm by hand, calling
each exported PDFium function directly. It does not go through the engine's per-call argument
bridge in `packages/engine/runtime/src/wasm/wasm-runtime.ts`, which does type dispatch and argument
marshalling on every call in the shipping runtime. That bridge runs identically regardless of which
arm sits behind it, so it would add a constant amount of overhead to every column here without
separating any of them. Skipping it keeps the comparison clean, but it also means every absolute
number in this file understates what a real call costs today.

**`geometry` is a transcription, not the reader.** It reproduces the loop at
[`PageGeometryReader.ts:109`](../packages/engine/services/src/features/geometry/PageGeometryReader.ts)
and its `readGlyphRaw` helper at `:135`, call for call, but it is a transcription because the real
reader needs `DocumentSession` and a built `engine-runtime` dist, which would have pulled the whole
workspace into this spike. It omits three things the real reader does per glyph:
`throwIfAborted(signal)` at `:110`, `normalizePdfRect` on the loose and tight boxes at `:156` and
`:162`, and the `{objectKey, ...raw}` object spread that builds each record at `:113`. All three cost
real time only in the thin arms (a and b), which build each glyph record on the JS side the way the
real reader does; the coarse arms never touch that per-glyph JS path in the first place. So all
three omissions make the thin arms look cheaper than `PageGeometryReader` actually is, which
understates the thin side of the comparison and can only shrink the measured coarsening win, never
inflate it. `geometry` came out as a win for coarsening on this fixture even before correcting for
this bias; correcting for it would widen that win, not close it.

**The timing fixture never exercises the rotation branch.** `report.pdf` page 4 has 0 non-upright
glyphs and 0 SPACE-flagged glyphs (see `fixture.mjs`'s header comment). The fairness gate's second
fixture, `rotated_text.pdf` page 0, is entirely non-upright and does cover that branch and the
ascent-flip and SPACE-flag paths, and all four arms agree on it. But no number in the tables above
was produced by a call that took that branch. If the rotation math has a performance cost distinct
from the upright path, it is not measured here.

**What this does not say.** It says nothing about `engine-services`, its 16,000-odd lines, or how
well they would port. It says nothing about iOS or Android, which is the actual argument for the
port in [`docs/research/rust-core-port.md`](../docs/research/rust-core-port.md); if every number
above argued against Rust, the case for the port would be unchanged, because the case was never
about web performance. It is Node only, wasm only. Native, where `mem.peek` is an N-API round trip
rather than a typed-array read, is expected to disagree with this, for the reasons in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md#native-after),
and is not measured here.
