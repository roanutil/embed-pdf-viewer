process.env.TZ = 'UTC';
import { suites } from '../src/suites/actions';
import { runScenario } from '../src/scenario';
import { emit } from '../src/vectors';
import { canonical, canonicalError, snapshotHooks } from '../src/canonical';
import { writeFileSync } from 'node:fs';
const rows = [];
for (const suite of suites) {
  if (
    suite.kind !== 'scenario' ||
    !['plugins/actions-script.json', 'plugins/actions-hover.json'].includes(suite.file)
  )
    continue;
  for (const [index, entry] of suite.cases.entries()) {
    try {
      const result = await runScenario(entry);
      rows.push({ file: suite.file, name: entry.name, index, input: canonical(entry), result });
      console.log(
        entry.name,
        JSON.stringify({
          throws: result.trace
            .filter((s) => s.outcome && 'threw' in s.outcome)
            .map((s) => s.outcome),
          unmatched: result.unmatched,
          rejected: result.trace.flatMap((s) => s.settled).filter((s) => 'rejected' in s),
        }),
      );
    } catch (error) {
      rows.push({
        file: suite.file,
        name: entry.name,
        index,
        input: canonical(entry),
        runnerException: canonicalError(error, snapshotHooks),
      });
      console.log(entry.name, 'RUNNER', canonicalError(error, snapshotHooks));
    }
  }
  await emit([suite], 'render');
  console.log('render OK', suite.file, suite.cases.length);
}
writeFileSync(
  new URL('../historical/task-12b2c4/current-inputs-runtime.json', import.meta.url),
  JSON.stringify(rows, null, 2) + '\n',
);
