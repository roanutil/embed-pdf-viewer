/**
 * Emits `vectors/files/write.json`: what `pieceInfo.update`,
 * `pieceInfo.clear`, `attachments.create` and `attachments.delete` leave in
 * the document.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run files-write
 *   pnpm --filter @embedpdf/tooling-engine-conformance run files-write:check
 *
 * G14's other gate. `crates/epdf-engine/tests/files_write.rs` replays it.
 *
 * # `/LastModified` is not compared
 *
 * Every piece-info setter stamps the document's modification date, and the
 * two engines run at different instants. The Rust runner asserts its shape
 * separately, the same way `/M` is handled in the annotation suites.
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

const PIECE_INFO_FIXTURE = 'tooling/engine-conformance/fixtures/piece_info.pdf';
const ATTACHMENT_FIXTURE = 'tooling/engine-conformance/fixtures/attachments.pdf';

interface FilesCase {
  readonly name: string;
  readonly covers: string;
  /** A piece-info write, on the catalog or on a page. */
  readonly pieceInfo?: {
    readonly application: string;
    readonly onPage?: true;
    readonly patch?: Record<string, unknown>;
    readonly clear?: true;
  };
  /** An attachment write. */
  readonly attachment?: {
    readonly create?: {
      readonly name: string;
      readonly bytes: number[];
      readonly mimeType?: string;
      readonly description?: string;
    };
    readonly deleteKey?: string;
  };
  readonly refused?: true;
}

const CASES: readonly FilesCase[] = [
  {
    name: 'piece-info-set-every-type',
    covers:
      'All five value shapes in one patch. A name is not a string: it is written with the name syntax, and a reader that took it for text would quote it.',
    pieceInfo: {
      application: 'com.embedpdf',
      patch: {
        aString: 'hello',
        aNumber: 42.5,
        aBoolean: true,
        anArray: ['one', 'two'],
        aName: { name: 'Draft' },
      },
    },
  },
  {
    name: 'piece-info-clear-one-key',
    covers:
      'A null removes one key, which is not the same as setting it empty: the fork has a separate call for it.',
    pieceInfo: { application: 'com.embedpdf', patch: { aString: null } },
  },
  {
    name: 'piece-info-new-application',
    covers: 'A patch under an application the document does not have yet, which creates one.',
    pieceInfo: { application: 'SomethingElse', patch: { hello: 'world' } },
  },
  {
    name: 'piece-info-on-a-page',
    covers:
      'The page holder. The two accessor families differ only by a page object number, so a branch written the wrong way round writes one while claiming the other.',
    pieceInfo: { application: 'com.embedpdf', onPage: true, patch: { pageKey: 'page value' } },
  },
  {
    name: 'piece-info-clear-application',
    covers: 'A whole application removed, which is a different call from removing its keys.',
    pieceInfo: { application: 'com.embedpdf', clear: true },
  },
  {
    name: 'piece-info-empty-patch',
    covers: 'A patch with no keys, which changes nothing and is not an error.',
    pieceInfo: { application: 'com.embedpdf', patch: {} },
  },
  {
    name: 'piece-info-empty-application-refused',
    covers: 'An application with no name has no dictionary to write into.',
    pieceInfo: { application: '', patch: { a: 'b' } },
    refused: true,
  },
  {
    name: 'piece-info-empty-key-refused',
    covers: 'A key with no name, on the same terms.',
    pieceInfo: { application: 'com.embedpdf', patch: { '': 'b' } },
    refused: true,
  },
  {
    name: 'piece-info-empty-name-refused',
    covers: 'An empty PDF name, which is not valid syntax: a slash alone is not a name.',
    pieceInfo: { application: 'com.embedpdf', patch: { aName: { name: '' } } },
    refused: true,
  },
  {
    name: 'attachment-create',
    covers:
      'An embedded file added to the catalog name tree. PDFium derives the size, the checksum and the creation date from the bytes, so the read back proves the stream was written.',
    attachment: {
      create: {
        name: 'added.txt',
        bytes: [104, 101, 108, 108, 111],
        mimeType: 'text/plain',
        description: 'A file added after the fact.',
      },
    },
  },
  {
    name: 'attachment-create-minimal',
    covers:
      'No declared type and no description. The default is the one a reader reports for a file whose producer said nothing.',
    attachment: { create: { name: 'bare.bin', bytes: [1, 2, 3] } },
  },
  {
    name: 'attachment-create-empty',
    covers:
      'A zero-byte attachment, which is a real attachment: the stream call is made whatever the length, or the tree would name a stream that does not exist.',
    attachment: { create: { name: 'empty.bin', bytes: [] } },
  },
  {
    name: 'attachment-create-duplicate-refused',
    covers:
      'A key the tree already has. Keys are the entry identity, so a duplicate would leave an address naming two things.',
    attachment: { create: { name: 'aaa-first', bytes: [1] } },
    refused: true,
  },
  {
    name: 'attachment-create-unnamed-refused',
    covers: 'A file with no name has no key.',
    attachment: { create: { name: '', bytes: [1] } },
    refused: true,
  },
  {
    name: 'attachment-delete',
    covers:
      'An entry unlinked by key. The tree is key-sorted, so removing one shifts the indices of the entries after it while the keys stay put.',
    attachment: { deleteKey: 'mmm-middle' },
  },
  {
    name: 'attachment-delete-unknown-refused',
    covers: 'A key the tree does not have.',
    attachment: { deleteKey: 'nothing.here' },
    refused: true,
  },
];

async function readCase(entry: FilesCase): Promise<unknown> {
  const fixture = entry.pieceInfo ? PIECE_INFO_FIXTURE : ATTACHMENT_FIXTURE;
  const bytes = new Uint8Array(await readFile(join(repoRoot, fixture)));
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
      { kind: 'bytes', id: `files-write:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const listing = await doc.pages.list();
      const firstPage = listing.pages[0]!.ref;

      const run = async () => {
        if (entry.pieceInfo) {
          const holder = entry.pieceInfo.onPage
            ? doc.page(firstPage).pieceInfo
            : doc.pieceInfo;
          if (entry.pieceInfo.clear) {
            await holder.clear(entry.pieceInfo.application);
          } else {
            await holder.update(entry.pieceInfo.application, entry.pieceInfo.patch as never);
          }
          // The whole holder afterwards, plus the list of applications:
          // a clear removes one from the list and a patch may add one.
          return {
            applications: await holder.applications(),
            read: entry.pieceInfo.application
              ? await holder.read(entry.pieceInfo.application)
              : null,
          };
        }
        const attachment = entry.attachment!;
        if (attachment.create) {
          const created = await doc.attachments.create({
            data: Uint8Array.from(attachment.create.bytes),
            name: attachment.create.name,
            mimeType: attachment.create.mimeType,
            description: attachment.create.description,
          } as never);
          return { created: created.created, listed: await doc.attachments.list() };
        }
        await doc.attachments.delete({ kind: 'key', key: attachment.deleteKey! } as never);
        return { listed: await doc.attachments.list() };
      };

      if (entry.refused) {
        let code: string | null = null;
        try {
          await run();
        } catch (error) {
          code = String((error as { code?: unknown })?.code ?? 'unknown');
        }
        if (code === null) throw new Error(`${entry.name}: expected a refusal`);
        return { ...entry, refusedWith: code };
      }
      return { ...entry, result: await run() };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

/** `/LastModified` and an attachment's creation date are clock-stamped. */
function normalizeStamps(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeStamps);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) =>
      (key === 'lastModified' || key === 'creationDate') && typeof entry === 'string'
        ? [key, '<stamped>']
        : [key, normalizeStamps(entry)],
    ),
  );
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'files.write' },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases
    .map((entry) => `  ${JSON.stringify(normalizeStamps(entry))}`)
    .join(',\n');
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
  const absolute = join(vectorsRoot, 'write.json');
  if (check) {
    const current = await readFile(absolute, 'utf8').catch(() => '');
    if (current !== text) {
      process.stderr.write('  STALE   files/write.json\n');
      process.exitCode = 1;
      return;
    }
    process.stdout.write('  ok      files/write.json\n');
    return;
  }
  await mkdir(vectorsRoot, { recursive: true });
  await writeFile(absolute, text, 'utf8');
  process.stdout.write('  written files/write.json\n');
}

main().catch((error) => {
  process.stderr.write(`generate-files-write-vectors: ${String(error?.stack ?? error)}\n`);
  process.exitCode = 1;
});
