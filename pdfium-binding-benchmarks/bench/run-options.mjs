export function parseRunOptions(args) {
  const options = { native: false, rounds: 7, budgetMs: 500, out: undefined };
  const seen = new Set();
  for (let i = 0; i < args.length; i++) {
    const name = args[i];
    if (!['--native', '--rounds', '--budget', '--out'].includes(name)) {
      throw new Error(`unknown option: ${name}`);
    }
    if (seen.has(name)) throw new Error(`duplicate option: ${name}`);
    seen.add(name);
    if (name === '--native') {
      options.native = true;
      continue;
    }
    const value = args[++i];
    if (value === undefined || !value.trim() || value.startsWith('--')) {
      throw new Error(`${name} requires a value`);
    }
    if (name === '--rounds') {
      options.rounds = Number(value);
      if (!Number.isSafeInteger(options.rounds) || options.rounds <= 0) {
        throw new Error('--rounds must be a positive safe integer');
      }
    } else if (name === '--budget') {
      options.budgetMs = Number(value);
      if (!Number.isFinite(options.budgetMs) || options.budgetMs <= 0) {
        throw new Error('--budget must be a positive finite number');
      }
    } else {
      options.out = value;
    }
  }
  return options;
}
