import { defineConfig } from 'vitest/config';

// `release.test.ts` needs `--expose-gc`, which only `vitest.gc.config.ts`'s
// pool passes (`test:gc`): vitest 2 reads `poolOptions` from the root config
// alone, so the flag cannot be scoped to one file within one run.
export default defineConfig({
  test: { include: ['test/**/*.test.ts'], exclude: ['test/release.test.ts'] },
});
