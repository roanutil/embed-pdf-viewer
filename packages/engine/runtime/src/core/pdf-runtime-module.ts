import type { PdfFunctions } from './pdf-functions.generated';

export type Ptr = bigint & { readonly __brand: 'PdfRuntimePtr' };
export type Callback = bigint & { readonly __brand: 'PdfRuntimeCallback' };

/** The null pointer; useful when the underlying API accepts a NULL value. */
export const NULL_PTR = 0n as Ptr;

export type MemoryValueKind = 'i8' | 'i16' | 'i32' | 'i64' | 'f32' | 'f64' | 'ptr';

export type CallbackKind = string;
export type CallbackFn<K extends CallbackKind = CallbackKind> = (...args: unknown[]) => unknown;

export interface PdfRuntimeMemory {
  alloc(bytes: number): Ptr;
  free(ptr: Ptr): void;
  readBytes(ptr: Ptr, len: number): Uint8Array;
  writeBytes(ptr: Ptr, data: Uint8Array): void;
  readU8String(ptr: Ptr): string;
  writeU8String(str: string): Ptr;
  readU16String(ptr: Ptr): string;
  writeU16String(str: string): Ptr;
  peek(ptr: Ptr, kind: MemoryValueKind, byteOffset?: number): number | bigint;
  poke(ptr: Ptr, kind: MemoryValueKind, value: number | bigint, byteOffset?: number): void;
}

export interface PdfRuntimeCallbacks {
  register<K extends CallbackKind>(kind: K, fn: CallbackFn<K>): Callback;
  dispose(callback: Callback): void;
}

export interface PdfFileAccessHandle {
  /** Pointer to the owned FPDF_FILEACCESS struct. */
  readonly ptr: Ptr;
  /** Releases callbacks, native file handles, and retained byte views. */
  close(): void;
}

export interface PdfRuntimeFileAccess {
  /**
   * Small in-memory file access, intended for layer deltas/artifacts.
   * Browser base documents should prefer EPDF_LoadMemBaseDocument64().
   */
  fromMemory(bytes: Uint8Array | ArrayBuffer): PdfFileAccessHandle;
  /** Native/node only: range-reads a local file without loading it into JS. */
  fromNodeFile(path: string): PdfFileAccessHandle;
}

export interface PdfFileWriteHandle {
  /** Pointer to the owned FPDF_FILEWRITE struct. */
  readonly ptr: Ptr;
  /** Flushes/closes the underlying writer and releases native resources. */
  close(): void;
}

export interface PdfRuntimeFileWrite {
  /** Native/node only: writes PDFium output directly to a local file. */
  toNodeFile(path: string): PdfFileWriteHandle;
}

/**
 * The Rust engine core's exports, present only when the module was linked with
 * `libepdf_wasm.a` (`EMBEDPDF_RUST_STATICLIB` in `build/compile.esm.sh`).
 *
 * Absent means the payload is PDFium alone, which is what ships today. Callers
 * branch on presence rather than on a build flag, so a runtime that lacks the
 * core degrades to the TypeScript reader instead of throwing.
 *
 * Stage A: every entry point here borrows a pointer the session owner acquired
 * and holds for the duration of the call. Nothing is retained, nothing is
 * closed. See `crates/epdf-wasm/src/lib.rs`.
 */
export interface PdfRuntimeRustCore {
  /**
   * Reads one page's geometry through the Rust reader.
   *
   * `pagePtr` must come from `PagePtrPool.acquire` and stay open across the
   * call. Returns the JSON envelope verbatim: `{ result }` or `{ error }`,
   * with `error` carrying an `EngineErrorCode`. Decoding is the caller's,
   * because the shape belongs to `engine-core` and this package does not
   * depend on it.
   */
  pageGeometry(pagePtr: Ptr): string;
  /**
   * Creates an engine that owns its own sessions — Stage B.
   *
   * Absent when the linked core predates the dispatcher, which is why it is
   * optional rather than assumed: a runtime payload and the package that
   * loads it ship separately, and a caller that reached for a missing export
   * would fault rather than fall back.
   */
  createHost?(): PdfRuntimeRustHost;
}

/**
 * One Rust engine: every wire operation, through one call.
 *
 * Not a session and not a document. A host holds as many of both as the
 * requests it is given open, and `destroy` releases all of them.
 *
 * Thread-confined to the worker that created it, like every other PDFium
 * handle: the pointer beneath this is an offset into one linear memory and
 * nothing here makes it safe to pass to another thread.
 */
export interface PdfRuntimeRustHost {
  /**
   * Runs one request.
   *
   * `request` is the wire envelope as JSON. `payloads` are its binary
   * arguments, positional: the operation names the first and the second,
   * because the transfer list this side already uses is an array.
   *
   * Returns the response envelope verbatim — `{ result }` or `{ error }` —
   * beside its own binary results. Decoding the JSON is the caller's,
   * because the shape belongs to `engine-core` and this package does not
   * depend on it.
   */
  call(request: string, payloads: readonly Uint8Array[]): PdfRuntimeRustResponse;
  /** Releases the engine and every document it still holds open. */
  destroy(): void;
}

export interface PdfRuntimeRustResponse {
  /** `{"result": ...}` or `{"error": ...}`, undecoded. */
  readonly json: string;
  /** Binary results, positional and copied out of the runtime's memory. */
  readonly payloads: Uint8Array[];
}

export interface PdfRuntimeModule {
  readonly kind: 'wasm' | 'native';
  readonly platform: string;
  readonly mem: PdfRuntimeMemory;
  readonly cb: PdfRuntimeCallbacks;
  readonly fileAccess: PdfRuntimeFileAccess;
  readonly fileWrite: PdfRuntimeFileWrite;
  readonly fn: PdfFunctions;
  /** Present only when the Rust core was linked into this module. */
  readonly rust?: PdfRuntimeRustCore;
  destroy(): Promise<void>;
}

export interface CreatePdfRuntimeOptions {
  /**
   * Runtime preference. Native is attempted first in Node by default, with
   * WebAssembly as the fallback.
   */
  prefer?: 'auto' | 'native' | 'wasm';
  /**
   * Exact URL of `embedpdf.wasm`. When set, the wasm runtime fetches the binary
   * from here instead of resolving it relative to the Emscripten glue
   * (`import.meta.url`). Must be an absolute URL when the runtime boots inside
   * a `blob:` worker, where relative resolution is meaningless.
   */
  wasmUrl?: string;
  /**
   * The `embedpdf.wasm` bytes, pre-fetched by the caller. Takes precedence over
   * `wasmUrl`; no network request is made (the air-gapped path).
   */
  wasmBinary?: ArrayBuffer | Uint8Array;
  /**
   * Emscripten module overrides for the wasm runtime (escape hatch). An
   * explicit `locateFile` / `wasmBinary` here wins over the first-class
   * options above.
   */
  wasm?: Record<string, unknown>;
}
