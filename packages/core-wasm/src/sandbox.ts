/**
 * The script sandbox the Rust actions plugin reaches through its live
 * adapter (`crates/plugins/actions/src/harness.rs`, `live_sandbox_factory`):
 * a factory whose promise resolves to `{disposed, boot, run, dispose}`, all
 * functions, so each crosses the wire as a `$fn` the kernel calls back
 * synchronously. Arguments and answers are the canonical JSON the wire
 * already converts, so the sandbox sees the same objects the TypeScript host
 * hands it.
 */
import type { ScriptSandbox, ScriptSandboxFactory } from '@embedpdf/core-js-sandbox';

/** A `ScriptSandbox` with `disposed` as a function, so it crosses as a `$fn`. */
export interface WireSandbox {
  disposed(): boolean;
  boot: ScriptSandbox['boot'];
  run: ScriptSandbox['run'];
  dispose(): void;
}

/** The QuickJS sandbox, imported on first use as the TypeScript default is
 *  (`packages/plugin/actions/src/scripting/environment.ts`). */
const quickJs: ScriptSandboxFactory = () =>
  import('@embedpdf/core-js-sandbox').then(({ createQuickJsSandbox }) => createQuickJsSandbox());

/** `factory`, or the QuickJS sandbox, as the wire's sandbox factory. */
export function createSandboxBridge(
  factory: ScriptSandboxFactory = quickJs,
): () => Promise<WireSandbox> {
  return async () => {
    const sandbox = await factory();
    return {
      disposed: () => sandbox.disposed,
      boot: (sources, input, budget) => sandbox.boot(sources, input, budget),
      run: (source, input, budget) => sandbox.run(source, input, budget),
      dispose: () => sandbox.dispose(),
    };
  };
}
