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
 *     `$host: n`; `host(n)` returns the original. The table only grows.
 *   - A `PageImageHandle`'s `source.bytes` crosses as `{"$byteLength": n}`,
 *     not the encoded image: the host table above already keeps the real
 *     bytes, and Rust only needs the count to cost the entry.
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
  /** The live value behind a `{"$host": n}`. */
  host(n: number): unknown;
  /** Keep a live value in the host table; answers its `n`. */
  hold(value: unknown): number;
  /** Live entries in the host and document tables, for the eval build's
   *  memory soak test. */
  counts(): { hosts: number; documents: number };
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
    for (const [key, member] of members) {
      if (member === undefined || (dropsFns && typeof member === 'function')) continue;
      if (h.signal && typeof AbortSignal !== 'undefined' && member instanceof AbortSignal) {
        h.signal(member);
        continue;
      }
      // A rendered image crosses into Rust as a byte count, not the bytes:
      // the host table (below) already keeps the live object, bytes and
      // all, for `objectUrl()` and a later `host(n)`.
      if (live && key === 'source' && isByteImageSource(member)) {
        out[key] = { kind: 'bytes', bytes: { $byteLength: member.bytes.byteLength } };
        continue;
      }
      out[key] = recur(member);
    }
    if (live) out.$host = h.hold(object);
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
 *  in produces them. `hook` sees every object first. */
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

export function createEngineBridge(engine: Engine): EngineBridge {
  const documents = new Map<string, DocumentHandle>();
  const hosts = new Map<number, unknown>();
  let nextHost = 1;

  const hold = (value: unknown): number => {
    const n = nextHost++;
    hosts.set(n, value);
    return n;
  };
  const host = (n: number): unknown => {
    if (!hosts.has(n)) throw new Error(`[engine-bridge] no host value ${n}`);
    return hosts.get(n);
  };
  const encode = (value: unknown) => JSON.stringify(canonical(value, hold));
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
    hold,
    counts() {
      return { hosts: hosts.size, documents: documents.size };
    },
  };
}
