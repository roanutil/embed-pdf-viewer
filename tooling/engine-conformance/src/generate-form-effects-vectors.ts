/**
 * Emits `vectors/forms/effects.json`: what an ordered batch of form effects
 * leaves behind, effect by effect.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-effects
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-effects:check
 *
 * `crates/epdf-engine/tests/form_effects.rs` replays it.
 *
 * # The statuses are the thing being pinned
 *
 * `forms.applyEffects` is the one operation whose interesting output is not
 * a document state but a vocabulary: applied, unchanged, rejected, failed,
 * skipped. A caller treats each differently, and two of the distinctions are
 * easy to get subtly wrong.
 *
 * `unchanged` versus `applied` decides whether a version bump happens at
 * all, and a script that recomputes every field on every keystroke sends
 * mostly no-ops. Getting it wrong means every character typed invalidates
 * every cache downstream.
 *
 * `rejected` versus `failed` decides whether the caller can report the
 * problem to a user or has to refetch the document. Getting it wrong means
 * either a spurious refetch or a stale view nobody knows is stale.
 *
 * Neither distinction is visible in the resulting field values, which is why
 * the statuses are recorded rather than only the documents.
 *
 * # What is not here
 *
 * `failed` and `skipped`. Both need a native write that fails AFTER preflight
 * accepted it, and nothing in this corpus produces one: every write PDFium
 * refuses, it refuses for a reason preflight catches first, which is the
 * point of having a preflight. Reaching those two statuses would mean
 * injecting a fault into the engine rather than presenting it a document,
 * and a vector that did that would be recording the fault rather than the
 * behaviour. They are covered by unit tests on each side instead.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';
import { flattenFormWidgets } from './wire-page-refs';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'forms');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const FIXTURE = 'tooling/engine-conformance/fixtures/form_fields.pdf';

/** One batch, sent as one call. */
interface EffectsCase {
  readonly name: string;
  readonly covers: string;
  readonly effects: readonly unknown[];
}

const TEXT = { kind: 'objectNumber', fieldObjectNumber: 4 } as const;
const NOTES = { kind: 'objectNumber', fieldObjectNumber: 5 } as const;
const CHECKBOX = { kind: 'objectNumber', fieldObjectNumber: 6 } as const;
const RADIO = { kind: 'objectNumber', fieldObjectNumber: 9 } as const;
const COMBO = { kind: 'objectNumber', fieldObjectNumber: 10 } as const;
const LIST = { kind: 'objectNumber', fieldObjectNumber: 11 } as const;
const MISSING = { kind: 'objectNumber', fieldObjectNumber: 9999 } as const;

const CASES: readonly EffectsCase[] = [
  {
    name: 'single-value-applies',
    covers: 'The simplest batch. One effect, one write, one changed widget.',
    effects: [{ kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } }],
  },
  {
    name: 'writing-the-same-value-is-unchanged',
    covers:
      'Written once then written again. The second is a no-op, and reporting it as applied would bump the version for a script that changed nothing.',
    effects: [
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
    ],
  },
  {
    name: 'a-later-effect-sees-the-earlier-one',
    covers:
      'Ordered, not batched. The second effect writes the value the first already wrote, so it can only be unchanged if the applier re-read between them.',
    effects: [
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Grace' } },
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Grace' } },
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
    ],
  },
  {
    name: 'a-rejection-does-not-stop-the-batch',
    covers:
      'Preflight refused the first and nothing was written for it, so the second still runs. Only a failed write stops a batch.',
    effects: [
      { kind: 'setValue', ref: TEXT, value: { type: 'toggle', state: 'Yes' } },
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
    ],
  },
  {
    name: 'an-unknown-field-is-rejected',
    covers: 'Resolution happens in preflight, so a missing field never reaches a write.',
    effects: [{ kind: 'setValue', ref: MISSING, value: { type: 'text', value: 'x' } }],
  },
  {
    name: 'a-toggle-state-no-widget-offers-is-rejected',
    covers:
      'Validated against the widgets rather than left to the engine: a state nobody offers would be written and then read back as nothing.',
    effects: [{ kind: 'setValue', ref: CHECKBOX, value: { type: 'toggle', state: 'Maybe' } }],
  },
  {
    name: 'a-choice-outside-the-options-is-rejected',
    covers: 'A list box holds only its own options; anything else is unreachable through the widget.',
    effects: [{ kind: 'setValue', ref: LIST, value: { type: 'choice', values: ['Klingon'] } }],
  },
  {
    name: 'a-repeated-choice-value-is-rejected',
    covers: 'Duplicates would select one option twice, which /V cannot represent.',
    effects: [{ kind: 'setValue', ref: LIST, value: { type: 'choice', values: ['en', 'en'] } }],
  },
  {
    name: 'a-combo-box-takes-one-value',
    covers:
      'More than one is refused rather than truncated. Both values here ARE options, so the refusal can only come from the count: a pair of non-options would be caught by the membership check first and prove nothing about this one.',
    effects: [{ kind: 'setValue', ref: COMBO, value: { type: 'choice', values: ['FR', 'DE'] } }],
  },
  {
    name: 'an-editable-combo-box-takes-free-text',
    covers: 'The one case where a value outside the options is legal, because the widget can type it.',
    effects: [{ kind: 'setValue', ref: COMBO, value: { type: 'choice', values: ['Atlantis'] } }],
  },
  {
    name: 'a-single-select-list-box-takes-one-value',
    covers:
      'The list box in this fixture is multi-select, so its two-value write is accepted. The pair here names real options, which is what separates the count rule from the membership rule.',
    effects: [{ kind: 'setValue', ref: LIST, value: { type: 'choice', values: ['fr', 'ja'] } }],
  },
  {
    name: 'reset-applies-when-the-value-differs-from-the-default',
    covers:
      'The fixture ships this field holding a value that is not its /DV, so a plain reset has work to do.',
    effects: [{ kind: 'reset', refs: [TEXT] }],
  },
  {
    name: 'resetting-twice-is-unchanged-the-second-time',
    covers:
      'The no-op case for a reset, reached by resetting first: the value now equals the default and there is nothing to put back.',
    effects: [
      { kind: 'reset', refs: [TEXT] },
      { kind: 'reset', refs: [TEXT] },
    ],
  },
  {
    name: 'reset-takes-several-fields-at-once',
    covers: 'A reset button names a list, which is why this effect is the only plural one.',
    effects: [
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada' } },
      { kind: 'setValue', ref: NOTES, value: { type: 'text', value: 'note' } },
      { kind: 'reset', refs: [TEXT, NOTES] },
    ],
  },
  {
    name: 'a-push-button-cannot-be-reset',
    covers: 'It has no value to go back to, and the refusal names the family.',
    effects: [{ kind: 'reset', refs: [{ kind: 'fqn', name: 'submit' }] }],
  },
  {
    name: 'reset-with-no-fields-is-rejected',
    covers: 'An empty list is a caller assembling a request wrong rather than a reset of nothing.',
    effects: [{ kind: 'reset', refs: [] }],
  },
  {
    name: 'display-changes-the-widgets',
    covers:
      'Judged entirely by which widgets the engine redrew, because the value is untouched and there is nothing else to compare.',
    effects: [{ kind: 'setDisplay', ref: TEXT, display: 'hidden' }],
  },
  {
    name: 'display-applies-to-every-family',
    covers: 'Flags live on widgets, which every family has, so no family refuses this.',
    effects: [
      { kind: 'setDisplay', ref: RADIO, display: 'noPrint' },
      { kind: 'setDisplay', ref: LIST, display: 'noView' },
      { kind: 'setDisplay', ref: CHECKBOX, display: 'visible' },
    ],
  },
  {
    name: 'appearance-text-leaves-the-value-alone',
    covers:
      'The whole point of the effect: the widget shows the formatted string and /V still holds the raw one, so the next calculation reads a number rather than a currency symbol.',
    effects: [
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: '1234.5' } },
      { kind: 'setAppearanceText', ref: TEXT, text: '$1,234.50' },
    ],
  },
  {
    name: 'appearance-text-does-not-apply-to-a-list-box',
    covers: 'Refused in preflight; only text and combo widgets have a drawable single line.',
    effects: [{ kind: 'setAppearanceText', ref: LIST, text: 'x' }],
  },
  {
    name: 'a-radio-switch-reports-both-widgets',
    covers:
      'One write, two changed widgets: the one turning on and the one turning off. A caller redrawing only what changed needs both.',
    effects: [{ kind: 'setValue', ref: RADIO, value: { type: 'toggle', state: 'Red' } }],
  },
  {
    name: 'an-all-no-op-batch-produces-no-meta',
    covers:
      'Nothing was written, so there is no artifact, no event and no version bump. This is the common outcome for a recompute-everything script: one effect the field already satisfies, one the document cannot satisfy at all.',
    effects: [
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada Lovelace' } },
      { kind: 'setValue', ref: MISSING, value: { type: 'text', value: 'x' } },
    ],
  },
  {
    name: 'a-no-op-in-the-middle-does-not-stop-the-batch',
    covers:
      'Only a FAILED write stops a batch. Unchanged and rejected both let the rest run, which is what keeps a recompute-everything script from being truncated by its own no-ops.',
    effects: [
      { kind: 'setValue', ref: TEXT, value: { type: 'text', value: 'Ada Lovelace' } },
      { kind: 'setValue', ref: MISSING, value: { type: 'text', value: 'x' } },
      { kind: 'setValue', ref: NOTES, value: { type: 'text', value: 'note' } },
    ],
  },
];

/** A field DTO reduced to what two engines can be expected to agree on. */
function field(dto: Record<string, unknown>): unknown {
  return {
    fieldObjectNumber: dto.fieldObjectNumber,
    name: dto.name,
    family: dto.family,
    value: dto.value ?? null,
    selectedValues: dto.selectedValues ?? null,
    valueEntry: dto.valueEntry,
  };
}

async function runCase(entry: EffectsCase): Promise<unknown> {
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
      { kind: 'bytes', id: `form-effects:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const result = await doc.forms.applyEffects(entry.effects as never);
      return {
        ...entry,
        // The error MESSAGES are not recorded, only the codes: the two
        // engines word their refusals differently and a comparison of prose
        // would pin the wording rather than the behaviour.
        results: result.results.map((item) => ({
          index: item.index,
          status: item.status,
          fields: item.fields.map((dto) => field(dto as never)),
          changedWidgets: flattenFormWidgets(item.changedWidgets),
          ...(item.error ? { errorCode: String(item.error.code ?? 'unknown') } : {}),
        })),
        changedWidgets: flattenFormWidgets(result.changedWidgets),
        // Only whether there is one. Its contents are a cloud concern and
        // empty on a local engine.
        wrote: result.meta !== null,
      };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(entries: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'forms.effects', fixture: FIXTURE },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = entries.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function emit(relative: string, text: string, check: boolean): Promise<boolean> {
  const absolute = join(vectorsRoot, relative);
  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write(`  ok      forms/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS forms/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written forms/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const names = new Set<string>();
  for (const entry of CASES) {
    if (names.has(entry.name)) throw new Error(`duplicate case name ${entry.name}`);
    names.add(entry.name);
  }

  const cases: unknown[] = [];
  for (const entry of CASES) {
    cases.push(await runCase(entry));
  }

  const differed = await emit('effects.json', serialize(cases), check);
  if (check && differed) {
    process.stderr.write(
      '\nForm effects vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run form-effects\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-form-effects-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
