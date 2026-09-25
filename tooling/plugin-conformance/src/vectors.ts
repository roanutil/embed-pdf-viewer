/**
 * Suites, and the files they write.
 *
 * A suite is either scenarios (`scenario.ts`) or function cases: a pure
 * function called with canonical arguments, and its canonical outcome. Both
 * kinds land in `vectors/<suite>.json` under one envelope the Rust runner
 * asserts the version of.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { canonical, canonicalError, isThenable, snapshotHooks } from './canonical';
import { runScenario, type ScenarioCase } from './scenario';

/** Bump when the envelope or the trace changes shape. */
export const SCHEMA_VERSION = 1;

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const vectorsRoot = join(packageRoot, 'vectors');

export interface FunctionCase {
  readonly name: string;
  /** A function name from the suite's `functions` table. */
  readonly fn: string;
  readonly args: readonly unknown[];
  readonly covers?: string;
}

export interface ScenarioSuite {
  readonly kind: 'scenario';
  /** `vectors/<file>`, e.g. `kernel/lifecycle.json`. */
  readonly file: string;
  readonly describes: string;
  readonly cases: readonly ScenarioCase[];
}

export interface FunctionSuite {
  readonly kind: 'function';
  readonly file: string;
  readonly describes: string;
  /** The functions under test, by the name cases use. Arguments arrive
   *  decoded (`$bytes`, `$map`, `$undefined`, ...). */
  readonly functions: Readonly<Record<string, (...args: never[]) => unknown>>;
  readonly cases: readonly FunctionCase[];
}

export type Suite = ScenarioSuite | FunctionSuite;

function decodePlain(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(decodePlain);
  if (value === null || typeof value !== 'object') return value;
  const object = value as Record<string, unknown>;
  if ('$undefined' in object) return undefined;
  if ('$number' in object) return Number(object.$number);
  if ('$bytes' in object) return new Uint8Array(Buffer.from(object.$bytes as string, 'base64'));
  if ('$map' in object) {
    return new Map((object.$map as [unknown, unknown][]).map(([k, v]) => [decodePlain(k), decodePlain(v)]));
  }
  if ('$set' in object) return new Set((object.$set as unknown[]).map(decodePlain));
  const out: Record<string, unknown> = {};
  for (const [key, member] of Object.entries(object)) out[key] = decodePlain(member);
  return out;
}

async function renderSuite(suite: Suite): Promise<unknown> {
  if (suite.kind === 'function') {
    const cases = [];
    for (const entry of suite.cases) {
      const fn = suite.functions[entry.fn];
      if (!fn) throw new Error(`[${suite.file}] no function "${entry.fn}"`);
      let outcome: unknown;
      try {
        const result = (fn as (...args: unknown[]) => unknown)(...entry.args.map(decodePlain));
        if (isThenable(result)) throw new Error(`[${suite.file}] ${entry.name}: function cases must be synchronous`);
        outcome = { returned: canonical(result, snapshotHooks) };
      } catch (error) {
        outcome = { threw: canonicalError(error, snapshotHooks) };
      }
      cases.push({ ...entry, outcome });
    }
    return { schemaVersion: SCHEMA_VERSION, kind: 'function', describes: suite.describes, cases };
  }
  const cases = [];
  for (const scenario of suite.cases) {
    const { trace, unmatched } = await runScenario(scenario);
    if (unmatched.length && !scenario.allowUnmatched) {
      throw new Error(`[${suite.file}] ${scenario.name}: unanswered engine calls:\n  ${unmatched.join('\n  ')}`);
    }
    cases.push({ ...scenario, trace });
  }
  return { schemaVersion: SCHEMA_VERSION, kind: 'scenario', describes: suite.describes, cases };
}

/** Write each suite, or with `check` fail on any file whose content would change. */
export async function emit(suites: readonly Suite[], check: boolean, only?: string): Promise<void> {
  const stale: string[] = [];
  const names = new Set<string>();
  for (const suite of suites) {
    if (names.has(suite.file)) throw new Error(`two suites write ${suite.file}`);
    names.add(suite.file);
    if (only && !suite.file.startsWith(only)) continue;
    const text = `${JSON.stringify(await renderSuite(suite), null, 2)}\n`;
    const path = join(vectorsRoot, suite.file);
    if (check) {
      const current = await readFile(path, 'utf8').catch(() => null);
      if (current !== text) stale.push(suite.file);
    } else {
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, text);
      console.log(`wrote vectors/${suite.file}`);
    }
  }
  if (stale.length) {
    console.error(`stale vectors (run \`pnpm --filter @embedpdf/tooling-plugin-conformance run vectors\`):`);
    for (const file of stale) console.error(`  vectors/${file}`);
    process.exitCode = 1;
  } else if (check) {
    console.log(`${names.size} vector files are current`);
  }
}
