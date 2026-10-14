/**
 * Fails when the operation matrix and the worker protocol disagree, and
 * rewrites the human-readable rendering in `OPERATIONS.md`.
 *
 * `multiplatform-port-order.md` Section 6: "CI should fail when a kind is
 * added to `worker-protocol.ts` without a row in the operation matrix this
 * document seeds." This is that check. It also runs the other direction, so a
 * row for a kind that no longer exists is a failure rather than a fossil.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run operations
 *   pnpm --filter @embedpdf/tooling-engine-conformance run operations:check
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { CONTROL_FRAMES, readWorkerRequestKinds } from './worker-kinds';
import { OPERATIONS, type OperationRow } from './operations';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const RENDERED = resolve(packageRoot, 'OPERATIONS.md');

/**
 * Counts from `multiplatform-plan.md` Section 2 and
 * `multiplatform-port-order.md` Section 6. Asserted rather than derived: they
 * are quoted in both documents, and a silent drift between the documents and
 * the code is exactly what this file exists to catch.
 */
const EXPECTED_TOTAL = 84;
const EXPECTED_CONTROL = 4;

function report(problems: string[]): never | void {
  if (problems.length === 0) {
    process.stdout.write(
      `  ok      ${OPERATIONS.length} operations, ${EXPECTED_CONTROL} of them control frames\n`,
    );
    return;
  }
  process.stderr.write(`Operation matrix is out of date:\n  ${problems.join('\n  ')}\n`);
  process.exitCode = 1;
}

/** The matrix as a table, grouped the way the port order groups it. */
function render(): string {
  const byGroup = new Map<string, OperationRow[]>();
  for (const row of OPERATIONS) {
    const rows = byGroup.get(row.group) ?? [];
    rows.push(row);
    byGroup.set(row.group, rows);
  }

  const lines = [
    '# Operation matrix',
    '',
    'Generated from `src/operations.ts`. Do not edit by hand; run',
    '`pnpm --filter @embedpdf/tooling-engine-conformance run operations`.',
    '',
    `Every one of the ${OPERATIONS.length} \`WorkerRequest\` kinds, the port group that owns it,`,
    'the ownership stage it moves under, the phase that moves it, and the',
    'cancellation case it has to document. Group assignments come from',
    '`multiplatform-port-order.md` Section 3; cancellation cases from',
    '`multiplatform-plan.md` Section 7.',
    '',
  ];

  for (const [group, rows] of byGroup) {
    lines.push(`## ${group}`, '');
    lines.push('| Operation | Stage | Phase | Cancellation | Rust | Note |');
    lines.push('| --- | --- | --- | --- | --- | --- |');
    for (const row of rows) {
      const note = row.note ? row.note.replace(/\|/g, '\\|') : '';
      lines.push(
        `| \`${row.kind}\` | ${row.stage} | ${row.phase} | ${row.cancellation} | ${row.rustStatus} | ${note} |`,
      );
    }
    lines.push('');
  }
  return `${lines.join('\n')}`;
}

function main(): void {
  const check = process.argv.includes('--check');
  const problems: string[] = [];
  const protocol = new Set(readWorkerRequestKinds());
  const matrix = new Map<string, OperationRow>();

  for (const row of OPERATIONS) {
    if (matrix.has(row.kind)) problems.push(`${row.kind}: listed twice`);
    matrix.set(row.kind, row);
  }

  for (const kind of protocol) {
    if (!matrix.has(kind)) {
      problems.push(`${kind}: in worker-protocol.ts with no row. Add one to operations.ts.`);
    }
  }
  for (const kind of matrix.keys()) {
    if (!protocol.has(kind)) {
      problems.push(`${kind}: has a row but is not a WorkerRequest kind any more.`);
    }
  }

  if (protocol.size !== EXPECTED_TOTAL) {
    problems.push(
      `worker-protocol.ts has ${protocol.size} request kinds; the plan documents ${EXPECTED_TOTAL}. ` +
        'Update multiplatform-plan.md Section 2 and multiplatform-port-order.md Section 6 together with this file.',
    );
  }

  const control = OPERATIONS.filter((row) => row.stage === 'control');
  const expectedControl = new Set<string>(CONTROL_FRAMES);
  for (const row of control) {
    if (!expectedControl.has(row.kind)) {
      problems.push(
        `${row.kind}: marked stage "control" but is not one of the four control frames.`,
      );
    }
    if (row.cancellation !== 'control') {
      problems.push(`${row.kind}: a control frame's cancellation case is "control".`);
    }
    if (row.group !== 'G3') {
      problems.push(`${row.kind}: control frames belong to G3, not ${row.group}.`);
    }
  }
  for (const kind of expectedControl) {
    const row = matrix.get(kind);
    if (row && row.stage !== 'control') {
      problems.push(`${kind}: a control frame, but its row says stage ${row.stage}.`);
    }
  }
  if (control.length !== EXPECTED_CONTROL) {
    problems.push(
      `${control.length} control frames in the matrix; the plan documents ${EXPECTED_CONTROL}.`,
    );
  }

  // Stage C is the write stage, and a write is never cancellable back to a
  // clean state. A Stage C row claiming anything else would be a claim that an
  // AbortError implies rollback, which Section 7 says it never does.
  for (const row of OPERATIONS) {
    if (row.stage === 'C' && row.cancellation === 'cooperative') {
      problems.push(
        `${row.kind}: a Stage C operation cannot be purely cooperative; say which side of the apply boundary it is on.`,
      );
    }
    if (row.cancellation === 'control' && row.stage !== 'control') {
      problems.push(`${row.kind}: only a control frame carries the "control" cancellation case.`);
    }
  }

  const rendered = render();
  let current: string | undefined;
  try {
    current = readFileSync(RENDERED, 'utf8');
  } catch {
    current = undefined;
  }
  if (current !== rendered) {
    if (check) {
      problems.push('OPERATIONS.md is stale. Run the operations script and commit the diff.');
    } else {
      writeFileSync(RENDERED, rendered);
      process.stdout.write('  written OPERATIONS.md\n');
    }
  }

  report(problems);
}

main();
