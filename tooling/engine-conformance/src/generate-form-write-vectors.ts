/**
 * Emits `vectors/forms/write.json`: what `forms.setValue` and `forms.reset`
 * leave in the document, and which widgets they say changed.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-write
 *   pnpm --filter @embedpdf/tooling-engine-conformance run form-write:check
 *
 * G12's first gate. `crates/epdf-engine/tests/form_write.rs` replays it.
 *
 * # The changed-widget list is half the point
 *
 * A field and its widgets are different objects, and a caller holding
 * rasters keyed by annotation cannot work out which ones to redraw from the
 * value alone. A radio group changes two widgets for one write: the one
 * turning on and the one turning off. So the list is recorded beside the
 * field, and a writer that reported the wrong set would pass a comparison of
 * values only.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  createLocalEngine,
  type FormFieldRef,
} from '../../../packages/engine/main/src/index';
import type { FormFieldValue } from '../../../packages/engine/core/src/forms/value';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'forms');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const FIXTURE = 'tooling/engine-conformance/fixtures/form_fields.pdf';

/** What an import or repair case does, as distinct from a value write. */
interface DocumentCase {
  readonly name: string;
  readonly covers: string;
  /** An import: the payload, in the format named or sniffed. */
  readonly importOf?: 'fdf' | 'xfdf';
  /** Text to import verbatim, instead of a round-tripped export. */
  readonly importText?: string;
  /** Whether the declared format is sent, or left to the sniffer. */
  readonly declareFormat?: boolean;
  /** A repair, and whether it bakes appearances. */
  readonly repair?: { readonly bakeAppearances: boolean };
  readonly refused?: true;
}

const DOCUMENT_CASES: readonly DocumentCase[] = [
  // FDF and XFDF interchange went in 7f0b73a3c ("Form bundles replace
  // FDF/XFDF"): `forms.exportData` and `forms.importData` are gone, and a
  // form travels as a `FormBundle` through `forms.export`, `forms.import`
  // and `forms.importValues`. Every case below imported FDF or XFDF bytes,
  // so they stay here, commented out.
  // {
  //   name: 'import-fdf-roundtrip',
  //   covers:
  //     'A document importing its own export. Every field is named and every value already matches, so nothing is skipped and the counts say the payload was understood.',
  //   importOf: 'fdf',
  //   declareFormat: true,
  // },
  // {
  //   name: 'import-xfdf-roundtrip',
  //   covers: 'The same through the other serialization.',
  //   importOf: 'xfdf',
  //   declareFormat: true,
  // },
  // {
  //   name: 'import-fdf-sniffed',
  //   covers:
  //     'The same payload with no declared format. FDF is a PDF-shaped file and XFDF is XML, so the first byte that is not whitespace decides, and a declaration that disagreed with the bytes would send the payload to the wrong parser.',
  //   importOf: 'fdf',
  // },
  // {
  //   name: 'import-xfdf-sniffed',
  //   covers: 'And the XML one, which the same byte tells apart.',
  //   importOf: 'xfdf',
  // },
  // {
  //   name: 'import-xfdf-changes-values',
  //   covers:
  //     'A payload that actually changes something, naming one field the document has and one it does not. The second is SKIPPED rather than failing: an export from one form legitimately names fields another lacks.',
  //   importText:
  //     '<?xml version="1.0" encoding="UTF-8"?>\n' +
  //     '<xfdf xmlns="http://ns.adobe.com/xfdf/"><fields>' +
  //     '<field name="billing.name"><value>Imported Name</value></field>' +
  //     '<field name="nobody.here"><value>ignored</value></field>' +
  //     '</fields></xfdf>',
  //   importOf: 'xfdf',
  //   declareFormat: true,
  // },
  // {
  //   name: 'import-xfdf-with-bom-sniffed',
  //   covers:
  //     'An XFDF payload behind a UTF-8 byte-order mark, which an exporter may write. The sniffer skips it; one that stopped at the first byte would send XML to the FDF parser.',
  //   importText:
  //     '\uFEFF<?xml version="1.0" encoding="UTF-8"?>\n' +
  //     '<xfdf xmlns="http://ns.adobe.com/xfdf/"><fields>' +
  //     '<field name="billing.name"><value>Behind A Mark</value></field>' +
  //     '</fields></xfdf>',
  // },
  // {
  //   name: 'import-format-disagrees-refused',
  //   covers:
  //     'XFDF bytes declared as FDF. The declaration wins over the sniffer, which is what makes a wrong declaration visible: the payload goes to the parser the caller named and that parser says no.',
  //   importText:
  //     '<?xml version="1.0" encoding="UTF-8"?>\n' +
  //     '<xfdf xmlns="http://ns.adobe.com/xfdf/"><fields>' +
  //     '<field name="billing.name"><value>Mislabelled</value></field>' +
  //     '</fields></xfdf>',
  //   importOf: 'fdf',
  //   declareFormat: true,
  //   refused: true,
  // },
  // {
  //   name: 'import-garbage-refused',
  //   covers: 'Bytes that are neither. The sniffer sends them to the FDF parser, which says so.',
  //   importText: 'this is not a form payload',
  //   refused: true,
  // },
  {
    name: 'repair-plain',
    covers:
      'A repair on a form that is already consistent. Nothing to link, and the fixture asks for /NeedAppearances, which stays set because nothing baked a stream to replace it.',
    repair: { bakeAppearances: false },
  },
  {
    name: 'repair-baking',
    covers:
      'The same with appearances baked. /NeedAppearances asks a viewer to draw the widgets itself, and once every widget has a stream of its own it is a request nobody needs to honour, so it can be cleared.',
    repair: { bakeAppearances: true },
  },
];

interface WriteCase {
  readonly name: string;
  readonly covers: string;
  /** How the field is addressed. */
  readonly ref: Record<string, unknown>;
  /** The value to set, or absent for a reset. */
  readonly value?: Record<string, unknown>;
  /** The engine is expected to refuse this one. */
  readonly refused?: true;
}

const CASES: readonly WriteCase[] = [
  {
    name: 'text-value',
    covers: 'The plainest write: a text field addressed by object number.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 4 },
    value: { type: 'text', value: 'Grace Hopper' },
  },
  {
    name: 'text-empty',
    covers:
      'Emptying a text field, which is a value rather than an absence: the entry stays and reads back as an empty string.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 4 },
    value: { type: 'text', value: '' },
  },
  {
    name: 'text-by-name',
    covers:
      'The same field addressed by its fully qualified name. A name reference exists for the fields that have no object number, and it resolves the ordinary ones too.',
    ref: { kind: 'fqn', name: 'billing.name' },
    value: { type: 'text', value: 'Katherine Johnson' },
  },
  {
    name: 'text-reset',
    covers:
      'A reset puts the field back to its /DV, which here is a value that differs from both the current one and the empty string.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 4 },
  },
  {
    name: 'text-reset-no-default',
    covers:
      'A reset on a field with no /DV. Nothing to go back to, so it empties rather than refusing.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 5 },
  },
  {
    name: 'checkbox-off',
    covers:
      'Clearing a checkbox. The state is null rather than the string Off: one removes /V and the other names an appearance.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 6 },
    value: { type: 'toggle', state: null },
  },
  {
    name: 'checkbox-on',
    covers:
      'Checking it again by its export value, which on this fixture is Agree rather than Yes. A writer that assumed Yes would be refused.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 6 },
    value: { type: 'toggle', state: 'Agree' },
  },
  {
    name: 'radio-switch',
    covers:
      'One write, TWO changed widgets: the one turning on and the one turning off. A value comparison alone cannot see the second.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 9 },
    value: { type: 'toggle', state: 'Red' },
  },
  {
    name: 'radio-clear-refused',
    covers:
      'Clearing a group whose /Ff has NoToggleToOff. The flag says a choice cannot be taken back once made, so PDFium refuses and the engine reports it rather than leaving the group in a state the flag forbids.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 9 },
    value: { type: 'toggle', state: null },
    refused: true,
  },
  {
    name: 'toggle-on-text-refused',
    covers:
      'A toggle value aimed at a text field. Refused before any write, because the caller has confused two fields and PDFium would otherwise answer with the code it uses for a value the field could have held.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 4 },
    value: { type: 'toggle', state: 'Yes' },
    refused: true,
  },
  {
    name: 'direct-field-refused',
    covers:
      'A field stored as a direct dictionary in /AcroForm /Fields. The reader reports it, by name, because a document that holds one is not broken; the write API addresses a field by object number and a direct dictionary has none, so it is unwritable rather than missing.',
    ref: { kind: 'fqn', name: 'inline.direct' },
    value: { type: 'text', value: 'x' },
    refused: true,
  },
  {
    name: 'missing-field-refused',
    covers: 'An object number no field has.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 9999 },
    value: { type: 'text', value: 'x' },
    refused: true,
  },
  {
    name: 'unknown-toggle-state-refused',
    covers: 'A state name the group has no widget for.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 9 },
    value: { type: 'toggle', state: 'Chartreuse' },
    refused: true,
  },
  {
    name: 'combobox-free-text',
    covers:
      'A value that is none of the options, on a combo box whose /Ff has Edit. The flag is what makes it free text rather than a refusal, and the options come back with none selected.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 10 },
    value: { type: 'choice', values: ['Atlantis'] },
  },
  {
    name: 'non-option-choice-refused',
    covers:
      'The same value against a LIST box, which has no Edit flag and therefore no free text: a value that is not one of its options is not a value it can hold.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 11 },
    value: { type: 'choice', values: ['XX'] },
    refused: true,
  },
  {
    name: 'combobox-value',
    covers: 'A combo box takes one of its options, by export value rather than by label.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 10 },
    value: { type: 'choice', values: ['DE'] },
  },
  {
    name: 'listbox-multi',
    covers: 'A list box takes several at once, and reports them in option order rather than in the order they were sent.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 11 },
    value: { type: 'choice', values: ['ja', 'en'] },
  },
  {
    name: 'listbox-none',
    covers: 'An empty selection, which is a list box with nothing chosen rather than a refused write.',
    ref: { kind: 'objectNumber', fieldObjectNumber: 11 },
    value: { type: 'choice', values: [] },
  },
];

type OpenedDocument = Awaited<ReturnType<ReturnType<typeof createLocalEngine>['open']>>;

/**
 * The cases spell refs and values the way the engine took them before
 * upstream's v3 forms; they are translated here. A field ref's
 * `fieldObjectNumber` is `objectNumber` since 595bdc6c7 ("Engine API one
 * name for object numbers"). `setValue` takes what a read returns since
 * d7bd2d277 ("Engine API v3 forms shapes"): `{ value }` for text, combo box
 * and radio, `{ checked }` for a checkbox and `{ selectedValues }` for a
 * list box. Which one depends on the field's family, so it is read first; a
 * field the form doesn't have is translated by the value's own type.
 */
function v3FieldRef(ref: Record<string, unknown>): FormFieldRef {
  return ref.kind === 'fqn'
    ? { kind: 'fqn', name: ref.name as string }
    : { kind: 'objectNumber', objectNumber: ref.fieldObjectNumber as number };
}

async function familyOf(doc: OpenedDocument, ref: FormFieldRef): Promise<string | null> {
  const { fields } = await doc.forms.list();
  const field = fields.find((candidate) =>
    ref.kind === 'fqn'
      ? candidate.name === ref.name
      : candidate.ref.kind === 'objectNumber' && candidate.ref.objectNumber === ref.objectNumber,
  );
  return field?.family ?? null;
}

function v3Value(value: Record<string, unknown>, family: string | null): FormFieldValue {
  switch (value.type) {
    case 'toggle':
      return family === 'radio'
        ? { value: value.state as string | null }
        : { checked: value.state !== null };
    case 'choice':
      return family === 'combobox'
        ? { value: (value.values as string[])[0] ?? null }
        : { selectedValues: value.values as string[] };
    default:
      return { value: value.value as string | null };
  }
}

async function readCase(entry: WriteCase): Promise<unknown> {
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
      { kind: 'bytes', id: `form-write:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const ref = v3FieldRef(entry.ref);
      const value = entry.value
        ? v3Value(entry.value, await familyOf(doc, ref))
        : undefined;
      const run = () =>
        value ? doc.forms.setValue(ref, value) : doc.forms.reset(ref);
      if (entry.refused) {
        // The MESSAGE is not recorded, only the code: the two engines word
        // their refusals differently and a comparison of prose would pin
        // the wording rather than the behaviour.
        let code: string | null = null;
        try {
          await run();
        } catch (error) {
          code = String((error as { code?: unknown })?.code ?? 'unknown');
        }
        if (code === null) throw new Error(`${entry.name}: expected a refusal`);
        return { ...entry, refusedWith: code };
      }
      const result = await run();
      // The changed widgets are `meta.changedWidgets` since 7bccc4f35
      // ("Forms and signatures: v3 names and shapes"). A reset returns the
      // fields it changed, and none for a field already at its default,
      // since d7bd2d277; the field as it stands afterwards is read back.
      return {
        ...entry,
        field: 'field' in result ? result.field : await doc.forms.get(ref),
        changedWidgets: result.meta.changedWidgets,
      };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

async function readDocumentCase(entry: DocumentCase): Promise<unknown> {
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
      { kind: 'bytes', id: `form-doc:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      if (entry.repair) {
        const result = await doc.forms.repair(entry.repair);
        const { meta: _meta, ...report } = result;
        return { ...entry, report };
      }
      throw new Error(`${entry.name}: FDF and XFDF imports were removed upstream (7f0b73a3c)`);
      /*
       * The import path, kept for the commented-out import cases above.
       * `exportData` and `importData` went in 7f0b73a3c.
       *
       * // The payload is either literal text or this document's own export,
       * // which is what makes the round-trip cases a round trip.
       * const payload = entry.importText
       *   ? new TextEncoder().encode(entry.importText)
       *   : (await doc.forms.exportData(entry.importOf)).bytes;
       * if (entry.refused) {
       *   let code: string | null = null;
       *   try {
       *     await doc.forms.importData(payload.buffer as ArrayBuffer, entry.importOf);
       *   } catch (error) {
       *     code = String((error as { code?: unknown })?.code ?? 'unknown');
       *   }
       *   if (code === null) throw new Error(`${entry.name}: expected a refusal`);
       *   return { ...entry, refusedWith: code, payload: Array.from(payload) };
       * }
       * const result = await doc.forms.importData(
       *   payload.buffer as ArrayBuffer,
       *   entry.declareFormat ? entry.importOf : undefined,
       * );
       * const { meta: _meta, snapshot, ...counts } = result as Record<string, unknown>;
       * return {
       *   ...entry,
       *   // The payload is recorded so the Rust runner imports the SAME bytes
       *   // rather than its own export, which would compare two writers.
       *   payload: Array.from(payload),
       *   counts,
       *   snapshot,
       * };
       */
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly (readonly unknown[])[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'forms.write', fixture: FIXTURE },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases[0].map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  const documents = cases[1].map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ],\n  "documentCases": [\n${documents}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) {
    try {
      cases.push(await readCase(entry));
    } catch (error) {
      process.stderr.write(`  case ${entry.name}: ${String((error as Error)?.message ?? error)}
`);
      throw error;
    }
  }
  const documents: unknown[] = [];
  for (const entry of DOCUMENT_CASES) {
    try {
      documents.push(await readDocumentCase(entry));
    } catch (error) {
      process.stderr.write(
        `  case ${entry.name}: ${String((error as Error)?.message ?? error)}\n`,
      );
      throw error;
    }
  }
  const text = serialize([cases, documents]);
  const absolute = join(vectorsRoot, 'write.json');
  if (check) {
    const current = await readFile(absolute, 'utf8').catch(() => '');
    if (current !== text) {
      process.stderr.write('  STALE   forms/write.json\n');
      process.exitCode = 1;
      return;
    }
    process.stdout.write('  ok      forms/write.json\n');
    return;
  }
  await mkdir(vectorsRoot, { recursive: true });
  await writeFile(absolute, text, 'utf8');
  process.stdout.write('  written forms/write.json\n');
}

main().catch((error) => {
  process.stderr.write(`generate-form-write-vectors: ${String(error?.stack ?? error)}\n`);
  process.exitCode = 1;
});
