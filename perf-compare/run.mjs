// Gate, then interleaved rounds. Writes results/<run-id>/: gate.json,
// samples.jsonl, and meta.json last, so a run killed half way has no meta.
//
//   node run.mjs [--rounds N | --quick] [--profile desktop|slow]
//                [--arms a,b,...] [--flows a,b,...]
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';
import { launchBrowser } from './lib/cdp.mjs';
import { loadConfig } from './lib/config.mjs';
import { collectEnv, DEVICE_SCALE_FACTOR, distSizes, THROTTLE, verifyFixtures, VIEWPORT } from './lib/env.mjs';
import { FLOW_ORDER, FLOWS, runFlow } from './lib/flows.mjs';
import { runGate } from './lib/gate.mjs';
import { serveArms } from './lib/serve.mjs';
import { workDir } from './scripts/materialize.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const { values } = parseArgs({
  options: {
    rounds: { type: 'string' },
    quick: { type: 'boolean', default: false },
    profile: { type: 'string', default: 'desktop' },
    arms: { type: 'string' },
    flows: { type: 'string' },
  },
});

const config = loadConfig();
const quick = values.quick;
const rounds = quick ? 3 : Number(values.rounds ?? 10);
const profile = values.profile;
if (!(profile in THROTTLE)) throw new Error(`--profile must be one of ${Object.keys(THROTTLE).join(', ')}`);
const rate = THROTTLE[profile];
const pick = (list, all, what) => {
  if (!list) return all;
  const names = list.split(',');
  for (const n of names) if (!all.includes(n)) throw new Error(`unknown ${what} ${n}`);
  return all.filter((n) => names.includes(n));
};
const arms = pick(values.arms, config.armOrder, 'arm');
const flows = pick(values.flows, FLOW_ORDER, 'flow');

const fixtures = verifyFixtures();
const stamps = {};
for (const id of arms) {
  const w = config.arms[id].worktree;
  const stampFile = path.join(workDir(), 'stamps', `${w}.json`);
  if (!existsSync(stampFile)) throw new Error(`no build for worktree ${w}; run scripts/build-entry.mjs`);
  stamps[w] = JSON.parse(readFileSync(stampFile, 'utf8'));
  if (!existsSync(path.join(workDir(), 'dist', id, 'index.html'))) {
    throw new Error(`no entry build for arm ${id}; run scripts/build-entry.mjs`);
  }
}

const started = new Date();
const runId = `${started.toISOString().replace(/[:.]/g, '-')}-${os.hostname().split('.')[0]}-${profile}`;
const out = path.join(here, 'results', runId);
mkdirSync(out, { recursive: true });
console.log(`run ${runId}: ${rounds} rounds, ${arms.length} arms, ${flows.length} flows${quick ? ' (quick)' : ''}`);

const { urls, close } = await serveArms(Object.fromEntries(arms.map((id) => [id, path.join(workDir(), 'dist', id)])));
const run = await launchBrowser();
const env = { viewport: VIEWPORT, deviceScaleFactor: DEVICE_SCALE_FACTOR };
try {
  const gate = await runGate(run, { config, arms, urls, stamps, env });
  writeFileSync(path.join(out, 'gate.json'), JSON.stringify(gate, null, 2));
  for (const [id, g] of Object.entries(gate)) console.log(`gate ${id}: ${g.pass ? 'pass' : `excluded: ${g.reason}`}`);
  const timed = arms.filter((id) => gate[id].pass);

  const samples = path.join(out, 'samples.jsonl');
  let memorySource = null;
  let workerThrottled = rate === 1 ? null : true;
  const jobs = flows.flatMap((flow) =>
    (flow === 'open' ? FLOWS.open.fixtures : [FLOWS[flow].fixture]).map((fixture) => ({ flow, fixture })),
  );
  for (let round = 0; round < rounds; round++) {
    // Round r starts at arm r mod n.
    const order = timed.map((_, i) => timed[(i + round) % timed.length]);
    for (const { flow, fixture } of jobs) {
      for (const arm of order) {
        const line = { arm, flow, fixture, round };
        try {
          const r = await runFlow(run, { origin: urls[arm], flow, fixture, rate, ...env });
          line.metrics = r.metrics;
          if (r.diagnostics) line.diagnostics = r.diagnostics;
          if (r.memorySource) memorySource ??= r.memorySource;
          if (r.memorySource === 'cdp') memorySource = 'cdp';
          if (r.workerThrottled === false) workerThrottled = false;
        } catch (error) {
          line.error = String(error.message ?? error).split('\n')[0];
        }
        appendFileSync(samples, JSON.stringify(line) + '\n');
        process.stdout.write(`\r  round ${round + 1}/${rounds} ${flow}/${fixture} ${arm}${' '.repeat(20)}`);
      }
    }
  }
  process.stdout.write('\n');

  const meta = {
    runId,
    ...collectEnv({ config, arms, stamps, dists: Object.fromEntries(arms.map((id) => [id, path.join(workDir(), 'dist', id)])), fixtures, browserVersion: run.browserVersion, profile, flows }),
    rounds,
    quick,
    armsRun: arms,
    sizes: Object.fromEntries(arms.map((id) => [id, distSizes(path.join(workDir(), 'dist', id))])),
    memorySource,
    workerThrottled,
    startedAt: started.toISOString(),
    completedAt: new Date().toISOString(),
  };
  writeFileSync(path.join(out, 'meta.json'), JSON.stringify(meta, null, 2));
  const minutes = ((Date.now() - started.getTime()) / 60_000).toFixed(1);
  console.log(`done in ${minutes} min: ${path.relative(process.cwd(), out)}`);
} finally {
  await run.close();
  await close();
}
