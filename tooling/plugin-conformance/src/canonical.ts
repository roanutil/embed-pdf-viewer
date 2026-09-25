/**
 * Canonical JSON: the one format a TypeScript trace and a Rust trace are both
 * written in, so the Rust runner can compare them value for value.
 *
 * The encoding is defined here and restated in `README.md`, which is what the
 * Rust side (`crates/plugin-conformance/src/canonical.rs`) implements. Every
 * rule exists because JSON alone would make two different behaviours print
 * the same:
 *
 *   undefined          → omitted as an object member, `{"$undefined":true}` elsewhere
 *   NaN, ±Infinity     → `{"$number":"NaN"|"Infinity"|"-Infinity"}`
 *   Uint8Array etc.    → `{"$bytes":"<base64>"}`
 *   Map                → `{"$map":[[key, value], ...]}` in insertion order
 *   Set                → `{"$set":[value, ...]}` in insertion order
 *   Error              → `{"$error":{"name","message", ...own enumerable members}}`
 *   AbortSignal        → `{"$signal":{"aborted":bool}}`
 *   function           → `{"$ref":n}`, registered with the harness so a later step can call it
 *   Promise            → `{"$promise":n}`, tracked so its settlement lands in the trace
 *
 * Numbers are compared as f64, so `1` and `1.0` are the same value. Object
 * member order is not compared; array order is.
 */

export interface CanonicalHooks {
  /** Register a function the plugin handed back; returns its ref id, or null
   *  where functions are only noted (`{"$function":true}`), as in state. */
  fn(value: (...args: unknown[]) => unknown, self?: unknown): number | null;
  /** Register a promise the plugin handed back; returns its promise id. */
  promise(value: PromiseLike<unknown>): number;
}

const noHooks: CanonicalHooks = {
  fn: () => {
    throw new Error('[canonical] a function appeared where none can be registered');
  },
  promise: () => {
    throw new Error('[canonical] a promise appeared where none can be registered');
  },
};

/** Hooks for a snapshot: functions are noted, never registered. */
export const snapshotHooks: CanonicalHooks = {
  fn: () => null,
  promise: () => {
    throw new Error('[canonical] a promise appeared in a state snapshot');
  },
};

const fnRef = (id: number | null) => (id === null ? { $function: true } : { $ref: id });

export function isThenable(value: unknown): value is PromiseLike<unknown> {
  return (
    value !== null &&
    (typeof value === 'object' || typeof value === 'function') &&
    typeof (value as { then?: unknown }).then === 'function'
  );
}

function base64(bytes: Uint8Array): string {
  return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength).toString('base64');
}

export function canonicalError(error: unknown, hooks: CanonicalHooks = noHooks): unknown {
  const e = error as Record<string, unknown> | null;
  const out: Record<string, unknown> = {
    name: typeof e?.name === 'string' ? e.name : 'Error',
    message: typeof e?.message === 'string' ? e.message : String(error),
  };
  // Every own enumerable member beyond those two: EngineError's `code` and
  // `details`, PermissionDenied's `required` and `context`, AbortError's
  // `reason`. A refusal's shape is behaviour a caller branches on.
  if (e && typeof e === 'object') {
    for (const key of Object.keys(e)) {
      if (key === 'stack' || key === 'name' || key === 'message' || e[key] === undefined) continue;
      out[key] = canonical(e[key], hooks);
    }
  }
  return { $error: out };
}

export function canonical(value: unknown, hooks: CanonicalHooks = noHooks, seen = new Set<object>()): unknown {
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
      return fnRef(hooks.fn(value as (...args: unknown[]) => unknown));
  }
  const object = value as object;
  if (isThenable(object)) return { $promise: hooks.promise(object) };
  if (object instanceof Error) return canonicalError(object, hooks);
  if (object instanceof Uint8Array) return { $bytes: base64(object) };
  if (ArrayBuffer.isView(object)) {
    return { $bytes: base64(new Uint8Array(object.buffer, object.byteOffset, object.byteLength)) };
  }
  if (object instanceof ArrayBuffer) return { $bytes: base64(new Uint8Array(object)) };
  if (typeof AbortSignal !== 'undefined' && object instanceof AbortSignal) {
    return { $signal: { aborted: object.aborted } };
  }
  if (object instanceof Date) return { $date: object.getTime() };
  if (seen.has(object)) throw new Error('[canonical] cyclic value');
  seen.add(object);
  try {
    if (Array.isArray(object)) {
      return object.map((item) => canonical(item, hooks, seen));
    }
    if (object instanceof Map) {
      return {
        $map: [...object.entries()].map(([k, v]) => [canonical(k, hooks, seen), canonical(v, hooks, seen)]),
      };
    }
    if (object instanceof Set) {
      return { $set: [...object].map((item) => canonical(item, hooks, seen)) };
    }
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(object)) {
      const member = (object as Record<string, unknown>)[key];
      if (member === undefined) continue;
      if (typeof member === 'function') {
        out[key] = fnRef(hooks.fn(member as (...args: unknown[]) => unknown, object));
        continue;
      }
      out[key] = canonical(member, hooks, seen);
    }
    return out;
  } finally {
    seen.delete(object);
  }
}

/** Stable text of a canonical value, for matching arguments against a table. */
export function canonicalKey(value: unknown): string {
  return JSON.stringify(sortKeys(value));
}

function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) out[key] = sortKeys((value as Record<string, unknown>)[key]);
    return out;
  }
  return value;
}
