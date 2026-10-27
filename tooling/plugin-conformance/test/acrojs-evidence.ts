/** Private C1 evidence capture. Writes historical evidence only; emit uses render mode. */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { suites } from '../src/suites/acrojs';
import { canonical, canonicalError, snapshotHooks } from '../src/canonical';
import { runScenario } from '../src/scenario';
import { emit } from '../src/vectors';
const root = new URL('../historical/task-12b2c1/', import.meta.url);
const write = (name: string, value: unknown) =>
  writeFileSync(new URL(name, root), JSON.stringify(value, null, 2) + '\n');
const hash = (value: unknown) =>
  createHash('sha256')
    .update(typeof value === 'string' ? value : JSON.stringify(value))
    .digest('hex');
const decode = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(decode);
  if (value === null || typeof value !== 'object') return value;
  if ('$number' in value) return Number(value.$number);
  if ('$undefined' in value) return undefined;
  return Object.fromEntries(Object.entries(value).map(([key, member]) => [key, decode(member)]));
};
interface Original {
  file: string;
  kind: string;
  name: string;
  occurrence: number;
  input: unknown;
  inputSha256: string;
}
const originals = JSON.parse(
  readFileSync(new URL('original-inputs.json', root), 'utf8'),
) as Original[];
type EvidenceOutcome = { returned: unknown } | { threw: unknown };
type Current = {
  file: string;
  kind: string;
  index: number;
  name: string;
  input: unknown;
  inputSha256: string;
} & ({ outcome: EvidenceOutcome } | { result: Awaited<ReturnType<typeof runScenario>> });
const current: Current[] = [];
for (const suite of suites) {
  for (const [index, entry] of suite.cases.entries()) {
    const input = canonical(entry);
    const common = {
      file: suite.file,
      kind: suite.kind,
      index,
      name: entry.name,
      input,
      inputSha256: hash(input),
    };
    if (suite.kind === 'function') {
      assert.ok('fn' in entry);
      let outcome: EvidenceOutcome;
      try {
        outcome = {
          returned: canonical(
            (suite.functions[entry.fn] as (...args: unknown[]) => unknown)(
              ...entry.args.map(decode),
            ),
            snapshotHooks,
          ),
        };
      } catch (error) {
        outcome = { threw: canonicalError(error, snapshotHooks) };
      }
      current.push({ ...common, outcome });
    } else {
      assert.ok('steps' in entry);
      current.push({ ...common, result: await runScenario(entry) });
    }
  }
  await emit([suite], 'render');
  console.log('render OK ' + suite.file + ' cases=' + suite.cases.length);
}
write('current-inputs-runtime.json', current);
const mappings = originals.map((original) => {
  assert.equal(hash(original.input), original.inputSha256, original.name);
  const now = current.filter((c) => c.file === original.file && c.name === original.name);
  assert.equal(now.length, 1, original.name);
  return {
    originalOccurrence: original.occurrence,
    file: original.file,
    name: original.name,
    originalInputSha256: original.inputSha256,
    currentIndex: now[0]!.index,
    currentInputSha256: now[0]!.inputSha256,
    inputChanged: original.inputSha256 !== now[0]!.inputSha256,
    target:
      original.kind === 'function'
        ? 'fn' in (now[0]!.input as object)
          ? (now[0]!.input as { fn: unknown }).fn
          : null
        : 'createScriptHost via script-host create/api',
    disposition:
      original.name === 'fields-from-snapshot'
        ? 'Current complete FormSnapshot; malformed bogus entry component retired, supported unsupported-entry witness retained'
        : original.name.startsWith('identity-') && original.inputSha256 !== now[0]!.inputSha256
          ? 'Current Identity keys; missing displayName/organization represented as undefined'
          : original.name === 'fields-empty'
            ? 'Current complete empty FormSnapshot'
            : original.kind === 'scenario'
              ? 'Exact original input retained; current create/API/cleanup driver'
              : 'Exact original input and target retained',
  };
});
const additions = current
  .filter((c) => !originals.some((o) => o.file === c.file && o.name === c.name))
  .map(({ file, name, index, inputSha256 }) => ({ file, name, index, inputSha256 }));
const traces = current.flatMap((c) => ('result' in c ? c.result.trace : []));
const summaries = {
  originals: originals.length,
  current: current.length,
  pure: current.filter((c) => c.kind === 'function').length,
  scenarios: current.filter((c) => c.kind === 'scenario').length,
  retiredWholeCases: 0,
  changedOriginalInputs: mappings.filter((m) => m.inputChanged).length,
  retainedOriginalInputs: mappings.filter((m) => !m.inputChanged).length,
  additions: additions.length,
  authoredSteps: current.reduce((n, c) => n + ('result' in c ? c.result.trace.length : 0), 0),
  constructionRefusals: traces.filter((t) => t.status === 'unconstructed').length,
  stepThrows: traces.filter((t) => t.outcome && 'threw' in t.outcome).length,
  unmatched: current.flatMap((c) => ('result' in c ? c.result.unmatched : [])).length,
  reports: traces.flatMap((t) => t.reports).length,
  pureThrows: current.filter((c) => 'outcome' in c && 'threw' in c.outcome).map((c) => c.name),
  settlements: traces.flatMap((t) => t.settled).length,
  rejectedSettlements: traces.flatMap((t) => t.settled).filter((s) => 'rejected' in s).length,
  callbacks: traces.flatMap((t) => t.callbacks).length,
  consoleCallbacks: traces.flatMap((t) => t.callbacks).filter((c) => c.fn.startsWith('console.'))
    .length,
};
write('accounting.json', {
  base: '79b16091b9950935e5df77966f4207a1eddcce07',
  summary: summaries,
  mappings,
  additions,
  partialRetirements: [
    {
      name: 'fields-from-snapshot',
      component: "{name:'bogus',valueEntry:{kind:'bogus'}}",
      evidence:
        'packages/engine/core/src/forms/value-entry.ts:2-6 has only none/scalar/array/unsupported; packages/core/acrojs/src/input.ts:10-18 has no default invalid-kind behavior. Old missing-widgets TypeError is not a semantic target; full historical definition retained.',
      replacement: 'typed text field with kind:unsupported; value and defaultValue null',
    },
  ],
});
write('source-evidence.json', {
  base: '79b16091b9950935e5df77966f4207a1eddcce07',
  originalSourceSha256: hash(readFileSync(new URL('original-acrojs.ts.txt', root), 'utf8')),
  currentSourceSha256: hash(
    readFileSync(new URL('../src/suites/acrojs.ts', import.meta.url), 'utf8'),
  ),
  originalInputsSha256: hash(readFileSync(new URL('original-inputs.json', root), 'utf8')),
  currentInputsRuntimeSha256: hash(
    readFileSync(new URL('current-inputs-runtime.json', root), 'utf8'),
  ),
  deferredInputsSha256: hash(readFileSync(new URL('deferred-inputs.json', root), 'utf8')),
  task11: {
    paths: ['core/acrojs.json', 'core/acrojs-host.json'],
    inputDiff: 0,
    disposition:
      'Neither is one of the 31 inherited Task11 drift cases. Seven fresh input migrations are explicitly mapped here.',
  },
  upstream: {
    identity: 'c78997346647b0e53f7c8d1b98b80865f05a299e packages/core/acrojs/src/host.ts:91-104',
    flags: 'c3b90c3a4c22c547d45fca2fe0b4de5936478241 packages/core/acrojs/src/input.ts',
    snapshot: '5da4ca52a packages/core/acrojs/src/input.ts:21-71',
    plugin:
      'packages/core/main/src/types.ts create/returned api/connect; packages/core/acrojs/src/host.ts:130-239 actual realm lifetime',
  },
});
console.log(JSON.stringify(summaries, null, 2));
