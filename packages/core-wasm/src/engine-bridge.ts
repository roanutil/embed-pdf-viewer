/**
 * The engine bridge: `@embedpdf/engine`'s `Engine` and `DocumentHandle`, as
 * the one-envelope-per-call contract the Rust kernel's plugins speak
 * (`crates/epdf-kernel/src/engine.rs`). `crates/epdf-web/src/engine.rs` is the
 * other half.
 *
 * This is `tooling/plugin-conformance/src/fake-engine.ts` in reverse. The
 * fake turns a TypeScript member chain into an envelope; the bridge turns an
 * envelope back into the member chain on a real handle:
 *
 *   - `path` is the chain, dot-separated: `pages.list`, `security.unlock`,
 *     `close`. With a `pon`, the chain starts at `page.` and runs on
 *     `doc.page(toPageRef(pon))`: `page.render.image`.
 *   - Arguments arrive as canonical JSON text and results leave as canonical
 *     JSON text, in the encoding of `tooling/plugin-conformance/src/canonical.ts`
 *     (`$undefined`, `$number`, `$bytes`, ...), which is what the kernel reads.
 *   - A rejection is the canonical `{"$error": {name, message, ...members}}`,
 *     so an `EngineError`'s `code` and `details` and a `PermissionDenied`'s
 *     `required` and `context` reach the kernel intact.
 *   - A value JSON cannot carry (a `PageImageHandle` with its `objectUrl()`,
 *     a `Blob`) is kept in a host table and crosses as its data members plus
 *     `$host: n`; `host(n)` returns the original. `release(n)` lets go of a
 *     rendered image once the render plugin's last Rust handle to it drops
 *     (`RenderHost::release_image`); everything else stays for the session.
 *   - A `PageImageHandle`'s `source.bytes` crosses as
 *     `{"$hostBytes": n, "$byteLength": len}`, not the encoded image. `n` is
 *     a second, separate host-table entry for the raw `Uint8Array` itself,
 *     from the one table every bridge this module builds shares (see
 *     `allHosts` below), so `n` is unique across bridges, not just within
 *     one. The module-level [`hostBytes`] resolves it and is the one
 *     function the wasm side installs, once per instance
 *     (`set_host_bytes_source`, called from `browser.ts`), not once per
 *     kernel: two live kernels (two viewers on a page, or a remount) must
 *     not race to overwrite each other's resolver. The render raster store
 *     only ever reads `$byteLength` and never calls `hostBytes`.
 *
 * Every asynchronous method answers a promise with an `abort(reason)` when
 * the engine's promise had one, which the Rust side calls from
 * `rt::Promise::abort`.
 */
import { toPageRef } from '@embedpdf/engine-core';
import type { DocumentHandle, Engine } from '@embedpdf/engine-core/runtime';

/** A bridge promise: every one resolves with canonical JSON text. */
export type BridgeCall<T = string> = Promise<T> & { abort?: (reason?: unknown) => void };

export interface EngineBridge {
  /** `engine.open(input, options)`; answers the handle's id. */
  open(inputJson: string, optionsJson?: string | null): BridgeCall;
  /** One asynchronous call on a document, or on its page `pon`. */
  call(docId: string, path: string, pon: number | null | undefined, argsJson: string): BridgeCall;
  /** Whether an optional member exists; `page.*` members are read off page 1. */
  has(docId: string, member: string): boolean;
  /** `doc.capabilities`, as canonical JSON text. */
  capabilities(docId: string): string;
  /** A synchronous `doc.security` read: a getter, or a method called with
   *  `argsJson`. Read live each time: `current` changes after an unlock. */
  security(docId: string, member: string, argsJson: string): string;
  /** `doc.page(toPageRef(pon)).pageIndex`. Throws what `page()` throws. */
  pageIndex(docId: string, pon: number): number;
  /** `doc.events.lastServerId()`, as a string. */
  lastServerId(docId: string): string | null;
  /** `doc.events.subscribe`, each event as canonical JSON text. */
  subscribe(docId: string, listener: (eventJson: string) => void): () => void;
  /** `engine.destroy()`. */
  destroy(): BridgeCall;
  /** The live value behind a `{"$host": n}` this bridge holds; throws for
   *  an id another bridge holds. */
  host(n: number): unknown;
  /** The live `Uint8Array` behind a `{"$hostBytes": n}` this bridge holds;
   *  throws if `n` is not a bytes entry this bridge holds. The module-level
   *  [`hostBytes`] is what Rust actually calls; this is the same lookup,
   *  scoped to one bridge, for this bridge's own callers. */
  hostBytes(n: number): Uint8Array;
  /** Keep a live value in the host table; answers its `n`. */
  hold(value: unknown): number;
  /** Let go of host value `n`, and of its `$hostBytes` entry when it is a
   *  rendered image: marked now, deleted on the next macrotask (see the
   *  release site for why). Throws for an id this bridge does not hold, and
   *  for one already released. */
  release(n: number): void;
  /** Entries not yet deleted: this bridge's in the host table (`hosts`),
   *  every bridge's (`allHosts`), and its documents, for the eval build's
   *  memory soak test. */
  counts(): { hosts: number; allHosts: number; documents: number };
}

// ── canonical JSON ────────────────────────────────────────────────────────────
//
// The rules of `tooling/plugin-conformance/src/canonical.ts`, less the
// harness hooks, with `$host` for what JSON cannot carry. Copied rather than
// imported: that module encodes bytes with Node's `Buffer`, and this one runs
// in the browser.

function base64(bytes: Uint8Array): string {
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(text);
}

function unbase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function isPlain(object: object): boolean {
  const proto = Object.getPrototypeOf(object);
  return proto === Object.prototype || proto === null;
}

/** `PageImageSource`'s `{ kind: 'bytes', bytes: Uint8Array }` case: the
 *  shape whose bytes stay on the JS side (see `canonical`'s `live` branch). */
function isByteImageSource(value: unknown): value is { kind: 'bytes'; bytes: Uint8Array } {
  if (value === null || typeof value !== 'object') return false;
  const source = value as { kind?: unknown; bytes?: unknown };
  return source.kind === 'bytes' && source.bytes instanceof Uint8Array;
}

/** `canonicalError`: `name`, `message`, then every own enumerable member. */
export function errorSpec(error: unknown, encode: (value: unknown) => unknown): unknown {
  const e = error as Record<string, unknown> | null;
  const out: Record<string, unknown> = {
    name: typeof e?.name === 'string' ? e.name : 'Error',
    message: typeof e?.message === 'string' ? e.message : String(error),
  };
  if (e && typeof e === 'object') {
    for (const key of Object.keys(e)) {
      if (key === 'stack' || key === 'name' || key === 'message' || e[key] === undefined) continue;
      out[key] = encode(e[key]);
    }
  }
  return { $error: out };
}

/** How [`canonical`] encodes what JSON cannot carry. */
export interface EncodeHooks {
  /** Register a live value; answers its host id. */
  hold(value: unknown): number;
  /** A rendered image held as `host`, whose raw bytes were held as `bytes`. */
  image?(host: number, bytes: number): void;
  /** Encode a function, member functions included. Without it a function is
   *  held, and a member function is dropped and marks its object live. */
  fn?(value: (...args: unknown[]) => unknown): unknown;
  /** Take an `AbortSignal` out of the value: it encodes as `undefined`. */
  signal?(value: AbortSignal): void;
}

/** Canonical form of `value`; `hooks.hold` registers what JSON cannot carry
 *  and answers its host id. */
export function canonical(
  value: unknown,
  hooks: EncodeHooks | ((value: unknown) => number),
  seen = new Set<object>(),
): unknown {
  const h = typeof hooks === 'function' ? { hold: hooks } : hooks;
  if (h.signal && typeof AbortSignal !== 'undefined' && value instanceof AbortSignal) {
    h.signal(value);
    return { $undefined: true };
  }
  if (value === undefined) return { $undefined: true };
  if (value === null) return null;
  switch (typeof value) {
    case 'boolean':
    case 'string':
      return value;
    case 'number':
      if (Number.isNaN(value)) return { $number: 'NaN' };
      if (value === Infinity) return { $number: 'Infinity' };
      if (value === -Infinity) return { $number: '-Infinity' };
      return Object.is(value, -0) ? 0 : value;
    case 'bigint':
      return { $bigint: value.toString() };
    case 'symbol':
      return { $symbol: value.description ?? '' };
    case 'function':
      return h.fn ? h.fn(value as (...args: unknown[]) => unknown) : { $host: h.hold(value) };
  }
  const object = value as object;
  const recur = (v: unknown) => canonical(v, h, seen);
  if (object instanceof Error) return errorSpec(object, recur);
  if (object instanceof Uint8Array) return { $bytes: base64(object) };
  if (ArrayBuffer.isView(object)) {
    return { $bytes: base64(new Uint8Array(object.buffer, object.byteOffset, object.byteLength)) };
  }
  if (object instanceof ArrayBuffer) return { $bytes: base64(new Uint8Array(object)) };
  if (object instanceof Date) return { $date: object.getTime() };
  if (seen.has(object)) throw new TypeError('[engine-bridge] cyclic value');
  seen.add(object);
  try {
    if (Array.isArray(object)) return object.map(recur);
    if (object instanceof Map)
      return { $map: [...object.entries()].map(([k, v]) => [recur(k), recur(v)]) };
    if (object instanceof Set) return { $set: [...object].map(recur) };
    const members = Object.entries(object);
    const dropsFns = !h.fn;
    const live =
      !isPlain(object) || (dropsFns && members.some(([, member]) => typeof member === 'function'));
    const out: Record<string, unknown> = {};
    let bytesId: number | undefined;
    for (const [key, member] of members) {
      if (member === undefined || (dropsFns && typeof member === 'function')) continue;
      if (h.signal && typeof AbortSignal !== 'undefined' && member instanceof AbortSignal) {
        h.signal(member);
        continue;
      }
      // A rendered image crosses into Rust as a byte count plus a resolvable
      // id, not the bytes themselves: the host table (below) already keeps
      // the live object, bytes and all, for `objectUrl()` and a later
      // `host(n)`; `h.hold(member.bytes)` registers a second entry for the
      // raw `Uint8Array`, which only a caller that actually needs pixels
      // resolves, through `hostBytes(n)`.
      if (live && key === 'source' && isByteImageSource(member)) {
        bytesId = h.hold(member.bytes);
        out[key] = {
          kind: 'bytes',
          bytes: { $hostBytes: bytesId, $byteLength: member.bytes.byteLength },
        };
        continue;
      }
      out[key] = recur(member);
    }
    if (live) {
      out.$host = h.hold(object);
      if (bytesId !== undefined) h.image?.(out.$host as number, bytesId);
    }
    return out;
  } finally {
    seen.delete(object);
  }
}

/** A decoder's own markers: `{ value }` when `object` is one, else
 *  `undefined` to fall through to the default rules. `recur` decodes a
 *  nested value with the same hook. */
export type DecodeHook = (
  object: Record<string, unknown>,
  recur: (value: unknown) => unknown,
) => { value: unknown } | undefined;

/** Canonical JSON back to live values: `$bytes` to a `Uint8Array`, `$host`
 *  to the held value, `$undefined` and `$number` to their numbers. `$date`,
 *  `$map`, `$set` and `$bigint` are not reversed: nothing the kernel sends
 *  in produces them. Neither is `$hostBytes`: it only ever flows JS to
 *  Rust (`canonical`'s encoder), never the other way, so `decode` never
 *  sees one. `hook` sees every object first. */
export function decode(value: unknown, host: (n: number) => unknown, hook?: DecodeHook): unknown {
  const recur = (item: unknown) => decode(item, host, hook);
  if (Array.isArray(value)) return value.map(recur);
  if (value === null || typeof value !== 'object') return value;
  const object = value as Record<string, unknown>;
  const special = hook?.(object, recur);
  if (special) return special.value;
  if (object.$undefined === true) return undefined;
  if (typeof object.$bytes === 'string') return unbase64(object.$bytes);
  if (typeof object.$number === 'string') return Number(object.$number);
  if (typeof object.$host === 'number') return host(object.$host);
  const out: Record<string, unknown> = {};
  for (const [key, member] of Object.entries(object)) {
    const live = recur(member);
    if (live !== undefined) out[key] = live;
  }
  return out;
}

/**
 * A host value as canonical JSON text for the kernel, with every `Blob`
 * (and so every `File`) read to `{"$bytes": ...}`: what `documents.open`
 * hands in from a file picker. Anything else JSON cannot carry is refused.
 */
export async function encodeInput(value: unknown): Promise<string> {
  const refuse = (): number => {
    throw new TypeError('[engine-bridge] input is not JSON-serializable');
  };
  return JSON.stringify(canonical(await readBlobs(value), refuse));
}

/** `value` with every `Blob` in it, however deep in plain objects and
 *  arrays, read to a `Uint8Array`. */
export async function readBlobs(value: unknown): Promise<unknown> {
  const read = async (v: unknown): Promise<unknown> => {
    if (typeof Blob !== 'undefined' && v instanceof Blob)
      return new Uint8Array(await v.arrayBuffer());
    if (Array.isArray(v)) return Promise.all(v.map(read));
    if (v !== null && typeof v === 'object' && isPlain(v)) {
      const out: Record<string, unknown> = {};
      for (const [key, member] of Object.entries(v)) out[key] = await read(member);
      return out;
    }
    return v;
  };
  return read(value);
}

// ── the bridge ────────────────────────────────────────────────────────────────

/** The live value behind every `$host`/`$hostBytes` id, for every bridge
 *  this module builds: one table, one counter (`nextHostId`), so an id is
 *  unique across bridges, not just within the bridge that made it. A
 *  bridge still tracks which ids are its own (`createEngineBridge`'s
 *  `ownHosts`), for `counts()` and for a later release/destroy hook to
 *  clear only its entries; this table is what a lookup actually reads. */
const allHosts = new Map<number, unknown>();
let nextHostId = 1;

/** Ids `release` marked, not yet deleted, each with the owning bridge's
 *  `ownHosts` so the deletion clears that too. */
const pendingRelease = new Map<number, Set<number>>();
let releaseScheduled = false;

/** Delete every pending release in one go: `release`'s macrotask. */
function flushReleases(): void {
  releaseScheduled = false;
  for (const [n, owner] of pendingRelease) {
    allHosts.delete(n);
    owner.delete(n);
  }
  pendingRelease.clear();
}

/** Whether `n` was handed out and has since been deleted. Ids are never
 *  reused and nothing but a release deletes, so an id below the counter the
 *  table no longer has is a released one: no tombstones to keep. */
function wasReleased(n: number): boolean {
  return Number.isInteger(n) && n > 0 && n < nextHostId && !allHosts.has(n);
}

/** A released id that came back: a bug, reported loudly (`console.error`
 *  plus the throw), never a silent `undefined`. */
function releasedError(message: string): Error {
  const error = new Error(message);
  console.error(error);
  return error;
}

/** The live `Uint8Array` behind a `{"$hostBytes": n}`, from whichever
 *  bridge holds it. This is the one function `set_host_bytes_source`
 *  installs into the wasm module: it does not matter which `WasmKernel`
 *  calls it, or how many are live, because every bridge's ids come from
 *  the one shared table above. Throws `[engine-bridge] no host bytes n`
 *  for an id nothing holds, or that is not bytes. */
export function hostBytes(n: number): Uint8Array {
  const value = allHosts.get(n);
  if (!(value instanceof Uint8Array)) {
    if (wasReleased(n)) throw releasedError(`[engine-bridge] host bytes ${n} was released`);
    throw new Error(`[engine-bridge] no host bytes ${n}`);
  }
  return value;
}

export function createEngineBridge(engine: Engine): EngineBridge {
  const documents = new Map<string, DocumentHandle>();
  /** This bridge's own ids into `allHosts`, for `counts()` and a later
   *  release/destroy hook. Lookups below still read `allHosts` directly:
   *  membership here only answers "does this bridge own `n`", which a
   *  foreign id fails, the same as a genuinely unknown one. */
  const ownHosts = new Set<number>();
  /** A rendered image's host id to its `$hostBytes` id, so releasing the
   *  image releases its bytes too. */
  const imageBytes = new Map<number, number>();

  const hold = (value: unknown): number => {
    const n = nextHostId++;
    allHosts.set(n, value);
    ownHosts.add(n);
    return n;
  };
  const host = (n: number): unknown => {
    if (!ownHosts.has(n)) {
      if (wasReleased(n)) throw releasedError(`[engine-bridge] host value ${n} was released`);
      throw new Error(`[engine-bridge] no host value ${n}`);
    }
    return allHosts.get(n);
  };
  const release = (n: number): void => {
    if (pendingRelease.has(n) || wasReleased(n)) {
      throw releasedError(`[engine-bridge] host value ${n} was already released`);
    }
    if (!ownHosts.has(n)) throw new Error(`[engine-bridge] no host value ${n}`);
    pendingRelease.set(n, ownHosts);
    const bytes = imageBytes.get(n);
    if (bytes !== undefined) {
      imageBytes.delete(n);
      pendingRelease.set(bytes, ownHosts);
    }
    // Deleted on the next macrotask, not now. The real invariant is not
    // that the decode is queued before this call: `render_pages` and an
    // already-evicted handle drop their last Rust clone (this call) just
    // before `resolver.settle` resolves the promise that carried the same
    // image (`runtime::to_js`), so there the decode is queued after. What
    // always holds is that the resolve happens in the same synchronous
    // stretch as the drop, or in an earlier one whose decode already ran.
    // Either way, the shim's `.then` decode reaction (a microtask) is
    // queued by the time this stretch ends, and queued microtasks all run
    // before the next macrotask; until then `host(n)` still answers.
    if (!releaseScheduled) {
      releaseScheduled = true;
      setTimeout(flushReleases, 0);
    }
  };
  const bridgeHostBytes = (n: number): Uint8Array => {
    const value = host(n);
    if (!(value instanceof Uint8Array)) {
      throw new Error(`[engine-bridge] host value ${n} is not bytes`);
    }
    return value;
  };
  const hooks: EncodeHooks = { hold, image: (n, bytes) => imageBytes.set(n, bytes) };
  const encode = (value: unknown) => JSON.stringify(canonical(value, hooks));
  const decodeArgs = (json: string) => decode(JSON.parse(json), host) as unknown[];
  const reason = (error: unknown) => errorSpec(error, (v) => canonical(v, hold));

  const document = (docId: string): DocumentHandle => {
    const doc = documents.get(docId);
    if (!doc) throw new Error(`[engine-bridge] no document "${docId}"`);
    return doc;
  };

  /** Settle through `map`, keeping the engine promise's `abort`. */
  function bridged<T>(run: () => unknown, map: (value: unknown) => T): BridgeCall<T> {
    let pending: unknown;
    try {
      pending = run();
    } catch (error) {
      return Promise.reject(reason(error));
    }
    const fail = (error: unknown): never => {
      throw reason(error);
    };
    const out: BridgeCall<T> = Promise.resolve(pending).then((value) => {
      try {
        return map(value);
      } catch (error) {
        return fail(error);
      }
    }, fail);
    const abort = (pending as { abort?: unknown } | null)?.abort;
    if (typeof abort === 'function') {
      out.abort = (why?: unknown) => (abort as (why?: unknown) => void).call(pending, why);
    }
    return out;
  }

  /** The object a member chain starts from, and the chain's segments. */
  function resolve(doc: DocumentHandle, path: string, pon: number | null | undefined) {
    const segments = path.split('.');
    if (pon === null || pon === undefined) return { root: doc as unknown, segments };
    if (segments[0] !== 'page') throw new TypeError(`[engine-bridge] "${path}" is not a page path`);
    return { root: doc.page(toPageRef(pon)) as unknown, segments: segments.slice(1) };
  }

  return {
    open(inputJson, optionsJson) {
      const input = decode(JSON.parse(inputJson), host);
      const args = optionsJson == null ? [input] : [input, decode(JSON.parse(optionsJson), host)];
      return bridged(
        () => (engine.open as (...a: unknown[]) => unknown)(...args),
        (value) => {
          const doc = value as DocumentHandle;
          documents.set(doc.id, doc);
          return encode(doc.id);
        },
      );
    },

    call(docId, path, pon, argsJson) {
      return bridged(
        () => {
          const { root, segments } = resolve(document(docId), path, pon);
          let target = root as Record<string, unknown>;
          for (const segment of segments.slice(0, -1))
            target = target[segment] as Record<string, unknown>;
          const method = segments[segments.length - 1];
          const fn = target?.[method];
          if (typeof fn !== 'function')
            throw new TypeError(`[engine-bridge] ${path} is not a function`);
          return (fn as (...a: unknown[]) => unknown).apply(target, decodeArgs(argsJson));
        },
        (value) => encode(value),
      );
    },

    has(docId, member) {
      const doc = documents.get(docId);
      if (!doc) return false;
      const segments = member.split('.');
      let target: unknown = doc;
      if (segments[0] === 'page') {
        try {
          target = doc.page(toPageRef(1));
        } catch {
          return false;
        }
        segments.shift();
      }
      try {
        for (const segment of segments) {
          if (target === null || target === undefined) return false;
          target = (target as Record<string, unknown>)[segment];
        }
        return target !== undefined;
      } catch {
        return false;
      }
    },

    capabilities(docId) {
      return encode(document(docId).capabilities);
    },

    security(docId, member, argsJson) {
      const security = document(docId).security as unknown as Record<string, unknown>;
      const read = security[member];
      if (typeof read !== 'function') return encode(read);
      return encode((read as (...a: unknown[]) => unknown).apply(security, decodeArgs(argsJson)));
    },

    pageIndex(docId, pon) {
      return document(docId).page(toPageRef(pon)).pageIndex;
    },

    lastServerId(docId) {
      const id = document(docId).events.lastServerId();
      return id === null || id === undefined ? null : String(id);
    },

    subscribe(docId, listener) {
      return document(docId).events.subscribe((event) => listener(encode(event)));
    },

    destroy() {
      return bridged(
        () => engine.destroy(),
        (value) => {
          documents.clear();
          return encode(value);
        },
      );
    },

    host,
    hostBytes: bridgeHostBytes,
    hold,
    release,
    counts() {
      return { hosts: ownHosts.size, allHosts: allHosts.size, documents: documents.size };
    },
  };
}
