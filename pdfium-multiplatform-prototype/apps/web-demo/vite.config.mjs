import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 5174,
    strictPort: true,
    // packages/web and build/out/web both live outside this app's root, and
    // Vite refuses to serve outside it without this. Only those two plus
    // this app's own root are actually consumed, so allow exactly those
    // rather than the whole pdfium-multiplatform-prototype/ root.
    fs: { allow: ['.', '../../packages/web', '../../build/out/web'] },
  },
  optimizeDeps: { exclude: ['@epdf-scaffold/web'] },
  build: { target: 'esnext' },
});
