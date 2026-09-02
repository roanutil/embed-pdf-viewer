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
import { makeWorkloads, WORKLOAD_TABLE } from './workloads.mjs';

export async function canonicalFor(armName) {
  const arm = await loadArm(armName);
  const fx = openFixture(arm);
  const w = makeWorkloads(arm, fx);
  const out = { glyphCount: fx.glyphCount, pageIndex: fx.pageIndex };
  for (const [id, wl] of Object.entries(w)) {
    if (id === 'dispose' || !wl.arms.includes(armName) || wl.canonical == null) continue;
    out[id] = wl.canonical();
  }
  w.dispose();
  fx.close();
  arm.close();
  return out;
}

/** Ids the workload table declares BOTH arms are supposed to run and verify. */
function expectedIds(armA, armB) {
  const runs = (name) =>
    new Set(
      WORKLOAD_TABLE.filter((w) => w.hasCanonical && w.arms.includes(name)).map((w) => w.id),
    );
  const a = runs(armA);
  const b = runs(armB);
  return new Set([...a].filter((id) => b.has(id)));
}

export function compare(baselineName, baseline, armName, actual) {
  const expected = expectedIds(baselineName, armName);

  const missing = [];
  for (const id of expected) {
    if (!(id in baseline)) missing.push(`${id} missing from ${baselineName}'s results`);
    if (!(id in actual)) missing.push(`${id} missing from ${armName}'s results`);
  }
  if (missing.length > 0) {
    console.error(
      `\nFAIRNESS GATE FAILED: arm ${armName} vs arm ${baselineName} is missing a result both ` +
        'arms are declared to produce (a workload vanished, or its `arms` list is wrong).',
    );
    for (const m of missing) console.error(`  ${m}`);
    return { ok: false, compared: 0 };
  }

  const failures = [];
  for (const id of expected) {
    const a = JSON.stringify(baseline[id]);
    const b = JSON.stringify(actual[id]);
    if (a === b) continue;
    failures.push({
      id,
      baseline: a.length > 400 ? `${a.slice(0, 400)}… (${a.length} chars)` : a,
      actual: b.length > 400 ? `${b.slice(0, 400)}… (${b.length} chars)` : b,
    });
  }
  if (failures.length > 0) {
    console.error(`\nFAIRNESS GATE FAILED: arm ${armName} disagrees with arm ${baselineName}.`);
    for (const f of failures) {
      console.error(`  ${f.id}\n    ${baselineName}: ${f.baseline}\n    ${armName}: ${f.actual}`);
    }
    return { ok: false, compared: expected.size };
  }

  return { ok: true, compared: expected.size };
}

if (import.meta.filename === process.argv[1]) {
  const arms = process.argv.slice(2).length ? process.argv.slice(2) : ['a'];
  const results = {};
  for (const a of arms) results[a] = await canonicalFor(a);
  const [first, ...rest] = arms;

  let ok = true;
  let compared;
  if (rest.length === 0) {
    // Nothing to compare against with a single arm; report what it would
    // have offered to a comparison, so the count still means something.
    compared = WORKLOAD_TABLE.filter((w) => w.hasCanonical && w.arms.includes(first)).length;
  } else {
    compared = Infinity;
    for (const a of rest) {
      const result = compare(first, results[first], a, results[a]);
      ok = result.ok && ok;
      compared = Math.min(compared, result.compared);
    }
  }
  if (!ok) process.exit(1);
  console.log(
    `fairness gate: ${arms.join(', ')} agree on ${results[first].glyphCount} glyphs, ` +
      `page ${results[first].pageIndex} (${compared} workload${compared === 1 ? '' : 's'} compared)`,
  );
}
