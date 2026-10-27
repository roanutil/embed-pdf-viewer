/**
 * Emits `vectors/files/attachments.json`: the `/EmbeddedFiles` listing and
 * what each entry decodes to.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run attachments
 *   pnpm --filter @embedpdf/tooling-engine-conformance run attachments:check
 *
 * G8's read gate. `crates/epdf-engine/tests/attachment_reader.rs` replays it.
 *
 * The bytes ARE recorded, unlike the annotation appearance vectors, and they
 * can be: an embedded file is decoded by PDFium's stream filters, which
 * produce the same bytes on every build because a deflate stream has one
 * correct decoding. There is no rasterizer in the path. They are recorded as
 * lowercase hex so the file stays diffable.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'files');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface AttachmentCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
}

const CASES: readonly AttachmentCase[] = [
  {
    name: 'tree',
    fixture: 'tooling/engine-conformance/fixtures/attachments.pdf',
    covers:
      'A key that is not the display name, an entry with no optional fields at all, a zero-byte file, two entries sharing one stream, and a filespec with no /EF whose download fails.',
  },
  {
    name: 'none',
    fixture: 'tooling/engine-conformance/fixtures/rich_metadata.pdf',
    covers:
      'A document with no /EmbeddedFiles name tree. The empty listing is a different answer from a failed one.',
  },
];

interface Downloaded {
  readonly key: string;
  readonly name?: string;
  readonly mimeType?: string;
  readonly size?: number;
  /** Lowercase hex, so the vector is diffable. */
  readonly bytes?: string;
  /** The `EngineErrorCode` when the download threw, which IS the contract. */
  readonly errorCode?: string;
  /** Its message, which is diagnostic and is compared by neither side. */
  readonly error?: string;
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

async function readCase(entry: AttachmentCase): Promise<unknown> {
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
      { kind: 'bytes', id: `attachments:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const attachments = doc.attachments;
      if (!attachments) throw new Error(`${entry.name}: the engine exposes no attachments`);
      // `list()` is `{ attachments }`, each naming itself by `ref`, since
      // a9434c0e2 ("Documents: v3 result shapes, attachments, redaction,
      // security"), and `download(ref)` returns the bytes alone since
      // c2d7a5194 ("Make attachment writes undoable change ops"): the name
      // and type are what the listing says, so they come from there. An
      // absent type is `null` in the listing.
      const { attachments: items } = await attachments.list();
      const downloads: Downloaded[] = [];
      for (const item of items) {
        try {
          const content = await attachments.download(item.ref);
          downloads.push({
            key: item.ref.key,
            name: item.name,
            ...(item.mimeType !== null ? { mimeType: item.mimeType } : {}),
            size: content.byteLength,
            bytes: hex(content),
          });
        } catch (error) {
          const code = (error as { code?: unknown }).code;
          downloads.push({
            key: item.ref.key,
            ...(typeof code === 'string' ? { errorCode: code } : {}),
            error: error instanceof Error ? error.message : String(error),
          });
        }
      }
      return { ...entry, items, downloads };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'files.attachments' },
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
  const absolute = join(vectorsRoot, 'attachments.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      files/attachments.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS files/attachments.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAttachment vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run attachments\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written files/attachments.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-attachment-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
