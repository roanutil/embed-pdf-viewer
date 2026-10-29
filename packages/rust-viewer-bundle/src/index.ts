/**
 * The viewer on the Rust kernel, for apps outside this repo.
 *
 * ```ts
 * import { createViewer } from '@mfb/embedpdf-rust-viewer';
 * const el = await createViewer({ target: '#viewer', src: '/report.pdf' });
 * el.addEventListener('epdf:ready', () => el.viewer!.execute('zoom:in'));
 * ```
 *
 * The snippet door (`packages/viewer/main/src/doors/snippet.ts`) with
 * `@embedpdf/core` and `@embedpdf/plugin-*` redirected to the Rust kernel by
 * this package's Vite config. The kernel's wasm and its hosts are set up once,
 * before the first element mounts, as `examples/rust-kernel-eval` does.
 */
import { kernelCounts } from '@embedpdf/core-wasm';
import { initWasm } from '@embedpdf/core-wasm/browser';
import EmbedPDF, { type InitOptions } from '../../viewer/main/src/doors/snippet';
import { installHosts } from './hosts';

declare const __BUILD_INFO__: {
  commit: string;
  branch: string;
  dirty: boolean;
  builtAt: string;
};

/** The commit this bundle was built from. */
export const buildInfo = __BUILD_INFO__;

let ready: Promise<void> | undefined;

/** Load the Rust kernel's wasm and install its hosts. Idempotent; `createViewer` calls it. */
export function initKernel(): Promise<void> {
  ready ??= initWasm().then(installHosts);
  return ready;
}

/** `EmbedPDF.init` on the Rust kernel: create an <embedpdf-viewer>, configure it, append it to `target`. */
export async function createViewer(options: InitOptions) {
  await initKernel();
  // Page geometry through the Rust engine core linked into the PDFium payload;
  // the engine falls back to TypeScript on a payload built without it.
  return EmbedPDF.init({ ...options, engine: options.engine ?? { rustRouting: { rustReads: true } } });
}

/** Live sizes of the kernel's tables, for chasing leaks from the console. */
export { kernelCounts };
