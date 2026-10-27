import assert from 'node:assert/strict';
import { suites } from '../src/suites/kernel';
import { runScenario } from '../src/scenario';
import { expectedRefusals } from './kernel-refusals';

const names = new Set<string>();
let steps = 0;
let refusals = 0;
for (const suite of suites) {
  if (suite.kind !== 'scenario') continue;
  for (const scenario of suite.cases) {
    assert.ok(!names.has(scenario.name), `duplicate case ${scenario.name}`);
    names.add(scenario.name);
    const result = await runScenario(scenario);
    assert.deepEqual(result.unmatched, [], `${scenario.name}: unmatched calls`);
    const errors = result.trace.flatMap((entry, step) => {
      const values = [
        entry.outcome && 'threw' in entry.outcome ? entry.outcome.threw : undefined,
        ...entry.settled.map((s) => s.rejected),
      ];
      return values
        .filter((value) => value !== undefined)
        .map((value) => {
          const {
            name,
            code = '',
            message,
          } = (value as { $error: { name: string; code?: string; message: string } }).$error;
          assert.ok(
            !/is not a function|Cannot read|no ref \d/.test(message),
            `${scenario.name}: obsolete API: ${message}`,
          );
          assert.notEqual(name, 'TypeError', `${scenario.name}: invalid call`);
          return `${step}:${name}:${code}:${message}`;
        });
    });
    assert.deepEqual(
      errors,
      expectedRefusals[scenario.name] ?? [],
      `${scenario.name}: intended refusals`,
    );
    steps += result.trace.length;
    refusals += errors.length;
    console.log(
      JSON.stringify({
        suite: suite.file,
        name: scenario.name,
        steps: result.trace.length,
        refusals: errors.length,
      }),
    );
  }
}
for (const name of Object.keys(expectedRefusals))
  assert.ok(names.has(name), `orphaned refusal expectation ${name}`);
assert.equal(names.size, 106);
console.log(JSON.stringify({ cases: names.size, steps, refusals, unmatched: 0, crashes: 0 }));
