/** Private C1 evidence capture. Writes historical evidence only; emit uses render mode. */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { suites } from '../src/suites/commands';
import { canonical, canonicalError, snapshotHooks } from '../src/canonical';
import { runScenario } from '../src/scenario';
import { emit } from '../src/vectors';
const root = new URL('../historical/task-12b2c5/', import.meta.url);
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
console.log('captured', current.length);
