import init, * as wasm from '../wasm/poc_geometry.js';

export type GeometryWasm = typeof wasm;

let mod: GeometryWasm | null = null;
let pending: Promise<GeometryWasm> | null = null;

/** Host services the Rust side is allowed to reach back into. */
export interface GeometryHost {
  log?: (message: string) => void;
}

/**
 * The tax the boundary charges.
 *
 * The TypeScript original needed no initialization at all: import it and call
 * it. This version cannot, because `WebAssembly.instantiate` is async and
 * synchronous compilation of anything over 4 kB is blocked on the browser main
 * thread. So every consumer of geometry now has an await in front of it, all
 * the way up to the React tree.
 */
export async function initGeometry(
  opts: { wasmUrl?: string | URL; wasmBytes?: BufferSource; host?: GeometryHost } = {},
): Promise<GeometryWasm> {
  if (mod) {
    // Already up, but the caller may be handing us a NEW host. Re-register it.
    wireHost(mod, opts.host);
    return mod;
  }
  if (pending) {
    return pending.then((w) => {
      wireHost(w, opts.host);
      return w;
    });
  }

  const source = opts.wasmBytes ?? opts.wasmUrl;
  // `pending` has to be cleared on rejection. `mod` is only set on success, so
  // a latched rejected promise is permanent: one transient failure (a dev
  // server restarting, a stale `wasmUrl`) and every later caller gets that same
  // rejection back from the `if (pending)` line above, with no way to retry
  // short of a page reload.
  const attempt = (source ? init({ module_or_path: source as never }) : init()).then(() => {
    mod = wasm;
    wireHost(wasm, opts.host);
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

/**
 * Host registration is separate from init, and repeatable.
 *
 * The first version wired the host only on the very first `initGeometry` call
 * and ignored it afterwards. Under React StrictMode that is silently broken:
 * the first effect run registers a callback, the cleanup invalidates that
 * closure, the second run's callback is dropped because init is already done,
 * and the host logger goes dead with no error anywhere. Registration is
 * mutable state, so treat it that way.
 */
function wireHost(w: GeometryWasm, host?: GeometryHost): void {
  const log = host?.log;
  if (!log) return;
  w.setHostLogger((m: unknown) => log(String(m)));
  w.greetHost();
}

export function isReady(): boolean {
  return mod !== null;
}

/**
 * Every exported function calls this. The throw is deliberate: a silent
 * fallback to a JavaScript implementation would hide exactly the divergence
 * this POC is meant to expose.
 */
export function required(): GeometryWasm {
  if (!mod) {
    throw new Error(
      'geometry wasm is not initialized. Call await initGeometry() before using @poc/geometry.',
    );
  }
  return mod;
}
