/**
 * The workload loop, shared by all three POCs. Each POC supplies only an
 * adapter; this file decides what gets run, at what sizes, and against which
 * views, so the three cannot drift apart.
 */
import { measure, normalizeDisplayList } from './harness.mjs';
import {
  ALLOC_FRAMES,
  ALLOC_SIZE,
  FRAME_SIZES,
  GESTURE_MOVES,
  GESTURE_SIZES,
  HIT_SIZES,
  MISS_POINT,
  PROBE,
  VIEW_FLAT,
  VIEW_PROJECTING,
  seedLayout,
} from './seeds.mjs';

const VIEWS = [
  ['flat', VIEW_FLAT],
  ['projecting', VIEW_PROJECTING],
];

/** Centre of the TOPMOST item, so hit-testing exits on its first check. */
function topCentre(items) {
  const q = items[items.length - 1].quad;
  return { x: (q[0].x + q[2].x) / 2, y: (q[0].y + q[2].y) / 2 };
}

/**
 * A closed loop of pointer positions: out and back, ending where it started.
 *
 * The ending matters. `pointerMove` sets the rect from the DOWN origin plus the
 * delta, so a gesture whose last move is not the down point leaves the shape
 * displaced. Repeat that a few thousand times and the shape walks off the page,
 * every subsequent `pointerDown` misses, and we end up timing the no-drag path
 * instead of a drag.
 */
function moveLoop(down, count) {
  const out = [];
  for (let i = 1; i <= count; i++) {
    out.push({ x: down.x + Math.sin((i / count) * Math.PI * 2) * 20, y: down.y });
  }
  return out;
}

/**
 * The fairness probe, on its own, so the gate can run before any timing.
 *
 * This used to be an untimed block at the top of `runAll`, which meant the gate
 * in `bench/run.mjs` could only read it back out of a round-1 timing file: by
 * the time a disagreement could be caught, all three POCs had already run a
 * full round and written their results. Building one 10-shape model and
 * reading one display list is cheap enough to pay for in its own pass.
 */
export async function runProbe(adapter) {
  await adapter.init();
  const s = adapter.build(seedLayout(PROBE.n, PROBE.anchored));
  const probe = normalizeDisplayList(s.displayList(PROBE.view));
  s.dispose();
  return { adapter: adapter.name, probe };
}

export async function runAll(adapter, { budgetMs = 500 } = {}) {
  await adapter.init();

  const results = { adapter: adapter.name, frame: [], frameRaw: [], hit: [], gesture: [] };

  // --- per-frame projection ---------------------------------------------
  for (const n of FRAME_SIZES) {
    for (const anchored of [false, true]) {
      const s = adapter.build(seedLayout(n, anchored));
      for (const [viewName, view] of VIEWS) {
        const m = measure(() => s.frame(view), { budgetMs });
        results.frame.push({ n, anchored, view: viewName, ...m });
        // Only rust-geometry-and-state has an undecoded form. Skipping it elsewhere is the point:
        // there is no buffer to read in an all-TypeScript build.
        if (typeof s.frameRaw === 'function') {
          const raw = measure(() => s.frameRaw(view), { budgetMs });
          results.frameRaw.push({ n, anchored, view: viewName, ...raw });
        }
      }
      s.dispose();
    }
  }

  // --- hit-test ---------------------------------------------------------
  for (const n of HIT_SIZES) {
    const s = adapter.build(seedLayout(n, true));
    const hitPoint = topCentre(s.displayList(VIEW_PROJECTING));
    for (const [kind, pt] of [
      ['hit', hitPoint],
      ['miss', MISS_POINT],
    ]) {
      const m = measure(() => s.hit(pt, VIEW_PROJECTING), { budgetMs });
      results.hit.push({ n, kind, ...m });
    }
    s.dispose();
  }

  // --- dispatch, as a synthetic gesture ---------------------------------
  for (const n of GESTURE_SIZES) {
    const s = adapter.build(seedLayout(n, false));
    const down = topCentre(s.displayList(VIEW_FLAT));
    const moves = moveLoop(down, GESTURE_MOVES);
    // Twice the budget: one iteration is 102 dispatches, so batches are coarse.
    const m = measure(() => s.gesture(down, VIEW_FLAT, moves), { budgetMs: budgetMs * 2 });
    results.gesture.push({
      n,
      moves: GESTURE_MOVES,
      nsPerMove: m.ns / GESTURE_MOVES,
      ...m,
    });
    s.dispose();
  }

  return results;
}

/**
 * JavaScript allocation pressure, as heap and ArrayBuffer bytes per frame.
 *
 * Getting this wrong twice was instructive, so both dead ends are recorded here.
 *
 * First attempt counted scavenges through `PerformanceObserver({entryTypes:['gc']})`
 * and read back almost nothing, which was blamed on Node not emitting an entry
 * per scavenge. That was wrong, and the same mistake is why the check below
 * used to be dead: Node delivers `gc` entries to the observer several
 * event-loop TURNS after the collection, and never on the turn that triggered
 * it. Drained with four `setTimeout(0)` turns, a loop churning two million
 * objects on Node v24.20.0 reports 15 entries, all MINOR. What was right in
 * that post-mortem is the kind constants, which are not what you would guess:
 * MINOR=1 but MAJOR=4 and INCREMENTAL=8, so the original mapping mislabelled
 * everything it did see.
 *
 * Second attempt measured `heapUsed` growth over 2,000 frames. That is
 * confounded by its own premise: 2,000 frames allocate enough to trigger
 * collections, those collections free the garbage mid-loop, and the net delta
 * came out at 150 bytes per frame for work that allocates five objects per
 * shape. It was measuring what survived, not what was produced.
 *
 * What works is a window short enough that no collection intervenes. Force a
 * GC, run a few hundred frames, read both heapUsed and arrayBuffers deltas,
 * and verify no GC fired. Typed-array backing stores are not in heapUsed;
 * wasm-bindgen's copied scene buffer must be counted too. If a collection
 * did, the sample is discarded and the window halves.
 *
 * That verification only works if the observer is drained, per the note above.
 * A single `await setImmediate` is not enough: it reports zero entries where
 * four timer turns report the collections that really happened. So `gcSeen`
 * was unconditionally 0, no sample was ever discarded, the halving below was
 * dead code, and the window stayed at 512 frames -- which is long enough to
 * collect, so this measured what survived after all. Exactly the failure the
 * rewrite existed to avoid, wearing the rewrite's clothes.
 */

/**
 * Waits long enough for the PerformanceObserver to receive `gc` entries for
 * collections that have already finished. Timer turns, not `setImmediate`;
 * see the note above.
 */
async function drainGcEntries() {
  for (let i = 0; i < 4; i++) await new Promise((r) => setTimeout(r, 0));
}
export async function runAlloc(adapter) {
  await adapter.init();
  const { PerformanceObserver, constants } = await import('node:perf_hooks');

  let gcSeen = 0;
  const obs = new PerformanceObserver((list) => {
    gcSeen += list.getEntries().length;
  });

  const s = adapter.build(seedLayout(ALLOC_SIZE, true));

  // Warm up, so tier-up allocations are not attributed to the frames.
  for (let i = 0; i < 300; i++) s.frame(VIEW_PROJECTING);

  let frames = ALLOC_FRAMES;
  let bytesPerFrame = NaN;
  let heapBytesPerFrame = NaN;
  let arrayBufferBytesPerFrame = NaN;
  let acc = 0;
  let attempts = 0;

  while (frames >= 8 && attempts < 8) {
    attempts++;
    obs.observe({ entryTypes: ['gc'] });
    globalThis.gc();
    globalThis.gc();
    // Let the two forced collections report themselves BEFORE the window
    // opens. Zeroing the counter first would count them against the window
    // and discard every sample.
    await drainGcEntries();
    gcSeen = 0;
    const before = process.memoryUsage();

    for (let i = 0; i < frames; i++) acc += s.frame(VIEW_PROJECTING);

    const after = process.memoryUsage();
    await drainGcEntries();
    obs.disconnect();

    const heapBytes = after.heapUsed - before.heapUsed;
    const arrayBufferBytes = after.arrayBuffers - before.arrayBuffers;
    if (gcSeen === 0 && heapBytes >= 0 && arrayBufferBytes >= 0 && heapBytes + arrayBufferBytes > 0) {
      heapBytesPerFrame = heapBytes / frames;
      arrayBufferBytesPerFrame = arrayBufferBytes / frames;
      bytesPerFrame = heapBytesPerFrame + arrayBufferBytesPerFrame;
      break;
    }
    // A collection landed inside the window, so the delta undercounts. Shrink.
    frames = Math.floor(frames / 2);
  }

  s.dispose();

  /**
   * Calibration, because `heapUsed` deltas are not an allocation counter.
   *
   * A `{x, y}` object with unboxed doubles should be about 24 bytes under V8
   * pointer compression. Measuring a known quantity the same way the workload is
   * measured tells the reader how much to trust the absolute figures. The
   * calibration applies to the heap component, not the separately accounted
   * ArrayBuffer backing stores. Neither component counts Rust's own allocations.
   */
  const calibration = (() => {
    const count = 100_000;
    const keep = new Array(count);
    globalThis.gc();
    globalThis.gc();
    const b = process.memoryUsage().heapUsed;
    for (let i = 0; i < count; i++) keep[i] = { x: i, y: i + 1 };
    const a = process.memoryUsage().heapUsed;
    // Touch it so nothing is eliminated.
    const guard = keep[count - 1].x + keep[0].y;
    return { objects: count, bytesPerObject: (a - b) / count, guard };
  })();

  return {
    adapter: adapter.name,
    frames,
    shapes: ALLOC_SIZE,
    attempts,
    calibration,
    allocationMetric: 'heapUsed+arrayBuffers',
    heapBytesPerFrame,
    arrayBufferBytesPerFrame,
    bytesPerFrame,
    bytesPerShape: bytesPerFrame / ALLOC_SIZE,
    gcDuringWindow: gcSeen,
    gcKindConstants: {
      minor: constants.NODE_PERFORMANCE_GC_MINOR,
      major: constants.NODE_PERFORMANCE_GC_MAJOR,
      incremental: constants.NODE_PERFORMANCE_GC_INCREMENTAL,
    },
    checksum: acc,
  };
}
