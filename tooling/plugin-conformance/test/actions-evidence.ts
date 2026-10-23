/** Private C2 capture; render only, never generates vectors. */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { suites } from '../src/suites/actions';
import { canonical, canonicalError, snapshotHooks } from '../src/canonical';
import { runScenario } from '../src/scenario';
import { emit } from '../src/vectors';
export const ownedFiles = [
  'plugins/actions.json',
  'plugins/actions-dispatcher.json',
  'plugins/actions-submit.json',
];
const history = new URL('../historical/task-12b2c2/', import.meta.url);
const write = (file: string, value: unknown) =>
  writeFileSync(new URL(file, history), JSON.stringify(value, null, 2) + '\n');
const hash = (value: unknown) =>
  createHash('sha256')
    .update(typeof value === 'string' ? value : JSON.stringify(value))
    .digest('hex');
interface Original {
  file: string;
  kind: string;
  name: string;
  input: unknown;
  inputSha256: string;
}
type Outcome = { returned: unknown } | { threw: unknown };
type Current = Original & { index: number } & (
    | { outcome: Outcome }
    | { result: Awaited<ReturnType<typeof runScenario>> }
  );
const current: Current[] = [];
for (const suite of suites.filter((suite) => ownedFiles.includes(suite.file))) {
  for (const [index, entry] of suite.cases.entries()) {
    const input = canonical(entry);
    const common = {
      file: suite.file,
      kind: suite.kind,
      name: entry.name,
      index,
      input,
      inputSha256: hash(input),
    };
    if (suite.kind === 'function') {
      assert.ok('fn' in entry);
      let outcome: Outcome;
      try {
        outcome = {
          returned: canonical(
            (suite.functions[entry.fn] as (...args: unknown[]) => unknown)(...entry.args),
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
  console.log(`render OK ${suite.file} ${suite.cases.length}`);
}
write('current-inputs-runtime.json', current);
const originals = JSON.parse(
  readFileSync(new URL('original-inputs.json', history), 'utf8'),
) as Original[];
const mappings = originals.map((original, originalOccurrence) => {
  assert.equal(hash(original.input), original.inputSha256);
  const found = current.filter(
    (entry) => entry.file === original.file && entry.name === original.name,
  );
  assert.equal(found.length, 1, original.name);
  const now = found[0]!;
  return {
    originalOccurrence,
    file: original.file,
    name: original.name,
    originalInputSha256: original.inputSha256,
    currentIndex: now.index,
    currentInputSha256: now.inputSha256,
    inputChanged: original.inputSha256 !== now.inputSha256,
    target:
      original.kind === 'function'
        ? (now.input as { fn: string }).fn
        : 'actionsPlugin current create/controller/host contract',
    disposition:
      'Mapped to current typed inputs and current capability/host ports; see source-evidence and partialRetirements',
  };
});
const traces = current.flatMap((entry) => ('result' in entry ? entry.result.trace : []));
const summary = {
  originals: originals.length,
  current: current.length,
  pure: current.filter((entry) => entry.kind === 'function').length,
  scenarios: current.filter((entry) => entry.kind === 'scenario').length,
  changedOriginalInputs: mappings.filter((entry) => entry.inputChanged).length,
  retainedOriginalInputs: mappings.filter((entry) => !entry.inputChanged).length,
  additions: current.filter(
    (entry) =>
      !originals.some((original) => original.file === entry.file && original.name === entry.name),
  ).length,
  constructionRefusals: traces.filter((entry) => entry.status === 'unconstructed').length,
  executedSteps: traces.filter((entry) => entry.status !== 'unconstructed').length,
  stepThrows: traces.filter((entry) => entry.outcome && 'threw' in entry.outcome).length,
  unmatched: current.flatMap((entry) => ('result' in entry ? entry.result.unmatched : [])).length,
  reports: traces.flatMap((entry) => entry.reports).length,
  pureThrows: current
    .filter((entry) => 'outcome' in entry && 'threw' in entry.outcome)
    .map((entry) => entry.name),
  settlements: traces.flatMap((entry) => entry.settled).length,
  rejected: traces.flatMap((entry) => entry.settled).filter((entry) => 'rejected' in entry).length,
  callbacks: traces.flatMap((entry) => entry.callbacks).length,
  console: traces
    .flatMap((entry) => entry.callbacks)
    .filter((entry) => entry.fn.startsWith('console.')).length,
};
write('accounting.json', { base: 'ac71d11080ebf4aa423f9e443c3686dedf517880', summary, mappings });
console.log(JSON.stringify(summary));

const archivedRuntime = JSON.parse(
  readFileSync(new URL('original-runtime.json', history), 'utf8'),
) as (Original & { outcome?: Outcome; result?: unknown; runnerException?: unknown })[];
const changedPaths = (before: unknown, after: unknown, path = '$'): string[] => {
  if (JSON.stringify(before) === JSON.stringify(after)) return [];
  if (before && after && typeof before === 'object' && typeof after === 'object') {
    const left = before as Record<string, unknown>;
    const right = after as Record<string, unknown>;
    return [...new Set([...Object.keys(left), ...Object.keys(right)])].flatMap((key) =>
      changedPaths(left[key], right[key], `${path}.${key}`),
    );
  }
  return [path];
};
const partialRetirements = [
  {
    name: 'get-action-tree',
    component:
      'direct page visible/invisible read steps, and PageLayout visible/invisible action members',
    disposition:
      'Retired two unsupported direct reads and malformed page members; page visibility triggers remain fully intact for C3.',
    evidence:
      'contract.ts ActionTreeSource page arm supports open|close only; 62c1245d1 contract already has the same restriction. This is malformed historical harness data, not removal of supported page visibility triggers.',
  },
  {
    name: 'get-action-tree',
    component: 'document without actions service expectation',
    disposition:
      'Second document now returns the empty required DocumentActionsSnapshot through actions.get; no service omission or restored old API.',
    evidence:
      'engine/DocumentHandle.ts actions is required; catalog.ts memoizes service.get; baseline fake already exposes all shared services.',
  },
  {
    name: 'policy-rows-merge-and-update-live',
    component: 'getPolicy/updatePolicy, null hide row, policyRevision counter',
    disposition:
      'Current getSettings/updateSettings policy rows, untouched hide row retained by omission; obsolete policyRevision behavior is not a current state/parity target.',
    evidence:
      'cdf4586c7 removes old policy access/update and revision-bearing state; current ActionsCapability extends SettingsApi<ActionsSettings>.',
  },
  {
    name: 'hide-form-failures',
    component: 'old per-entry form results including rejected status and nested error',
    disposition:
      'One current FormCommitSinkResult: failed with error, failed without error, applied; exact refusal behavior preserved.',
    evidence:
      'host-contract.ts FormCommitSinkResult and dispatch/run.ts interpretHide result.status check; archived original preserved in full.',
  },
];
write('accounting.json', {
  base: 'ac71d11080ebf4aa423f9e443c3686dedf517880',
  summary: {
    ...summary,
    retiredWholeCases: 0,
    retiredUnsupportedSteps: 2,
    addedBogusRootWitnessSteps: 1,
  },
  mappings: mappings.map((mapping, index) => {
    const original = originals[index]!;
    const before = archivedRuntime.find(
      (entry) => entry.file === original.file && entry.name === original.name,
    )!;
    const after = current.find(
      (entry) => entry.file === original.file && entry.name === original.name,
    )!;
    return {
      ...mapping,
      originalOutcomeSha256: hash(before.outcome ?? before.result ?? before.runnerException),
      currentOutcomeSha256: hash('outcome' in after ? after.outcome : after.result),
      changedInputPaths: changedPaths(original.input, after.input),
    };
  }),
  additions: [],
  partialRetirements,
  before: { cases: 76, authoredSteps: 321, executedSteps: 321, stepThrows: 36, runnerExceptions: 0, unmatched: 9, pureThrows: 0 },
  movedComponents: [
    {
      name: 'policy-matrix-defaults',
      original: 'root.next[11] bogus node',
      current: 'standalone malformedTree(bogus) execution in the same scenario',
      reason:
        'Valid typed trees cannot contain a bogus node. The explicitly malformed witness stays active with exact no-executor result and no diagnostic; no cast enters valid DTO construction.',
    },
  ],
});
const sourceOf = (name: string) =>
  name === 'hooks-deliver-and-isolate'
    ? 'core/main/src/context.ts event source/report port and event-hook.ts; actions/services/events.ts'
    : name === 'executors-are-last-wins-and-identity-safe'
      ? 'actions/services/ports.ts registerExecutor; actions/dispatch/run.ts'
      : [
            'resolver-failures',
            'no-resolver',
            'no-sink',
            'the-document-home',
            'handler-contract',
            'submit-policy',
          ].includes(name)
        ? 'actions/submit/perform.ts and dispatch/run.ts policy/submit branch'
        : 'actions/dispatch/run.ts; services/policy.ts; lifecycle/document-events.ts print door';
write(
  'negative-channels.json',
  current.flatMap((entry) => {
    if (!('result' in entry)) return [];
    const negatives = entry.result.trace
      .map((step, stepIndex) => ({
        stepIndex,
        returned: step.outcome && 'returned' in step.outcome ? step.outcome.returned : null,
        settled: step.settled.filter(
          (settlement) =>
            'rejected' in settlement ||
            (settlement.resolved &&
              typeof settlement.resolved === 'object' &&
              'diagnostics' in settlement.resolved &&
              Array.isArray(settlement.resolved.diagnostics) &&
              settlement.resolved.diagnostics.length > 0),
        ),
        callbacks: step.callbacks.filter(
          (call) => call.fn.includes('Diagnostic') || call.fn.startsWith('console.'),
        ),
        reports: step.reports,
        threw: step.outcome && 'threw' in step.outcome ? step.outcome.threw : null,
      }))
      .filter(
        (step) => step.settled.length || step.callbacks.length || step.reports.length || step.threw,
      );
    return negatives.length
      ? [
          {
            file: entry.file,
            name: entry.name,
            sourceOwner: sourceOf(entry.name),
            channels: negatives,
          },
        ]
      : [];
  }),
);
write('source-evidence.json', {
  base: 'ac71d11080ebf4aa423f9e443c3686dedf517880',
  originalSourceSha256: hash(readFileSync(new URL('original-actions.ts.txt', history), 'utf8')),
  currentSourceSha256: hash(
    readFileSync(new URL('../src/suites/actions.ts', import.meta.url), 'utf8'),
  ),
  currentFixtureSha256: hash(
    readFileSync(new URL('../src/suites/actions-current-fixtures.ts', import.meta.url), 'utf8'),
  ),
  originalInputsSha256: hash(readFileSync(new URL('original-inputs.json', history), 'utf8')),
  originalRuntimeSha256: hash(readFileSync(new URL('original-runtime.json', history), 'utf8')),
  currentInputsRuntimeSha256: hash(
    readFileSync(new URL('current-inputs-runtime.json', history), 'utf8'),
  ),
  preservedDeferredRuntimeSha256: hash(
    readFileSync(new URL('deferred-runtime.json', history), 'utf8'),
  ),
  preservedLinkRuntimeSha256: hash(readFileSync(new URL('link-runtime.json', history), 'utf8')),
  task11: {
    paths: ownedFiles,
    inputDiff: 0,
    disposition:
      'All three binding Task11 paths have inputDiff0; these are fresh Task12 migrations, not inherited drift fixes.',
  },
  upstream: {
    get: '83f6598d22d86e454e7f9110b078d8e0124cbba1 DocumentActionsService.read -> get',
    settingsEventsAndCreate:
      'cdf4586c7115f64d7ec80f362149c6b5bc6cc719 ActionsCapability settings/public events; createActionsController returns api/connect',
    form: '5da4ca52ad31040143d5f272a451d5dd4e2eec53 FormReader/current complete FormSnapshot; 98e5c270c sink change',
    refsAndDto:
      '83f6598d22d86e454e7f9110b078d8e0124cbba1 refs; current PdfAction.ts/PdfDestination.ts/PageLayout and complete reviewed form/annotation builders',
    unsupportedDirectVisibility:
      '62c1245d1 already limits ActionTreeSource.page to open|close; current contract.ts111 retains it, triggers154 still supports visible|invisible',
    submit:
      'actions/submit/intent.ts toFormSubmissionRequest; engine/core/src/forms/submission.ts FormSubmissionRequest ISO sentAt and typed receipt',
    negatives:
      'Exact current results/callbacks/reports/operations asserted in actions-current.test.ts, with the complete named channels in negative-channels.json',
  },
});
