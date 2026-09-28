// The stub types `../wasm/epdf_web.js` when `wasm/` is not built. The
// reference makes it apply to any program that pulls this file in (the eval
// app's typecheck), not only to this package's own `include`.
/// <reference path="./wasm.d.ts" />
/**
 * The browser build of `WasmKernel` (`wasm/`, from `pnpm build:wasm`). Await
 * `initWasm()` before the first `createKernel`.
 */
import { hostBytes } from './engine-bridge';
import { setWasmKernel, setWasmMemoryBytes } from './kernel';
import init, { WasmKernel, set_host_bytes_source, wasm_memory_bytes } from '../wasm/epdf_web.js';

export async function initWasm(): Promise<void> {
  await init();
  setWasmKernel(WasmKernel);
  setWasmMemoryBytes(wasm_memory_bytes);
  // Once per wasm instance, not once per kernel: every `WasmKernel` this
  // module builds shares this one resolver, over `engine-bridge.ts`'s
  // shared host table, so two live kernels never race to overwrite it.
  set_host_bytes_source(hostBytes);
}
