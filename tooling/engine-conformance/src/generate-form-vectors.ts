/**
 * Emits `vectors/forms/snapshot.json`: the reconciled field tree, its values
 * and its widgets.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run forms
 *   pnpm --filter @embedpdf/tooling-engine-conformance run forms:check
 *
 * G8's forms gate. `crates/epdf-engine/tests/form_reader.rs` replays it.
 *
 * `exportData` is NOT recorded. FDF and XFDF are serializations PDFium
 * writes, so both engines would be comparing one writer to itself, and the
 * output carries a source path that differs by working directory. The Rust
 * side asserts the export's shape directly instead.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'forms');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface FormCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
}

const CASES: readonly FormCase[] = [
  {
    name: 'families',
    fixture: 'tooling/engine-conformance/fixtures/form_fields.pdf',
    covers:
      'One field of every family: a radio group that is one field with two widgets, a checkbox whose export value is not Yes, a list box whose /V is an array, a /V that is a number, a field reachable only through the page, and a /CO slot naming a field that does not exist.',
  },
  {
    name: 'one-merged-field',
    fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
    covers:
      'A single merged field and widget, which is the shape the widget-to-field join reads.',
  },
  {
    name: 'no-form',
    fixture: 'tooling/engine-conformance/fixtures/rich_metadata.pdf',
    covers:
      'A document with no /AcroForm. An empty snapshot with a form kind of none is a different answer from a failed read.',
  },
];

async function readCase(entry: FormCase): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, entry.fixture)));
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
      { kind: 'bytes', id: `forms:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const forms = doc.forms;
      if (!forms) throw new Error(`${entry.name}: the engine exposes no forms service`);
      return { ...entry, snapshot: await forms.list() };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'forms.snapshot' },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) cases.push(await readCase(entry));
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'snapshot.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      forms/snapshot.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS forms/snapshot.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nForm vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run forms\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written forms/snapshot.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-form-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
