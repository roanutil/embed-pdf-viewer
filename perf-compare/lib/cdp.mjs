// CDP helpers: metrics, GC, memory and CPU throttle, for the page and for the
// engine's dedicated worker.
//
// Playwright's CDPSession can't address the child sessions a flattened
// auto-attach creates, and the worker needs one: it has to be throttled
// before its first instruction. So the browser also listens on a debugging
// port, and this module keeps its own CDP connection to it, attaching to each
// page's target by id.
import { chromium } from 'playwright-core';

/** `PERF_PORT_OFFSET`: added to every port this harness listens on or
 *  launches with, so runs that share a host don't collide. */
export const portOffset = () => Number(process.env.PERF_PORT_OFFSET ?? 0);

export const CHROMIUM_ARGS = ['--enable-blink-features=ForceEagerMeasureMemory', '--js-flags=--expose-gc'];

// One connection to the browser endpoint. `send` takes an optional session id
// for a flattened child session.
async function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', reject, { once: true });
  });
  let nextId = 1;
  const pending = new Map();
  const listeners = new Set();
  ws.addEventListener('message', (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id !== undefined) {
      const p = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) p?.reject(new Error(`${p.method}: ${msg.error.message}`));
      else p?.resolve(msg.result);
      return;
    }
    for (const fn of listeners) fn(msg);
  });
  return {
    send(method, params = {}, sessionId) {
      const id = nextId++;
      ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
      return new Promise((resolve, reject) => pending.set(id, { resolve, reject, method }));
    },
    on(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    close: () => ws.close(),
  };
}

// Launches Chromium with the harness flags and a debugging port for connect().
// The full build in new-headless mode, not the headless shell: the shell
// rejects measureUserAgentSpecificMemory() with "is not available" even on a
// cross-origin isolated page.
export async function launchBrowser({ port = 9333 + portOffset() } = {}) {
  let browser;
  try {
    browser = await chromium.launch({ headless: true, channel: 'chromium', args: [...CHROMIUM_ARGS, `--remote-debugging-port=${port}`] });
  } catch (error) {
    if (/Executable doesn't exist/.test(String(error))) {
      throw new Error('Chromium is missing: run `pnpm exec playwright-core install chromium` in perf-compare/');
    }
    throw error;
  }
  const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  const cdp = await connect(version.webSocketDebuggerUrl);
  return {
    browser,
    cdp,
    browserVersion: browser.version(),
    async close() {
      cdp.close();
      await browser.close();
    },
  };
}

const METRIC = (metrics, name) => metrics.find((m) => m.name === name)?.value ?? 0;

// The trace events that enter JavaScript on a renderer's main thread. Only the
// outermost is counted, so a microtask checkpoint at the end of a callback
// isn't counted twice. `RunMicrotasks` is why this reads a trace at all:
// `ScriptDuration` leaves microtasks out, and a Preact render is one.
const JS_EVENTS = new Set(['FunctionCall', 'RunMicrotasks', 'EvaluateScript', 'v8.evaluateModule']);
const TRACE_CATEGORIES = 'devtools.timeline,v8.execute';
const debug = process.env.PERF_DEBUG ? (...a) => console.error('[cdp]', (performance.now() / 1000).toFixed(1), ...a) : () => {};
// A trace that hasn't started or completed by now never will: fail the
// sample, not the run.
const TRACE_TIMEOUT_MS = 60_000;
async function within(promise, what) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} didn't answer in ${TRACE_TIMEOUT_MS} ms`)), TRACE_TIMEOUT_MS);
  });
  try {
    return await Promise.race([promise, late]);
  } finally {
    clearTimeout(timer);
  }
}

// Milliseconds of JavaScript on the busiest renderer main thread in `events`:
// the page's own, since it's the only page running.
function mainThreadJsMs(events) {
  const main = new Set();
  for (const e of events) {
    if (e.name === 'thread_name' && e.args?.name === 'CrRendererMain') main.add(`${e.pid}:${e.tid}`);
  }
  const byThread = new Map();
  for (const e of events) {
    if (e.ph !== 'X' || !e.dur || !JS_EVENTS.has(e.name)) continue;
    const thread = `${e.pid}:${e.tid}`;
    if (!main.has(thread)) continue;
    if (!byThread.has(thread)) byThread.set(thread, []);
    byThread.get(thread).push(e);
  }
  let best = 0;
  for (const list of byThread.values()) {
    list.sort((a, b) => a.ts - b.ts || b.dur - a.dur);
    let total = 0;
    let end = -Infinity;
    for (const e of list) {
      if (e.ts >= end) {
        total += e.dur;
        end = e.ts + e.dur;
      }
    }
    best = Math.max(best, total);
  }
  return best / 1000;
}

// Must run on a fresh page, before page.goto: the auto-attach has to be in
// place before the engine worker starts.
export async function openCdp(page, cdp) {
  debug('openCdp: getTargetInfo');
  const probe = await page.context().newCDPSession(page);
  const { targetInfo } = await probe.send('Target.getTargetInfo');
  await probe.detach();
  debug('openCdp: attachToTarget');
  const { sessionId: pageSession } = await cdp.send('Target.attachToTarget', {
    targetId: targetInfo.targetId,
    flatten: true,
  });

  let rate = 1;
  let workerThrottled = true;
  const workers = new Map(); // sessionId -> { performance: boolean }

  const onAttached = async (sessionId, info) => {
    if (info.type !== 'worker') {
      await cdp.send('Runtime.runIfWaitingForDebugger', {}, sessionId).catch(() => {});
      return;
    }
    const worker = { performance: false };
    workers.set(sessionId, worker);
    // Chromium 140 answers "'Performance.enable' wasn't found" here and
    // "Operation is only supported for pages, not workers" for the throttle,
    // so workerMetrics() is null and workerThrottled goes false. Both are
    // still tried, for a Chromium that adds them.
    try {
      await cdp.send('Performance.enable', {}, sessionId);
      worker.performance = true;
    } catch {}
    if (rate !== 1) {
      try {
        await cdp.send('Emulation.setCPUThrottlingRate', { rate }, sessionId);
      } catch {
        workerThrottled = false;
      }
    }
    await cdp.send('Runtime.runIfWaitingForDebugger', {}, sessionId).catch(() => {});
    // After the resume: a paused worker never answers collectGarbage.
    await cdp.send('HeapProfiler.enable', {}, sessionId).catch(() => {});
  };
  const off = cdp.on((msg) => {
    if (msg.method === 'Target.attachedToTarget' && msg.sessionId === pageSession) {
      onAttached(msg.params.sessionId, msg.params.targetInfo);
    }
    if (msg.method === 'Target.detachedFromTarget' && msg.sessionId === pageSession) {
      workers.delete(msg.params.sessionId);
    }
  });
  debug('openCdp: Performance.enable');
  await cdp.send('Performance.enable', {}, pageSession);
  debug('openCdp: setAutoAttach');
  await cdp.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: true, flatten: true }, pageSession);
  debug('openCdp: ready');

  const readMetrics = async (sessionId) => {
    const { metrics } = await cdp.send('Performance.getMetrics', {}, sessionId);
    // CDP reports seconds.
    return { taskMs: METRIC(metrics, 'TaskDuration') * 1000 };
  };
  const sessions = () => [pageSession, ...workers.keys()];

  // A browser-wide trace over one measured window, for `mainJsMs`.
  let trace = null;
  const offTrace = cdp.on((msg) => {
    if (msg.sessionId || !trace) return;
    if (msg.method === 'Tracing.dataCollected') trace.events.push(...msg.params.value);
    if (msg.method === 'Tracing.tracingComplete') trace.done();
  });

  const self = {
    get workerThrottled() {
      return workerThrottled;
    },
    workerCount: () => workers.size,
    metrics: () => readMetrics(pageSession),
    /** Start a measured window: a trace, then the page's counters. */
    async cpuStart() {
      if (trace) throw new Error('cpuStart: a window is already open');
      const fresh = () => {
        let done;
        trace = { events: [], complete: new Promise((resolve) => (done = resolve)) };
        trace.done = done;
      };
      fresh();
      debug('Tracing.start');
      const start = () =>
        within(
          cdp.send('Tracing.start', { categories: TRACE_CATEGORIES, transferMode: 'ReportEvents' }),
          'Tracing.start',
        );
      await start()
        .catch(async (error) => {
          // A trace some earlier window never ended: end it, drop what it
          // reports, and start this one.
          if (!/already been started/.test(error.message)) throw error;
          await cdp.send('Tracing.end').catch(() => {});
          await within(trace.complete, 'the stray trace').catch(() => {});
          fresh();
          await start();
        })
        .catch((error) => {
          trace = null;
          throw error;
        });
      debug('Tracing.start done');
      return readMetrics(pageSession);
    },
    /** End it: main-thread task time from the counters, and JavaScript
     *  time, microtasks included, from the trace. */
    async cpuEnd(before) {
      const after = await readMetrics(pageSession);
      const open = trace;
      debug('Tracing.end');
      await cdp.send('Tracing.end');
      debug('Tracing.end sent, events so far', open.events.length);
      try {
        await within(open.complete, 'the trace');
      } finally {
        trace = null;
      }
      debug('trace complete', open.events.length);
      return {
        mainTaskMs: after.taskMs - before.taskMs,
        mainJsMs: mainThreadJsMs(open.events),
      };
    },
    async workerMetrics() {
      const [sessionId, worker] = [...workers.entries()].find(([, w]) => w.performance) ?? [];
      if (!worker) return null;
      try {
        return { taskMs: (await readMetrics(sessionId)).taskMs };
      } catch {
        return null;
      }
    },
    async gc() {
      // Bounded, so a worker that stops answering can't hang a run.
      const collect = (s) =>
        Promise.race([
          cdp.send('HeapProfiler.collectGarbage', {}, s).catch(() => {}),
          new Promise((resolve) => setTimeout(resolve, 5000)),
        ]);
      await Promise.all(sessions().map(collect));
    },
    // Total and DedicatedWorker bytes from measureUserAgentSpecificMemory,
    // after a GC. Falls back to per-target JS heap sizes, which leave wasm
    // linear memory out, when the page isn't cross-origin isolated.
    async memory() {
      await self.gc();
      const uasm = await page.evaluate(async () => {
        if (!crossOriginIsolated || !performance.measureUserAgentSpecificMemory) return null;
        try {
          const result = await performance.measureUserAgentSpecificMemory();
          let worker = 0;
          for (const entry of result.breakdown) {
            if (entry.attribution.some((a) => a.scope === 'DedicatedWorkerGlobalScope')) worker += entry.bytes;
          }
          return { total: result.bytes, worker };
        } catch {
          return null;
        }
      });
      if (uasm) return { ...uasm, source: 'uasm' };
      let total = 0;
      let worker = 0;
      for (const s of sessions()) {
        const { usedSize } = await cdp.send('Runtime.getHeapUsage', {}, s).catch(() => ({ usedSize: 0 }));
        total += usedSize;
        if (s !== pageSession) worker += usedSize;
      }
      return { total, worker, source: 'cdp' };
    },
    async throttle(next) {
      rate = next;
      await cdp.send('Emulation.setCPUThrottlingRate', { rate }, pageSession);
      for (const s of workers.keys()) {
        try {
          await cdp.send('Emulation.setCPUThrottlingRate', { rate }, s);
        } catch {
          workerThrottled = false;
        }
      }
    },
    async close() {
      // A flow that threw inside its measured window left the trace running;
      // end it, or every later window on this browser fails to start one.
      if (trace) {
        const open = trace;
        await cdp.send('Tracing.end').catch(() => {});
        await within(open.complete, 'the trace').catch(() => {});
        trace = null;
      }
      off();
      offTrace();
      await cdp.send('Target.detachFromTarget', { sessionId: pageSession }).catch(() => {});
    },
  };
  return self;
}
