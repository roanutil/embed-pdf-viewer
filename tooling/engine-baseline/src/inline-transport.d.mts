/**
 * Types for `@embedpdf/engine/workers/inline-transport.node.js`, which the
 * engine's worker build emits without declarations. Only what `run.mts` uses.
 */
declare module '@embedpdf/engine/workers/inline-transport.node.js' {
  import type { Transport } from '@embedpdf/engine';
  import type { PdfRuntimeModule } from '@embedpdf/engine-runtime';

  export class InlineTransport implements Transport {
    constructor(runtime: PdfRuntimeModule, options?: { rustReads?: boolean });
    send: Transport['send'];
    onMessage: Transport['onMessage'];
    terminate: Transport['terminate'];
  }
}
