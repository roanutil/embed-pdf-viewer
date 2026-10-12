/**
 * The completeness gate: how much of the ported TypeScript the generators
 * actually execute.
 *
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run coverage
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run coverage:check
 *
 * Runs `generate.ts --check` under c8, keeps the files this port covers
 * (`packages/core/<ported>/src`, `packages/plugin/<id>/src`), and writes
 * `vectors/coverage.json`: per file, the statement and branch counts and the
 * exact lines and branches no vector reaches. `--check` fails when that file
 * would change, in either direction: a regression names the lines a case
 * stopped reaching, and an improvement has to be committed so the next
 * regression is visible against it.
 *
 * A line in the uncovered list is a claim someone has to defend.
 * `multiplatform-plugins-status.md` gives the reason for each one that
 * remains: unreachable from the public surface, DOM-only, or a gap.
 *
 * c8 runs with `--all=false`, so a ported file no generator imports would
 * be missing from the report rather than counted as uncovered. `unloaded`
 * lists those files, found on disk: type-only modules and barrels, mostly.
 * It is held to the same either-direction check, so a new file no vector
 * loads shows up in review instead of passing unseen.
 *
 * Reaching a line is not the same as comparing what it did. A scenario case
 * with `snapshot: 'none'` records no state, so a reducer it drives is checked
 * only through the actions, outcomes and engine calls around it. A second run
 * (`generate.ts --compared-only`) leaves those cases out, and each file's
 * `unsnapshottedOnly` lists the lines only they reach. They count as covered
 * above; the list is held to the same either-direction check, so a new case
 * that skips the snapshot shows up in review as lines added here.
 */
import { spawnSync } from 'node:child_process';
import { mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';

import { vectorsRoot } from './vectors';

const PORTED = /^packages\/(core\/(main|geometry|stage|ui|annotation|acrojs|signature)|plugin\/[^/]+)\/src\//;
/** JavaScript that runs inside the script sandbox. It ships to Rust as data
 *  (`crates/plugins/acrojs/prelude.js`), not as a port, so its lines are not
 *  this gate's to count. */
const NOT_PORTED = new Set(['packages/core/acrojs/src/prelude.ts']);

const packageRoot = resolve(vectorsRoot, '..');
const repoRoot = resolve(packageRoot, '../..');
const check = process.argv.includes('--check');

interface IstanbulFile {
  statementMap: Record<string, { start: { line: number }; end: { line: number } }>;
  s: Record<string, number>;
  branchMap: Record<string, { loc: { start: { line: number; column: number } } }>;
  b: Record<string, number[]>;
}

/** Run the generators with `args` under c8 and return the raw report, keyed
 *  by repo-relative path, for the files this port covers. */
async function measure(args: readonly string[]): Promise<Map<string, IstanbulFile>> {
  const reportDir = await mkdtemp(join(tmpdir(), 'plugin-coverage-'));
  try {
    const run = spawnSync(
      'npx',
      [
        'c8',
        '--reporter=json',
        `--report-dir=${reportDir}`,
        '--all=false',
        '--allowExternal',
        '--exclude=**/node_modules/**',
        '--exclude=**/*.test.ts',
        'tsx',
        '--conditions=development',
        'src/generate.ts',
        ...args,
      ],
      { cwd: packageRoot, stdio: ['ignore', 'inherit', 'inherit'] },
    );
    if (run.status !== 0) {
      console.error('the generators failed under coverage; fix vectors:check first');
      process.exit(1);
    }
    const raw = JSON.parse(await readFile(join(reportDir, 'coverage-final.json'), 'utf8')) as Record<string, IstanbulFile>;
    const out = new Map<string, IstanbulFile>();
    for (const [absolute, file] of Object.entries(raw)) {
      const path = relative(repoRoot, absolute);
      if (PORTED.test(path) && !NOT_PORTED.has(path)) out.set(path, file);
    }
    return out;
  } finally {
    await rm(reportDir, { recursive: true, force: true });
  }
}

/** Every ported source file on disk, repo-relative: what `measure` would
 *  report if each one were loaded. */
async function portedOnDisk(): Promise<string[]> {
  const out: string[] = [];
  const walk = async (dir: string): Promise<void> => {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== 'node_modules') await walk(path);
        continue;
      }
      const rel = relative(repoRoot, path);
      if (!/\.tsx?$/.test(rel) || /\.test\.tsx?$|\.d\.ts$/.test(rel)) continue;
      if (PORTED.test(rel) && !NOT_PORTED.has(rel)) out.push(rel);
    }
  };
  await walk(join(repoRoot, 'packages/core'));
  await walk(join(repoRoot, 'packages/plugin'));
  return out.sort();
}

/** The lines of every statement `file` never ran. */
function uncoveredLinesOf(file: IstanbulFile | undefined): Set<number> {
  const lines = new Set<number>();
  if (!file) return lines;
  for (const [id, count] of Object.entries(file.s)) {
    if (count > 0) continue;
    const { start, end } = file.statementMap[id];
    for (let line = start.line; line <= end.line; line += 1) lines.add(line);
  }
  return lines;
}

/** `[3, 4, 5, 9]` as `['3-5', '9']`. */
function ranges(lines: readonly number[]): string[] {
  const out: string[] = [];
  for (let i = 0; i < lines.length; ) {
    let j = i;
    while (j + 1 < lines.length && lines[j + 1] === lines[j] + 1) j += 1;
    out.push(i === j ? `${lines[i]}` : `${lines[i]}-${lines[j]}`);
    i = j + 1;
  }
  return out;
}

const all = await measure(['--check']);
const compared = await measure(['--compared-only']);

const files: Record<string, unknown> = {};
let statements = 0;
let statementsHit = 0;
let branches = 0;
let branchesHit = 0;
let unsnapshottedLines = 0;
for (const [path, file] of all) {
  const uncoveredLines = uncoveredLinesOf(file);
  const hit = Object.values(file.s).filter((count) => count > 0).length;
  const uncoveredBranches: string[] = [];
  let branchCount = 0;
  let branchHit = 0;
  for (const [id, counts] of Object.entries(file.b)) {
    const { line, column } = file.branchMap[id].loc.start;
    counts.forEach((count, arm) => {
      branchCount += 1;
      if (count > 0) branchHit += 1;
      else uncoveredBranches.push(`${line}:${column}#${arm}`);
    });
  }
  // A file the compared run never loaded ran none of its statements there.
  const comparedFile = compared.get(path) ?? { ...file, s: Object.fromEntries(Object.keys(file.s).map((id) => [id, 0])) };
  const unsnapshotted = [...uncoveredLinesOf(comparedFile)].filter((line) => !uncoveredLines.has(line)).sort((a, b) => a - b);
  unsnapshottedLines += unsnapshotted.length;
  const total = Object.keys(file.s).length;
  statements += total;
  statementsHit += hit;
  branches += branchCount;
  branchesHit += branchHit;
  files[path] = {
    statements: `${hit}/${total}`,
    branches: `${branchHit}/${branchCount}`,
    uncoveredLines: [...uncoveredLines].sort((a, b) => a - b),
    uncoveredBranches,
    ...(unsnapshotted.length ? { unsnapshottedOnly: ranges(unsnapshotted) } : {}),
  };
}
const sorted = Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b)));
const unloaded = (await portedOnDisk()).filter((path) => !all.has(path));
const text = `${JSON.stringify(
  {
    total: {
      statements: `${statementsHit}/${statements}`,
      branches: `${branchesHit}/${branches}`,
      unsnapshottedOnlyLines: unsnapshottedLines,
      unloadedFiles: unloaded.length,
    },
    unloaded,
    files: sorted,
  },
  null,
  2,
)}\n`;
const summary = `statements ${statementsHit}/${statements}, branches ${branchesHit}/${branches}, ${unsnapshottedLines} lines reached only by snapshot-none cases, ${unloaded.length} ported files no generator loads`;
const target = join(vectorsRoot, 'coverage.json');
if (check) {
  const current = await readFile(target, 'utf8').catch(() => null);
  if (current !== text) {
    console.error('vectors/coverage.json is stale: run `pnpm --filter @embedpdf/tooling-plugin-conformance run coverage` and review the diff');
    process.exit(1);
  }
  console.log(`coverage is current: ${summary}`);
} else {
  await writeFile(target, text);
  console.log(`wrote vectors/coverage.json: ${summary}`);
}
