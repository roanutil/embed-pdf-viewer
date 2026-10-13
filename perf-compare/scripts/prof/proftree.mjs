// node scripts/prof/proftree.mjs <prefix> [minMs] [maxDepth]: merged call tree (by frame name), averaged.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const S = process.env.PROF_OUT ?? fileURLToPath(new URL('../../.work/prof/out', import.meta.url));
const [prefix, minMs = '1', maxDepth = '40'] = process.argv.slice(2);
const files = readdirSync(S).filter((f) => f.startsWith(`fp-${prefix}-`) && /-\d+\.cpuprofile$/.test(f) && f.replace(`fp-${prefix}-`, '').match(/^\d+\.cpuprofile$/));
const clean = (s) => s.replace(/\[[0-9a-f]{12,}\]/g, '').replace(/<(core|alloc|std|serde_core|serde_json|wasm_bindgen|js_sys)::/g, '<').replace(/\b(core|alloc|std)::/g, '');
const root = { name: '(root)', ms: 0, kids: new Map() };
for (const f of files) {
  const p = JSON.parse(readFileSync(`${S}/${f}`, 'utf8'));
  const byId = new Map(p.nodes.map((n) => [n.id, n]));
  const parent = new Map();
  for (const n of p.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
  const t = new Map();
  p.samples.forEach((id, i) => t.set(id, (t.get(id) ?? 0) + (p.timeDeltas[i + 1] ?? 0) / 1000));
  for (const [id, ms] of t) {
    const path = [];
    for (let x = id; x !== undefined; x = parent.get(x)) {
      const c = byId.get(x).callFrame;
      if (c.functionName === '(root)') break;
      path.unshift(clean(`${c.functionName || '(anon)'}${c.url.endsWith('.js') ? ':' + (c.lineNumber + 1) : ''}`));
    }
    if (path[0] === '(idle)') continue;
    let node = root; node.ms += ms / files.length;
    for (const k of path) {
      if (!node.kids.has(k)) node.kids.set(k, { name: k, ms: 0, kids: new Map() });
      node = node.kids.get(k); node.ms += ms / files.length;
    }
  }
}
const out = [];
(function walk(n, d) {
  // collapse single-child chains
  let label = n.name, cur = n;
  while (cur.kids.size === 1 && [...cur.kids.values()][0].ms > cur.ms - 0.05) { cur = [...cur.kids.values()][0]; label += ' > ' + cur.name; }
  out.push(`${'  '.repeat(d)}${n.ms.toFixed(1).padStart(6)} ${label.length > 260 ? label.slice(0, 120) + ' … ' + label.slice(-130) : label}`);
  if (d >= Number(maxDepth)) return;
  for (const k of [...cur.kids.values()].sort((a, b) => b.ms - a.ms)) if (k.ms >= Number(minMs)) walk(k, d + 1);
})(root, 0);
console.log(`${files.length} profiles\n${out.join('\n')}`);
