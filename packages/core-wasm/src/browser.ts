// The stub types `../wasm/epdf_web.js` when `wasm/` is not built. The
// reference makes it apply to any program that pulls this file in (the eval
// app's typecheck), not only to this package's own `include`.
/// <reference path="./wasm.d.ts" />
/**
 * The browser build of `WasmKernel` (`wasm/`, from `pnpm build:wasm`). Await
 * `initWasm()` before the first `createKernel`.
 */
import init, { WasmKernel, wasm_memory_bytes } from '../wasm/epdf_web.js';
import { setWasmKernel, setWasmMemoryBytes } from './kernel';

export async function initWasm(): Promise<void> {
  await init();
  setWasmKernel(WasmKernel);
  setWasmMemoryBytes(wasm_memory_bytes);
}
