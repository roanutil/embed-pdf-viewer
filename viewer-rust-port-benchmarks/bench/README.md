# Viewer benchmark procedure

The benchmark measures geometry, hit-testing, state updates, and JavaScript
allocation estimates for the three viewer variants. See
[the design](../../docs/research/rust-core-port-benchmarks.md) and
[historical results](RESULTS.md).

## Run

Install dependencies and build each viewer first. Then run from
`viewer-rust-port-benchmarks`:

```bash
node bench/run.mjs --rounds 5 --budget 500 --alloc-rounds 3
node bench/compare.mjs
```

The runner replaces per-round probe, timing, and allocation JSON in
`bench/results/`; copy any local results you need to preserve before running.
It preserves `cold-start.json`. The comparison script prints tables and
**overwrites `bench/RESULTS.md`**, including any manually added explanations.
It rejects allocation samples with the old heap-only metric.

The repository commits only the cold-start summary, not the timing or
allocation JSON. A fresh checkout therefore cannot reproduce the historical
timing tables by running `compare.mjs` alone. Run the benchmark to produce a
new dataset, and retain its raw files with any new claims.

## How it measures

`seeds.mjs` defines model sizes and layouts. `workloads.mjs` defines the
operations; `harness.mjs` measures them. Each variant supplies a `bench/adapter.ts`
and a `bench/node.ts` entry point.

Each variant runs in a separate Node process with its own workspace dependencies.
Rounds use the same order: TypeScript, Rust geometry, then Rust geometry and
state. Interleaving limits some drift between complete runs, but the fixed
order can still introduce position bias.

Before timing, the runner compares a ten-shape display list from each variant.
It checks metadata and coordinates, allowing small coordinate differences.
This catches disagreement in that probe; it is not proof that every timed
model or operation agrees.

The harness tunes a batch size, times batches, and divides elapsed time by call
count. Each operation returns a number that contributes to an accumulator.
The reported p95 is the 95th percentile of batch averages, not individual-call
latency. The comparison's “worst spread” is the largest `(max - min) / median`
across rounds for one variant in that row. Large spread indicates instability,
without identifying its cause.

Allocation estimates add changes in `heapUsed` and `arrayBuffers` over a short
window with no observed garbage collection. They include copied typed-array
storage but exclude Rust's internal allocations and other external memory.
They are estimates, not a complete count of allocated bytes.

Run the allocation regression check with:

```bash
node --expose-gc --test bench/alloc.test.mjs
```

## Browser startup procedure

Startup requires a browser. This runner does not automate it. The demos expose
`window.__coldStart` from their `apps/demo/src/marks.ts` files.

Build the three demos, then start each preview in a separate terminal from the
benchmark root:

```bash
(cd typescript-baseline && pnpm build)
(cd rust-geometry-only && pnpm build)
(cd rust-geometry-and-state && pnpm build)
```

```bash
(cd typescript-baseline/apps/demo && pnpm exec vite preview --port 4171)
```

```bash
(cd rust-geometry-only/apps/demo && pnpm exec vite preview --port 4172)
```

```bash
(cd rust-geometry-and-state/apps/demo && pnpm exec vite preview --port 4173)
```

Open each preview and run this in its browser console. It loads five same-origin
iframes and omits samples that do not produce a display list within the polling
window:

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
console.table(runs.filter((r) => r !== null).map((r) => ({
  wasmLeg: r.wasmReady == null ? null : r.wasmReady - r.boot,
  firstFrame: r.firstFrame,
})));
```

Record the successful sample count, medians, and first-frame range in
`bench/results/cold-start.json`. `wasmLeg` is `wasmReady - boot`; the TypeScript
baseline also records that interval even though it loads no Wasm.
`firstFrame` is a timestamp since navigation started, recorded by the mounted viewer’s
React effect after its initial render. It is not a measured paint time or a duration from
`boot`; first contentful paint is recorded separately as `fcp` when available.

Repeated iframe loads may reuse cached assets and compiled modules. Record the
browser/cache conditions and do not treat these samples as uncached downloads
or guaranteed fresh Wasm compilation. The current summary has no individual
samples or build hashes, so its original browser timings cannot be independently
recalculated.
