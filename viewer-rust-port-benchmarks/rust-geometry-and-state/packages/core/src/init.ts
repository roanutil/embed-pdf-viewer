import init, * as wasm from '../wasm/poc_core.js';

export type CoreWasm = typeof wasm;

let mod: CoreWasm | null = null;
let pending: Promise<CoreWasm> | null = null;

export interface CoreHost {
  log?: (message: string) => void;
}

/**
 * Same async init as rust-geometry-only, and the same tax. The difference is where the tax
 * lands: rust-geometry-only put an await in front of a geometry helper that a dozen pure
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
  // `pending` has to be cleared on rejection. `mod` is only set on success, so
  // a latched rejected promise is permanent: one transient failure (a dev
  // server restarting, a stale `wasmUrl`) and every later caller gets that same
  // rejection back from the `if (pending)` line above, with no way to retry
  // short of a page reload.
  const attempt = (source ? init({ module_or_path: source as never }) : init()).then(() => {
    mod = wasm;
    return wasm;
  });
  pending = attempt;
  // Rejection handled here so the identity check can clear the latch; the
  // caller gets `attempt` itself, so the error still reaches them exactly once.
  attempt.catch(() => {
    if (pending === attempt) pending = null;
  });
  return attempt;
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
