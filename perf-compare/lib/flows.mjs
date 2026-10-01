// The nine flows. For the first eight, the table in viewer-perf-compare-plan.md
// (Task 7, Step 2) is the contract: starting state, warm-up, measured action,
// done condition. `coldScroll`, added since, is `scroll` without its warm-up.
//
// Every time is page clock minus page clock (performance.now() in the page).
// The runner calls start, warmup, start, then measure; only measure is timed.
import { openCdp } from './cdp.mjs';
import { slope, summarize } from './stats.mjs';

/** @typedef {{ fixture: string, start(page): Promise<void>, warmup?(page, cdp): Promise<void>, measure(page, cdp): Promise<{ metrics: Record<string, number>, diagnostics?: unknown, answers?: unknown }> }} Flow */

// Drag regions, as fractions of the page box. Picked on ebook.pdf page index 1
// at zoom 1: four lines of the "1 Pay through the nose" paragraph, and the
// single line "You need PDF functionality in your app."
const SELECT_DRAG = { from: [0.2, 0.384], to: [0.78, 0.475] };
const HIGHLIGHT_DRAG = { from: [0.11, 0.252], to: [0.69, 0.252] };
// #viewer fills the window, so "outside" is the tab bar's empty left corner:
// over no page, so no hover can ask for text geometry.
const PARKED = { x: 4, y: 4 };

const now = (page) => page.evaluate(() => performance.now());
const settle = (page, since, opts = {}) =>
  page.evaluate(([s, o]) => window.__probe.settledAt(s, o), [since, opts]);

async function waitFor(page, fn, arg, what, timeout = 30_000) {
  try {
    await page.waitForFunction(fn, arg, { timeout, polling: 16 });
  } catch {
    throw new Error(`timed out waiting for ${what}`);
  }
  return now(page);
}

// Main-thread task time, and JavaScript time from a trace of the same window
// (`mainJsMs`). It replaces `ScriptDuration`, which leaves microtasks out: a
// Preact render is one, so a change that moved work out of a render read as
// more script when the JavaScript run was the same.
const cpu = (cdp, before) => cdp.cpuEnd(before);

async function goTo(page, index) {
  await page.evaluate((i) => window.__perf.goToPageIndex(i), index);
  return waitFor(page, (i) => window.__perf.currentPageIndex() === i, index, `page index ${index}`);
}

async function zoomTo(page, zoom) {
  await page.evaluate((z) => window.__perf.zoomTo(z), zoom);
  return waitFor(page, (z) => Math.abs(window.__perf.zoomLevel() - z) < 1e-6, zoom, `zoom ${zoom}`);
}

// The shared starting state: no query, no selection, pointer tool, the given
// page at zoom 1, settled.
function startAt(pageIndex, { park = false } = {}) {
  return async (page) => {
    await page.evaluate(() => {
      window.__perf.clearSearch();
      window.__perf.clearSelection();
      window.__perf.execute('pointer:toggle');
    });
    if (park) await page.mouse.move(PARKED.x, PARKED.y);
    await zoomTo(page, 1);
    await goTo(page, pageIndex);
    await settle(page, await now(page), { requireChange: false });
  };
}

async function drag(page, pageIndex, { from, to }) {
  const box = await page.evaluate((i) => window.__perf.pageBox(i), pageIndex);
  const at = ([fx, fy]) => [box.x + box.width * fx, box.y + box.height * fy];
  const [x0, y0] = at(from);
  const [x1, y1] = at(to);
  await page.mouse.move(x0, y0, { steps: 4 });
  await page.mouse.down();
  await page.mouse.move(x1, y1, { steps: 12 });
  await page.mouse.up();
}

async function readSelection(page, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const text = await page.evaluate(() => window.__perf.readSelection());
    if (text) return { text, at: await now(page) };
    if (Date.now() > deadline) throw new Error('the drag selected no text');
    await page.waitForTimeout(16);
  }
}

async function wheel(page, count) {
  for (let i = 0; i < count; i++) {
    await page.mouse.wheel(0, 240);
    await page.waitForTimeout(16);
  }
}

async function centre(page) {
  const box = await page.locator('#viewer').boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
}

/** @type {Record<string, Flow>} */
export const FLOWS = {
  // Measured by the runner's open path (openFlow below), not start/measure.
  open: { fixtures: ['ebook', 'large'] },

  scroll: {
    fixture: 'large',
    async start(page) {
      await startAt(0)(page);
      await centre(page);
    },
    async warmup(page) {
      await goTo(page, 200);
      await centre(page);
      await wheel(page, 20);
      await settle(page, await now(page), { requireChange: false });
    },
    async measure(page, cdp) {
      await centre(page);
      const before = await cdp.cpuStart();
      // Frames and long tasks from here on only.
      await page.evaluate(() => window.__probe.reset());
      const t0 = await now(page);
      await wheel(page, 60);
      const tLast = await now(page);
      // requireChange counts from t0, not tLast: the scroll's renders may all
      // land during the wheel burst, and that's a settled page, not a failure.
      const settled = await settle(page, t0, { doneAt: tLast });
      const load = await cpu(cdp, before);
      const { frames, longTasks } = await page.evaluate(() => ({
        frames: window.__probe.frames(),
        longTasks: window.__probe.longTasks(),
      }));
      const s = summarize(frames);
      const inFlow = longTasks.filter((l) => l.start >= t0);
      return {
        metrics: {
          settleMs: settled - tLast,
          wallMs: settled - t0,
          frameP50Ms: s.median,
          frameP95Ms: s.p95,
          jankFrames: frames.filter((f) => f > 50).length,
          longTasks: inFlow.length,
          longTaskMs: inFlow.reduce((a, l) => a + l.duration, 0),
          ...load,
        },
      };
    },
  },

  // The same 60 wheels on a document just opened, with no warm-up, so every
  // page the scroll reaches is reached for the first time: its scale and its
  // tiles load during the measured window. A first pass through a document
  // does that; `scroll`, after its warm-up, doesn't time it.
  coldScroll: {
    fixture: 'large',
    start: (page) => FLOWS.scroll.start(page),
    measure: (page, cdp) => FLOWS.scroll.measure(page, cdp),
  },

  jump: {
    fixture: 'large',
    start: startAt(0),
    async warmup(page) {
      await goTo(page, 200);
      await settle(page, await now(page), { requireChange: false });
    },
    async measure(page, cdp) {
      const before = await cdp.cpuStart();
      const t0 = await now(page);
      const done = await goTo(page, 300);
      const settled = await settle(page, t0, { doneAt: done });
      return { metrics: { settleMs: settled - t0, ...(await cpu(cdp, before)) } };
    },
  },

  zoom: {
    fixture: 'ebook',
    start: startAt(0),
    async warmup(page) {
      for (const z of [1.5, 3]) {
        const t = await now(page);
        await zoomTo(page, z);
        await settle(page, t, { requireChange: false });
      }
    },
    async measure(page, cdp) {
      const before = await cdp.cpuStart();
      const steps = {};
      for (const [z, key, requireChange] of [
        [2, 'zoom2SettleMs', true],
        [4, 'zoom4SettleMs', true],
        // Can be served from the starting state's tiles, so it's labeled
        // apart and never read as a cold render.
        [1, 'backTo1SettleMs', false],
      ]) {
        const t = await now(page);
        const done = await zoomTo(page, z);
        steps[key] = (await settle(page, t, { doneAt: done, requireChange })) - t;
      }
      return {
        metrics: { settleMs: steps.zoom2SettleMs + steps.zoom4SettleMs, ...steps, ...(await cpu(cdp, before)) },
      };
    },
  },

  select: {
    fixture: 'ebook',
    start: startAt(1, { park: true }),
    async warmup(page) {
      await goTo(page, 2);
      await settle(page, await now(page), { requireChange: false });
      await drag(page, 2, SELECT_DRAG);
      await page.evaluate(() => window.__perf.readSelection()).catch(() => {});
      await page.mouse.move(PARKED.x, PARKED.y);
    },
    async measure(page, cdp) {
      const before = await cdp.cpuStart();
      // Before the pointer first enters the page: on main the selection
      // handler's onHover already fetches text geometry.
      const t0 = await now(page);
      await drag(page, 1, SELECT_DRAG);
      const { text, at } = await readSelection(page);
      const settled = await settle(page, t0, { doneAt: at, requireChange: false });
      return { metrics: { settleMs: settled - t0, ...(await cpu(cdp, before)) }, answers: { selection: text } };
    },
  },

  search: {
    fixture: 'ebook',
    start: startAt(0),
    // No warm-up: a warm-up search fills every page's text cache.
    async measure(page, cdp) {
      const before = await cdp.cpuStart();
      const { ms, hits } = await page.evaluate(async () => {
        const t0 = performance.now();
        const hits = await window.__perf.search('the');
        return { ms: performance.now() - t0, hits };
      });
      return { metrics: { searchMs: ms, ...(await cpu(cdp, before)) }, answers: { hits } };
    },
  },

  annotate: {
    fixture: 'ebook',
    start: startAt(1, { park: true }),
    async warmup(page) {
      await goTo(page, 2);
      await settle(page, await now(page), { requireChange: false });
      const count = await page.evaluate(() => window.__perf.annotationCount(2));
      await page.evaluate(() => window.__perf.execute('annotation:add-highlight'));
      await drag(page, 2, HIGHLIGHT_DRAG);
      await waitFor(page, (n) => window.__perf.annotationCount(2) > n, count, 'the warm-up highlight', 10_000).catch(
        () => {},
      );
      await page.mouse.move(PARKED.x, PARKED.y);
    },
    async measure(page, cdp) {
      const countBefore = await page.evaluate(() => window.__perf.annotationCount(1));
      await page.evaluate(() => window.__perf.execute('annotation:add-highlight'));
      const before = await cdp.cpuStart();
      // After the tool is on, before the pointer first enters the page.
      const t0 = await now(page);
      await drag(page, 1, HIGHLIGHT_DRAG);
      const done = await waitFor(
        page,
        (n) => window.__perf.annotationCount(1) === n + 1,
        countBefore,
        'the highlight to be added',
        10_000,
      );
      const settled = await settle(page, t0, { doneAt: done, requireChange: false });
      const countAfter = await page.evaluate(() => window.__perf.annotationCount(1));
      return {
        metrics: { settleMs: settled - t0, ...(await cpu(cdp, before)) },
        answers: { countBefore, countAfter },
      };
    },
  },

  soak: {
    fixture: 'ebook',
    start: startAt(0),
    // No warm-up; cycles 1 to 4 are its warm-up and aren't reported.
    async measure(page, cdp) {
      const last = (await page.evaluate(() => window.__perf.pageCount())) - 1;
      const totals = [];
      const diagnostics = {};
      // Cached tiles can make a step change nothing, so no step requires one.
      const step = async (action) => {
        const t = await now(page);
        const done = await action();
        await settle(page, t, { doneAt: done, requireChange: false });
      };
      for (let cycle = 1; cycle <= 30; cycle++) {
        await step(() => goTo(page, last));
        await step(() => zoomTo(page, 3));
        await step(() => goTo(page, 0));
        await step(() => zoomTo(page, 1));
        if (cycle >= 5) totals.push((await cdp.memory()).total);
        if (cycle === 5 || cycle === 30) {
          diagnostics[`cycle${cycle}`] = await page.evaluate(() => window.__perf.kernelCounts());
        }
      }
      return {
        metrics: {
          heapCycle5Bytes: totals[0],
          heapCycle30Bytes: totals[totals.length - 1],
          heapSoakSlopeBytes: slope(totals),
        },
        diagnostics: diagnostics.cycle5 ? diagnostics : undefined,
      };
    },
  },
};

export const FLOW_ORDER = Object.keys(FLOWS);

// Cold open on a page the runner prepared: parked on /__blank with CDP
// attached and the throttle on.
export async function openFlow(page, cdp, url) {
  const before = await cdp.cpuStart();
  await page.goto(url);
  await page.waitForFunction(() => window.__perf, null, { timeout: 60_000 });
  await page.evaluate(() => window.__perf.opened);
  await page.waitForFunction(() => window.__probe.firstPaintAt !== null, null, { timeout: 30_000 });
  // performance.now() is relative to navigation start, so 0 is the start.
  const settled = await settle(page, 0);
  const marks = await page.evaluate(() => ({
    readyMs: performance.getEntriesByName('perf:ready')[0].startTime,
    openedMs: performance.getEntriesByName('perf:opened')[0].startTime,
    firstPaintMs: window.__probe.firstPaintAt,
  }));
  const load = await cpu(cdp, before);
  const memory = await cdp.memory();
  return {
    metrics: {
      ...marks,
      settledMs: settled,
      ...load,
      heapAfterOpenBytes: memory.total,
      workerHeapAfterOpenBytes: memory.worker,
    },
    memorySource: memory.source,
  };
}

const probePath = new URL('./probe.js', import.meta.url).pathname;

// One flow on one arm, in a fresh context: park on /__blank, attach CDP and
// throttle there (same origin, so the viewer's navigation keeps the process
// and the throttle), then open and run. `inspect(page)`, when given, runs
// after the measurement and its answer comes back as `inspected`.
export async function runFlow(run, { origin, flow, fixture, rate, viewport, deviceScaleFactor, inspect }) {
  const context = await run.browser.newContext({ viewport, deviceScaleFactor });
  try {
    await context.addInitScript({ path: probePath });
    const page = await context.newPage();
    if (process.env.PERF_DEBUG) console.error('[flow]', flow, fixture, origin);
    await page.goto(`${origin}/__blank`);
    const cdp = await openCdp(page, run.cdp);
    try {
      await cdp.throttle(rate);
      if (process.env.PERF_DEBUG) console.error('[flow] throttled');
      const url = `${origin}/?doc=${fixture}`;
      let result;
      if (flow === 'open') {
        result = await openFlow(page, cdp, url);
      } else {
        const f = FLOWS[flow];
        await page.goto(url);
        await page.waitForFunction(() => window.__perf, null, { timeout: 60_000 });
        await page.evaluate(() => window.__perf.opened);
        await settle(page, 0);
        await f.start(page);
        if (f.warmup) {
          await f.warmup(page, cdp);
          await f.start(page);
        }
        await page.evaluate(() => window.__probe.reset());
        result = await f.measure(page, cdp);
      }
      const inspected = inspect ? await inspect(page) : undefined;
      return { ...result, inspected, workerThrottled: rate === 1 ? null : cdp.workerThrottled };
    } finally {
      await cdp.close();
    }
  } finally {
    await context.close();
  }
}
