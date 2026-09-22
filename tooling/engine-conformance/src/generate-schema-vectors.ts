/**
 * Emits `vectors/schema/<type>.json`: each hand-written case from
 * `schema-cases.ts` with the shipping zod schema's verdict on it, and the
 * value it parsed to when it accepted.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run schema
 *   pnpm --filter @embedpdf/tooling-engine-conformance run schema:check
 *
 * The Rust runner is `crates/epdf-engine/tests/schema_parity.rs`. It asserts
 * that serde accepts exactly what zod accepts and re-serializes to the same
 * document. Nothing here encodes an expected verdict: if the schema changes,
 * this file changes and Rust is what goes red.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { MetadataPatchSchema } from '../../../packages/engine/core/src/wire/schemas';

import { SCHEMA_CASES, type SchemaCase, type SchemaName } from './schema-cases';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'schema');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface ParsedCase extends SchemaCase {
  /** Whether the shipping zod schema accepted the input. */
  readonly valid: boolean;
  /**
   * What the schema parsed the input to, re-serialized. Present only when
   * `valid`. Rust must decode the input and serialize back to exactly this,
   * which is what catches a three-state field collapsing absent into null.
   */
  readonly parsed?: unknown;
}

const VALIDATORS: Record<SchemaName, (input: unknown) => { valid: boolean; parsed?: unknown }> = {
  MetadataPatch: (input) => {
    const result = MetadataPatchSchema.safeParse(input);
    // JSON round-trip so an undefined-valued key from zod does not reach the
    // file as a key at all, which is what the wire would do anyway.
    return result.success
      ? { valid: true, parsed: JSON.parse(JSON.stringify(result.data)) as unknown }
      : { valid: false };
  },
};

function serialize(name: SchemaName, cases: readonly ParsedCase[]): string {
  const head = JSON.stringify({ schemaVersion: SCHEMA_VERSION, schema: name }, null, 2).replace(
    /\n}$/,
    ',',
  );
  const body = cases.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const differences: string[] = [];

  for (const [name, cases] of Object.entries(SCHEMA_CASES) as [
    SchemaName,
    readonly SchemaCase[],
  ][]) {
    const seen = new Set<string>();
    const parsed: ParsedCase[] = cases.map((entry) => {
      if (seen.has(entry.name)) throw new Error(`${name}: duplicate case name ${entry.name}`);
      seen.add(entry.name);
      return { ...entry, ...VALIDATORS[name](entry.input) };
    });

    const relative = `${name}.json`;
    const absolute = join(vectorsRoot, relative);
    const next = serialize(name, parsed);
    let current: string | undefined;
    try {
      current = await readFile(absolute, 'utf8');
    } catch {
      current = undefined;
    }
    if (current === next) {
      process.stdout.write(`  ok      schema/${relative}\n`);
      continue;
    }
    differences.push(relative);
    if (check) {
      process.stdout.write(
        `  DIFFERS schema/${relative}${current === undefined ? ' (missing)' : ''}\n`,
      );
      continue;
    }
    await mkdir(dirname(absolute), { recursive: true });
    await writeFile(absolute, next);
    process.stdout.write(`  written schema/${relative}\n`);
  }

  if (check && differences.length > 0) {
    process.stderr.write(
      `\n${differences.length} schema vector(s) differ. Regenerate with:\n` +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run schema\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-schema-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
