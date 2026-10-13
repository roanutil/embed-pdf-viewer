/**
 * Rebuilds `crates/plugin-conformance/member-order-allow.json`: the paths
 * where the Rust replay doesn't compare object member order (see the README,
 * "Comparing").
 *
 *   pnpm run member-order:allow          add what the current tree fails on
 *   pnpm run member-order:allow --fresh  start from an empty list
 *
 * It runs `cargo test --workspace` with the order check on, adds every
 * member-order failure (vector file, case, path with indexes as `[]`) to the
 * list, and runs again, since the comparer reports one difference per case,
 * until a run adds nothing. Any other failure stops it. Read the diff before
 * committing: an entry belongs here only where the TypeScript passes a
 * caller's object through, or where one type's order depends on which
 * function built it. Anything else is a Rust order to fix.
 */
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

type AllowList = Record<string, Record<string, string[]>>;

const root = fileURLToPath(new URL('../../..', import.meta.url));
const listFile = path.join(root, 'crates/plugin-conformance/member-order-allow.json');
const MAX_RUNS = 60;

const read = (): AllowList => JSON.parse(readFileSync(listFile, 'utf8'));
const write = (list: AllowList) => {
  const sorted: AllowList = {};
  for (const file of Object.keys(list).sort()) {
    sorted[file] = {};
    for (const name of Object.keys(list[file]).sort()) sorted[file][name] = list[file][name];
  }
  writeFileSync(listFile, JSON.stringify(sorted, null, 2) + '\n');
};

if (process.argv.includes('--fresh')) write({});

for (let run = 1; run <= MAX_RUNS; run++) {
  const result = spawnSync('cargo', ['test', '--workspace', '-q', '--no-fail-fast'], {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, PLUGIN_CONFORMANCE_MEMBER_ORDER: '1' },
    maxBuffer: 256 * 1024 * 1024,
  });
  const output = `${result.stdout}\n${result.stderr}`;
  const list = read();
  const others: string[] = [];
  let added = 0;
  let file: string | undefined;
  for (const line of output.split('\n')) {
    const header = line.match(/^(\S+\.json): \d+ of \d+ cases differ/);
    if (header) {
      file = header[1];
      continue;
    }
    if (!file || !line.startsWith('  ')) continue;
    const order = line.match(/^\s+(.+?): (?:step \d+: )?(\$\S*): member order: /);
    if (order) {
      const [, name, at] = order;
      const pattern = at.replace(/\[\d+\]/g, '[]');
      const paths = ((list[file] ??= {})[name] ??= []);
      if (!paths.includes(pattern)) {
        paths.push(pattern);
        added++;
      }
    } else if (/^\s+\S.*?: /.test(line)) {
      others.push(`${file}: ${line.trim()}`);
    }
  }
  write(list);
  console.log(`run ${run}: ${added} added`);
  if (others.length > 0) {
    console.error(`failures that aren't member order, fix these first:\n  ${others.join('\n  ')}`);
    process.exit(1);
  }
  if (added === 0) {
    if (result.status !== 0) {
      console.error('cargo test failed without a conformance difference; see its output');
      process.exit(1);
    }
    const count = Object.values(list)
      .flatMap((cases) => Object.values(cases))
      .flat().length;
    console.log(`done: ${count} paths`);
    process.exit(0);
  }
}
console.error(`still adding paths after ${MAX_RUNS} runs`);
process.exit(1);
