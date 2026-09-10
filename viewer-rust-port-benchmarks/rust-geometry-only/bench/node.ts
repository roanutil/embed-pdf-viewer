import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { environment, sinkValue } from '../../bench/harness.mjs';
import { runAll, runAlloc, runProbe } from '../../bench/workloads.mjs';
import * as adapter from './adapter';

const round = Number(process.env.BENCH_ROUND ?? '1');
const budgetMs = Number(process.env.BENCH_BUDGET_MS ?? '500');
const mode = process.env.BENCH_MODE ?? 'timing';

const out = resolve(
  import.meta.dirname,
  '../../bench/results',
  `${adapter.name}-${mode}-r${round}.json`,
);

// `probe` is the fairness pass bench/run.mjs runs before any timing. It writes
// no environment block: nothing compares probes across machines, and the gate
// only reads `probe`.
const payload =
  mode === 'probe'
    ? { mode, round, ...(await runProbe(adapter)) }
    : mode === 'alloc'
      ? { mode, round, env: await environment(), ...(await runAlloc(adapter)) }
      : { mode, round, budgetMs, env: await environment(), ...(await runAll(adapter, { budgetMs })) };

writeFileSync(out, JSON.stringify({ ...payload, sink: sinkValue() }, null, 2) + '\n');
process.stdout.write(`${adapter.name} ${mode} round ${round} -> ${out}\n`);
