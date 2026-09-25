import type { DocumentSecurityProbeInfo } from '@embedpdf/engine-core/runtime';
import { EngineError, EngineErrorCode } from '@embedpdf/engine-core/runtime';
import type { PdfRuntimeModule, PdfRuntimeRustHost } from '@embedpdf/engine-runtime';

/**
 * G3w: a Rust-owned document session, under the real transport, opt-in.
 *
 * Stage A's rule is that TypeScript owns the session and Rust borrows pages
 * from it. This is the one exception, and it is deliberately narrow: only the
 * session-lifecycle kinds route here, and only for documents opened with the
 * rehearsal flag set. No read ever does.
 *
 * # Why a rehearsal exists at all
 *
 * G10 moves session authority to Rust for every document. If that were the
 * first time a Rust-owned session ran on web, every failure of the transport,
 * the framing, the lifetimes and the teardown would surface at once, mixed
 * together, in a change that also moved every read. This separates them: the
 * session moves first, under a flag, carrying no reads, so that what is being
 * tested is the transport and nothing else.
 *
 * # What it is not
 *
 * Not the public routing profile. `rustReads` selects the Stage A geometry
 * reader and leaves the session TypeScript's; this flag is internal and must
 * not change that profile's behaviour. A document opened here cannot be read
 * through the TypeScript services at all, because the handles it would need
 * live on the other side of the boundary; a read that reaches one is a
 * routing bug and is reported as one rather than quietly falling back.
 */
export class RustSessionRehearsal {
  private readonly host: PdfRuntimeRustHost;
  /** Document ids opened on the Rust side, so reads can refuse precisely. */
  private readonly owned = new Set<string>();
  private destroyed = false;

  constructor(runtime: PdfRuntimeModule) {
    const createHost = runtime.rust?.createHost;
    if (!createHost) {
      // Construction is the right place to fail. A caller that got here
      // without checking `supportedBy` has a routing bug, and finding that
      // out at the first open would blame the document.
      throw new EngineError(
        EngineErrorCode.RuntimeUnavailable,
        'this runtime was built without the Rust engine dispatcher',
      );
    }
    this.host = createHost.call(runtime.rust);
  }

  /** Whether a runtime can serve the rehearsal at all. */
  static supportedBy(runtime: PdfRuntimeModule): boolean {
    return runtime.rust?.createHost !== undefined;
  }

  /** Whether this document's session lives on the Rust side. */
  owns(docId: string): boolean {
    return this.owned.has(docId);
  }

  /**
   * Opens a document on the Rust session.
   *
   * The bytes cross as a payload rather than inside the JSON, which is the
   * whole reason the transport has payloads: a PDF base64'd through a string
   * is the same megabytes twice.
   *
   * A password comes back as a probe, not as a throw. The session parks with
   * its key and its bytes on the other side, so the later unlock does not
   * transfer the document again; a thrown error here would make the caller
   * tear down something that is waiting for a prompt.
   */
  openFatMemory(
    docId: string,
    bytes: Uint8Array,
    password: string | undefined,
  ): DocumentSecurityProbeInfo {
    const result = this.call<{ security: DocumentSecurityProbeInfo }>(
      { kind: 'open.fatMem', docId, ...(password === undefined ? {} : { password }) },
      [bytes],
    );
    this.owned.add(docId);
    return result.security;
  }

  /** `document.checkPasswordPermissions` against a Rust-owned session. */
  checkPasswordPermissions(
    docId: string,
    password: string,
    mode: 'any' | 'owner' | undefined,
  ): DocumentSecurityProbeInfo {
    return this.call<DocumentSecurityProbeInfo>({
      kind: 'document.checkPasswordPermissions',
      docId,
      password,
      ...(mode === undefined ? {} : { mode }),
    });
  }

  /** `document.probeSecurityFile`, which needs no session. */
  probeSecurityFile(path: string, password: string | undefined): DocumentSecurityProbeInfo {
    return this.call<DocumentSecurityProbeInfo>({
      kind: 'document.probeSecurityFile',
      path,
      ...(password === undefined ? {} : { password }),
    });
  }

  /** Closes every session for a document. Safe for one this does not own. */
  close(docId: string): void {
    if (!this.owned.has(docId)) return;
    this.call({ kind: 'close', docId });
    this.owned.delete(docId);
  }

  /** Closes one layer view, leaving the base session open. */
  closeLayer(docId: string, layerName: string): void {
    if (!this.owned.has(docId)) return;
    this.call({ kind: 'layer.close', docId, layerName });
  }

  /**
   * Releases the engine and everything it holds.
   *
   * Idempotent, because a worker tearing down after an error will call this
   * alongside its own cleanup and a second destroy on the other side is a
   * crash with no stack.
   */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.owned.clear();
    this.host.destroy();
  }

  /**
   * One request, with the envelope decoded and the error rethrown.
   *
   * The codes are one vocabulary across both engines, so a refusal from Rust
   * is rethrown as itself rather than translated. A response that is neither
   * a result nor an error is a transport fault and says so: blaming the
   * document for it would send somebody looking at the wrong thing.
   */
  private call<T>(request: Record<string, unknown>, payloads: readonly Uint8Array[] = []): T {
    if (this.destroyed) {
      throw new EngineError(
        EngineErrorCode.RuntimeUnavailable,
        'the Rust session rehearsal has been destroyed',
      );
    }
    const response = this.host.call(JSON.stringify(request), payloads);
    const envelope = JSON.parse(response.json) as {
      result?: T;
      error?: { code: EngineErrorCode; message: string; details?: Record<string, unknown> };
    };
    if (envelope.error) {
      throw new EngineError(envelope.error.code, envelope.error.message, envelope.error.details);
    }
    if (envelope.result === undefined) {
      throw new EngineError(
        EngineErrorCode.WireFormat,
        `the Rust core answered '${String(request.kind)}' with neither a result nor an error`,
      );
    }
    return envelope.result;
  }
}
