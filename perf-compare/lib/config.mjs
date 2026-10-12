import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const DEFAULT = fileURLToPath(new URL('../arms.json', import.meta.url));
const RECIPES = new Set(['typescript', 'rust-core', 'rust-kernel', 'rust-kernel-lto']);
const KERNELS = new Set(['typescript', 'rust']);

export function loadConfig(file = DEFAULT) {
  const raw = JSON.parse(readFileSync(file, 'utf8'));
  for (const [id, w] of Object.entries(raw.worktrees)) {
    if (!/^[0-9a-f]{40}$/.test(w.sha)) throw new Error(`worktree ${id}: sha must be 40 hex characters`);
    if (!RECIPES.has(w.recipe)) throw new Error(`worktree ${id}: unknown recipe ${w.recipe}`);
    if (!w.ref) throw new Error(`worktree ${id}: ref is required`);
  }
  for (const [id, arm] of Object.entries(raw.arms)) {
    if (!raw.worktrees[arm.worktree]) throw new Error(`arm ${id}: unknown worktree ${arm.worktree}`);
    if (!KERNELS.has(arm.kernel)) throw new Error(`arm ${id}: unknown kernel ${arm.kernel}`);
  }
  for (const pair of raw.pairs) {
    for (const id of pair) {
      if (!raw.arms[id]) throw new Error(`pair ${pair.join('/')}: unknown arm ${id}`);
    }
  }
  for (const key of ['baseline', 'noiseArm']) {
    if (!raw.arms[raw[key]]) throw new Error(`${key}: unknown arm ${raw[key]}`);
  }
  return { ...raw, armOrder: Object.keys(raw.arms) };
}
