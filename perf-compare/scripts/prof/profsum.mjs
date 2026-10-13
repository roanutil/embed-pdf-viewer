// node scripts/prof/profsum.mjs <glob prefix> [top]: self and inclusive time per function,
// averaged over the profiles fp-<prefix>-*.cpuprofile.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const S = process.env.PROF_OUT ?? fileURLToPath(new URL('../../.work/prof/out', import.meta.url));
const [prefix, top = '25'] = process.argv.slice(2);
const files = readdirSync(S).filter((f) => f.startsWith(`fp-${prefix}-`) && f.endsWith('.cpuprofile'));
const self = new Map(), incl = new Map(), cat = new Map();
const short = (u) => u.split('/').pop().replace(/-[A-Za-z0-9_]{8}\.js$/, '.js');
for (const f of files) {
  const p = JSON.parse(readFileSync(`${S}/${f}`, 'utf8'));
  const byId = new Map(p.nodes.map((n) => [n.id, n]));
  const parent = new Map();
  for (const n of p.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
  const key = (n) => { const c = n.callFrame; return `${c.functionName || '(anon)'} ${short(c.url)}:${c.lineNumber + 1}`; };
  const t = new Map();
  p.samples.forEach((id, i) => t.set(id, (t.get(id) ?? 0) + (p.timeDeltas[i + 1] ?? 0) / 1000));
  for (const [id, ms] of t) {
    const n = byId.get(id);
    self.set(key(n), (self.get(key(n)) ?? 0) + ms / files.length);
    const u = n.callFrame.url;
    const c = /^\((idle|program|garbage collector|root)\)$/.test(n.callFrame.functionName) ? n.callFrame.functionName : u.startsWith('wasm') ? 'wasm' : short(u) || '(native/no url)';
    cat.set(c, (cat.get(c) ?? 0) + ms / files.length);
    const seen = new Set();
    for (let x = id; x !== undefined; x = parent.get(x)) {
      const k = key(byId.get(x));
      if (seen.has(k)) continue;
      seen.add(k);
      incl.set(k, (incl.get(k) ?? 0) + ms / files.length);
    }
  }
}
const show = (m, n) => [...m].sort((a, b) => b[1] - a[1]).slice(0, n).map(([k, v]) => `${v.toFixed(1).padStart(7)}  ${k}`).join('\n');
console.log(`${files.length} profiles\n-- by category (self ms)\n${show(cat, 12)}\n-- self\n${show(self, Number(top))}\n-- inclusive\n${show(incl, Number(top))}`);
