/**
 * TypeScript reference adapter for the Rust geometry parity tests.
 * From the repository root:
 *   pnpm exec tsx --conditions=development tooling/engine-parity/read-geometry.ts <pdf-path> <page-index>
 *
 * The page index is zero-based; relative paths resolve from the caller's cwd.
 * Success writes one PageGeometrySnapshot JSON value plus a newline to stdout.
 * Errors go to stderr with exit code 1. The parent harness owns the deadline.
 */
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { createLocalEngine } from '../../packages/engine/main/src/index';

const usage = 'Usage: read-geometry.ts <pdf-path> <page-index> (zero-based)';

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--help') {
    process.stderr.write(`${usage}\n`);
    return;
  }
  if (args.length !== 2 || !args[0] || !/^\d+$/.test(args[1])) {
    throw new Error(usage);
  }
  const [pdfPath, indexArg] = args;
  const pageIndex = Number(indexArg);
  if (!Number.isSafeInteger(pageIndex)) {
    throw new Error(`Invalid page index: ${indexArg}; expected a nonnegative safe integer`);
  }

  const bytes = new Uint8Array(await readFile(resolve(pdfPath)));
  const engine = createLocalEngine({
    runtime: {
      prefer: 'wasm',
      // Reserve stdout for the snapshot, including when PDFium logs diagnostics.
      wasm: {
        print: (message: string) => process.stderr.write(`${message}\n`),
        printErr: (message: string) => process.stderr.write(`${message}\n`),
      },
    },
  });

  let json: string;
  try {
    const doc = await engine.open(
      { kind: 'bytes', id: 'geometry-parity', bytes },
      { scope: ['*'] },
    );
    try {
      const { pages, pageCount } = await doc.pages.list();
      const page = pages.find((candidate) => candidate.index === pageIndex);
      if (!page) {
        throw new Error(
          `Page index ${pageIndex} is out of range for ${pdfPath} (${pageCount} pages)`,
        );
      }
      const snapshot = await doc.page(page.pageObjectNumber).geometry.read();
      json = JSON.stringify(snapshot, (key, value: unknown) => {
        // JSON.stringify otherwise silently converts NaN/Infinity to null.
        if (typeof value === 'number' && !Number.isFinite(value)) {
          throw new Error(`Non-finite geometry value at field ${key}: ${value}`);
        }
        return value;
      });
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }

  // Emit only after successful cleanup, so a failed invocation has no snapshot.
  process.stdout.write(`${json}\n`);
}

main().catch((error: unknown) => {
  process.stderr.write(
    `read-geometry: ${error instanceof Error ? error.message : String(error)}\n`,
  );
  process.exitCode = 1;
});
