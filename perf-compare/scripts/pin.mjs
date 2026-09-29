// Moves every worktree's pin to the current tip of its ref. Commit the result.
import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const file = fileURLToPath(new URL('../arms.json', import.meta.url));
const raw = JSON.parse(readFileSync(file, 'utf8'));
const only = process.argv.slice(2);
for (const [id, w] of Object.entries(raw.worktrees)) {
  if (only.length && !only.includes(id)) continue;
  const sha = execFileSync('git', ['rev-parse', `${w.ref}^{commit}`], { encoding: 'utf8' }).trim();
  if (sha !== w.sha) console.log(`${id}: ${w.sha.slice(0, 9)} -> ${sha.slice(0, 9)}`);
  w.sha = sha;
}
writeFileSync(file, JSON.stringify(raw, null, 2) + '\n');
