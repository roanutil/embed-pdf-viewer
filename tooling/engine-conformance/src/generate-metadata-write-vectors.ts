/**
 * Emits `vectors/metadata/update.json`: what `metadata.update` leaves in
 * the document information dictionary.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run metadata-write
 *   pnpm --filter @embedpdf/tooling-engine-conformance run metadata-write:check
 *
 * G14's gate. `crates/epdf-engine/tests/metadata_write.rs` replays it.
 *
 * # The three states are the whole point
 *
 * Absent leaves a field, `null` removes the key, and a value sets it. An
 * EMPTY string is a value: a document that set `/Title ()` said something
 * different from one that set no title, and the reader tells them apart.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { createLocalEngine } from '../../../packages/engine/main/src/index';

import { createRecordingEngine } from './recording-engine';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'metadata');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/** Every standard field filled, plus custom keys and a /Trapped. */
const FIXTURE = 'tooling/engine-conformance/fixtures/rich_metadata.pdf';

interface UpdateCase {
  readonly name: string;
  readonly covers: string;
  readonly patch: Record<string, unknown>;
  readonly refused?: true;
}

const CASES: readonly UpdateCase[] = [
  {
    name: 'set-one-field',
    covers: 'A single field set, and every other left exactly as it was.',
    patch: { title: 'A New Title' },
  },
  {
    name: 'clear-one-field',
    covers: 'A null removes the key, which is not the same as setting it empty.',
    patch: { title: null },
  },
  {
    name: 'set-empty-string',
    covers:
      'An empty string SETS the key to nothing. A document that says its title is blank has said something; one with no title has not.',
    patch: { title: '' },
  },
  {
    name: 'set-every-string-field',
    covers: 'All six at once.',
    patch: {
      title: 'T',
      author: 'A',
      subject: 'S',
      keywords: 'K',
      producer: 'P',
      creator: 'C',
    },
  },
  {
    name: 'clear-every-string-field',
    covers: 'And all six removed.',
    patch: {
      title: null,
      author: null,
      subject: null,
      keywords: null,
      producer: null,
      creator: null,
    },
  },
  {
    name: 'set-dates',
    covers:
      'The two dates, which arrive as ISO 8601 and are stored as PDF dates. The wire vocabulary and the file vocabulary are different vocabularies.',
    patch: { created: '2021-03-04T05:06:07Z', modified: '2022-07-08T09:10:11Z' },
  },
  {
    name: 'clear-dates',
    covers: 'And removed.',
    patch: { created: null, modified: null },
  },
  {
    name: 'set-trapped-true',
    covers:
      'The /Trapped name. Two-state, not three: the wire has no absence, so a patch leaving it alone omits the field.',
    patch: { trapped: 'true' },
  },
  {
    name: 'set-trapped-unknown',
    covers: 'The third of the three names, which is a value rather than a missing one.',
    patch: { trapped: 'unknown' },
  },
  {
    name: 'set-custom-keys',
    covers: 'Custom keys, which write straight into the same dictionary the standard fields use.',
    patch: { custom: { Department: 'Legal', Matter: '2024-118' } },
  },
  {
    name: 'clear-custom-key',
    covers: 'A custom key removed, which is per key rather than a replacement of the map.',
    patch: { custom: { 'Case-Id': null } },
  },
  {
    name: 'custom-key-shadowing-a-standard-one',
    covers:
      'A custom key named Title, which would overwrite the structured field and be read back as the title. Skipped rather than refused: the caller used the wrong door and the right one is next to it.',
    patch: { custom: { Title: 'shadow' } },
    // `metadata.custom.update` refuses this key where `metadata.update` used
    // to skip it (a4b8259f7, "Custom metadata keys are an object of their
    // own").
    refused: true,
  },
  {
    name: 'custom-key-with-a-slash',
    covers: 'A key a caller wrote as a PDF name. Skipped on the same terms.',
    patch: { custom: { '/Leading': 'x' } },
    // Refused since a4b8259f7, as above.
    refused: true,
  },
  {
    name: 'custom-key-outside-ascii',
    covers: 'A key with a character a PDF name cannot hold.',
    patch: { custom: { 'Café': 'x' } },
    // Refused since a4b8259f7, as above.
    refused: true,
  },
  {
    name: 'empty-patch',
    covers: 'A patch that says nothing, which changes nothing and is not an error.',
    patch: {},
  },
];

type OpenedDocument = Awaited<ReturnType<ReturnType<typeof createLocalEngine>['open']>>;

/**
 * Sends a case's patch the way the engine now takes it. The cases keep their
 * old spelling, translated here:
 *
 * - `created` and `modified` are `createdAt` and `modifiedAt` since
 *   fd175ba50 ("One date type IsoDateTime").
 * - `custom` is no longer part of `metadata.update`: the Info dict's other
 *   keys are `metadata.custom`, written by `metadata.custom.update`, since
 *   a4b8259f7 ("Custom metadata keys are an object of their own").
 *
 * The case records the custom keys back under `custom`, where the vector has
 * always carried them, re-read after the write so a case that only touches
 * one side still records both. A patch with both kinds of key is two
 * writes, standard first, so a refused custom key can leave the standard
 * write done: a refused case records the refusal and the metadata as it
 * stands afterwards, so that shows. `request` holds both wire requests then.
 */
async function update(
  doc: OpenedDocument,
  patch: Record<string, unknown>,
): Promise<void> {
  const { custom, created, modified, ...standard } = patch;
  const metadataPatch: Record<string, unknown> = { ...standard };
  if (created !== undefined) metadataPatch.createdAt = created;
  if (modified !== undefined) metadataPatch.modifiedAt = modified;
  // A patch of nothing but custom keys sends no standard-field write, as
  // `metadata.update` would have changed none.
  if (Object.keys(metadataPatch).length > 0 || custom === undefined) {
    await doc.metadata.update(metadataPatch as never);
  }
  if (custom !== undefined) {
    await doc.metadata.custom.update(custom as Record<string, string | null>);
  }
}

/** The Info dict as it stands, the custom keys back under `custom`. */
async function readBack(doc: OpenedDocument): Promise<Record<string, unknown>> {
  return { ...(await doc.metadata.get()), custom: await doc.metadata.custom.get() };
}

async function readCase(entry: UpdateCase): Promise<unknown> {
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
      { kind: 'bytes', id: `metadata-write:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      // `request` is the wire request the case's old spelling became (two
      // when the patch has both kinds of key), which the Rust runner
      // replays (see `recording-engine.ts`).
      const sent = await capture(() => update(doc, entry.patch));
      if (entry.refused) {
        if (sent.ok) throw new Error(`${entry.name}: expected a refusal`);
        const code = String((sent.error as { code?: unknown })?.code ?? 'unknown');
        return { ...entry, request: sent.request, refusedWith: code, metadata: await readBack(doc) };
      }
      if (!sent.ok) throw sent.error;
      return { ...entry, request: sent.request, metadata: await readBack(doc) };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'metadata.update', fixture: FIXTURE },
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
  const absolute = join(vectorsRoot, 'update.json');
  if (check) {
    const current = await readFile(absolute, 'utf8').catch(() => '');
    if (current !== text) {
      process.stderr.write('  STALE   metadata/update.json\n');
      process.exitCode = 1;
      return;
    }
    process.stdout.write('  ok      metadata/update.json\n');
    return;
  }
  await mkdir(vectorsRoot, { recursive: true });
  await writeFile(absolute, text, 'utf8');
  process.stdout.write('  written metadata/update.json\n');
}

main().catch((error) => {
  process.stderr.write(`generate-metadata-write-vectors: ${String(error?.stack ?? error)}\n`);
  process.exitCode = 1;
});
