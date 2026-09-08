import { initGeometry } from './init';
import type { GeometryHost, GeometryWasm } from './init';

const WASM_FILE = 'poc_geometry_bg.wasm';
const PKG_RELATIVE = `packages/geometry/wasm/${WASM_FILE}`;

/**
 * The Node and vitest loader: read the wasm bytes off disk instead of fetching
 * them.
 *
 * It lives in its own entry point (`@poc/geometry/node`) so browser consumers
 * never pull `node:fs` into their type graph, which is the same reason the real
 * repo splits `index.browser.ts` from `index.node.ts`. Note what this proves:
 * one Rust core, two host loaders, and the core knows about neither.
 */
export async function initGeometryFromDisk(
  host?: GeometryHost,
  wasmPath?: string,
): Promise<GeometryWasm> {
  const { readFile } = await import('node:fs/promises');
  const bytes = await readFile(wasmPath ?? (await locateWasm()));
  return initGeometry({ wasmBytes: bytes, ...(host ? { host } : {}) });
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
    `could not locate ${WASM_FILE}. Run \`pnpm --filter @poc/geometry build\` first. Tried:\n` +
      candidates.map((c) => `  ${c}`).join('\n'),
  );
}
