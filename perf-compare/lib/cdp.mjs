// CDP helpers: metrics, GC, memory and CPU throttle, for the page and for the
// engine's dedicated worker.
//
// Playwright's CDPSession can't address the child sessions a flattened
// auto-attach creates, and the worker needs one: it has to be throttled
// before its first instruction. So the browser also listens on a debugging
// port, and this module keeps its own CDP connection to it, attaching to each
// page's target by id.
import { chromium } from 'playwright-core';

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
export async function launchBrowser({ port = 9333 } = {}) {
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

// Must run on a fresh page, before page.goto: the auto-attach has to be in
// place before the engine worker starts.
export async function openCdp(page, cdp) {
  const probe = await page.context().newCDPSession(page);
  const { targetInfo } = await probe.send('Target.getTargetInfo');
  await probe.detach();
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
  await cdp.send('Performance.enable', {}, pageSession);
  await cdp.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: true, flatten: true }, pageSession);

  const readMetrics = async (sessionId) => {
    const { metrics } = await cdp.send('Performance.getMetrics', {}, sessionId);
    // CDP reports seconds.
    return { taskMs: METRIC(metrics, 'TaskDuration') * 1000, scriptMs: METRIC(metrics, 'ScriptDuration') * 1000 };
  };
  const sessions = () => [pageSession, ...workers.keys()];

  const self = {
    get workerThrottled() {
      return workerThrottled;
    },
    workerCount: () => workers.size,
    metrics: () => readMetrics(pageSession),
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
      off();
      await cdp.send('Target.detachFromTarget', { sessionId: pageSession }).catch(() => {});
    },
  };
  return self;
}
