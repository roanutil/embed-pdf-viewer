/**
 * The browser globals the TypeScript plugins read for themselves, handed to
 * the Rust plugins as host inputs (`@embedpdf/core-wasm`'s `setHosts`):
 *
 *   - stage's `Scheduler` (`crates/plugins/stage/src/contract.rs`) over
 *     `requestAnimationFrame`, so motion runs on real frames;
 *   - render's `RenderHost` (`crates/plugins/render/src/host_contract.rs`):
 *     `requestFrame` over `requestAnimationFrame`, `warn` and `debug` over
 *     `console`;
 *   - annotation's `AnnotationHost` (`crates/plugins/annotation/src/contract.rs`):
 *     `randomUuid` over `crypto.randomUUID`, so new annotation ids differ
 *     across sessions, and `consoleError` over `console.error`, with the
 *     canonical `{$error}` the Rust side sends as a live `Error`, as the
 *     kernel reports.
 *
 * Not wired: signing's `WebCrypto` and `PersonalKeyStore`, so signatures run
 * with no signer and a mark is drawn without sealing.
 */
import { setHosts, toError } from '@embedpdf/core-wasm';

export function installHosts(): void {
  setHosts({
    scheduler: {
      raf: (frame) => requestAnimationFrame(frame),
      caf: (handle) => cancelAnimationFrame(handle),
    },
    render: {
      requestFrame: (frame) => void requestAnimationFrame(() => frame()),
      warn: (message) => console.warn(message),
      debug: (message) => console.debug(message),
    },
    annotation: {
      randomUuid: () => crypto.randomUUID(),
      consoleError: (message, error) =>
        error === undefined ? console.error(message) : console.error(message, toError(error)),
    },
  });
}
