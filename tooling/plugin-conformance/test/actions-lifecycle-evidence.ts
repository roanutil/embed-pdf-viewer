/** C3 private runtime capture. Rendering never writes vectors. */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { suites } from '../src/suites/actions';
import { canonical } from '../src/canonical';
import { runScenario } from '../src/scenario';
import { emit } from '../src/vectors';
const history = new URL('../historical/task-12b2c3/', import.meta.url);
const files = ['plugins/actions-triggers.json', 'plugins/actions-lifecycle.json'];
const write = (name: string, value: unknown) =>
  writeFileSync(new URL(name, history), JSON.stringify(value, null, 2) + '\n');
const hash = (value: unknown) =>
  createHash('sha256')
    .update(typeof value === 'string' ? value : JSON.stringify(value))
    .digest('hex');
const current: {
  file: string;
  kind: string;
  name: string;
  index: number;
  input: unknown;
  inputSha256: string;
  result: Awaited<ReturnType<typeof runScenario>>;
}[] = [];
for (const suite of suites.filter((suite) => files.includes(suite.file))) {
  assert.equal(suite.kind, 'scenario');
  if (suite.kind !== 'scenario') continue;
  for (const [index, entry] of suite.cases.entries())
    current.push({
      file: suite.file,
      kind: suite.kind,
      name: entry.name,
      index,
      input: canonical(entry),
      inputSha256: hash(canonical(entry)),
      result: await runScenario(entry),
    });
  await emit([suite], 'render');
  console.log(`render OK ${suite.file} ${suite.cases.length}`);
}
write('current-inputs-runtime.json', current);
const before = JSON.parse(readFileSync(new URL('original-runtime.json', history), 'utf8')) as {
  file: string;
  name: string;
  input: unknown;
  result?: unknown;
  runnerException?: unknown;
}[];
const changedPaths = (a: unknown, b: unknown, path = '$'): string[] => {
  if (JSON.stringify(a) === JSON.stringify(b)) return [];
  if (a && b && typeof a === 'object' && typeof b === 'object') {
    const left = a as Record<string, unknown>,
      right = b as Record<string, unknown>;
    return [...new Set([...Object.keys(left), ...Object.keys(right)])].flatMap((key) =>
      changedPaths(left[key], right[key], `${path}.${key}`),
    );
  }
  return [path];
};
const traces = current.flatMap((x) => x.result.trace);
const summary = {
  originals: before.length,
  current: current.length,
  authoredSteps: current.reduce((n, x) => n + (x.input as { steps: unknown[] }).steps.length, 0),
  executedSteps: traces.length,
  stepThrows: traces.filter((x) => x.outcome && 'threw' in x.outcome).length,
  unmatched: current.flatMap((x) => x.result.unmatched).length,
  reports: traces.flatMap((x) => x.reports).length,
  settlements: traces.flatMap((x) => x.settled).length,
  rejected: traces.flatMap((x) => x.settled).filter((x) => 'rejected' in x).length,
  callbacks: traces.flatMap((x) => x.callbacks).length,
  console: traces.flatMap((x) => x.callbacks).filter((x) => x.fn.startsWith('console.')).length,
};
write('accounting.json', {
  base: '04c8e8611a933d2e190724cc0f79bce921add132',
  summary,
  mappings: before.map((old, originalOccurrence) => {
    const found = current.filter((now) => now.file === old.file && now.name === old.name);
    assert.equal(found.length, 1, old.name);
    const now = found[0]!;
    return {
      originalOccurrence,
      file: old.file,
      name: old.name,
      currentIndex: now.index,
      originalInputSha256: hash(old.input),
      currentInputSha256: now.inputSha256,
      changedInputPaths: changedPaths(old.input, now.input),
      originalOutcomeSha256: hash(old.result ?? old.runnerException),
      currentOutcomeSha256: hash(now.result),
      target: 'shipping actionsPlugin dispatch/lifecycle/public APIs',
    };
  }),
});
console.log(JSON.stringify(summary));
const originalInputs = JSON.parse(
  readFileSync(new URL('original-inputs.json', history), 'utf8'),
) as { file: string; name: string; input: unknown; inputSha256: string }[];
for (const entry of originalInputs) assert.equal(hash(entry.input), entry.inputSha256);
const c2Deferred = JSON.parse(
  readFileSync(new URL('../task-12b2c2/deferred-runtime.json', history), 'utf8'),
) as { file: string }[];
assert.deepEqual(
  before,
  c2Deferred.filter((x) => files.includes(x.file)),
);
const changes = [
  {
    name: 'activate-resolves-the-annotation-tree',
    component: 'NM/revision-index/numberless annotation identities',
    disposition:
      'Removed identity spellings are archived. Current objectNumber/baseIndex refs retain matched, wrong-kind, wrong-page, missing-ref and inline-annotation cases before invocation.',
    source:
      'engine/core/src/identity/AnnotationRef.ts; actions/services/annotation-rows.ts; 83f6598d22d86e454e7f9110b078d8e0124cbba1',
  },
  {
    name: 'lifecycle-cache-and-invalidation',
    component: 'malformed annotation.updated/metadata/stream notices',
    disposition:
      'Typed confirmed annotations.updated, complete metadata.updated and stream.desynced notices replace malformed inputs. Added close/reopen/re-register/visible/close steps exercise fresh resource ownership.',
    source:
      'dispatch/triggers.ts annotationPagesOf/invalidate; engine/core/src/events/DocumentEvent.ts',
  },
  {
    name: 'run-document-verb-serializes',
    component: 'duplicated per-callback defer sequence',
    disposition:
      'First callback genuinely holds op1; distinct second callback resolves bytes-2. Both save operations now complete in order without an unanswered second op1.',
    source: 'lifecycle/document-events.ts runDocumentVerb and services/queue.ts',
  },
  {
    name: 'no-actions-service',
    component: 'absent optional actions-service expectation',
    disposition:
      'Current required actions.get returns the empty catalog; no service-omission fiction or stale read method is restored.',
    source: 'engine/DocumentHandle.ts and services/catalog.ts',
  },
  {
    name: 'headless-falls-back-to-the-first-page',
    component: 'executor installed after eager fallback completed',
    disposition:
      'Explicit pending fallback-catalog starts at bring-up and settles only after executor installation; successful O-8 is independently asserted.',
    source:
      'lifecycle/open-sequence.ts finally releaseBarrier; page-lifecycle.ts headless fallback',
  },
  {
    name: 'open-sequence-read-fails',
    component: 'unobserved eager failure before listener installation',
    disposition:
      'The actual actions.get request holds failed-open-catalog, fails after listener registration, releases fallback O-1 and later report O-3. Retry uses the current empty catalog.',
    source: 'services/catalog.ts rejection eviction; lifecycle/open-sequence.ts catch/finally',
  },
  {
    names: [
      'auto-fires-on-adapter-install',
      'open-sequence-completed-event',
      'headless-falls-back-to-the-first-page',
      'headless-with-a-report',
      'headless-with-no-pages',
      'open-sequence-read-fails',
    ],
    component: 'nonexistent page999 drain trigger',
    disposition:
      'Valid inert did-save document trigger drains the actual queue. Fabricated missing-page diagnostic is retired rather than accepted as an unnamed negative.',
    source:
      'dispatch/dispatcher.ts document event door; lifecycle/document-events.ts safe tree lookup',
  },
];
const accounting = JSON.parse(readFileSync(new URL('accounting.json', history), 'utf8'));
const originalTraces = before.flatMap((x) =>
  x.result && typeof x.result === 'object' && 'trace' in x.result
    ? (x.result as { trace: typeof traces }).trace
    : [],
);
write('accounting.json', {
  ...accounting,
  summary: {
    ...summary,
    changedInputs: accounting.mappings.filter(
      (x: { changedInputPaths: string[] }) => x.changedInputPaths.length,
    ).length,
    wholeRetirements: 0,
    additions: 0,
  },
  before: {
    cases: before.length,
    authoredSteps: before.reduce((n, x) => n + (x.input as { steps: unknown[] }).steps.length, 0),
    observedSteps: originalTraces.length,
    runnerExceptions: before.filter((x) => 'runnerException' in x).length,
    stepThrows: originalTraces.filter((x) => x.outcome && 'threw' in x.outcome).length,
    unmatched: before.flatMap((x) =>
      x.result && typeof x.result === 'object' && 'unmatched' in x.result
        ? (x.result as { unmatched: unknown[] }).unmatched
        : [],
    ).length,
  },
  partialReplacements: changes,
});
write(
  'negative-channels.json',
  current.flatMap((entry) => {
    const channels = entry.result.trace
      .map((s, stepIndex) => ({
        stepIndex,
        rejected: s.settled.filter((x) => 'rejected' in x),
        actionResults: s.settled.filter(
          (x) =>
            x.resolved &&
            typeof x.resolved === 'object' &&
            'status' in x.resolved &&
            x.resolved.status !== 'executed',
        ),
        callbacks: s.callbacks.filter(
          (x) => x.fn === 'onDiagnosticReported' || x.fn.startsWith('console.'),
        ),
        reports: s.reports,
        threw: s.outcome && 'threw' in s.outcome ? s.outcome : null,
      }))
      .filter(
        (s) =>
          s.rejected.length ||
          s.actionResults.length ||
          s.callbacks.length ||
          s.reports.length ||
          s.threw,
      );
    return channels.length
      ? [
          {
            name: entry.name,
            file: entry.file,
            sourceOwner: entry.file.endsWith('triggers.json')
              ? 'dispatch/dispatcher.ts,dispatch/triggers.ts,dispatch/run.ts'
              : entry.index < 14
                ? 'lifecycle/open-sequence.ts,page-lifecycle.ts; core context/event-hook report port'
                : 'lifecycle/document-events.ts,dispatch/run.ts; scripting/executor.ts for supplemental M14',
            channels,
          },
        ]
      : [];
  }),
);
const sourceFiles = [
  '../../../packages/plugin/actions/src/contract.ts',
  '../../../packages/plugin/actions/src/host-contract.ts',
  '../../../packages/plugin/actions/src/dispatch/dispatcher.ts',
  '../../../packages/plugin/actions/src/dispatch/triggers.ts',
  '../../../packages/plugin/actions/src/lifecycle/open-sequence.ts',
  '../../../packages/plugin/actions/src/lifecycle/page-lifecycle.ts',
  '../../../packages/plugin/actions/src/lifecycle/document-events.ts',
  '../../../packages/plugin/actions/src/services/catalog.ts',
  '../../../packages/plugin/actions/src/services/annotation-rows.ts',
  '../../../packages/plugin/actions/src/scripting/surface.ts',
  '../../../packages/plugin/actions/src/scripting/executor.ts',
  '../../../packages/engine/core/src/identity/AnnotationRef.ts',
  '../../../packages/engine/core/src/events/DocumentEvent.ts',
  '../src/suites/actions-current-fixtures.ts',
  '../src/scenario.ts',
  '../src/fake-engine.ts',
  '../src/canonical.ts',
  '../src/vectors.ts',
];
write('source-evidence.json', {
  base: '04c8e8611a933d2e190724cc0f79bce921add132',
  provenance: {
    original42: 'Exact filtered C2 deferred-runtime authority; no recapture or baseline refresh',
    originalSource:
      'BASE full actions.ts archived before source edits; owned trigger/lifecycle spans unchanged from frozen C baseline',
    task11: {
      paths: files,
      inputDiff: 0,
      disposition: 'Fresh Task12 migrations, neither path has inherited Task11 input drift',
    },
  },
  sourceHashes: Object.fromEntries(
    sourceFiles.map((path) => [path, hash(readFileSync(new URL(path, import.meta.url), 'utf8'))]),
  ),
  archiveHashes: Object.fromEntries(
    [
      'original-actions.ts.txt',
      'original-inputs.json',
      'original-runtime.json',
      'current-inputs-runtime.json',
      'accounting.json',
      'negative-channels.json',
    ].map((path) => [path, hash(readFileSync(new URL(path, history), 'utf8'))]),
  ),
  immutableAuthorities: Object.fromEntries(
    [
      '../task-12b2c2/deferred-runtime.json',
      '../task-12b2c2/current-inputs-runtime.json',
      '../task-12b2c2/link-runtime.json',
    ].map((path) => [path, hash(readFileSync(new URL(path, history), 'utf8'))]),
  ),
  rulings: {
    guardProgression:
      'Binding C3 brief authorizes exactly two active-vector paths and C2 deferred exclusion plus scripting-to-pure source span; observer/factories/builders unchanged',
    M14: 'Executor print consults print row for adapter path and rejects non-user kernel path; form surface bypasses adapter print row but rejects non-user kernel path. Supplemental typed current public-controller test preserves both; no production convergence.',
  },
  upstream: {
    getAndRefs: '83f6598d22d86e454e7f9110b078d8e0124cbba1',
    settingsEventsAndCreate: 'cdf4586c7115f64d7ec80f362149c6b5bc6cc719',
    visibility:
      'dispatch/triggers.ts planPageSteps visible/invisible annotations remain supported; ActionTreeSource.page open/close restriction never removed trigger visibility',
  },
});
