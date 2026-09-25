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
 */
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';

import { vectorsRoot } from './vectors';

const PORTED = /^packages\/(core\/(main|geometry|stage|ui|annotation|acrojs)|plugin\/[^/]+)\/src\//;

const packageRoot = resolve(vectorsRoot, '..');
const repoRoot = resolve(packageRoot, '../..');
const check = process.argv.includes('--check');

interface IstanbulFile {
  statementMap: Record<string, { start: { line: number }; end: { line: number } }>;
  s: Record<string, number>;
  branchMap: Record<string, { loc: { start: { line: number; column: number } } }>;
  b: Record<string, number[]>;
}

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
      '--check',
    ],
    { cwd: packageRoot, stdio: ['ignore', 'inherit', 'inherit'] },
  );
  if (run.status !== 0) {
    console.error('the generators failed under coverage; fix vectors:check first');
    process.exit(1);
  }
  const raw = JSON.parse(await readFile(join(reportDir, 'coverage-final.json'), 'utf8')) as Record<string, IstanbulFile>;

  const files: Record<string, unknown> = {};
  let statements = 0;
  let statementsHit = 0;
  let branches = 0;
  let branchesHit = 0;
  for (const [absolute, file] of Object.entries(raw)) {
    const path = relative(repoRoot, absolute);
    if (!PORTED.test(path)) continue;
    const uncoveredLines = new Set<number>();
    let hit = 0;
    for (const [id, count] of Object.entries(file.s)) {
      if (count > 0) {
        hit += 1;
        continue;
      }
      const { start, end } = file.statementMap[id];
      for (let line = start.line; line <= end.line; line += 1) uncoveredLines.add(line);
    }
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
    };
  }
  const sorted = Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b)));
  const text = `${JSON.stringify(
    {
      total: { statements: `${statementsHit}/${statements}`, branches: `${branchesHit}/${branches}` },
      files: sorted,
    },
    null,
    2,
  )}\n`;
  const target = join(vectorsRoot, 'coverage.json');
  if (check) {
    const current = await readFile(target, 'utf8').catch(() => null);
    if (current !== text) {
      console.error('vectors/coverage.json is stale: run `pnpm --filter @embedpdf/tooling-plugin-conformance run coverage` and review the diff');
      process.exit(1);
    }
    console.log('coverage is current');
  } else {
    await writeFile(target, text);
    console.log(`wrote vectors/coverage.json: statements ${statementsHit}/${statements}, branches ${branchesHit}/${branches}`);
  }
} finally {
  await rm(reportDir, { recursive: true, force: true });
}
