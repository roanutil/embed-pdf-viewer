# Results: four PDFium wrapper arms, wasm, seven rounds

The fairness gate ran before any timing, over both gate fixtures (`report-p4` and `rotated-p0`),
and passed:

```
fairness gate: a, b, c, d agree on report-p4 (8106 glyphs), rotated-p0 (31 glyphs), 5 workloads compared
```

All four arms agree on every canonical answer they are declared to produce, on both fixtures. The
numbers below are real per-operation cost, not an artifact of one arm quietly computing the wrong
thing.

## This is a re-run; the previous wasm data is gone

`bench/run.mjs` used to clear its entire results directory at the start of every run, wasm or
native. The first `--native` run after a wasm run deleted that wasm run's raw JSON along with it,
so the wasm tables previously published in this file had no committed data behind them; only the
28 native JSONs from that sitting survived, because native ran last. The clearing bug is fixed:
`run.mjs` now clears only the files it is about to write (native runs clear `native-*.json`, wasm
runs clear the rest), so the two result sets can no longer take each other out. Rather than
regenerate wasm alone and end up quoting a wasm run and a native run from two
different sittings under one set of tables, both runs below were redone together, in the same
sitting, on the same fixed driver. `bench/results/` now holds all 56 result files this file's
numbers are drawn from, `<arm>-r<round>.json` and `native-<arm>-r<round>.json`, seven rounds each
for arms a, b, c, d, and they are committed alongside this file rather than gitignored.

## This run fixes six review findings

A review of the previous version of this file and its driver found six more ways an arm could be
doing different work for a reason unrelated to the wrapper under test. All six are fixed in the
numbers below.

**Arm order was never rotated.** `bench/run.mjs` ran every round in the same order, a, b, c, d.
Interleaving rounds corrects clock drift across a multi-minute run, but it does nothing about
position within a round, and the environment stamps showed background load decaying inside every
round: whichever arm ran first paid the highest load, whichever ran last paid the lowest. The
previous run's `null-call` cell showed arm b, a strict wrapper around the exact call arm a makes,
beating arm a in all seven rounds. A wrapper cannot be faster than the thing it wraps, so that was
the position effect showing up in the one cell built to price the wrapper. `bench/run.mjs` now
rotates the order by round index (round 1: a, b, c, d; round 2: b, c, d, a; round 3: c, d, a, b;
and so on), and each result file records the order that round actually used in an `order` field, so
the rotation is auditable after the fact. Rounds stay outer, arms stay inner, one process per arm.
The per-round orders used for this run were a,b,c,d / b,c,d,a / c,d,a,b / d,a,b,c / a,b,c,d /
b,c,d,a / c,d,a,b. What rotation revealed about `null-call` itself is in "What the numbers say"
below.

**The thin loops rebuilt a dynamic property key on every call.** `geometryThin`, `searchThin`,
`textThin`, and `renderThin` in `bench/workloads.mjs` called through `fn[`${p}Name`](...)` inside
their hot paths, so every call rebuilt a template string and did a dynamic property lookup. Measured
at about 7.7 ns per lookup, that is roughly 62 µs per `geometryThin()` call (two guaranteed lookups
per glyph over 8,106 glyphs) and about 601 lookups per `searchThin()` call, enough to inflate the
coarse win by roughly 4 percent on `geometry` and 10 percent on `search` in the previous run. The
four thin functions now resolve their per-arm function references once, into local consts, outside
the loop, the same way `p` and `coarse` were already resolved once per arm. This run does not
isolate the before/after delta with a controlled pair of runs; the fix is applied going forward, and
the `geometry` and `search` numbers below should read a few percent closer to the true coarsening
win than the previous file's did.

The hoist reaches the thin side only. `geometryCoarse`, `textCoarse`, `renderCoarse`, and
`searchCoarse` still rebuild the same kind of template string and do a dynamic property lookup
inside their timed body, once per call rather than once per glyph or hit, because a coarse call
crosses once instead of looping per record. One lookup against a 563 µs to 2.8 ms call is immaterial
on its own, but it means this fix equalized which side pays a per-call cost, not which side pays
none at all.

**`searchCoarse` broke the rule `geometryCoarse` follows.** `searchCoarse` read its result buffer
with `mem.readBytes`, a `HEAPU8.slice` copy, while `geometryCoarse` deliberately reads through a
live-heap `DataView` and carries a comment explaining why (the previous run's Finding 6).
`searchCoarse` now builds the same kind of live-heap `DataView`, rebuilt on every call for the same
heap-growth reason, with the same explanation. In practice this fixture's search hits are few
enough (`search`'s result buffer is a small fraction of `geometry`'s 8,106-record buffer) that the
measured effect is close to zero; the principle was still worth fixing, and the two coarse paths no
longer disagree about it.

**`bench/probe.test.mjs` proved less than it claimed.** It called `EPDF_GetPageSizeByIndexNormalized`
into a buffer, read the width and height, then called `rs_probe_page_size` into the same buffer and
asserted the buffer still held those values. An `rs_probe_page_size` that returned 1 and wrote
nothing would have passed. It now uses two separate buffers, so the assertion proves the two paths
actually agree, plus the existing plausibility check that the dimensions exceed zero. This is a test
correctness fix with no effect on the timing numbers.

**`bench/run.mjs` under-reported gate coverage.** It printed "agree on 8106 glyphs, page 4" while
`compare()` had actually checked both entries in `GATE_FIXTURES`; `bench/gate.mjs`'s own CLI printed
the honest two-fixture line. `run.mjs` is the artifact people read, so it now reports every fixture
compared, in the same wording `gate.mjs` uses; that is the gate line quoted at the top of this file.

**This file was publishing a stale test result.** It stated the suite "passed 10 of 11 tests" and
that `probe.test.mjs` failed importing a build output no current script produces. Commit `d6108963`
repointed that test before this run. This re-run's suite
(`gate.test.mjs`, `arm-b.test.mjs`, `arm-c.test.mjs`, `arm-d.test.mjs`, `fixture.test.mjs`,
`probe.test.mjs`, `native-b.test.mjs`, `native-cd.test.mjs`, eight files, swapping `run.test.mjs` for
the two native-arm suites) passed all 13 of 13 tests, `gate.test.mjs` alone contributing six of
them. There is no known failing test in this suite as of this run.

## Earlier bugs already fixed, unaffected by this run

The two runs before this one already corrected an unoptimized-link bug (arm d was built at `-O0`
against arm c's Rust at `-O3`, producing a fake 25-to-33-percent `geometry` gap that was a compiler
flag, not a language difference) and a decode-allocation bug (`geometryCoarse` allocated four
throwaway arrays and closures per glyph where `geometryThin` did not). Both fixes are unchanged by
today's findings and are still in effect; see the git history on this file for the detail that was
here before. `build/link-wasm.sh` still links all four arms at `-O3` from the one shared `link()`
body, with the substituted Homebrew `wasm-opt` 132 standing in for the emsdk 3.1.72 build's own
`wasm-opt`, which predates the two wasm feature names rustc 1.98's precompiled std emits. That
substitution is a local build-environment workaround, not a committed change, and it is what
produced the arm b, c, and d binaries measured below.

## Environment

```
Node v24.20.0 · Apple M4 Max · darwin 25.6.0
arch arm64 · 16 cores · loadavg 4.31 at the start of the run
emsdk emcc 3.1.72 (437140d) · rustc 1.98.0 (88d9e12ae, 2026-08-18)
fixture: examples/snippet-react/public/report.pdf, page 4, 8106 glyphs
rounds: 7, interleaved a/b/c/d with the order rotated each round, budget 500ms per cell
```

`emsdk` is not `unknown` here: `build/emsdk.sh` was sourced before `bench/run.mjs` ran, so `em++`
was on `PATH` and `bench/node.mjs` recorded the real version instead of falling back to its
default.

The `node --test` suite passed 13 of 13 tests across the eight files listed above. No test failed
or was skipped.

macOS thermal throttling is not controllable from here. Rounds are interleaved and rotated in order
within each round, for exactly that reason: drift and position bias should land on all four arms
rather than on whoever runs first or last. Worst observed round-to-round spread (max/min across the
7 rounds, same arm and workload):

```
d/geometry-copyout 1.28x, b/null-peek 1.19x, a/render-copyout-4x 1.14x
```

`null-peek` measures a single `getValue` call, around 4 nanoseconds; a 1.2x spread there is a
sub-nanosecond absolute wobble that a 500ms sampling budget cannot fully average out at that scale.
`bench/compare.mjs` treats spread past about 1.3x as a thermal-drift signal; the largest spread here
sits at 1.28x, just under that line, so nothing here signals throttling distorted the comparisons
that matter.

A note on what the numbers mean: `harness.mjs` measures in batches (auto-tuned so one batch takes at
least 2ms), and reports the median and p95 of those batch means, not of individual calls. The p95
column below is the p95 of batch means: it shows run-to-run variance within a workload, not
per-call tail latency, and should not be read as a worst-case frame cost.

## Wasm

### Median nanoseconds per call, by workload and arm

| workload | a direct | b rust thin | c rust coarse | d c++ coarse | b/a | c/a | d/a |
|---|---|---|---|---|---|---|---|
| `null-call` | 350.6 ns | 328.5 ns | n/a | n/a | 0.94x | n/a | n/a |
| `null-peek` | 4.0 ns | 4.0 ns | n/a | n/a | 0.99x | n/a | n/a |
| `null-readbytes` | 30.4 ns | 30.5 ns | n/a | n/a | 1.00x | n/a | n/a |
| `geometry` | 2.711 ms | 2.787 ms | 1.055 ms | 1.050 ms | 1.03x | 0.39x | 0.39x |
| `geometry-copyout` | n/a | n/a | 19.60 µs | 19.68 µs | n/a | n/a | n/a |
| `text` | 119.11 µs | 121.31 µs | 121.63 µs | 120.17 µs | 1.02x | 1.02x | 1.01x |
| `render-1x` | 2.844 ms | 2.894 ms | 2.897 ms | 2.808 ms | 1.02x | 1.02x | 0.99x |
| `render-4x` | 15.818 ms | 16.429 ms | 16.449 ms | 15.797 ms | 1.04x | 1.04x | 1.00x |
| `render-copyout-4x` | 591.76 µs | 598.23 µs | 593.67 µs | 601.39 µs | 1.01x | 1.00x | 1.02x |
| `search` | 100.64 µs | 102.87 µs | 30.60 µs | 32.10 µs | 1.02x | 0.30x | 0.32x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 372.7 ns | 345.6 ns | n/a | n/a |
| `null-peek` | 4.1 ns | 4.1 ns | n/a | n/a |
| `null-readbytes` | 31.5 ns | 31.5 ns | n/a | n/a |
| `geometry` | 3.002 ms | 3.143 ms | 1.115 ms | 1.134 ms |
| `geometry-copyout` | n/a | n/a | 22.94 µs | 23.86 µs |
| `text` | 124.59 µs | 126.19 µs | 127.69 µs | 127.09 µs |
| `render-1x` | 2.946 ms | 2.988 ms | 3.006 ms | 2.904 ms |
| `render-4x` | 16.234 ms | 16.745 ms | 16.781 ms | 16.337 ms |
| `render-copyout-4x` | 628.92 µs | 656.08 µs | 664.66 µs | 650.34 µs |
| `search` | 104.20 µs | 106.86 µs | 31.79 µs | 33.37 µs |

### Module size

| arm | wasm bytes | delta vs a |
|---|---|---|
| a direct | 4,676,714 | n/a |
| b rust thin | 4,680,516 | +3.7 kB |
| c rust coarse | 4,681,488 | +4.7 kB |
| d c++ coarse | 4,684,877 | +8.0 kB |

These are the same wasm byte counts as the previous run: no link flag or exported-function set
changed here, only the JS-side benchmark code and the driver.

## What the numbers say

With arm order rotated, `null-call` still shows arm b faster than arm a, in every one of the seven
rounds: a ran 346.4, 348.3, 354.4, 361.6, 350.6, 353.4, 348.4 ns across the seven rounds, in that
order, while b ran 323.6, 324.3, 329.8, 334.0, 328.5, 329.5, 327.2 ns in the same seven rounds. Arm a
occupied every position from first to last across those rounds (round order was a,b,c,d then
b,c,d,a then c,d,a,b then d,a,b,c then a,b,c,d then b,c,d,a then c,d,a,b), and b still came out ahead
every time. That rules out the position-decay explanation the previous run's rotation was meant to
test for. The honest reading is not that arm b's wrapper has negative cost: a wrapper cannot be
faster than the bare call it wraps. The honest reading is that this cell, at 300-some nanoseconds a
call with a roughly 20-to-28-nanosecond gap between arms, is below what a 500ms sampling budget and
Node's own call overhead can resolve on this hardware. The `null-call` cell cannot currently tell
these two arms apart; it cannot be read as evidence of a negative tax, and it cannot yet be read as
evidence the wrapper is free either. Something at or below the noise floor of this measurement setup
is happening in that gap, and this benchmark does not have the resolution to say what.

On the workloads that do real PDFium work, the wrapper tax reads as small and directionally
consistent: arm b costs 1 to 4 percent more than arm a on `geometry` (1.03x), `text` (1.02x),
`render-1x` (1.02x), `render-4x` (1.04x), and `search` (1.02x). That is inside the pre-registered 15
percent band in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md),
and well inside the 25 percent line past which the prediction said to suspect the harness rather
than the result.

The `render-1x` and `render-4x` numbers are a half dose of that tax, undisclosed until now.
`crates/shim/src/lib.rs` exports no `rs_FPDFBitmap_FillRect`, so `renderThin` calls the unprefixed
`fn.FPDFBitmap_FillRect` on every arm, arm b included, rather than going through arm b's shim.
`renderThin` makes two PDFium calls per render, `FPDFBitmap_FillRect` and the actual render call, and
arm b only pays its one extra wasm frame on the second. So the 1.02x and 1.04x quoted above for
`render-1x` and `render-4x` price one shim frame out of two calls, not two out of two: the true
per-call wrapper tax on a fully-shimmed render is conservatively higher than what these two cells
report. Not fixed here, because adding the missing shim function would require a rebuild and a
re-run.

`render-4x` is the result the previous write-up missed. Arm a and arm d cluster low in six of the
seven rounds (a from 15.585 to 16.477 ms, d from 15.641 to 16.278 ms) while arm b and arm c cluster
higher (b from 16.335 to 16.486 ms excluding one outlier round, c from 16.299 to 16.579 ms). Round 6
is the exception, and this time it inverts the order outright rather than just blunting it: b at
16.454 ms and c at 16.579 ms are the two lowest of the round, while d at 16.668 ms and a at
16.688 ms, ordinarily the low pair, become the two highest. Because the inversion this round hits
both a and d rather than only a, "arm d stayed fastest regardless" does not hold here the way the
previous write-up found it did; d is the fastest arm in five of the seven rounds and loses that spot
in rounds 1 (to a) and 6 (to b). Rendering calls no Rust code
on any arm's timed path (arm b's `rs_FPDF_RenderPageBitmap` is a 1:1 shim around the same PDFium
call arm a makes, and arm d's `cc_render_page` is C++), so this is not a coarsening effect and not a
wrapper-call effect. Arms b and c are the two arms with a Rust static archive (`libshim.a`,
`libops.a`) linked into the module; arms a and d are not. The pattern that survived rotation is
consistent with linking a Rust archive into the module costing something on this workload,
independent of whether any Rust code in that archive is ever called, rather than with anything to
do with coarsening the crossing. Six of seven rounds is not all seven, and this benchmark was not
built to isolate what about the link produces the effect (code layout, page cache behavior, or
something in how Binaryen's `-O3` pass treats the extra section are all consistent with what is
measured here); the finding is the correlation with which arms link a Rust archive, not a mechanism.

There is evidence in this same run that cuts against that reading, and it belongs here rather than
in a footnote. On `null-call` the Rust-linked arm is the FASTER one, by 21 to 28 nanoseconds in
every round. If linking `libshim.a` imposed a general cost on the module, arm b should be slower
there too, and it is not. So the two observations do not tell one story: on a millisecond-scale
PDFium workload the two Rust-linked arms are slower, and on a microsecond-scale one the Rust-linked
arm is faster. What both are consistent with is that each arm is a separately linked binary, and
that code layout between two separate links moves PDFium-dominated timings by a few percent in
whichever direction it happens to land. That reading also explains why the `null-call` gap survived
rotation: it was never a scheduling artifact.

This matters for how much weight any sub-5-percent cross-arm difference in this document can carry,
including the render one. Treat differences at that scale as unresolved rather than as findings. The
experiment that would separate the two explanations is cheap and was not run: link `libshim.a` into
a module that never calls into it, and compare that against arm a. If the render gap follows the
archive, it is a linkage cost; if it does not, it was layout. Worth doing before anyone acts on it.

Coarsening the crossing wins on `geometry` and `search`, for both languages behind it. Arm c lands
at 0.39x and arm d at 0.39x on `geometry` (about 2.6x faster than arm a); on `search`, arm c is 0.30x
and arm d is 0.32x (about 3.3x and 3.1x faster than arm a). Search landed inside the prediction's
3x-to-10x band for coarsening a chatty per-glyph crossing into one call; geometry, at 2.6x, landed
just under it. `geometryThin` makes two
wasm calls per glyph (`FPDFText_GetTextObject` and `EPDFText_GetCharGeometry`) plus a third on every
object-key change, so 8,106 glyphs cost on the order of sixteen thousand crossings, where
`geometryCoarse` makes exactly one crossing and decodes PDFium's per-glyph struct into a packed
112-byte record on the native side. `render` and `text` land within 1 to 4 percent everywhere,
matching the prediction that render is dominated by PDFium's own bitmap work and that a single
already-coarse call like `text` has nothing left for coarsening to remove.

Arm c and arm d are indistinguishable on `geometry`: the median gap is under half a percent (c
1.055 ms, d 1.050 ms) and the per-round ratio of d over c runs 0.977x to 1.029x across all seven
rounds, a tighter spread than the worst same-arm, same-workload round-to-round spread measured
anywhere in this run (1.28x on `d/geometry-copyout`). On `search`, the two arms are close to
indistinguishable but not quite: arm c beat arm d in six of the seven rounds, by 4 to 9 percent
(per-round ratio of d over c ran 1.046x to 1.088x over those six); round 1 is the exception, where
arm d was faster instead, by about 3 percent (31.51 µs against 32.35 µs). That gap is larger than
either arm's own round-to-round spread in the rounds where it shows up. This is the one
place in this benchmark where a consistent, repeatable difference between the Rust and C++
implementations of the same coarse operation shows up. It is a difference between
`rs_search_page` and `cc_search_page` specifically, on this fixture's roughly six hundred search
hits, not a general claim about the two languages: `geometry`, the workload with far more records
crossing per call, shows no such gap. The two operations were written independently and are not
required to compile to identical code; the fact that one shows a small, consistent gap and the
other does not is itself informative that the languages are not the moving part on `geometry`.

These are lower bounds on real per-call cost. `bench/arm.mjs` cwraps every arm by hand, calling each
exported PDFium function directly. It does not go through the engine's per-call argument bridge in
`packages/engine/runtime/src/wasm/wasm-runtime.ts`, which does type dispatch and argument
marshalling on every call in the shipping runtime. That bridge runs identically regardless of which
arm sits behind it, so it would add a constant amount of overhead to every column here without
separating any of them. Skipping it keeps the comparison clean, but it also means every absolute
number in this file understates what a real call costs today.

`geometry` is a transcription, not the reader. It reproduces the loop at
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
inflate it.

A related asymmetry in how thin reads its fields is not fixed here and should be disclosed instead:
`geometryThin` reads each field through `mem.peekF32`, which calls into `Module.getValue` under this
build's `-sASSERTIONS=1`, while `geometryCoarse` reads the same shape of data through a plain
`DataView`. Measured over 8,106 records that is 202.4 µs against 121.3 µs, roughly 81 µs of thin's
cost on this fixture. That is not being changed, because `mem.peekF32` is standing in for the same
memory-helper call the real engine runtime uses; fixing it in the benchmark would make the thin arm
look faster than the code it is modelling. So a few percent of the `geometry` and `search`
coarsening win in the tables above comes from this read-mechanism difference rather than from
coarsening itself, on top of the transcription omissions above, both of which run in the same
direction: understating the thin side, and so understating, never overstating, the coarsening win.

One more asymmetry runs the other way, against the coarse arm, and is also not fixed here.
`geometryCoarse` follows the rule that the coarse and thin decodes must differ only in where the
data comes from, never in how much garbage they make (`bench/workloads.mjs`'s comment above
`dvRectAt`); `searchCoarse` does not. Its hit loop builds `hits[i] = [0, 4, 8, 12].map(...)`, one
throwaway array literal and closure per hit, about 600 per call on this fixture. That is the same
class of allocation bug this run's Finding 6 already fixed elsewhere, and it is left in place rather
than fixed, because fixing it would change a published number, and re-running the benchmark to
correct it is not worth a change that can only improve the coarse arm's own result. So the published
`search` coarsening figures, 3.3x and 3.1x, understate the true win slightly.

The timing fixture never exercises the rotation branch. `report.pdf` page 4 has 0 non-upright glyphs
and 0 SPACE-flagged glyphs (see `fixture.mjs`'s header comment). The fairness gate's second fixture,
`rotated_text.pdf` page 0, is entirely non-upright and does cover that branch and the ascent-flip and
SPACE-flag paths, and all four arms agree on it. But no number in the tables above was produced by a
call that took that branch. If the rotation math has a performance cost distinct from the upright
path, it is not measured here.

What this does not say: it says nothing about `engine-services`, its 16,000-odd lines, or how well
they would port. It says nothing about iOS or Android, which is the actual argument for the port in
[`docs/research/rust-core-port.md`](../docs/research/rust-core-port.md); if every number above
argued against Rust, the case for the port would be unchanged, because the case was never about web
performance. This section is Node only, wasm only. `mem.peek` is an N-API round trip natively rather
than a typed-array read, for the reasons in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md#native-after),
and native was expected to disagree with the numbers above. It does, on one workload and not the
other; see the Native section below.

## Native

Same four arms, same gate, same fixture, same rotation protocol, run against native addons instead
of wasm modules. Arm a is `packages/engine/runtime`'s own shipping addon, built with
`node-addon-api` in C++ and taking BigInt pointers. Arms b and c are one `napi-rs` addon built from
this spike's own Rust crates, taking `f64` pointers. Arm d is a second `node-addon-api` addon
(`cpp/ops_napi.cc`, built through `build/CMakeLists.txt`) wrapping the same `cpp/ops.cc` the wasm arm
d uses, also on `f64` pointers.

The native fairness gate ran before any timing, over both gate fixtures, and passed:

```
native fairness gate: a, b, c, d agree on report-p4 (8106 glyphs), rotated-p0 (31 glyphs), 5 workloads compared
```

Before that gate ran, `bench/arm.mjs`'s `probeExternalBuffers` confirmed `view_buffer` hands back a
real zero-copy view rather than a silent copy (see "Fixes applied before this run" below); it passed
on this Node build, so the coarse-arm numbers below are not paying a hidden memcpy.

Seven rounds, interleaved and rotated by round index exactly as the wasm run was: round 1 ran
a,b,c,d; round 2 b,c,d,a; round 3 c,d,a,b; round 4 d,a,b,c; round 5 a,b,c,d; round 6 b,c,d,a; round 7
c,d,a,b. Each result file under `bench/results/native-<arm>-r<round>.json` records the order its own
round used. `wasmBytes` is `null` in every one of those files; there is no wasm module to size.

### Environment

```
Node v24.20.0 · Apple M4 Max · darwin 25.6.0
arch arm64 · 16 cores · rustc 1.98.0 (88d9e12ae, 2026-08-18)
fixture: examples/snippet-react/public/report.pdf, page 4, 8106 glyphs
rounds: 7, interleaved a/b/c/d with the order rotated each round, budget 500ms per cell
```

Loadavg, sampled from the first arm scheduled in each round's rotation, ran 2.61, 3.09, 2.91, 3.44,
4.19, 3.87, 3.57 across the seven rounds: it rose through the first five rounds and eased off in the
last two, not the monotonic fall the previous sitting recorded. That trend does not by itself rule
out thermal drift the way a falling loadavg would; the round-to-round pattern on one workload below
is the more direct evidence, and is discussed there on its own terms rather than argued from
loadavg.

### Median nanoseconds per call, by workload and arm

| workload | a direct | b rust thin | c rust coarse | d c++ coarse | b/a | c/a | d/a |
|---|---|---|---|---|---|---|---|
| `null-call` | 205.1 ns | 191.5 ns | n/a | n/a | 0.93x | n/a | n/a |
| `null-peek` | 38.8 ns | 14.9 ns | n/a | n/a | 0.38x | n/a | n/a |
| `null-readbytes` | 273.0 ns | 288.6 ns | n/a | n/a | 1.06x | n/a | n/a |
| `geometry` | 8.612 ms | 4.148 ms | 577.05 µs | 579.94 µs | 0.48x | 0.07x | 0.07x |
| `geometry-copyout` | n/a | n/a | 71.12 µs | 71.96 µs | n/a | n/a | n/a |
| `text` | 126.82 µs | 126.38 µs | 128.21 µs | 128.88 µs | 1.00x | 1.01x | 1.02x |
| `render-1x` | 1.546 ms | 1.563 ms | 1.560 ms | 1.553 ms | 1.01x | 1.01x | 1.00x |
| `render-4x` | 8.491 ms | 8.501 ms | 8.510 ms | 8.569 ms | 1.00x | 1.00x | 1.01x |
| `render-copyout-4x` | 1.634 ms | 2.380 ms | 2.493 ms | 2.508 ms | 1.46x | 1.53x | 1.54x |
| `search` | 81.83 µs | 64.23 µs | 39.65 µs | 39.83 µs | 0.78x | 0.48x | 0.49x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 220.8 ns | 203.0 ns | n/a | n/a |
| `null-peek` | 40.6 ns | 15.2 ns | n/a | n/a |
| `null-readbytes` | 291.6 ns | 303.1 ns | n/a | n/a |
| `geometry` | 9.825 ms | 4.586 ms | 642.67 µs | 673.12 µs |
| `geometry-copyout` | n/a | n/a | 91.80 µs | 94.34 µs |
| `text` | 133.84 µs | 135.06 µs | 137.21 µs | 135.08 µs |
| `render-1x` | 1.640 ms | 1.639 ms | 1.608 ms | 1.634 ms |
| `render-4x` | 8.877 ms | 8.835 ms | 8.736 ms | 8.938 ms |
| `render-copyout-4x` | 1.779 ms | 3.180 ms | 6.621 ms | 6.830 ms |
| `search` | 84.90 µs | 67.07 µs | 40.70 µs | 42.16 µs |

There is no wasm-byte-size table here; `bench/compare.mjs --native` skips it, since `wasmBytes` is
null on every native result file.

Worst round-to-round spread (max/min across the seven rounds, same arm and workload):

```
c/render-copyout-4x 1.99x, a/render-copyout-4x 1.80x, d/render-copyout-4x 1.20x
```

`bench/compare.mjs` treats a spread past about 1.3x as a thermal-drift signal, and three of the four
arms cross it on `render-copyout-4x`, worse than the previous sitting recorded on this same cell
(1.68x). The loadavg trend logged above does not cleanly rule out thermal drift the way it did last
time (loadavg rose rather than fell through most of this run), so this run leans on the shape of the
per-round numbers rather than on loadavg to make the case. That shape does not match gradual
throttling. Arm a and arm c both spike hardest in round 1 (a at 2.890 ms against 1.605-1.641 ms for
rounds 2-7, about 78 percent above that range's mean; c at 4.556 ms against 2.293-3.027 ms, about 79
percent above), which a monotonic thermal effect would not produce on its own. Arms b and d do not
peak in round 1 at all: b peaks in round 2 (2.657 ms against 2.327-2.545 ms elsewhere, about 11
percent above the rest) and d peaks in round 2 as well (2.920 ms against 2.440-2.665 ms elsewhere,
about 16 percent above the rest). `render-copyout-4x` copies the full 4x bitmap out with
`mem.readBytes`, roughly 31 MB per call, the single largest copy in this benchmark, and a transient
concentrated in the first one or two rounds, differently timed per arm, reads more like a cold-cache
or first-large-allocation effect than like drift that should grow smoothly across all seven rounds
and hit every arm at the same time. It is a weaker case than the previous sitting could make,
because excluding round 1 alone does not fully tame it this time: arm c's own round 2 value
(3.027 ms) still leaves a 1.32x spread across rounds 2-7, over the compare.mjs line by itself.
Excluding round 1, the remaining spread is 1.02x for a, 1.14x for b, 1.32x for c, and 1.20x for d.
Treat this cell's absolute numbers as noisier in this run than elsewhere in this document, and treat
the `render-copyout-4x` finding below (arm a diverging from b, c, and d) as the only thing this cell
is used to support.

### The a-versus-b confound

Native arm a is `node-addon-api` C++ taking BigInt pointers; native arm b is `napi-rs` Rust taking
`f64` pointers. An a-versus-b gap here mixes three things that changed at once: the language, the
binding framework generating the N-API glue, and the pointer representation crossing the boundary on
every argument. The wasm run could report a wrapper tax for arm b (1 to 4 percent, see "What the
numbers say" above) because all four wasm arms share one Emscripten export surface; nothing else
changed. That precondition does not hold here, and neither `b/a` nor any ratio computed against arm
a in the table above should be read as a native wrapper tax. Reporting `b/a`'s 0.93x on `null-call`
as "the Rust wrapper is 7 percent cheaper natively" would repeat the exact mistake `null-call`'s wasm
write-up already flagged, with one more confound layered on.

The two clean comparisons are the ones holding language and framework fixed: b versus c (`napi-rs`
Rust, thin versus coarse) and a versus d (`node-addon-api` C++, thin versus coarse, `cpp/ops_napi.cc`
says outright that this pairing exists so "A-versus-D isolates 'coarse' from everything else"). Even
that pairing is not perfectly clean: arm a still carries the BigInt marshalling arm d does not, on
every one of the roughly sixteen thousand crossings `geometryThin` makes over this fixture's 8,106
glyphs (two guaranteed BigInt-marshalled calls per glyph plus one more on every object-key change,
as the wasm section above quantifies). The coarsening numbers below use both pairings and say which
one is more trustworthy where it matters.

That said, the size of the gap is worth recording as a lead rather than only as a nuisance. On
`geometry` arm a takes 8.612 ms and arm b 4.148 ms for the same PDFium calls in the same order, and
on `null-peek` 38.8 ns against 14.9 ns. Whatever the cause, something in the shipping addon's
per-call path costs roughly twice what the `napi-rs` path costs, and it costs it on the read that
`PageGeometryReader` performs up to twenty-one times per glyph through `mem.peek` at
`packages/engine/runtime/src/native/native-runtime.ts:57`. I cannot say from these numbers how much
of that is the BigInt pointer representation and how much is `node-addon-api` against `napi-rs`,
because this benchmark changed both at once and was never designed to separate them. But the
question is worth answering on its own, independently of anything to do with Rust, and it is cheap
to answer: add an f64-pointer entry point to the existing C++ addon and time the two conventions
through one binding framework. If the BigInt convention is most of it, that is a change to the
addon's argument marshalling with no port attached.

### Geometry: the predicted result

The prediction in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md#predictions)
was that the native `geometry` win would be larger than wasm's, "plausibly past 10x," because
`mem.peek` is a full N-API round trip natively and a typed-array read in wasm. It is larger, by more
than the prediction's own floor.

Held within one language and framework, thin-to-coarse coarsening wins 7.19x on this fixture (arm b
at 4.148 ms against arm c at 577.05 µs, both `napi-rs`) and 14.85x held within C++ (arm a at 8.612 ms
against arm d at 579.94 µs, both `node-addon-api`, modulo the BigInt caveat above, which likely
inflates arm a's absolute cost and therefore this second number somewhat). Either way, this is 2.8x
to 5.8x larger than wasm's `geometry` win of about 2.6x. The direction and the rough size both match
the prediction.

The mechanism is visible in the null-probe cells, but it is not the same peek for both pairings.
Arm a's `mem.peek32` costs 38.8 ns natively against 4.0 ns in wasm, roughly 9.7x, and that is the
pairing behind the 14.85x figure. Arm b's `mem.peek32` costs 14.9 ns natively against 4.0 ns in
wasm, roughly 3.7x, and that is the pairing behind 7.19x. Nine or ten times is close to the right
multiple for arm a's peek; well under four times is the multiple for arm b's.

Arm a's peek is expensive for reasons that have nothing to do with coarsening. `bench/arm.mjs:243`
calls `raw.peek(P(p), 'i32')`, converting the pointer to a BigInt through `P()` and dispatching on a
JavaScript string type tag on every call, the same shape as the shipping addon's `peek(ptr, kind)`
at `packages/engine/runtime/src/native/native-runtime.ts:57`. `readGlyphRaw` (`bench/workloads.mjs`)
makes up to 21 such peeks per glyph, on top of the object-key and font-size calls every glyph pays;
over 8,106 glyphs that call happens on the order of 170,000 times in one `geometry` iteration, and
`rs_read_page_geometry` collapses all of it into one crossing. So the 14.85x figure prices
string-tag dispatch, BigInt marshalling, and node-addon-api's own per-call cost together, not
"coarsening held within C++" by itself; arm d's coarse call never goes through arm a's peek path, so
none of those three costs is shared between the two arms being compared. That is three distinct
effects bundled into one number, not one effect at a larger scale. The 7.19x figure is the more
trustworthy of the two: arm b's peek is a direct `f64` call into `napi-rs` with no string dispatch
and no BigInt conversion, so it avoids all three.

`geometry-copyout` gives one more data point for the same story. It isolates the cost of a full
memcpy readback of the packed record buffer (908 KB): 71.12 µs (c) and 71.96 µs (d) natively against
19.60 µs and 19.68 µs in wasm, roughly 3.6x the cost for the same number of bytes. `view_buffer`'s
zero-copy design (Fix 1 above) is worth more here than it was in wasm, precisely because the memcpy
it avoids costs more here than it did there.

### Search: the prediction did not hold

The wasm write-up's search win was 3.1x to 3.3x. Held within one language and framework, native
search wins only 1.62x (b at 64.23 µs, c at 39.65 µs, both `napi-rs`) and 2.05x held within C++ (a at
81.83 µs, d at 39.83 µs). Both are smaller than wasm's, not larger. This is the one place in this run
where "native's coarsening win is bigger" does not hold, and it should be reported as a real result,
not smoothed over to match the geometry finding or the pre-registered prediction.

The likely reason is visible in the same null-probe cells that explain `geometry`. `searchThin`
(`bench/workloads.mjs`) is dominated by function calls, not peeks: `FPDFText_FindNext`,
`GetSchResultIndex`, `GetSchCount`, `CountRects`, and `GetRect` per hit, against roughly six hundred
hits on this fixture, with only four `peekF64` calls per matched rect. `null-call` costs 205.1 ns
natively against 350.6 ns in wasm for arm a, a native call costing a bit over half (0.58x) a wasm
one on this machine. `search`'s thin loop is call-bound rather than peek-bound, and the operation
being
collapsed away was already cheaper natively than it was in wasm, so there is less to win by
collapsing it. This is inferred from the shape of the two loops and the null-probe numbers, not
measured directly cell for cell, and it would take a dedicated per-call-type breakdown to confirm.

### Arms c and d: still indistinguishable, and more so than in wasm

`geometry`'s per-round ratio of d over c runs 0.994x to 1.024x across the seven rounds, tighter than
this run's own worst same-arm spread (1.99x on `c/render-copyout-4x`, discussed above, or 1.20x on
`d/render-copyout-4x` excluding that cell's round-1/round-2 anomaly). `search`'s per-round ratio
runs 0.965x to 1.022x, with no consistent direction: c beats d in four of the seven rounds and d
beats c in three. That is a change from wasm, where arm c beat arm d in six of the seven rounds on
`search`, by 4 to 9 percent, a small but mostly repeatable gap the wasm write-up called out as the
one place a language difference showed up. That gap is gone natively.
Whether it went away because the operation changed (a native coarse call does more per crossing: it
also builds the zero-copy `ArrayBuffer` view, see the caveat below) or because it was never a
language effect to begin with and wasm's Binaryen `-O3` pass treated the two static libraries
differently is not something this run can separate; the honest statement is that the wasm-only
finding did not reproduce here.

`text`, `render-1x`, and `render-4x` land within about 2 percent across all four arms, matching the
prediction that rendering is dominated by PDFium's own bitmap work and that an already-coarse call
like `text` has nothing left for coarsening to remove, the same result wasm found.

`render-copyout-4x` is the one workload where arm a and arms b/c/d visibly diverge (1.634 ms against
2.380-2.508 ms; these medians include the round-1/round-2 anomaly discussed above, which affects all
four arms to varying degrees but does not change which side of the gap either one falls on), and it
should not be read as a
coarse-versus-thin or Rust-versus-C++ result. Arms b, c, and d all read their bytes through
`napi-arms.node`'s `readBytes`; arm a uses its own shipping addon's implementation. The gap is
consistent with the two addons implementing that one function differently, not with anything this
study is measuring.

### What the native coarse arms pay that wasm's do not

`geometryCoarse` and `searchCoarse` call `mem.viewBuffer` on every timed iteration. In wasm that is a
plain object literal aliasing the live `HEAPU8.buffer` (`bench/arm.mjs`'s wasm `viewBuffer`), free of
any allocation. Natively it is `napi-arms.node`'s `view_buffer`, which asks V8 for a real external
`ArrayBuffer`, backed by a `BackingStore` and a finalizer callback, once per call. That is one V8
object and one finalizer registration per coarse call, not a memcpy, and it is far cheaper than the
copy it replaces (see `geometry-copyout` above). But it is a real asymmetry against the wasm coarse
arms, present in every `geometry` and `search` number for arms c and d in this section, and it is not
present in arm a or arm b's numbers at all. The coarsening win reported above is the win net of this
cost, not in addition to it.

`view_buffer` (`crates/napi-arms/src/lib.rs:231`, the lock and lookup at `:251`) pays one more
small, undisclosed cost on top of the `BackingStore` and finalizer above: it takes a mutex lock and
does a `HashMap` lookup against `ALLOCATIONS` on every coarse call, to bound `len` against the
allocation actually registered for that pointer (Fix 2 above). Same direction as the `BackingStore`
cost, and just as tiny relative to the calls it sits inside; noted here for completeness rather than
because it moves any number in this section.

### Fat LTO versus thin LTO

Arm c's `Cargo.toml` sets `lto = true`, fat LTO across the whole dependency graph. Arm d's
`build/CMakeLists.txt` enables `INTERPROCEDURAL_OPTIMIZATION`, which on this toolchain (Apple clang
21.0.0, confirmed in `build/build/build.ninja`) resolves to `-flto=thin`, not fat LTO. The two are
not the same optimization. `CMakeLists.txt` never enforced that they even agree on trying: until this
run, `check_ipo_supported`'s failure was silently swallowed, so a toolchain without IPO support at
all would have built arm d with no cross-TU inlining while arm c kept fat LTO unconditionally, and
nothing would have said so. The CMake fix in this run turns that silence into a `message(WARNING)`
naming the consequence. On this toolchain IPO was supported, so arm d did get thin LTO; arm c's fat
LTO and arm d's thin LTO are still two different compilation strategies, and the close agreement
between c and d in every table above is despite that difference, not because it was controlled away.

### Fixes applied before this run

Three findings surfaced before this run were fixed rather than reported as caveats, because each one
could otherwise have corrupted the numbers above invisibly:

`crates/napi-arms/src/lib.rs`'s `view_buffer` used
`env.create_arraybuffer_with_borrowed_data`, which maps to `napi_create_external_arraybuffer`. If a
runtime returns `napi_no_external_buffers_allowed` (Electron, or `NODE_API_DISABLE_EXTERNAL_BUFFERS`),
napi-rs falls back silently to `napi_create_arraybuffer` plus a `ptr::copy_nonoverlapping`, a full
memcpy per call, with no error and no log. That would have restored the exact copy asymmetry the
coarse arms exist to remove, invisibly. `bench/arm.mjs` now probes this once at native arm load
(`probeExternalBuffers`): it writes a sentinel through `mem.writeBytes`, takes a view through
`mem.viewBuffer`, mutates a byte through that view, and confirms the mutation is visible through
`mem.peek32` on the original pointer. A prior version of this same idea wrote through `writeBytes`
and read the pointer back, which proves the pointer is readable and nothing about whether the view
aliases it; mutating through the view is the only direction that actually distinguishes a view from a
copy. This probe passed on Node v24.20.0, so the numbers above are not paying a hidden copy, but it
did not pass by construction; a run under Electron should check this line before trusting its own
coarse-arm numbers.

`view_buffer`'s `len: u32` parameter was unchecked. A negative record count from a coarse function
(nothing produces one today) would wrap to roughly 4 GiB and hand back an external `ArrayBuffer`
spanning memory this addon does not own, silently readable from JS, where wasm's `DataView` would
throw `RangeError` instead. `alloc` now records the byte length behind every pointer it returns in a
process-wide map, and `view_buffer` refuses a `len` that exceeds the allocation registered for its
pointer, returning a JS `Error` instead of aliasing foreign memory.

`build/CMakeLists.txt` swallowed a `check_ipo_supported` failure silently, described above under
"Fat LTO versus thin LTO." It now emits a `message(WARNING)` naming the asymmetry that would result.

None of the three changed a measured number in this run: the probe passed, no coarse call in this
fixture asks for an out-of-range view, and IPO was supported on this toolchain. They are here because
each one is a way this specific study was already shown to fail once (the unoptimized-link bug in
"Earlier bugs already fixed" above is the same class of failure as the LTO one) and none of the three
would have announced itself if it had happened.
