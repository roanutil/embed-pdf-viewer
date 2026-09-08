/**
 * Flag parsing for run.mjs, split out so it can be tested without running a
 * benchmark. Validation happens before run.mjs touches bench/results: a
 * forgotten value used to become `Number(undefined)`, which is NaN, and the
 * per-round files were already deleted by the time the round loop ran zero
 * times and printed success.
 */
export function parseRunOptions(args) {
  const options = { rounds: 5, budgetMs: 500, allocRounds: 3 };
  const seen = new Set();
  for (let i = 0; i < args.length; i++) {
    const name = args[i];
    if (!['--rounds', '--budget', '--alloc-rounds'].includes(name)) {
      throw new Error(`unknown option: ${name}`);
    }
    if (seen.has(name)) throw new Error(`duplicate option: ${name}`);
    seen.add(name);
    const value = args[++i];
    if (value === undefined || !value.trim() || value.startsWith('--')) {
      throw new Error(`${name} requires a value`);
    }
    const n = Number(value);
    if (name === '--budget') {
      if (!Number.isFinite(n) || n <= 0) throw new Error('--budget must be a positive finite number');
      options.budgetMs = n;
    } else {
      if (!Number.isSafeInteger(n) || n <= 0) throw new Error(`${name} must be a positive safe integer`);
      if (name === '--rounds') options.rounds = n;
      else options.allocRounds = n;
    }
  }
  return options;
}
