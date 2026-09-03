import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 5174,
    // packages/web and build/out/web both live outside this app's root, and
    // Vite refuses to serve outside it without this.
    fs: { allow: ['../..'] },
  },
  optimizeDeps: { exclude: ['@epdf-scaffold/web'] },
});
