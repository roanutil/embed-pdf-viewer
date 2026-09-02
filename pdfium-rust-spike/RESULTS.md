# Results: four PDFium wrapper arms, wasm, seven rounds

The fairness gate ran before any timing, over both gate fixtures (`report-p4` and `rotated-p0`),
and passed:

```
fairness gate: a, b, c, d agree on 8106 glyphs, page 4
```

All four arms agree on every canonical answer they are declared to produce, on both fixtures. The
numbers below are real per-operation cost, not an artifact of one arm quietly computing the wrong
thing.

## Environment

```
Node v24.20.0 · Apple M4 Max · darwin 25.6.0
arch arm64 · 16 cores · loadavg 4.27 at the start of the run
emsdk 3.1.72 (emcc 437140d) · rustc 1.98.0 (88d9e12a, 2026-08-18)
fixture: examples/snippet-react/public/report.pdf, page 4, 8106 glyphs
rounds: 7, interleaved a/b/c/d, budget 500ms per cell
```

macOS thermal throttling is not controllable from here. Rounds are interleaved for exactly that
reason, so drift lands on all four arms rather than on whoever runs last. Worst observed
round-to-round spread (max/min across the 7 rounds, same arm and workload):

```
a/null-peek 1.23x, b/null-peek 1.22x, a/geometry 1.07x
```

`null-peek` measures a single `getValue` call, around 4 nanoseconds; a 1.2x spread there is a
sub-nanosecond absolute wobble, the kind of thing a 500ms sampling budget cannot fully average out
at that scale. Every other workload's spread is under 1.1x. Nothing here suggests thermal throttling
distorted the comparisons that matter.

A note on what the numbers mean: `harness.mjs` measures in batches (auto-tuned so one batch takes at
least 2ms), and reports the median and p95 of those batch means, not of individual calls. The p95
column below is the p95 of batch means: it shows run-to-run variance within a workload, not
per-call tail latency, and should not be read as a worst-case frame cost.

## Wasm

### Median nanoseconds per call, by workload and arm

| workload | a direct | b rust thin | c rust coarse | d c++ coarse | b/a | c/a | d/a |
|---|---|---|---|---|---|---|---|
| `null-call` | 324.3 ns | 335.6 ns | — | — | 1.03x | — | — |
| `null-peek` | 4.0 ns | 3.4 ns | — | — | 0.84x | — | — |
| `null-readbytes` | 30.5 ns | 30.3 ns | — | — | 0.99x | — | — |
| `geometry` | 3.559 ms | 3.713 ms | 5.588 ms | 5.832 ms | 1.04x | 1.57x | 1.64x |
| `geometry-copyout` | — | — | 20.14 µs | 20.15 µs | — | — | — |
| `text` | 121.04 µs | 123.45 µs | 123.14 µs | 122.04 µs | 1.02x | 1.02x | 1.01x |
| `render-1x` | 2.811 ms | 2.947 ms | 2.945 ms | 2.817 ms | 1.05x | 1.05x | 1.00x |
| `render-4x` | 15.691 ms | 16.476 ms | 16.460 ms | 15.697 ms | 1.05x | 1.05x | 1.00x |
| `render-copyout-4x` | 580.60 µs | 580.41 µs | 571.95 µs | 574.31 µs | 1.00x | 0.99x | 0.99x |
| `search` | 110.59 µs | 114.38 µs | 30.45 µs | 31.73 µs | 1.03x | 0.28x | 0.29x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 333.0 ns | 346.9 ns | — | — |
| `null-peek` | 4.1 ns | 4.0 ns | — | — |
| `null-readbytes` | 31.1 ns | 31.0 ns | — | — |
| `geometry` | 3.914 ms | 4.122 ms | 8.290 ms | 6.329 ms |
| `geometry-copyout` | — | — | 22.32 µs | 21.71 µs |
| `text` | 126.00 µs | 126.74 µs | 126.55 µs | 125.36 µs |
| `render-1x` | 2.855 ms | 2.984 ms | 3.000 ms | 2.864 ms |
| `render-4x` | 15.777 ms | 16.707 ms | 16.707 ms | 15.845 ms |
| `render-copyout-4x` | 601.99 µs | 598.36 µs | 594.70 µs | 594.58 µs |
| `search` | 113.02 µs | 118.71 µs | 35.33 µs | 37.98 µs |

### Module size

| arm | wasm bytes | delta vs a |
|---|---|---|
| a direct | 5,180,492 | — |
| b rust thin | 5,183,471 | +2.9 kB |
| c rust coarse | 5,184,644 | +4.1 kB |
| d c++ coarse | 5,188,608 | +7.9 kB |

## What the numbers say

**The wrapper tax is real and small.** Arm B costs 3 to 5 percent more than arm A on every workload
that runs PDFium work (`geometry` 1.04x, `text` 1.02x, `render-1x` 1.05x, `render-4x` 1.05x,
`search` 1.03x), and 3 percent on the pure `null-call` crossing with nothing behind it. That is
inside the pre-registered 15 percent band in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md),
and well inside the 25 percent line past which the prediction said to suspect the harness rather
than the result. One extra Rust wasm frame per call costs a few nanoseconds. `null-peek` at 0.84x
and `null-readbytes` at 0.99x are noise: both are single-digit-nanosecond operations where a 500ms
sampling budget cannot resolve better than about a nanosecond, and the round-to-round spread above
confirms `null-peek` is the noisiest cell in the whole table.

**Coarsening the crossing did not win on `geometry`, and that contradicts the prediction.** The
prediction was 3x to 10x faster for arm C against arm A. What actually happened is arm C at 1.57x
slower and arm D at 1.64x slower. The likely reason is that `geometry`'s `run()` still has to decode
every one of the 8,106 glyphs into the same JS object shape the thin arms build, because that shape
is what the fairness gate compares against the canonical answer. Coarse arms pay for that JS decode
too, and on top of it they pay to pack PDFium's per-glyph struct into their own 112-byte record
buffer inside Rust or C++, a serialization step the thin arms never perform because they read
PDFium's native struct directly. `search` shows the opposite pattern and confirms the mechanism:
arm C runs at 0.28x (roughly 3.5x faster) because a search over this fixture returns far fewer hits
than there are glyphs, so the packed-buffer decode is cheap relative to the crossings it replaces.
Coarsening a loop only pays off in wasm when what comes back across the boundary is small relative
to what it replaces; a per-item result that must be decoded into JS one item at a time regardless of
how it arrived erases the saving. `render` and `text` land within 1 to 5 percent everywhere, matching
the prediction that render is dominated by PDFium's own bitmap work and unaffected by any wrapper.

**Arm C and arm D are indistinguishable everywhere they both run.** `geometry` 5.588ms vs 5.832ms
(c 4 percent faster), `text` 123.14µs vs 122.04µs (d 1 percent faster), `render-1x` 2.945ms vs
2.817ms (d 4 percent faster), `render-4x` 16.460ms vs 15.697ms (d 5 percent faster), `search`
30.45µs vs 31.73µs (c 4 percent faster). Every gap is inside the round-to-round spread measured
above (up to 1.07x on a real workload), so none of them is a result, only noise. This is the
single most decision-relevant number in this file: on this fixture, moving a loop across the
TypeScript/wasm boundary is what buys the win, and Rust buys nothing beyond what C++ already buys.
The honest reading is that the PDFium fork can keep collapsing hot loops into coarse `EPDF*`
functions in C++, the way `PageTextReader.ts:93` already does with a single `EPDFText_GetTextFull`
call for a whole page, without a Rust layer in between. Module size agrees: arm C adds 4.1 kB and
arm D adds 7.9 kB over arm A, both far under the 100 to 400 kB the prediction allowed for a Rust
staticlib, and small enough on their own that size is not the deciding factor either way.

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
real reader does; the coarse arms never touch that per-glyph JS path in the first place. So all three
omissions make the thin arms look cheaper than `PageGeometryReader` actually is, which understates
the thin side of the comparison and can only shrink the measured coarsening win, never inflate it.
Given that `geometry` came out as a loss for coarsening on this fixture, correcting for this bias
would make coarsening look worse, not better.

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
