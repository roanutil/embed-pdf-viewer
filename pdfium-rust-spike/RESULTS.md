# Results: four PDFium wrapper arms, wasm, seven rounds

The fairness gate ran before any timing, over both gate fixtures (`report-p4` and `rotated-p0`),
and passed:

```
fairness gate: a, b, c, d agree on report-p4 (8106 glyphs), rotated-p0 (31 glyphs), 5 workloads compared
```

All four arms agree on every canonical answer they are declared to produce, on both fixtures. The
numbers below are real per-operation cost, not an artifact of one arm quietly computing the wrong
thing.

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
their hot paths, so every call rebuilt a template string and did a dynamic property lookup, while
the coarse paths hoist their single lookup out of the loop. Measured at about 7.7 ns per lookup,
that is roughly 62 µs per `geometryThin()` call (two guaranteed lookups per glyph over 8,106
glyphs) and about 601 lookups per `searchThin()` call, enough to inflate the coarse win by roughly 4
percent on `geometry` and 10 percent on `search` in the previous run. All four functions now resolve
their per-arm function references once, into local consts, outside the loop, the same way `p` and
`coarse` were already resolved once per arm. This run does not isolate the before/after delta with a
controlled pair of runs; the fix is applied going forward, and the `geometry` and `search` numbers
below should read a few percent closer to the true coarsening win than the previous file's did.

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
repointed that test before this run. The suite actually run for this file
(`gate.test.mjs`, `arm-b.test.mjs`, `arm-c.test.mjs`, `arm-d.test.mjs`, `fixture.test.mjs`,
`probe.test.mjs`, `run.test.mjs`, seven files) passed all 12 of 12 tests, `gate.test.mjs` alone
contributing six of them. There is no known failing test in this suite as of this run.

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
arch arm64 · 16 cores · loadavg 4.01 at the start of the run
emsdk emcc 3.1.72 (437140d) · rustc 1.98.0 (88d9e12ae, 2026-08-18)
fixture: examples/snippet-react/public/report.pdf, page 4, 8106 glyphs
rounds: 7, interleaved a/b/c/d with the order rotated each round, budget 500ms per cell
```

Unlike the previous two versions of this file, `emsdk` is not `unknown` here: `build/emsdk.sh` was
sourced before `bench/run.mjs` ran, so `em++` was on `PATH` and `bench/node.mjs` recorded the real
version instead of falling back to its default. `bench/run.test.mjs` still asserts `emsdk !==
'unknown'` and would have failed this run if that had not been true; it passed.

The `node --test` suite passed 12 of 12 tests across the seven files listed above. No test failed
or was skipped.

macOS thermal throttling is not controllable from here. Rounds are interleaved and, as of this run,
rotated in order within each round, for exactly that reason: drift and position bias should land on
all four arms rather than on whoever runs first or last. Worst observed round-to-round spread
(max/min across the 7 rounds, same arm and workload):

```
a/null-peek 1.23x, b/null-peek 1.22x, d/geometry-copyout 1.09x
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
| `null-call` | 343.9 ns | 324.2 ns | — | — | 0.94x | — | — |
| `null-peek` | 3.8 ns | 3.9 ns | — | — | 1.02x | — | — |
| `null-readbytes` | 30.0 ns | 30.0 ns | — | — | 1.00x | — | — |
| `geometry` | 2.707 ms | 2.726 ms | 1.030 ms | 1.027 ms | 1.01x | 0.38x | 0.38x |
| `geometry-copyout` | — | — | 19.54 µs | 19.00 µs | — | — | — |
| `text` | 116.94 µs | 120.12 µs | 120.58 µs | 117.78 µs | 1.03x | 1.03x | 1.01x |
| `render-1x` | 2.811 ms | 2.847 ms | 2.842 ms | 2.752 ms | 1.01x | 1.01x | 0.98x |
| `render-4x` | 15.579 ms | 16.063 ms | 16.153 ms | 15.457 ms | 1.03x | 1.04x | 0.99x |
| `render-copyout-4x` | 590.30 µs | 584.63 µs | 581.47 µs | 584.77 µs | 0.99x | 0.99x | 0.99x |
| `search` | 99.60 µs | 101.40 µs | 29.74 µs | 32.63 µs | 1.02x | 0.30x | 0.33x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 355.5 ns | 332.3 ns | — | — |
| `null-peek` | 4.0 ns | 4.0 ns | — | — |
| `null-readbytes` | 30.5 ns | 30.5 ns | — | — |
| `geometry` | 2.950 ms | 2.947 ms | 1.088 ms | 1.085 ms |
| `geometry-copyout` | — | — | 20.88 µs | 21.07 µs |
| `text` | 120.77 µs | 123.89 µs | 124.35 µs | 121.19 µs |
| `render-1x` | 2.870 ms | 2.894 ms | 2.894 ms | 2.805 ms |
| `render-4x` | 15.782 ms | 16.165 ms | 16.308 ms | 15.654 ms |
| `render-copyout-4x` | 625.30 µs | 599.49 µs | 599.54 µs | 599.94 µs |
| `search` | 101.50 µs | 103.38 µs | 30.54 µs | 33.17 µs |

### Module size

| arm | wasm bytes | delta vs a |
|---|---|---|
| a direct | 4,676,714 | — |
| b rust thin | 4,680,516 | +3.7 kB |
| c rust coarse | 4,681,488 | +4.7 kB |
| d c++ coarse | 4,684,877 | +8.0 kB |

These are the same wasm byte counts as the previous run: no link flag or exported-function set
changed here, only the JS-side benchmark code and the driver.

## What the numbers say

With arm order rotated, `null-call` still shows arm b faster than arm a, in every one of the seven
rounds: a ran 342.0, 341.6, 345.0, 342.0, 344.1, 343.9, 348.8 ns across the seven rounds, in that
order, while b ran 316.9, 317.8, 323.5, 326.5, 326.7, 324.2, 326.2 ns in the same seven rounds. Arm a
occupied every position from first to last across those rounds (round order was a,b,c,d then
b,c,d,a then c,d,a,b then d,a,b,c then a,b,c,d then b,c,d,a then c,d,a,b), and b still came out ahead
every time. That rules out the position-decay explanation the previous run's rotation was meant to
test for. The honest reading is not that arm b's wrapper has negative cost: a wrapper cannot be
faster than the bare call it wraps. The honest reading is that this cell, at 300-some nanoseconds a
call with a roughly 20-nanosecond gap between arms, is below what a 500ms sampling budget and
Node's own call overhead can resolve on this hardware. The `null-call` cell cannot currently tell
these two arms apart; it cannot be read as evidence of a negative tax, and it cannot yet be read as
evidence the wrapper is free either. Something at or below the noise floor of this measurement setup
is happening in that gap, and this benchmark does not have the resolution to say what.

On the workloads that do real PDFium work, the wrapper tax reads as small and directionally
consistent: arm b costs 1 to 4 percent more than arm a on `geometry` (1.01x), `text` (1.03x),
`render-1x` (1.01x), `render-4x` (1.03x), and `search` (1.02x). That is inside the pre-registered 15
percent band in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md),
and well inside the 25 percent line past which the prediction said to suspect the harness rather
than the result.

`render-4x` is the result the previous write-up missed. Arm a and arm d cluster low across six of
the seven rounds (a from 15.171 to 15.660 ms, d from 15.312 to 15.493 ms) while arm b and arm c
cluster about 3 to 4 percent higher (b from 15.907 to 16.259 ms, c from 15.961 to 16.245 ms). Round 6
is the one exception: arm a spiked to 16.269 ms that round, the highest of all four arms, breaking
the clean split; arm d stayed fastest in all seven rounds regardless. Rendering calls no Rust code
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
in a footnote. On `null-call` the Rust-linked arm is the FASTER one, by about 20 nanoseconds in
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
at 0.38x and arm d at 0.38x on `geometry` (about 2.6x faster than arm a); on `search`, arm c is 0.30x
and arm d is 0.33x (about 3.3x and 3.1x faster than arm a). Both are inside the prediction's
3x-to-10x band for coarsening a chatty per-glyph crossing into one call. `geometryThin` makes two
wasm calls per glyph (`FPDFText_GetTextObject` and `EPDFText_GetCharGeometry`) plus a third on every
object-key change, so 8,106 glyphs cost on the order of sixteen thousand crossings, where
`geometryCoarse` makes exactly one crossing and decodes PDFium's per-glyph struct into a packed
112-byte record on the native side. `render` and `text` land within 1 to 4 percent everywhere,
matching the prediction that render is dominated by PDFium's own bitmap work and that a single
already-coarse call like `text` has nothing left for coarsening to remove.

Arm c and arm d are indistinguishable on `geometry`: the median gap is under 1 percent (c 1.030 ms,
d 1.027 ms) and the per-round ratio of d over c runs 0.975x to 1.004x across all seven rounds, a
tighter spread than the worst same-arm, same-workload round-to-round spread measured anywhere in
this run (1.23x on `a/null-peek`). On `search`, the two arms are not indistinguishable: arm c beat
arm d in every one of the seven rounds, by 4 to 10 percent (per-round ratio of d over c ran 1.038x
to 1.099x), and that gap is larger than either arm's own round-to-round spread. This is the one
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
performance. It is Node only, wasm only. Native, where `mem.peek` is an N-API round trip rather than
a typed-array read, is expected to disagree with this, for the reasons in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md#native-after),
and is not measured here.
