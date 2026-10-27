/**
 * Emits `vectors/forms/field.json`: what `forms.create`, `forms.update`,
 * `forms.delete` and `forms.removeWidget` (once `createField`,
 * `updateField`, `deleteField` and `detachWidget`) leave in the document.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-field
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-field:check
 *
 * G12's structural gate. `crates/epdf-engine/tests/form_field.rs` replays it.
 *
 * # Each case starts from the fixture and says what it did
 *
 * The object numbers a create hands out depend on what the document already
 * holds, so every case opens a fresh copy: sharing one would make each
 * case's numbers depend on the cases before it.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  toPageRef,
  type FormFieldRef,
  type PageRef,
  type PdfRect,
} from '../../../packages/engine/main/src/index';
import type { FormFieldDraft } from '../../../packages/engine/core/src/forms/draft';
import { pageBoxOf } from '../../../packages/engine/core/src/geometry/pageSpace';

import { createRecordingEngine } from './recording-engine';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'forms');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const FIXTURE = 'tooling/engine-conformance/fixtures/form_fields.pdf';

/** The fixture's only page. */
const PAGE = 3;

interface FieldCase {
  readonly name: string;
  readonly covers: string;
  /** Create a field, then optionally act on it. */
  readonly draft?: Record<string, unknown>;
  /** Patch an existing field (or the one just created). */
  readonly patchOf?: Record<string, unknown>;
  readonly patch?: Record<string, unknown>;
  /** Delete a field. */
  readonly deleteOf?: Record<string, unknown>;
  /** Attach or detach a widget, naming it by object number. */
  readonly attachTo?: Record<string, unknown>;
  readonly detachFrom?: Record<string, unknown>;
  readonly widgetObjectNumber?: number;
  readonly onState?: string;
  readonly refused?: true;
}

const CASES: readonly FieldCase[] = [
  {
    name: 'create-text-bare',
    covers:
      'A field with a name and nothing else. No widget, so it appears nowhere on any page: legal, and what an import-only field looks like.',
    draft: { family: 'text', name: 'new.plain' },
  },
  {
    name: 'create-text-full',
    covers:
      'Every member a text draft carries, including a widget. The field and the widget are born apart and joined by a third call.',
    draft: {
      family: 'text',
      name: 'new.full',
      multiline: true, required: true,
      alternateName: 'Tell us more',
      mappingName: 'more_export',
      defaultValue: 'nothing yet',
      maxLength: 200,
      widget: { pageObjectNumber: PAGE, rect: { left: 72, bottom: 300, right: 300, top: 340 } },
    },
  },
  {
    name: 'create-checkbox-default-state',
    covers:
      'A checkbox widget with no onState. Yes is the default, which is the conventional token and not the only legal one.',
    draft: {
      family: 'checkbox',
      name: 'new.box',
      widget: { pageObjectNumber: PAGE, rect: { left: 72, bottom: 260, right: 92, top: 280 } },
    },
  },
  {
    name: 'create-radio-group',
    covers:
      'ONE field with TWO widgets, which is the ISO radio model. Each carries the state it turns on with, because that is how the group tells them apart.',
    draft: {
      family: 'radio',
      name: 'new.colour',
      noToggleToOff: true,
      widgets: [
        {
          pageObjectNumber: PAGE,
          rect: { left: 72, bottom: 220, right: 92, top: 240 },
          onState: 'Cyan',
        },
        {
          pageObjectNumber: PAGE,
          rect: { left: 102, bottom: 220, right: 122, top: 240 },
          onState: 'Magenta',
        },
      ],
    },
  },
  {
    name: 'create-combobox-with-options',
    covers: 'A choice field and its options, which carry a label and an export value separately.',
    draft: {
      family: 'combobox',
      name: 'new.size',
      edit: true,
      options: [
        { label: 'Small', value: 'S' },
        { label: 'Large', value: 'L' },
      ],
      widget: { pageObjectNumber: PAGE, rect: { left: 320, bottom: 300, right: 500, top: 330 } },
    },
  },
  {
    name: 'create-listbox-styled',
    covers:
      'A widget with an appearance, which goes through the widget-plane writer the annotation kind uses, so a widget placed here and one created through annotations.create cannot drift.',
    draft: {
      family: 'listbox',
      name: 'new.langs',
      multiSelect: true,
      options: [
        { label: 'Welsh', value: 'cy' },
        { label: 'Irish', value: 'ga' },
      ],
      widget: {
          pageObjectNumber: PAGE,
          rect: { left: 320, bottom: 200, right: 500, top: 280 },
          appearance: {
            color: { r: 40, g: 40, b: 40 },
            interiorColor: { r: 250, g: 250, b: 240 },
            strokeWidth: 2,
            borderStyle: 'inset',
            fontFamily: 'helvetica',
            fontSize: 10,
          },
        },
    },
  },
  {
    name: 'create-text-comb',
    covers:
      'The comb flag, which draws fixed character cells and is meaningful only with a maxLength. Its bit is 25 in the spec table and 24 as a shift, which is where a table read straight off §12.7.4 goes wrong by one.',
    draft: { family: 'text', name: 'new.comb', comb: true, maxLength: 8 },
  },
  {
    name: 'create-unnamed-refused',
    covers: 'A field with no name. A field is addressed by name, so one without is unreachable.',
    draft: { family: 'text', name: '' },
    refused: true,
  },
  {
    name: 'create-text-with-options-refused',
    covers:
      'Options on a family that has none. They would be written and then never read, which is a draft saying something the field cannot mean.',
    draft: { family: 'text', name: 'new.odd', options: [{ label: 'A', value: 'a' }] },
    refused: true,
  },
  // `attachWidget` adopted an existing widget annotation into a field. It
  // went in d7bd2d277 ("Engine API v3 forms shapes"): `addWidget(ref,
  // placement)` creates the widget instead, and nothing adopts one. This
  // case and `attach-already-attached-refused` were about adoption, so they
  // stay here, commented out.
  // {
  //   name: 'attach-off-state-refused',
  //   covers:
  //     'Adopting into a toggle with the Off state. A toggle addresses its widgets by the state they turn ON with, so one named Off could never be turned on.',
  //   attachTo: { kind: 'objectNumber', fieldObjectNumber: 9 },
  //   widgetObjectNumber: 14,
  //   onState: 'Off',
  //   refused: true,
  // },
  {
    name: 'create-name-collision-refused',
    covers: 'A name a sibling already has.',
    draft: { family: 'text', name: 'notes' },
    refused: true,
  },
  {
    name: 'create-radio-without-state-refused',
    covers:
      'A radio widget with no onState. The group addresses its widgets by state, so one without could never be turned on.',
    draft: {
      family: 'radio',
      name: 'new.bad',
      widgets: [{ pageObjectNumber: PAGE, rect: { left: 72, bottom: 180, right: 92, top: 200 } }],
    },
    refused: true,
  },
  {
    name: 'update-rename',
    covers: 'A field renamed, which changes its own /T segment rather than moving it.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 5 },
    patch: { family: 'text', name: 'renamed' },
  },
  {
    name: 'update-flags-and-names',
    covers: 'Flags set and cleared in one write, plus the two optional names.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 4 },
    patch: {
      family: 'text',
      required: false, readOnly: true,
      alternateName: 'Renamed label',
      mappingName: 'renamed_export',
    },
  },
  {
    name: 'update-clear-names',
    covers:
      'The tri-state clears. There is no remover for either name, so a clear writes an empty string and the reader reports an empty /TU as absent.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 4 },
    patch: { family: 'text', alternateName: null, mappingName: null },
  },
  {
    name: 'update-clear-default-value',
    covers:
      'Removing /DV, which is not the same as setting it to nothing: one entry goes away and the other holds an empty string.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 4 },
    patch: { family: 'text', defaultValue: null },
  },
  {
    name: 'update-max-length-clear',
    covers: 'Clearing the limit, which is writing zero: an absent /MaxLen means no limit.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 4 },
    patch: { family: 'text', maxLength: null },
  },
  {
    name: 'update-options',
    covers:
      'A replacement option list. PDFium re-syncs the selection: an export value that is no longer an option is dropped.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 11 },
    patch: {
      family: 'listbox',
      options: [
        { label: 'English', value: 'en' },
        { label: 'Welsh', value: 'cy' },
      ],
    },
  },
  {
    name: 'update-family-mismatch-refused',
    covers:
      'A text patch aimed at a list box. Refused before anything is written, because the members that follow are family-specific.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 11 },
    patch: { family: 'text', name: 'wrong' },
    refused: true,
  },
  {
    name: 'update-rename-collision-refused',
    covers: 'A rename onto a name a sibling already has.',
    patchOf: { kind: 'objectNumber', fieldObjectNumber: 5 },
    patch: { family: 'text', name: 'quantity' },
    refused: true,
  },
  {
    name: 'delete-merged-field-refused',
    covers:
      'A MERGED field/widget dictionary, which is the ordinary AcroForm shape. The field goes and the same object is then asked to go as an annotation, and the annotation plane refuses to delete a widget. Recorded as the refusal it is rather than worked around.',
    deleteOf: { kind: 'objectNumber', fieldObjectNumber: 5 },
    // Upstream deletes a merged field now, field and widget in one change
    // (60602f579, "Forms and signatures: behaviour fixes"). The case keeps
    // its name and records what the delete leaves.
  },
  {
    name: 'delete-radio-group',
    covers: 'A group deleted, which detaches BOTH its widgets.',
    deleteOf: { kind: 'objectNumber', fieldObjectNumber: 9 },
  },
  {
    name: 'detach-widget',
    covers:
      'A widget released from its field. The annotation stays on the page and stops being a control.',
    detachFrom: { kind: 'objectNumber', fieldObjectNumber: 9 },
    widgetObjectNumber: 7,
  },
  {
    name: 'detach-foreign-widget-refused',
    covers: 'A widget that belongs to a different field.',
    detachFrom: { kind: 'objectNumber', fieldObjectNumber: 9 },
    widgetObjectNumber: 4,
    refused: true,
  },
  // See `attach-off-state-refused`: adoption went in d7bd2d277.
  // {
  //   name: 'attach-already-attached-refused',
  //   covers: 'A widget that already belongs to a field cannot be adopted by a second one.',
  //   attachTo: { kind: 'objectNumber', fieldObjectNumber: 6 },
  //   widgetObjectNumber: 7,
  //   onState: 'Red',
  //   refused: true,
  // },
];

/**
 * The cases spell field refs, drafts and widgets the way the engine took
 * them before upstream's v3 forms; they are translated here.
 *
 * - A field ref's `fieldObjectNumber` is `objectNumber` since 595bdc6c7
 *   ("Engine API one name for object numbers").
 * - `createField`, `updateField` and `deleteField` are `create`, `update` and
 *   `delete` (7bccc4f35, "Forms and signatures: v3 names and shapes").
 * - Every draft takes `widgets`, with the style flat on each and a toggle's
 *   `onState` as its `exportValue` (d7bd2d277, "Engine API v3 forms
 *   shapes"); colours are `'#rrggbb'` (c053c3d33). A widget names its page
 *   as a `PageRef` and its place in page space (c053c3d33, "Page space,
 *   forms and measure"), converted through the page's visible box.
 */
function v3FieldRef(ref: Record<string, unknown>): FormFieldRef {
  return { kind: 'objectNumber', objectNumber: ref.fieldObjectNumber as number };
}

function hexOf(value: unknown): unknown {
  if (value === null || typeof value !== 'object') return value;
  const { r, g, b } = value as { r: number; g: number; b: number };
  return `#${[r, g, b].map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
}

const COLOUR_KEYS = new Set(['color', 'interiorColor', 'fontColor']);

function v3FieldDraft(
  draft: Record<string, unknown>,
  pages: readonly { ref: PageRef; pdfCropBox: PdfRect }[],
): FormFieldDraft {
  const { widget, widgets, ...rest } = draft;
  const placements = [
    ...(widget ? [widget] : []),
    ...((widgets as unknown[] | undefined) ?? []),
  ] as Record<string, unknown>[];
  if (placements.length === 0) return rest as unknown as FormFieldDraft;
  return {
    ...rest,
    widgets: placements.map((placement) => {
      const { pageObjectNumber, rect, onState, appearance, ...other } = placement;
      const page = pages.find((candidate) => candidate.ref.objectNumber === pageObjectNumber);
      if (!page) throw new Error(`no page ${String(pageObjectNumber)}`);
      const style = Object.fromEntries(
        Object.entries((appearance as Record<string, unknown> | undefined) ?? {}).map(
          ([key, value]) => [key, COLOUR_KEYS.has(key) ? hexOf(value) : value],
        ),
      );
      return {
        ...other,
        ...style,
        page: page.ref,
        rect: pageBoxOf(rect as PdfRect, page.pdfCropBox),
        ...(onState !== undefined ? { exportValue: onState } : {}),
      };
    }),
  } as unknown as FormFieldDraft;
}

async function readCase(entry: FieldCase): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, FIXTURE)));
  const { engine, capture } = createRecordingEngine({
    prefer: 'wasm',
    wasm: {
      print: (message: string) => process.stderr.write(`${message}\n`),
      printErr: (message: string) => process.stderr.write(`${message}\n`),
    },
  });
  try {
    const doc = await engine.open(
      { kind: 'bytes', id: `form-field:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const { pages } = await doc.pages.list();
      const run = async () => {
        if (entry.draft) {
          return { field: (await doc.forms.create(v3FieldDraft(entry.draft, pages))).field };
        }
        if (entry.patchOf) {
          return {
            field: (await doc.forms.update(v3FieldRef(entry.patchOf), entry.patch as never)).field,
          };
        }
        if (entry.deleteOf) {
          // The result is `{ meta }` since 7bccc4f35 ("Forms and signatures:
          // v3 names and shapes"): what went is in `meta.changedFields` and
          // `meta.changedWidgets`.
          const result = await doc.forms.delete(v3FieldRef(entry.deleteOf));
          return {
            changedFields: result.meta.changedFields,
            changedWidgets: result.meta.changedWidgets,
          };
        }
        // `detachWidget` is `removeWidget`, naming the widget by its
        // annotation ref (7bccc4f35).
        return {
          field: (
            await doc.forms.removeWidget(v3FieldRef(entry.detachFrom!), {
              kind: 'objectNumber',
              page: toPageRef(PAGE),
              objectNumber: entry.widgetObjectNumber!,
            })
          ).field,
        };
      };

      // `request` is the wire request the case's old spelling became, which
      // the Rust runner replays (see `recording-engine.ts`).
      const sent = await capture(run);
      if (entry.refused) {
        if (sent.ok) throw new Error(`${entry.name}: expected a refusal`);
        const code = String((sent.error as { code?: unknown })?.code ?? 'unknown');
        return { ...entry, request: sent.request, refusedWith: code };
      }
      if (!sent.ok) throw sent.error;
      return { ...entry, request: sent.request, result: sent.value };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'forms.field', fixture: FIXTURE },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) {
    try {
      cases.push(await readCase(entry));
    } catch (error) {
      process.stderr.write(`  case ${entry.name}: ${String((error as Error)?.message ?? error)}\n`);
      throw error;
    }
  }
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'field.json');
  if (check) {
    const current = await readFile(absolute, 'utf8').catch(() => '');
    if (current !== text) {
      process.stderr.write('  STALE   forms/field.json\n');
      process.exitCode = 1;
      return;
    }
    process.stdout.write('  ok      forms/field.json\n');
    return;
  }
  await mkdir(vectorsRoot, { recursive: true });
  await writeFile(absolute, text, 'utf8');
  process.stdout.write('  written forms/field.json\n');
}

main().catch((error) => {
  process.stderr.write(`generate-form-field-vectors: ${String(error?.stack ?? error)}\n`);
  process.exitCode = 1;
});
