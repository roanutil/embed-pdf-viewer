#!/usr/bin/env node
/**
 * The only file that knows what "the same answer" means.
 *
 * This runs before any timing. A wrong arm can be fast, and nothing about a
 * timing table would reveal it. The previous study on this branch caught real
 * divergence exactly here, which is the reason to keep paying for it.
 */
import { loadArm } from './arm.mjs';
import { openFixture } from './fixture.mjs';
import { makeWorkloads } from './workloads.mjs';

export async function canonicalFor(armName) {
  const arm = await loadArm(armName);
  const fx = openFixture(arm);
  const w = makeWorkloads(arm, fx);
  const out = { glyphCount: fx.glyphCount, pageIndex: fx.pageIndex };
  for (const [id, wl] of Object.entries(w)) {
    if (id === 'dispose' || !wl.arms.includes(armName)) continue;
    out[id] = wl.canonical();
  }
  w.dispose();
  fx.close();
  arm.close();
  return out;
}

export function compare(baselineName, baseline, armName, actual) {
  const failures = [];
  for (const [id, want] of Object.entries(baseline)) {
    if (!(id in actual)) continue;
    const a = JSON.stringify(want);
    const b = JSON.stringify(actual[id]);
    if (a === b) continue;
    failures.push({
      id,
      baseline: a.length > 400 ? `${a.slice(0, 400)}… (${a.length} chars)` : a,
      actual: b.length > 400 ? `${b.slice(0, 400)}… (${b.length} chars)` : b,
    });
  }
  if (failures.length === 0) return true;
  console.error(`\nFAIRNESS GATE FAILED: arm ${armName} disagrees with arm ${baselineName}.`);
  for (const f of failures) {
    console.error(`  ${f.id}\n    ${baselineName}: ${f.baseline}\n    ${armName}: ${f.actual}`);
  }
  return false;
}

if (import.meta.filename === process.argv[1]) {
  const arms = process.argv.slice(2).length ? process.argv.slice(2) : ['a'];
  const results = {};
  for (const a of arms) results[a] = await canonicalFor(a);
  const [first, ...rest] = arms;
  let ok = true;
  for (const a of rest) ok = compare(first, results[first], a, results[a]) && ok;
  if (!ok) process.exit(1);
  console.log(
    `fairness gate: ${arms.join(', ')} agree on ${results[first].glyphCount} glyphs, ` +
      `page ${results[first].pageIndex}`,
  );
}
