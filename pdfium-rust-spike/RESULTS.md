# Results: four PDFium wrapper arms, wasm, seven rounds

The fairness gate ran before any timing, over both gate fixtures (`report-p4` and `rotated-p0`),
and passed:

```
fairness gate: a, b, c, d agree on report-p4 (8106 glyphs), rotated-p0 (31 glyphs), 5 workloads compared
```

All four arms agree on every canonical answer they are declared to produce, on both fixtures. The
numbers below are real per-operation cost, not an artifact of one arm quietly computing the wrong
thing.

## This is a re-run, and what moved between the two

Commit `1566b1fb` is the reason for it, and two of its four fixes reach this directory. One lands
in a timed path: `geometryCoarse` and `searchCoarse` in `bench/workloads.mjs` share one `fits()`
guard now, so every coarse call pays one extra JS call and one comparison. The other is the change
that guard exists for, `rs_read_page_geometry` and `cc_read_page_geometry` returning `-count` when
the glyphs do not fit in `cap` rather than the unclamped count, and that one shows up in the module
sizes: arm c links 1,134 bytes larger than the previous sitting, arm d 1,119 bytes larger, and arms
a and b are byte-identical.

One more change lands with this re-run rather than with that commit. `crates/napi-arms/build.rs`
defaults `PDFIUM_LIB_DIR` to the directory `build/fetch.sh` writes instead of requiring it, so
`cargo test` at the workspace root builds instead of dying in a build script. It compiles no
differently and moves no number here.

Both halves were run together, in one sitting, on the same driver, because quoting a wasm run and
a native run from two different sittings under one set of tables invites exactly the cross-sitting
comparison neither supports. `bench/results/` holds all 56 files these numbers are drawn from,
`<arm>-r<round>.json` and `native-<arm>-r<round>.json`, seven rounds each for arms a, b, c, d,
committed alongside this file rather than gitignored. They are committed because `run.mjs` used to
clear its whole results directory, so the first `--native` run after a wasm run deleted the wasm
set; that bug is fixed and the git history on this file carries the detail.

## Six findings fixed before the previous run, still in effect

A review of an earlier version of this file and its driver found six ways an arm could be doing
different work for a reason unrelated to the wrapper under test. All six were fixed before the
previous sitting, none of them has been touched since, and they are in effect in the numbers
below.

**Arm order was never rotated.** `bench/run.mjs` ran every round in the same order, a, b, c, d.
Interleaving rounds corrects clock drift across a multi-minute run, but it does nothing about
position within a round, and the environment stamps showed background load decaying inside every
round: whichever arm ran first paid the highest load, whichever ran last paid the lowest. The
unrotated run's `null-call` cell showed arm b, a strict wrapper around the exact call arm a makes,
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
coarse win by roughly 4 percent on `geometry` and 10 percent on `search` in the run that had it. The
four thin functions now resolve their per-arm function references once, into local consts, outside
the loop, the same way `p` and `coarse` were already resolved once per arm. No run has isolated the
before/after delta with a controlled pair; the fix is simply applied from that sitting onward.

The hoist reaches the thin side only. `geometryCoarse`, `textCoarse`, `renderCoarse`, and
`searchCoarse` still rebuild the same kind of template string and do a dynamic property lookup
inside their timed body, once per call rather than once per glyph or hit, because a coarse call
crosses once instead of looping per record. One lookup against a 563 µs to 2.8 ms call is immaterial
on its own, but it means this fix equalized which side pays a per-call cost, not which side pays
none at all.

**`searchCoarse` broke the rule `geometryCoarse` follows.** `searchCoarse` read its result buffer
with `mem.readBytes`, a `HEAPU8.slice` copy, while `geometryCoarse` deliberately reads through a
live-heap `DataView` and carries a comment explaining why (Finding 6).
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
repointed that test. For this sitting, `node --test bench/*.test.mjs` passed all 18 of 18 tests
across ten files, `gate.test.mjs` alone contributing six of them and `overflow.test.mjs`, new in
`1566b1fb`, four. There is no known failing test in this suite as of this run. `run.test.mjs`
needs `em++` on `PATH` to pass its `emsdk` assertion; without it that one test fails on the
environment stamp rather than on anything it measures.

## Earlier bugs already fixed, unaffected by this run

Earlier sittings corrected an unoptimized-link bug (arm d was built at `-O0`
against arm c's Rust at `-O3`, producing a fake 25-to-33-percent `geometry` gap that was a compiler
flag, not a language difference) and a decode-allocation bug (`geometryCoarse` allocated four
throwaway arrays and closures per glyph where `geometryThin` did not). Both fixes are unchanged by
this run's changes and are still in effect; see the git history on this file for the detail that was
here before. `build/link-wasm.sh` still links all four arms at `-O3` from the one shared `link()`
body, with the substituted Homebrew `wasm-opt` 132 standing in for the emsdk 3.1.72 build's own
`wasm-opt`, which predates the two wasm feature names rustc 1.98's precompiled std emits. That
substitution is a local build-environment workaround, not a committed change, and it is what
produced the arm b, c, and d binaries measured below.

## Environment

```
Node v24.20.0 · Apple M4 Max · darwin 25.6.0
arch arm64 · 16 cores · loadavg 6.12 to 15.52 across the seven rounds
emsdk emcc 3.1.72 (437140d) · rustc 1.98.0 (88d9e12ae, 2026-08-18)
fixture: examples/snippet-react/public/report.pdf, page 4, 8106 glyphs
rounds: 7, interleaved a/b/c/d with the order rotated each round, budget 500ms per cell
```

`emsdk` is not `unknown` here: `build/emsdk.sh` was sourced before `bench/run.mjs` ran, so `em++`
was on `PATH` and `bench/node.mjs` recorded the real version instead of falling back to its
default.

The `node --test` suite passed 18 of 18 tests across ten files. No test failed or was skipped.

This sitting ran busier than the previous one, which reported loadavg 4.31 at its start. Loadavg
sampled from the first arm in each round ran 6.12, 10.20, 8.41, 15.52, 12.38, 13.60, 10.74, with no
trend. Absolute numbers moved a few percent against the previous sitting as a result, `geometry`
and `geometry-copyout` most visibly. So where this file looks back at the previous sitting it
compares ratios and findings, never a cell in nanoseconds; every timing comparison is within this
sitting.

macOS thermal throttling is not controllable from here. Rounds are interleaved and rotated in order
within each round, for exactly that reason: drift and position bias should land on all four arms
rather than on whoever runs first or last. Worst observed round-to-round spread (max/min across the
7 rounds, same arm and workload):

```
b/null-peek 1.21x, c/geometry-copyout 1.18x, d/geometry-copyout 1.17x
```

`null-peek` measures a single `getValue` call, around 4 nanoseconds; a 1.21x spread there is a
sub-nanosecond absolute wobble that a 500ms sampling budget cannot fully average out at that scale.
Arm b's seven `null-peek` values run 4.0, 3.7, 3.4, 3.4, 4.0, 3.3, 4.0 ns, which is what that
looks like. `bench/compare.mjs` treats spread past about 1.3x as a thermal-drift signal; the
largest spread here sits at 1.21x, comfortably under that line, so nothing here signals throttling
distorted the comparisons that matter.

A note on what the numbers mean: `harness.mjs` measures in batches (auto-tuned so one batch takes at
least 2ms), and reports the median and p95 of those batch means, not of individual calls. The p95
column below is the p95 of batch means: it shows run-to-run variance within a workload, not
per-call tail latency, and should not be read as a worst-case frame cost.

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
| b rust thin | 4,680,516 | +3.7 kB |
| c rust coarse | 4,682,622 | +5.8 kB |
| d c++ coarse | 4,685,996 | +9.1 kB |

Arms a and b are byte-identical to the previous run. Arms c and d are not: c links 1,134 bytes
larger and d 1,119 bytes larger, which is `rs_read_page_geometry` and `cc_read_page_geometry`
learning to return `-count` on overflow. No link flag or exported-function set changed.

## What the numbers say

With arm order rotated, `null-call` still shows arm b faster than arm a, in every one of the seven
rounds: a ran 353.2, 348.3, 356.4, 358.3, 350.4, 350.9, 351.2 ns across the seven rounds, in that
order, while b ran 324.5, 327.3, 336.8, 334.1, 328.8, 339.5, 329.4 ns in the same seven rounds. Arm a
occupied every position from first to last across those rounds (round order was a,b,c,d then
b,c,d,a then c,d,a,b then d,a,b,c then a,b,c,d then b,c,d,a then c,d,a,b), and b still came out ahead
every time. That rules out the position-decay explanation rotation was introduced to test for, and
it is the second sitting in a row to rule it out. The honest reading is not that arm b's wrapper has
negative cost: a wrapper cannot be faster than the bare call it wraps. The honest reading is that
this cell, at 300-some nanoseconds a call with a roughly 11-to-29-nanosecond gap between arms, is
below what a 500ms sampling budget and Node's own call overhead can resolve on this hardware. The
`null-call` cell cannot currently tell these two arms apart; it cannot be read as evidence of a
negative tax, and it cannot yet be read as evidence the wrapper is free either. Something at or
below the noise floor of this measurement setup is happening in that gap, and this benchmark does
not have the resolution to say what.

On the workloads that do real PDFium work, the wrapper tax reads as small and directionally
consistent: arm b costs 1 to 4 percent more than arm a on `geometry` (1.02x), `text` (1.01x),
`render-1x` (1.01x), `render-4x` (1.04x), and `search` (1.02x). That is inside the pre-registered 15
percent band in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md),
and well inside the 25 percent line past which the prediction said to suspect the harness rather
than the result.

The `render-1x` and `render-4x` numbers are a half dose of that tax.
`crates/shim/src/lib.rs` exports no `rs_FPDFBitmap_FillRect`, so `renderThin` calls the unprefixed
`fn.FPDFBitmap_FillRect` on every arm, arm b included, rather than going through arm b's shim.
`renderThin` makes two PDFium calls per render, `FPDFBitmap_FillRect` and the actual render call, and
arm b only pays its one extra wasm frame on the second. So the 1.01x and 1.04x quoted above for
`render-1x` and `render-4x` price one shim frame out of two calls, not two out of two: the true
per-call wrapper tax on a fully-shimmed render is conservatively higher than what these two cells
report. Not fixed here, because adding the missing shim function would require a rebuild and a
re-run.

`render-4x` splits the four arms into two pairs, and this sitting splits them cleanly. Arms a and d
run low in all seven rounds (a from 15.206 to 16.417 ms, d from 15.814 to 16.345 ms) while arms b
and c run high in all seven (b from 16.541 to 17.277 ms, c from 16.714 to 17.436 ms). The two
groups do not overlap at any point: the slowest a-or-d round, 16.417 ms, is still faster than the
fastest b-or-c round, 16.541 ms. Within the low pair, a is fastest in rounds 1 through 4 and d in
rounds 5 through 7, which is the only part of this that looks like ordinary noise. The previous
sitting saw the same split in six rounds of seven, with round 6 inverting it outright; this one has
no exception round. Rendering calls no Rust code
on any arm's timed path (arm b's `rs_FPDF_RenderPageBitmap` is a 1:1 shim around the same PDFium
call arm a makes, and arm d's `cc_render_page` is C++), so this is not a coarsening effect and not a
wrapper-call effect. Arms b and c are the two arms with a Rust static archive (`libshim.a`,
`libops.a`) linked into the module; arms a and d are not. The pattern that survived rotation is
consistent with linking a Rust archive into the module costing something on this workload,
independent of whether any Rust code in that archive is ever called, rather than with anything to
do with coarsening the crossing. Two sittings agreeing, one of them with no exception round, is
better evidence than the previous write-up had, and it is still only a correlation: this benchmark
was not built to isolate what about the link produces the effect, and code layout, page cache
behavior, and how Binaryen's `-O3` pass treats the extra section are all consistent with what is
measured here. The finding is the correlation with which arms link a Rust archive, not a mechanism.

There is evidence in this same run that cuts against that reading, and it belongs here rather than
in a footnote. On `null-call` the Rust-linked arm is the FASTER one, by 11 to 29 nanoseconds in
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
and arm d is 0.31x (about 3.3x and 3.2x faster than arm a). Search landed inside the prediction's
3x-to-10x band for coarsening a chatty per-glyph crossing into one call; geometry, at 2.6x, landed
just under it. `geometryThin` makes two
wasm calls per glyph (`FPDFText_GetTextObject` and `EPDFText_GetCharGeometry`) plus a third on every
object-key change, so 8,106 glyphs cost on the order of sixteen thousand crossings, where
`geometryCoarse` makes exactly one crossing and decodes PDFium's per-glyph struct into a packed
112-byte record on the native side. `render` and `text` land within 1 to 4 percent everywhere,
matching the prediction that render is dominated by PDFium's own bitmap work and that a single
already-coarse call like `text` has nothing left for coarsening to remove.

Arm c and arm d are indistinguishable on `geometry`: the median gap is a little over one percent
(c 1.070 ms, d 1.057 ms, d the faster of the two) and the per-round ratio of d over c runs 0.962x to
1.048x across all seven rounds, a tighter spread than the worst same-arm, same-workload
round-to-round spread measured anywhere in this run (1.21x on `b/null-peek`). On `search`, the two
arms are close to indistinguishable but not quite: arm c beat arm d in all seven rounds this time,
by 0.1 to 7.4 percent (per-round ratio of d over c ran 1.001x to 1.074x). The previous sitting saw
the same direction in six rounds of seven, by 4 to 9 percent, so the gap reproduced but shrank. It
sits at the edge of what this cell can resolve: arm c's own round-to-round spread on `search` is
1.04x, which covers most of the range the gap moves in. This is the one
place in this benchmark where a repeatable difference between the Rust and C++
implementations of the same coarse operation shows up. It is a difference between
`rs_search_page` and `cc_search_page` specifically, on this fixture's 120 search
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
throwaway array literal and closure per hit, 120 per call on this fixture. That is the same
class of allocation bug Finding 6 already fixed elsewhere, and it is left in place rather
than fixed, because fixing it would change a published number, and re-running the benchmark to
correct it is not worth a change that can only improve the coarse arm's own result. So the published
`search` coarsening figures, 3.3x and 3.2x, understate the true win slightly.

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

Loadavg, sampled from the first arm scheduled in each round's rotation, ran 7.58, 7.61, 9.37, 7.00,
5.87, 4.98, 11.01 across the seven rounds, with no trend in either direction and a spike in the
last one. That rules nothing in or out on its own; the round-to-round pattern on one workload below
is the direct evidence, and is discussed there on its own terms rather than argued from loadavg.

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

There is no wasm-byte-size table here; `bench/compare.mjs --native` skips it, since `wasmBytes` is
null on every native result file.

Worst round-to-round spread (max/min across the seven rounds, same arm and workload):

```
c/render-copyout-4x 1.75x, a/render-copyout-4x 1.31x, d/render-copyout-4x 1.14x
```

`bench/compare.mjs` treats a spread past about 1.3x as a thermal-drift signal, and two of the four
arms cross it on `render-copyout-4x`. The shape does not match gradual throttling: it is round 1,
and almost only round 1. Arm c spends 4.243 ms there against 2.426 to 2.859 ms in rounds 2 through
7, and arm a spends 2.172 ms against 1.663 to 1.758 ms. Arm d peaks in round 1 too, more mildly
(2.773 ms against 2.424 to 2.749 ms), and arm b does not peak there at all, running 2.478 ms in
round 1 against a 2.342 to 2.506 ms range that contains it. Drop round 1 and the whole cell falls
back in line: 1.06x for a, 1.07x for b, 1.18x for c, 1.13x for d, every one of them under the
compare.mjs line. `render-copyout-4x` copies the full 4x bitmap out with `mem.readBytes`, roughly
31 MB per call, the single largest copy in this benchmark, and a transient that large in the first
round and gone by the second reads as a cold-cache or first-large-allocation effect rather than as
drift, which would grow across rounds and hit every arm together. This is a cleaner case than the
previous sitting could make, where excluding round 1 still left arm c at 1.32x. Treat this cell's
absolute numbers as the noisiest in this document, and treat the `render-copyout-4x` finding below
(arm a diverging from b, c, and d) as the only thing this cell is used to support.

### The a-versus-b confound

Native arm a is `node-addon-api` C++ taking BigInt pointers; native arm b is `napi-rs` Rust taking
`f64` pointers. An a-versus-b gap here mixes three things that changed at once: the language, the
binding framework generating the N-API glue, and the pointer representation crossing the boundary on
every argument. The wasm run could report a wrapper tax for arm b (1 to 4 percent, see "What the
numbers say" above) because all four wasm arms share one Emscripten export surface; nothing else
changed. That precondition does not hold here, and neither `b/a` nor any ratio computed against arm
a in the table above should be read as a native wrapper tax. Reporting `b/a`'s 0.96x on `null-call`
as "the Rust wrapper is 4 percent cheaper natively" would repeat the exact mistake `null-call`'s wasm
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
`geometry` arm a takes 8.642 ms and arm b 4.134 ms for the same PDFium calls in the same order, and
on `null-peek` 39.2 ns against 15.0 ns. Whatever the cause, something in the shipping addon's
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

Held within one language and framework, thin-to-coarse coarsening wins 7.16x on this fixture (arm b
at 4.134 ms against arm c at 577.37 µs, both `napi-rs`) and 14.87x held within C++ (arm a at 8.642 ms
against arm d at 581.16 µs, both `node-addon-api`, modulo the BigInt caveat above, which likely
inflates arm a's absolute cost and therefore this second number somewhat). Either way, this is 2.7x
to 5.6x larger than wasm's `geometry` win of about 2.6x. The direction and the rough size both match
the prediction. Both figures land within 0.5 percent of the previous sitting's 7.19x and 14.85x,
which is the strongest reproduction anywhere in this document.

The mechanism is visible in the null-probe cells, but it is not the same peek for both pairings.
Arm a's `mem.peek32` costs 39.2 ns natively against 4.0 ns in wasm, roughly 9.8x, and that is the
pairing behind the 14.87x figure. Arm b's `mem.peek32` costs 15.0 ns natively against a wasm median
of 3.7 ns, roughly 4x, and that is the pairing behind 7.16x. Take the 4x loosely: arm b's wasm
`null-peek` carries the worst round-to-round spread in the whole wasm run at 1.21x, so the multiple
is somewhere between 3.7x and 4.1x depending on which round you read. Ten times is close to the
right multiple for arm a's peek; four times is the multiple for arm b's.

Arm a's peek is expensive for reasons that have nothing to do with coarsening. `bench/arm.mjs:243`
calls `raw.peek(P(p), 'i32')`, converting the pointer to a BigInt through `P()` and dispatching on a
JavaScript string type tag on every call, the same shape as the shipping addon's `peek(ptr, kind)`
at `packages/engine/runtime/src/native/native-runtime.ts:57`. `readGlyphRaw` (`bench/workloads.mjs`)
makes up to 21 such peeks per glyph, on top of the object-key and font-size calls every glyph pays;
over 8,106 glyphs that call happens on the order of 170,000 times in one `geometry` iteration, and
`rs_read_page_geometry` collapses all of it into one crossing. So the 14.87x figure prices
string-tag dispatch, BigInt marshalling, and node-addon-api's own per-call cost together, not
"coarsening held within C++" by itself; arm d's coarse call never goes through arm a's peek path, so
none of those three costs is shared between the two arms being compared. That is three distinct
effects bundled into one number, not one effect at a larger scale. The 7.16x figure is the more
trustworthy of the two: arm b's peek is a direct `f64` call into `napi-rs` with no string dispatch
and no BigInt conversion, so it avoids all three.

`geometry-copyout` gives one more data point for the same story. It isolates the cost of a full
memcpy readback of the packed record buffer (908 KB): 71.31 µs (c) and 72.90 µs (d) natively against
22.39 µs and 21.72 µs in wasm, roughly 3.2x and 3.4x the cost for the same number of bytes.
`view_buffer`'s
zero-copy design (Fix 1 above) is worth more here than it was in wasm, precisely because the memcpy
it avoids costs more here than it did there.

### Search: the prediction did not hold

The wasm write-up's search win was 3.2x to 3.3x. Held within one language and framework, native
search wins only 1.62x (b at 64.48 µs, c at 39.84 µs, both `napi-rs`) and 2.06x held within C++ (a at
82.19 µs, d at 39.99 µs). Both are smaller than wasm's, not larger. This is the one place in this run
where "native's coarsening win is bigger" does not hold, and it should be reported as a real result,
not smoothed over to match the geometry finding or the pre-registered prediction.

The likely reason is visible in the same null-probe cells that explain `geometry`. `searchThin`
(`bench/workloads.mjs`) is dominated by function calls, not peeks: `FPDFText_FindNext`,
`GetSchResultIndex`, `GetSchCount`, `CountRects`, and `GetRect` per hit, 602 PDFium calls against
120 hit rects on this fixture, with only four `peekF64` calls per matched rect. `null-call` costs
200.3 ns natively against 351.2 ns in wasm for arm a, a native call costing a bit over half (0.57x)
a wasm one on this machine. `search`'s thin loop is call-bound rather than peek-bound, and the operation
being
collapsed away was already cheaper natively than it was in wasm, so there is less to win by
collapsing it. This is inferred from the shape of the two loops and the null-probe numbers, not
measured directly cell for cell, and it would take a dedicated per-call-type breakdown to confirm.

### Arms c and d: still indistinguishable, and more so than in wasm

`geometry`'s per-round ratio of d over c runs 0.993x to 1.034x across the seven rounds, tighter than
this run's own worst same-arm spread (1.75x on `c/render-copyout-4x`, discussed above, or 1.18x on
that same cell once round 1 is dropped). `search`'s per-round ratio runs 0.978x to 1.025x, with no
consistent direction: c beats d in five of the seven rounds and d beats c in two, and the largest
gap either way is 2.5 percent. That is a change from wasm, where arm c beat arm d in all seven
rounds on `search`, by 0.1 to 7.4 percent, the one place the wasm write-up found a language
difference showing up. That gap is gone natively.
Whether it went away because the operation changed (a native coarse call does more per crossing: it
also builds the zero-copy `ArrayBuffer` view, see the caveat below) or because it was never a
language effect to begin with and wasm's Binaryen `-O3` pass treated the two static libraries
differently is not something this run can separate; the honest statement is that the wasm-only
finding did not reproduce here.

`text`, `render-1x`, and `render-4x` land within 2 percent across all four arms, `render-4x`'s
1.9 percent spread between arm a and arm d being the widest of the three, matching the
prediction that rendering is dominated by PDFium's own bitmap work and that an already-coarse call
like `text` has nothing left for coarsening to remove, the same result wasm found.

`render-copyout-4x` is the one workload where arm a and arms b/c/d visibly diverge (1.706 ms against
2.398-2.647 ms; these medians include the round-1 transient discussed above, which affects three of
the four arms but does not change which side of the gap either one falls on), and it
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
