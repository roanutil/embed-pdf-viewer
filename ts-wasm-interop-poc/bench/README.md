# bench

Turns the crossing counts the three POCs assert into microseconds. Design and
rationale live in [`docs/research/rust-core-port-benchmarks.md`](../../docs/research/rust-core-port-benchmarks.md);
the numbers live in [`RESULTS.md`](./RESULTS.md).

## Running it

```
node bench/run.mjs --rounds 5 --budget 500 --alloc-rounds 3
node bench/compare.mjs
```

`run.mjs` needs the wasm built first and will tell you which POC is missing it.
`compare.mjs` prints the tables and writes `RESULTS.md`.

Cold start is not scriptable here. It needs a real browser, so the demos carry
`performance.mark` calls (`apps/demo/src/marks.ts`) and expose
`window.__coldStart`. The procedure is at the bottom of this file, and the
numbers get pasted into `results/cold-start.json`.

## Layout

```
seeds.mjs       the workloads as data: sizes, views, seed layouts
harness.mjs     sampling and statistics, zero dependencies, runs in Node or a browser
workloads.mjs   what gets measured, shared by all three POCs
run.mjs         round-robin driver
compare.mjs     aggregates results/*.json into RESULTS.md
poc-N/bench/    adapter.ts (the only per-POC code) and node.ts (the entry point)
```

## Why it works this way

All three POCs define packages called `@poc/geometry`, `@poc/shapes` and
`@poc/core`. One Node process cannot import three different modules under the
same specifier, and the POCs' mutual independence is the property they exist to
demonstrate. So each benchmarks itself in its own process and writes JSON.

Rounds are interleaved (round 1: p1, p2, p3; round 2: p1, p2, p3) rather than run
per-POC to completion. Clock speed drifts over a six minute run, and doing all of
p1 then all of p2 would hand the drift to whoever went last. Only medians across
rounds are comparable, which is why `compare.mjs` prints a "worst spread" column:
a large value there means that row drifted and its two significant figures are
not real.

A fairness gate runs before any timing. poc-3 could build its model through
Rust's `seedShapes` while poc-1 and poc-2 build theirs in TypeScript, so
different shape sets are a live risk that no timing would reveal. Instead the
seed layout lives in `seeds.mjs` as data, each adapter applies it through its own
dispatch path, and the gate compares all three display lists corner by corner
before continuing. It aborts the run on any disagreement.

## Measurement notes

Timing uses batch means: a batch size is auto-tuned so one batch takes at least
2 ms, whole batches are timed, and the result is divided. Timing individual calls
would let `performance.now()` dominate a 33 ns function. The consequence is that
the reported p95 is the p95 of batch means, so it captures run-to-run drift and
not per-call tail latency. Do not read it as a worst-case frame time.

Every measured function returns a number that the harness accumulates. Without
that, V8 is entitled to delete the call, and a benchmark of nothing looks
excellent.

Allocation figures are `heapUsed` deltas over a window short enough that no
collection intervenes, not an allocation counter. `RESULTS.md` carries the
calibration: a `{x, y}` object measures about 73 bytes by this method against a
~24 byte theoretical minimum, so absolutes are good to roughly 3x. The
comparison between implementations is sound because all three are measured
identically.

Two earlier attempts at the allocation metric are documented in `workloads.mjs`,
because both failed quietly rather than loudly. Counting scavenges through
`PerformanceObserver` does not work: Node 24 emits no perf entry per scavenge,
and its GC kind constants are MINOR=1, MAJOR=4, INCREMENTAL=8, not the 1/2/4 you
would guess. Measuring heap growth over 2,000 frames does not work either, since
that many frames trigger collections which free the garbage mid-loop; it
reported 150 bytes per frame for work that allocates five objects per shape.

## Cold start procedure

Build and preview all three, then drive a browser:

```
for d in poc-1-ts poc-2-rust-geometry poc-3-rust-core; do (cd $d && pnpm build); done
cd poc-1-ts/apps/demo         && npx vite preview --port 4171 &
cd poc-2-rust-geometry/apps/demo && npx vite preview --port 4172 &
cd poc-3-rust-core/apps/demo  && npx vite preview --port 4173 &
```

Open each port and run this in the console. Same-origin iframes are the trick:
they give repeated fresh page loads inside one JS context, where navigating away
would destroy it.

```js
const runs = [];
for (let i = 0; i < 5; i++) {
  runs.push(await new Promise((resolve) => {
    const f = document.createElement('iframe');
    f.style.cssText = 'position:fixed;left:-9999px;width:1200px;height:900px';
    f.src = `${location.origin}/?cb=${i}-${Date.now()}`;
    f.onload = () => {
      const poll = (n) => {
        const cs = f.contentWindow.__coldStart;
        if (cs?.firstFrame != null) { const c = { ...cs }; f.remove(); resolve(c); }
        else if (n <= 0) { f.remove(); resolve(null); }
        else setTimeout(() => poll(n - 1), 40);
      };
      poll(60);
    };
    document.body.appendChild(f);
  }));
}
console.table(runs.map((r) => ({ wasmLeg: r.wasmReady - r.boot, firstFrame: r.firstFrame })));
```

Take the medians into `results/cold-start.json`. Note what this does NOT measure:
the hashed assets come from the memory cache, so the wasm leg is compile plus
instantiate with no download. Add the transfer for a real network.
