import init, * as wasm from '../wasm/poc_core.js';

export type CoreWasm = typeof wasm;

let mod: CoreWasm | null = null;
let pending: Promise<CoreWasm> | null = null;

export interface CoreHost {
  log?: (message: string) => void;
}

/**
 * Same async init as poc-2, and the same tax. The difference is where the tax
 * lands: poc-2 put an await in front of a geometry helper that a dozen pure
 * functions called, so the asynchrony spread upward through packages that had
 * no business knowing about it. Here it sits at the one place the application
 * already had a lifecycle, which is the argument for a coarse boundary quite
 * apart from the per-call cost.
 */
export async function initCore(
  opts: { wasmUrl?: string | URL; wasmBytes?: BufferSource } = {},
): Promise<CoreWasm> {
  if (mod) return mod;
  if (pending) return pending;
  const source = opts.wasmBytes ?? opts.wasmUrl;
  pending = (source ? init({ module_or_path: source as never }) : init()).then(() => {
    mod = wasm;
    return wasm;
  });
  return pending;
}

export function isReady(): boolean {
  return mod !== null;
}

export function required(): CoreWasm {
  if (!mod) {
    throw new Error('core wasm is not initialized. Call await initCore() first.');
  }
  return mod;
}
