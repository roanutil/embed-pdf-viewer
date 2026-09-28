/**
 * The browser build of `WasmKernel` (`wasm/`, from `pnpm build:wasm`). Await
 * `initWasm()` before the first `createKernel`.
 */
import init, { WasmKernel } from '../wasm/epdf_web.js';
import { setWasmKernel } from './kernel';

export async function initWasm(): Promise<void> {
  await init();
  setWasmKernel(WasmKernel);
}
