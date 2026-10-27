/**
 * Emits `vectors/wire/requests.json`: every request kind the worker protocol
 * accepts and every argument it carries.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run wire
 *   pnpm --filter @embedpdf/tooling-engine-conformance run wire:check
 *
 * This is what makes the Rust bindings checkable. `crates/epdf-engine` routes
 * one JSON envelope rather than exporting sixty-five functions, so nothing in
 * its signature says which kinds exist or what each one takes; the compiler
 * cannot catch a missing arm or a misspelled argument key.
 * `crates/epdf-engine/tests/wire_contract.rs` replays this file to catch both.
 *
 * The field TYPES are recorded as the checker prints them. They are prose: a
 * human reads them when an argument's shape is in question, and the Rust
 * asserts only the names and which of them are optional, because that is the
 * part a dispatcher can get wrong silently.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { CONTROL_FRAMES, readWorkerRequestShapes } from './worker-kinds';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'wire');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/**
 * Asserted rather than derived; both port documents quote it. 89 after the
 * catch-up to upstream b456ee4fd (the committed vectors were cut at 65).
 */
const EXPECTED_KINDS = 89;

function serialize(entries: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'wire.requests' },
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
    process.stdout.write(`  ok      wire/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS wire/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written wire/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const control = new Set<string>(CONTROL_FRAMES);
  const shapes = readWorkerRequestShapes();

  if (shapes.length !== EXPECTED_KINDS) {
    throw new Error(`expected ${EXPECTED_KINDS} request kinds, the union has ${shapes.length}`);
  }

  const cases = shapes.map((shape) => ({
    ...shape,
    // A control frame steers the worker instead of asking it for a result,
    // so it has no result to compare and is exempt from the routing check
    // that the rest of the file drives.
    control: control.has(shape.kind),
  }));

  const differed = await emit('requests.json', serialize(cases), check);
  if (check && differed) {
    process.stderr.write(
      '\nWire vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run wire\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-wire-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
