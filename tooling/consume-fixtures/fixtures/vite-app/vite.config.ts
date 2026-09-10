import { defineConfig } from 'vite';
import { existsSync } from 'node:fs';
import path from 'node:path';

// Mirrors the documented consumer config: the engine worker code-splits
// (wasm target resolution), so the worker bundle must be ES format.
export default defineConfig({
  worker: { format: 'es' },
  build: {
    rollupOptions: {
      input: {
        main: path.resolve('index.html'),
        ...(existsSync('e2e.html') ? { e2e: path.resolve('e2e.html') } : {}),
      },
    },
  },
});
