#!/usr/bin/env node
/**
 * The only file that knows what "the same answer" means.
 *
 * This runs before any timing. A wrong arm can be fast, and nothing about a
 * timing table would reveal it. The previous study on this branch caught real
 * divergence exactly here, which is the reason to keep paying for it.
 *
 * It runs over every fixture in `GATE_FIXTURES`, not just the one timing
 * uses: the timing fixture (report.pdf page 4) is entirely upright, so a
 * gate that only ever saw that page would never compare the rotation /
 * ascent-flip branch or the SPACE flag branch that all four arms implement.
 */
import { loadArm } from './arm.mjs';
import { GATE_FIXTURES, openFixture } from './fixture.mjs';
import { makeWorkloads, WORKLOAD_TABLE } from './workloads.mjs';

/**
 * `{ [fixtureLabel]: { glyphCount, pageIndex, ...canonical answers } }` for
 * every fixture in `GATE_FIXTURES`. Also spreads the first fixture's answers
 * onto the top level, so `canonicalFor(arm).glyphCount` and friends keep
 * meaning "the timing fixture", as they did before this had more than one.
 *
 * Shared by `canonicalFor` and `canonicalForNative` below, which differ only
 * in which loader opens the arm -- collapsed into one helper so a future
 * change to the fixture loop can't land in one and not the other.
 */
async function canonicalUsing(loader, armName) {
  const arm = await loader(armName);
  const fixtures = {};
  for (const fixtureSpec of GATE_FIXTURES) {
    const fx = openFixture(arm, fixtureSpec);
    const w = makeWorkloads(arm, fx);
    const result = { glyphCount: fx.glyphCount, pageIndex: fx.pageIndex };
    for (const [id, wl] of Object.entries(w)) {
      if (id === 'dispose' || !wl.arms.includes(armName) || wl.canonical == null) continue;
      result[id] = wl.canonical();
    }
    w.dispose();
    fx.close();
    fixtures[fixtureSpec.label] = result;
  }
  arm.close();
  return { ...fixtures[GATE_FIXTURES[0].label], fixtures };
}

export async function canonicalFor(armName) {
  return canonicalUsing(loadArm, armName);
}

/**
 * Same shape as `canonicalFor` above, but for a native arm loaded through
 * `loadNativeArm` rather than `loadArm`. `compare()` doesn't know or care
 * which loader produced its two arguments.
 */
export async function canonicalForNative(armName) {
  const { loadNativeArm } = await import('./arm.mjs');
  return canonicalUsing(loadNativeArm, armName);
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

/**
 * `baseline`/`actual` are whole `canonicalFor()` results (each carrying a
 * `.fixtures` field keyed by fixture label). Compares every fixture in
 * `GATE_FIXTURES`, so a divergence that only shows up on one page (the
 * rotation/SPACE branches, say) is caught even when every other fixture
 * agrees.
 */
export function compare(baselineName, baseline, armName, actual) {
  const baselineFixtures = baseline.fixtures;
  const actualFixtures = actual.fixtures;
  const expected = expectedIds(baselineName, armName);

  const missing = [];
  for (const { label } of GATE_FIXTURES) {
    const baseline = baselineFixtures[label] ?? {};
    const actual = actualFixtures[label] ?? {};
    for (const id of expected) {
      if (!(id in baseline)) missing.push(`${label}: ${id} missing from ${baselineName}'s results`);
      if (!(id in actual)) missing.push(`${label}: ${id} missing from ${armName}'s results`);
    }
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
  for (const { label } of GATE_FIXTURES) {
    const baseline = baselineFixtures[label];
    const actual = actualFixtures[label];
    for (const id of expected) {
      const a = JSON.stringify(baseline[id]);
      const b = JSON.stringify(actual[id]);
      if (a === b) continue;
      failures.push({
        fixture: label,
        id,
        baseline: a.length > 400 ? `${a.slice(0, 400)}… (${a.length} chars)` : a,
        actual: b.length > 400 ? `${b.slice(0, 400)}… (${b.length} chars)` : b,
      });
    }
  }
  if (failures.length > 0) {
    console.error(`\nFAIRNESS GATE FAILED: arm ${armName} disagrees with arm ${baselineName}.`);
    for (const f of failures) {
      console.error(
        `  [${f.fixture}] ${f.id}\n    ${baselineName}: ${f.baseline}\n    ${armName}: ${f.actual}`,
      );
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
  const fixtureSummary = GATE_FIXTURES.map(
    ({ label }) => `${label} (${results[first].fixtures[label].glyphCount} glyphs)`,
  ).join(', ');
  console.log(
    `fairness gate: ${arms.join(', ')} agree on ${fixtureSummary}, ` +
      `${compared} workload${compared === 1 ? '' : 's'} compared`,
  );
}
