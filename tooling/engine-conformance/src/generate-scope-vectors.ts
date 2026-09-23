/**
 * Emits `vectors/scope/parse.json` and `vectors/scope/resolve.json`: what the
 * scope grammar accepts and refuses, and what a scope array grants once
 * `pdf.permissions` has expanded.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run scope
 *   pnpm --filter @embedpdf/tooling-engine-conformance run scope:check
 *
 * This is G3's gate. `crates/epdf-engine/tests/scope_conformance.rs` runs the
 * Rust resolver against the same files. The two implementations are a
 * deliberate duplicate rather than a port, so these vectors are the only thing
 * that keeps them the same rule.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { decodePdfBits } from '../../../packages/engine/core/src/auth/scope/pdf-bits';
import { parseScope } from '../../../packages/engine/core/src/auth/scope/parser';
import {
  checkCapability,
  checkCollab,
  checkSetGroup,
  expandRawScope,
} from '../../../packages/engine/core/src/auth/scope/resolver';
import type {
  DocCapability,
  PdfBits,
} from '../../../packages/engine/core/src/auth/scope/types';

import {
  ALL_CAPABILITIES,
  BITS_CASES,
  PARSE_CASES,
  RESOLVE_CASES,
  type ParseCase,
  type ResolveCase,
} from './scope-cases';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'scope');

/** Bump when either envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface ParseResult extends ParseCase {
  readonly valid: boolean;
  /** The parsed shape, verbatim, when it parsed. */
  readonly parsed?: unknown;
  /** `InvalidScope`'s message, when it did not. Diagnostic, not a contract. */
  readonly issue?: string;
}

interface BitsResult {
  readonly name: string;
  readonly raw: number | null;
  readonly bits: PdfBits;
}

interface PerBitsResult {
  readonly bits: string;
  /** Every granted capability, sorted, so the file does not encode Set order. */
  readonly expanded: readonly string[];
  /**
   * `checkCapability` for every capability in the closed set. Redundant with
   * `expanded` except under a wildcard, which short-circuits before expansion;
   * that difference is the reason both are recorded.
   */
  readonly granted: readonly string[];
  readonly collab?: readonly { readonly probe: string; readonly allowed: boolean }[];
  readonly setGroup?: readonly { readonly probe: string; readonly allowed: boolean }[];
}

interface ResolveResult extends ResolveCase {
  readonly results: readonly PerBitsResult[];
}

const BITS_BY_NAME = new Map(BITS_CASES.map((b) => [b.name, b]));

function runParseCase(entry: ParseCase): ParseResult {
  try {
    return { ...entry, valid: true, parsed: parseScope(entry.scope) };
  } catch (error) {
    return { ...entry, valid: false, issue: error instanceof Error ? error.message : String(error) };
  }
}

function runResolveCase(entry: ResolveCase): ResolveResult {
  const identity = entry.identity ?? {};
  const results = entry.bits.map((name): PerBitsResult => {
    const declared = BITS_BY_NAME.get(name);
    if (!declared) throw new Error(`resolve case ${entry.name} names unknown bits ${name}`);
    const bits = decodePdfBits(declared.raw);

    const expanded = [...expandRawScope(entry.scope, bits)].sort();
    const granted = ALL_CAPABILITIES.filter((c) =>
      checkCapability(c as DocCapability, entry.scope, bits),
    );

    const collab = entry.collab?.map((probe) => ({
      probe: `${probe.action}:${probe.target.userId ?? '-'}/${probe.target.groupId ?? '-'}`,
      allowed: checkCollab(probe.action, probe.target, entry.scope, identity, bits),
    }));
    const setGroup = entry.setGroup?.map((probe) => ({
      probe: `${probe.newGroupId}<-${probe.callerDefaultGroupId ?? '-'}`,
      allowed: checkSetGroup(probe.newGroupId, probe.callerDefaultGroupId, entry.scope, bits),
    }));

    return { bits: name, expanded, granted, ...(collab && { collab }), ...(setGroup && { setGroup }) };
  });
  return { ...entry, results };
}

function serialize(kind: string, extra: Record<string, unknown>, entries: readonly unknown[]): string {
  const head = JSON.stringify({ schemaVersion: SCHEMA_VERSION, kind, ...extra }, null, 2).replace(
    /\n}$/,
    ',',
  );
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
    process.stdout.write(`  ok      scope/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(`  DIFFERS scope/${relative}${current === undefined ? ' (missing)' : ''}\n`);
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written scope/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  for (const [label, entries] of [
    ['parse', PARSE_CASES],
    ['resolve', RESOLVE_CASES],
  ] as const) {
    const names = new Set<string>();
    for (const entry of entries) {
      if (names.has(entry.name)) throw new Error(`duplicate ${label} case name ${entry.name}`);
      names.add(entry.name);
    }
  }

  // The decoded bit table travels with the resolve file, so the Rust side
  // checks its own decoder against this one rather than trusting that two
  // spellings of ISO 32000 bit positions agree.
  const bitsTable: readonly BitsResult[] = BITS_CASES.map((b) => ({
    name: b.name,
    raw: b.raw,
    bits: decodePdfBits(b.raw),
  }));

  let differed = false;
  differed =
    (await emit(
      'parse.json',
      serialize('scope.parse', {}, PARSE_CASES.map(runParseCase)),
      check,
    )) || differed;
  differed =
    (await emit(
      'resolve.json',
      serialize(
        'scope.resolve',
        { capabilities: ALL_CAPABILITIES, bits: bitsTable },
        RESOLVE_CASES.map(runResolveCase),
      ),
      check,
    )) || differed;

  if (check && differed) {
    process.stderr.write(
      '\nScope vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run scope\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-scope-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
