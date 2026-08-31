import { initCore } from './init';
import type { CoreWasm } from './init';

const WASM_FILE = 'poc_core_bg.wasm';
const PKG_RELATIVE = `packages/core/wasm/${WASM_FILE}`;

/**
 * Node and vitest loader: read the wasm bytes off disk instead of fetching them.
 *
 * One core, two host loaders, and the core knows about neither. That is the
 * same split the real repo already has between `index.browser.ts` and
 * `index.node.ts`, and it is the part of a Rust port that is genuinely easy.
 */
export async function initCoreFromDisk(wasmPath?: string): Promise<CoreWasm> {
  const { readFile } = await import('node:fs/promises');
  return initCore({ wasmBytes: await readFile(wasmPath ?? (await locateWasm())) });
}

/**
 * `import.meta.url` is not reliably a `file:` URL. Under vitest's jsdom
 * environment it comes through as an http URL, and `fileURLToPath` throws
 * "The URL must be of scheme file". So try the module-relative path when it is
 * usable and walk up from the working directory when it is not.
 */
async function locateWasm(): Promise<string> {
  const { access } = await import('node:fs/promises');
  const { dirname, resolve } = await import('node:path');

  const candidates: string[] = [];

  if (import.meta.url.startsWith('file:')) {
    const { fileURLToPath } = await import('node:url');
    candidates.push(resolve(dirname(fileURLToPath(import.meta.url)), '..', 'wasm', WASM_FILE));
  }

  let dir = process.cwd();
  for (let i = 0; i < 6; i++) {
    candidates.push(resolve(dir, PKG_RELATIVE));
    const up = dirname(dir);
    if (up === dir) break;
    dir = up;
  }

  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      // try the next one
    }
  }

  throw new Error(
    `could not locate ${WASM_FILE}. Run \`pnpm --filter @poc/core build\` first. Tried:\n` +
      candidates.map((c) => `  ${c}`).join('\n'),
  );
}
