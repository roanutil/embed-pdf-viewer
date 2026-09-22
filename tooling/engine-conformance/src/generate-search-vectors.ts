/**
 * Emits `vectors/search/regex-dialect.json` and `vectors/search/fold.json`:
 * what `validateSearchRegex`, `matchRegex` and `foldText` actually do, case by
 * case.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run search
 *   pnpm --filter @embedpdf/tooling-engine-conformance run search:check
 *
 * This is G5's entry gate. `crates/epdf-engine/tests/search_dialect.rs` runs a
 * candidate Rust engine against the same file, and the port does not start
 * until a candidate reproduces it.
 *
 * Every offset is a UTF-16 code-unit index into the original text, which is
 * what JavaScript produces and what `PageTextSnapshot` addresses.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { foldText } from '../../../packages/engine/core/src/search/fold';
import { matchRegex, validateSearchRegex } from '../../../packages/engine/core/src/search/regex';

import { FOLD_CASES, REGEX_CASES, type FoldCase, type RegexCase } from './search-cases';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'search');

/** Bump when either envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface MatchRange {
  readonly start: number;
  readonly length: number;
}

interface RegexResult extends RegexCase {
  /** `validateSearchRegex`'s verdict. `issue` is absent when it accepted. */
  readonly valid: boolean;
  readonly issue?: string;
  /** One entry per text, in order. Absent when the pattern was refused. */
  readonly results?: readonly (readonly MatchRange[])[];
}

interface FoldResult extends FoldCase {
  readonly folded: string;
  /** Folded code unit to the UTF-16 index of the code point it came from. */
  readonly map: readonly number[];
}

function runRegexCase(entry: RegexCase): RegexResult {
  const validation = validateSearchRegex(entry.pattern);
  if (!validation.ok) {
    return { ...entry, valid: false, issue: validation.issue };
  }
  const results = entry.texts.map((text) =>
    matchRegex(text, {
      text: entry.pattern,
      regex: true,
      matchCase: entry.matchCase ?? false,
      wholeWord: entry.wholeWord ?? false,
    }),
  );
  return { ...entry, valid: true, results };
}

function runFoldCase(entry: FoldCase): FoldResult {
  const folded = foldText(entry.text, {
    keepCase: entry.keepCase,
    keepMarks: entry.keepMarks,
  });
  return { ...entry, folded: folded.folded, map: [...folded.map] };
}

function serialize(kind: string, entries: readonly unknown[]): string {
  const head = JSON.stringify({ schemaVersion: SCHEMA_VERSION, kind }, null, 2).replace(
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
    process.stdout.write(`  ok      search/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS search/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written search/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  const names = new Set<string>();
  for (const entry of [...REGEX_CASES, ...FOLD_CASES]) {
    const key = `${'pattern' in entry ? 'regex' : 'fold'}:${entry.name}`;
    if (names.has(key)) throw new Error(`duplicate case name ${key}`);
    names.add(key);
  }

  let differed = false;
  differed =
    (await emit(
      'regex-dialect.json',
      serialize('search.regex', REGEX_CASES.map(runRegexCase)),
      check,
    )) || differed;
  differed =
    (await emit('fold.json', serialize('search.fold', FOLD_CASES.map(runFoldCase)), check)) ||
    differed;

  if (check && differed) {
    process.stderr.write(
      '\nSearch vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run search\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-search-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
