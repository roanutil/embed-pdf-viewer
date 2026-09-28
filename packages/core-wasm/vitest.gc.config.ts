import { defineConfig } from 'vitest/config';

// `release.test.ts` forces a collection with `gc()`, which Node defines only
// under `--expose-gc`; see `vitest.config.ts` for why this is its own run.
export default defineConfig({
  test: {
    include: ['test/release.test.ts'],
    pool: 'forks',
    poolOptions: { forks: { execArgv: ['--expose-gc'] } },
  },
});
