import type { PageGeometrySnapshot, PageObjectNumber } from '@embedpdf/engine-core/runtime';
import { EngineError, EngineErrorCode } from '@embedpdf/engine-core/runtime';
import type { PdfRuntimeModule, PdfRuntimeRustCore } from '@embedpdf/engine-runtime';

import type { DocumentSession } from '../../document-session/DocumentSession';
import { throwIfAborted } from '../../shared/abort';

/**
 * `pages.geometry` through the Rust engine core. Stage A.
 *
 * Interchangeable with {@link PageGeometryReader}: same constructor, same
 * `read`, same `PageGeometrySnapshot`. The difference is where the glyph loop
 * runs. The TypeScript reader calls `EPDFText_GetCharGeometry` once per glyph
 * across the WASM boundary; this one crosses once for the whole page and Rust
 * does the loop on the other side.
 *
 * The session stays TypeScript's. This borrows a page pointer from
 * `PagePtrPool.acquire` and releases it in a `finally`, exactly as the
 * TypeScript reader does, and Rust retains nothing across the call. That is
 * the whole Stage A contract, and it is why both readers can run against the
 * same live session without either one owning it.
 *
 * Not reachable from consumer packages: `WorkerHost` chooses, and only when
 * the runtime was linked with the core. See `crates/epdf-wasm/src/lib.rs` for
 * the other side.
 */
export class RustPageGeometryReader {
  private readonly core: PdfRuntimeRustCore;

  constructor(
    runtime: PdfRuntimeModule,
    private readonly session: DocumentSession,
  ) {
    if (!runtime.rust) {
      // Construction is the right place to fail: a caller that got here
      // without checking `supportedBy` has a routing bug, and finding that out
      // at the first page read would blame the document.
      throw new EngineError(
        EngineErrorCode.RuntimeUnavailable,
        'this runtime was built without the Rust engine core',
      );
    }
    this.core = runtime.rust;
  }

  /** Whether a runtime can serve this reader at all. */
  static supportedBy(runtime: PdfRuntimeModule): boolean {
    return runtime.rust !== undefined;
  }

  read(pageObjectNumber: PageObjectNumber, signal: AbortSignal): PageGeometrySnapshot {
    throwIfAborted(signal);
    const pool = this.session.pagePool();
    const pagePtr = pool.acquire(pageObjectNumber);

    try {
      throwIfAborted(signal);
      // One crossing for the page. There is no second abort check inside:
      // the Rust side is a single synchronous call, so it is the
      // non-preemptible case from the cancellation contract, same as a render.
      const envelope = JSON.parse(this.core.pageGeometry(pagePtr)) as {
        result?: PageGeometrySnapshot;
        error?: { code: EngineErrorCode; message: string; details?: Record<string, unknown> };
      };

      if (envelope.error) {
        // The codes are one vocabulary across both engines, so this rethrows
        // rather than translates.
        throw new EngineError(envelope.error.code, envelope.error.message, envelope.error.details);
      }
      if (!envelope.result) {
        throw new EngineError(
          EngineErrorCode.WireFormat,
          `the Rust core returned neither a result nor an error for page object ${pageObjectNumber}`,
        );
      }
      return envelope.result;
    } finally {
      pool.release(pageObjectNumber);
    }
  }
}
