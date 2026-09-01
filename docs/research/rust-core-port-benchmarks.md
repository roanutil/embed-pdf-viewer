# Benchmarking the interop spike

The three POCs in `ts-wasm-interop-poc/` already count boundary crossings, and the counts are pinned
as assertions: 10 crossings for ten unanchored shapes and 70 for ten anchored ones in
`poc-2-rust-geometry/packages/shapes/src/shapes.test.ts:183` and `:196`, and exactly 1 per frame at
any shape count in `poc-3-rust-core/packages/core/src/store.test.ts:40` and `:53`.

A count is not a cost. We measured a single crossing at 1.015 µs through a flat `Float64Array` and
2.735 µs through `serde-wasm-bindgen` objects, from the demo's `measure` button. Multiply that out
and poc-2 spends something like 7 µs per anchored shape per frame, which at 60 fps and 100
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
strokes: Vec[][] }`. Marshalling that costs far more than marshalling a quad. So every poc-2 number
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

poc-3 builds a model through `seedShapes` in `crates/core/src/lib.rs`, while poc-1 and poc-2 build
one through `update(model, { t: 'add' })`. Those are different code paths with different default
sizes and different colour cycling. If the three end up with different shape sets, every number
downstream is void and nothing about the numbers themselves would reveal it.

So the seed layout lives in the shared harness as plain data, an array of `{ x, y, color, anchored }`,
and each adapter applies it through its own dispatch path. Then, before timing, the harness asks all
three for one display list at a fixed view and compares them corner by corner to 1e-9. Any
disagreement aborts the run.

The frozen vectors in `ts-wasm-interop-poc/vectors/anchor-vectors.json` already cover the projection
math across all three implementations. They do not cover the full `scene()` path, the flags word, or
the id and colour table. This gate does.

## Workloads

Four, in the order they should be built. Each cell reports median and p95 in nanoseconds, never mean.

### 1. Per-frame projection

`scene(view)` across N ∈ {1, 10, 100, 1000, 10000} shapes, × {all anchored, none anchored}, ×
{a view that projects (`zoom: 3, rotation: 90`), a view that does not (`zoom: 1, rotation: 0`)}.
Twenty cells per POC.

This is the headline. It is also where the crossover should be visible: poc-1 should win at N=1
because a crossing costs a microsecond and the TypeScript work is nanoseconds, and poc-3 should win
by a wide margin at N=1000 because it does one crossing where poc-2 does seven thousand.

### 2. Hit-test and dispatch

Hit-test at N ∈ {10, 100, 1000}, with a point over the topmost shape and a point over nothing. The
miss is the interesting case: `hit_test` in `poc-3-rust-core/crates/core/src/model.rs:139` walks
every shape in reverse before giving up, so a miss is the worst case and it costs poc-3 exactly one
crossing.

Dispatch runs a synthetic gesture: one `pointerDown`, 100 `pointerMove`, one `pointerUp`, reported as
µs per `pointerMove`. I expect poc-3 to win this at large N, and for a reason that has nothing to do
with wasm: `replace` in `poc-1-ts/packages/shapes/src/update.ts:19` is `shapes.map(...)`, so poc-1
allocates N objects on every single pointer move, while `update` in
`poc-3-rust-core/crates/core/src/model.rs:157` takes `&mut Model` and mutates in place. If that shows
up, it is a finding about immutable-model reducers, not about Rust.

### 3. Cold start

Browser only, and the only workload where poc-1 wins by construction: 0 bytes of wasm against
130.7 kB for poc-2 and 277.2 kB for poc-3.

Each demo gets `performance.mark` calls at four points: navigation start, wasm instantiate resolved,
first `scene()` returned, first paint. Then five hard reloads with the cache disabled, driven through
the Chrome DevTools MCP, reading the marks plus FCP from a `PerformanceObserver`.

This needs a small instrumentation change to the three `App.tsx` files. That is honest work, not
measurement pollution, and the marks should stay in afterwards.

### 4. Allocation and GC pressure

The noisiest of the four, and its job is to explain the frame numbers rather than to be a headline.

In Node, run a fixed 10,000-frame loop under `--expose-gc`, force a collection before and after, and
report the `heapUsed` delta plus a scavenge count scraped from `--trace-gc`. Scavenges per 10,000
frames is the more stable of the two and the better proxy for allocation pressure.

poc-2 is the one to watch: every geometry call allocates a `Float64Array` on the way in and four
`{x, y}` objects plus a tuple on the way out, in `packages/geometry/src/index.ts`. Seven of those per
anchored shape per frame is a lot of garbage for arithmetic that fits in registers.

In the browser, take one pair of heap snapshots through the DevTools MCP as a spot check and leave it
there. `performance.measureUserAgentSpecificMemory()` needs cross-origin isolation and is not worth
the setup for a one-off.

## Method

Rules that apply to every cell, because getting these wrong is how benchmarks lie.

Warm up to V8's optimizing tier before sampling: 1,000 iterations or 100 ms, whichever comes first.
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
ts-wasm-interop-poc/
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

Adapters are the only per-POC code, and each is thin. poc-1 and poc-2 expose the same pure functions,
so their adapters differ by one `await initGeometryFromDisk()`. poc-3's adapter wraps
`createCoreStore()` instead, because state lives in Rust there.

## Predictions

Written down before measuring, so that a mismeasurement is obvious rather than persuasive.

| Workload | Expected winner | Reason |
|---|---|---|
| Frame, N=1, unanchored | poc-1 | one crossing costs ~1 µs; the TypeScript work is nanoseconds |
| Frame, N=1000, anchored | poc-3, by a lot | 1 crossing against roughly 7,000 |
| Frame crossover | N somewhere between 10 and 50 | where marshalling stops dominating |
| `pointerMove`, N=1000 | poc-3 | poc-1 allocates N objects per move; Rust mutates in place |
| Hit-test miss, N=1000 | poc-3 | walks all N in Rust for one crossing |
| Cold start | poc-1 | 0 kB of wasm against 130.7 kB and 277.2 kB |
| Anything at all | never poc-2 | it pays the boundary without moving any work across it |

If poc-2 wins a single cell, the harness is wrong and the numbers should be thrown away.

## What a good outcome looks like

One table in this document with 20 frame cells × 3 implementations, a stated crossover point, a cold
start cost in milliseconds, and a sentence in `rust-core-port.md` replacing "I have no measurement of
what a realistic WASM crossing costs" with a number.

If the crossover lands below roughly 20 shapes, wave 4 of the port plan looks better than I argued
and the dual-implementation worry in that document deserves revisiting. If it lands above a few
hundred, the plan's ordering stands and the annotation brain should stay in TypeScript on web.

I do not know which. That is the point of measuring.
