import { pdfFunctionSignatures } from '../core/pdf-functions.generated';
import type {
  PdfFunctionAbiSlot,
  PdfFunctionSignature,
  PdfFunctions,
} from '../core/pdf-functions.generated';
import type {
  Callback,
  CallbackFn,
  CallbackKind,
  CreatePdfRuntimeOptions,
  MemoryValueKind,
  PdfRuntimeCallbacks,
  PdfRuntimeFileAccess,
  PdfRuntimeFileWrite,
  PdfRuntimeMemory,
  PdfRuntimeModule,
  PdfRuntimeRustCore,
  PdfRuntimeRustHost,
  PdfRuntimeRustResponse,
  Ptr,
} from '../core/pdf-runtime-module';

type EmscriptenModule = Record<string, any> & {
  HEAPU8: Uint8Array;
  _malloc?: (bytes: number) => number;
  _free?: (ptr: number) => void;
  cwrap?: (
    name: string,
    result: string | null,
    params: readonly string[],
  ) => (...args: unknown[]) => unknown;
  addFunction?: (fn: (...args: unknown[]) => unknown, signature?: string) => number;
  removeFunction?: (ptr: number) => void;
  getValue?: (ptr: number, type: string) => number;
  setValue?: (ptr: number, value: number | bigint, type: string) => void;
  UTF8ToString?: (ptr: number) => string;
  UTF16ToString?: (ptr: number) => string;
  stringToUTF8?: (str: string, ptr: number, maxBytes: number) => void;
  stringToUTF16?: (str: string, ptr: number, maxBytes: number) => void;
};

function toPtr(value: number | bigint): Ptr {
  return BigInt(value) as Ptr;
}

function toNumber(ptr: Ptr | Callback): number {
  return Number(ptr);
}

function wasmValueKind(kind: MemoryValueKind): string {
  switch (kind) {
    case 'i8':
      return 'i8';
    case 'i16':
      return 'i16';
    case 'i32':
      return 'i32';
    case 'i64':
      return 'i64';
    case 'f32':
      return 'float';
    case 'f64':
      return 'double';
    case 'ptr':
      return '*';
  }
}

/**
 * Binds the Rust core's exports, or returns undefined when the module was
 * linked without them.
 *
 * Presence is probed rather than configured: a payload built without
 * `EMBEDPDF_RUST_STATICLIB` simply has no `_epdf_page_geometry`, and callers
 * fall back to the TypeScript reader. That keeps the two builds one code path
 * instead of two.
 */
function createWasmRustCore(
  module: EmscriptenModule,
  mem: PdfRuntimeMemory,
): PdfRuntimeRustCore | undefined {
  const pageGeometry = module._epdf_page_geometry as
    | ((page: number, outLen: number) => number)
    | undefined;
  const bufferFree = module._epdf_buffer_free as
    | ((ptr: number, len: number) => void)
    | undefined;
  if (!pageGeometry || !bufferFree) return undefined;

  const hostCreate = module._epdf_host_create as (() => number) | undefined;
  const hostCall = module._epdf_host_call as
    | ((
        host: number,
        request: number,
        requestLen: number,
        payloads: number,
        payloadsLen: number,
        outLen: number,
      ) => number)
    | undefined;
  const hostDestroy = module._epdf_host_destroy as ((host: number) => void) | undefined;

  return {
    pageGeometry(pagePtr) {
      // The length comes back through an out-parameter because the payload is
      // UTF-8 JSON, which may contain any byte a string can, so a NUL
      // terminator would truncate it.
      const lenPtr = mem.alloc(4);
      try {
        const out = pageGeometry(toNumber(pagePtr), toNumber(lenPtr));
        if (out === 0) {
          throw new Error('the Rust core refused a page read without reporting why');
        }
        const len = Number(mem.peek(lenPtr, 'i32'));
        try {
          // A copy, deliberately: the buffer goes back to Rust on the next
          // line, and a view into freed linear memory reads as whatever lands
          // there next.
          return new TextDecoder().decode(module.HEAPU8.slice(out, out + len));
        } finally {
          bufferFree(out, len);
        }
      } finally {
        mem.free(lenPtr);
      }
    },

    // Probed separately from `pageGeometry`: a payload built before the
    // dispatcher landed has the geometry export and not these three, and a
    // caller that assumed both would fault rather than fall back.
    ...(hostCreate && hostCall && hostDestroy
      ? { createHost: () => createWasmRustHost(module, mem, hostCreate, hostCall, hostDestroy) }
      : {}),
  };
}

/**
 * The little-endian `u32` framing `epdf_engine::host::frame` documents.
 *
 * Lengths rather than offsets throughout, so a reader walks forward once and
 * never computes a position that could land outside the buffer:
 *
 *     [u32 jsonLen][json]([u32 count]([u32 len][bytes])*count)
 *
 * A request carries only the second half, because the JSON goes across as
 * its own pointer and length.
 */
function packPayloads(payloads: readonly Uint8Array[]): Uint8Array {
  let size = 4;
  for (const payload of payloads) size += 4 + payload.length;
  const out = new Uint8Array(size);
  const view = new DataView(out.buffer);
  view.setUint32(0, payloads.length, true);
  let at = 4;
  for (const payload of payloads) {
    view.setUint32(at, payload.length, true);
    at += 4;
    out.set(payload, at);
    at += payload.length;
  }
  return out;
}

/** The response half of the same framing. */
function unpackResponse(bytes: Uint8Array): PdfRuntimeRustResponse {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const need = (at: number, length: number) => {
    if (at + length > bytes.length) {
      throw new Error('the Rust core returned a response frame that ends early');
    }
  };

  need(0, 4);
  const jsonLen = view.getUint32(0, true);
  need(4, jsonLen);
  const json = new TextDecoder().decode(bytes.subarray(4, 4 + jsonLen));

  let at = 4 + jsonLen;
  need(at, 4);
  const count = view.getUint32(at, true);
  at += 4;

  const payloads: Uint8Array[] = [];
  for (let index = 0; index < count; index++) {
    need(at, 4);
    const length = view.getUint32(at, true);
    at += 4;
    need(at, length);
    // `slice` rather than `subarray`: the caller keeps these and the buffer
    // they point into is about to go back to Rust.
    payloads.push(bytes.slice(at, at + length));
    at += length;
  }
  if (at !== bytes.length) {
    throw new Error('the Rust core returned a response frame with unclaimed bytes');
  }
  return { json, payloads };
}

function createWasmRustHost(
  module: EmscriptenModule,
  mem: PdfRuntimeMemory,
  create: () => number,
  call: (
    host: number,
    request: number,
    requestLen: number,
    payloads: number,
    payloadsLen: number,
    outLen: number,
  ) => number,
  destroyHost: (host: number) => void,
): PdfRuntimeRustHost {
  const host = create();
  if (host === 0) {
    throw new Error('the Rust core could not create an engine');
  }
  let live = true;

  return {
    call(request, payloads) {
      if (!live) {
        throw new Error('this Rust engine has been destroyed');
      }
      const encoded = new TextEncoder().encode(request);
      const framed = packPayloads(payloads);
      // Three allocations, freed in the reverse order in one `finally` each,
      // because an exception between them would otherwise leak linear memory
      // that nothing in this module tracks.
      const requestPtr = mem.alloc(encoded.length || 1);
      try {
        module.HEAPU8.set(encoded, toNumber(requestPtr));
        const payloadsPtr = mem.alloc(framed.length);
        try {
          module.HEAPU8.set(framed, toNumber(payloadsPtr));
          const lenPtr = mem.alloc(4);
          try {
            const out = call(
              host,
              toNumber(requestPtr),
              encoded.length,
              toNumber(payloadsPtr),
              framed.length,
              toNumber(lenPtr),
            );
            if (out === 0) {
              throw new Error('the Rust core refused a request without reporting why');
            }
            const len = Number(mem.peek(lenPtr, 'i32'));
            try {
              // Copied out before the buffer goes back: `unpackResponse`
              // reads it, and a view into freed linear memory reads as
              // whatever lands there next.
              return unpackResponse(module.HEAPU8.slice(out, out + len));
            } finally {
              module._epdf_buffer_free?.(out, len);
            }
          } finally {
            mem.free(lenPtr);
          }
        } finally {
          mem.free(payloadsPtr);
        }
      } finally {
        mem.free(requestPtr);
      }
    },

    destroy() {
      // Idempotent, because a host unwinding from an error may well call
      // this twice and a double free here is a crash with no stack.
      if (!live) return;
      live = false;
      destroyHost(host);
    },
  };
}

function createWasmMemory(module: EmscriptenModule): PdfRuntimeMemory {
  const malloc = module._malloc ?? module.wasmExports?.malloc;
  const free = module._free ?? module.wasmExports?.free;

  if (!malloc || !free) {
    throw new Error('WASM runtime does not expose malloc/free');
  }

  return {
    alloc(bytes) {
      return toPtr(malloc(bytes));
    },
    free(ptr) {
      free(toNumber(ptr));
    },
    readBytes(ptr, len) {
      return module.HEAPU8.slice(toNumber(ptr), toNumber(ptr) + len);
    },
    writeBytes(ptr, data) {
      module.HEAPU8.set(data, toNumber(ptr));
    },
    readU8String(ptr) {
      return module.UTF8ToString?.(toNumber(ptr)) ?? '';
    },
    writeU8String(str) {
      const bytes = new TextEncoder().encode(`${str}\0`);
      const ptr = toPtr(malloc(bytes.length));
      module.HEAPU8.set(bytes, toNumber(ptr));
      return ptr;
    },
    readU16String(ptr) {
      return module.UTF16ToString?.(toNumber(ptr)) ?? '';
    },
    writeU16String(str) {
      const bytes = (str.length + 1) * 2;
      const ptr = toPtr(malloc(bytes));
      module.stringToUTF16?.(str, toNumber(ptr), bytes);
      return ptr;
    },
    peek(ptr, kind, byteOffset = 0) {
      return module.getValue?.(toNumber(ptr) + byteOffset, wasmValueKind(kind)) ?? 0;
    },
    peekWords(ptr, bytes) {
      const { buffer } = module.HEAPU8;
      const at = toNumber(ptr);
      return {
        f32: new Float32Array(buffer, at, bytes >> 2),
        i32: new Int32Array(buffer, at, bytes >> 2),
      };
    },
    poke(ptr, kind, value, byteOffset = 0) {
      module.setValue?.(toNumber(ptr) + byteOffset, value, wasmValueKind(kind));
    },
  };
}

function createWasmCallbacks(module: EmscriptenModule): PdfRuntimeCallbacks {
  return {
    register(kind: CallbackKind, fn: CallbackFn): Callback {
      if (!module.addFunction) throw new Error('WASM runtime does not expose addFunction');
      return BigInt(module.addFunction(fn, kind)) as Callback;
    },
    dispose(callback) {
      module.removeFunction?.(toNumber(callback));
    },
  };
}

function createWasmFileAccess(
  mem: PdfRuntimeMemory,
  callbacks: PdfRuntimeCallbacks,
): PdfRuntimeFileAccess {
  return {
    fromMemory(input) {
      const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
      if (bytes.byteLength > 0xffffffff) {
        throw new RangeError('FPDF_FILEACCESS on wasm supports files up to 4 GiB');
      }

      const structPtr = mem.alloc(12);
      let callback: Callback | null = null;
      let closed = false;

      try {
        callback = callbacks.register('iiiii', (_param, position, outPtr, size) => {
          const begin = Number(position);
          const length = Number(size);
          if (
            !Number.isSafeInteger(begin) ||
            !Number.isSafeInteger(length) ||
            begin < 0 ||
            length < 0 ||
            begin + length > bytes.byteLength
          ) {
            return 0;
          }
          mem.writeBytes(toPtr(outPtr as number), bytes.subarray(begin, begin + length));
          return 1;
        });

        mem.poke(structPtr, 'i32', bytes.byteLength, 0);
        mem.poke(structPtr, 'i32', toNumber(callback), 4);
        mem.poke(structPtr, 'i32', 0, 8);

        return {
          ptr: structPtr,
          close() {
            if (closed) return;
            closed = true;
            callbacks.dispose(callback as Callback);
            mem.free(structPtr);
          },
        };
      } catch (error) {
        if (callback) callbacks.dispose(callback);
        mem.free(structPtr);
        throw error;
      }
    },
    fromNodeFile() {
      throw new Error('fromNodeFile() is only available on the native Node runtime');
    },
    readRange() {
      throw new Error('readRange() is only available on the native Node runtime');
    },
    sizeOf() {
      throw new Error('sizeOf() is only available on the native Node runtime');
    },
  };
}

function createWasmFileWrite(): PdfRuntimeFileWrite {
  return {
    toNodeFile() {
      throw new Error('toNodeFile() is only available on the native Node runtime');
    },
    writeRange() {
      throw new Error('writeRange() is only available on the native Node runtime');
    },
    appendBytes() {
      throw new Error('appendBytes() is only available on the native Node runtime');
    },
    removeFile() {
      throw new Error('removeFile() is only available on the native Node runtime');
    },
  };
}

/**
 * Translate a JS argument to whatever the wasm32 ABI expects for this slot.
 *
 * The codegen tells the truth on each target now (size_t = i32 on wasm), so
 * the only translation left is `Ptr` (TS bigint) → numeric address for cwrap.
 */
function fromJsArg(meta: PdfFunctionAbiSlot | undefined, value: unknown): unknown {
  if (!meta) return value;
  if (meta.ts === 'Ptr' && typeof value === 'bigint') return Number(value);
  return value;
}

function toJsResult(meta: PdfFunctionAbiSlot | null, value: unknown): unknown {
  if (!meta) return undefined;
  if (meta.ts === 'Ptr' && typeof value === 'number') return toPtr(value);
  if (meta.ts === 'bigint' && typeof value === 'number') return BigInt(value);
  // A C bool arrives as 0 or 1 whenever cwrap hands back the raw export:
  // without ASSERTIONS it does for every all-numeric signature, and it
  // counts 'boolean' as numeric (emscripten's library_ccall.js).
  if (meta.ts === 'boolean' && typeof value === 'number') return value !== 0;
  return value;
}

/**
 * The wasm artifact is built WITHOUT `WASM_BIGINT`, so Emscripten
 * legalizes every `i64` parameter at the JS boundary into an (i32 low,
 * i32 high) pair — the export wrapper takes one extra argument per i64
 * and rejects BigInt values outright. Expand each declared i64 argument
 * into its legalized pair before calling.
 */
function expandI64Args(signature: PdfFunctionSignature, args: unknown[]): unknown[] {
  const expanded: unknown[] = [];
  signature.params.forEach((p, i) => {
    if (p.wasm.kind === 'i64') {
      const v = BigInt((args[i] ?? 0) as number | bigint);
      expanded.push(Number(v & 0xffffffffn), Number((v >> 32n) & 0xffffffffn));
    } else {
      expanded.push(args[i]);
    }
  });
  return expanded;
}

function createWasmFunctions(module: EmscriptenModule): PdfFunctions {
  const out: Record<string, (...args: unknown[]) => unknown> = {};
  for (const [name, signature] of Object.entries(pdfFunctionSignatures)) {
    const hasI64Params = signature.params.some((p) => p.wasm.kind === 'i64');
    const bridge =
      (call: (...args: unknown[]) => unknown) =>
      (...args: unknown[]) => {
        const mapped = args.map((arg, i) => fromJsArg(signature.params[i], arg));
        return toJsResult(
          signature.result,
          call(...(hasI64Params ? expandI64Args(signature, mapped) : mapped)),
        );
      };

    if (module.cwrap) {
      // `null` only appears for void return; params never carry it.
      // i64 params are legalized (see expandI64Args): two i32 slots each.
      const paramTypes = signature.params.flatMap((p) =>
        p.wasm.kind === 'i64' ? ['number', 'number'] : [p.wasm.cwrap as string],
      );
      out[name] = bridge(
        module.cwrap(name, signature.result ? signature.result.wasm.cwrap : null, paramTypes),
      );
    } else if (typeof module[`_${name}`] === 'function') {
      out[name] = bridge(module[`_${name}`].bind(module));
    }
  }
  return out as unknown as PdfFunctions;
}

/**
 * Fold the first-class wasm source options into the Emscripten module
 * options. An explicit `locateFile` / `wasmBinary` in `opts.wasm` wins —
 * the raw escape hatch stays authoritative.
 */
function buildEmscriptenOptions(opts: CreatePdfRuntimeOptions): Record<string, unknown> {
  const moduleOptions: Record<string, unknown> = { ...(opts.wasm ?? {}) };
  if (opts.wasmBinary !== undefined && moduleOptions.wasmBinary === undefined) {
    moduleOptions.wasmBinary =
      opts.wasmBinary instanceof Uint8Array ? opts.wasmBinary : new Uint8Array(opts.wasmBinary);
  }
  const wasmUrl = opts.wasmUrl;
  if (wasmUrl !== undefined && moduleOptions.locateFile === undefined) {
    moduleOptions.locateFile = (path: string, prefix: string) =>
      path.endsWith('.wasm') ? wasmUrl : prefix + path;
  }
  // Bytes in hand must mean NO location is ever needed: without a locateFile,
  // Emscripten still computes the wasm's URL via `new URL('embedpdf.wasm',
  // import.meta.url)`, which THROWS inside a blob: worker (blob URLs can't be
  // a base). The name is only used as a lookup key against wasmBinary — the
  // glue never fetches it.
  if (moduleOptions.wasmBinary !== undefined && moduleOptions.locateFile === undefined) {
    moduleOptions.locateFile = (path: string, prefix: string) =>
      path.endsWith('.wasm') ? path : prefix + path;
  }
  return moduleOptions;
}

export async function createWasmRuntime(
  opts: CreatePdfRuntimeOptions = {},
): Promise<PdfRuntimeModule> {
  const wasmPackage = await import('@embedpdf/engine-runtime-wasm32');
  const createModule = (wasmPackage.default ?? wasmPackage) as (
    opts?: Record<string, unknown>,
  ) => Promise<EmscriptenModule>;
  const module = await createModule(buildEmscriptenOptions(opts));
  const mem = createWasmMemory(module);
  const cb = createWasmCallbacks(module);

  return {
    kind: 'wasm',
    platform: 'wasm32',
    mem,
    cb,
    fileAccess: createWasmFileAccess(mem, cb),
    fileWrite: createWasmFileWrite(),
    fn: createWasmFunctions(module),
    rust: createWasmRustCore(module, mem),
    async destroy() {
      module._free = module._free;
    },
  };
}
