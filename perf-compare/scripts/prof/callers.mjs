// node scripts/prof/callers.mjs <prefix> <regex> [depth]: who calls frames matching regex (ms, averaged).
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const S = process.env.PROF_OUT ?? fileURLToPath(new URL('../../.work/prof/out', import.meta.url));
const [prefix, re, depth = '3'] = process.argv.slice(2);
const rx = new RegExp(re), files = readdirSync(S).filter((f) => f.startsWith(`fp-${prefix}-`) && f.endsWith('.cpuprofile'));
const out = new Map();
const clean = (s) => s.replace(/\[[0-9a-f]{16}\]/g, '').replace(/<([^<>]*?) as [^<>]*?>/g, '$1').slice(0, 110);
for (const f of files) {
  const p = JSON.parse(readFileSync(`${S}/${f}`, 'utf8'));
  const byId = new Map(p.nodes.map((n) => [n.id, n])), parent = new Map();
  for (const n of p.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
  p.samples.forEach((id, i) => {
    const ms = (p.timeDeltas[i + 1] ?? 0) / 1000;
    const chain = [];
    for (let x = id; x !== undefined; x = parent.get(x)) chain.push(byId.get(x).callFrame.functionName);
    const at = chain.findIndex((n) => rx.test(n));
    if (at < 0) return;
    // the outermost match, then the frames above it that aren't serde/alloc noise
    let top = at; for (let k = at; k < chain.length; k++) if (rx.test(chain[k])) top = k;
    const above = chain.slice(top + 1).filter((n) => !/serde|dlmalloc|alloc::|core::fmt|^\(anon\)$|multivalue|js-to-wasm/.test(n)).slice(0, Number(depth)).map(clean).join(' < ');
    out.set(above, (out.get(above) ?? 0) + ms / files.length);
  });
}
for (const [k, v] of [...out].sort((a, b) => b[1] - a[1]).slice(0, 15)) console.log(v.toFixed(1).padStart(6), k);
