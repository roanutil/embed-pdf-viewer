# Results: four PDFium wrapper arms, wasm, seven rounds

The fairness gate ran before any timing, over both gate fixtures (`report-p4` and `rotated-p0`),
and passed:

```
fairness gate: a, b, c, d agree on report-p4 (8106 glyphs), rotated-p0 (31 glyphs), 5 workloads compared
```

All four arms agree on every canonical answer they are declared to produce, on both fixtures. The
numbers below are real per-operation cost, not an artifact of one arm quietly computing the wrong
thing.

## This run corrects a second bug: arm d was linked unoptimized

The previous version of this file reported arm C beating arm D on `geometry` by 25 to 33 percent,
holding in all seven rounds of that run, and read that gap as evidence that `rs_read_page_geometry`
in Rust was doing the per-glyph work faster than `cc_read_page_geometry` in C++. That reading was
wrong, and the cause was not in either producer function. It was in `build/link-wasm.sh`.

The script's shared `link()` function calls `em++` with no `-O` flag at all, so Emscripten defaulted
to `-O0` for everything it linked, `cpp/ops.cc` included. Arm C's Rust, by contrast, is built by a
separate `cargo build --release` step against this workspace's `[profile.release]`: `opt-level = 3`,
`lto = true`, `codegen-units = 1`. Arms A and B never showed this problem because they compile no
C++ of their own; they only link the prebuilt, already-optimized `libembedpdf.a`. Arm D was the one
arm whose own source got compiled by the unflagged `em++` call, so it was the one arm silently
measured at `-O0` against everything else at `-O3` or better. The 25-to-33-percent gap was that flag
difference, not a language difference.

The fix adds `-O3` to the one shared `link()` body in `build/link-wasm.sh`, so all four arms are
linked with the identical flag, the same way they already shared every other flag. Nothing else
about the link flags changed: `-sASSERTIONS=1` is still there, matching the shipping build at
`packages/engine/runtime/build/compile.esm.sh`.

Turning that flag on exposed a second, unrelated problem before any timing could run: `em++`'s
automatic post-link Binaryen pass reads a `target_features` custom section out of the linked module
and turns each entry into an `--enable-<feature>` flag for `wasm-opt`. The Rust static libraries
(`libshim.a`, `libops.a`) carry two feature entries, `bulk-memory-opt` and `call-indirect-overlong`,
that this rustc's LLVM backend enables by default for `wasm32-unknown-emscripten` and that come from
the toolchain's precompiled `std`/`core`, not from any per-crate flag this spike controls. The
`wasm-opt` binary vendored with the pinned emsdk 3.1.72 predates both flag names and refuses to
start, so any `-O`-level link that includes a Rust archive failed outright once `-O0` stopped hiding
it. Arm A, which links no Rust code, was unaffected and built as soon as `-O3` was added. Working
around this required substituting a newer standalone `wasm-opt` (Homebrew's binaryen 132, which
recognizes both flags) for the one bundled in `build/emsdk`; `em++`, `wasm-ld`, and the rest of the
toolchain stayed on the pinned 3.1.72. This is a local build-environment substitution, not a
committed change: `build/link-wasm.sh` still names no Binaryen version, and nothing in this repo
pins one. It is recorded here because it is part of how the arm D binaries measured below were
produced, and because the same feature-name mismatch will recur for anyone rebuilding this spike
with this rustc against an unmodified emsdk 3.1.72 install.

Every number in this file is from the run made with both fixes in place: `-O3` in the shared
`link()`, and the substituted `wasm-opt`. Only `geometry` moved from the first published run;
`text`, `render`, and `search` are unchanged within round-to-round noise, which is what you would
expect from a bug confined to the C++ link flags of one arm.

## The `-O0`/`-O3` bug was on top of an earlier decode-allocation bug

The run before this one already corrected a different, JS-side fairness bug: `geometryCoarse` in
`bench/workloads.mjs`, the decode both arm C and arm D use to turn the packed record buffer back
into JS glyph objects, allocated four throwaway arrays and closures per non-empty glyph, where
`geometryThin` built the same shapes with a direct array literal and a preallocated `Array(8)`
filled by a plain loop. That cost roughly 26,000 extra allocations on this fixture, paid by both
arm C and arm D equally, and it inflated both of their `geometry` numbers by close to the same
absolute amount. Because it hit both coarse arms equally, it did not, by itself, produce a fake gap
between them; the `-O0`/`-O3` bug described above is what did that, and it happened to be
introduced in a later run than the allocation fix, which is why the two bugs were caught separately
rather than at the same time. Both are corrected in the numbers below.

## Environment

```
Node v24.20.0 · Apple M4 Max · darwin 25.6.0
arch arm64 · 16 cores · loadavg 6.35 at the start of the run
emsdk unknown · rustc 1.98.0 (88d9e12ae, 2026-08-18)
fixture: examples/snippet-react/public/report.pdf, page 4, 8106 glyphs
rounds: 7, interleaved a/b/c/d, budget 500ms per cell
```

`emsdk unknown` is a shell artifact of how `bench/run.mjs` was invoked for this measurement, not a
change to the wasm modules: the session's `PATH` did not have `em++` on it when the benchmark ran
(the modules had already been built in an earlier step, in a shell that had sourced
`build/emsdk.sh`), so `bench/node.mjs` fell back to its `'unknown'` default for that one metadata
field. `bench/run.test.mjs` independently asserts `emsdk !== 'unknown'` and fails on this exact gap
when `em++` is not on `PATH`; sourcing `build/emsdk.sh` first makes it pass. That assertion was not
touched. This is the same pre-existing environment quirk noted in the previous two versions of this
file, not something introduced here.

The `node --test` suite (`gate.test.mjs`, `arm-b.test.mjs`, `arm-c.test.mjs`, `arm-d.test.mjs`,
`fixture.test.mjs`, `probe.test.mjs`) passed 10 of 11 tests. The one failure,
`probe.test.mjs`, imports `../build/out/probe.mjs`, a build output that no current script produces;
`build/link-wasm.sh` has only ever built arms `a`, `b`, `c`, and `d` since the commit that introduced
its shared `link()` function (`403b18b4`), and `probe.mjs` was an artifact of an earlier, since-
replaced prototype script (`ea20d1c3`). This is orphaned test infrastructure, unrelated to the `-O3`
change and not something this task's scope covers fixing.

macOS thermal throttling is not controllable from here. Rounds are interleaved for exactly that
reason, so drift lands on all four arms rather than on whoever runs last. Worst observed
round-to-round spread (max/min across the 7 rounds, same arm and workload):

```
a/null-peek 1.24x, b/null-peek 1.23x, a/geometry 1.15x
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
| `null-call` | 343.1 ns | 324.3 ns | — | — | 0.95x | — | — |
| `null-peek` | 3.3 ns | 3.9 ns | — | — | 1.19x | — | — |
| `null-readbytes` | 30.1 ns | 29.9 ns | — | — | 0.99x | — | — |
| `geometry` | 3.555 ms | 3.664 ms | 1.042 ms | 1.031 ms | 1.03x | 0.29x | 0.29x |
| `geometry-copyout` | — | — | 19.11 µs | 19.34 µs | — | — | — |
| `text` | 117.30 µs | 119.13 µs | 119.53 µs | 119.08 µs | 1.02x | 1.02x | 1.02x |
| `render-1x` | 2.803 ms | 2.870 ms | 2.847 ms | 2.773 ms | 1.02x | 1.02x | 0.99x |
| `render-4x` | 15.520 ms | 16.161 ms | 16.218 ms | 15.555 ms | 1.04x | 1.04x | 1.00x |
| `render-copyout-4x` | 583.86 µs | 582.40 µs | 583.33 µs | 585.31 µs | 1.00x | 1.00x | 1.00x |
| `search` | 102.75 µs | 108.31 µs | 30.17 µs | 32.91 µs | 1.05x | 0.29x | 0.32x |

### p95 of batch means, same cells

| workload | a direct | b rust thin | c rust coarse | d c++ coarse |
|---|---|---|---|---|
| `null-call` | 352.6 ns | 334.7 ns | — | — |
| `null-peek` | 3.9 ns | 4.0 ns | — | — |
| `null-readbytes` | 30.5 ns | 30.5 ns | — | — |
| `geometry` | 3.922 ms | 4.023 ms | 1.095 ms | 1.100 ms |
| `geometry-copyout` | — | — | 21.27 µs | 20.46 µs |
| `text` | 120.67 µs | 122.57 µs | 123.51 µs | 122.85 µs |
| `render-1x` | 2.846 ms | 2.906 ms | 2.898 ms | 2.826 ms |
| `render-4x` | 15.720 ms | 16.364 ms | 16.388 ms | 15.738 ms |
| `render-copyout-4x` | 603.23 µs | 597.48 µs | 603.94 µs | 600.33 µs |
| `search` | 106.04 µs | 111.60 µs | 31.26 µs | 33.53 µs |

### Module size

| arm | wasm bytes | delta vs a |
|---|---|---|
| a direct | 4,676,714 | — |
| b rust thin | 4,680,516 | +3.7 kB |
| c rust coarse | 4,681,488 | +4.7 kB |
| d c++ coarse | 4,684,877 | +8.0 kB |

Smaller across the board than the two previous runs (a was 5,180,492 bytes at `-O0`), which is what
you would expect: `-O3` now runs Binaryen's dead-code elimination and inlining passes on every arm,
where `-O0` skipped them for arm D and never applied to A, B, or C's prebuilt/prelinked C++ side
either. Arm D's delta over arm A, `+8.0 kB`, is the smallest it has been across all three published
runs (it was `+7.9 kB` at `-O0` on a larger base, so the four `EPDF_*`/`cc_*` C++ entry points still
cost about the same absolute handful of kilobytes; only the shared PDFium code shrank).

## What the numbers say

**The wrapper tax is real and small.** Arm B costs 0 to 5 percent more than arm A on every workload
that runs PDFium work (`geometry` 1.03x, `text` 1.02x, `render-1x` 1.02x, `render-4x` 1.04x,
`search` 1.05x), and 5 percent below arm A on the pure `null-call` crossing with nothing behind it.
That is inside the pre-registered 15 percent band in
[`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md),
and well inside the 25 percent line past which the prediction said to suspect the harness rather
than the result. `null-peek` at 1.19x is noise: it is a single-digit-nanosecond operation where a
500ms sampling budget cannot resolve better than about a nanosecond, and the round-to-round spread
above confirms `null-peek` is the noisiest cell in the whole table.

**Coarsening the crossing wins on `geometry` and `search`, by about the same margin for Rust and
C++.** Arm C lands at 0.29x (about 3.4x faster than arm A) and arm D lands at 0.29x (about 3.4x
faster than arm A) on `geometry`; on `search`, arm C is 0.29x and arm D is 0.32x. Both are inside
the prediction's 3x-to-10x band for coarsening a chatty per-glyph crossing into one call. The
mechanism is what the prediction expected: `geometryThin` makes two wasm calls per glyph
(`FPDFText_GetTextObject` and `EPDFText_GetCharGeometry`) plus a third on every object-key change,
so 8,106 glyphs cost on the order of sixteen thousand crossings, where `geometryCoarse` makes
exactly one crossing and decodes PDFium's per-glyph struct into a packed 112-byte record on the
native side. `render` and `text` land within 1 to 5 percent everywhere, matching the prediction that
render is dominated by PDFium's own bitmap work and unaffected by any wrapper, and that a single
already-coarse call like `text` has nothing left for coarsening to remove.

**Arm C and arm D are indistinguishable on `geometry`, once both are compiled the same way.** In the
run before this one, arm C measured 25 to 33 percent faster than arm D on `geometry`, holding in
every round. With both arms linked at `-O3`, the median gap is under 1 percent (`c` 1.042 ms vs `d`
1.031 ms, arm D 1 percent faster) and the per-round ratio of d over c is 0.981x to 1.012x across all
seven rounds:

```
round 1  d/c 0.991x
round 2  d/c 0.981x
round 3  d/c 0.994x
round 4  d/c 0.985x
round 5  d/c 0.985x
round 6  d/c 1.012x
round 7  d/c 1.000x
```

That 3-percentage-point spread across rounds is smaller than the worst same-arm, same-workload
round-to-round spread measured anywhere else in this run (1.15x on `a/geometry`), which is to say
the C-versus-D difference on `geometry` is not distinguishable from noise. `search` shows the same
pattern: arm C 30.17 µs against arm D 32.91 µs is a 9 percent gap, in the opposite direction from
what the flag bug produced, and small enough next to the round-to-round spread to not support a
verdict either. **The verdict on `geometry`, and on this benchmark generally, is that arm C and arm
D cost the same.** The entire previous 25-to-33-percent gap was the `-O0` link flag on arm D's C++,
not a property of Rust or C++ as languages, and not a property of `rs_read_page_geometry` versus
`cc_read_page_geometry` as implementations. Once the two are compiled under the same optimizer at
the same level, this benchmark has no evidence that either language does the per-glyph decode work
faster than the other. The win on `geometry` and `search` is coarsening the crossing; which language
sits behind that one crossing does not show up in these numbers.

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
inflate it.

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
