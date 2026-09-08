# Benchmarking the interop spike

The three POCs in `viewer-rust-port-benchmarks/` already count boundary crossings, and the counts are pinned
as assertions: 10 crossings for ten unanchored shapes and 70 for ten anchored ones in
`rust-geometry-only/packages/shapes/src/shapes.test.ts:183` and `:196`, and exactly 1 per frame at
any shape count in `rust-geometry-and-state/packages/core/src/store.test.ts:40` and `:53`.

A count is not a cost. We measured a single crossing at 1.015 µs through a flat `Float64Array` and
2.735 µs through `serde-wasm-bindgen` objects, from the demo's `measure` button. Multiply that out
and rust-geometry-only spends something like 7 µs per anchored shape per frame, which at 60 fps and 100
annotations would be catastrophic and at 5 annotations would be free. Nobody should be guessing which
end of that range the real viewer sits at.

So this is a plan to turn the counts into microseconds. It is a one-off measurement, not a CI gate.

## What this cannot tell us

Worth saying before the design, because the temptation to over-read the results will be strong.

It says nothing about the PDFium linkage question, which is wave 0 of
[the port plan](./rust-core-port.md) and the thing that can invalidate everything below it. These
crates are pure Rust with no C++ dependency.

It says nothing about whether 16,281 lines of `engine-services` port well. A 377-line Rust brain is
not evidence about that, in either direction.

And the toy's geometry is one variant, a rectangle's four corners, 8 floats. The real `Geom` in
`packages/core/annotation/src/types.ts:121` is a seven-variant union including `{ t: 'ink';
strokes: Vec[][] }`. Marshalling that costs far more than marshalling a quad. So every rust-geometry-only number
here is a LOWER bound on what the real boundary would cost, and I would not be surprised by a factor
of five.

## The problem the design has to solve

All three POCs define packages named `@poc/geometry`, `@poc/shapes`, `@poc/core`. One Node process
cannot import three different modules under the same specifier, and the POCs' mutual independence is
the whole property they exist to demonstrate. Aliasing them into a single harness would trade that
away for noise control.

So each POC benchmarks itself, in its own process, and writes JSON. A driver runs the three in
round-robin order: round 1 is p1, p2, p3, round 2 is p1, p2, p3, and so on for k rounds. The
aggregator takes medians across rounds. That matters because a laptop's clock speed drifts over the
course of a run, and running all of p1 then all of p2 then all of p3 would hand the drift to whoever
went last. Interleaving spreads it.

The cost of this choice: a single round's numbers are not comparable across POCs, only the medians
across interleaved rounds are. I think that is the right trade, but it does mean k needs to be large
enough to be meaningful. Start at k=7 and look at the spread before trusting anything.

## The fairness gate

This is the part I would most expect to get wrong, so it runs before any timing.

rust-geometry-and-state builds a model through `seedShapes` in `crates/core/src/lib.rs`, while typescript-baseline and rust-geometry-only build
one through `update(model, { t: 'add' })`. Those are different code paths with different default
sizes and different colour cycling. If the three end up with different shape sets, every number
downstream is void and nothing about the numbers themselves would reveal it.

So the seed layout lives in the shared harness as plain data, an array of `{ x, y, color, anchored }`,
and each adapter applies it through its own dispatch path. Then, before timing, the harness asks all
three for one display list at a fixed view and compares them corner by corner to 1e-9. Any
disagreement aborts the run.

The frozen vectors in `viewer-rust-port-benchmarks/vectors/anchor-vectors.json` already cover the projection
math across all three implementations. They do not cover the full `scene()` path, the flags word, or
the id and colour table. This gate does.

## Workloads

Four, in the order they should be built. Each cell reports median and p95 in nanoseconds, never mean.

### 1. Per-frame projection

`scene(view)` across N ∈ {1, 10, 100, 1000, 10000} shapes, × {all anchored, none anchored}, ×
{a view that projects (`zoom: 3, rotation: 90`), a view that does not (`zoom: 1, rotation: 0`)}.
Twenty cells per POC.

This is the headline. It is also where the crossover should be visible: typescript-baseline should win at N=1
because a crossing costs a microsecond and the TypeScript work is nanoseconds, and rust-geometry-and-state should win
by a wide margin at N=1000 because it does one crossing where rust-geometry-only does seven thousand.

### 2. Hit-test and dispatch

Hit-test at N ∈ {10, 100, 1000}, with a point over the topmost shape and a point over nothing. The
miss is the interesting case: `hit_test` in `rust-geometry-and-state/crates/core/src/model.rs:139` walks
every shape in reverse before giving up, so a miss is the worst case and it costs rust-geometry-and-state exactly one
crossing.

Dispatch runs a synthetic gesture: one `pointerDown`, 100 `pointerMove`, one `pointerUp`, reported as
µs per `pointerMove`. I expect rust-geometry-and-state to win this at large N, and for a reason that has nothing to do
with wasm: `replace` in `typescript-baseline/packages/shapes/src/update.ts:19` is `shapes.map(...)`, so typescript-baseline
allocates N objects on every single pointer move, while `update` in
`rust-geometry-and-state/crates/core/src/model.rs:157` takes `&mut Model` and mutates in place. If that shows
up, it is a finding about immutable-model reducers, not about Rust.

### 3. Cold start

Browser only, and the only workload where typescript-baseline wins by construction: 0 bytes of wasm against
130.7 kB for rust-geometry-only and 277.2 kB for rust-geometry-and-state.

Each demo gets `performance.mark` calls at four points: navigation start, wasm instantiate resolved,
first `scene()` returned, first paint. Then five hard reloads with the cache disabled, driven through
the Chrome DevTools MCP, reading the marks plus FCP from a `PerformanceObserver`.

This needs a small instrumentation change to the three `App.tsx` files. That is honest work, not
measurement pollution, and the marks should stay in afterwards.

### 4. Allocation and GC pressure

The noisiest of the four, and its job is to explain the frame numbers rather than to be a headline.

In Node, force a collection and then run a window of frames SHORT enough that no collection
intervenes, reporting the `heapUsed` delta divided by the frame count. Verify no GC fired; if one
did, halve the window and retry.

(Two earlier designs for this are recorded in `bench/workloads.mjs` because both failed quietly. A
10,000-frame window is self-defeating, because that many frames trigger the very collections whose
absence the measurement depends on: it reported 150 bytes per frame for work that allocates five
objects per shape. Before that I counted scavenges through `PerformanceObserver`, read back almost
nothing, and concluded Node 24 emits no perf entry per scavenge. It does. The entries arrive several
event-loop turns after the collection, and I was draining with a single `setImmediate`: with four
`setTimeout(0)` turns, two million churned objects report 15 MINOR entries on Node v24.20.0. The
kind constants really are MINOR=1, MAJOR=4, INCREMENTAL=8, not the 1/2/4 I assumed.)

rust-geometry-only is the one to watch: every geometry call allocates a `Float64Array` on the way in and four
`{x, y}` objects plus a tuple on the way out, in `packages/geometry/src/index.ts`. Seven of those per
anchored shape per frame is a lot of garbage for arithmetic that fits in registers.

In the browser, take one pair of heap snapshots through the DevTools MCP as a spot check and leave it
there. `performance.measureUserAgentSpecificMemory()` needs cross-origin isolation and is not worth
the setup for a one-off.

## Method

Rules that apply to every cell, because getting these wrong is how benchmarks lie.

Warm up to V8's optimizing tier before sampling: 10,000 iterations or 200 ms, whichever comes first.
(I first wrote 1,000 and 100 ms here, which is too few calls to reliably tier up a small function.)
Sample against a fixed time budget per cell, 500 ms, counting iterations, rather than a fixed
iteration count. N=1 and N=10000 differ by four orders of magnitude and a fixed count starves one end
or wastes minutes on the other.

Consume every result. Sum a coordinate off the returned display list into an accumulator the harness
prints at the end. An unconsumed `scene()` call is a `scene()` call V8 is allowed to delete.

Report median and p95. Means on benchmark data are a way of reporting the worst outlier as if it were
typical.

Stamp the environment into every results file: Node version, OS version, CPU model, and whether the
machine is on battery. Absolute numbers here are machine-specific and a results file without that
header is a trap for whoever reads it in six months.

Accept that macOS thermal throttling is not controllable. The interleaving is the mitigation, and if
the spread across rounds is wide, say so in the write-up rather than picking the prettiest round.

## Shape of it

```
viewer-rust-port-benchmarks/
  bench/
    seeds.mjs        shared seed layouts as data, so the three models are provably identical
    harness.mjs      zero-dep sampling and statistics, imported by relative path
    run.mjs          round-robin driver, k rounds, spawns each POC's node.mjs
    compare.mjs      reads results/*.json, prints a table and a markdown block
    results/         one JSON per POC per round, with the environment header
  poc-N/bench/
    adapter.mjs      maps the shared workloads onto this POC's API
    node.mjs         entry point, writes ../../bench/results/<poc>-<round>.json
    browser.html     the same workloads in a page, results on window.__bench
```

The harness is deliberately dependency-free, roughly 120 lines, and copied nowhere: each adapter
imports it by relative path. I chose that over `tinybench` because the workloads need per-cell setup
that a generic runner fights, and because adding a devDependency to three workspaces to time a loop
is a poor trade. The cost is real, though: I own the statistics, and mine will be less battle-tested
than `tinybench`'s. If the numbers come out strange, that is the first thing to suspect.

Adapters are the only per-POC code, and each is thin. typescript-baseline and rust-geometry-only expose the same pure functions,
so their adapters differ by one `await initGeometryFromDisk()`. rust-geometry-and-state's adapter wraps
`createCoreStore()` instead, because state lives in Rust there.

## Predictions

Written down before measuring, so that a mismeasurement is obvious rather than persuasive.

| Workload | Expected winner | Reason |
|---|---|---|
| Frame, N=1, unanchored | typescript-baseline | one crossing costs ~1 µs; the TypeScript work is nanoseconds |
| Frame, N=1000, anchored | rust-geometry-and-state, by a lot | 1 crossing against roughly 7,000 |
| Frame crossover | N somewhere between 10 and 50 | where marshalling stops dominating |
| `pointerMove`, N=1000 | rust-geometry-and-state | typescript-baseline allocates N objects per move; Rust mutates in place |
| Hit-test miss, N=1000 | rust-geometry-and-state | walks all N in Rust for one crossing |
| Cold start | typescript-baseline | 0 kB of wasm against 130.7 kB and 277.2 kB |
| Anything at all | never rust-geometry-only | it pays the boundary without moving any work across it |

If rust-geometry-only wins a single cell, the harness is wrong and the numbers should be thrown away.

## What a good outcome looks like

One table in this document with 20 frame cells × 3 implementations, a stated crossover point, a cold
start cost in milliseconds, and a sentence in `rust-core-port.md` replacing "I have no measurement of
what a realistic WASM crossing costs" with a number.

If the crossover lands below roughly 20 shapes, wave 4 of the port plan looks better than I argued
and the dual-implementation worry in that document deserves revisiting. If it lands above a few
hundred, the plan's ordering stands and the annotation brain should stay in TypeScript on web.

I do not know which. That is the point of measuring.

## What actually happened

Built, run over five interleaved rounds, and the predictions above were mostly wrong. The full
tables are in [`viewer-rust-port-benchmarks/bench/RESULTS.md`](../../viewer-rust-port-benchmarks/bench/RESULTS.md);
what follows is what I did not expect.

**rust-geometry-and-state never beat typescript-baseline on a single frame cell.** Not at 10 shapes, not at 10,000. It ran 1.6x to
3.1x slower everywhere, and the crossover I predicted between n=10 and n=50 does not exist. At
n=1000 anchored and projecting, the workload the whole port plan cares about, typescript-baseline takes 127 µs and
rust-geometry-and-state takes 207 µs, for a frame that crosses the boundary exactly once against typescript-baseline's zero.

So I added a measurement that was not in this plan: `sceneRaw`, which returns the flat
`Float64Array` without decoding it into `DisplayList` objects. That decode allocates one item plus
four points per shape, which is the same allocation typescript-baseline performs, so I expected it to be the whole
explanation. It is about half of it. The decode is 44% to 53% of rust-geometry-and-state's frame cost at n=1000 and
above. Strip it entirely and rust-geometry-and-state is STILL 1.35x to 1.75x slower than typescript-baseline.

Which leaves one conclusion I cannot argue my way out of: for arithmetic this simple, V8's JIT beats
Rust in wasm plus a single boundary crossing. Not by a little. The hit-test numbers say the same
thing from another angle, where rust-geometry-and-state loses even the n=1000 miss case, 154 µs against 128 µs, and
that case walks all thousand shapes in Rust for one crossing while typescript-baseline walks all thousand in
JavaScript.

**rust-geometry-only won 0 of 20 frame cells**, which is the one prediction that held cleanly. It ran 4.6x to 8.4x
slower than typescript-baseline across the board. A fine-grained boundary is exactly as bad as the plan said.

**Dispatch was the only place Rust won**, and for a reason that has nothing to do with Rust. rust-geometry-and-state
costs 18.7x typescript-baseline per `pointerMove` at 10 shapes, because every message pays a serde crossing. At
1,000 shapes it costs 0.33x, because `replace` at `typescript-baseline/packages/shapes/src/update.ts:19` is
`shapes.map(...)` and allocates a thousand objects on every pointer move while Rust mutates in place.
That is a finding about immutable-model reducers, and typescript-baseline could win it back with a targeted fix.

**Cold start** cost 6.3 ms to instantiate 130.7 kB and 7.8 ms for 277.2 kB, pushing first frame from
19.0 ms to 21.2 ms and 24.8 ms. Smaller than I feared, though that is localhost with the hashed
assets served from cache, so the download is not in those numbers.

**The benchmark found two bugs**, which is a decent argument for having built it. rust-geometry-and-state's decode
looked up ids with `rows.find(...)` inside the per-item loop, making `scene()` O(n^2): invisible at
demo sizes and 184x slower than typescript-baseline at 10,000 shapes, for a frame that still crossed the boundary
exactly once. And the first allocation metric reported 0 scavenges for all three implementations,
which is not a plausible answer for 200,000 object allocations and should have been the giveaway.

### What this changes

I opened this document hoping to replace a missing number in
[the port plan](./rust-core-port.md). The number turns out to argue against the framing of the
question. There is no crossover to find, so the paragraph I wrote about wave 4 looking better if the
crossover landed under twenty shapes is moot.

What survives, and is strengthened: the case for this port is code sharing across platforms, not
performance on web. Every frame metric here says a Rust core makes the web build slower, and the
honest reason to do it anyway is that iOS and Android otherwise need a reimplementation of
`engine-services` that will drift.

The caveat that matters most. This toy's per-shape math is a handful of multiplies and adds, which is
precisely the shape V8 optimizes best. The real annotation core does considerably more per shape:
cloudy border generation in `packages/core/annotation/src/cloudy.ts` is 570 lines, ink resampling and
polygon hit-testing are not four floats and a winding test. So this result does NOT say Rust would
lose on the real workload. It says the boundary and the JavaScript object materialization together
eat any win on math this cheap, and that a real port should pick its first slice by asking how much
compute sits behind each crossing.

Which is, in fairness, what wave 2 of the port plan already says. I just have numbers for it now.
