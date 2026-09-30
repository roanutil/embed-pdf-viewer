import { kernelCounts as counts } from '@embedpdf/core-wasm';
import { initWasm } from '@embedpdf/core-wasm/browser';
import { installHosts } from './src/hosts';

export async function setupKernel(): Promise<void> {
  await initWasm();
  installHosts();
}
export const kernelCounts = (): Record<string, number> | null => counts();
