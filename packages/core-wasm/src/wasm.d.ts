/**
 * The browser wasm-bindgen build (`wasm/`, from `pnpm build:wasm`): it is
 * gitignored build output, so `browser.ts`'s `import init, { WasmKernel }
 * from '../wasm/epdf_web.js'` has nothing to resolve on a clean checkout,
 * and `typecheck` would need the build first. This stub gives that import
 * a shape instead, so `typecheck` runs without it.
 *
 * The shape mirrors `wasm-node/epdf_web.d.ts` (the Node target this
 * package's tests already build against): both targets export the same
 * `WasmKernel` class, and only the browser target adds `init`.
 */
declare module '*/wasm/epdf_web.js' {
  import type { RustKernelClass } from './proxy';

  export const WasmKernel: RustKernelClass;
  export function wasm_memory_bytes(): number;
  export function set_host_bytes_source(f: (n: number) => Uint8Array): void;
  export function set_host_release(f: (n: number) => void): void;
  export default function init(module_or_path?: unknown): Promise<unknown>;
}
