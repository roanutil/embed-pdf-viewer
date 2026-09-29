# Viewer Performance Comparison Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A tracked, one-command harness that builds the web viewer from `main`, `research/multi-platform-rust-core-impl` and `research/multi-platform-rust-core-impl-plugins` at pinned commits, drives the same flows against each in Chromium, and reports load, interaction, CPU and memory differences with confidence intervals.

**Architecture:** A self-contained `perf-compare/` directory at the repo root, sibling to `pdfium-binding-benchmarks/`. It materializes each branch into its own git worktree, builds that worktree with a per-branch recipe, then builds one shared entry page into it so every arm runs the same viewer code through the same door. A static server serves each arm on its own origin. A Playwright runner drives interleaved rounds, a fairness gate checks every arm gives the same answers first, and a report script turns raw samples into ratios against `main`.

**Tech Stack:** Node 24.20.0, pnpm 10.34.0, `playwright-core` pinned exactly, plain ESM with `node:test`, Vite from each arm's own `packages/viewer/main`, Rust 1.98.0, Emscripten from each arm's `build-engine-runtime.yml`, `wasm-bindgen-cli` 0.2.127.

## Why this shape

We've already got three pieces of perf tooling on these branches, and none of them answers "is the viewer faster or slower on branch X than on `main`".

`tooling/viewer-e2e` comes closest. It runs real flows in Playwright and writes a JSON with `navigationToFirstPageFrameMs`, `flowWallMs` and render round trips. But it doesn't exist on `main` (it arrived in `284b9b97b`), it serves `examples/viewer-react` through the Vite dev server, and it can't select the Rust kernel. `tooling/engine-baseline` measures one wire kind, `pages.geometry`, in Node with an inline transport. `examples/rust-kernel-eval` on the plugins branch has in-page counters and no timing at all: `docs/research/rust-plugins-web-eval.md` says "Nothing was timed".

The three branches also put Rust in different places, which is what makes a naive comparison misleading:

- `main` is TypeScript all the way down, with PDFium in a worker.
- The impl branch links a Rust engine core into the PDFium payload by default (`packages/engine/runtime/scripts/build-target.sh`), but the viewer only routes to it when `rustRouting: { rustReads: true }` is passed, and then only for `pages.geometry`. Everything else stays on the TypeScript path.
- The plugins branch moves the kernel and 19 plugins into Rust on the main thread (`crates/epdf-web`, `packages/core-wasm`), talking JSON text over `packages/core-wasm/src/proxy.ts`. PDFium stays in the same TypeScript worker, without `rustRouting`.

So we compare five arms rather than three, and we report the pairs that isolate each change alongside the headline ratio against `main`. The cost is build time: three worktrees and two locally built wasm payloads. The first run is slow and the cache carries every run after it.

We build every arm as a production Vite bundle and serve it statically. The dev server serves unbundled modules, and `examples/rust-kernel-eval` only has a dev mode, so timing either would measure Vite's module graph as much as the viewer.

## What we compare

### The two halves of the viewer

The web viewer has two halves, and the branches put Rust into different ones.

The **kernel** runs on the page's main thread. It holds the viewer's state: which document is open, which page you're on, the zoom level, the current text selection, the annotations. The plugins (stage, search, selection, annotation, and so on) live here too. On `main` and the impl branch it's TypeScript (`packages/core/main` and `packages/plugin/*`). On the plugins branch it's Rust compiled to wasm (`crates/epdf-kernel`, `crates/plugins/*`), and the TypeScript UI talks to it by passing JSON text across `packages/core-wasm/src/proxy.ts`.

The **engine** runs in a web worker. It's PDFium compiled to wasm, plus the TypeScript code in `packages/engine/services` that asks PDFium for things: parse this file, render this page to pixels, give me the text on page 4. The kernel sends it requests and waits for answers. Each kind of request has a name, such as `pages.geometry`, which returns the position of every character on a page. Text selection and hit testing use it.

The impl branch adds a second implementation of those requests, written in Rust (`crates/epdf-engine`), and links it into the engine's wasm file next to the TypeScript one. By default the worker still sends every request to the TypeScript code, so the Rust code is in the file and never runs. Passing `rustRouting: { rustReads: true }` when the engine starts makes the worker send one request kind, `pages.geometry`, to Rust instead. That's what "engine requests" means in the table below: which implementation answers the worker's requests. The impl branch routes nothing else to Rust yet.

### Arms

An arm is one thing we build and time: a branch, plus the switches it runs with. The arm names are the branch (`impl` is `research/multi-platform-rust-core-impl`, `plugins` is `research/multi-platform-rust-core-impl-plugins`) followed by what's different about that arm.

| Arm | Branch | Kernel | Engine requests | What it's for |
| --- | --- | --- | --- | --- |
| `main` | `main` | TypeScript | All TypeScript | The baseline everything is compared against |
| `main-aa` | `main` | TypeScript | All TypeScript | A second copy of `main`, built and served separately. Any difference between it and `main` is noise, and tells us how big a difference has to be before it means something |
| `impl-ts` | impl | TypeScript | All TypeScript, with the Rust code linked in but unused | The impl branch as it runs by default |
| `impl-rust` | impl | TypeScript | `pages.geometry` in Rust, the rest TypeScript | The impl branch with its Rust switch on |
| `plugins` | plugins | Rust | All TypeScript | The plugins branch as it runs in `examples/rust-kernel-eval` |

The plugins branch contains all of the impl branch too, but its eval app doesn't turn on `rustReads`, so its engine behaves like `impl-ts`.

### Pairs in the report

Comparing every arm against `main` gives a headline number but doesn't say why an arm is faster or slower, because most arms differ from `main` in more than one way. So the report also compares pairs that differ in exactly one thing.

| Pair | The one thing that differs | Question it answers |
| --- | --- | --- |
| `main` vs `main-aa` | Nothing | How big is a difference that means nothing? |
| `main` vs `impl-ts` | The engine's wasm file now also contains the unused Rust code | Does carrying that code cost anything on its own? Expect a bigger download and maybe a slower start, and no change once a document is open. |
| `impl-ts` vs `impl-rust` | `pages.geometry` answered by Rust instead of TypeScript | Is the Rust geometry code faster in the real viewer? Expect a change in the select flow and nowhere else. |
| `impl-ts` vs `plugins` | The kernel and plugins are Rust instead of TypeScript | What does moving the kernel to Rust cost or save, with the engine held the same? |
| `main` vs `plugins` | Everything the plugins branch changes | The headline number for the plugins branch |

### Metrics

Every time and byte metric is lower-is-better. Every metric is recorded per arm, per flow, per round.

| Group | Metric | How it's measured |
| --- | --- | --- |
| Size | `engineWasmBytes`, `engineWasmBrotliBytes` | The engine `.wasm` in the arm's `dist`, raw and brotli level 11 |
| Size | `kernelWasmBytes`, `kernelWasmBrotliBytes` | The wire kernel `.wasm` (plugins arm only; zero elsewhere) |
| Size | `jsBytes`, `jsBrotliBytes` | Every `.js` in the arm's `dist`, summed |
| Open | `readyMs` | Navigation start to the entry's `perf:ready` mark (`epdf:ready` fired) |
| Open | `openedMs` | Navigation start to `perf:opened` (the handle's `opened` event) |
| Open | `firstPaintMs` | Navigation start to the first frame after a visible page `img` has decoded, the same rule as `tooling/viewer-e2e/performance.ts` |
| Open | `settledMs` | Navigation start to the last change in visible page images before a 250 ms quiet window |
| Interaction | `settleMs` | Input or API call to the same settled rule, per flow step |
| Interaction | `frameP50Ms`, `frameP95Ms`, `jankFrames` | `requestAnimationFrame` intervals during scroll; `jankFrames` counts intervals over 50 ms |
| Interaction | `longTaskMs`, `longTasks` | `PerformanceObserver` `longtask` entries during the flow |
| CPU | `mainTaskMs`, `mainScriptMs` | CDP `Performance.getMetrics` `TaskDuration` and `ScriptDuration` deltas on the page target |
| CPU | `workerTaskMs` | The same on the engine worker target, when CDP exposes it (checked in Task 6) |
| Memory | `heapAfterOpenBytes` | `performance.measureUserAgentSpecificMemory()` total after a forced GC, once open has settled |
| Memory | `heapSoakSlopeBytes` | Least-squares slope of that total across the soak cycles, in bytes per cycle |
| Memory | `workerHeapAfterOpenBytes` | The `DedicatedWorker` share of that breakdown |

`measureUserAgentSpecificMemory` needs a cross-origin isolated page, so the server sends `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp` to every arm, and Chromium runs with `--enable-blink-features=ForceEagerMeasureMemory` so the promise resolves in about a second instead of up to 20. Task 6 includes a self-test that grows a `WebAssembly.Memory` by 64 MiB and checks the total moves by at least that much. If it doesn't, wasm memory isn't being counted and the memory metrics fall back to CDP `Runtime.getHeapUsage` per target, which the report must then label as JS-heap-only.

The plugins arm also records `window.__epdfCounts()`-style kernel table counts during the soak. They're diagnostic, not compared, because no other arm has them.

### Flows

| Flow | Fixture | Steps | Primary metrics |
| --- | --- | --- | --- |
| `open` | `ebook.pdf`, `large.pdf` | Cold navigation in a fresh context | `readyMs`, `openedMs`, `firstPaintMs`, `settledMs` |
| `scroll` | `large.pdf` | 60 wheel events of 240 px at 16 ms spacing from page 1 | Frame stats, `longTaskMs`, `settleMs` after the last wheel |
| `jump` | `large.pdf` | `goToPageIndex(300)` | `settleMs` |
| `zoom` | `ebook.pdf` | `zoomTo` 1 → 2 → 4 → 1, settling between | `settleMs` per step and summed |
| `select` | `ebook.pdf` | Pointer drag across four text lines on page 2 | `settleMs` from `mouseup` to `readText()` resolving non-empty |
| `search` | `ebook.pdf` | `search({ text: 'the' })` | Time to the promise resolving; `hitCount` goes to the gate |
| `annotate` | `ebook.pdf` | `execute('annotation:add-highlight')`, then drag across one line | Time from `mouseup` to the annotation list growing by one and the page settling |
| `soak` | `ebook.pdf` | 30 cycles of `goToPageIndex(last)`, `zoomTo(3)`, `goToPageIndex(0)`, `zoomTo(1)` | `heapSoakSlopeBytes`, plus cycle-5 and cycle-30 totals |

API-driven steps use the public `ViewerHandle` lenses (`StageToken`, `SearchToken`, `SelectionToken`, `AnnotationToken`), so they don't depend on pointer timing. `scroll`, `select` and `annotate` use Playwright input because the input path is part of what those flows measure.

### Statistics

We report median and p95. Never mean.

Each round opens every arm once per flow, in an order rotated by round (round `r` starts at arm `r mod 5`). Each flow gets a fresh browser context: open the fixture, run the flow once unmeasured, then once measured. `open` has no warm-up because cold is the point. The default is `R = 10` rounds; `--quick` is `R = 3` and its report is stamped "quick, not for publication".

For each pair and metric the report gives the ratio `median(candidate) / median(reference)` and a 95% bootstrap interval (10,000 resamples, seeded, so a rerun of `report.mjs` over the same raw data prints the same numbers). The verdict is "slower" or "faster" only when the interval excludes 1.0 and the ratio's distance from 1.0 is larger than the `main` vs `main-aa` interval's widest distance from 1.0 for the same metric, with a floor of 3%. Anything else is "no detectable difference".

### Environment

Every run writes `meta.json` with the harness commit, each arm's resolved SHA, the sha256 of each arm's engine and kernel wasm, each fixture's sha256, the Chromium and Playwright versions, Node version, OS, CPU model and core count, total memory, power source (`pmset -g batt` on macOS), the viewport (1280×800), device scale factor (2) and CPU throttle rate. `report.mjs --against <other-run>` refuses to compare two runs whose environment, fixtures or flow set differ, the same rule `tooling/viewer-e2e/compare-performance.mjs` enforces.

There are two profiles. `desktop` runs unthrottled. `slow` applies CDP `Emulation.setCPUThrottlingRate` at 4× to the page, which approximates a mid-range laptop and makes main-thread differences easier to see. A developer runs on AC power with other apps closed; `doctor.mjs` warns on battery.

## Global Constraints

- Node 24.20.0 and pnpm 10.34.0, pinned in `mise.toml`. Run `mise install` if either is missing.
- Rust 1.98.0 with `wasm32-unknown-emscripten` and `wasm32-unknown-unknown`, from `rust-toolchain.toml`.
- Emscripten version is read from `EMSDK_VERSION` in the arm worktree's `.github/workflows/build-engine-runtime.yml` (3.1.72 at all three pinned SHAs). Never hard-code it in the harness.
- `wasm-bindgen-cli` exactly 0.2.127. `crates/epdf-web/scripts/build.sh` rejects any other version.
- `perf-compare/` is not a pnpm workspace member. It has its own `package.json` and `pnpm-lock.yaml` and installs with `pnpm install --ignore-workspace`. Don't add it to `pnpm-workspace.yaml` or `turbo.json`.
- The harness never commits to, pushes, or modifies tracked files in an arm worktree. It only adds untracked files (the entry and kernel shim) and build outputs.
- Arm SHAs are pinned in `perf-compare/arms.json`. Moving a pin is a commit, made with `scripts/pin.mjs`.
- Raw results and worktrees are gitignored. A result becomes tracked only when someone runs `report.mjs --publish`, which writes `perf-compare/RESULTS.md`.
- Report median and p95. Never mean.
- Every arm gets identical server headers, viewport, device scale factor, Chromium flags and throttle.

---

## File Structure

```
perf-compare/
  .gitignore              .work/, results/, node_modules/
  README.md               what it measures, how to run it, how long it takes
  package.json            playwright-core, scripts
  pnpm-lock.yaml
  arms.json               worktrees (ref + sha + recipe) and arms (flags)
  perf.mjs                one command: doctor, materialize, build, gate, run, report
  run.mjs                 gate + interleaved rounds, writes results/<run-id>/
  report.mjs              results/<run-id>/ -> report.md, --against, --publish
  RESULTS.md              the last published result (created by --publish)
  fixtures/
    generate.mjs          writes large.pdf deterministically
    large.pdf             400 pages, committed so its hash never drifts
    ebook.pdf             copied from packages/viewer/main/public at main 2516e278
    fixtures.json         name -> sha256
  entry/
    index.html            one <div id="viewer">
    main.ts               EmbedPDF.init + window.__perf bridge + marks
    globals.d.ts          PerfBridge and __probe window types
    kernel-typescript.ts  no-op kernel setup
    kernel-rust.ts        initWasm() + installHosts(), copied into plugins worktree
    vite.config.mjs       arm-aware build: aliases, rust-kernel redirect
  lib/
    config.mjs            load and validate arms.json
    stats.mjs             quantiles, bootstrap ratio, verdict, slope
    env.mjs               meta.json collection
    serve.mjs             static server with COOP/COEP, one port per arm
    probe.js              init script: paint/settle, frames, long tasks
    cdp.mjs               metrics, GC, memory, throttle
    flows.mjs             the eight flows
    gate.mjs              cross-arm answer check
  scripts/
    doctor.mjs            toolchain and machine checks
    pin.mjs               resolve branch tips into arms.json
    materialize.mjs       git worktree add at pinned sha
    build-arm.mjs         run the recipe, build the entry, cache by key
  test/
    config.test.mjs
    stats.test.mjs
    env.test.mjs
    serve.test.mjs
    memory.selftest.mjs   the 64 MiB wasm memory check (needs Chromium)
```

`arms.json` is the only file that knows which branches exist. `build-arm.mjs` is the only file that knows how a worktree is built. `flows.mjs` is the only file that knows what is measured. `gate.mjs` is the only file that knows what "the same answer" means. Adding a sixth arm should only touch `arms.json`, plus `build-arm.mjs` if it needs a new recipe.

---

### Task 1: Skeleton, config and pins

**Files:**
- Create: `perf-compare/.gitignore`, `perf-compare/package.json`, `perf-compare/arms.json`
- Create: `perf-compare/lib/config.mjs`, `perf-compare/scripts/pin.mjs`
- Test: `perf-compare/test/config.test.mjs`

**Interfaces:**
- Produces: `loadConfig(path?: string): Config` where `Config = { baseline: string, noiseArm: string, worktrees: Record<string, { ref: string, sha: string, recipe: 'typescript' | 'rust-core' | 'rust-kernel' }>, arms: Record<string, { worktree: string, kernel: 'typescript' | 'rust', rustReads: boolean }>, pairs: [string, string][], armOrder: string[] }`. `armOrder` is `Object.keys(arms)` in file order.

- [ ] **Step 1: Write the skeleton**

`perf-compare/.gitignore`:

```
node_modules/
.work/
results/
```

`perf-compare/package.json`:

```json
{
  "name": "embedpdf-perf-compare",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "node --test test/*.test.mjs",
    "doctor": "node scripts/doctor.mjs",
    "compare": "node perf.mjs"
  },
  "dependencies": {
    "playwright-core": "1.55.0"
  }
}
```

`perf-compare/arms.json`:

```json
{
  "baseline": "main",
  "noiseArm": "main-aa",
  "worktrees": {
    "main": {
      "ref": "main",
      "sha": "2516e2786ee894383220ecd436fbd248182ef444",
      "recipe": "typescript"
    },
    "impl": {
      "ref": "research/multi-platform-rust-core-impl",
      "sha": "27801926782369a067b2f9116b81a4c3ec5bcf03",
      "recipe": "rust-core"
    },
    "plugins": {
      "ref": "research/multi-platform-rust-core-impl-plugins",
      "sha": "17296a72681f364b5d94ddc7a52bc7b5e66348a2",
      "recipe": "rust-kernel"
    }
  },
  "arms": {
    "main": { "worktree": "main", "kernel": "typescript", "rustReads": false },
    "main-aa": { "worktree": "main", "kernel": "typescript", "rustReads": false },
    "impl-ts": { "worktree": "impl", "kernel": "typescript", "rustReads": false },
    "impl-rust": { "worktree": "impl", "kernel": "typescript", "rustReads": true },
    "plugins": { "worktree": "plugins", "kernel": "rust", "rustReads": false }
  },
  "pairs": [
    ["main", "main-aa"],
    ["main", "impl-ts"],
    ["impl-ts", "impl-rust"],
    ["impl-ts", "plugins"],
    ["main", "plugins"]
  ]
}
```

- [ ] **Step 2: Write the failing test**

`perf-compare/test/config.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { loadConfig } from '../lib/config.mjs';

const write = (obj) => {
  const file = path.join(mkdtempSync(path.join(tmpdir(), 'perf-')), 'arms.json');
  writeFileSync(file, JSON.stringify(obj));
  return file;
};

const valid = {
  baseline: 'a',
  noiseArm: 'b',
  worktrees: { w: { ref: 'main', sha: 'f'.repeat(40), recipe: 'typescript' } },
  arms: {
    a: { worktree: 'w', kernel: 'typescript', rustReads: false },
    b: { worktree: 'w', kernel: 'typescript', rustReads: false },
  },
  pairs: [['a', 'b']],
};

test('the tracked arms.json loads', () => {
  const config = loadConfig();
  assert.deepEqual(config.armOrder, ['main', 'main-aa', 'impl-ts', 'impl-rust', 'plugins']);
});

test('armOrder follows file order', () => {
  assert.deepEqual(loadConfig(write(valid)).armOrder, ['a', 'b']);
});

test('rejects an arm naming an unknown worktree', () => {
  const bad = structuredClone(valid);
  bad.arms.a.worktree = 'nope';
  assert.throws(() => loadConfig(write(bad)), /arm a: unknown worktree nope/);
});

test('rejects a short sha', () => {
  const bad = structuredClone(valid);
  bad.worktrees.w.sha = 'abc123';
  assert.throws(() => loadConfig(write(bad)), /worktree w: sha must be 40 hex characters/);
});

test('rejects a pair naming an unknown arm', () => {
  const bad = structuredClone(valid);
  bad.pairs = [['a', 'zzz']];
  assert.throws(() => loadConfig(write(bad)), /pair a\/zzz: unknown arm zzz/);
});

test('rejects rustReads on the rust kernel', () => {
  const bad = structuredClone(valid);
  bad.arms.a = { worktree: 'w', kernel: 'rust', rustReads: true };
  assert.throws(() => loadConfig(write(bad)), /arm a: rustReads needs the typescript kernel/);
});
```

- [ ] **Step 3: Run it and watch it fail**

Run: `cd perf-compare && pnpm install --ignore-workspace && pnpm test`
Expected: FAIL, `Cannot find module '../lib/config.mjs'`.

- [ ] **Step 4: Implement `lib/config.mjs`**

```js
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const DEFAULT = fileURLToPath(new URL('../arms.json', import.meta.url));
const RECIPES = new Set(['typescript', 'rust-core', 'rust-kernel']);
const KERNELS = new Set(['typescript', 'rust']);

export function loadConfig(file = DEFAULT) {
  const raw = JSON.parse(readFileSync(file, 'utf8'));
  for (const [id, w] of Object.entries(raw.worktrees)) {
    if (!/^[0-9a-f]{40}$/.test(w.sha)) throw new Error(`worktree ${id}: sha must be 40 hex characters`);
    if (!RECIPES.has(w.recipe)) throw new Error(`worktree ${id}: unknown recipe ${w.recipe}`);
    if (!w.ref) throw new Error(`worktree ${id}: ref is required`);
  }
  for (const [id, arm] of Object.entries(raw.arms)) {
    if (!raw.worktrees[arm.worktree]) throw new Error(`arm ${id}: unknown worktree ${arm.worktree}`);
    if (!KERNELS.has(arm.kernel)) throw new Error(`arm ${id}: unknown kernel ${arm.kernel}`);
    if (arm.kernel === 'rust' && arm.rustReads) throw new Error(`arm ${id}: rustReads needs the typescript kernel`);
  }
  for (const pair of raw.pairs) {
    for (const id of pair) {
      if (!raw.arms[id]) throw new Error(`pair ${pair.join('/')}: unknown arm ${id}`);
    }
  }
  for (const key of ['baseline', 'noiseArm']) {
    if (!raw.arms[raw[key]]) throw new Error(`${key}: unknown arm ${raw[key]}`);
  }
  return { ...raw, armOrder: Object.keys(raw.arms) };
}
```

- [ ] **Step 5: Implement `scripts/pin.mjs`**

```js
// Moves every worktree's pin to the current tip of its ref. Commit the result.
import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const file = fileURLToPath(new URL('../arms.json', import.meta.url));
const raw = JSON.parse(readFileSync(file, 'utf8'));
const only = process.argv.slice(2);
for (const [id, w] of Object.entries(raw.worktrees)) {
  if (only.length && !only.includes(id)) continue;
  const sha = execFileSync('git', ['rev-parse', `${w.ref}^{commit}`], { encoding: 'utf8' }).trim();
  if (sha !== w.sha) console.log(`${id}: ${w.sha.slice(0, 9)} -> ${sha.slice(0, 9)}`);
  w.sha = sha;
}
writeFileSync(file, JSON.stringify(raw, null, 2) + '\n');
```

- [ ] **Step 6: Run the tests**

Run: `cd perf-compare && pnpm test`
Expected: 6 passing.

- [ ] **Step 7: Commit**

```bash
git add perf-compare/.gitignore perf-compare/package.json perf-compare/pnpm-lock.yaml perf-compare/arms.json perf-compare/lib/config.mjs perf-compare/scripts/pin.mjs perf-compare/test/config.test.mjs
git commit -m "Pin the three branches the viewer perf comparison builds"
```

---

### Task 2: Statistics

**Files:**
- Create: `perf-compare/lib/stats.mjs`
- Test: `perf-compare/test/stats.test.mjs`

**Interfaces:**
- Produces:
  - `summarize(values: number[]): { n, median, p95, min, max }`
  - `bootstrapRatio(reference: number[], candidate: number[], opts?: { resamples?: number, seed?: number }): { ratio, lo, hi }`, where `ratio = median(candidate) / median(reference)`
  - `noiseBand(interval: { lo, hi }): number`, the interval's widest distance from 1.0
  - `verdict(interval: { ratio, lo, hi }, noise: number): 'slower' | 'faster' | 'same'`
  - `slope(ys: number[]): number`, least squares against `x = 0..n-1`

- [ ] **Step 1: Write the failing test**

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summarize, bootstrapRatio, noiseBand, verdict, slope } from '../lib/stats.mjs';

test('summarize uses linear-interpolated quantiles', () => {
  const s = summarize([5, 1, 4, 2, 3]);
  assert.equal(s.median, 3);
  assert.equal(s.p95, 4.8);
  assert.equal(s.min, 1);
  assert.equal(s.max, 5);
  assert.equal(s.n, 5);
});

test('summarize rejects non-finite samples', () => {
  assert.throws(() => summarize([1, NaN]), /finite/);
  assert.throws(() => summarize([]), /at least one/);
});

test('bootstrap is deterministic for a seed', () => {
  const a = [10, 11, 9, 10, 12, 10, 11];
  const b = [20, 21, 19, 22, 20, 20, 21];
  assert.deepEqual(bootstrapRatio(a, b, { seed: 7 }), bootstrapRatio(a, b, { seed: 7 }));
});

test('bootstrap brackets a clear 2x', () => {
  const a = [10, 11, 9, 10, 12, 10, 11, 10, 9, 11];
  const b = a.map((x) => x * 2);
  const r = bootstrapRatio(a, b);
  assert.equal(r.ratio, 2);
  assert.ok(r.lo > 1.5 && r.hi < 2.5, JSON.stringify(r));
});

test('verdict respects the noise band and the 3% floor', () => {
  assert.equal(verdict({ ratio: 1.2, lo: 1.1, hi: 1.3 }, 0.05), 'slower');
  assert.equal(verdict({ ratio: 0.8, lo: 0.7, hi: 0.9 }, 0.05), 'faster');
  assert.equal(verdict({ ratio: 1.2, lo: 0.95, hi: 1.3 }, 0.05), 'same');
  assert.equal(verdict({ ratio: 1.04, lo: 1.01, hi: 1.06 }, 0.05), 'same');
  assert.equal(verdict({ ratio: 1.02, lo: 1.01, hi: 1.03 }, 0), 'same');
});

test('noiseBand is the widest distance from 1', () => {
  assert.equal(noiseBand({ lo: 0.96, hi: 1.02 }).toFixed(2), '0.04');
});

test('slope of a line', () => {
  assert.equal(slope([10, 12, 14, 16]), 2);
  assert.equal(slope([5, 5, 5]), 0);
});
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd perf-compare && node --test test/stats.test.mjs`
Expected: FAIL, `Cannot find module '../lib/stats.mjs'`.

- [ ] **Step 3: Implement `lib/stats.mjs`**

```js
const FLOOR = 0.03;

function quantile(sorted, q) {
  if (sorted.length === 1) return sorted[0];
  const pos = (sorted.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

function sorted(values) {
  if (!values.length) throw new Error('A summary needs at least one sample');
  if (values.some((v) => !Number.isFinite(v))) throw new Error('Samples must be finite');
  return [...values].sort((a, b) => a - b);
}

export function summarize(values) {
  const s = sorted(values);
  return { n: s.length, median: quantile(s, 0.5), p95: quantile(s, 0.95), min: s[0], max: s[s.length - 1] };
}

// mulberry32: small, seedable, and good enough for resampling indices.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const median = (values) => quantile(sorted(values), 0.5);

function resample(values, next) {
  const out = new Array(values.length);
  for (let i = 0; i < values.length; i++) out[i] = values[Math.floor(next() * values.length)];
  return out;
}

export function bootstrapRatio(reference, candidate, { resamples = 10_000, seed = 1 } = {}) {
  const next = rng(seed);
  const ratios = new Array(resamples);
  for (let i = 0; i < resamples; i++) {
    ratios[i] = median(resample(candidate, next)) / median(resample(reference, next));
  }
  ratios.sort((a, b) => a - b);
  return {
    ratio: median(candidate) / median(reference),
    lo: quantile(ratios, 0.025),
    hi: quantile(ratios, 0.975),
  };
}

export const noiseBand = ({ lo, hi }) => Math.max(Math.abs(1 - lo), Math.abs(hi - 1));

export function verdict({ ratio, lo, hi }, noise) {
  const threshold = Math.max(FLOOR, noise);
  if (lo > 1 && ratio - 1 > threshold) return 'slower';
  if (hi < 1 && 1 - ratio > threshold) return 'faster';
  return 'same';
}

export function slope(ys) {
  const n = ys.length;
  const mx = (n - 1) / 2;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let num = 0;
  let den = 0;
  ys.forEach((y, x) => {
    num += (x - mx) * (y - my);
    den += (x - mx) ** 2;
  });
  return den === 0 ? 0 : num / den;
}
```

`bootstrapRatio` divides by a resampled median, so a reference with a zero median (for example `jankFrames` on a smooth arm) would produce `Infinity`. `report.mjs` handles count metrics differently (Task 8), so that's never passed in here.

- [ ] **Step 4: Run the tests**

Run: `cd perf-compare && node --test test/stats.test.mjs`
Expected: 7 passing.

- [ ] **Step 5: Commit**

```bash
git add perf-compare/lib/stats.mjs perf-compare/test/stats.test.mjs
git commit -m "Add the bootstrap ratio and noise-band verdict for perf reports"
```

---

### Task 3: Doctor and materialize

**Files:**
- Create: `perf-compare/scripts/doctor.mjs`, `perf-compare/scripts/materialize.mjs`

**Interfaces:**
- Consumes: `loadConfig()` (Task 1).
- Produces: `workDir(): string` (in `materialize.mjs`), which is `process.env.PERF_WORK_DIR ?? <repo>/perf-compare/.work`; `worktreePath(id): string` = `<workDir>/worktrees/<id>`; `materialize(config): void`.

- [ ] **Step 1: Write `scripts/doctor.mjs`**

It checks what every recipe needs and exits non-zero with every failure listed, not just the first:

```js
import { execFileSync } from 'node:child_process';
import os from 'node:os';

const failures = [];
const warnings = [];
const run = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  } catch {
    return null;
  }
};
const need = (label, actual, expected) => {
  if (actual === null) failures.push(`${label}: not found (need ${expected})`);
  else if (!actual.includes(expected)) failures.push(`${label}: found "${actual}", need ${expected}`);
};

need('node', process.version, 'v24.20.0');
need('pnpm', run('pnpm', ['--version']), '10.34.0');
need('rustc', run('rustc', ['--version']), '1.98.0');
const targets = run('rustup', ['target', 'list', '--installed']) ?? '';
for (const t of ['wasm32-unknown-emscripten', 'wasm32-unknown-unknown']) {
  if (!targets.split('\n').includes(t)) failures.push(`rustup target ${t}: missing (rustup target add ${t})`);
}
need('wasm-bindgen', run('wasm-bindgen', ['--version']), '0.2.127');
if (run('git', ['--version']) === null) failures.push('git: not found');

if (process.platform === 'darwin') {
  const batt = run('pmset', ['-g', 'batt']) ?? '';
  if (batt.includes("'Battery Power'")) warnings.push('running on battery: plug in before a real run');
}
const freeGiB = os.freemem() / 2 ** 30;
if (freeGiB < 4) warnings.push(`only ${freeGiB.toFixed(1)} GiB free memory`);

for (const w of warnings) console.warn(`warn: ${w}`);
for (const f of failures) console.error(`fail: ${f}`);
if (failures.length) process.exit(1);
console.log('doctor: ok');
```

Emscripten isn't checked here because the version comes from each worktree. `build-arm.mjs` installs it (Task 4).

- [ ] **Step 2: Run the doctor on this machine**

Run: `node perf-compare/scripts/doctor.mjs`
Expected: `doctor: ok`, or a list of `fail:` lines. Fix each (`mise install`, `rustup target add ...`, `cargo install wasm-bindgen-cli --version 0.2.127`) until it passes.

- [ ] **Step 3: Write `scripts/materialize.mjs`**

```js
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../lib/config.mjs';

const repo = fileURLToPath(new URL('../..', import.meta.url));
export const workDir = () => process.env.PERF_WORK_DIR ?? path.join(repo, 'perf-compare/.work');
export const worktreePath = (id) => path.join(workDir(), 'worktrees', id);

const git = (args, cwd = repo) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
const hasCommit = (sha) => {
  try {
    git(['cat-file', '-e', `${sha}^{commit}`]);
    return true;
  } catch {
    return false;
  }
};

export function materialize(config) {
  mkdirSync(path.join(workDir(), 'worktrees'), { recursive: true });
  for (const [id, w] of Object.entries(config.worktrees)) {
    const dir = worktreePath(id);
    if (!existsSync(dir)) {
      if (!hasCommit(w.sha)) git(['fetch', '--quiet', 'origin', w.sha]);
      git(['worktree', 'add', '--detach', dir, w.sha]);
    }
    const head = git(['rev-parse', 'HEAD'], dir);
    if (head !== w.sha) {
      const dirty = git(['status', '--porcelain', '--untracked-files=no'], dir);
      if (dirty) throw new Error(`worktree ${id} has tracked changes; remove ${dir} and rerun`);
      git(['checkout', '--quiet', '--detach', w.sha], dir);
    }
    git(['submodule', 'update', '--init', '--recursive', '--quiet'], dir);
    console.log(`${id}: ${dir} at ${w.sha.slice(0, 9)}`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) materialize(loadConfig());
```

The submodule is `packages/engine/runtime/runtime-src`. It's only needed when a recipe builds the payload locally, but initializing it everywhere keeps the three trees alike and costs one clone the first time.

A commit that exists only on the machine that pinned it works there and fails with a `git fetch` error everywhere else. Task 9's README says to push a branch before pinning it.

- [ ] **Step 4: Run it**

Run: `node perf-compare/scripts/materialize.mjs`
Expected: three lines, one per worktree, each at its pinned short SHA. `git worktree list` shows three detached worktrees under `perf-compare/.work/worktrees/`.

- [ ] **Step 5: Commit**

```bash
git add perf-compare/scripts/doctor.mjs perf-compare/scripts/materialize.mjs
git commit -m "Check toolchains and materialize pinned worktrees for perf runs"
```

---

### Task 4: Build recipes and the cache

**Files:**
- Create: `perf-compare/scripts/build-arm.mjs`

**Interfaces:**
- Consumes: `loadConfig()`, `worktreePath(id)`, `workDir()`.
- Produces: `buildWorktree(id, config): { engineWasm: string, engineWasmSha256: string }` and a stamp file `<workDir>/stamps/<worktreeId>.json` = `{ key, engineWasm, engineWasmSha256, rustCore: boolean }`. The cache key is `sha256(worktree sha + recipe + contents of build-arm.mjs)`, so editing a recipe invalidates every worktree it touches.

The three recipes share a prefix and differ in two places:

| Step | `typescript` | `rust-core` | `rust-kernel` |
| --- | --- | --- | --- |
| `pnpm install --frozen-lockfile` | yes | yes | yes |
| Engine payload | fetch | fetch, else build locally | fetch, else build locally |
| `verify-rust-core.mjs` must pass | no | yes | no |
| Turbo build of `@embedpdf/viewer...` | yes | yes | yes |
| Kernel wasm (`build:wasm`) + eval bootstrap | no | no | yes |

"Fetch, else build locally" exists because `scripts/fetch-payload.sh` looks up a release named after a hash of the runtime sources, and the impl branch changed `build-target.sh`, so its hash may have no published payload. We record which path ran in the stamp.

- [ ] **Step 1: Write the recipe runner**

```js
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../lib/config.mjs';
import { workDir, worktreePath } from './materialize.mjs';

const self = readFileSync(fileURLToPath(import.meta.url));
const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

function sh(cmd, cwd, env = {}) {
  console.log(`  $ ${cmd}`);
  execFileSync('bash', ['-euo', 'pipefail', '-c', cmd], {
    cwd,
    stdio: 'inherit',
    env: { ...process.env, ...env },
  });
}

function emsdkVersion(dir) {
  const wf = readFileSync(path.join(dir, '.github/workflows/build-engine-runtime.yml'), 'utf8');
  const m = /EMSDK_VERSION:\s*([0-9.]+)/.exec(wf);
  if (!m) throw new Error(`${dir}: no EMSDK_VERSION in build-engine-runtime.yml`);
  return m[1];
}

function ensureEmsdk(version) {
  const root = path.join(workDir(), 'emsdk');
  if (!existsSync(root)) sh(`git clone --depth 1 https://github.com/emscripten-core/emsdk.git ${root}`, workDir());
  sh(`./emsdk install ${version} && ./emsdk activate ${version}`, root);
  return path.join(root, 'emsdk_env.sh');
}

const wasmPath = (dir) => path.join(dir, 'packages/engine/runtime/npm/wasm32/lib/embedpdf.wasm');

function payload(dir, { allowLocal }) {
  try {
    sh('pnpm --filter @embedpdf/engine-runtime fetch:payload', dir);
    return 'fetched';
  } catch (error) {
    if (!allowLocal) throw error;
    console.log('  no published payload for this tree; building wasm32 locally');
    const env = ensureEmsdk(emsdkVersion(dir));
    sh(`source ${env} && pnpm --filter @embedpdf/engine-runtime build:target wasm32`, dir);
    return 'built';
  }
}

export function buildWorktree(id, config) {
  const w = config.worktrees[id];
  const dir = worktreePath(id);
  const key = sha256(Buffer.concat([Buffer.from(w.sha + w.recipe), self]));
  const stampFile = path.join(workDir(), 'stamps', `${id}.json`);
  if (existsSync(stampFile)) {
    const stamp = JSON.parse(readFileSync(stampFile, 'utf8'));
    if (stamp.key === key && existsSync(stamp.engineWasm)) return stamp;
  }
  console.log(`building ${id} (${w.recipe})`);
  sh('pnpm install --frozen-lockfile', dir);
  const source = payload(dir, { allowLocal: w.recipe !== 'typescript' });
  let rustCore = false;
  if (w.recipe === 'rust-core') {
    sh('node packages/engine/runtime/scripts/verify-rust-core.mjs', dir);
    rustCore = true;
  }
  sh('pnpm exec turbo run build --filter=@embedpdf/viewer...', dir);
  if (w.recipe === 'rust-kernel') {
    sh('pnpm --filter @embedpdf/core-wasm build:wasm', dir);
    sh('pnpm bootstrap', path.join(dir, 'examples/rust-kernel-eval'));
  }
  const engineWasm = wasmPath(dir);
  const stamp = { key, source, rustCore, engineWasm, engineWasmSha256: sha256(readFileSync(engineWasm)) };
  mkdirSync(path.dirname(stampFile), { recursive: true });
  writeFileSync(stampFile, JSON.stringify(stamp, null, 2));
  return stamp;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const config = loadConfig();
  const only = process.argv.slice(2);
  for (const id of Object.keys(config.worktrees)) {
    if (!only.length || only.includes(id)) console.log(buildWorktree(id, config));
  }
}
```

- [ ] **Step 2: Build `main`**

Run: `node perf-compare/scripts/build-arm.mjs main`
Expected: a stamp with `"source": "fetched"` and `"rustCore": false`. If `fetch:payload` fails on `main`, stop: that's an infrastructure problem the harness shouldn't paper over, and `main` has no local-build fallback on purpose.

- [ ] **Step 3: Build impl**

Run: `node perf-compare/scripts/build-arm.mjs impl`
Expected: `verify-rust-core.mjs` passes, and the stamp has `"rustCore": true`. Record the wall-clock time of this step in the README (Task 9): the local emcc build is the slowest thing the harness does.

Check `packages/engine/runtime/engine-runtime-build.local.json` in the impl worktree before this step. Its wasm32 URL points at `file:///Users/bobsingor/...`. If `build:target wasm32` reads it and fails, the recipe needs `EMBEDPDF_RUNTIME_BUILD_CONFIG` (or whatever `fetch-libpdfium.sh` reads) pointed at the tracked release manifest. Add that to the `sh()` env in `payload()` and note it in the README rather than editing the worktree.

- [ ] **Step 4: Build plugins**

Run: `node perf-compare/scripts/build-arm.mjs plugins`
Expected: `packages/core-wasm/wasm/` exists in the plugins worktree, and the stamp records whether the payload was fetched or built.

- [ ] **Step 5: Confirm the cache**

Run: `time node perf-compare/scripts/build-arm.mjs`
Expected: three stamps printed with no `$` lines, in under five seconds.

- [ ] **Step 6: Commit**

```bash
git add perf-compare/scripts/build-arm.mjs
git commit -m "Build each perf worktree with its branch recipe, cached by sha"
```

---

### Task 5: The shared entry

Every arm runs this page. It mounts the viewer through `packages/viewer/main/src/doors/local.ts` from source, which is exactly what `examples/rust-kernel-eval/src/main.ts` and `packages/viewer/main/src/dev.ts` do, so the only differences between arms are the ones `arms.json` asks for.

**Files:**
- Create: `perf-compare/entry/index.html`, `perf-compare/entry/main.ts`, `perf-compare/entry/kernel-typescript.ts`, `perf-compare/entry/kernel-rust.ts`, `perf-compare/entry/vite.config.mjs`
- Modify: `perf-compare/scripts/build-arm.mjs` (add `buildEntry`)

**Interfaces:**
- Consumes: `buildWorktree()` stamps.
- Produces: `buildEntry(armId, config): string` returning `<workDir>/dist/<armId>/`. The page exposes `window.__perf`:

```ts
interface PerfBridge {
  arm: { id: string; kernel: 'typescript' | 'rust'; rustReads: boolean };
  ready: Promise<void>;                       // epdf:ready
  opened: Promise<void>;                      // first 'opened'
  pageCount(): number;
  goToPageIndex(i: number): void;
  zoomTo(z: number): void;
  zoomLevel(): number;
  search(text: string): Promise<number>;      // resolves with the hit count
  readSelection(): Promise<string>;
  annotationCount(pageIndex: number): number;
  execute(command: string): void;
  kernelCounts(): Record<string, number> | null;
}
```

- [ ] **Step 1: Write the page**

`perf-compare/entry/index.html`:

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>perf-compare</title>
    <style>html, body, #viewer { margin: 0; width: 100vw; height: 100vh; }</style>
  </head>
  <body>
    <div id="viewer"></div>
    <script type="module" src="./main.ts"></script>
  </body>
</html>
```

`perf-compare/entry/kernel-typescript.ts`:

```ts
export async function setupKernel(): Promise<void> {}
export const kernelCounts = (): Record<string, number> | null => null;
```

`perf-compare/entry/kernel-rust.ts` (copied into `<plugins>/examples/rust-kernel-eval/perf-kernel-rust.ts` at build time, so `@embedpdf/core-wasm` resolves from that package's `node_modules`):

```ts
import { kernelCounts as counts } from '@embedpdf/core-wasm';
import { initWasm } from '@embedpdf/core-wasm/browser';
import { installHosts } from './src/hosts';

export async function setupKernel(): Promise<void> {
  await initWasm();
  installHosts();
}
export const kernelCounts = (): Record<string, number> | null => counts();
```

`perf-compare/entry/main.ts`:

```ts
import EmbedPDF, { StageToken, SearchToken, SelectionToken, AnnotationToken } from '@perf/door';
import { setupKernel, kernelCounts } from '@perf/kernel';

declare const __PERF_ARM__: { id: string; kernel: 'typescript' | 'rust'; rustReads: boolean };
const arm = __PERF_ARM__;
const doc = new URLSearchParams(location.search).get('doc') ?? 'ebook';

await setupKernel();

const el = EmbedPDF.init({
  target: '#viewer',
  src: `/${doc}.pdf`,
  // `registerLocalEngine` spreads this bag into `localEngine()`, which is
  // where `rustRouting` is read on the impl branch. Other branches ignore it.
  ...(arm.rustReads ? { engine: { rustRouting: { rustReads: true } } as never } : {}),
});

const ready = new Promise<void>((resolve) =>
  el.addEventListener('epdf:ready', () => {
    performance.mark('perf:ready');
    resolve();
  }, { once: true }),
);

const opened = ready.then(
  () =>
    new Promise<void>((resolve) => {
      const off = el.viewer!.on('opened', () => {
        performance.mark('perf:opened');
        off();
        resolve();
      });
    }),
);

const v = () => el.viewer!;
window.__perf = {
  arm,
  ready,
  opened,
  pageCount: () => v().get(StageToken).listCurrentItemPages().length,
  goToPageIndex: (i) => v().get(StageToken).goToPageIndex(i, { behavior: 'instant' } as never),
  zoomTo: (z) => v().get(StageToken).zoomTo(z),
  zoomLevel: () => v().get(StageToken).getZoomLevel(),
  search: async (text) => {
    await v().get(SearchToken).search({ text });
    return v().get(SearchToken).getHitCount();
  },
  readSelection: () => v().get(SelectionToken).readText(),
  annotationCount: (pageIndex) => {
    const stage = v().get(StageToken);
    const page = stage.listCurrentItemPages()[pageIndex];
    return v().get(AnnotationToken).list(page.pon).length;
  },
  execute: (command) => v().execute(command),
  kernelCounts,
};
```

`pageCount` via `listCurrentItemPages()` and the `goToPageIndex` options type are guesses at the lens surface from `packages/plugin/stage/src/contract.ts:446` and `:479` on `main`. Step 4 typechecks this file against all three worktrees, which is where a wrong name shows up. Fix it against `main`'s contract and confirm the other two agree.

- [ ] **Step 2: Write the Vite config**

`perf-compare/entry/vite.config.mjs` runs with its working directory set to `<worktree>/packages/viewer/main`, so `vite`, `preact` and `@tailwindcss/vite` resolve from that package, as they do for `packages/viewer/main/vite.config.ts`:

```js
import { existsSync } from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

const arm = JSON.parse(process.env.PERF_ARM);
const worktree = process.env.PERF_WORKTREE;
const viewerMain = path.join(worktree, 'packages/viewer/main');
const require = createRequire(path.join(viewerMain, 'package.json'));
const preact = (s) => require.resolve(s);

// Copied from examples/rust-kernel-eval/vite.config.ts on the plugins branch.
// Kept here, not imported, so the other two worktrees build without it.
function rustKernel() {
  const coreWasm = path.join(worktree, 'packages/core-wasm');
  const redirects = path.join(coreWasm, 'src/redirect');
  const REDIRECTED = /^@embedpdf\/(core|plugin-[a-z0-9-]+)$/;
  return {
    name: 'rust-kernel',
    enforce: 'pre',
    resolveId(id, importer) {
      const match = REDIRECTED.exec(id);
      if (!match || !importer) return null;
      const importerPath = importer.replace(/^\0/, '').replace(/[?#].*$/, '');
      if (path.resolve(importerPath).startsWith(coreWasm + path.sep)) return null;
      const target = path.join(redirects, `${match[1]}.ts`);
      if (!existsSync(target)) throw new Error(`[rust-kernel] no redirect for ${id}`);
      return target;
    },
  };
}

const kernel =
  arm.kernel === 'rust'
    ? path.join(worktree, 'examples/rust-kernel-eval/perf-kernel-rust.ts')
    : path.join(process.env.PERF_ENTRY, 'kernel-typescript.ts');

export default defineConfig({
  root: process.env.PERF_ENTRY,
  publicDir: process.env.PERF_FIXTURES,
  plugins: [...(arm.kernel === 'rust' ? [rustKernel()] : []), tailwindcss()],
  resolve: {
    alias: [
      { find: '@perf/door', replacement: path.join(viewerMain, 'src/doors/local.ts') },
      { find: '@perf/kernel', replacement: kernel },
      { find: 'react-dom/client', replacement: preact('preact/compat/client') },
      { find: 'react-dom', replacement: preact('preact/compat') },
      { find: 'react/jsx-runtime', replacement: preact('preact/jsx-runtime') },
      { find: 'react/jsx-dev-runtime', replacement: preact('preact/jsx-dev-runtime') },
      { find: /^react$/, replacement: preact('preact/compat') },
    ],
  },
  define: {
    __PERF_ARM__: JSON.stringify(arm),
    'process.env.NODE_ENV': JSON.stringify('production'),
  },
  build: {
    outDir: process.env.PERF_OUT,
    emptyOutDir: true,
    target: 'es2022',
    sourcemap: false,
  },
});
```

The `react*` aliases mirror `packages/viewer/main/vite.config.ts` on `main`. If that file's alias list differs on either research branch, copy the union here and say so in a comment.

- [ ] **Step 3: Add `buildEntry` to `build-arm.mjs`**

```js
const entryDir = fileURLToPath(new URL('../entry', import.meta.url));
const fixturesDir = fileURLToPath(new URL('../fixtures', import.meta.url));

export function buildEntry(armId, config) {
  const arm = { id: armId, ...config.arms[armId] };
  const dir = worktreePath(arm.worktree);
  const out = path.join(workDir(), 'dist', armId);
  if (arm.kernel === 'rust') {
    copyFileSync(path.join(entryDir, 'kernel-rust.ts'), path.join(dir, 'examples/rust-kernel-eval/perf-kernel-rust.ts'));
  }
  sh(`pnpm exec vite build --config ${path.join(entryDir, 'vite.config.mjs')}`, path.join(dir, 'packages/viewer/main'), {
    PERF_ARM: JSON.stringify({ id: arm.id, kernel: arm.kernel, rustReads: arm.rustReads }),
    PERF_WORKTREE: dir,
    PERF_ENTRY: entryDir,
    PERF_FIXTURES: fixturesDir,
    PERF_OUT: out,
  });
  return out;
}
```

Add `copyFileSync` to the `node:fs` import. Extend the CLI block so `node scripts/build-arm.mjs` also runs `buildEntry` for every arm after its worktree builds. The entry is rebuilt every run: it takes seconds, and caching it would hide edits to `entry/`.

- [ ] **Step 4: Typecheck the entry against each worktree**

`tsc` doesn't take `paths` on the command line, so add a `typecheckEntry(armId, config)` helper to `build-arm.mjs` that writes `<workDir>/tsconfig/<armId>.json` and runs `pnpm exec tsc -p <that file>` from `<worktree>/packages/viewer/main`. Call it at the top of `buildEntry`.

```js
function typecheckEntry(armId, config) {
  const arm = config.arms[armId];
  const dir = worktreePath(arm.worktree);
  const viewerMain = path.join(dir, 'packages/viewer/main');
  const kernel =
    arm.kernel === 'rust'
      ? path.join(dir, 'examples/rust-kernel-eval/perf-kernel-rust.ts')
      : path.join(entryDir, 'kernel-typescript.ts');
  const file = path.join(workDir(), 'tsconfig', `${armId}.json`);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(
    file,
    JSON.stringify({
      compilerOptions: {
        noEmit: true,
        strict: true,
        skipLibCheck: true,
        target: 'es2022',
        module: 'esnext',
        moduleResolution: 'bundler',
        jsx: 'react-jsx',
        types: [],
        baseUrl: viewerMain,
        paths: { '@perf/door': ['src/doors/local.ts'], '@perf/kernel': [kernel] },
      },
      files: [path.join(entryDir, 'main.ts'), path.join(entryDir, 'globals.d.ts')],
    }),
  );
  sh(`pnpm exec tsc -p ${file}`, viewerMain);
}
```

Add `perf-compare/entry/globals.d.ts` declaring `window.__perf: PerfBridge` and `window.__probe`, with the `PerfBridge` interface above. On the plugins arm, `kernel-rust.ts` has to be copied into the worktree before this runs, so move the copy to the top of `typecheckEntry`.

Run: `node perf-compare/scripts/build-arm.mjs`
Expected: `tsc` passes for all five arms. A method-name error means the lens surface differs between branches. The fix goes in `main.ts`, and the README records the difference.

- [ ] **Step 5: Build all five entries and look at them**

Run: `node perf-compare/scripts/build-arm.mjs`, then `npx serve -l 5301 perf-compare/.work/dist/plugins` and open `http://localhost:5301/?doc=ebook`.
Expected: the viewer paints `ebook.pdf`. On the plugins arm, `window.__perf.kernelCounts()` returns an object in the console; on every other arm it returns `null`. `window.__perf.search('the')` resolves to the same count on every arm.

`serve` is for this manual look only. The harness uses its own server (Task 6).

- [ ] **Step 6: Commit**

```bash
git add perf-compare/entry perf-compare/scripts/build-arm.mjs
git commit -m "Build one viewer entry into every perf arm"
```

---

### Task 6: Server, probe, CDP and the memory self-test

**Files:**
- Create: `perf-compare/lib/serve.mjs`, `perf-compare/lib/probe.js`, `perf-compare/lib/cdp.mjs`, `perf-compare/lib/env.mjs`
- Create: `perf-compare/fixtures/generate.mjs`, `perf-compare/fixtures/fixtures.json`, `perf-compare/fixtures/large.pdf`, `perf-compare/fixtures/ebook.pdf`
- Test: `perf-compare/test/serve.test.mjs`, `perf-compare/test/env.test.mjs`, `perf-compare/test/memory.selftest.mjs`

**Interfaces:**
- Produces:
  - `serveArms(dists: Record<armId, dir>): Promise<{ urls: Record<armId, string>, close(): Promise<void> }>`, ports from 5301 upward in `armOrder`
  - `installProbe` (the `probe.js` source, used with `context.addInitScript({ path })`); in the page: `window.__probe = { firstPaintAt, settledAt(sinceMs) : Promise<number>, frames(): number[], longTasks(): {start, duration}[], reset() }`
  - `openCdp(page): Promise<Cdp>` with `metrics(): Promise<{ taskMs, scriptMs }>`, `workerMetrics(): Promise<{ taskMs } | null>`, `gc(): Promise<void>`, `memory(): Promise<{ total, worker, source: 'uasm' | 'cdp' }>`, `throttle(rate: number): Promise<void>`
  - `collectEnv({ config, stamps, fixtures, browserVersion, profile }): Meta`

- [ ] **Step 1: Fixtures**

`fixtures/generate.mjs` writes `large.pdf`: 400 US Letter pages, each with 40 lines of Courier text that include the page number and the word `the` a fixed number of times, and one filled rectangle so every page has vector content. Use the same hand-written object writer as `tooling/viewer-e2e/fixtures/generate.mjs` (copy the `build()` function, attributed in a comment) so no PDF library computes the file. Commit the output, so a change to the generator shows up as a changed hash rather than a silently different fixture.

Copy `ebook.pdf` from `git show 2516e2786:packages/viewer/main/public/ebook.pdf` (909,039 bytes). `fixtures.json` maps each name to its sha256; `env.mjs` recomputes the hashes at run start and fails on a mismatch.

- [ ] **Step 2: Server, with a test**

`lib/serve.mjs` is a `node:http` static server, one listener per arm, that sends these headers on every response and nothing else varies by arm:

```js
const HEADERS = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
  'Cross-Origin-Resource-Policy': 'same-origin',
  'Cache-Control': 'no-store',
};
```

Content types: `.html`, `.js`, `.mjs`, `.css`, `.wasm` (`application/wasm`, required for streaming compile), `.pdf`, `.ttf`, `.json`. Unknown paths return 404, never `index.html`, so a missing asset fails loudly.

`test/serve.test.mjs` serves a temp dir with an `index.html` and a `.wasm`, fetches both, and asserts the four headers and `application/wasm`.

- [ ] **Step 3: Probe**

`lib/probe.js` is an init script, so it runs before the entry. It must not import anything:

```js
(() => {
  const state = { firstPaintAt: null, frames: [], longTasks: [], lastChange: performance.now() };
  let last = null;
  const tick = (t) => {
    if (last !== null) state.frames.push(t - last);
    last = t;
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  new PerformanceObserver((list) => {
    for (const e of list.getEntries()) state.longTasks.push({ start: e.startTime, duration: e.duration });
  }).observe({ type: 'longtask', buffered: true });

  const root = () => document.querySelector('embedpdf-viewer')?.shadowRoot ?? null;
  const visibleImages = () =>
    [...(root()?.querySelectorAll('img[draggable="false"]') ?? [])].filter((img) => {
      const r = img.getBoundingClientRect();
      return r.width > 0 && r.height > 0 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
    });
  const allDecoded = () => {
    const imgs = visibleImages();
    return imgs.length > 0 && imgs.every((img) => img.complete && img.naturalWidth > 0);
  };

  // Watch the shadow root once it exists: any img added or re-sourced is a change.
  const attach = () => {
    const r = root();
    if (!r) return requestAnimationFrame(attach);
    new MutationObserver(() => (state.lastChange = performance.now())).observe(r, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['src'],
    });
    const firstPaint = () => {
      if (allDecoded()) requestAnimationFrame((t) => (state.firstPaintAt ??= t));
      else requestAnimationFrame(firstPaint);
    };
    firstPaint();
  };
  attach();

  const QUIET_MS = 250;
  window.__probe = {
    get firstPaintAt() {
      return state.firstPaintAt;
    },
    // Resolves with the time of the last change once the page has been quiet
    // for QUIET_MS with every visible image decoded. Not the detection time.
    settledAt: (sinceMs) =>
      new Promise((resolve, reject) => {
        const deadline = performance.now() + 30_000;
        const check = () => {
          const now = performance.now();
          if (now > deadline) return reject(new Error('page never settled'));
          if (now - state.lastChange >= QUIET_MS && allDecoded()) return resolve(Math.max(state.lastChange, sinceMs));
          setTimeout(check, 16);
        };
        check();
      }),
    frames: () => state.frames.slice(),
    longTasks: () => state.longTasks.slice(),
    reset: () => {
      state.frames = [];
      state.longTasks = [];
      last = null;
    },
  };
})();
```

The same caveat as `tooling/viewer-e2e/performance.ts` applies and goes in the README: "first paint" is a frame opportunity after a decoded image, not a compositor timestamp.

- [ ] **Step 4: CDP helpers**

`lib/cdp.mjs` opens a CDP session on the page with `page.context().newCDPSession(page)`, enables `Performance`, and calls `Target.setAutoAttach({ autoAttach: true, waitForDebuggerOnStart: false, flatten: true })` to get a session for the engine worker. Playwright also exposes `page.workers()`; use `context.newCDPSession(worker)` on the one whose URL contains `embedpdf-worker` when it's available.

`metrics()` reads `Performance.getMetrics` and returns `TaskDuration` and `ScriptDuration` in ms (CDP reports seconds). `workerMetrics()` tries the same on the worker session and returns `null` if `Performance.enable` is rejected there; the report then prints `n/a` for `workerTaskMs` rather than zero.

`gc()` sends `HeapProfiler.collectGarbage` to the page and the worker session.

`memory()` calls `gc()`, then evaluates `performance.measureUserAgentSpecificMemory()` in the page and sums `bytes` for the whole result and for breakdown entries whose `attribution[].scope` is `DedicatedWorkerGlobalScope`. If `crossOriginIsolated` is false or the call throws, it falls back to `Runtime.getHeapUsage` on each session and sets `source: 'cdp'`.

`throttle(rate)` sends `Emulation.setCPUThrottlingRate({ rate })`.

- [ ] **Step 5: The memory self-test**

`test/memory.selftest.mjs` launches Chromium with the harness flags, serves a page with only this script, and asserts the `uasm` total grows by at least 64 MiB:

```js
const before = await cdp.memory();
await page.evaluate(() => {
  window.__keep = new WebAssembly.Memory({ initial: 1, maximum: 2048 });
  window.__keep.grow(1024); // 1024 pages of 64 KiB = 64 MiB
  new Uint8Array(window.__keep.buffer).fill(1);
});
const after = await cdp.memory();
assert.equal(after.source, 'uasm');
assert.ok(after.total - before.total >= 64 * 2 ** 20, `grew ${after.total - before.total}`);
```

Run: `node --test perf-compare/test/memory.selftest.mjs`
Expected: PASS. If it fails on `source`, cross-origin isolation isn't on: check the headers. If it fails on the size, UASM isn't counting wasm memory in this Chromium, and the plan's fallback applies: set `memory()` to always return `source: 'cdp'` and make `report.mjs` label memory rows "JS heap only, wasm linear memory not included".

- [ ] **Step 6: `lib/env.mjs`, with a test**

`collectEnv` returns the fields listed under "Environment" above. `test/env.test.mjs` asserts every field is present and non-empty, and that `compatible(metaA, metaB)` (also exported) returns a list of differing keys among `browserVersion`, `cpuModel`, `fixtures`, `profile`, `viewport`, `deviceScaleFactor`, `flows`.

- [ ] **Step 7: Run all Node tests**

Run: `cd perf-compare && pnpm test && node --test test/memory.selftest.mjs`
Expected: all passing.

- [ ] **Step 8: Commit**

```bash
git add perf-compare/lib perf-compare/fixtures perf-compare/test
git commit -m "Add the perf server, page probe, CDP helpers and fixtures"
```

---

### Task 7: Gate, flows and the runner

**Files:**
- Create: `perf-compare/lib/gate.mjs`, `perf-compare/lib/flows.mjs`, `perf-compare/run.mjs`

**Interfaces:**
- Consumes: everything above.
- Produces: `results/<run-id>/meta.json`, `results/<run-id>/gate.json`, `results/<run-id>/samples.jsonl`. One sample line is `{ arm, flow, fixture, round, metrics: Record<string, number>, error?: string }`. `run-id` is `<ISO date>-<hostname>-<profile>`.

- [ ] **Step 1: Gate**

`lib/gate.mjs` opens each arm once on each fixture and records: `pageCount`, `search('the')` hit count on `ebook`, the text returned by `readSelection()` after the `select` flow's drag, `annotationCount(1)` before and after the `annotate` flow, and a sha256 of a 200×200 screenshot of page 1 at zoom 1.

Answers must match across arms exactly, except the screenshot, where arms on the same engine wasm must match exactly and arms on different wasm must be within 1% of differing pixels (PDFium in the Rust-core payload is the same fork, but a relink can move anti-aliasing by a pixel). An arm that fails the gate is excluded from timing, `gate.json` records why, and the report prints the arm as `excluded: <reason>` rather than dropping it silently.

The gate also asserts two things about the arms themselves: `window.__perf.arm` matches `arms.json`, and on `plugins`, `kernelCounts()` is non-null (the Rust kernel really mounted). For `impl-rust`, the stamp's `rustCore: true` is the check: the viewer can't ask the worker which reader served a page, as `examples/viewer-react/src/e2e.tsx` explains.

- [ ] **Step 2: Flows**

`lib/flows.mjs` exports one async function per flow with the signature `(page, cdp) => Promise<Record<string, number>>`. Each one records `const t0 = await page.evaluate(() => performance.now())` before its action and reads `__probe.settledAt(t0)` after, so every time is page-clock minus page-clock and never mixes Node's clock in. Each flow also takes `metrics()` before and after and returns the deltas as `mainTaskMs` and `mainScriptMs`.

`scroll`:

```js
export async function scroll(page, cdp) {
  const box = await page.locator('#viewer').boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.evaluate(() => window.__probe.reset());
  const before = await cdp.metrics();
  const t0 = await page.evaluate(() => performance.now());
  for (let i = 0; i < 60; i++) {
    await page.mouse.wheel(0, 240);
    await page.waitForTimeout(16);
  }
  const tLast = await page.evaluate(() => performance.now());
  const settled = await page.evaluate((t) => window.__probe.settledAt(t), tLast);
  const after = await cdp.metrics();
  const { frames, longTasks } = await page.evaluate(() => ({
    frames: window.__probe.frames(),
    longTasks: window.__probe.longTasks(),
  }));
  const s = summarize(frames);
  const inFlow = longTasks.filter((l) => l.start >= t0);
  return {
    settleMs: settled - tLast,
    wallMs: settled - t0,
    frameP50Ms: s.median,
    frameP95Ms: s.p95,
    jankFrames: frames.filter((f) => f > 50).length,
    longTasks: inFlow.length,
    longTaskMs: inFlow.reduce((a, l) => a + l.duration, 0),
    mainTaskMs: after.taskMs - before.taskMs,
    mainScriptMs: after.scriptMs - before.scriptMs,
  };
}
```

The rest follow the table under "Flows". `select` and `annotate` compute their drag coordinates from `StageToken.pageRectToViewport` on page index 1 plus the entry's shadow-root offset, exposed through a `__perf.pageBox(i)` helper added to `main.ts` in this step, so the drag hits the same text on every arm regardless of layout. `annotate` waits for `annotationCount(1)` to grow by one with a 10 s timeout. `soak` returns `heapCycle5Bytes`, `heapCycle30Bytes` and `heapSoakSlopeBytes` via `slope()` over all 30 `memory().total` samples, and on the plugins arm writes `kernelCounts()` at cycles 5 and 30 into the sample as `diagnostics`.

`open` is different from the others: it creates the page, navigates, and reads `performance.getEntriesByName('perf:ready')[0].startTime` and friends, which are already relative to navigation start.

- [ ] **Step 3: Runner**

`run.mjs` takes `--rounds N` (default 10), `--quick` (3 rounds), `--profile desktop|slow`, `--arms a,b,...`, `--flows a,b,...`. It:

1. Loads config, verifies fixture hashes, reads stamps, serves `dist/<arm>` for each arm.
2. Launches Chromium once via `playwright-core` with `--enable-blink-features=ForceEagerMeasureMemory` and `--js-flags=--expose-gc`, `headless: true`, and exits with a message naming `npx playwright-core install chromium` if the browser is missing.
3. Runs the gate. Writes `gate.json`.
4. For round `r` in `0..R-1`, for each flow, for each arm in `armOrder` rotated left by `r`: new context (viewport 1280×800, `deviceScaleFactor: 2`), `addInitScript({ path: probe.js })`, open the flow's fixture, wait for `__perf.opened` and a settle, apply the profile's throttle, run the flow once unmeasured (except `open`), run it measured, append the sample line, close the context.
5. Writes `meta.json` last, with `completedAt`, so a run killed half way is recognizable by the missing file.

A flow that throws writes a sample with `error` and no metrics. It doesn't stop the run.

- [ ] **Step 4: Run it quick on two arms**

Run: `node perf-compare/run.mjs --quick --arms main,main-aa`
Expected: `gate.json` shows both arms passing, `samples.jsonl` has `3 rounds × 8 flows × 2 arms = 48` lines with no `error` field, and wall time is printed at the end.

- [ ] **Step 5: Run it quick on all arms**

Run: `node perf-compare/run.mjs --quick`
Expected: 120 sample lines. Any arm excluded by the gate is a finding about that branch, not a harness bug, unless the reason is a harness-side error (a missing `__perf` method, a timeout in the probe). Fix harness-side causes; record branch-side ones.

- [ ] **Step 6: Commit**

```bash
git add perf-compare/lib/gate.mjs perf-compare/lib/flows.mjs perf-compare/run.mjs perf-compare/entry/main.ts
git commit -m "Gate every perf arm on the same answers, then run interleaved rounds"
```

---

### Task 8: Report

**Files:**
- Create: `perf-compare/report.mjs`

**Interfaces:**
- Consumes: `results/<run-id>/`, `stats.mjs`, `env.compatible()`.
- Produces: `results/<run-id>/report.md` and stdout; `--publish` also writes `perf-compare/RESULTS.md`; `--against <run-id>` compares the same arm across two runs.

- [ ] **Step 1: Write the report**

The report has these sections, in order, and each table's rows are `flow / metric`:

1. A header line with run id, rounds, profile, and "quick, not for publication" when `--quick` was used.
2. Environment, from `meta.json`, including each arm's SHA and engine wasm sha256.
3. Gate: one line per arm, `pass` or `excluded: <reason>`.
4. Sizes: one row per size metric, one column per arm. No statistics: sizes are deterministic.
5. One table per pair in `arms.json`, columns `reference median [p95]`, `candidate median [p95]`, `ratio [95% CI]`, `verdict`. The noise band for each metric comes from the `main` vs `main-aa` pair of the same run.
6. Memory: `heapAfterOpenBytes`, `workerHeapAfterOpenBytes`, `heapSoakSlopeBytes` per arm, median and p95, labeled "JS heap only" when `meta.memorySource` is `cdp`.
7. Diagnostics: the plugins arm's kernel counts at soak cycles 5 and 30.

Count metrics (`jankFrames`, `longTasks`) are reported as median and p95 per arm with a difference of medians, not a ratio, because a median of zero makes the ratio undefined.

`--against <other>` refuses to run when `compatible()` returns anything, printing each differing key. It then prints one table per arm present in both runs, reference being the other run. This is the "did my change on the plugins branch help" mode, and why pins are commits: a developer moves the `plugins` pin, reruns, and compares.

- [ ] **Step 2: Run it on the quick run**

Run: `node perf-compare/report.mjs results/<run-id>`
Expected: every section present, the `main` vs `main-aa` table showing `same` on every row (if it doesn't at `R = 3`, that's expected noise and exactly why the real run uses 10), and `report.md` written.

- [ ] **Step 3: Check determinism**

Run the same command again and `diff` the two `report.md` files.
Expected: identical.

- [ ] **Step 4: Commit**

```bash
git add perf-compare/report.mjs
git commit -m "Report perf ratios per pair with bootstrap intervals and a noise band"
```

---

### Task 9: One command, the README and the first real run

**Files:**
- Create: `perf-compare/perf.mjs`, `perf-compare/README.md`
- Create (by `--publish`): `perf-compare/RESULTS.md`

- [ ] **Step 1: `perf.mjs`**

```js
// doctor -> materialize -> build -> run -> report. Any flag after the first
// `--` goes to run.mjs.
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
const node = (script, args = []) =>
  execFileSync(process.execPath, [script, ...args], { cwd: here, stdio: 'inherit' });

const sep = process.argv.indexOf('--');
const runArgs = sep === -1 ? [] : process.argv.slice(sep + 1);
node('scripts/doctor.mjs');
node('scripts/materialize.mjs');
node('scripts/build-arm.mjs');
node('run.mjs', runArgs);
node('report.mjs', ['--latest']);
```

Add `--latest` to `report.mjs`: pick the newest directory under `results/` that has a `meta.json`.

- [ ] **Step 2: README**

`perf-compare/README.md` covers, in the house voice (load the `writing-style` skill first): what the five arms are and which pair answers which question; the one command (`cd perf-compare && pnpm install --ignore-workspace && pnpm compare`); measured first-run build time and a full `R = 10` run time from Step 3 below, stated as measured on a named machine; disk use of `.work/`; how to move a pin (`node scripts/pin.mjs plugins`, commit `arms.json`); how to compare before and after a change (`--against`); how to add an arm; the caveats (first paint is a frame opportunity, `jankFrames` is rAF-based and headless, memory source, the `slow` profile is CPU throttling not a real device); and that a pinned commit must exist on `origin`.

- [ ] **Step 3: The first real run**

Run: `cd perf-compare && pnpm compare -- --rounds 10 --profile desktop`, then again with `--profile slow`.
Expected: both complete, and in both `main` vs `main-aa` is `same` on at least 90% of rows. If more than 10% of A/A rows say `slower` or `faster`, the harness has a systematic bias between two identical builds (port, order, or build nondeterminism). Find it before publishing anything.

- [ ] **Step 4: Publish**

Run: `node report.mjs --latest --publish`
Expected: `perf-compare/RESULTS.md` written. Read it before committing. The write-up paragraph at its top is hand-written, not generated: which pairs moved, by how much, and which rows are `same`.

- [ ] **Step 5: Commit**

```bash
git add perf-compare/perf.mjs perf-compare/report.mjs perf-compare/README.md perf-compare/RESULTS.md
git commit -m "Run the viewer perf comparison with one command and publish the first result"
```

---

## What this plan doesn't cover

We're not measuring native platforms. `platforms/apple` and `platforms/android` exist on the impl branch, but the comparison with `main` only makes sense on the web, the one platform all three branches ship.

We're not adding a CI workflow yet. A `workflow_dispatch` workflow only shows up once the file is on the default branch, and shared runners are too noisy for headline numbers anyway. If this harness moves to `main`, a manual-dispatch workflow on a macOS runner that uploads `results/` as an artifact is the natural follow-up, labeled smoke-only.

Firefox and WebKit are out. `measureUserAgentSpecificMemory` and CDP are Chromium-only, and adding browsers multiplies run time by three for a question (does Rust-in-wasm cost more than TypeScript) whose answer shouldn't depend much on the engine. That's a guess, and a cheap one to test later by adding a `--browser` flag to `run.mjs` with memory and CPU metrics marked `n/a`.

Engine-level numbers for `pages.geometry` in isolation already exist in `tooling/engine-baseline/RESULTS.md`. This harness measures what a user of the viewer would see, which includes the worker hop that `engine-baseline` deliberately leaves out.
