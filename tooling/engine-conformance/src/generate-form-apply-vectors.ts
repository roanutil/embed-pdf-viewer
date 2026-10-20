/**
 * Emits `vectors/forms/apply.json`: what one `doc.apply` change of form
 * script effects leaves behind, and what it answers.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-apply
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-apply:check
 *
 * It replaces `generate-form-effects-vectors.ts` and `forms/effects.json`.
 * Upstream removed `forms.applyEffects` in 98e5c270c ("Forms engine round:
 * prediction helpers, widget looks, field rights; applyEffects removed"): a
 * script run's effects are one `doc.apply` change now, its ops in order, all
 * of them or none. `formEffectOps` in `core/src/forms/effects.ts` is the one
 * mapping from an effect to its op, and the cases here are written as the ops
 * it produces, in the v3 shapes the wire carries: a field ref's
 * `objectNumber` (595bdc6c7), and a value as a read returns it, `{ value }`,
 * `{ checked }` or `{ selectedValues }` (d7bd2d277, "Engine API v3 forms
 * shapes").
 *
 * What is recorded is the change's answer: one item per op, each the field
 * and widgets that op wrote and its own meta, and the change's meta. A
 * change the engine refuses records the error code; nothing of it was
 * written. The opId is fixed per case so the answer is the same on every run.
 *
 * The effects cases that were about the per-effect status vocabulary
 * (`unchanged` against `applied`, a rejection or a no-op letting the rest of
 * the batch run, an all-no-op batch writing nothing) have no counterpart: a
 * change has no per-op status and stops at the first refusal. They stay
 * below, commented out.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';
import type { ChangeOp } from '../../../packages/engine/core/src/mutation/Change';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'forms');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const FIXTURE = 'tooling/engine-conformance/fixtures/form_fields.pdf';

/** One change, sent as one call. */
interface ApplyCase {
  readonly name: string;
  readonly covers: string;
  readonly ops: readonly ChangeOp[];
}

const TEXT = { kind: 'objectNumber', objectNumber: 4 } as const;
const NOTES = { kind: 'objectNumber', objectNumber: 5 } as const;
const RADIO = { kind: 'objectNumber', objectNumber: 9 } as const;
const COMBO = { kind: 'objectNumber', objectNumber: 10 } as const;
const LIST = { kind: 'objectNumber', objectNumber: 11 } as const;
const CHECKBOX = { kind: 'objectNumber', objectNumber: 6 } as const;
const MISSING = { kind: 'objectNumber', objectNumber: 9999 } as const;

const CASES: readonly ApplyCase[] = [
  {
    name: 'single-value-applies',
    covers: 'The simplest batch. One effect, one write, one changed widget.',
    ops: [{ type: 'forms.setValue', field: TEXT, value: { value: 'Ada' } }],
  },
  // The per-effect status vocabulary is gone with `forms.applyEffects`
  // (98e5c270c): a change answers one item per op with no `unchanged` or
  // `rejected`, and the first refusal refuses the whole change. These cases
  // were about that vocabulary, so they stay here, commented out.
  // {
  //   name: 'writing-the-same-value-is-unchanged',
  //   covers:
  //     'Written once then written again. The second is a no-op, and reporting it as applied would bump the version for a script that changed nothing.',
  //   effects: [
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
  //   ],
  // },
  // {
  //   name: 'a-later-effect-sees-the-earlier-one',
  //   covers:
  //     'Ordered, not batched. The second effect writes the value the first already wrote, so it can only be unchanged if the applier re-read between them.',
  //   effects: [
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Grace' } },
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Grace' } },
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
  //   ],
  // },
  // {
  //   name: 'a-rejection-does-not-stop-the-batch',
  //   covers:
  //     'Preflight refused the first and nothing was written for it, so the second still runs. Only a failed write stops a batch.',
  //   effects: [
  //     { kind: 'setValue', ref: TEXT, value: { type: 'toggle', state: 'Yes' } },
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
  //   ],
  // },
  {
    name: 'an-unknown-field-is-rejected',
    covers: 'Resolution happens in preflight, so a missing field never reaches a write.',
    ops: [{ type: 'forms.setValue', field: MISSING, value: { value: 'x' } }],
  },
  // A checkbox's value is `{ checked }` since d7bd2d277: a write can't name
  // a state any more, so a state no widget offers can't be sent. This case
  // stays here, commented out.
  // {
  //   name: 'a-toggle-state-no-widget-offers-is-rejected',
  //   covers:
  //     'Validated against the widgets rather than left to the engine: a state nobody offers would be written and then read back as nothing.',
  //   effects: [{ kind: 'setValue', ref: CHECKBOX, value: { type: 'toggle', state: 'Maybe' } }],
  // },
  {
    name: 'a-choice-outside-the-options-is-rejected',
    covers: 'A list box holds only its own options; anything else is unreachable through the widget.',
    ops: [{ type: 'forms.setValue', field: LIST, value: { selectedValues: ['Klingon'] } }],
  },
  {
    name: 'a-repeated-choice-value-is-rejected',
    covers: 'Duplicates would select one option twice, which /V cannot represent.',
    ops: [{ type: 'forms.setValue', field: LIST, value: { selectedValues: ['en', 'en'] } }],
  },
  {
    name: 'a-combo-box-takes-one-value',
    covers:
      'More than one is refused rather than truncated. Both values here ARE options, so the refusal can only come from the count: a pair of non-options would be caught by the membership check first and prove nothing about this one.',
    ops: [{ type: 'forms.setValue', field: COMBO, value: { selectedValues: ['FR', 'DE'] } }],
  },
  {
    name: 'an-editable-combo-box-takes-free-text',
    covers: 'The one case where a value outside the options is legal, because the widget can type it.',
    ops: [{ type: 'forms.setValue', field: COMBO, value: { value: 'Atlantis' } }],
  },
  {
    name: 'a-single-select-list-box-takes-one-value',
    covers:
      'The list box in this fixture is multi-select, so its two-value write is accepted. The pair here names real options, which is what separates the count rule from the membership rule.',
    ops: [{ type: 'forms.setValue', field: LIST, value: { selectedValues: ['fr', 'ja'] } }],
  },
  {
    name: 'reset-applies-when-the-value-differs-from-the-default',
    covers:
      'The fixture ships this field holding a value that is not its /DV, so a plain reset has work to do.',
    ops: [{ type: 'forms.reset', fields: [TEXT] }],
  },
  // See the first commented-out group: the second reset's `unchanged` was
  // the subject.
  // {
  //   name: 'resetting-twice-is-unchanged-the-second-time',
  //   covers:
  //     'The no-op case for a reset, reached by resetting first: the value now equals the default and there is nothing to put back.',
  //   effects: [
  //     { kind: 'reset', refs: [TEXT] },
  //     { kind: 'reset', refs: [TEXT] },
  //   ],
  // },
  {
    name: 'reset-takes-several-fields-at-once',
    covers: 'A reset button names a list, which is why this effect is the only plural one.',
    ops: [
      { type: 'forms.setValue', field: TEXT, value: { value: 'Ada' } },
      { type: 'forms.setValue', field: NOTES, value: { value: 'note' } },
      { type: 'forms.reset', fields: [TEXT, NOTES] },
    ],
  },
  {
    name: 'a-push-button-cannot-be-reset',
    covers: 'It has no value to go back to, and the refusal names the family.',
    ops: [{ type: 'forms.reset', fields: [{ kind: 'fqn', name: 'submit' }] }],
  },
  {
    name: 'reset-with-no-fields-is-rejected',
    covers: 'An empty list is a caller assembling a request wrong rather than a reset of nothing.',
    ops: [{ type: 'forms.reset', fields: [] }],
  },
  {
    name: 'display-changes-the-widgets',
    covers:
      'Judged entirely by which widgets the engine redrew, because the value is untouched and there is nothing else to compare.',
    ops: [{ type: 'forms.setDisplay', field: TEXT, display: 'hidden' }],
  },
  {
    name: 'display-applies-to-every-family',
    covers: 'Flags live on widgets, which every family has, so no family refuses this.',
    ops: [
      { type: 'forms.setDisplay', field: RADIO, display: 'noPrint' },
      { type: 'forms.setDisplay', field: LIST, display: 'noView' },
      { type: 'forms.setDisplay', field: CHECKBOX, display: 'visible' },
    ],
  },
  {
    name: 'appearance-text-leaves-the-value-alone',
    covers:
      'The whole point of the effect: the widget shows the formatted string and /V still holds the raw one, so the next calculation reads a number rather than a currency symbol.',
    ops: [
      { type: 'forms.setValue', field: TEXT, value: { value: '1234.5' } },
      { type: 'forms.setAppearanceText', field: TEXT, text: '$1,234.50' },
    ],
  },
  {
    name: 'appearance-text-does-not-apply-to-a-list-box',
    covers: 'Refused in preflight; only text and combo widgets have a drawable single line.',
    ops: [{ type: 'forms.setAppearanceText', field: LIST, text: 'x' }],
  },
  {
    name: 'a-radio-switch-reports-both-widgets',
    covers:
      'One write, two changed widgets: the one turning on and the one turning off. A caller redrawing only what changed needs both.',
    ops: [{ type: 'forms.setValue', field: RADIO, value: { value: 'Red' } }],
  },
  // See the first commented-out group: these were about a batch writing
  // nothing, and running on, when some effects are no-ops or refused.
  // {
  //   name: 'an-all-no-op-batch-produces-no-meta',
  //   covers:
  //     'Nothing was written, so there is no artifact, no event and no version bump. This is the common outcome for a recompute-everything script: one effect the field already satisfies, one the document cannot satisfy at all.',
  //   effects: [
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada Lovelace' } },
  //     { kind: 'setValue', ref: MISSING, value: { type: 'text', value: 'x' } },
  //   ],
  // },
  // {
  //   name: 'a-no-op-in-the-middle-does-not-stop-the-batch',
  //   covers:
  //     'Only a FAILED write stops a batch. Unchanged and rejected both let the rest run, which is what keeps a recompute-everything script from being truncated by its own no-ops.',
  //   effects: [
  //     { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada Lovelace' } },
  //     { kind: 'setValue', ref: MISSING, value: { type: 'text', value: 'x' } },
  //     { kind: 'setValue', ref: NOTES, value: { type: 'text', value: 'note' } },
  //   ],
  // },
];

async function runCase(entry: ApplyCase): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, FIXTURE)));
  const engine = createLocalEngine({
    runtime: {
      prefer: 'wasm',
      wasm: {
        print: (message: string) => process.stderr.write(`${message}\n`),
        printErr: (message: string) => process.stderr.write(`${message}\n`),
      },
    },
  });
  try {
    const doc = await engine.open(
      { kind: 'bytes', id: `form-apply:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      try {
        const result = await doc.apply({ ops: entry.ops }, { opId: `form-apply:${entry.name}` });
        return { ...entry, result };
      } catch (error) {
        // The MESSAGE is not recorded, only the code: the two engines word
        // their refusals differently.
        const code = (error as { code?: unknown })?.code;
        if (code === undefined) throw error;
        return { ...entry, refusedWith: String(code) };
      }
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(entries: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'document.apply.forms', fixture: FIXTURE },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = entries.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const names = new Set<string>();
  for (const entry of CASES) {
    if (names.has(entry.name)) throw new Error(`duplicate case name ${entry.name}`);
    names.add(entry.name);
  }

  const cases: unknown[] = [];
  for (const entry of CASES) cases.push(await runCase(entry));
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'apply.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      forms/apply.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS forms/apply.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nForm apply vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run form-apply\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written forms/apply.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-form-apply-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
