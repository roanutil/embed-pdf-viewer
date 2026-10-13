// node scripts/prof/flowprof.mjs <arm> <flow> [reps]: main-thread CPU profiles of the
// harness's own measured window (open or a FLOWS entry), one per rep.
// Writes $PROF_OUT (default .work/prof/out)/fp-<flow>-<arm>-<i>.cpuprofile.
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { workDir } from '../materialize.mjs';
import { launchBrowser, openCdp } from '../../lib/cdp.mjs';
import { serveArms } from '../../lib/serve.mjs';
import { FLOWS, openFlow } from '../../lib/flows.mjs';
const S = process.env.PROF_OUT ?? fileURLToPath(new URL('../../.work/prof/out', import.meta.url));
mkdirSync(S, { recursive: true });
const [arm, flow, reps = '5'] = process.argv.slice(2);
const fixture = flow === 'open' ? (process.env.FIXTURE ?? 'large') : FLOWS[flow].fixture;
const { urls, close } = await serveArms({ [arm]: path.join(workDir(), 'dist', arm) }, { basePort: 5420 });
const run = await launchBrowser({ port: 9350 });
const probePath = fileURLToPath(new URL('../../lib/probe.js', import.meta.url));
for (let i = 0; i < Number(reps); i++) {
  const context = await run.browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
  await context.addInitScript({ path: probePath });
  const page = await context.newPage();
  await page.goto(`${urls[arm]}/__blank`);
  const cdp = await openCdp(page, run.cdp);
  const prof = await context.newCDPSession(page);
  await prof.send('Profiler.enable');
  await prof.send('Profiler.setSamplingInterval', { interval: 100 });
  const wrapped = Object.create(cdp);
  wrapped.cpuStart = async () => { const b = await cdp.cpuStart(); await prof.send('Profiler.start'); return b; };
  wrapped.cpuEnd = async (b) => {
    const { profile } = await prof.send('Profiler.stop');
    writeFileSync(`${S}/fp-${flow}-${arm}-${i}.cpuprofile`, JSON.stringify(profile));
    return cdp.cpuEnd(b);
  };
  const url = `${urls[arm]}/?doc=${fixture}`;
  let result;
  if (flow === 'open') result = await openFlow(page, wrapped, url);
  else {
    const f = FLOWS[flow];
    await page.goto(url);
    await page.waitForFunction(() => window.__perf, null, { timeout: 60_000 });
    await page.evaluate(() => window.__perf.opened);
    await page.evaluate(() => window.__probe.settledAt(0, { requireChange: false }));
    await f.start(page);
    if (f.warmup) { await f.warmup(page, wrapped); await f.start(page); }
    await page.evaluate(() => window.__probe.reset());
    result = await f.measure(page, wrapped);
  }
  console.log(arm, flow, i, JSON.stringify(Object.fromEntries(Object.entries(result.metrics).filter(([k]) => /Ms$/.test(k)).map(([k, v]) => [k, Math.round(v)]))));
  await cdp.close();
  await context.close();
}
await run.close();
await close();
